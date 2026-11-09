//! Trusted backend queue observations. No addresses, packet data or MACs.
use crate::Error;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counters {
    /// 0 down, 1 up, 2 not yet observed. Speed 0 means unknown/inapplicable.
    pub link: u64,
    pub speed_mbps: u64,
    /// 0 unknown/inapplicable, 1 half, 2 full.
    pub duplex: u64,
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub rx_errors: u64,
    pub tx_submitted: u64,
    pub tx_bytes: u64,
    pub tx_completed: u64,
    pub tx_completed_bytes: u64,
    pub tx_errors: u64,
    pub tx_busy: u64,
    pub tx_pending: u64,
    pub registers: LinkRegisters,
}
/// Versioned fixed register snapshot. Missing values must be zero and are not
/// interpreted as link down. Numeric error codes are independent of Rust enums.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkRegisters {
    /// 0 unavailable, 1 complete, 2 failed (possibly partial).
    pub state: u32,
    /// 0 none, 1 IO, 2 timeout, 3 removed, 4 semaphore, 5 PHY, 6 state,
    /// 7 range, 8 firmware/ownership.
    pub error: u32,
    pub valid: u32,
    pub elapsed_ms: u32,
    /// CTRL, STATUS, CTRL_EXT, PHY_CTRL.
    pub mac: [u32; 4],
    /// BMCR, BMSR first/second, ADV, LPA, CTRL1000, STAT1000, STATUS2,
    /// HV_OEM_BITS, CV_SMB_CTRL.
    pub phy: [u16; 10],
}
impl LinkRegisters {
    pub fn validate(self) -> Result<(), Error> {
        if self.valid & !0x3fff != 0
            || match self.state {
                0 => self != Self::default(),
                1 => self.error != 0 || self.valid != 0x3fff,
                2 => !(1..=8).contains(&self.error),
                _ => true,
            }
        {
            return Err(Error::Range);
        }
        for (i, value) in self
            .mac
            .into_iter()
            .chain(self.phy.map(u32::from))
            .enumerate()
        {
            if self.valid & (1 << i) == 0 && value != 0 {
                return Err(Error::Reserved);
            }
        }
        Ok(())
    }
    /// All-ones MDIO readback is retained raw but not presented as link UP.
    pub fn bit(self, slot: usize, mask: u32) -> &'static str {
        let value = match slot {
            0..=3 => self.mac[slot],
            4..=13 => u32::from(self.phy[slot - 4]),
            _ => return "unknown",
        };
        if self.valid & (1 << slot) == 0 || value == if slot < 4 { u32::MAX } else { 0xffff } {
            "unknown"
        } else if value & mask != 0 {
            "1"
        } else {
            "0"
        }
    }
}
impl Counters {
    fn words(self) -> [u64; 13] {
        [
            self.link,
            self.speed_mbps,
            self.duplex,
            self.rx_packets,
            self.rx_bytes,
            self.rx_errors,
            self.tx_submitted,
            self.tx_bytes,
            self.tx_completed,
            self.tx_completed_bytes,
            self.tx_errors,
            self.tx_busy,
            self.tx_pending,
        ]
    }
    pub fn validate(self) -> Result<(), Error> {
        if self.link > 2
            || !matches!(self.speed_mbps, 0 | 10 | 100 | 1000)
            || self.duplex > 2
            || (self.link != 1 && (self.speed_mbps != 0 || self.duplex != 0))
            || self.tx_completed > self.tx_submitted
            || self.tx_completed_bytes > self.tx_bytes
            || self.tx_pending > 63
        {
            return Err(Error::Range);
        }
        self.registers.validate()
    }
    pub fn encode(self) -> Result<[u8; 384], Error> {
        self.validate()?;
        let mut out = [0; 384];
        out[..8].copy_from_slice(b"ONETD002");
        for (i, n) in self.words().iter().enumerate() {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        let r = self.registers;
        for (i, n) in [r.state, r.error, r.valid, r.elapsed_ms]
            .into_iter()
            .chain(r.mac)
            .enumerate()
        {
            out[112 + i * 4..116 + i * 4].copy_from_slice(&n.to_le_bytes());
        }
        for (i, n) in r.phy.into_iter().enumerate() {
            out[144 + i * 2..146 + i * 2].copy_from_slice(&n.to_le_bytes());
        }
        Ok(out)
    }
    pub fn decode(data: &[u8; 384]) -> Result<Self, Error> {
        let old = &data[..8] == b"ONETD001";
        if !old && &data[..8] != b"ONETD002" {
            return Err(Error::Version);
        }
        if data[if old { 112 } else { 164 }..].iter().any(|b| *b != 0) {
            return Err(Error::Reserved);
        }
        let n = |i: usize| u64::from_le_bytes(data[8 + i * 8..16 + i * 8].try_into().unwrap());
        let word =
            |i: usize| u32::from_le_bytes(data[112 + i * 4..116 + i * 4].try_into().unwrap());
        let registers = if old {
            LinkRegisters::default()
        } else {
            LinkRegisters {
                state: word(0),
                error: word(1),
                valid: word(2),
                elapsed_ms: word(3),
                mac: core::array::from_fn(|i| word(4 + i)),
                phy: core::array::from_fn(|i| {
                    u16::from_le_bytes(data[144 + i * 2..146 + i * 2].try_into().unwrap())
                }),
            }
        };
        let out = Self {
            link: n(0),
            speed_mbps: n(1),
            duplex: n(2),
            rx_packets: n(3),
            rx_bytes: n(4),
            rx_errors: n(5),
            tx_submitted: n(6),
            tx_bytes: n(7),
            tx_completed: n(8),
            tx_completed_bytes: n(9),
            tx_errors: n(10),
            tx_busy: n(11),
            tx_pending: n(12),
            registers,
        };
        out.validate()?;
        Ok(out)
    }
    pub fn follows(self, old: Self) -> bool {
        self.words()[3..12]
            .iter()
            .zip(&old.words()[3..12])
            .all(|(a, b)| a >= b)
    }
}
