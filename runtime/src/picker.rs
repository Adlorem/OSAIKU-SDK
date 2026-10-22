//! Human-selected file scopes. OPEN asks for a chooser; it grants no file.
//! Read/replace use only the opaque returned token, never paths or object IDs.
pub const SYSCALL: u64 = 398;
pub const OPEN: u64 = 1;
pub const STATUS: u64 = 2;
pub const CANCEL: u64 = 3;
pub const READ: u64 = 4;
pub const REPLACE: u64 = 5;
pub const CAPACITY: usize = 4096;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub version: u64,
    pub sequence: u64,
    pub operation: u64,
    pub window: u64,
    pub incarnation: u64,
    pub token: u64,
    pub rights: u64,
    pub offset: u64,
    pub length: u64,
    pub bytes: [u8; CAPACITY],
}
impl Request {
    pub const EMPTY: Self = Self {
        version: 1,
        sequence: 1,
        operation: OPEN,
        window: 0,
        incarnation: 0,
        token: 0,
        rights: 0,
        offset: 0,
        length: 0,
        bytes: [0; CAPACITY],
    };
    pub fn canonical(&self) -> bool {
        use crate::launch::permission as p;
        let selected = p::SELECTED_FILE_READ | p::SELECTED_FILE_WRITE;
        self.version == 1
            && self.sequence != 0
            && self.window != 0
            && self.incarnation != 0
            && self.length <= CAPACITY as u64
            && match self.operation {
                OPEN => {
                    self.token == 0
                        && self.rights != 0
                        && self.rights & !selected == 0
                        && self.offset == 0
                        && self.length == 0
                        && self.bytes == [0; CAPACITY]
                }
                STATUS | CANCEL => {
                    self.token != 0
                        && self.rights == 0
                        && self.offset == 0
                        && self.length == 0
                        && self.bytes == [0; CAPACITY]
                }
                READ => {
                    self.token != 0
                        && self.rights == 0
                        && self.length != 0
                        && self.bytes == [0; CAPACITY]
                }
                REPLACE => {
                    self.token != 0
                        && self.rights == 0
                        && self.offset == 0
                        && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
                }
                _ => false,
            }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub version: u64,
    pub sequence: u64,
    pub token: u64,
    pub status: u64,
    pub length: u64,
    pub bytes: [u8; CAPACITY],
}
impl Reply {
    pub const EMPTY: Self = Self {
        version: 1,
        sequence: 0,
        token: 0,
        status: 0,
        length: 0,
        bytes: [0; CAPACITY],
    };
    pub fn valid_for(&self, r: &Request) -> bool {
        r.canonical()
            && self.version == 1
            && self.sequence == r.sequence
            && self.status <= 6
            && (r.operation == OPEN || self.token == r.token)
            && self.length <= CAPACITY as u64
            && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
            && (self.status == 0 || self.length == 0)
            && ((r.operation == READ && self.length <= r.length) || self.length == 0)
    }
}
/// Borrowed wire workspace for small-stack native applications. No payload
/// array is allocated by this function; the caller may retain it in its image.
pub fn exchange_into_with(
    r: &Request,
    out: &mut Reply,
    mut f: impl FnMut(&Request, &mut Reply) -> isize,
) -> Result<(), isize> {
    *out = Reply::EMPTY;
    if !r.canonical() {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    let code = f(r, out);
    if code != 0 || !out.valid_for(r) {
        *out = Reply::EMPTY;
        return Err(if code != 0 {
            code
        } else {
            crate::abi::SYSCALL_ERR_INVALID_STATE
        });
    }
    Ok(())
}
pub fn exchange_with(
    r: &Request,
    f: impl FnMut(&Request, &mut Reply) -> isize,
) -> Result<Reply, isize> {
    let mut out = Reply::EMPTY;
    exchange_into_with(r, &mut out, f)?;
    Ok(out)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange_into(r: &Request, out: &mut Reply) -> Result<(), isize> {
    exchange_into_with(r, out, |r, out| {
        let code: isize;
        unsafe {
            core::arch::asm!("int 0x80",inlateout("rax") SYSCALL as isize=>code,in("rdi")r as *const Request,in("rsi")out as *mut Reply,in("rdx")0u64,in("r8")0u64,in("r9")0u64,options(nostack));
        }
        code
    })
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(r: &Request) -> Result<Reply, isize> {
    let mut out = Reply::EMPTY;
    exchange_into(r, &mut out)?;
    Ok(out)
}
