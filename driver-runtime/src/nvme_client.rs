//! Storage-owner adapter to an already admitted NVMe process. This adapter
//! conveys no partition or write grant: those remain with its trusted caller.
use crate::{block::Error, nvme::MAX_IO_BYTES, nvme_process as wire};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub generation: u64,
    pub namespace: u32,
    pub blocks: u64,
    pub limit: usize,
    pub cap: u64,
    pub version: u32,
}
pub trait Transport {
    fn live(&self, generation: u64) -> bool;
    /// Synchronous receipt; borrowed slices must no longer be used on return.
    fn exchange(
        &mut self,
        generation: u64,
        operation: u64,
        lba: u64,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), Error>;
    /// Several already validated page runs as one GATHER receipt. Transports
    /// without a gathered request may write the pages one exchange at a time.
    fn gather(&mut self, generation: u64, runs: &[(u64, &[&[u8; 4096]])]) -> Result<(), Error> {
        for (lba, pages) in runs {
            for (i, page) in pages.iter().enumerate() {
                self.exchange(
                    generation,
                    wire::WRITE,
                    lba + i as u64 * 8,
                    &page[..],
                    &mut [],
                )?;
            }
        }
        Ok(())
    }
    fn quarantine(&mut self);
    fn read_runs(
        &mut self,
        generation: u64,
        runs: &[(u64, usize)],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let mut at = 0;
        for &(lba, pages) in runs {
            let n = pages * 4096;
            self.exchange(generation, wire::READ, lba, &[], &mut out[at..at + n])?;
            at += n;
        }
        Ok(())
    }
}
pub struct Client<T: Transport> {
    transport: T,
    geometry: Geometry,
    closed: bool,
}
impl<T: Transport> Client<T> {
    pub fn attach(transport: T, geometry: Geometry) -> Result<Self, Error> {
        if geometry.generation == 0
            || geometry.namespace == 0
            || geometry.blocks == 0
            || !(4096..=MAX_IO_BYTES).contains(&geometry.limit)
            || !geometry.limit.is_power_of_two()
        {
            return Err(Error::InvalidRange);
        }
        crate::nvme::Capabilities::parse(geometry.cap, geometry.version)?;
        if !transport.live(geometry.generation) {
            return Err(Error::Stale);
        }
        Ok(Self {
            transport,
            geometry,
            closed: false,
        })
    }
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
    pub fn live(&self) -> bool {
        !self.closed && self.transport.live(self.geometry.generation)
    }
    fn check(&mut self, lba: u64, bytes: usize) -> Result<(), Error> {
        if !self.live() {
            self.quarantine();
            return Err(Error::Stale);
        }
        if bytes == 0
            || bytes > MAX_IO_BYTES
            || bytes % 512 != 0
            || lba
                .checked_add((bytes / 512) as u64)
                .is_none_or(|end| end > self.geometry.blocks)
        {
            return Err(Error::InvalidRange);
        }
        Ok(())
    }
    fn send(
        &mut self,
        operation: u64,
        lba: u64,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), Error> {
        if !self.live() {
            self.quarantine();
            return Err(Error::Stale);
        }
        if let Err(error) =
            self.transport
                .exchange(self.geometry.generation, operation, lba, input, output)
        {
            self.quarantine();
            return Err(
                if matches!(
                    operation,
                    wire::WRITE | wire::WRITE_FUA | wire::FLUSH | wire::SHUTDOWN
                ) {
                    Error::UnknownCommit
                } else {
                    error
                },
            );
        }
        if operation != wire::SHUTDOWN && !self.live() {
            self.quarantine();
            return Err(
                if matches!(
                    operation,
                    wire::WRITE | wire::WRITE_FUA | wire::FLUSH | wire::SHUTDOWN
                ) {
                    Error::UnknownCommit
                } else {
                    Error::Stale
                },
            );
        }
        Ok(())
    }
    // Each request fills at most one fixed arena buffer. The driver splits it
    // into commands no larger than the controller limit and keeps them in
    // flight together, so a smaller MDTS no longer costs extra round trips.
    pub fn read(&mut self, lba: u64, bytes: &mut [u8]) -> Result<(), Error> {
        self.check(lba, bytes.len())?;
        for (i, chunk) in bytes.chunks_mut(MAX_IO_BYTES).enumerate() {
            self.send(
                wire::READ,
                lba + (i * MAX_IO_BYTES / 512) as u64,
                &[],
                chunk,
            )?;
        }
        Ok(())
    }
    pub fn write(&mut self, lba: u64, bytes: &[u8], durable: bool) -> Result<(), Error> {
        self.check(lba, bytes.len())?;
        for (i, chunk) in bytes.chunks(MAX_IO_BYTES).enumerate() {
            self.send(
                if durable {
                    wire::WRITE_FUA
                } else {
                    wire::WRITE
                },
                lba + (i * MAX_IO_BYTES / 512) as u64,
                chunk,
                &mut [],
            )
            .map_err(|error| {
                // Earlier chunks already reached the device. Losing the
                // owner before the next submission is still a partial write,
                // never a definite refusal of the whole caller request.
                if i == 0 {
                    error
                } else {
                    Error::UnknownCommit
                }
            })?;
        }
        Ok(())
    }
    /// Linux blk-mq style request grouping, preserving the caller's scatter
    /// order in a bounded arena. Validate the entire list before dispatch.
    pub fn read_runs(&mut self, runs: &[(u64, usize)], out: &mut [u8]) -> Result<(), Error> {
        if runs.is_empty() || runs.len() > wire::MAX_SEGMENTS {
            return Err(Error::InvalidRange);
        }
        let mut total = 0usize;
        for &(lba, pages) in runs {
            let bytes = pages.checked_mul(4096).ok_or(Error::InvalidRange)?;
            self.check(lba, bytes)?;
            total = total.checked_add(bytes).ok_or(Error::InvalidRange)?;
        }
        if total > MAX_IO_BYTES || total != out.len() {
            return Err(Error::InvalidRange);
        }
        if let Err(e) = self
            .transport
            .read_runs(self.geometry.generation, runs, out)
        {
            self.quarantine();
            return Err(e);
        }
        Ok(())
    }
    /// Write up to `wire::MAX_SEGMENTS` runs of whole pages, together no
    /// larger than one arena buffer, as a single request. Every run is
    /// checked before anything is sent; afterwards any failure is uncertain.
    pub fn write_runs(&mut self, runs: &[(u64, &[&[u8; 4096]])]) -> Result<(), Error> {
        if runs.is_empty() || runs.len() > wire::MAX_SEGMENTS {
            return Err(Error::InvalidRange);
        }
        let mut pages = 0;
        for (lba, run) in runs {
            pages += run.len();
            if run.is_empty()
                || (run.len() as u64)
                    .checked_mul(8)
                    .and_then(|n| lba.checked_add(n))
                    .is_none_or(|end| end > self.geometry.blocks)
            {
                return Err(Error::InvalidRange);
            }
        }
        if pages * 4096 > MAX_IO_BYTES {
            return Err(Error::InvalidRange);
        }
        if !self.live() {
            self.quarantine();
            return Err(Error::Stale);
        }
        if self
            .transport
            .gather(self.geometry.generation, runs)
            .is_err()
        {
            self.quarantine();
            return Err(Error::UnknownCommit);
        }
        Ok(())
    }
    pub fn flush(&mut self) -> Result<(), Error> {
        self.send(wire::FLUSH, 0, &[], &mut [])
    }
    pub fn identify(&mut self, cns: u8, out: &mut [u8; 4096]) -> Result<(), Error> {
        if cns > 3 {
            return Err(Error::InvalidRange);
        }
        self.send(wire::IDENTIFY, u64::from(cns), &[], out)
    }
    pub fn shutdown(&mut self) -> Result<(), Error> {
        let result = self.send(wire::SHUTDOWN, 0, &[], &mut []);
        self.closed = true;
        result
    }
    pub fn quarantine(&mut self) {
        self.closed = true;
        self.transport.quarantine();
    }
}
