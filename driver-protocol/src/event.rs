//! Generation-bound, two-party, coalescing event. last_seen is the value from
//! the previous wait, never a fresh sample taken immediately before parking.
pub const SYSCALL: u64 = 405;
pub const CREATE: u64 = 1;
pub const NOTIFY: u64 = 2;
pub const WAIT: u64 = 3;
pub const REVOKE: u64 = 4;
pub const MAX_WAIT_MS: u64 = 30_000;
