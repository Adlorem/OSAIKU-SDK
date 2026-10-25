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
            READ | WRITE => {
                if r.bytes == 0
                    || r.bytes as usize > crate::nvme::MAX_IO_BYTES
                    || r.bytes % 512 != 0
                    || r.buffer == 0
                    || r.lba.checked_add(u64::from(r.bytes) / 512).is_none()
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
        self.dirty |= operation == WRITE;
        self.request = Some(r);
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
