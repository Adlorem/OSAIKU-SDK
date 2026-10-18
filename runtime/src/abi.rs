pub const SYSCALL_VECTOR: u8 = 0x80;
pub const MAX_LOG_WRITE_LEN: usize = 160;
pub const MAX_IPC_MESSAGE_LEN: usize = 160;
pub const SYSCALL_OK: isize = 0;
pub const SYSCALL_ERR_INVALID_BUFFER: isize = -1;
pub const SYSCALL_ERR_INVALID_TARGET: isize = -2;
pub const SYSCALL_ERR_MAILBOX_EMPTY: isize = -3;
pub const SYSCALL_ERR_MAILBOX_FULL: isize = -4;
pub const SYSCALL_ERR_INVALID_STATE: isize = -5;
pub const SYSCALL_ERR_BUFFER_TOO_SMALL: isize = -6;
pub const SYSCALL_ERR_NOT_FOUND: isize = -7;
pub const SYSCALL_ERR_PERMISSION_DENIED: isize = -8;
pub const SYSCALL_ERR_TARGET_FAULTED: isize = -9;
pub const SYSCALL_ERR_LIMIT_EXCEEDED: isize = -10;
pub const SYSCALL_ERR_NOT_SUPPORTED: isize = -11;
/// The address named an instance that is not the one installed, or named none
/// where one is required.
///
/// Distinct from `NOT_FOUND`, which says the identity resolves to nothing, and
/// from `PERMISSION_DENIED`, which says the sender may not address it at all.
/// This one says the address was for somebody else.
pub const SYSCALL_ERR_STALE_ENDPOINT: isize = -12;
/// Transient admission pressure; no command or content was consumed.
pub const SYSCALL_ERR_BUSY: isize = -13;
pub const SYSCALL_ERR_TIMEOUT: isize = -14;

/// Published numbers for the initial native runtime subset.
pub mod number {
    pub const LOG_WRITE: u64 = 0;
    pub const PROCESS_EXIT: u64 = 1;
    pub const SCHEDULER_YIELD: u64 = 22;
    pub const MODEL_RUNTIME_CLOCK: u64 = 380;
    pub const PROCESS_OBSERVE: u64 = 388;
    pub const PROCESS_SELF_ENDPOINT: u64 = 336;
    pub const PROCESS_SESSION_MEMORY_TRANSITION: u64 = 334;
    pub const IPC_SEND: u64 = 2;
    pub const YIELD_TO: u64 = 4;
    pub const POWER_PREPARE: u64 = 356;
    pub const IPC_RECEIVE_FROM_ENDPOINT: u64 = 337;
    pub const IPC_TRY_RECEIVE_FROM_ENDPOINT: u64 = 352;
    pub const IPC_RECEIVE_TIMED: u64 = 359;
    pub const UI_ACTION_TAKE: u64 = 365;
    pub const UI_CONTENT_SUBMIT: u64 = 363;
    pub const UI_SURFACE: u64 = 368;
    pub const UI_VIEW_INVALIDATE: u64 = 375;
    pub const UI_CONTEXT: u64 = 376;
    pub const SYSTEM_CONTEXT: u64 = 379;
    pub const FILE_OPERATION: u64 = 347;
    pub const THREAD_CREATE: u64 = 370;
    pub const THREAD_JOIN: u64 = 371;
    pub const THREAD_EXIT: u64 = 372;
    pub const THREAD_SLEEP: u64 = 374;
}
