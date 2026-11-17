//! Retained page ownership interfaces. Implementors, not wire claims, enforce authority.
pub const PAGE: u64 = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Permission,
    Stale,
    Busy,
    Exhausted,
    Timeout,
    Quarantined,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Physical(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuVirtual(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuVirtual(u64);
impl Physical {
    pub fn page(value: u64) -> Result<Self, Error> {
        if value == 0 || value & (PAGE - 1) != 0 || value >= 1 << 39 {
            Err(Error::Invalid)
        } else {
            Ok(Self(value))
        }
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
impl CpuVirtual {
    /// # Safety
    /// An exclusive WB mapping of the provider's live page, valid until release.
    pub unsafe fn owned(value: u64) -> Self {
        Self(value)
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
impl GpuVirtual {
    pub fn page(value: u64) -> Result<Self, Error> {
        if value < PAGE || value & (PAGE - 1) != 0 || value >= 1 << 47 {
            Err(Error::Invalid)
        } else {
            Ok(Self(value))
        }
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Data,
    Table,
}
/// Implementors own the real allocator, zero before publishing, scrub before
/// release, charge owner + class, and exclude firmware/MMIO/other allocations.
/// `release` failure retains the token and charge. CPU aliases must be WB only.
pub unsafe trait Frames {
    type Frame;
    fn allocate(&mut self, owner: u64, class: Class) -> Result<Self::Frame, Error>;
    fn physical(&self, frame: &Self::Frame) -> Physical;
    fn cpu(&self, frame: &Self::Frame) -> CpuVirtual;
    fn release(&mut self, frame: &mut Self::Frame) -> Result<(), Error>;
    fn clean_invalidate(&mut self, frame: &Self::Frame);
    fn fence(&mut self);
}
/// A route-produced page capability, not a cast of a CPU address.
#[derive(Debug)]
pub struct DmaPage {
    address: u64,
    device: u64,
    generation: u64,
    serial: u64,
}
impl DmaPage {
    /// # Safety
    /// The provider has established this exact live mapping of owned, pinned
    /// RAM; serial is never reused within this device generation. No Drop unmap.
    pub unsafe fn established(
        address: u64,
        device: u64,
        generation: u64,
        serial: u64,
    ) -> Result<Self, Error> {
        if address == 0
            || address & 4095 != 0
            || address >= 1 << 39
            || device == 0
            || generation == 0
            || serial == 0
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            address,
            device,
            generation,
            serial,
        })
    }
    pub fn serial(&self) -> u64 {
        self.serial
    }
    pub fn address(&self) -> u64 {
        self.address
    }
    pub fn matches(&self, device: u64, generation: u64) -> bool {
        self.device == device && self.generation == generation
    }
}
/// A map error either leaves no mapping or makes `healthy` false. Unmap returns
/// only after IOTLB invalidation/drain. Active firmware translation is never disabled.
/// A failed route stays unhealthy until teardown; it cannot recover in place.
pub unsafe trait Dma {
    fn map(&mut self, physical: Physical, writable: bool) -> Result<DmaPage, Error>;
    /// Publish a bounded scatter batch. Every established capability, including
    /// a partial prefix on error, must be returned in `output`. An uncertain
    /// publication/fence must make the route unhealthy before returning.
    fn map_pages(
        &mut self,
        physical: &[Physical],
        writable: bool,
        output: &mut [Option<DmaPage>],
    ) -> Result<(), Error> {
        if physical.len() != output.len() || output.iter().any(Option::is_some) {
            return Err(Error::Invalid);
        }
        for (page, out) in physical.iter().zip(output) {
            *out = Some(self.map(*page, writable)?);
        }
        Ok(())
    }
    fn unmap(&mut self, page: &DmaPage) -> Result<(), Error>;
    /// Success proves every leaf detached and drained. On error the caller
    /// retains all capabilities/backing, including any already detached prefix.
    fn unmap_pages(&mut self, pages: &[&DmaPage]) -> Result<(), Error> {
        if pages.is_empty() || pages.len() > 32 {
            return Err(Error::Invalid);
        }
        for page in pages {
            self.unmap(page)?;
        }
        Ok(())
    }
    fn healthy(&self) -> bool;
}
