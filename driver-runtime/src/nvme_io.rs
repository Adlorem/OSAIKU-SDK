//! NVMe-specific register/arena filter over a platform-admitted DRV-11 domain.
use crate::{controller_io::Platform, nvme_process as wire, process::Message, protocol::Error};
pub const DMA_BYTES: u64 = crate::nvme::DMA_PAGES * 4096;
/// One synchronous trusted-owner copy, serialized against revoke/restart.
/// Does not enlarge the DMA arena, a process message, or the controller limit.
pub const OWNER_COPY_BYTES: usize = 64 * 1024;
pub struct Registers {
    pub aperture: u32,
    pub stride: u32,
    pub forbidden: [Option<(u32, u32)>; 2],
}
impl Registers {
    fn allows(&self, o: u64, write: bool) -> bool {
        let Ok(o) = u32::try_from(o) else {
            return false;
        };
        if o & 3 != 0
            || o.checked_add(4).is_none_or(|n| n > self.aperture)
            || self
                .forbidden
                .iter()
                .flatten()
                .any(|(a, b)| (o & !4095) < *b && *a < (o & !4095).saturating_add(4096))
        {
            return false;
        }
        if write {
            matches!(o, 0xc | 0x10 | 0x14 | 0x24 | 0x28 | 0x2c | 0x30 | 0x34)
                || (0..4).any(|i| o == 0x1000 + i * self.stride)
        } else {
            matches!(o, 0 | 4 | 8 | 0x14 | 0x1c)
        }
    }
}
pub struct Gate<P: Platform> {
    platform: P,
    registers: Registers,
    base: u64,
    mask: u64,
    namespace: u32,
    closed: bool,
}
impl<P: Platform> Gate<P> {
    /// Typed kernel-owner operations. Retain the same aperture exclusions and
    /// closed-state checks as the process ABI without serializing a message.
    pub fn read_register(&mut self, offset: u32) -> Result<u32, Error> {
        if self.closed || !self.registers.allows(u64::from(offset), false) { return Err(Error::Permission); }
        self.platform.read_register(offset)
    }
    pub fn write_register(&mut self, offset: u32, value: u32) -> Result<(), Error> {
        if self.closed || !self.registers.allows(u64::from(offset), true) { return Err(Error::Permission); }
        self.platform.write_register(offset, value)
    }
    pub fn fence(&mut self) -> Result<(), Error> {
        if self.closed { return Err(Error::Stale); }
        self.platform.fence()
    }
    pub fn now(&mut self) -> u64 { self.platform.now() }
    /// Trusted owner only, serialized with revoke/restart. Each call still
    /// enters Platform's current-domain check; exchange keeps its 256-byte ABI.
    pub fn owner_read(&mut self, address: u64, bytes: &mut [u8]) -> Result<(), Error> {
        let offset = self.owner_range(address, bytes.len())?;
        self.platform.read_arena(offset, bytes)
    }
    pub fn owner_write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Error> {
        let offset = self.owner_range(address, bytes.len())?;
        self.platform.write_arena(offset, bytes)
    }
    fn owner_range(&self, address: u64, length: usize) -> Result<u64, Error> {
        if self.closed {
            return Err(Error::Stale);
        }
        let offset = address.checked_sub(self.base).ok_or(Error::Range)?;
        if length == 0 || length > OWNER_COPY_BYTES
            || offset.checked_add(length as u64).is_none_or(|end| end > DMA_BYTES)
        {
            return Err(Error::Range);
        }
        Ok(offset)
    }
    pub fn new(
        platform: P,
        registers: Registers,
        base: u64,
        bytes: u64,
        mask: u64,
        namespace: u32,
    ) -> Result<Self, Error> {
        if namespace == 0
            || base == 0
            || base & 4095 != 0
            || bytes != DMA_BYTES
            || base.checked_add(bytes - 1).is_none_or(|n| n > mask)
            || !matches!(registers.stride, 4 | 8 | 16 | 32 | 64)
            || registers.aperture < 0x1000 + 4 * registers.stride
            || registers.aperture > 65536
            || registers
                .forbidden
                .iter()
                .flatten()
                .any(|(a, b)| a >= b || *b > registers.aperture)
        {
            return Err(Error::Range);
        }
        Ok(Self {
            platform,
            registers,
            base,
            mask,
            namespace,
            closed: false,
        })
    }
    pub fn exchange(&mut self, mut m: Message) -> Result<Message, Error> {
        if self.closed {
            return Err(Error::Stale);
        }
        if m.endpoint != wire::ENDPOINT
            || m.operation != wire::IO
            || m.offset != 0
            || m.length > 256
        {
            return Err(Error::Range);
        }
        match m.sequence {
            wire::READ_REGISTER | wire::WRITE_REGISTER => {
                if m.length != 4
                    || !self
                        .registers
                        .allows(m.value, m.sequence == wire::WRITE_REGISTER)
                {
                    return Err(Error::Permission);
                }
                if m.sequence == wire::READ_REGISTER {
                    m.value = u64::from(self.platform.read_register(m.value as u32)?)
                } else {
                    self.platform.write_register(
                        m.value as u32,
                        u32::from_le_bytes(m.payload[..4].try_into().unwrap()),
                    )?
                }
                m.payload.fill(0);
            }
            wire::READ_ARENA | wire::WRITE_ARENA => {
                let o = m.value.checked_sub(self.base).ok_or(Error::Range)?;
                if m.length == 0 || o.checked_add(m.length).is_none_or(|n| n > DMA_BYTES) {
                    return Err(Error::Range);
                }
                if m.sequence == wire::READ_ARENA {
                    m.payload.fill(0);
                    self.platform
                        .read_arena(o, &mut m.payload[..m.length as usize])?
                } else {
                    self.platform
                        .write_arena(o, &m.payload[..m.length as usize])?;
                    m.payload.fill(0)
                }
            }
            wire::PUBLISH | wire::ACQUIRE | wire::CLOCK | wire::GRANT => {
                if m.length != 0 || m.value != 0 {
                    return Err(Error::Range);
                }
                m.payload.fill(0);
                match m.sequence {
                    wire::CLOCK => m.value = self.platform.now(),
                    wire::GRANT => {
                        for (i, n) in [self.base, DMA_BYTES, self.mask, u64::from(self.namespace)]
                            .into_iter()
                            .enumerate()
                        {
                            m.payload[i * 8..i * 8 + 8].copy_from_slice(&n.to_le_bytes());
                        }
                        m.length = 32;
                    }
                    _ => self.platform.fence()?,
                }
            }
            wire::SUBMIT => {
                // Same authority as WRITE_ARENA into a submission ring page,
                // PUBLISH and WRITE_REGISTER of that queue's tail doorbell.
                let entries = (m.length / 64) as usize;
                if m.length % 64 != 0 {
                    return Err(Error::Range);
                }
                let (queue, tail, slots) = wire::parse_submit(m.value, entries)?;
                let doorbell = self.doorbell(queue, false)?;
                let ring = crate::nvme::dma_region(usize::from(queue) * 2)
                    .map_err(|_| Error::Range)?
                    .0;
                for (i, slot) in slots[..entries].iter().enumerate() {
                    self.platform
                        .write_arena(ring + u64::from(*slot) * 64, &m.payload[i * 64..i * 64 + 64])?;
                }
                self.platform.fence()?;
                self.platform.write_register(doorbell, u32::from(tail))?;
                m.payload.fill(0);
                m.length = 0;
            }
            wire::REAP => {
                // Same authority as READ_ARENA of a completion ring page, a
                // CSTS read and WRITE_REGISTER of that queue's head doorbell.
                // The driver still validates every copied entry itself.
                if m.length != 0 || !self.registers.allows(0x1c, false) {
                    return Err(Error::Range);
                }
                let (queue, head, phase, entries, max) = wire::parse_reap(m.value)?;
                let doorbell = self.doorbell(queue, true)?;
                let ring = crate::nvme::dma_region(usize::from(queue) * 2 + 1)
                    .map_err(|_| Error::Range)?
                    .0;
                m.payload.fill(0);
                let (mut index, mut expected, mut count) = (head, phase, 0);
                while count < max {
                    let at = ring + u64::from(index) * 16;
                    // Dword 3 carries the phase tag: read the others after it.
                    let mut status = [0; 4];
                    self.platform.read_arena(at + 12, &mut status)?;
                    if ((u32::from_le_bytes(status) >> 16) & 1 != 0) != expected {
                        break;
                    }
                    // Ownership must be acquired before copying CQ contents.
                    self.platform.fence()?;
                    self.platform
                        .read_arena(at, &mut m.payload[count * 16..count * 16 + 16])?;
                    count += 1;
                    index += 1;
                    if index == entries {
                        index = 0;
                        expected = !expected;
                    }
                }
                m.offset = 0;
                if count == 0 {
                    m.offset = u64::from(self.platform.read_register(0x1c)?);
                } else {
                    self.platform.fence()?;
                    self.platform.write_register(doorbell, u32::from(index))?;
                }
                m.length = (count * 16) as u64;
                m.value = self.platform.now();
            }
            _ => return Err(Error::Permission),
        }
        Ok(m)
    }
    fn doorbell(&self, queue: u16, completion: bool) -> Result<u32, Error> {
        let offset = 0x1000 + (2 * u32::from(queue) + u32::from(completion)) * self.registers.stride;
        if !self.registers.allows(u64::from(offset), true) {
            return Err(Error::Permission);
        }
        Ok(offset)
    }
    /// Trusted lifecycle owner only; never dispatched from a process message.
    pub fn restart(&mut self, generation: u64) -> Result<(), Error> {
        if !self.closed || generation == 0 || !self.platform.restart(generation) {
            return Err(Error::State);
        }
        self.closed = false;
        Ok(())
    }
    pub fn revoke(&mut self) -> bool {
        self.closed = true;
        self.platform.revoke()
    }
}
