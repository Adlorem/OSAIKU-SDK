use crate::{abi::*, process::*, raw};
pub fn process_observe(key: observe::Notice) -> isize {
    unsafe {
        raw::syscall(
            number::PROCESS_OBSERVE,
            [
                observe::SUBSCRIBE,
                key.endpoint,
                key.window,
                key.incarnation,
                observe::VERSION,
            ],
        ) as isize
    }
}
pub fn process_unobserve(key: observe::Notice) -> isize {
    unsafe {
        raw::syscall(
            number::PROCESS_OBSERVE,
            [
                observe::REMOVE,
                key.endpoint,
                key.window,
                key.incarnation,
                observe::VERSION,
            ],
        ) as isize
    }
}
pub fn process_observation() -> Result<observe::Notice, isize> {
    let mut notice = observe::Notice::default();
    let code = unsafe {
        raw::syscall(
            number::PROCESS_OBSERVE,
            [
                observe::TAKE,
                &mut notice as *mut _ as u64,
                core::mem::size_of_val(&notice) as u64,
                0,
                0,
            ],
        ) as isize
    };
    if code == 0 {
        Ok(notice)
    } else {
        Err(code)
    }
}

/// Calibrated monotonic milliseconds; no wall-clock or model authority.
pub fn monotonic_ms() -> Result<u64, isize> {
    let value = unsafe { raw::syscall(number::MODEL_RUNTIME_CLOCK, [0; 5]) } as i64;
    if value < 0 {
        Err(value as isize)
    } else {
        Ok(value as u64)
    }
}

/// Query this process's pending shutdown preparation epoch. This grants no
/// authority to initiate shutdown. Zero means no preparation is pending.
pub fn shutdown_prepare_epoch() -> Result<Option<u64>, isize> {
    // SAFETY: native target, no pointers; the kernel selects this caller's epoch.
    let result = unsafe { raw::syscall(number::POWER_PREPARE, [0; 5]) };
    if (result as i64) < 0 {
        Err(result as isize)
    } else {
        Ok((result != 0).then_some(result))
    }
}

/// Answer a kernel-issued preparation epoch after the app has checked its own
/// unsaved state/work. This does not initiate or approve a power action.
pub fn shutdown_prepare_reply(epoch: u64, ready: bool) -> isize {
    // SAFETY: native target; epoch authority/liveness remain kernel-owned.
    unsafe {
        raw::syscall(
            number::POWER_PREPARE,
            [epoch, if ready { 1 } else { 2 }, 0, 0, 0],
        ) as isize
    }
}

/// Write a bounded UTF-8 message to the kernel log. Returns the native status.
pub fn log_write(message: &str) -> isize {
    if message.len() > MAX_LOG_WRITE_LEN {
        return SYSCALL_ERR_INVALID_BUFFER;
    }
    // SAFETY: the shared borrow remains live and immutable until the trap returns.
    unsafe {
        raw::syscall(
            number::LOG_WRITE,
            [message.as_ptr() as u64, message.len() as u64, 0, 0, 0],
        ) as isize
    }
}

/// Obtain the kernel-issued address of this process instance.
pub fn process_self() -> Result<ProcessEndpoint, isize> {
    let mut endpoint = ProcessEndpoint::unbound(ProcessId::KERNEL);
    // SAFETY: a complete aligned endpoint is exclusively borrowed for the call.
    let result = unsafe {
        raw::syscall(
            number::PROCESS_SELF_ENDPOINT,
            [&mut endpoint as *mut ProcessEndpoint as u64, 0, 0, 0, 0],
        ) as isize
    };
    if result != SYSCALL_OK {
        Err(result)
    } else if endpoint.process_id == ProcessId::KERNEL || !endpoint.names_instance() {
        Err(SYSCALL_ERR_INVALID_STATE)
    } else {
        Ok(endpoint)
    }
}

/// Yield once without acknowledging any application shutdown preparation.
/// Lifecycle hosts must handle their preparation protocol before yielding.
pub fn scheduler_yield() -> isize {
    // SAFETY: no pointers; authority and scheduling remain kernel-owned.
    unsafe { raw::syscall(number::SCHEDULER_YIELD, [0; 5]) as isize }
}

/// Transfer control once to an endpoint; return when the caller is resumed or
/// the kernel rejects activation. Permission and generation checks remain in
/// the kernel. This does not acknowledge application shutdown preparation.
pub fn yield_to(endpoint: ProcessEndpoint) -> isize {
    // SAFETY: native target; no pointers, kernel-owned activation policy.
    unsafe { raw::yield_to(endpoint) }
}

/// Send one reply, then transfer control to its recipient. No retry or shutdown
/// acknowledgment. A Handoff error means the reply was already sent; do not
/// resend it. Use only when the host protocol expects a direct handoff.
pub fn ipc_reply_and_yield(
    endpoint: ProcessEndpoint,
    message: &[u8],
) -> Result<(), crate::ipc::ReplyError> {
    crate::ipc::send_then_handoff(|| ipc_send(endpoint, message), || yield_to(endpoint))
}

/// Exit this process; resources are reclaimed by its existing kernel lifecycle.
pub fn process_exit(code: u64) -> ! {
    // SAFETY: available only for the native freestanding target.
    unsafe { raw::exit(code) }
}

/// Send one nonempty message of at most MAX_IPC_MESSAGE_LEN bytes. The kernel
/// validates bounds and endpoint authority; no truncation, splitting or retry.
pub fn ipc_send(endpoint: ProcessEndpoint, message: &[u8]) -> isize {
    // SAFETY: native target; shared borrow lives through the copying trap.
    unsafe { raw::ipc_send(endpoint, message) }
}

/// Receive once with a full sender endpoint. A positive result is the number
/// of delivered bytes; only that prefix and sender are a newly received message.
/// Capacity must be 1..=MAX_IPC_MESSAGE_LEN. On an error, neither output is a new
/// message and previous contents must not be interpreted as one. MAILBOX_EMPTY,
/// TIMEOUT and RECEIVE_INTERRUPTED remain distinct; this call never retries or
/// acknowledges shutdown readiness. Authenticate the sender in the app protocol.
pub fn ipc_receive(
    mode: crate::ipc::ReceiveMode,
    buffer: &mut [u8],
    sender: &mut ProcessEndpoint,
) -> isize {
    // SAFETY: native target and exclusive borrows cover the entire blocked call.
    unsafe { raw::ipc_receive(mode, buffer, sender) }
}

/// Take one UI event without waiting. Only SYSCALL_OK means the output is a new
/// event; MAILBOX_EMPTY and other errors leave no new event to process. The
/// kernel restricts delivery to the calling owner. Match the event's full target
/// identity and source revision against retained bindings before applying it;
/// do not reinterpret an old row/action using the latest view's data.
pub fn ui_action_take(event: &mut crate::window::Event) -> isize {
    // SAFETY: native target; the kernel copies into the exclusive live output.
    unsafe { raw::ui_action_take(event) }
}

/// Submit one recipe without retry. The kernel validates size, format, hash,
/// owner and armed transaction. BUSY consumes nothing; callers choose their
/// retry/yield budget. A negative error does not produce a new status record.
/// Nonnegative results are broker channel statuses; only OK means accepted,
/// and acceptance is not proof that the compositor has presented the revision.
pub fn ui_content_submit(
    frame: &crate::window::UiDocumentWindowContentFrame,
    bytes: &[u8],
    status: &mut crate::window::UiDocumentWindowChannelStatusRecord,
) -> isize {
    // SAFETY: native target, immutable inputs and exclusive live status output.
    unsafe { raw::ui_content_submit(frame, bytes, status) }
}

/// Map or release one session page; returns the native transition status.
/// Negative errors provide no new receipt. Nonnegative outcomes require checking
/// the receipt, request ID/hash, session, owner and mapping identity. No retries,
/// automatic request-ID advancement or ownership of the returned address.
///
/// # Safety
/// Before RELEASE, end all page accesses/borrows and destroy its live values.
/// No other continuation may use the page after success. Control buffers must
/// reside in the registered process image/stack, outside all session mappings.
/// A returned address may only be dereferenced after successful MAP and complete
/// receipt validation, while that exact mapping remains owned and live.
pub unsafe fn session_memory_transition(
    request: &crate::session::ProcessSessionMemoryRequest,
    receipt: &mut crate::session::ProcessSessionMemoryReceipt,
) -> isize {
    // SAFETY: native target; caller explicitly maintains mapping lifetimes.
    unsafe { raw::session_memory_transition(request, receipt) }
}

/// Coalesced repaint request for this process's exact current window identity.
pub fn ui_view_invalidate(target: &osaiku_ui::Identity) -> isize {
    unsafe {
        raw::syscall(
            number::UI_VIEW_INVALIDATE,
            [target as *const _ as u64, 0, 0, 0, 0],
        ) as isize
    }
}
