//! Atomic small private files. The kernel resolves the caller's live package
//! lease; no publisher, path, Context or capability supplied here grants access.
pub const SYSCALL: u64 = 396;
pub const VERSION: u64 = 1;
pub const CAPACITY: usize = 4096;
pub const READ: u64 = 1;
pub const REPLACE: u64 = 2;
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub version: u64,
    pub sequence: u64,
    pub operation: u64,
    pub offset: u64,
    pub length: u64,
    pub name: [u8; 64],
    pub bytes: [u8; CAPACITY],
}
impl Request {
    pub const EMPTY: Self = Self {
        version: VERSION,
        sequence: 0,
        operation: 0,
        offset: 0,
        length: 0,
        name: [0; 64],
        bytes: [0; CAPACITY],
    };
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name.iter().position(|b| *b == 0).unwrap_or(64)]
    }
    pub fn canonical(&self) -> bool {
        let name = self.name();
        self.version == VERSION
            && self.sequence != 0
            && !name.is_empty()
            && name.len() < 64
            && name != b"."
            && name != b".."
            && name
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
            && self.name[name.len()..].iter().all(|b| *b == 0)
            && self.length <= CAPACITY as u64
            && match self.operation {
                READ => {
                    self.length != 0 && self.offset <= 65536 && self.bytes.iter().all(|b| *b == 0)
                }
                REPLACE => {
                    self.offset == 0 && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
                }
                _ => false,
            }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reply {
    pub version: u64,
    pub sequence: u64,
    pub status: u64,
    pub length: u64,
    pub bytes: [u8; CAPACITY],
}
const _: () = assert!(core::mem::size_of::<Request>() == 4200);
const _: () = assert!(core::mem::size_of::<Reply>() == 4128);
impl Reply {
    pub const EMPTY: Self = Self {
        version: VERSION,
        sequence: 0,
        status: crate::packages::status::UNAVAILABLE,
        length: 0,
        bytes: [0; CAPACITY],
    };
    pub fn valid_for(&self, request: &Request) -> bool {
        request.canonical()
            && self.version == VERSION
            && self.sequence == request.sequence
            && self.status <= crate::packages::status::FAILED
            && self.length <= request.length
            && (self.status == crate::packages::status::OK || self.length == 0)
            && (request.operation == READ || self.length == 0)
            && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
    }
}
pub fn exchange_with(
    request: &Request,
    mut exchange: impl FnMut(&Request, &mut Reply) -> isize,
) -> Result<Reply, isize> {
    if !request.canonical() {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    let mut reply = Reply::EMPTY;
    let code = exchange(request, &mut reply);
    if code != crate::abi::SYSCALL_OK {
        return Err(code);
    }
    if !reply.valid_for(request) {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    Ok(reply)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(request: &Request) -> Result<Reply, isize> {
    exchange_with(request, |request, reply| {
        let result: isize;
        unsafe {
            core::arch::asm!("int 0x80", inlateout("rax") SYSCALL as isize => result,
                in("rdi") request as *const Request, in("rsi") reply as *mut Reply,
                in("rdx") 0u64, in("r8") 0u64, in("r9") 0u64, options(nostack));
        }
        result
    })
}
