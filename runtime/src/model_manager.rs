//! Assistant-owned model lifecycle. Copied scalars only; no paths or memory grants.
pub const SYSCALL: u64 = 387;
pub const STATUS: u64 = 0;
/// Retired operation, always refused.
pub const NEXT: u64 = 1;
/// Retired operation, always refused.
pub const LOAD: u64 = 2;
pub const UNLOAD: u64 = 3;
pub const CANCEL: u64 = 4;
/// Retired picker selection.
pub const SELECT_FILE: u64 = 5;
pub const LOAD_SELECTED: u64 = 6;
pub const LIST: u64 = 7;
pub const READ_SETTINGS: u64 = 8;
pub const WRITE_SETTINGS: u64 = 9;
pub const VERSION: u64 = 4;
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
    pub version: u64,
    pub token: u64,
    pub selection: u64,
    pub length: u64,
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
    pub settings: [u8; 40],
}
impl Record {
    pub const EMPTY: Self = Self {
        version: VERSION,
        token: 0,
        selection: 0,
        length: 0,
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
        settings: [0; 40],
    };
    pub fn valid_request(self, operation: u64) -> bool {
        self.version == VERSION
            && self.window != 0
            && self.incarnation != 0
            && match operation {
                STATUS => true,
                LIST => {
                    self.sequence != 0
                        && self.token == 0
                        && self.selection == 0
                        && self.object == 0
                        && self.revision == 0
                        && self.context == 0
                }
                LOAD_SELECTED => {
                    self.sequence != 0
                        && self.selection != 0
                        && self.token == 0
                        && self.object == 0
                        && self.revision == 0
                        && matches!(self.context, 2048 | 4096)
                        && self.after == 0
                }
                READ_SETTINGS | WRITE_SETTINGS => {
                    self.sequence != 0
                        && self.token == 0
                        && self.selection == 0
                        && self.object == 0
                        && self.revision == 0
                        && self.context == 0
                        && self.after == 0
                        && if operation == READ_SETTINGS {
                            self.settings == [0; 40]
                        } else {
                            crate::assistant::settings::Preferences::decode(&self.settings)
                                .is_some()
                        }
                }
                UNLOAD | CANCEL => {
                    self.sequence != 0
                        && self.token == 0
                        && self.selection == 0
                        && self.object == 0
                        && self.revision == 0
                        && self.context == 0
                        && self.after == 0
                }
                _ => false,
            }
    }
    /// File names can exceed the display field; truncate only at a UTF-8 boundary.
    pub fn set_label(&mut self, text: &str) {
        self.name.fill(0);
        let mut n = text.len().min(self.name.len());
        let shortened = n < text.len();
        if shortened {
            n -= "…".len();
        }
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        self.name[..n].copy_from_slice(&text.as_bytes()[..n]);
        if shortened {
            self.name[n..n + "…".len()].copy_from_slice("…".as_bytes());
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

const _: [(); 304] = [(); core::mem::size_of::<Record>()];
