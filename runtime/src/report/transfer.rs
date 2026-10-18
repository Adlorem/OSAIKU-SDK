//! Bounded definition transfer over an already authenticated host channel.
//! Numeric window identities bind transactions; they grant no IPC authority.
use osaiku_ui::{Identity, MAX_WIRE};
pub const PACKET_BYTES: usize = 152;
pub const CHUNK_BYTES: usize = 80;
pub const REPLY_BYTES: usize = 80;
const MAGIC: &[u8; 8] = b"OSAIRPT1";
const REPLY_MAGIC: &[u8; 8] = b"OSAIRPA1";
pub const BEGIN: u16 = 1;
pub const CHUNK: u16 = 2;
pub const FINISH: u16 = 3;
pub const CANCEL: u16 = 4;
/// Select an immutable, host-registered definition by wire hash and length.
pub const CATALOG: u16 = 5;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Status {
    Receiving = 1,
    Staged = 2,
    Cancelled = 3,
    Invalid = 4,
    Stale = 5,
    Busy = 6,
    Rejected = 7,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub kind: u16,
    pub request: u64,
    pub target: Identity,
    pub offset: u32,
    pub total: u32,
    pub bytes: &'a [u8],
}
fn word(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn put(b: &mut [u8], at: usize, value: u64) {
    b[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn identity(b: &[u8]) -> Identity {
    Identity {
        epoch: word(b, 24),
        owner: word(b, 32),
        window: word(b, 40),
        incarnation: word(b, 48),
        revision: word(b, 56),
    }
}
fn put_identity(b: &mut [u8], id: Identity) {
    for (i, v) in [id.epoch, id.owner, id.window, id.incarnation, id.revision]
        .iter()
        .enumerate()
    {
        put(b, 24 + i * 8, *v);
    }
}
fn live(id: Identity) -> bool {
    id.epoch != 0 && id.owner != 0 && id.window != 0 && id.incarnation != 0 && id.revision != 0
}
pub fn candidate(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}
impl Packet<'_> {
    fn valid(&self) -> bool {
        self.request != 0
            && live(self.target)
            && self.total > 0
            && self.total as usize <= MAX_WIRE
            && match self.kind {
                CATALOG => self.offset == 0 && self.bytes.len() == 8,
                BEGIN | CANCEL => self.offset == 0 && self.bytes.is_empty(),
                CHUNK => {
                    !self.bytes.is_empty()
                        && self.bytes.len() <= CHUNK_BYTES
                        && self
                            .offset
                            .checked_add(self.bytes.len() as u32)
                            .is_some_and(|end| end <= self.total)
                }
                FINISH => self.offset == self.total && self.bytes.is_empty(),
                _ => false,
            }
    }
    pub fn encode(&self) -> Option<[u8; PACKET_BYTES]> {
        if !self.valid() {
            return None;
        }
        let mut out = [0; PACKET_BYTES];
        out[..8].copy_from_slice(MAGIC);
        out[8..10].copy_from_slice(&1u16.to_le_bytes());
        out[10..12].copy_from_slice(&self.kind.to_le_bytes());
        out[12..16].copy_from_slice(&(self.bytes.len() as u32).to_le_bytes());
        put(&mut out, 16, self.request);
        put_identity(&mut out, self.target);
        out[64..68].copy_from_slice(&self.offset.to_le_bytes());
        out[68..72].copy_from_slice(&self.total.to_le_bytes());
        out[72..72 + self.bytes.len()].copy_from_slice(self.bytes);
        Some(out)
    }
}
impl<'a> Packet<'a> {
    pub fn decode(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() != PACKET_BYTES || !candidate(bytes) || bytes[8..10] != 1u16.to_le_bytes() {
            return None;
        }
        let len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
        if len > CHUNK_BYTES || bytes[72 + len..].iter().any(|b| *b != 0) {
            return None;
        }
        let packet = Self {
            kind: u16::from_le_bytes(bytes[10..12].try_into().ok()?),
            request: word(bytes, 16),
            target: identity(bytes),
            offset: u32::from_le_bytes(bytes[64..68].try_into().ok()?),
            total: u32::from_le_bytes(bytes[68..72].try_into().ok()?),
            bytes: &bytes[72..72 + len],
        };
        packet.valid().then_some(packet)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: Status,
    pub request: u64,
    pub target: Identity,
    pub received: u32,
    pub total: u32,
}
impl Reply {
    pub fn for_packet(packet: Packet<'_>, status: Status, received: u32) -> Self {
        Self {
            status,
            request: packet.request,
            target: packet.target,
            received,
            total: packet.total,
        }
    }
    pub fn encode(self) -> [u8; REPLY_BYTES] {
        let mut b = [0; REPLY_BYTES];
        b[..8].copy_from_slice(REPLY_MAGIC);
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[10..12].copy_from_slice(&(self.status as u16).to_le_bytes());
        put(&mut b, 16, self.request);
        put_identity(&mut b, self.target);
        b[64..68].copy_from_slice(&self.received.to_le_bytes());
        b[68..72].copy_from_slice(&self.total.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != REPLY_BYTES
            || &b[..8] != REPLY_MAGIC
            || b[8..10] != 1u16.to_le_bytes()
            || b[12..16] != [0; 4]
            || b[72..] != [0; 8]
        {
            return None;
        }
        let status = match u16::from_le_bytes(b[10..12].try_into().ok()?) {
            1 => Status::Receiving,
            2 => Status::Staged,
            3 => Status::Cancelled,
            4 => Status::Invalid,
            5 => Status::Stale,
            6 => Status::Busy,
            7 => Status::Rejected,
            _ => return None,
        };
        let result = Self {
            status,
            request: word(b, 16),
            target: identity(b),
            received: u32::from_le_bytes(b[64..68].try_into().ok()?),
            total: u32::from_le_bytes(b[68..72].try_into().ok()?),
        };
        (result.request != 0
            && live(result.target)
            && result.total > 0
            && result.total as usize <= MAX_WIRE
            && result.received <= result.total)
            .then_some(result)
    }
    /// Sender must separately authenticate the IPC reply endpoint.
    pub fn matches(self, packet: Packet<'_>) -> bool {
        packet.valid()
            && self.request == packet.request
            && self.target == packet.target
            && self.total == packet.total
            && match packet.kind {
                BEGIN => self.status == Status::Receiving && self.received == 0,
                CHUNK => {
                    self.status == Status::Receiving
                        && self.received == packet.offset + packet.bytes.len() as u32
                }
                FINISH | CATALOG => self.status == Status::Staged && self.received == packet.total,
                CANCEL => self.status == Status::Cancelled,
                _ => false,
            }
    }
}

#[derive(Clone, Copy)]
struct Transfer {
    request: u64,
    target: Identity,
    total: u32,
    received: u32,
    ready: bool,
}
/// Two fixed wire slots: accepted definition plus candidate. Keep in image/heap
/// storage. Finish means staged, never published; acceptance requires commit.
pub struct Inbox {
    slots: [[u8; MAX_WIRE]; 2],
    active: usize,
    length: usize,
    pending: Option<Transfer>,
    last_request: u64,
}
impl Inbox {
    pub const EMPTY: Self = Self {
        slots: [[0; MAX_WIRE]; 2],
        active: 0,
        length: 0,
        pending: None,
        last_request: 0,
    };
    pub fn receive(
        &mut self,
        packet: Packet<'_>,
        current: Identity,
        validate: impl FnOnce(&[u8]) -> bool,
    ) -> Reply {
        let status = self.accept(packet, current, validate);
        let received = self
            .pending
            .filter(|p| {
                p.request == packet.request && p.target == packet.target && p.total == packet.total
            })
            .map_or(0, |p| p.received);
        Reply::for_packet(packet, status, received)
    }
    /// The catalog is supplied by the application, never by the IPC caller.
    pub fn receive_catalog(
        &mut self,
        packet: Packet<'_>,
        current: Identity,
        catalog: &[&[u8]],
        validate: impl FnOnce(&[u8]) -> bool,
    ) -> Reply {
        if packet.kind != CATALOG {
            return self.receive(packet, current, validate);
        }
        let status = if !packet.valid() {
            Status::Invalid
        } else if packet.target != current || packet.request <= self.last_request {
            Status::Stale
        } else if self.pending.is_some() {
            Status::Busy
        } else {
            self.last_request = packet.request;
            let key = u64::from_le_bytes(packet.bytes.try_into().unwrap());
            match catalog.iter().copied().find(|wire| {
                wire.len() == packet.total as usize && osaiku_ui::wire_hash(wire) == key
            }) {
                Some(wire) if validate(wire) => {
                    self.slots[1 - self.active][..wire.len()].copy_from_slice(wire);
                    self.pending = Some(Transfer {
                        request: packet.request,
                        target: current,
                        total: packet.total,
                        received: packet.total,
                        ready: true,
                    });
                    Status::Staged
                }
                _ => Status::Rejected,
            }
        };
        Reply::for_packet(
            packet,
            status,
            if status == Status::Staged {
                packet.total
            } else {
                0
            },
        )
    }
    fn accept(
        &mut self,
        p: Packet<'_>,
        current: Identity,
        validate: impl FnOnce(&[u8]) -> bool,
    ) -> Status {
        if !p.valid() {
            return Status::Invalid;
        }
        if p.kind == CATALOG {
            return Status::Rejected;
        }
        if p.target != current {
            return Status::Stale;
        }
        if p.kind == BEGIN {
            if self.pending.is_some() {
                return Status::Busy;
            }
            if p.request <= self.last_request {
                return Status::Stale;
            }
            self.last_request = p.request;
            self.pending = Some(Transfer {
                request: p.request,
                target: p.target,
                total: p.total,
                received: 0,
                ready: false,
            });
            return Status::Receiving;
        }
        let Some(mut t) = self.pending else {
            return Status::Stale;
        };
        if (t.request, t.target, t.total) != (p.request, p.target, p.total) {
            return Status::Stale;
        }
        if p.kind == CANCEL {
            self.discard();
            return Status::Cancelled;
        }
        if t.ready {
            return Status::Busy;
        }
        if p.kind == CHUNK {
            if p.offset != t.received {
                return Status::Stale;
            }
            let end = p.offset as usize + p.bytes.len();
            self.slots[1 - self.active][p.offset as usize..end].copy_from_slice(p.bytes);
            t.received = end as u32;
            self.pending = Some(t);
            return Status::Receiving;
        }
        if t.received != t.total {
            return Status::Stale;
        }
        if !validate(&self.slots[1 - self.active][..t.total as usize]) {
            self.discard();
            return Status::Rejected;
        }
        t.ready = true;
        self.pending = Some(t);
        Status::Staged
    }
    pub fn definition<'a>(&'a self, fallback: &'a [u8]) -> &'a [u8] {
        if let Some(t) = self.pending.filter(|t| t.ready) {
            &self.slots[1 - self.active][..t.total as usize]
        } else if self.length != 0 {
            &self.slots[self.active][..self.length]
        } else {
            fallback
        }
    }
    pub fn staged(&self) -> bool {
        self.pending.is_some_and(|t| t.ready)
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    /// A locally opened artifact passed the same schema admission as uploads.
    /// It still requires a successful publication before becoming active.
    pub fn stage_local(&mut self, wire: &[u8], target: Identity) -> bool {
        if self.pending.is_some() || wire.is_empty() || wire.len() > MAX_WIRE {
            return false;
        }
        self.slots[1 - self.active][..wire.len()].copy_from_slice(wire);
        self.pending = Some(Transfer {
            request: 0,
            target,
            total: wire.len() as u32,
            received: wire.len() as u32,
            ready: true,
        });
        true
    }
    /// Called after the corresponding report publication is accepted. Any
    /// partial transfer bound to the prior revision expires on publication.
    pub fn commit(&mut self) {
        if let Some(t) = self.pending.filter(|t| t.ready) {
            self.active = 1 - self.active;
            self.length = t.total as usize;
        }
        self.discard();
    }
    pub fn discard(&mut self) {
        self.slots[1 - self.active].fill(0);
        self.pending = None;
    }
    pub fn close(&mut self) {
        self.slots[0].fill(0);
        self.slots[1].fill(0);
        self.length = 0;
        self.pending = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadError {
    Invalid,
    Sender,
    Reply,
    Refused(Status),
    Complete,
}
/// Stop-and-wait producer: advance only after the exact receiver acknowledges
/// the preceding fragment. Staged completion still requires a normal window
/// presentation transaction and its successful publication receipt.
pub struct Upload<'a> {
    target: Identity,
    request: u64,
    wire: &'a [u8],
    offset: usize,
    kind: u16,
}
impl<'a> Upload<'a> {
    pub fn new(target: Identity, request: u64, wire: &'a [u8]) -> Result<Self, UploadError> {
        if !live(target)
            || !crate::process::ProcessEndpoint::decode(target.owner).names_instance()
            || request == 0
            || wire.is_empty()
            || wire.len() > MAX_WIRE
        {
            return Err(UploadError::Invalid);
        }
        Ok(Self {
            target,
            request,
            wire,
            offset: 0,
            kind: BEGIN,
        })
    }
    pub fn packet(&self) -> Result<Packet<'a>, UploadError> {
        if self.kind == 0 {
            return Err(UploadError::Complete);
        }
        let bytes = if self.kind == CHUNK {
            &self.wire[self.offset..(self.offset + CHUNK_BYTES).min(self.wire.len())]
        } else {
            &[]
        };
        Ok(Packet {
            kind: self.kind,
            request: self.request,
            target: self.target,
            offset: self.offset as u32,
            total: self.wire.len() as u32,
            bytes,
        })
    }
    pub fn acknowledge(
        &mut self,
        sender: crate::process::ProcessEndpoint,
        bytes: &[u8],
    ) -> Result<(), UploadError> {
        if sender.encode() != self.target.owner {
            return Err(UploadError::Sender);
        }
        let packet = self.packet()?;
        let reply = Reply::decode(bytes).ok_or(UploadError::Reply)?;
        if reply.request != packet.request
            || reply.target != packet.target
            || reply.total != packet.total
        {
            return Err(UploadError::Reply);
        }
        if matches!(
            reply.status,
            Status::Invalid | Status::Stale | Status::Busy | Status::Rejected
        ) {
            return Err(UploadError::Refused(reply.status));
        }
        if !reply.matches(packet) {
            return Err(UploadError::Reply);
        }
        match self.kind {
            BEGIN => self.kind = CHUNK,
            CHUNK => {
                self.offset += packet.bytes.len();
                if self.offset == self.wire.len() {
                    self.kind = FINISH;
                }
            }
            FINISH => self.kind = 0,
            _ => return Err(UploadError::Reply),
        }
        Ok(())
    }
    pub fn cancellation(&self) -> Packet<'a> {
        Packet {
            kind: CANCEL,
            request: self.request,
            target: self.target,
            offset: 0,
            total: self.wire.len() as u32,
            bytes: &[],
        }
    }
    pub fn staged(&self) -> bool {
        self.kind == 0
    }
}
