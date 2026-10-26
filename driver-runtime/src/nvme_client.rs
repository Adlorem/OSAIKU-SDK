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
    fn quarantine(&mut self);
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
                if matches!(operation, wire::WRITE | wire::FLUSH | wire::SHUTDOWN) {
                    Error::UnknownCommit
                } else {
                    error
                },
            );
        }
        Ok(())
    }
    pub fn read(&mut self, lba: u64, bytes: &mut [u8]) -> Result<(), Error> {
        self.check(lba, bytes.len())?;
        for (i, chunk) in bytes.chunks_mut(self.geometry.limit).enumerate() {
            self.send(
                wire::READ,
                lba + (i * self.geometry.limit / 512) as u64,
                &[],
                chunk,
            )?;
        }
        Ok(())
    }
    pub fn write(&mut self, lba: u64, bytes: &[u8], durable: bool) -> Result<(), Error> {
        self.check(lba, bytes.len())?;
        for (i, chunk) in bytes.chunks(self.geometry.limit).enumerate() {
            self.send(
                wire::WRITE,
                lba + (i * self.geometry.limit / 512) as u64,
                chunk,
                &mut [],
            )?;
        }
        if durable {
            self.flush()?;
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
