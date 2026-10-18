//! The x86_64 interrupt calling convention shared with the OS runtime adapter.
use crate::abi::{number, SYSCALL_VECTOR};
use core::arch::asm;

/// Invoke the OSAIKU software-interrupt gate. Unused arguments must be zero.
///
/// # Safety
/// Execute only in an OSAIKU user process with its syscall gate installed.
/// The caller must uphold the selected syscall's pointer, length, aliasing and
/// lifetime contract. All referenced buffers must remain valid through return,
/// including any blocking/rescheduling. Numbers confer no kernel authority.
#[inline]
pub unsafe fn syscall(number: u64, args: [u64; 5]) -> u64 {
    let result: u64;
    // No nomem/readonly/preserves_flags: the kernel may read/write buffers,
    // change flags and reschedule before returning. Keep C caller clobbers.
    unsafe {
        asm!(
            "int {vector}",
            vector = const SYSCALL_VECTOR,
            inlateout("rax") number => result,
            in("rdi") args[0],
            in("rsi") args[1],
            in("rdx") args[2],
            in("r8") args[3],
            in("r9") args[4],
            clobber_abi("C"),
        );
    }
    result
}

/// Terminate the calling process through the dedicated non-returning trap.
///
/// # Safety
/// Execute only in an OSAIKU user process with its syscall gate installed.
#[inline]
pub unsafe fn exit(code: u64) -> ! {
    unsafe {
        asm!(
            "int {vector}",
            vector = const SYSCALL_VECTOR,
            in("rax") number::PROCESS_EXIT,
            in("rdi") code,
            clobber_abi("C"),
            options(noreturn),
        );
    }
}

/// Send exactly one message to an endpoint, preserving native error precedence.
///
/// # Safety
/// Execute only in an OSAIKU user process. The kernel must implement the SDK
/// IPC contract: copy the borrowed bytes during this call, never retain them.
pub unsafe fn ipc_send(endpoint: crate::process::ProcessEndpoint, message: &[u8]) -> isize {
    let (number, args) = crate::ipc::send_invocation(endpoint, message);
    // SAFETY: borrowed input is live for the complete synchronous trap.
    unsafe { syscall(number, args) as isize }
}

/// Transfer control once to the endpoint under kernel activation policy.
///
/// # Safety
/// Execute only in an OSAIKU user process with its syscall gate installed.
pub unsafe fn yield_to(endpoint: crate::process::ProcessEndpoint) -> isize {
    let (number, args) = crate::ipc::yield_invocation(endpoint);
    // SAFETY: no pointers; kernel validates authority and the target instance.
    unsafe { syscall(number, args) as isize }
}

/// Receive once into exclusive buffers, including the complete sender endpoint.
///
/// # Safety
/// Execute only in an OSAIKU user process. The kernel must respect buffer bounds
/// and release pending output pointers before this call returns, even when
/// interrupted/timed out. Neither buffer may be accessed by another thread or
/// continuation while blocked. The Rust exclusive borrows cover this call.
pub unsafe fn ipc_receive(
    mode: crate::ipc::ReceiveMode,
    buffer: &mut [u8],
    sender: &mut crate::process::ProcessEndpoint,
) -> isize {
    let (number, args) = mode.invocation(buffer, sender);
    // SAFETY: caller grants exclusive live outputs through any rescheduling.
    unsafe { syscall(number, args) as isize }
}

/// Take one presented UI event from the calling application's queue.
///
/// # Safety
/// Execute only in an OSAIKU user process. The kernel must copy at most one
/// complete Event into this exclusive borrow and retain no pointer after return.
pub unsafe fn ui_action_take(event: &mut crate::window::Event) -> isize {
    // SAFETY: the complete aligned output is exclusively borrowed for the call.
    unsafe {
        syscall(
            number::UI_ACTION_TAKE,
            [event as *mut crate::window::Event as u64, 0, 0, 0, 0],
        ) as isize
    }
}

/// Drain input for one caller-owned window. Zero selects any window.
///
/// # Safety
/// Execute only in an OSAIKU application process. Keep the event output valid
/// and exclusively borrowed for the duration of the copying call.
pub unsafe fn ui_action_take_for(window: u64, event: &mut crate::window::Event) -> isize {
    unsafe {
        syscall(
            number::UI_ACTION_TAKE,
            [event as *mut crate::window::Event as u64, window, 0, 0, 0],
        ) as isize
    }
}

/// Submit one bounded UI recipe to the caller's armed window transaction.
///
/// # Safety
/// Execute only in an OSAIKU user process. The kernel must copy the borrowed
/// inputs and release all pointers before return, including deferred admission.
/// It must write at most one complete record into the exclusive status borrow.
pub unsafe fn ui_content_submit(
    frame: &crate::window::UiDocumentWindowContentFrame,
    bytes: &[u8],
    status: &mut crate::window::UiDocumentWindowChannelStatusRecord,
) -> isize {
    let args = crate::window::content_submit_arguments(frame, bytes, status);
    // SAFETY: inputs and exclusive output stay live through the synchronous call.
    unsafe { syscall(number::UI_CONTENT_SUBMIT, args) as isize }
}

/// Perform one caller-scoped session MAP/RELEASE transition without retry.
///
/// # Safety
/// Execute only in an OSAIKU application process. Before RELEASE, end all
/// accesses/borrows and destroy live values in the page; no continuation may
/// access it after success. Keep request/receipt in the registered process
/// image or stack, not a session mapping. The kernel must bound its receipt
/// write and retain no control-buffer pointers after the synchronous return.
pub unsafe fn session_memory_transition(
    request: &crate::session::ProcessSessionMemoryRequest,
    receipt: &mut crate::session::ProcessSessionMemoryReceipt,
) -> isize {
    let args = crate::session::transition_arguments(request, receipt);
    // SAFETY: caller upholds both buffer and released-mapping lifetimes.
    unsafe { syscall(number::PROCESS_SESSION_MEMORY_TRANSITION, args) as isize }
}
