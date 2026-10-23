//! Native desktop and filesystem client operations. The kernel checks the
//! caller's protected profile and every window/object binding; names grant no rights.
use crate::{abi::*, process::*, session::*};
pub use crate::{
    ipc::ReplyError, log_write, process_exit, scheduler_yield, ui_view_invalidate,
    yield_to as yield_to_endpoint,
};
pub use crate::{
    ipc_reply_and_yield, ipc_send as ipc_send_bytes_to, ipc_send as ipc_send_reply_to,
};
pub fn process_self_endpoint(out: &mut ProcessEndpoint) -> isize {
    match crate::process_self() {
        Ok(v) => {
            *out = v;
            0
        }
        Err(e) => e,
    }
}
pub fn ipc_recv_from_endpoint(b: &mut [u8], s: &mut ProcessEndpoint) -> isize {
    crate::ipc_receive(crate::ipc::ReceiveMode::Blocking, b, s)
}
pub fn ipc_recv_timed(b: &mut [u8], s: &mut ProcessEndpoint, t: u64) -> isize {
    crate::ipc_receive(crate::ipc::ReceiveMode::Timed { timeout_us: t }, b, s)
}
static UI_PUBLISHED_EPOCH: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
fn syscall1(number: u64, a0: u64) -> u64 {
    unsafe { crate::raw::syscall(number, [a0, 0, 0, 0, 0]) }
}
fn syscall2(number: u64, a0: u64, a1: u64) -> u64 {
    unsafe { crate::raw::syscall(number, [a0, a1, 0, 0, 0]) }
}
fn syscall3(number: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    unsafe { crate::raw::syscall(number, [a0, a1, a2, 0, 0]) }
}
fn syscall4(number: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    unsafe { crate::raw::syscall(number, [a0, a1, a2, a3, 0]) }
}
fn syscall5(number: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    unsafe { crate::raw::syscall(number, [a0, a1, a2, a3, a4]) }
}
pub fn file_operation(
    request: &crate::files::FileRequest,
    reply: &mut crate::files::FileReply,
) -> isize {
    syscall2(
        crate::abi::number::FILE_OPERATION,
        request as *const _ as u64,
        reply as *mut _ as u64,
    ) as isize
}
/// # Safety
/// The entry, argument and aligned stack must remain valid for the new thread
/// until it exits; concurrent access must obey Rust's aliasing rules.
pub unsafe fn thread_create(request: &crate::threads::Create) -> isize {
    syscall1(
        crate::abi::number::THREAD_CREATE,
        request as *const _ as u64,
    ) as isize
}
pub fn thread_join(id: u64, code: &mut u64) -> isize {
    syscall2(crate::abi::number::THREAD_JOIN, id, code as *mut _ as u64) as isize
}
pub fn thread_sleep(micros: u64) -> isize {
    syscall1(crate::abi::number::THREAD_SLEEP, micros) as isize
}
pub fn thread_exit(code: u64) -> ! {
    syscall1(crate::abi::number::THREAD_EXIT, code);
    loop {
        core::hint::spin_loop();
    }
}
pub fn file_manager_content_submit(
    payload: &crate::file_manager::ContentFrame,
    bytes: &[u8],
    status: &mut crate::window::UiDocumentWindowChannelStatusRecord,
) -> isize {
    syscall4(
        348,
        payload as *const _ as u64,
        status as *mut _ as u64,
        bytes.as_ptr() as u64,
        bytes.len() as u64,
    ) as isize
}
pub fn file_export_snapshot(
    window: u64,
    generation: u64,
    view: &mut crate::file_export::View,
) -> isize {
    syscall3(351, window, generation, view as *mut _ as u64) as isize
}
pub fn file_browser_snapshot(
    window: u64,
    generation: u64,
    view: &mut crate::file_manager::View,
) -> isize {
    syscall4(
        crate::file_browser::SYSCALL,
        crate::file_browser::SNAPSHOT,
        window,
        generation,
        view as *mut _ as u64,
    ) as isize
}
pub fn file_browser_devices(
    window: u64,
    generation: u64,
    devices: &mut crate::file_browser::Devices,
) -> isize {
    syscall4(
        crate::file_browser::SYSCALL,
        crate::file_browser::DEVICES,
        window,
        generation,
        devices as *mut _ as u64,
    ) as isize
}
pub fn file_browser_viewport(window: u64, generation: u64, first: u64, hash: u64) -> isize {
    syscall5(
        crate::file_browser::SYSCALL,
        crate::file_browser::VIEWPORT,
        window,
        generation,
        first,
        hash,
    ) as isize
}
pub fn ui_action_take_for(window: u64, event: &mut crate::window::Event) -> isize {
    // SAFETY: output is exclusively borrowed through the copying trap.
    unsafe { crate::raw::ui_action_take_for(window, event) }
}
pub fn ui_published_epoch() -> u64 {
    UI_PUBLISHED_EPOCH.load(core::sync::atomic::Ordering::Acquire)
}
pub fn ui_frame_offer(
    request: &crate::window::UiDocumentWindowPresentRequest,
    owner: crate::process::ProcessEndpoint,
) -> Result<crate::surface::FrameOffer, isize> {
    use crate::surface::*;
    let query = Request {
        version: VERSION,
        operation: QUERY,
        ..Request::default()
    };
    let mut receipt = Receipt::default();
    let code = unsafe { transition(&query, &mut [], &mut receipt) };
    if code != 0 {
        return Err(code);
    }
    let target = Target {
        owner: owner.encode(),
        window: request.expected_window_id,
        incarnation: request.expected_window_generation,
        epoch: receipt.epoch,
    };
    frame_offer_with(
        target,
        request.request_id,
        request.request_hash,
        |r, b, o| unsafe { transition(r, b, o) },
    )
    .map_err(|_| crate::abi::SYSCALL_ERR_INVALID_STATE)
}
pub fn ui_publish_pixels_with_images(
    request: &crate::window::UiDocumentWindowPresentRequest,
    owner: crate::process::ProcessEndpoint,
    bytes: &[u8],
    images: &'static [osaiku_ui::image::ImageSet],
) -> isize {
    let offer = match ui_frame_offer(request, owner) {
        Ok(v) => v,
        Err(code) => return code,
    };
    ui_publish_offered_pixels_with_images(offer, bytes, images)
}
pub fn ui_publish_offered_pixels_with_images(
    offer: crate::surface::FrameOffer,
    bytes: &[u8],
    images: &'static [osaiku_ui::image::ImageSet],
) -> isize {
    use crate::surface::*;
    static RASTER: crate::hosted::HostedRaster = crate::hosted::HostedRaster::new();
    match RASTER.publish_with_images(
        offer,
        bytes,
        images,
        |r, b, o| unsafe { transition(r, b, o) },
        || {
            let code = scheduler_yield();
            if code == 0 {
                Ok(())
            } else {
                Err(code)
            }
        },
    ) {
        Ok(handle) => {
            UI_PUBLISHED_EPOCH.store(handle.target.epoch, core::sync::atomic::Ordering::Release);
            0
        }
        Err(crate::hosted::RasterError::Publish(
            PublishError::Receipt | PublishError::Cleanup { .. },
        )) => process_exit(57),
        Err(_) => crate::abi::SYSCALL_ERR_INVALID_STATE,
    }
}
/// # Safety
/// A released mapping must have no live references, pending workers or executing
/// code. Newly mapped memory must be initialized before typed access.
pub unsafe fn process_session_memory_transition(
    request: &ProcessSessionMemoryRequest,
    receipt_out: &mut ProcessSessionMemoryReceipt,
) -> isize {
    // SAFETY: native adapter; caller upholds the mapping lifetime contract.
    unsafe { crate::raw::session_memory_transition(request, receipt_out) }
}
