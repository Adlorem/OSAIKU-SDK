//! Bounded installed-package observations. Records describe state; they never
//! grant installation, launch, publisher trust or access to another app's data.
//! RDI Request, RSI Reply; all other argument registers must be zero.
pub const SYSCALL: u64 = 395;
pub const VERSION: u64 = 1;
pub const MAX_ITEMS: u64 = 32;
pub mod operation {
    pub const STATUS: u64 = 1;
    pub const ITEM: u64 = 2;
    pub const OUTCOME: u64 = 3;
}
pub mod status {
    pub const OK: u64 = 0;
    pub const BUSY: u64 = 1;
    pub const STALE: u64 = 2;
    pub const DENIED: u64 = 3;
    pub const UNAVAILABLE: u64 = 4;
    pub const FAILED: u64 = 5;
    pub const END: u64 = 6;
}
pub mod outcome {
    pub const UNKNOWN: u64 = 0;
    pub const PENDING: u64 = 1;
    pub const INSTALLED: u64 = 2;
    pub const CANCELLED: u64 = 3;
    pub const INTERRUPTED: u64 = 4;
    pub const REMOVED: u64 = 5;
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub version: u64,
    pub operation: u64,
    pub sequence: u64,
    pub mount: u64,
    pub catalog: u64,
    pub index: u64,
    pub operation_id: [u8; 32],
}
impl Request {
    pub const EMPTY: Self = Self {
        version: VERSION,
        operation: 0,
        sequence: 0,
        mount: 0,
        catalog: 0,
        index: 0,
        operation_id: [0; 32],
    };
    pub fn canonical(&self) -> bool {
        self.version == VERSION
            && self.sequence != 0
            && match self.operation {
                operation::STATUS => {
                    self.mount == 0
                        && self.catalog == 0
                        && self.index == 0
                        && self.operation_id == [0; 32]
                }
                operation::ITEM => {
                    self.mount != 0
                        && self.catalog != 0
                        && self.index < MAX_ITEMS
                        && self.operation_id == [0; 32]
                }
                operation::OUTCOME => {
                    self.mount != 0
                        && self.catalog == 0
                        && self.index == 0
                        && self.operation_id != [0; 32]
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
    pub status: u64,
    pub mount: u64,
    pub catalog: u64,
    pub policy_epoch: u64,
    pub index: u64,
    pub release: u64,
    pub length: u64,
    pub running: u64,
    pub retained: u64,
    pub outcome: u64,
    pub publisher: [u8; 32],
    pub digest: [u8; 32],
    pub package: [u8; 128],
}
impl Reply {
    pub const EMPTY: Self = Self {
        version: VERSION,
        sequence: 0,
        status: status::UNAVAILABLE,
        mount: 0,
        catalog: 0,
        policy_epoch: 0,
        index: 0,
        release: 0,
        length: 0,
        running: 0,
        retained: 0,
        outcome: 0,
        publisher: [0; 32],
        digest: [0; 32],
        package: [0; 128],
    };
    pub fn valid_for(&self, request: &Request) -> bool {
        if !request.canonical()
            || self.version != VERSION
            || self.sequence != request.sequence
            || self.index != request.index
            || self.status > status::END
        {
            return false;
        }
        let empty_item = self.release == 0
            && self.length == 0
            && self.running == 0
            && self.retained == 0
            && self.publisher == [0; 32]
            && self.digest == [0; 32]
            && self.package == [0; 128];
        if self.status != status::OK {
            return empty_item
                && self.outcome == 0
                && if self.status == status::END {
                    request.operation == operation::ITEM
                        && self.mount == request.mount
                        && self.catalog == request.catalog
                        && self.policy_epoch != 0
                } else {
                    self.mount == 0 && self.catalog == 0 && self.policy_epoch == 0
                };
        }
        if self.mount == 0
            || self.catalog == 0
            || self.policy_epoch == 0
            || (request.mount != 0 && self.mount != request.mount)
            || (request.catalog != 0 && self.catalog != request.catalog)
        {
            return false;
        }
        match request.operation {
            operation::STATUS => empty_item && self.outcome == 0,
            operation::OUTCOME => empty_item && self.outcome <= outcome::REMOVED,
            operation::ITEM => {
                self.outcome == 0
                    && self.release != 0
                    && self.length != 0
                    && self.retained <= 1
                    && self.running <= 64
                    && self.publisher != [0; 32]
                    && self.digest != [0; 32]
                    && self.package[0] != 0
                    && self
                        .package
                        .iter()
                        .skip_while(|b| **b != 0)
                        .all(|b| *b == 0)
            }
            _ => false,
        }
    }
}
const _: () = assert!(core::mem::size_of::<Request>() == 80);
const _: () = assert!(core::mem::size_of::<Reply>() == 288);
pub fn read_with(
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
pub fn read(request: &Request) -> Result<Reply, isize> {
    read_with(request, |request, reply| {
        let result: isize;
        unsafe {
            core::arch::asm!("int 0x80", inlateout("rax") SYSCALL as isize => result,
            in("rdi") request as *const Request, in("rsi") reply as *mut Reply,
            in("rdx") 0u64, in("r8") 0u64, in("r9") 0u64, options(nostack));
        }
        result
    })
}
