//! Bounded offscreen BCS service, transported by UI_SURFACE without a window.
//! Kernel generates every command/address. This does not advertise a renderer,
//! accept raw batches, or expose DMA mappings. A job belongs to its submitter.
use crate::graphics::Handle;
pub const OPERATION: u32 = 23;
pub const VERSION: u64 = 1;
pub const SUBMIT: u32 = 1;
pub const QUERY: u32 = 2;
pub const WAIT: u32 = 3;
pub const RELEASE: u32 = 4;
pub const QUEUED: u32 = 1;
pub const RUNNING: u32 = 2;
pub const COMPLETE: u32 = 3;
pub const FAILED: u32 = 4;
pub const WAIT_TIMEOUT: u32 = 5;
pub const RELEASED: u32 = 6;
pub const ZERO: Handle = Handle {
    owner: 0,
    device: 0,
    generation: 0,
    serial: 0,
    kind: 0,
    reserved: 0,
};
/// Input output fields must be zero. SUBMIT accepts a nonzero seed, priority
/// 0 (background) or 1 (normal), and an optional same-owner dependency.
/// WAIT's timeout concerns this wait, not cancellation of the GPU request.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub version: u64,
    pub operation: u32,
    pub priority: u32,
    pub job: Handle,
    pub dependency: Handle,
    pub timeout_us: u64,
    pub seed: u32,
    pub state: u32,
    /// Stable service errors: 1 device lost, 2 execution timeout,
    /// 3 dependency failure, 4 readback mismatch.
    pub error: u32,
    pub reserved: u32,
    pub values: [u32; 64],
}
impl Default for Request {
    fn default() -> Self {
        Self {
            version: VERSION,
            operation: 0,
            priority: 0,
            job: ZERO,
            dependency: ZERO,
            timeout_us: 0,
            seed: 0,
            state: 0,
            error: 0,
            reserved: 0,
            values: [0; 64],
        }
    }
}
impl Request {
    pub fn valid(&self) -> bool {
        if self.version != VERSION
            || self.state != 0
            || self.error != 0
            || self.reserved != 0
            || self.values != [0; 64]
        {
            return false;
        }
        match self.operation {
            SUBMIT => {
                self.job == ZERO && self.seed != 0 && self.priority <= 1 && self.timeout_us == 0
            }
            QUERY | RELEASE => {
                self.job != ZERO
                    && self.dependency == ZERO
                    && self.seed == 0
                    && self.priority == 0
                    && self.timeout_us == 0
            }
            WAIT => {
                self.job != ZERO
                    && self.dependency == ZERO
                    && self.seed == 0
                    && self.priority == 0
                    && self.timeout_us > 0
                    && self.timeout_us <= 2_000_000
            }
            _ => false,
        }
    }
}
/// Saved by the kernel scheduler; never carries a userspace pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    pub job: Handle,
    pub deadline: u64,
}

pub fn exchange_with(
    request: Request,
    mut exchange: impl FnMut(&crate::surface::Request, &mut [u8], &mut crate::surface::Receipt) -> isize,
) -> Result<Request, crate::surface::PublishError> {
    use crate::surface::{PublishError, Receipt};
    if !request.valid() {
        return Err(PublishError::Receipt);
    }
    let mut response = request;
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut response as *mut Request).cast::<u8>(),
            core::mem::size_of::<Request>(),
        )
    };
    let mut receipt = Receipt::default();
    let result = exchange(
        &crate::surface::Request {
            version: crate::surface::REQUEST_VERSION,
            operation: OPERATION,
            ..crate::surface::Request::default()
        },
        bytes,
        &mut receipt,
    );
    if result != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(result));
    }
    if receipt != Receipt::default()
        || response.version != VERSION
        || response.reserved != 0
        || response.operation != request.operation
        || !(QUEUED..=RELEASED).contains(&response.state)
        || (request.operation != SUBMIT && response.job != request.job)
        || response.job.kind != crate::graphics::kind::JOB
        || response.job.reserved != 0
        || response.job.owner == 0
        || response.job.device == 0
        || response.job.generation == 0
        || response.job.serial == 0
        || (response.state != COMPLETE && response.values != [0; 64])
    {
        return Err(PublishError::Receipt);
    }
    Ok(response)
}
const _: () = assert!(core::mem::size_of::<Request>() == 376);
