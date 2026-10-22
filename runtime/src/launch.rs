//! Kernel-issued identity of the current application. This read-only record is
//! descriptive: copying it to another process never transfers authority.
use crate::{abi::*, process::ProcessEndpoint};
pub use osaiku_package::application::permission;
pub const SYSCALL: u64 = 394;
pub const FIRST_DYNAMIC_APPLICATION: u64 = 65536;
pub const VERSION: u64 = 1;
pub const BOOT_IMAGE: u64 = 1;
pub const PACKAGE: u64 = 2;
/// Ordinary package authenticated by the immutable running system bank.
pub const BANK_PACKAGE: u64 = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub version: u64,
    pub source: u64,
    pub application: u64,
    pub generation: u64,
    pub release: u64,
    pub policy_epoch: u64,
    pub permissions: u64,
    pub owner: u64,
    pub publisher: [u8; 32],
    pub digest: [u8; 32],
    pub package: [u8; 128],
}
impl Context {
    pub const EMPTY: Self = Self {
        version: VERSION,
        source: 0,
        application: 0,
        generation: 0,
        release: 0,
        policy_epoch: 0,
        permissions: 0,
        owner: 0,
        publisher: [0; 32],
        digest: [0; 32],
        package: [0; 128],
    };
    pub fn valid_for(&self, owner: ProcessEndpoint) -> bool {
        self.version == VERSION
            && self.owner == owner.encode()
            && owner.process_id.0 != 0
            && owner.names_instance()
            && self.application != 0
            && self.generation != 0
            && self.permissions & !permission::KNOWN == 0
            && match self.source {
                BOOT_IMAGE => {
                    self.release == 0
                        && self.policy_epoch == 0
                        && self.publisher == [0; 32]
                        && self.digest == [0; 32]
                }
                PACKAGE | BANK_PACKAGE => {
                    self.release != 0
                        && self.policy_epoch != 0
                        && self.publisher != [0; 32]
                        && self.digest != [0; 32]
                }
                _ => false,
            }
            && self.package[0] != 0
            && self
                .package
                .iter()
                .skip_while(|b| **b != 0)
                .all(|b| *b == 0)
    }
}
const _: () = assert!(core::mem::size_of::<Context>() == 256);
pub fn read_with(
    owner: ProcessEndpoint,
    mut exchange: impl FnMut(&mut Context) -> isize,
) -> Result<Context, isize> {
    let mut context = Context::EMPTY;
    let code = exchange(&mut context);
    if code != SYSCALL_OK {
        return Err(code);
    }
    if !context.valid_for(owner) {
        return Err(SYSCALL_ERR_INVALID_STATE);
    }
    Ok(context)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn current() -> Result<Context, isize> {
    let owner = crate::process_self()?;
    read_with(owner, |context| unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                context as *mut Context as u64,
                core::mem::size_of::<Context>() as u64,
                0,
                0,
                0,
            ],
        ) as isize
    })
}
