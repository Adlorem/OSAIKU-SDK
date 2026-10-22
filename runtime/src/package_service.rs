//! Protected package broker. A token identifies kernel-retained human intent;
//! no message can supply a package path, trust key, permissions or consent.
pub const SYSCALL: u64 = 397;
pub const VERSION: u64 = 1;
pub const POLL: u64 = 0;
pub const EXECUTE: u64 = 1;
pub const STATUS: u64 = 2;
pub const CANCEL: u64 = 3;
/// initd acknowledges an opened window, or reports a failed first-frame transaction.
pub const OPENED: u64 = 4;
pub const OPEN_FAILED: u64 = 5;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub version: u64,
    pub operation: u64,
    pub sequence: u64,
    pub token: u64,
}
impl Request {
    pub const EMPTY: Self = Self {
        version: VERSION,
        operation: POLL,
        sequence: 1,
        token: 0,
    };
    pub fn canonical(&self) -> bool {
        self.version == VERSION
            && self.sequence != 0
            && self.operation <= OPEN_FAILED
            && ((self.operation == POLL) == (self.token == 0))
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub version: u64,
    pub sequence: u64,
    pub token: u64,
    pub status: u64,
    pub phase: u64,
    pub completed: u64,
    pub total: u64,
    pub endpoint: u64,
}
impl Reply {
    pub const EMPTY: Self = Self {
        version: VERSION,
        sequence: 0,
        token: 0,
        status: 0,
        phase: 0,
        completed: 0,
        total: 0,
        endpoint: 0,
    };
    pub fn valid_for(&self, r: &Request) -> bool {
        r.canonical()
            && self.version == VERSION
            && self.sequence == r.sequence
            && self.status <= 6
            && self.phase <= 5
            && (r.operation == POLL || self.token == r.token)
            && self.completed <= self.total
    }
}
pub fn exchange_with(
    r: &Request,
    mut f: impl FnMut(&Request, &mut Reply) -> isize,
) -> Result<Reply, isize> {
    if !r.canonical() {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    let mut out = Reply::EMPTY;
    let code = f(r, &mut out);
    if code != crate::abi::SYSCALL_OK {
        return Err(code);
    }
    if !out.valid_for(r) {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    Ok(out)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(r: &Request) -> Result<Reply, isize> {
    exchange_with(r, |r, out| {
        let code: isize;
        unsafe {
            core::arch::asm!("int 0x80",inlateout("rax") SYSCALL as isize=>code,in("rdi") r as *const Request,in("rsi") out as *mut Reply,in("rdx")0u64,in("r8")0u64,in("r9")0u64,options(nostack));
        }
        code
    })
}
/// Call between service transactions. Restarting a service polls the same token;
/// EXECUTE is idempotent and never replays an admitted or completed operation.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn poll() {
    if let Ok(r) = exchange(&Request::EMPTY) {
        if r.token != 0 && r.phase == 1 {
            let _ = exchange(&Request {
                operation: EXECUTE,
                token: r.token,
                ..Request::EMPTY
            });
        }
    }
}
