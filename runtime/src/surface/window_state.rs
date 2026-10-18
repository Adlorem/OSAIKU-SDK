//! Owner-scoped presentation state, never an input or publication grant.
use super::*;
pub const WINDOW_STATE: u32 = 21;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub version: u64,
    pub target: Target,
    pub visible: u64,
    pub theme: u64,
    pub width: u64,
    pub height: u64,
    pub geometry_revision: u64,
}
impl WindowState {
    pub fn valid(&self) -> bool {
        self.version == 1
            && self.target.valid()
            && self.visible <= 1
            && self.theme <= 1
            && self.width > 0
            && self.width <= u32::MAX as u64
            && self.height > 0
            && self.height <= u32::MAX as u64
            && self.geometry_revision != 0
    }
}
pub fn window_state_with(
    target: Target,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<WindowState, PublishError> {
    let request = Request {
        version: VERSION,
        operation: WINDOW_STATE,
        handle: Handle {
            target,
            ..Handle::default()
        },
        ..Request::default()
    };
    let mut state = WindowState::default();
    let mut receipt = Receipt::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut state as *mut WindowState).cast::<u8>(),
            core::mem::size_of::<WindowState>(),
        )
    };
    let code = exchange(&request, bytes, &mut receipt);
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    if !state.valid()
        || state.target != target
        || receipt.handle != request.handle
        || receipt.epoch != target.epoch
    {
        return Err(PublishError::Receipt);
    }
    Ok(state)
}
const _: () = assert!(core::mem::size_of::<WindowState>() == 80);
