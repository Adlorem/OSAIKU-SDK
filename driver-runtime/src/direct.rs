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
    dma: u64,
    copy: u64,
    iova: u64,
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
        let profile = Profile::decode(&receipt.data)?;
        let slot = (receipt.offset - MMIO_START) / 0x100000;
        if profile.service_dma && slot >= DMA_WINDOW_BYTES / SERVICE_SLOT_BYTES {
            return Err(Error::Range);
        }
        if profile.service_dma
            && (receipt.sequence != 0 || receipt.endpoint != 0)
            && (receipt.sequence != DMA_START + slot * SERVICE_SLOT_BYTES
                || receipt.endpoint != receipt.sequence + SERVICE_COPY_OFFSET
                || receipt.length == 0
                || receipt.length & (PAGE - 1) != 0
                || receipt
                    .length
                    .checked_add(profile.dma_pages * PAGE)
                    .is_none())
        {
            return Err(Error::Range);
        }
        if !profile.service_dma && (receipt.sequence != 0 || receipt.endpoint != 0) {
            return Err(Error::Reserved);
        }
        Ok(Self {
            handle: receipt.handle,
            profile,
            base: receipt.offset,
            dma: receipt.sequence,
            copy: receipt.endpoint,
            iova: receipt.length,
        })
    }
    pub fn handle(&self) -> Handle {
        self.handle
    }
    pub fn dma_bytes(&self) -> u64 { self.profile.dma_pages * PAGE }
    fn bytes(&self, dma: bool, offset: usize, bytes: usize) -> Result<u64, Error> {
        let base = if dma { self.dma } else { self.copy };
        let limit = if dma {
            self.profile.dma_pages
        } else {
            COPY_PAGES
        } * PAGE;
        if !self.profile.service_dma
            || base == 0
            || offset
                .checked_add(bytes)
                .is_none_or(|end| end as u64 > limit)
        {
            return Err(Error::Permission);
        }
        Ok(base + offset as u64)
    }
    pub fn read_dma(&self, offset: usize, out: &mut [u8]) -> Result<(), Error> {
        let at = self.bytes(true, offset, out.len())?;
        fence(Ordering::Acquire);
        for (i, b) in out.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile((at + i as u64) as *const u8) };
        }
        Ok(())
    }
    pub fn write_dma(&mut self, offset: usize, data: &[u8]) -> Result<(), Error> {
        let at = self.bytes(true, offset, data.len())?;
        for (i, b) in data.iter().enumerate() {
            unsafe { core::ptr::write_volatile((at + i as u64) as *mut u8, *b) };
        }
        fence(Ordering::Release);
        Ok(())
    }
    pub fn dma_iova(&self, page: u64) -> Result<u64, Error> {
        self.bytes(
            true,
            usize::try_from(page.checked_mul(PAGE).ok_or(Error::Range)?)
                .map_err(|_| Error::Range)?,
            PAGE as usize,
        )?;
        self.iova.checked_add(page * PAGE).ok_or(Error::Range)
    }
    /// Copy one untrusted TX frame into private service memory before parsing.
    pub fn read_copy_tx(&self, out: &mut [u8]) -> Result<(), Error> {
        if !(14..=1518).contains(&out.len()) {
            return Err(Error::Range);
        }
        let at = self.bytes(false, 0, out.len())?;
        fence(Ordering::Acquire);
        for (i, b) in out.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile((at + i as u64) as *const u8) };
        }
        Ok(())
    }
    pub fn write_copy_rx(&mut self, data: &[u8]) -> Result<(), Error> {
        if !(14..=1518).contains(&data.len()) {
            return Err(Error::Range);
        }
        let at = self.bytes(false, PAGE as usize, data.len())?;
        for (i, b) in data.iter().enumerate() {
            unsafe { core::ptr::write_volatile((at + i as u64) as *mut u8, *b) };
        }
        fence(Ordering::Release);
        Ok(())
    }
    fn address(&self, offset: u64, width: u64) -> Result<u64, Error> {
        let end = offset.checked_add(width).ok_or(Error::Range)?;
        if offset % width != 0
            || !self.profile.regions[..self.profile.count].iter().any(|r| {
                r.kind == PageKind::Mediated
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
        if !matches!(width, 1 | 4 | 8) || (width == 1 && value > u8::MAX as u64) || (width == 4 && value > u32::MAX as u64) {
            return Err(Error::Size);
        }
        let at = self.address(offset, width)?;
        fence(Ordering::SeqCst);
        unsafe {
            if width == 1 {
                core::ptr::write_volatile(at as *mut u8, value as u8);
            } else if width == 4 {
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
            || receipt.length
                != if profile.service_dma {
                    COPY_PAGES * PAGE
                } else {
                    profile.dma_pages * PAGE
                }
            || receipt.sequence == 0
        {
            let mut revoke = Message::new(op::REVOKE);
            revoke.handle = identity.handle;
            let _ = exchange(&revoke);
            return Err(Error::State);
        }
        let iova = u64::from_le_bytes(receipt.data[..8].try_into().unwrap());
        if (profile.service_dma && iova != 0)
            || (!profile.service_dma
                && (iova == 0
                    || iova & (PAGE - 1) != 0
                    || iova.checked_add(receipt.length).is_none()))
        {
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
                r.kind == PageKind::Direct
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
    pub fn read8(&self, offset: u64) -> Result<u8, Error> {
        let at = self.register(offset, 1)?;
        let value = unsafe { core::ptr::read_volatile(at as *const u8) };
        fence(Ordering::Acquire);
        Ok(value)
    }
    pub fn read16(&self, offset: u64) -> Result<u16, Error> {
        let at = self.register(offset, 2)?;
        let value = unsafe { core::ptr::read_volatile(at as *const u16) };
        fence(Ordering::Acquire);
        Ok(u16::from_le(value))
    }
    pub fn write8(&mut self, offset: u64, value: u8) -> Result<(), Error> {
        let at = self.register(offset, 1)?;
        fence(Ordering::SeqCst);
        unsafe {
            core::ptr::write_volatile(at as *mut u8, value);
        }
        Ok(())
    }
    pub fn write16(&mut self, offset: u64, value: u16) -> Result<(), Error> {
        let at = self.register(offset, 2)?;
        fence(Ordering::SeqCst);
        unsafe {
            core::ptr::write_volatile(at as *mut u16, value.to_le());
        }
        Ok(())
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
        if self.profile.service_dma {
            return Err(Error::Permission);
        }
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
        self.read_rx_at(page, 0, output)
    }
    pub fn copy_frame_tx(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.closed || !self.mapped {
            return Err(Error::Stale);
        }
        if !self.profile.service_dma || !(14..=1518).contains(&data.len()) {
            return Err(Error::Permission);
        }
        for (i, b) in data.iter().enumerate() {
            unsafe { core::ptr::write_volatile((DMA_START + i as u64) as *mut u8, *b) };
        }
        fence(Ordering::Release);
        Ok(())
    }
    pub fn copy_frame_rx(&self, out: &mut [u8]) -> Result<(), Error> {
        if self.closed || !self.mapped {
            return Err(Error::Stale);
        }
        if !self.profile.service_dma || !(14..=1518).contains(&out.len()) {
            return Err(Error::Permission);
        }
        fence(Ordering::Acquire);
        for (i, b) in out.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile((DMA_START + PAGE + i as u64) as *const u8) };
        }
        Ok(())
    }
    pub fn read_rx_at(&self, page: u64, offset: usize, output: &mut [u8]) -> Result<(), Error> {
        self.dma_iova(page)?;
        if page == 0
            || page > (self.profile.dma_pages - 1) / 2
            || offset
                .checked_add(output.len())
                .is_none_or(|end| end > PAGE as usize)
        {
            return Err(Error::Range);
        }
        fence(Ordering::Acquire);
        for (i, byte) in output.iter_mut().enumerate() {
            *byte = unsafe {
                core::ptr::read_volatile(
                    (DMA_START + page * PAGE + (offset + i) as u64) as *const u8,
                )
            };
        }
        Ok(())
    }
    pub fn copy_tx(&mut self, page: u64, input: &[u8]) -> Result<(), Error> {
        self.write_tx_at(page, 0, input)
    }
    pub fn write_tx_at(&mut self, page: u64, offset: usize, input: &[u8]) -> Result<(), Error> {
        self.dma_iova(page)?;
        if page <= (self.profile.dma_pages - 1) / 2
            || offset
                .checked_add(input.len())
                .is_none_or(|end| end > PAGE as usize)
        {
            return Err(Error::Range);
        }
        for (i, byte) in input.iter().enumerate() {
            unsafe {
                core::ptr::write_volatile(
                    (DMA_START + page * PAGE + (offset + i) as u64) as *mut u8,
                    *byte,
                );
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
