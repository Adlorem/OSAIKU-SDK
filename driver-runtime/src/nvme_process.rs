//! Generation-bound NVMe block transport. A completed IRQ is never a receipt.
pub use crate::controller::{
    ACQUIRE, CLOCK, COMPLETE, FETCH, GRANT, IO, PUBLISH, READ_ARENA, READ_REGISTER, WRITE_ARENA,
    WRITE_REGISTER,
};
use crate::protocol::Error;
pub const ENDPOINT: u64 = 7;
pub const BYTES: usize = 64;
pub const READ: u64 = 1;
pub const WRITE: u64 = 2;
pub const FLUSH: u64 = 3;
pub const SHUTDOWN: u64 = 4;
/// Read-only Identify CNS 0..3, using the namespace admitted by the owner.
pub const IDENTIFY: u64 = 5;
/// Several whole-page runs written by one request. The owner packs the runs,
/// in order, into the generation's fixed write buffer. `Request::lba` carries
/// the run count; the runs follow the 64-byte header in the same FETCH payload.
pub const GATHER: u64 = 6;
/// Scatter reads into the fixed read arena, with one receipt for all runs.
pub const READ_GATHER: u64 = 7;
/// Write with NVMe FUA. Only this request's bytes become durable; preceding
/// ordinary writes still require their own flush barrier.
pub const WRITE_FUA: u64 = 8;
/// `IO` sub-operation: copy up to `SUBMIT_ENTRIES` 64-byte entries into their
/// ring slots, publish them and write the queue's tail doorbell. It confers
/// nothing beyond the existing arena-write, fence and doorbell-write steps.
pub const SUBMIT: u64 = 32;
/// `IO` sub-operation: copy host-owned completion entries in ring order and
/// acknowledge them with one head doorbell write. The reply carries the
/// owner's microsecond clock and, when nothing completed, CSTS.
pub const REAP: u64 = 33;
pub const SUBMIT_ENTRIES: usize = crate::process::PAYLOAD / 64;
pub const REAP_ENTRIES: usize = crate::process::PAYLOAD / 16;
/// Ring entries in one 4 KiB submission page; queues never exceed this.
pub const RING_SLOTS: u16 = 64;
pub const MAX_SEGMENTS: usize = (crate::process::PAYLOAD - BYTES) / 8;
/// One run of whole 4 KiB pages in a GATHER request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub lba: u64,
    pub pages: u16,
}
impl Segment {
    const LBA_BITS: u32 = 48;
    pub fn encode(self) -> Result<[u8; 8], Error> {
        if self.pages == 0
            || usize::from(self.pages) > crate::nvme::READ_PAGES
            || self.lba >> Self::LBA_BITS != 0
        {
            return Err(Error::Range);
        }
        Ok((self.lba | (u64::from(self.pages - 1) << Self::LBA_BITS)).to_le_bytes())
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        let v = u64::from_le_bytes(b.try_into().map_err(|_| Error::Size)?);
        let pages = (v >> Self::LBA_BITS) + 1;
        if pages as usize > crate::nvme::READ_PAGES {
            return Err(Error::Range);
        }
        Ok(Self {
            lba: v & ((1 << Self::LBA_BITS) - 1),
            pages: pages as u16,
        })
    }
}
/// Validated GATHER runs: 1..=MAX_SEGMENTS non-empty runs whose pages fit
/// the fixed write buffer together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segments {
    items: [Segment; MAX_SEGMENTS],
    count: usize,
}
impl Segments {
    pub const EMPTY: Self = Self {
        items: [Segment { lba: 0, pages: 0 }; MAX_SEGMENTS],
        count: 0,
    };
    pub fn new(runs: &[Segment]) -> Result<Self, Error> {
        if runs.is_empty() || runs.len() > MAX_SEGMENTS {
            return Err(Error::Range);
        }
        let mut s = Self::EMPTY;
        let mut pages = 0;
        for (slot, run) in s.items.iter_mut().zip(runs) {
            run.encode()?;
            pages += usize::from(run.pages);
            *slot = *run;
        }
        if pages > crate::nvme::READ_PAGES {
            return Err(Error::Range);
        }
        s.count = runs.len();
        Ok(s)
    }
    pub fn as_slice(&self) -> &[Segment] {
        &self.items[..self.count]
    }
    pub fn bytes(&self) -> usize {
        self.as_slice()
            .iter()
            .map(|s| usize::from(s.pages) * 4096)
            .sum()
    }
}
/// Decode one FETCH payload: the request header and, for GATHER, its runs.
pub fn decode_fetch(b: &[u8]) -> Result<(Request, Segments), Error> {
    if b.len() < BYTES {
        return Err(Error::Size);
    }
    let r = Request::decode(&b[..BYTES])?;
    if !matches!(r.operation, GATHER | READ_GATHER) {
        if b.len() != BYTES {
            return Err(Error::Size);
        }
        return Ok((r, Segments::EMPTY));
    }
    let count = r.lba as usize;
    if b.len() != BYTES + count * 8 {
        return Err(Error::Size);
    }
    let mut runs = [Segment::default(); MAX_SEGMENTS];
    for (i, run) in runs[..count].iter_mut().enumerate() {
        *run = Segment::decode(&b[BYTES + i * 8..BYTES + i * 8 + 8])?;
    }
    let s = Segments::new(&runs[..count])?;
    if s.bytes() != r.bytes as usize {
        return Err(Error::Range);
    }
    Ok((r, s))
}
/// `SUBMIT` value: queue, new tail and the ring slot of each copied entry.
pub fn submit_value(queue: u16, tail: u16, slots: &[u16]) -> Result<u64, Error> {
    if queue > 1
        || tail >= RING_SLOTS
        || slots.is_empty()
        || slots.len() > SUBMIT_ENTRIES
        || slots.iter().any(|s| *s >= RING_SLOTS)
    {
        return Err(Error::Range);
    }
    let mut v = u64::from(queue) | (u64::from(tail) << 8);
    for (i, s) in slots.iter().enumerate() {
        v |= u64::from(*s) << (16 + 8 * i);
    }
    Ok(v)
}
/// Inverse of `submit_value` for `entries` copied entries.
pub fn parse_submit(
    value: u64,
    entries: usize,
) -> Result<(u16, u16, [u16; SUBMIT_ENTRIES]), Error> {
    if entries == 0 || entries > SUBMIT_ENTRIES || value >> (16 + 8 * entries) != 0 {
        return Err(Error::Range);
    }
    let queue = (value & 0xff) as u16;
    let tail = ((value >> 8) & 0xff) as u16;
    let slots = core::array::from_fn(|i| ((value >> (16 + 8 * i)) & 0xff) as u16);
    if queue > 1 || tail >= RING_SLOTS || slots[..entries].iter().any(|s| *s >= RING_SLOTS) {
        return Err(Error::Range);
    }
    Ok((queue, tail, slots))
}
/// `REAP` value: queue, expected head and phase, ring size and entry limit.
pub fn reap_value(
    queue: u16,
    head: u16,
    phase: bool,
    entries: u16,
    max: usize,
) -> Result<u64, Error> {
    if queue > 1
        || !(2..=RING_SLOTS).contains(&entries)
        || head >= entries
        || max == 0
        || max > REAP_ENTRIES
    {
        return Err(Error::Range);
    }
    Ok(u64::from(queue)
        | (u64::from(head) << 8)
        | (u64::from(phase) << 16)
        | (u64::from(entries) << 24)
        | ((max as u64) << 32))
}
/// Inverse of `reap_value`: (queue, head, phase, ring entries, limit).
pub fn parse_reap(value: u64) -> Result<(u16, u16, bool, u16, usize), Error> {
    if value >> 40 != 0 || (value >> 17) & 0x7f != 0 {
        return Err(Error::Range);
    }
    let queue = (value & 0xff) as u16;
    let head = ((value >> 8) & 0xff) as u16;
    let entries = ((value >> 24) & 0xff) as u16;
    let max = ((value >> 32) & 0xff) as usize;
    reap_value(queue, head, (value >> 16) & 1 == 1, entries, max)?;
    Ok((queue, head, (value >> 16) & 1 == 1, entries, max))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub generation: u64,
    pub sequence: u64,
    pub namespace: u32,
    pub operation: u64,
    pub lba: u64,
    pub bytes: u32,
    pub buffer: u64,
}
impl Request {
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        for (i, n) in [
            self.generation,
            self.sequence,
            u64::from(self.namespace),
            self.operation,
            self.lba,
            u64::from(self.bytes),
            self.buffer,
        ]
        .into_iter()
        .enumerate()
        {
            b[i * 8..i * 8 + 8].copy_from_slice(&n.to_le_bytes())
        }
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != BYTES {
            return Err(Error::Size);
        }
        let n = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        if n(7) != 0 {
            return Err(Error::Reserved);
        }
        let r = Self {
            generation: n(0),
            sequence: n(1),
            namespace: u32::try_from(n(2)).map_err(|_| Error::Range)?,
            operation: n(3),
            lba: n(4),
            bytes: u32::try_from(n(5)).map_err(|_| Error::Range)?,
            buffer: n(6),
        };
        if r.generation == 0 || r.sequence == 0 || r.namespace == 0 {
            return Err(Error::InvalidId);
        }
        match r.operation {
            IDENTIFY => {
                if r.lba > 3 || r.bytes != 4096 || r.buffer == 0 {
                    return Err(Error::Range);
                }
            }
            READ | WRITE | WRITE_FUA => {
                if r.bytes == 0
                    || r.bytes as usize > crate::nvme::MAX_IO_BYTES
                    || r.bytes % 512 != 0
                    || r.buffer == 0
                    || r.lba.checked_add(u64::from(r.bytes) / 512).is_none()
                {
                    return Err(Error::Range);
                }
            }
            GATHER | READ_GATHER => {
                if r.lba == 0
                    || r.lba > MAX_SEGMENTS as u64
                    || r.bytes == 0
                    || r.bytes as usize > crate::nvme::MAX_IO_BYTES
                    || r.bytes % 4096 != 0
                    || r.buffer == 0
                {
                    return Err(Error::Range);
                }
            }
            FLUSH | SHUTDOWN => {
                if r.bytes != 0 || r.buffer != 0 || r.lba != 0 {
                    return Err(Error::Range);
                }
            }
            _ => return Err(Error::Permission),
        }
        Ok(r)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub generation: u64,
    pub sequence: u64,
    pub bytes: u64,
    pub failed: bool,
}
impl Reply {
    pub fn encode(self) -> [u8; 32] {
        let mut b = [0; 32];
        for (i, n) in [
            self.generation,
            self.sequence,
            self.bytes,
            u64::from(self.failed),
        ]
        .into_iter()
        .enumerate()
        {
            b[i * 8..i * 8 + 8].copy_from_slice(&n.to_le_bytes())
        }
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != 32 {
            return Err(Error::Size);
        }
        let n = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        if n(3) > 1 {
            return Err(Error::Reserved);
        }
        Ok(Self {
            generation: n(0),
            sequence: n(1),
            bytes: n(2),
            failed: n(3) == 1,
        })
    }
}
/// One command owns both data and PRP pages until its exact receipt is taken.
/// No in-place restart: even a clean controller update requires a new boot.
pub struct Channel {
    generation: u64,
    next: u64,
    request: Option<Request>,
    segments: Segments,
    dispatched: bool,
    reply: Option<Reply>,
    closed: bool,
    dirty: bool,
}
impl Channel {
    pub const fn new(generation: u64) -> Self {
        Self {
            generation,
            next: 1,
            request: None,
            segments: Segments::EMPTY,
            dispatched: false,
            reply: None,
            closed: false,
            dirty: false,
        }
    }
    pub fn submit(
        &mut self,
        namespace: u32,
        operation: u64,
        lba: u64,
        bytes: u32,
        buffer: u64,
    ) -> Result<u64, Error> {
        // A gathered write is only valid together with its runs.
        if matches!(operation, GATHER | READ_GATHER) {
            return Err(Error::Permission);
        }
        self.enqueue(namespace, operation, lba, bytes, buffer, Segments::EMPTY)
    }
    /// Several page runs the owner already packed, in order, at `buffer`.
    pub fn submit_gather(
        &mut self,
        namespace: u32,
        runs: &[Segment],
        buffer: u64,
    ) -> Result<u64, Error> {
        let segments = Segments::new(runs)?;
        self.enqueue(
            namespace,
            GATHER,
            runs.len() as u64,
            segments.bytes() as u32,
            buffer,
            segments,
        )
    }
    pub fn submit_read_gather(
        &mut self,
        namespace: u32,
        runs: &[Segment],
        buffer: u64,
    ) -> Result<u64, Error> {
        let segments = Segments::new(runs)?;
        self.enqueue(
            namespace,
            READ_GATHER,
            runs.len() as u64,
            segments.bytes() as u32,
            buffer,
            segments,
        )
    }
    fn enqueue(
        &mut self,
        namespace: u32,
        operation: u64,
        lba: u64,
        bytes: u32,
        buffer: u64,
        segments: Segments,
    ) -> Result<u64, Error> {
        if self.closed || self.generation == 0 {
            return Err(Error::State);
        }
        if self.request.is_some() {
            return Err(Error::Backpressure);
        }
        let r = Request {
            generation: self.generation,
            sequence: self.next,
            namespace,
            operation,
            lba,
            bytes,
            buffer,
        };
        Request::decode(&r.encode())?;
        self.next = self.next.checked_add(1).ok_or(Error::Exhausted)?;
        self.dirty |= matches!(operation, WRITE | GATHER);
        self.request = Some(r);
        self.segments = segments;
        self.dispatched = false;
        Ok(r.sequence)
    }
    pub fn fetch(&mut self) -> Option<Request> {
        if self.closed || self.dispatched {
            return None;
        }
        let r = self.request?;
        self.dispatched = true;
        Some(r)
    }
    /// Runs of the current GATHER request; empty for every other operation.
    pub fn segments(&self) -> &[Segment] {
        self.segments.as_slice()
    }
    /// Dispatch the pending request as one FETCH payload: the 64-byte header
    /// followed, for GATHER, by its runs. Returns the payload length.
    pub fn fetch_encoded(&mut self, out: &mut [u8]) -> Option<usize> {
        let runs = self.segments.as_slice();
        let length = BYTES + runs.len() * 8;
        if self.closed || self.dispatched || self.request.is_none() || out.len() < length {
            return None;
        }
        for (i, run) in runs.iter().enumerate() {
            out[BYTES + i * 8..BYTES + i * 8 + 8].copy_from_slice(&run.encode().ok()?);
        }
        let r = self.fetch()?;
        out[..BYTES].copy_from_slice(&r.encode());
        Some(length)
    }
    pub fn complete(&mut self, r: Reply) -> Result<(), Error> {
        let q = self.request.ok_or(Error::Stale)?;
        if self.closed
            || !self.dispatched
            || self.reply.is_some()
            || r.generation != self.generation
            || r.sequence != q.sequence
            || r.bytes != if r.failed { 0 } else { u64::from(q.bytes) }
        {
            return Err(Error::Stale);
        }
        if r.failed {
            self.fail();
            return Err(Error::State);
        }
        if matches!(q.operation, FLUSH | SHUTDOWN) {
            self.dirty = false
        }
        self.reply = Some(r);
        Ok(())
    }
    pub fn take(&mut self, id: u64) -> Result<Option<Reply>, Error> {
        if self.closed {
            return Err(Error::State);
        }
        if self.request.is_none_or(|r| r.sequence != id) {
            return Err(Error::Stale);
        }
        let r = self.reply.take();
        if r.is_some() {
            self.request = None;
            self.segments = Segments::EMPTY;
            self.dispatched = false
        }
        Ok(r)
    }
    pub fn readable(&self) -> bool {
        !self.closed && self.request.is_some() && !self.dispatched
    }
    pub fn idle(&self) -> bool {
        !self.closed && self.request.is_none()
    }
    pub fn clean(&self) -> bool {
        self.idle() && !self.dirty
    }
    pub fn active(&self) -> bool {
        !self.closed
    }
    pub fn fail(&mut self) {
        self.closed = true;
        self.reply = None;
    }
}
