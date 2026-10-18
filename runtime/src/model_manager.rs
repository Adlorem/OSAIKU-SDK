//! Assistant-owned model lifecycle. Copied scalars only; no paths or memory grants.
pub const SYSCALL: u64 = 387;
pub const STATUS: u64 = 0;
pub const NEXT: u64 = 1;
pub const LOAD: u64 = 2;
pub const UNLOAD: u64 = 3;
pub const CANCEL: u64 = 4;
pub const IDLE: u64 = 0;
pub const PENDING: u64 = 1;
pub const COMPLETE: u64 = 2;
pub const CANCELLED: u64 = 3;
pub const UNAVAILABLE: u64 = 4;
pub const INVALID: u64 = 5;
pub const STALE: u64 = 6;
pub const BUSY: u64 = 7;
pub const MEMORY: u64 = 8;
pub const INTEGRITY: u64 = 9;
pub const TIMEOUT: u64 = 10;
pub const FAILED: u64 = 11;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub window: u64,
    pub incarnation: u64,
    pub sequence: u64,
    pub after: u64,
    pub object: u64,
    pub revision: u64,
    pub context: u64,
    pub status: u64,
    /// 1 queued, 2 authentication/read, 3 copying, 4 attaching, 5 retirement.
    pub phase: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
    pub endpoint: u64,
    pub active_object: u64,
    pub active_context: u64,
    pub active_bytes: u64,
    pub package: [u8; 32],
    /// Bounded inert catalog label. Never interpreted as a path or command.
    pub name: [u8; 80],
}
impl Record {
    pub const EMPTY: Self = Self {
        window: 0,
        incarnation: 0,
        sequence: 0,
        after: 0,
        object: 0,
        revision: 0,
        context: 0,
        status: 0,
        phase: 0,
        read_bytes: 0,
        written_bytes: 0,
        endpoint: 0,
        active_object: 0,
        active_context: 0,
        active_bytes: 0,
        package: [0; 32],
        name: [0; 80],
    };
    pub fn valid_request(self, operation: u64) -> bool {
        self.window != 0
            && self.incarnation != 0
            && match operation {
                STATUS => true,
                NEXT => {
                    self.sequence != 0
                        && self.object == 0
                        && self.revision == 0
                        && self.context == 0
                }
                LOAD => {
                    self.sequence != 0
                        && self.object != 0
                        && self.revision != 0
                        && matches!(self.context, 2048 | 4096)
                        && self.after == 0
                }
                UNLOAD | CANCEL => {
                    self.sequence != 0
                        && self.object == 0
                        && self.revision == 0
                        && self.context == 0
                        && self.after == 0
                }
                _ => false,
            }
    }
    pub fn label(&self) -> &str {
        let n = self
            .name
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..n]).unwrap_or("Invalid model label")
    }
}
pub fn transition(operation: u64, record: &mut Record) -> isize {
    unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                operation,
                record as *mut _ as u64,
                core::mem::size_of::<Record>() as u64,
                0,
                0,
            ],
        ) as isize
    }
}
