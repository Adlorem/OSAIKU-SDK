//! NVMe-specific register/arena filter over a platform-admitted DRV-11 domain.
use crate::{controller_io::Platform, nvme_process as wire, process::Message, protocol::Error};
pub const DMA_BYTES: u64 = crate::nvme::DMA_PAGES * 4096;
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
            _ => return Err(Error::Permission),
        }
        Ok(m)
    }
    pub fn revoke(&mut self) -> bool {
        self.closed = true;
        self.platform.revoke()
    }
}
