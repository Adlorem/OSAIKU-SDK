//! Versioned, process-local thread ABI. IDs confer no IPC or process authority.
pub const VERSION: u64 = 1;
/// Ordinary applications retain two charged workers; modeld admits eight.
pub const DEFAULT_WORKERS: usize = 2;
pub const MAX_WORKERS: usize = 8;
pub const ORDINARY_TASKS: usize = MAX_WORKERS + 1;
pub const fn ordinary_selection(index: usize) -> usize {
    if index == 0 {
        0
    } else {
        index + 1
    }
}
pub const TLS_BYTES: usize = 64;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Create {
    pub version: u64,
    pub entry: u64,
    pub argument: u64,
    pub stack_base: u64,
    pub stack_bytes: u64,
    pub tls: u64,
}
impl Create {
    pub fn valid(&self) -> bool {
        self.version == VERSION
            && self.entry != 0
            && self.entry < (1 << 47)
            && self.stack_base % 16 == 0
            && (8192..=1024 * 1024).contains(&self.stack_bytes)
            && self.stack_bytes % 16 == 0
            && self.tls != 0
            && self.tls % 16 == 0
            && self
                .stack_base
                .checked_add(self.stack_bytes)
                .is_some_and(|end| {
                    end < (1 << 47)
                        && (self.tls.saturating_add(TLS_BYTES as u64) <= self.stack_base
                            || self.tls >= end)
                })
            && self
                .tls
                .checked_add(TLS_BYTES as u64)
                .is_some_and(|end| end < (1 << 47))
    }
}
