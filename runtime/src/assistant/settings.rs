//! Versioned integer-only generation configuration. Each request owns a copy.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sampling {
    pub version: u32,
    pub temperature: u32,
    pub top_p: u32,
    pub top_k: u32,
    pub presence: u32,
    pub repeat: u32,
    pub seed: u32,
    pub reserved: u32,
}
impl Sampling {
    pub const DEFAULT: Self = Self {
        version: 1,
        temperature: 700,
        top_p: 800,
        top_k: 20,
        presence: 1500,
        repeat: 1000,
        seed: 42,
        reserved: 0,
    };
    pub fn valid(self) -> bool {
        self.version == 1
            && self.temperature >= 100
            && self.temperature <= 2000
            && self.top_p >= 100
            && self.top_p <= 1000
            && self.top_k >= 1
            && self.top_k <= 100
            && self.presence <= 2000
            && self.repeat >= 1000
            && self.repeat <= 2000
            && self.seed != u32::MAX
            && self.reserved == 0
    }
    pub fn encode(self) -> [u8; 32] {
        let mut b = [0; 32];
        for (slot, v) in b.chunks_exact_mut(4).zip([
            self.version,
            self.temperature,
            self.top_p,
            self.top_k,
            self.presence,
            self.repeat,
            self.seed,
            self.reserved,
        ]) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != 32 {
            return None;
        }
        let n = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let s = Self {
            version: n(0),
            temperature: n(4),
            top_p: n(8),
            top_k: n(12),
            presence: n(16),
            repeat: n(20),
            seed: n(24),
            reserved: n(28),
        };
        s.valid().then_some(s)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub sampling: Sampling,
    pub context: u32,
}
impl Preferences {
    pub const DEFAULT: Self = Self {
        sampling: Sampling::DEFAULT,
        context: 2048,
    };
    pub fn encode(self) -> [u8; 40] {
        let mut b = [0; 40];
        b[..32].copy_from_slice(&self.sampling.encode());
        b[32..36].copy_from_slice(&self.context.to_le_bytes());
        b[36..].copy_from_slice(&1u32.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != 40 || b[36..] != 1u32.to_le_bytes() {
            return None;
        }
        let context = u32::from_le_bytes(b[32..36].try_into().ok()?);
        if !matches!(context, 2048 | 4096) {
            return None;
        }
        Some(Self {
            sampling: Sampling::decode(&b[..32])?,
            context,
        })
    }
}
