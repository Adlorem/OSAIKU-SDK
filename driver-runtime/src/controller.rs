//! Controller process v1. Every offset is relative to an owner-granted arena
//! or aperture; there is no operation for PCI/config, IRQ routing or physical RAM.
use crate::protocol::Error;
pub const ENDPOINT: u64 = 6;
pub const IO: u64 = 17;
pub const FETCH: u64 = 18;
pub const COMPLETE: u64 = 19;
pub const READ_REGISTER: u64 = 1;
pub const WRITE_REGISTER: u64 = 2;
pub const READ_ARENA: u64 = 3;
pub const WRITE_ARENA: u64 = 4;
pub const PUBLISH: u64 = 5;
pub const ACQUIRE: u64 = 6;
pub const CLOCK: u64 = 7;
pub const GRANT: u64 = 8;
pub const BYTES: usize = 64;
pub const QUEUE: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub sequence: u64,
    pub generation: u64,
    pub operation: u8,
    pub slot: u8,
    pub endpoint: u8,
    pub port: u8,
    pub speed: u8,
    pub kind: u8,
    pub interval: u8,
    pub packet: u16,
    pub length: u16,
    pub setup: [u8; 8],
    pub timeout: u64,
}
pub mod request {
    pub const NOOP: u8 = 1;
    pub const ENABLE: u8 = 2;
    pub const DISABLE: u8 = 3;
    pub const ADDRESS: u8 = 4;
    pub const CONFIGURE: u8 = 5;
    pub const CONTROL: u8 = 6;
    pub const TRANSFER: u8 = 7;
    pub const RESET_PORT: u8 = 8;
    pub const PORT: u8 = 9;
    pub const STOP: u8 = 10;
    /// Full-speed EP0 packet size discovered by USB core. No ring address.
    pub const EVALUATE_EP0: u8 = 11;
}
impl Request {
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(&self.sequence.to_le_bytes());
        b[8..16].copy_from_slice(&self.generation.to_le_bytes());
        b[16..23].copy_from_slice(&[
            self.operation,
            self.slot,
            self.endpoint,
            self.port,
            self.speed,
            self.kind,
            self.interval,
        ]);
        b[24..26].copy_from_slice(&self.packet.to_le_bytes());
        b[26..28].copy_from_slice(&self.length.to_le_bytes());
        b[32..40].copy_from_slice(&self.setup);
        b[40..48].copy_from_slice(&self.timeout.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != BYTES {
            return Err(Error::Size);
        }
        if b[23] != 0 || b[28..32].iter().chain(b[48..].iter()).any(|v| *v != 0) {
            return Err(Error::Reserved);
        }
        let n = |a| u64::from_le_bytes(b[a..a + 8].try_into().unwrap());
        let r = Self {
            sequence: n(0),
            generation: n(8),
            operation: b[16],
            slot: b[17],
            endpoint: b[18],
            port: b[19],
            speed: b[20],
            kind: b[21],
            interval: b[22],
            packet: u16::from_le_bytes(b[24..26].try_into().unwrap()),
            length: u16::from_le_bytes(b[26..28].try_into().unwrap()),
            setup: b[32..40].try_into().unwrap(),
            timeout: n(40),
        };
        if r.sequence == 0
            || r.generation == 0
            || !(1..=11).contains(&r.operation)
            || r.length > 4096
        {
            return Err(Error::Range);
        }
        Ok(r)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub sequence: u64,
    pub generation: u64,
    pub slot: u8,
    pub endpoint: u8,
    pub code: u8,
    pub port: u8,
    pub actual: u32,
}
impl Reply {
    pub fn encode(self) -> [u8; 32] {
        let mut b = [0; 32];
        b[..8].copy_from_slice(&self.sequence.to_le_bytes());
        b[8..16].copy_from_slice(&self.generation.to_le_bytes());
        b[16..20].copy_from_slice(&[self.slot, self.endpoint, self.code, self.port]);
        b[20..24].copy_from_slice(&self.actual.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != 32 {
            return Err(Error::Size);
        }
        if b[24..].iter().any(|v| *v != 0) {
            return Err(Error::Reserved);
        }
        Ok(Self {
            sequence: u64::from_le_bytes(b[..8].try_into().unwrap()),
            generation: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            slot: b[16],
            endpoint: b[17],
            code: b[18],
            port: b[19],
            actual: u32::from_le_bytes(b[20..24].try_into().unwrap()),
        })
    }
}
#[derive(Clone, Copy)]
struct Entry {
    request: Request,
    dispatched: bool,
    reply: Option<Reply>,
}
/// Owner-private queue. Failure invalidates every child and outstanding receipt
/// of this controller generation; no automatic retry of a transfer/write.
pub struct Channel {
    generation: u64,
    last: u64,
    closed: bool,
    entries: [Option<Entry>; QUEUE],
    clients: u32,
    boot_media: bool,
    alternative_input: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Live,
    Restart,
}
impl Channel {
    pub const fn new(generation: u64) -> Self {
        Self {
            generation,
            last: 0,
            closed: generation == 0,
            entries: [None; QUEUE],
            clients: 0,
            boot_media: false,
            alternative_input: false,
        }
    }
    pub fn submit(&mut self, mut request: Request) -> Result<u64, Error> {
        if self.closed || request.generation != self.generation {
            return Err(Error::Stale);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|e| e.is_none())
            .ok_or(Error::Backpressure)?;
        self.last = self.last.checked_add(1).ok_or(Error::Exhausted)?;
        request.sequence = self.last;
        Request::decode(&request.encode())?;
        *slot = Some(Entry {
            request,
            dispatched: false,
            reply: None,
        });
        Ok(self.last)
    }
    pub fn fetch(&mut self) -> Option<Request> {
        if self.closed {
            return None;
        }
        let e = self
            .entries
            .iter_mut()
            .flatten()
            .filter(|e| !e.dispatched)
            .min_by_key(|e| e.request.sequence)?;
        e.dispatched = true;
        Some(e.request)
    }
    pub fn complete(&mut self, reply: Reply) -> Result<(), Error> {
        if self.closed || reply.generation != self.generation {
            return Err(Error::Stale);
        }
        let e = self
            .entries
            .iter_mut()
            .flatten()
            .find(|e| e.request.sequence == reply.sequence)
            .ok_or(Error::Stale)?;
        if !e.dispatched
            || e.reply.is_some()
            || (e.request.operation != request::PORT && reply.actual > 4096)
        {
            return Err(Error::Permission);
        }
        let q = e.request;
        let (slot, endpoint, port, limit) = match q.operation {
            request::ENABLE => (reply.slot, 0, 0, 0),
            request::CONTROL => (
                q.slot,
                1,
                0,
                u32::from(u16::from_le_bytes([q.setup[6], q.setup[7]])),
            ),
            request::TRANSFER => (q.slot, q.endpoint, 0, u32::from(q.length)),
            request::PORT => (0, 0, q.port, u32::MAX),
            request::RESET_PORT => (0, 0, q.port, 0),
            _ => (q.slot, 0, 0, 0),
        };
        if reply.slot != slot
            || reply.endpoint != endpoint
            || reply.port != port
            || reply.actual > limit
            || (q.operation == request::ENABLE && reply.code == 1 && !(1..=8).contains(&reply.slot))
        {
            return Err(Error::Permission);
        }
        e.reply = Some(reply);
        Ok(())
    }
    pub fn take(&mut self, sequence: u64) -> Result<Option<Reply>, Error> {
        if self.closed {
            return Err(Error::Stale);
        }
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.is_some_and(|e| e.request.sequence == sequence))
            .ok_or(Error::Stale)?;
        if let Some(r) = e.as_ref().unwrap().reply {
            *e = None;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
    pub fn readable(&self) -> bool {
        !self.closed && self.entries.iter().flatten().any(|e| !e.dispatched)
    }
    pub fn fail(&mut self) {
        self.closed = true;
        self.entries.fill(None);
        self.clients = 0;
    }
    pub fn active(&self) -> bool {
        !self.closed
    }
    /// Trusted dependency inventory, never values supplied in peer messages.
    pub fn dependencies(&mut self, clients: u32, boot_media: bool, alternative_input: bool) {
        self.clients = clients;
        self.boot_media = boot_media;
        self.alternative_input = alternative_input;
    }
    pub fn update(&self) -> Update {
        if self.closed
            || self.clients != 0
            || self.boot_media
            || !self.alternative_input
            || self.entries.iter().any(Option::is_some)
        {
            Update::Restart
        } else {
            Update::Live
        }
    }
}
