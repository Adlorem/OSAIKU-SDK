//! Direct-device ABI v2. A wire handle is a claim, never platform authority.
use crate::Error;

pub const SYSCALL: u64 = 406;
pub const MAJOR: u64 = 2;
pub const BYTES: usize = 512;
pub const DATA: usize = 128;
pub const MAX_PAGES: u64 = 256;
pub const MAX_REGIONS: usize = 16;
pub const MAX_BATCH: usize = 16;
pub const PAGE: u64 = 4096;
pub const MMIO_START: u64 = 0xa100_0000;
pub const DMA_START: u64 = 0xa200_0000;
pub const WINDOW_BYTES: u64 = 16 * 1024 * 1024;
pub mod op {
    pub const INVENTORY: u64 = 1;
    pub const ASSIGN: u64 = 2;
    pub const IDENTITY: u64 = 3;
    pub const MAP: u64 = 4;
    // 5 is reserved. MAP returns the single bounded DMA arena as well as MMIO.
    pub const START: u64 = 6;
    pub const REVOKE: u64 = 7;
    pub const STATUS: u64 = 8;
    pub const MEDIATED_SUBMIT: u64 = 9;
    pub const MEDIATED_TAKE: u64 = 10;
    pub const MEDIATED_COMPLETE: u64 = 11;
    pub const HEARTBEAT: u64 = 12;
    pub const MEDIATED_RESULT: u64 = 13;
    pub const RESET: u64 = 14;
    /// Read-only standard vendor capability descriptors for this assignment.
    /// Cursor counts descriptors; no arbitrary config offset or write exists.
    pub const PCI_CAPABILITY: u64 = 15;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle {
    pub id: u64,
    pub generation: u64,
}
impl Handle {
    pub const NONE: Self = Self {
        id: 0,
        generation: 0,
    };
    pub fn valid(self) -> bool {
        self.id != 0 && self.generation != 0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message {
    pub operation: u64,
    pub handle: Handle,
    pub device: u64,
    pub connection: u64,
    pub endpoint: u64,
    pub sequence: u64,
    pub offset: u64,
    pub length: u64,
    pub value: u64,
    pub data: [u8; BYTES - DATA],
}
impl Message {
    pub const fn new(operation: u64) -> Self {
        Self {
            operation,
            handle: Handle::NONE,
            device: 0,
            connection: 0,
            endpoint: 0,
            sequence: 0,
            offset: 0,
            length: 0,
            value: 0,
            data: [0; BYTES - DATA],
        }
    }
    pub fn encode(&self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"ODIRECT2");
        for (i, n) in [
            MAJOR,
            self.operation,
            self.handle.id,
            self.handle.generation,
            self.device,
            self.connection,
            self.endpoint,
            self.sequence,
            self.offset,
            self.length,
            self.value,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[DATA..].copy_from_slice(&self.data);
        b
    }
    pub fn decode(b: &[u8; BYTES]) -> Result<Self, Error> {
        let n = |i: usize| u64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap());
        if &b[..8] != b"ODIRECT2" || n(0) != MAJOR {
            return Err(Error::Version);
        }
        if b[96..DATA].iter().any(|b| *b != 0) {
            return Err(Error::Reserved);
        }
        if !(op::INVENTORY..=op::PCI_CAPABILITY).contains(&n(1)) {
            return Err(Error::Kind);
        }
        let mut m = Self::new(n(1));
        m.handle = Handle {
            id: n(2),
            generation: n(3),
        };
        m.device = n(4);
        m.connection = n(5);
        m.endpoint = n(6);
        m.sequence = n(7);
        m.offset = n(8);
        m.length = n(9);
        m.value = n(10);
        m.data.copy_from_slice(&b[DATA..]);
        Ok(m)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PageKind {
    Forbidden = 0,
    Direct = 1,
    Mediated = 2,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub bar: u8,
    pub kind: PageKind,
    pub first: u32,
    pub pages: u32,
}
impl Region {
    pub const EMPTY: Self = Self {
        bar: 0,
        kind: PageKind::Forbidden,
        first: 0,
        pages: 0,
    };
    pub fn end(self) -> Result<u64, Error> {
        if self.bar >= 6 || self.pages == 0 {
            return Err(Error::Range);
        }
        u64::from(self.first)
            .checked_add(u64::from(self.pages))
            .ok_or(Error::Range)
    }
}
/// Whole-page policy supplied only by the authenticated hardware service.
/// Omitted pages are forbidden. Native admission also excludes MSI-X/PBA and
/// aliases and checks BAR lengths; a valid profile alone grants no mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Profile {
    pub regions: [Region; MAX_REGIONS],
    pub count: usize,
    pub dma_pages: u64,
    pub irq_per_second: u32,
    pub irq_burst: u32,
}
impl Profile {
    pub const EMPTY: Self = Self {
        regions: [Region::EMPTY; MAX_REGIONS],
        count: 0,
        dma_pages: 0,
        irq_per_second: 0,
        irq_burst: 0,
    };
    pub fn validate(&self) -> Result<(), Error> {
        if self.count == 0
            || self.count > MAX_REGIONS
            || self.dma_pages == 0
            || self.dma_pages > MAX_PAGES
            || self.irq_per_second == 0
            || self.irq_per_second > 100_000
            || self.irq_burst == 0
            || self.irq_burst > self.irq_per_second
        {
            return Err(Error::Range);
        }
        for (i, r) in self.regions[..self.count].iter().enumerate() {
            let end = r.end()?;
            if end * PAGE > WINDOW_BYTES {
                return Err(Error::Range);
            }
            for other in &self.regions[..i] {
                if r.bar == other.bar
                    && u64::from(r.first) < other.end()?
                    && u64::from(other.first) < end
                {
                    return Err(Error::Permission);
                }
            }
        }
        if self.regions[self.count..]
            .iter()
            .any(|r| *r != Region::EMPTY)
        {
            return Err(Error::Reserved);
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<[u8; BYTES - DATA], Error> {
        self.validate()?;
        let mut b = [0; BYTES - DATA];
        b[..8].copy_from_slice(&self.dma_pages.to_le_bytes());
        b[8..12].copy_from_slice(&self.irq_per_second.to_le_bytes());
        b[12..16].copy_from_slice(&self.irq_burst.to_le_bytes());
        b[16..24].copy_from_slice(&(self.count as u64).to_le_bytes());
        for (i, r) in self.regions[..self.count].iter().enumerate() {
            let at = 32 + 16 * i;
            b[at] = r.bar;
            b[at + 1] = r.kind as u8;
            b[at + 4..at + 8].copy_from_slice(&r.first.to_le_bytes());
            b[at + 8..at + 12].copy_from_slice(&r.pages.to_le_bytes());
        }
        Ok(b)
    }
    pub fn decode(b: &[u8; BYTES - DATA]) -> Result<Self, Error> {
        let mut p = Self::EMPTY;
        p.dma_pages = u64::from_le_bytes(b[..8].try_into().unwrap());
        p.irq_per_second = u32::from_le_bytes(b[8..12].try_into().unwrap());
        p.irq_burst = u32::from_le_bytes(b[12..16].try_into().unwrap());
        p.count = usize::try_from(u64::from_le_bytes(b[16..24].try_into().unwrap()))
            .map_err(|_| Error::Range)?;
        if p.count > MAX_REGIONS {
            return Err(Error::Range);
        }
        if b[24..32]
            .iter()
            .chain(&b[32 + p.count * 16..])
            .any(|v| *v != 0)
        {
            return Err(Error::Reserved);
        }
        for (i, r) in p.regions[..p.count].iter_mut().enumerate() {
            let row = &b[32 + i * 16..48 + i * 16];
            if row[2..4].iter().chain(&row[12..]).any(|v| *v != 0) {
                return Err(Error::Reserved);
            }
            *r = Region {
                bar: row[0],
                kind: match row[1] {
                    0 => PageKind::Forbidden,
                    1 => PageKind::Direct,
                    2 => PageKind::Mediated,
                    _ => return Err(Error::Kind),
                },
                first: u32::from_le_bytes(row[4..8].try_into().unwrap()),
                pages: u32::from_le_bytes(row[8..12].try_into().unwrap()),
            };
        }
        p.validate()?;
        Ok(p)
    }
}
