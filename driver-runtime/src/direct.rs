//! Process-side DirectDevice v2. CPU pointers and IOVAs are different types.
//! Dropping a value never claims hardware fencing or frees DMA backing.
use crate::protocol::{direct::*, Error};
use core::sync::atomic::{fence, Ordering};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaAddress(u64);
impl DmaAddress {
    pub fn value(self) -> u64 {
        self.0
    }
}

pub struct DirectDevice {
    handle: Handle,
    profile: Profile,
    event: u64,
    iova: u64,
    mapped: bool,
    closed: bool,
}
/// Policy-service-only mapping. Its addresses never enter a driver response.
pub struct HardwareMapping {
    handle: Handle,
    profile: Profile,
    base: u64,
}
impl HardwareMapping {
    /// # Safety
    /// `receipt` must be this hardware service's authenticated MAP response;
    /// the owner must stop using the value when its assignment is revoked.
    pub unsafe fn mapped(receipt: Message) -> Result<Self, Error> {
        if receipt.operation != op::MAP
            || !receipt.handle.valid()
            || receipt.offset < MMIO_START
            || receipt.offset >= MMIO_START + WINDOW_BYTES
            || receipt.offset & 0xfffff != 0
        {
            return Err(Error::Range);
        }
        Ok(Self {
            handle: receipt.handle,
            profile: Profile::decode(&receipt.data)?,
            base: receipt.offset,
        })
    }
    pub fn handle(&self) -> Handle {
        self.handle
    }
    fn address(&self, offset: u64, width: u64) -> Result<u64, Error> {
        let end = offset.checked_add(width).ok_or(Error::Range)?;
        if offset % width != 0
            || !self.profile.regions[..self.profile.count].iter().any(|r| {
                r.bar == 0
                    && r.kind == PageKind::Mediated
                    && offset >= u64::from(r.first) * PAGE
                    && r.end().is_ok_and(|e| end <= e * PAGE)
            })
        {
            return Err(Error::Permission);
        }
        self.base
            .checked_add(offset)
            .filter(|a| {
                a.checked_add(width)
                    .is_some_and(|e| e <= MMIO_START + WINDOW_BYTES)
            })
            .ok_or(Error::Range)
    }
    pub fn read(&self, offset: u64, width: u64) -> Result<u64, Error> {
        if !matches!(width, 4 | 8) {
            return Err(Error::Size);
        }
        let at = self.address(offset, width)?;
        let value = unsafe {
            if width == 4 {
                u32::from_le(core::ptr::read_volatile(at as *const u32)) as u64
            } else {
                u64::from_le(core::ptr::read_volatile(at as *const u64))
            }
        };
        fence(Ordering::Acquire);
        Ok(value)
    }
    pub fn write(&mut self, offset: u64, width: u64, value: u64) -> Result<(), Error> {
        if !matches!(width, 4 | 8) || (width == 4 && value > u32::MAX as u64) {
            return Err(Error::Size);
        }
        let at = self.address(offset, width)?;
        fence(Ordering::SeqCst);
        unsafe {
            if width == 4 {
                core::ptr::write_volatile(at as *mut u32, (value as u32).to_le());
            } else {
                core::ptr::write_volatile(at as *mut u64, value.to_le());
            }
        }
        Ok(())
    }
}
impl DirectDevice {
    /// # Safety
    /// `exchange` must be the authenticated kernel Direct syscall. Returned
    /// mappings must belong exclusively to this process and remain valid until
    /// explicit revoke or process termination. Never build this from peer IPC.
    pub unsafe fn connect(
        mut exchange: impl FnMut(&Message) -> Result<Message, Error>,
    ) -> Result<Self, Error> {
        let identity = exchange(&Message::new(op::IDENTITY))?;
        if !identity.handle.valid() || identity.endpoint == 0 {
            return Err(Error::Stale);
        }
        let profile = Profile::decode(&identity.data)?;
        let mut map = Message::new(op::MAP);
        map.handle = identity.handle;
        let receipt = exchange(&map)?;
        if receipt.handle != identity.handle
            || receipt.offset != MMIO_START
            || receipt.value != DMA_START
            || receipt.length != profile.dma_pages * PAGE
            || receipt.sequence == 0
        {
            let mut revoke = Message::new(op::REVOKE);
            revoke.handle = identity.handle;
            let _ = exchange(&revoke);
            return Err(Error::State);
        }
        let iova = u64::from_le_bytes(receipt.data[..8].try_into().unwrap());
        if iova == 0 || iova & (PAGE - 1) != 0 || iova.checked_add(receipt.length).is_none() {
            let mut revoke = Message::new(op::REVOKE);
            revoke.handle = identity.handle;
            let _ = exchange(&revoke);
            return Err(Error::Range);
        }
        Ok(Self {
            handle: identity.handle,
            profile,
            event: receipt.sequence,
            iova,
            mapped: true,
            closed: false,
        })
    }
    pub fn handle(&self) -> Handle {
        self.handle
    }
    pub fn irq_event(&self) -> (u64, u64) {
        (self.event, self.handle.generation)
    }
    pub fn profile(&self) -> Profile {
        self.profile
    }
    pub fn request(&self, operation: u64) -> Result<Message, Error> {
        if self.closed || !self.mapped {
            return Err(Error::Stale);
        }
        let mut m = Message::new(operation);
        m.handle = self.handle;
        Ok(m)
    }
    pub fn close(
        &mut self,
        mut exchange: impl FnMut(&Message) -> Result<Message, Error>,
    ) -> Result<(), Error> {
        let m = self.request(op::REVOKE)?;
        self.closed = true;
        self.mapped = false;
        exchange(&m).map(|_| ())
    }
    pub fn lost(&mut self) {
        self.closed = true;
        self.mapped = false;
    }
    fn register(&self, offset: u64, width: u64) -> Result<u64, Error> {
        if self.closed || !self.mapped {
            return Err(Error::Stale);
        }
        let end = offset.checked_add(width).ok_or(Error::Range)?;
        if offset % width != 0
            || !self.profile.regions[..self.profile.count].iter().any(|r| {
                r.bar == 0
                    && r.kind == PageKind::Direct
                    && offset >= u64::from(r.first) * PAGE
                    && r.end().is_ok_and(|e| end <= e * PAGE)
            })
        {
            return Err(Error::Permission);
        }
        MMIO_START.checked_add(offset).ok_or(Error::Range)
    }
    pub fn read32(&self, offset: u64) -> Result<u32, Error> {
        let address = self.register(offset, 4)?;
        let value = unsafe { core::ptr::read_volatile(address as *const u32) };
        fence(Ordering::Acquire);
        Ok(u32::from_le(value))
    }
    pub fn write32(&mut self, offset: u64, value: u32) -> Result<(), Error> {
        let address = self.register(offset, 4)?;
        fence(Ordering::SeqCst);
        unsafe {
            core::ptr::write_volatile(address as *mut u32, value.to_le());
        }
        Ok(())
    }
    pub fn read64(&self, offset: u64) -> Result<u64, Error> {
        let address = self.register(offset, 8)?;
        let value = unsafe { core::ptr::read_volatile(address as *const u64) };
        fence(Ordering::Acquire);
        Ok(u64::from_le(value))
    }
    pub fn write64(&mut self, offset: u64, value: u64) -> Result<(), Error> {
        let address = self.register(offset, 8)?;
        fence(Ordering::SeqCst);
        unsafe {
            core::ptr::write_volatile(address as *mut u64, value.to_le());
        }
        Ok(())
    }
    /// Page zero holds device descriptors (R/W DMA). Remaining pages are split
    /// into RX (device Write) and TX (device Read). L2 queue metadata is separate.
    pub fn dma_iova(&self, page: u64) -> Result<DmaAddress, Error> {
        if self.closed || !self.mapped {
            return Err(Error::Stale);
        }
        if page >= self.profile.dma_pages {
            return Err(Error::Range);
        }
        self.iova
            .checked_add(page * PAGE)
            .map(DmaAddress)
            .ok_or(Error::Range)
    }
    pub fn read_descriptor(&self, offset: usize, output: &mut [u8]) -> Result<(), Error> {
        self.dma_iova(0)?;
        if offset
            .checked_add(output.len())
            .is_none_or(|end| end > PAGE as usize)
        {
            return Err(Error::Range);
        }
        fence(Ordering::Acquire);
        for (i, byte) in output.iter_mut().enumerate() {
            *byte =
                unsafe { core::ptr::read_volatile((DMA_START + (offset + i) as u64) as *const u8) };
        }
        Ok(())
    }
    pub fn write_descriptor(&mut self, offset: usize, input: &[u8]) -> Result<(), Error> {
        self.dma_iova(0)?;
        if offset
            .checked_add(input.len())
            .is_none_or(|end| end > PAGE as usize)
        {
            return Err(Error::Range);
        }
        for (i, byte) in input.iter().enumerate() {
            unsafe {
                core::ptr::write_volatile((DMA_START + (offset + i) as u64) as *mut u8, *byte);
            }
        }
        fence(Ordering::Release);
        Ok(())
    }
    /// Control tuples are interpreted by the policy service, never the kernel.
    pub fn mediated(&self, operations: &[[u64; 3]]) -> Result<Message, Error> {
        if operations.is_empty() || operations.len() > MAX_BATCH {
            return Err(Error::Size);
        }
        let mut m = self.request(op::MEDIATED_SUBMIT)?;
        m.length = operations.len() as u64;
        for (row, values) in m.data.chunks_exact_mut(24).zip(operations) {
            for (bytes, value) in row.chunks_exact_mut(8).zip(values) {
                bytes.copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(m)
    }
    pub fn mediated_result(&self, sequence: u64) -> Result<Message, Error> {
        if sequence == 0 {
            return Err(Error::Range);
        }
        let mut m = self.request(op::MEDIATED_RESULT)?;
        m.sequence = sequence;
        Ok(m)
    }
    pub fn copy_rx(&self, page: u64, output: &mut [u8]) -> Result<(), Error> {
        self.dma_iova(page)?;
        if page == 0 || page > (self.profile.dma_pages - 1) / 2 || output.len() > PAGE as usize {
            return Err(Error::Range);
        }
        fence(Ordering::Acquire);
        for (i, byte) in output.iter_mut().enumerate() {
            *byte = unsafe {
                core::ptr::read_volatile((DMA_START + page * PAGE + i as u64) as *const u8)
            };
        }
        Ok(())
    }
    pub fn copy_tx(&mut self, page: u64, input: &[u8]) -> Result<(), Error> {
        self.dma_iova(page)?;
        if page <= (self.profile.dma_pages - 1) / 2 || input.len() > PAGE as usize {
            return Err(Error::Range);
        }
        for (i, byte) in input.iter().enumerate() {
            unsafe {
                core::ptr::write_volatile((DMA_START + page * PAGE + i as u64) as *mut u8, *byte);
            }
        }
        fence(Ordering::Release);
        Ok(())
    }
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn connect() -> Result<DirectDevice, Error> {
    unsafe { DirectDevice::connect(|m| crate::native::direct(m).map_err(|_| Error::State)) }
}
