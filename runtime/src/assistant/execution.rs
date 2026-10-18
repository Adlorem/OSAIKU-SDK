//! V1 execution policy encoding. Interactive work has no elapsed-time budget;
//! transport, progress, receiver and cleanup watches remain separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Until(u64),
    Interactive,
}
pub const VERSION: u64 = 1;
const INTERACTIVE: u64 = 1 << 63;
impl Policy {
    pub const fn encode(self) -> u64 {
        match self {
            Self::Until(ms) => ms,
            Self::Interactive => INTERACTIVE,
        }
    }
    pub fn decode(value: u64) -> Option<Self> {
        match value {
            INTERACTIVE => Some(Self::Interactive),
            1..INTERACTIVE => Some(Self::Until(value)),
            _ => None,
        }
    }
    pub fn expired(self, now: u64) -> bool {
        matches!(self, Self::Until(deadline) if now >= deadline)
    }
    pub fn admitted(self, now: u64, maximum: u64) -> bool {
        match self {
            Self::Interactive => now < INTERACTIVE,
            Self::Until(deadline) => deadline
                .checked_sub(now)
                .is_some_and(|n| n > 0 && n <= maximum),
        }
    }
    pub fn micros(self) -> Option<Self> {
        match self {
            Self::Interactive => Some(Self::Interactive),
            Self::Until(ms) => ms
                .checked_mul(1000)
                .filter(|us| *us < INTERACTIVE)
                .map(Self::Until),
        }
    }
    pub fn cap(self, deadline: u64) -> u64 {
        match self {
            Self::Interactive => deadline,
            Self::Until(end) => deadline.min(end),
        }
    }
}
