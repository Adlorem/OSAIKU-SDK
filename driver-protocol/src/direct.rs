//! Direct-device ABI v2. A wire handle is a claim, never platform authority.
use crate::Error;

pub const SYSCALL: u64 = 406;
pub const MAJOR: u64 = 2;
pub const BYTES: usize = 512;
pub const DATA: usize = 128;
pub const LEGACY_MAX_PAGES: u64 = 256;
/// Larger arenas require the trusted service-DMA profile; ordinary direct
/// clients retain their original one-MiB ceiling.
pub const MAX_PAGES: u64 = 1024;
pub const MAX_REGIONS: usize = 16;
pub const MAX_BATCH: usize = 16;
pub const PAGE: u64 = 4096;
pub const MMIO_START: u64 = 0xa100_0000;
pub const DMA_START: u64 = 0xa200_0000;
pub const WINDOW_BYTES: u64 = 16 * 1024 * 1024;
pub const DMA_WINDOW_BYTES: u64 = 64 * 1024 * 1024;
pub const COPY_PAGES: u64 = 2;
pub const SERVICE_SLOT_BYTES: u64 = 8 * 1024 * 1024;
pub const SERVICE_COPY_OFFSET: u64 = MAX_PAGES * PAGE;
const _: () = assert!(MMIO_START + WINDOW_BYTES == DMA_START);
const _: () = assert!(SERVICE_COPY_OFFSET + COPY_PAGES * PAGE <= SERVICE_SLOT_BYTES);
const _: () = assert!(8 * SERVICE_SLOT_BYTES == DMA_WINDOW_BYTES);

/// Device access layout, independent of register protocol. Page zero is RW;
/// the next pages are device-write-only, the tail device-read-only.
pub const fn device_read_start(pages: u64) -> u64 {
    (pages / 2 + pages % 2) * PAGE
}
/// A page-aligned bidirectional subrange in the device-read tail. This value
/// describes layout only; only the authenticated service-DMA owner may submit
/// it during initial SERVICE_COMPLETE, before publication/BME. It is not an
/// address grant and cannot cover another lease, MMIO or client copy buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoherentRange { first: u64, end: u64 }
impl CoherentRange {
    pub fn decode(pages: u64, offset: u64, length: u64) -> Result<Option<Self>, Error> {
        if !(3..=MAX_PAGES).contains(&pages) { return Err(Error::Range); }
        if offset == 0 && length == 0 { return Ok(None); }
        let end = offset.checked_add(length).ok_or(Error::Range)?;
        if length == 0 || offset % PAGE != 0 || length % PAGE != 0
            || offset < device_read_start(pages) || end > pages * PAGE {
            return Err(Error::Range);
        }
        Ok(Some(Self { first: offset / PAGE, end: end / PAGE }))
    }
    pub fn contains(self, page: u64) -> bool { page >= self.first && page < self.end }
    pub fn first_page(self) -> u64 { self.first }
    pub fn pages(self) -> u64 { self.end - self.first }
}
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
    /// Trusted service only: bounded family initialization/reset transaction.
    pub const SERVICE_BEGIN: u64 = 16;
    /// value=0 completes; value=1 aborts. Successful initial service-DMA
    /// completion may supply offset/length of one CoherentRange. 0/0 retains
    /// the directional layout. Recovery cannot change an existing layout.
    pub const SERVICE_COMPLETE: u64 = 17;
    pub const SERVICE_CHECK: u64 = 18;
    /// Session-bound read of one aligned dword in standard device-specific
    /// configuration (0x40..=0xfc). Trusted service only; no PCI writes.
    pub const SERVICE_CONFIG_READ: u64 = 19;
    pub const SERVICE_START: u64 = 20;
    /// Trusted backend observation only; never grants DMA/reset authority.
    pub const SERVICE_DIAGNOSTIC: u64 = 21;
    /// Read-only policy/resource preflight. value bit 0: candidate qualified;
    /// bit 1: this exact Bound lease has client mappings ready for SERVICE_BEGIN.
    /// bit 2 alone: revoked Wi-Fi lease has an observed discrete function-FLR
    /// recovery scope. RESET still requires a new explicit policy operation,
    /// retired aliases/client/IRQ and a native reset completion receipt.
    /// bit 3 alone: CNVi recovery candidate; PLDR_SCOPE must still qualify
    /// ACPI/BT dependencies and an independent ME DMA requester before ME_OPEN.
    /// bit 4 alone: retired Wi-Fi lease qualifies for CSR-only ordinary MAC
    /// restart through SERVICE_BEGIN; no DMA or PLDR authority is implied.
    pub const ADMISSION: u64 = 22;
    /// Trusted hardware_manager only. ME has its own requester, arena and epoch.
    pub const ME_OPEN: u64 = 23;
    pub const ME_CHECK: u64 = 24;
    pub const ME_CLOSE: u64 = 25;
    /// Asynchronous native ACPI reset-scope query, execution and completion.
    pub const PLDR_SCOPE: u64 = 26;
    pub const PLDR_BEGIN: u64 = 27;
    pub const PLDR_STATUS: u64 = 28;
    /// Bounded HECI PIO, with no raw BAR mapping in userspace.
    pub const ME_READ: u64 = 29;
    pub const ME_WRITE: u64 = 30;
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
        if !(op::INVENTORY..=op::ME_WRITE).contains(&n(1)) || n(1) == 5 {
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
    /// Family initialization gates DMA publication and START. Every MMIO page
    /// must be mediated; only the authenticated service may complete the gate.
    pub service_lifecycle: bool,
    /// Only the authenticated service maps DMA. Client maps COPY_PAGES of
    /// ordinary CPU-only memory, receives no IOVA and cannot START hardware.
    pub service_dma: bool,
    /// Bind START and continued DMA to the kernel Wi-Fi policy session.
    pub wifi_policy: bool,
    /// This register transport implements MSI only; never silently choose MSI-X.
    pub msi_only: bool,
}
impl Profile {
    pub const EMPTY: Self = Self {
        regions: [Region::EMPTY; MAX_REGIONS],
        count: 0,
        dma_pages: 0,
        irq_per_second: 0,
        irq_burst: 0,
        service_lifecycle: false,
        service_dma: false,
        wifi_policy: false,
        msi_only: false,
    };
    pub fn validate(&self) -> Result<(), Error> {
        if self.service_dma && !self.service_lifecycle || self.wifi_policy && !self.service_dma {
            return Err(Error::Permission);
        }
        if self.count == 0
            || self.count > MAX_REGIONS
            || self.dma_pages == 0
            || self.dma_pages > MAX_PAGES
            || (!self.service_dma && self.dma_pages > LEGACY_MAX_PAGES)
            || self.irq_per_second == 0
            || self.irq_per_second > 100_000
            || self.irq_burst == 0
            || self.irq_burst > self.irq_per_second
        {
            return Err(Error::Range);
        }
        for (i, r) in self.regions[..self.count].iter().enumerate() {
            if self.service_lifecycle && r.kind == PageKind::Direct {
                return Err(Error::Permission);
            }
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
        b[24] = u8::from(self.service_lifecycle);
        b[25] = u8::from(self.service_dma);
        b[26] = u8::from(self.wifi_policy);
        b[27] = u8::from(self.msi_only);
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
        if b[24] > 1 || b[25] > 1 || b[26] > 1 || b[27] > 1 {
            return Err(Error::Reserved);
        }
        p.service_lifecycle = b[24] == 1;
        p.service_dma = b[25] == 1;
        p.wifi_policy = b[26] == 1;
        p.msi_only = b[27] == 1;
        if b[28..32]
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
