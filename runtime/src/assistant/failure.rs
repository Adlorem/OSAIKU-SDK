//! Content-free failure observation. The kernel authenticates the reporting window.
use super::{op, Status};
pub const MAGIC: u64 = u64::from_le_bytes(*b"OSAIAFL1");
pub const BYTES: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub session: u64,
    pub request: u64,
    pub window: u64,
    pub incarnation: u64,
    pub status: Status,
    pub elapsed_ms: u64,
    pub operation: u64,
}
impl Record {
    pub fn valid(self) -> bool {
        self.session != 0
            && self.request != 0
            && self.window != 0
            && self.incarnation != 0
            && (self.status == Status::StartupTimeout && self.operation == op::CONNECTION
                || matches!(
                    self.status,
                    Status::RequestTimeout
                        | Status::ServiceTimeout
                        | Status::ClockFailure
                        | Status::NoProgress
                ) && (((op::NEW..=op::DIAGNOSTICS).contains(&self.operation) || self.operation == op::OBSERVE)
                && self.operation != op::REPLY
                // A clock fault can be detected while an established session is idle.
                || self.operation == 0 && self.status == Status::ClockFailure))
    }
    pub fn encode(self) -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        for (out, word) in bytes.chunks_exact_mut(8).zip([
            MAGIC,
            self.session,
            self.request,
            self.window,
            self.incarnation,
            self.status as u64,
            self.elapsed_ms,
            self.operation,
        ]) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != BYTES {
            return None;
        }
        let w = |i: usize| u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        if w(0) != MAGIC {
            return None;
        }
        let r = Self {
            session: w(1),
            request: w(2),
            window: w(3),
            incarnation: w(4),
            status: match w(5) {
                12 => Status::RequestTimeout,
                13 => Status::ServiceTimeout,
                14 => Status::ClockFailure,
                15 => Status::NoProgress,
                17 => Status::StartupTimeout,
                _ => return None,
            },
            elapsed_ms: w(6),
            operation: w(7),
        };
        r.valid().then_some(r)
    }
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    pub fn submit(self) -> isize {
        if !self.valid() {
            return crate::abi::SYSCALL_ERR_INVALID_BUFFER;
        }
        let bytes = self.encode();
        // SAFETY: initialized fixed-size buffer remains live during the syscall.
        unsafe {
            crate::raw::syscall(
                crate::abi::number::LOG_WRITE,
                [bytes.as_ptr() as u64, bytes.len() as u64, 0, 0, 0],
            ) as isize
        }
    }
}
