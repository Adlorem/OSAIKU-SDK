//! L2 transport: private geometry, explicit wire entries and bounded ownership.
//! Completion circulates a slot; it never releases a kernel DMA lease.
use crate::protocol::Error;
pub const ENTRY_BYTES: usize = 32;
pub const MIN_RING: u32 = 64;
pub const MAX_RING: u32 = 4096;
pub const MAX_SLOT_BYTES: u32 = 65536;
pub const MAJOR: u16 = 1;
pub const COMPLETED: u16 = 0;
pub const DROPPED: u16 = 1;
pub const FAILED: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub major: u16,
    pub supported: u64,
    pub required: u64,
}
impl Offer {
    pub fn encode(self) -> [u8; 32] {
        let mut b = [0; 32];
        b[..4].copy_from_slice(b"ONQ1");
        b[4..6].copy_from_slice(&self.major.to_le_bytes());
        b[8..16].copy_from_slice(&self.supported.to_le_bytes());
        b[16..24].copy_from_slice(&self.required.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8; 32]) -> Result<Self, Error> {
        if &b[..4] != b"ONQ1" || u16::from_le_bytes([b[4], b[5]]) != MAJOR {
            return Err(Error::Version);
        }
        if b[6..8].iter().chain(&b[24..]).any(|n| *n != 0) {
            return Err(Error::Reserved);
        }
        let offer = Self {
            major: MAJOR,
            supported: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            required: u64::from_le_bytes(b[16..24].try_into().unwrap()),
        };
        // First profile deliberately negotiates no offloads.
        if offer.required != 0 {
            return Err(Error::Features);
        }
        Ok(offer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub generation: u32,
    pub capacity: u32,
    pub slots: u32,
    pub slot_bytes: u32,
    pub mtu: u32,
}
impl Geometry {
    pub fn validate(self) -> Result<(), Error> {
        if self.generation == 0
            || !self.capacity.is_power_of_two()
            || !(MIN_RING..=MAX_RING).contains(&self.capacity)
            || self.slots == 0
            || self.slots > self.capacity
            || self.slot_bytes == 0
            || self.slot_bytes > MAX_SLOT_BYTES
            || self.slot_bytes & 4095 != 0
            || self.mtu < 68
            || self.mtu.checked_add(18).is_none_or(|n| n > self.slot_bytes)
        {
            return Err(Error::Range);
        }
        Ok(())
    }
    pub fn data_bytes(self) -> u64 {
        u64::from(self.slots) * u64::from(self.slot_bytes)
    }
    pub fn offset(self, slot: u32) -> Result<u64, Error> {
        if slot >= self.slots {
            return Err(Error::Range);
        }
        Ok(u64::from(slot) * u64::from(self.slot_bytes))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingKind {
    RxDone,
    RxReturn,
    TxSubmit,
    TxDone,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub slot: u32,
    pub generation: u32,
    pub length: u32,
    pub status: u16,
}
impl Entry {
    pub fn encode(self) -> [u8; ENTRY_BYTES] {
        let mut b = [0; ENTRY_BYTES];
        b[..4].copy_from_slice(&self.slot.to_le_bytes());
        b[4..8].copy_from_slice(&self.generation.to_le_bytes());
        b[8..12].copy_from_slice(&self.length.to_le_bytes());
        b[14..16].copy_from_slice(&self.status.to_le_bytes());
        b
    }
    /// Input is a private snapshot, never a reference into a peer's mapping.
    pub fn decode(b: [u8; ENTRY_BYTES], g: Geometry, kind: RingKind) -> Result<Self, Error> {
        g.validate()?;
        if b[12..14].iter().chain(&b[16..]).any(|n| *n != 0) {
            return Err(Error::Reserved);
        }
        let e = Self {
            slot: u32::from_le_bytes(b[..4].try_into().unwrap()),
            generation: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            length: u32::from_le_bytes(b[8..12].try_into().unwrap()),
            status: u16::from_le_bytes(b[14..16].try_into().unwrap()),
        };
        if e.generation != g.generation {
            return Err(Error::Stale);
        }
        g.offset(e.slot)?;
        if e.length > g.slot_bytes || e.status > FAILED {
            return Err(Error::Range);
        }
        match kind {
            RingKind::RxReturn if e.length != 0 || e.status != COMPLETED => {
                return Err(Error::Range)
            }
            RingKind::TxSubmit if e.status != COMPLETED => return Err(Error::Range),
            _ => (),
        }
        if matches!(kind, RingKind::RxDone | RingKind::TxSubmit)
            && (e.length < 14 || e.length > g.mtu + 18)
        {
            return Err(Error::Range);
        }
        Ok(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub mac: [u8; 6],
    pub mtu: u32,
    pub up: bool,
    pub generation: u32,
}
impl Link {
    pub fn validate(self, g: Geometry) -> Result<(), Error> {
        g.validate()?;
        if self.generation != g.generation {
            return Err(Error::Stale);
        }
        if self.mac == [0; 6] || self.mac[0] & 1 != 0 || self.mtu != g.mtu {
            return Err(Error::Range);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub packets: u64,
    pub bytes: u64,
    pub full: u64,
    pub invalid: u64,
    pub dropped: u64,
}

/// The counters below are private. Even the index stored on our own shared
/// page is not reread as an authority. Peer progress is checked modulo 2^32.
pub struct Cursor {
    local: u32,
    peer: u32,
    capacity: u32,
    producer: bool,
}
impl Cursor {
    pub fn matches(&self, capacity: u32, producer: bool) -> bool {
        self.capacity == capacity && self.producer == producer
    }
    pub fn new(capacity: u32, producer: bool) -> Result<Self, Error> {
        if !capacity.is_power_of_two() || !(MIN_RING..=MAX_RING).contains(&capacity) {
            return Err(Error::Range);
        }
        Ok(Self {
            local: 0,
            peer: 0,
            capacity,
            producer,
        })
    }
    pub fn available(&mut self, peer: u32) -> Result<u32, Error> {
        let progress = peer.wrapping_sub(self.peer);
        let used = if self.producer {
            self.local.wrapping_sub(peer)
        } else {
            peer.wrapping_sub(self.local)
        };
        if progress > self.capacity || used > self.capacity {
            return Err(Error::Range);
        }
        self.peer = peer;
        Ok(if self.producer {
            self.capacity - used
        } else {
            used
        })
    }
    pub fn index(&self) -> usize {
        (self.local & (self.capacity - 1)) as usize
    }
    pub fn advance(&mut self) -> u32 {
        self.local = self.local.wrapping_add(1);
        self.local
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Local,
    Peer,
    Return,
}
/// One ledger per direction on each side. Only a validated transition can
/// circulate a slot; duplicate/premature returns cannot create a new credit.
pub struct Ownership<const N: usize> {
    slots: [Slot; N],
    count: usize,
    generation: u32,
    closed: bool,
}
impl<const N: usize> Ownership<N> {
    pub fn new(g: Geometry, initially_local: bool) -> Result<Self, Error> {
        g.validate()?;
        if g.slots as usize > N {
            return Err(Error::Size);
        }
        Ok(Self {
            slots: [if initially_local {
                Slot::Local
            } else {
                Slot::Peer
            }; N],
            count: g.slots as usize,
            generation: g.generation,
            closed: false,
        })
    }
    fn change(&mut self, e: Entry, from: Slot, to: Slot) -> Result<(), Error> {
        if self.closed || e.generation != self.generation {
            return Err(Error::Stale);
        }
        if e.slot as usize >= self.count {
            return Err(Error::Range);
        }
        let s = &mut self.slots[e.slot as usize];
        if *s != from {
            return Err(Error::State);
        }
        *s = to;
        Ok(())
    }
    pub fn publish(&mut self, e: Entry) -> Result<(), Error> {
        self.change(e, Slot::Local, Slot::Peer)
    }
    pub fn receive(&mut self, e: Entry) -> Result<(), Error> {
        self.change(e, Slot::Peer, Slot::Local)
    }
    pub fn queue_return(&mut self, e: Entry) -> Result<(), Error> {
        self.change(e, Slot::Local, Slot::Return)
    }
    pub fn returned(&mut self, e: Entry) -> Result<(), Error> {
        self.change(e, Slot::Return, Slot::Peer)
    }
    pub fn close(&mut self) {
        self.closed = true;
    }
}

/// Ownership returns survive a full shared ring. There can be at most one
/// pending return per slot. The caller transitions its private ledger first.
pub struct Returns<const N: usize> {
    entries: [Entry; N],
    head: usize,
    count: usize,
    limit: usize,
    deadline: Option<u64>,
}
impl<const N: usize> Returns<N> {
    pub fn new(slots: usize) -> Result<Self, Error> {
        if slots == 0 || slots > N {
            return Err(Error::Size);
        }
        Ok(Self {
            entries: [Entry::default(); N],
            head: 0,
            count: 0,
            limit: slots,
            deadline: None,
        })
    }
    pub fn push(&mut self, e: Entry, now: u64, timeout: u64) -> Result<(), Error> {
        if self.count == self.limit {
            return Err(Error::Backpressure);
        }
        if timeout == 0 || now.checked_add(timeout).is_none() {
            return Err(Error::Deadline);
        }
        if self.entries().any(|p| p.slot == e.slot) {
            return Err(Error::State);
        }
        self.entries[(self.head + self.count) % self.limit] = e;
        if self.count == 0 {
            self.deadline = Some(now + timeout);
        }
        self.count += 1;
        Ok(())
    }
    fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        (0..self.count).map(|i| self.entries[(self.head + i) % self.limit])
    }
    pub fn front(&self) -> Option<Entry> {
        (self.count != 0).then(|| self.entries[self.head])
    }
    pub fn published(&mut self) -> Result<(), Error> {
        if self.count == 0 {
            return Err(Error::State);
        }
        self.head = (self.head + 1) % self.limit;
        self.count -= 1;
        if self.count == 0 {
            self.deadline = None;
        }
        Ok(())
    }
    pub fn expired(&self, now: u64) -> bool {
        self.deadline.is_some_and(|d| now >= d)
    }
}
