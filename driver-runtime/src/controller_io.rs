//! Trusted mediation of one admitted controller. Peers only name offsets in
//! retained resources. No PCI, IRQ destination or allocator interface exists.
use crate::{controller as wire, process::Message, protocol::Error};
pub const DMA_BYTES: u64 = 344064;
pub trait Platform {
    fn read_register(&mut self, offset: u32) -> Result<u32, Error>;
    fn write_register(&mut self, offset: u32, value: u32) -> Result<(), Error>;
    fn read_arena(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Error>;
    fn write_arena(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Error>;
    fn fence(&mut self) -> Result<(), Error>;
    fn now(&mut self) -> u64;
    fn zero_arena(&mut self, offset: u64, bytes: usize) -> Result<(), Error> {
        for at in (0..bytes).step_by(256) {
            self.write_arena(offset + at as u64, &[0; 256][..(bytes - at).min(256)])?;
        }
        Ok(())
    }
    /// Only return true after stopping the device, DMA fences and IRQ join.
    fn revoke(&mut self) -> bool;
    /// Trusted owner only: stopped hardware, joined IRQs and retained DMA.
    fn restart(&mut self, _generation: u64) -> bool {
        false
    }
}
/// Immutable register inventory from the trusted owner. The complete MSI-X
/// table/PBA pages are excluded, even if they alias a requested safe register.
pub struct Registers {
    pub aperture: u32,
    pub operational: u32,
    pub runtime: u32,
    pub doorbells: u32,
    pub ports: u8,
    pub slots: u8,
    pub forbidden: [Option<(u32, u32)>; 2],
}
impl Registers {
    fn allows(&self, offset: u64, write: bool) -> bool {
        let Ok(o) = u32::try_from(offset) else {
            return false;
        };
        if o & 3 != 0
            || o.checked_add(4).is_none_or(|end| end > self.aperture)
            || self
                .forbidden
                .iter()
                .flatten()
                .any(|(start, end)| o & !4095 < *end && *start < (o & !4095).saturating_add(4096))
        {
            return false;
        }
        if !write && o < 32 {
            return true;
        }
        let op = o.checked_sub(self.operational);
        if op.is_some_and(|d| {
            if write {
                matches!(d, 0 | 4 | 0x18 | 0x1c | 0x30 | 0x34 | 0x38)
            } else {
                matches!(d, 0 | 4 | 8 | 0x18 | 0x1c | 0x30 | 0x34 | 0x38)
            }
        }) {
            return true;
        }
        if op.is_some_and(|d| {
            d >= 0x400 && d < 0x400 + u32::from(self.ports) * 16 && (d - 0x400) % 16 == 0
        }) {
            return true;
        }
        if o.checked_sub(self.runtime)
            .is_some_and(|d| matches!(d, 0x20 | 0x24 | 0x28 | 0x30 | 0x34 | 0x38 | 0x3c))
        {
            return true;
        }
        write
            && o.checked_sub(self.doorbells)
                .is_some_and(|d| d <= u32::from(self.slots) * 4)
    }
}
pub struct Gate<P: Platform> {
    platform: P,
    registers: Registers,
    base: u64,
    bytes: u64,
    closed: bool,
}
impl<P: Platform> Gate<P> {
    /// Trusted owner supplies only a DRV-11 admitted, enabled domain.
    pub fn new(platform: P, registers: Registers, base: u64, bytes: u64) -> Result<Self, Error> {
        if base == 0
            || base & 4095 != 0
            || bytes < DMA_BYTES
            || base.checked_add(bytes).is_none()
            || registers.aperture > 65536
            || registers.ports == 0
            || registers.slots == 0
            || registers.operational < 32
            || registers.runtime >= registers.aperture
            || registers.doorbells >= registers.aperture
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
            bytes,
            closed: false,
        })
    }
    pub fn exchange(&mut self, mut m: Message) -> Result<Message, Error> {
        if self.closed {
            return Err(Error::Stale);
        }
        if m.operation != wire::IO || m.offset != 0 || m.length > 256 {
            return Err(Error::Range);
        }
        match m.sequence {
            wire::ZERO_ARENA => {
                let offset = m.value.checked_sub(self.base).ok_or(Error::Range)?;
                let bytes = u64::from_le_bytes(m.payload[..8].try_into().unwrap());
                if m.length != 8
                    || !(1..=4096).contains(&bytes)
                    || m.payload[8..] != [0; 248]
                    || offset.checked_add(bytes).is_none_or(|n| n > self.bytes)
                {
                    return Err(Error::Range);
                }
                self.platform.zero_arena(offset, bytes as usize)?;
                m.length = 0;
                m.payload.fill(0);
            }
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
                    )?;
                }
                m.payload.fill(0);
            }
            wire::READ_ARENA | wire::WRITE_ARENA => {
                let offset = m.value.checked_sub(self.base).ok_or(Error::Range)?;
                if offset.checked_add(m.length).is_none_or(|n| n > self.bytes) {
                    return Err(Error::Range);
                }
                if m.sequence == wire::READ_ARENA {
                    m.payload.fill(0);
                    self.platform
                        .read_arena(offset, &mut m.payload[..m.length as usize])?
                } else {
                    self.platform
                        .write_arena(offset, &m.payload[..m.length as usize])?;
                    m.payload.fill(0);
                }
            }
            wire::PUBLISH | wire::ACQUIRE | wire::CLOCK | wire::GRANT => {
                if m.length != 0 || m.value != 0 {
                    return Err(Error::Range);
                }
                m.payload.fill(0);
                match m.sequence {
                    wire::PUBLISH | wire::ACQUIRE => self.platform.fence()?,
                    wire::CLOCK => m.value = self.platform.now(),
                    wire::GRANT => {
                        for (i, n) in [self.base, self.bytes, u64::from(self.registers.aperture)]
                            .into_iter()
                            .enumerate()
                        {
                            m.payload[i * 8..i * 8 + 8].copy_from_slice(&n.to_le_bytes());
                        }
                        m.payload[24..32].copy_from_slice(&wire::FEATURE_ZERO_ARENA.to_le_bytes());
                        m.length = 32;
                    }
                    _ => unreachable!(),
                }
            }
            _ => return Err(Error::Permission),
        }
        Ok(m)
    }
    pub fn revoke(&mut self) -> bool {
        self.closed = true;
        self.platform.revoke()
    }
    /// Not exposed through exchange(): a driver cannot grant itself a restart.
    pub fn restart(&mut self, generation: u64) -> Result<(), Error> {
        if !self.closed || generation == 0 || !self.platform.restart(generation) {
            return Err(Error::State);
        }
        self.closed = false;
        Ok(())
    }
}
