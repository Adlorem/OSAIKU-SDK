use super::*;
use crate::{abi::*, application::*, ipc::ReceiveMode, process::*, session::*, *};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use osaiku_ui::Identity;

struct Wire(UnsafeCell<[u8; osaiku_ui::MAX_WIRE]>);
// Only the process's unique, non-returning run() invocation accesses this buffer.
unsafe impl Sync for Wire {}
static WIRE: Wire = Wire(UnsafeCell::new([0; osaiku_ui::MAX_WIRE]));
static RUNNING: AtomicBool = AtomicBool::new(false);
static RASTER: HostedRaster = HostedRaster::new();
const INITD: ProcessId = ProcessId(1);

struct State<A> {
    window: WindowSession,
    app: A,
    current: Identity,
    previous: Identity,
    dirty: bool,
    model_changed: bool,
    visible: bool,
    next_observation: u64,
}

/// Run the current fixed-service window-host protocol. The image must already
/// be admitted for A::APP_ID; this cannot register a program or grant authority.
/// One process owns one session. All IPC senders and submitted status records
/// are checked; view preparation/refusal preserves the last accepted picture.
/// Protocol/transport corruption exits the process for kernel reclamation.
pub fn run<A: WindowApplication>() -> ! {
    run_impl::<A, true>()
}

/// Compatibility alias for the application-rendered publication path.
pub fn run_with_surfaces<A: WindowApplication>() -> ! {
    run_impl::<A, true>()
}

fn run_impl<A: WindowApplication, const SURFACES: bool>() -> ! {
    if A::APP_ID == 0
        || RUNNING.swap(true, Ordering::AcqRel)
        || core::mem::size_of::<State<A>>() > PROCESS_SESSION_MEMORY_PAGE_SIZE as usize
    {
        process_exit(40);
    }
    let owner = process_self().unwrap_or_else(|_| process_exit(41));
    let mut session: Option<SessionPage<State<A>>> = None;
    let mut memory_id = 0;
    let mut teardown_id = 0;
    loop {
        checkpoint(&mut session);
        let mut bytes = [0; MAX_IPC_MESSAGE_LEN];
        let mut sender = ProcessEndpoint::unbound(ProcessId::KERNEL);
        let mut mode = ReceiveMode::Blocking;
        if let Some(page) = session.as_mut() {
            let state = page.value_mut();
            if state.current.epoch != 0 && state.app.observe_window_state() {
                if let Ok(now) = monotonic_ms() {
                    if now >= state.next_observation {
                        state.next_observation = now.saturating_add(200);
                        let id = state.current;
                        let target = crate::surface::Target {
                            owner: id.owner,
                            window: id.window,
                            incarnation: id.incarnation,
                            epoch: id.epoch,
                        };
                        if let Ok(observation) = crate::surface::window_state_with(
                            target,
                            |req, bytes, receipt| unsafe {
                                crate::surface::transition(req, bytes, receipt)
                            },
                        ) {
                            state.visible = observation.visible != 0;
                            let changed = state.app.window_state(observation);
                            state.model_changed |= changed;
                            state.dirty |= changed;
                        }
                    }
                }
            }
            let changed = state.app.poll();
            state.model_changed |= changed;
            state.dirty |= changed;
            if state.visible && state.dirty && state.current.revision != 0 {
                let _ = ui_view_invalidate(&state.current);
            }
            if let Some(interval) = state
                .app
                .poll_interval_us()
                .or(state.dirty.then_some(50_000))
            {
                mode = ReceiveMode::Timed {
                    timeout_us: interval.clamp(1, 200_000),
                };
            }
        }
        let received = ipc_receive(mode, &mut bytes, &mut sender);
        if matches!(received, SYSCALL_ERR_TIMEOUT | SYSCALL_ERR_MAILBOX_EMPTY) {
            continue;
        }
        if received == crate::ipc::RECEIVE_INTERRUPTED as isize {
            continue;
        }
        if received <= 0 || received as usize > bytes.len() {
            process_exit(21);
        }
        if sender.process_id != INITD
            && session.as_mut().is_some_and(|p| {
                p.value_mut()
                    .app
                    .service_message(sender, &bytes[..received as usize])
            })
        {
            continue;
        }
        let message = ApplicationHost::FixedService(INITD)
            .classify(sender, &bytes[..received as usize])
            .unwrap_or_else(|_| process_exit(22));
        match message {
            ApplicationMessage::Report => send(sender, APPLICATION_REPLY_RUNNING, true),
            ApplicationMessage::Exit => {
                if let Some(mut page) = session.take() {
                    page.value_mut().app.closing();
                    release(page.retire(), &mut memory_id);
                }
                process_exit(0);
            }
            ApplicationMessage::Present(payload) => {
                let request = UiDocumentWindowPresentRequest::decode(payload)
                    .unwrap_or_else(|| process_exit(44));
                let state = session
                    .as_mut()
                    .unwrap_or_else(|| process_exit(44))
                    .value_mut();
                if request.expected_app_id != A::APP_ID || !state.window.matches_present(&request) {
                    process_exit(44);
                }
                // SAFETY: run's process-wide guard uniquely owns WIRE forever.
                let wire = unsafe { &mut *WIRE.0.get() };
                let len = state
                    .app
                    .render(wire)
                    .map_err(|error| log_present_failure(A::APP_ID, "render", &error))
                    .ok()
                    .filter(|len| *len > 0 && *len <= wire.len());
                let sealed = if SURFACES {
                    len.and_then(|len| {
                        let result = state
                            .window
                            .frame_offer_with(
                                &request,
                                A::APP_ID,
                                owner,
                                |req, bytes, receipt| unsafe {
                                    crate::surface::transition(req, bytes, receipt)
                                },
                            )
                            .map_err(RasterError::Publish)
                            .and_then(|offer| {
                                RASTER.publish_with_images(
                                    offer,
                                    &wire[..len],
                                    state.app.images(),
                                    |req, bytes, receipt| unsafe {
                                        crate::surface::transition(req, bytes, receipt)
                                    },
                                    || {
                                        let code = scheduler_yield();
                                        if code == SYSCALL_OK {
                                            Ok(())
                                        } else {
                                            Err(code)
                                        }
                                    },
                                )
                            });
                        if let Err(error) = &result {
                            log_present_failure(A::APP_ID, "raster", error);
                        }
                        match result {
                            Ok(handle) => Some(handle),
                            Err(
                                RasterError::Publish(
                                    crate::surface::PublishError::Receipt
                                    | crate::surface::PublishError::Cleanup { .. },
                                )
                                | RasterError::Checkpoint(_),
                            ) => process_exit(57),
                            Err(_) => None,
                        }
                    })
                } else {
                    None
                };
                let mut status = UiDocumentWindowChannelStatusRecord::EMPTY;
                let accepted = len
                    .filter(|_| !SURFACES || sealed.is_some())
                    .and_then(|len| {
                        let frame = UiDocumentWindowContentFrame::for_ui(&request, &wire[..len])?;
                        // Bound backpressure; a refusal leaves the previous view.
                        let mut result = SYSCALL_ERR_BUSY;
                        for _ in 0..8 {
                            result = ui_content_submit(&frame, &wire[..len], &mut status);
                            if result != SYSCALL_ERR_BUSY {
                                break;
                            }
                            if scheduler_yield() != SYSCALL_OK {
                                process_exit(49);
                            }
                        }
                        if result != UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK as isize {
                            log_present_failure(A::APP_ID, "submit", &result);
                        }
                        (result == UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK as isize).then_some(())
                    })
                    .is_some();
                clear(wire);
                if accepted {
                    if !status.is_submitted_for(&request, owner) {
                        process_exit(47);
                    }
                    state.previous = state.current;
                    state.current = Identity {
                        epoch: sealed.map_or(0, |h| h.target.epoch),
                        owner: owner.encode(),
                        window: request.expected_window_id,
                        incarnation: request.expected_window_generation,
                        revision: status.next_public_generation,
                    };
                    if let Some(handle) = sealed {
                        if (
                            handle.target.owner,
                            handle.target.window,
                            handle.target.incarnation,
                            handle.revision,
                        ) != (
                            state.current.owner,
                            state.current.window,
                            state.current.incarnation,
                            state.current.revision,
                        ) {
                            process_exit(47);
                        }
                    }
                    if state.previous.epoch != state.current.epoch {
                        state.previous = osaiku_ui::Action::EMPTY.target;
                    }
                    state.dirty = state.model_changed;
                    state.app.submitted(state.current);
                    send(sender, &status.encode(), false);
                } else {
                    state.app.submission_refused();
                    send(sender, &ui_content_declined(&request), false);
                }
            }
            ApplicationMessage::Teardown(payload) => {
                let notice = WindowSessionTeardownNotice::decode(payload);
                let validated = validate_window_session_teardown_notice(
                    notice.as_ref(),
                    A::APP_ID,
                    u64::from(owner.process_id.0),
                    teardown_id,
                );
                let receipt = match validated {
                    Err(error) => {
                        if error.consumes_notice_id() {
                            if let Some(notice) = notice.as_ref() {
                                teardown_id = notice.notice_id;
                            }
                        }
                        WindowSessionTeardownReceipt::rejected(
                            notice.as_ref(),
                            error.status(),
                            error.reason(),
                            u64::from(session.is_some()),
                        )
                    }
                    Ok(notice) => {
                        teardown_id = notice.notice_id;
                        if session
                            .as_ref()
                            .is_some_and(|p| p.value().window.matches_teardown(notice))
                        {
                            // Check cursor exhaustion before detaching the owner.
                            memory_id.checked_add(1).unwrap_or_else(|| process_exit(51));
                            session.as_mut().unwrap().value_mut().app.closing();
                            let mapping = session.take().unwrap().retire();
                            release(mapping, &mut memory_id);
                            WindowSessionTeardownReceipt::released(notice, &mapping, 0)
                        } else {
                            WindowSessionTeardownReceipt::rejected(
                                Some(notice),
                                WINDOW_SESSION_TEARDOWN_STATUS_STALE,
                                WINDOW_SESSION_TEARDOWN_REASON_UNKNOWN_SESSION,
                                u64::from(session.is_some()),
                            )
                        }
                    }
                }
                .unwrap_or_else(|| process_exit(54));
                send(sender, &receipt.encode(), true);
            }
            ApplicationMessage::Other(payload) if payload.len() == DOCUMENT_EDITOR_COMMAND_SIZE => {
                let command = WindowCommand::decode(payload).unwrap_or_else(|| process_exit(31));
                let response = if command.command_kind == DOCUMENT_EDITOR_COMMAND_OPEN
                    && session.is_none()
                {
                    match WindowSession::open(&command) {
                        Err(error) => error,
                        Ok(window) => {
                            let id = memory_id.checked_add(1).unwrap_or_else(|| process_exit(50));
                            let request = ProcessSessionMemoryRequest::map(
                                id,
                                window.session_key(),
                                core::mem::size_of::<State<A>>(),
                            )
                            .unwrap_or_else(|| process_exit(50));
                            let mut mapping = ProcessSessionMemoryReceipt::EMPTY;
                            // SAFETY: new MAP with live stack control records.
                            let result =
                                unsafe { session_memory_transition(&request, &mut mapping) };
                            if result == PROCESS_SESSION_MEMORY_STATUS_RESOURCE_UNAVAILABLE as isize
                                && mapping.matches_request(&request, u64::from(owner.process_id.0))
                                && mapping.status
                                    == PROCESS_SESSION_MEMORY_STATUS_RESOURCE_UNAVAILABLE
                                && mapping.decision == PROCESS_SESSION_MEMORY_DECISION_REJECTED
                            {
                                let reason = resource_reason(mapping.reason)
                                    .unwrap_or_else(|| process_exit(50));
                                reply(
                                    &DocumentEditorStateRecord::canonical_unbound(),
                                    &command,
                                    DOCUMENT_EDITOR_STATUS_RESOURCE_UNAVAILABLE,
                                    DOCUMENT_EDITOR_DECISION_REJECTED,
                                    reason,
                                )
                            } else {
                                let response = window.opened_reply(&command);
                                let value = State {
                                    window,
                                    app: A::new(),
                                    current: osaiku_ui::Action::EMPTY.target,
                                    previous: osaiku_ui::Action::EMPTY.target,
                                    dirty: false,
                                    model_changed: false,
                                    visible: true,
                                    next_observation: 0,
                                };
                                // SAFETY: private just-mapped page, no aliases; the handle
                                // remains outside it and the SDK checks metadata before access.
                                let page = unsafe {
                                    SessionPage::adopt(
                                        &request,
                                        &mapping,
                                        result,
                                        u64::from(owner.process_id.0),
                                        value,
                                    )
                                }
                                .unwrap_or_else(|_| process_exit(50));
                                session = Some(page);
                                memory_id = id;
                                response
                            }
                        }
                    }
                } else if let Some(page) = session.as_mut() {
                    let state = page.value_mut();
                    match state.window.validate_query(&command) {
                        Err(error) => error,
                        Ok(()) => {
                            // Exact reads confirm the observed snapshot. Collect input and
                            // advance it only on a wildcard refresh, never on confirmation.
                            let refresh = command.expected_editor_generation == 0;
                            let mut changed = refresh && state.model_changed;
                            for _ in 0..if refresh {
                                osaiku_ui::input::CAPACITY
                            } else {
                                0
                            } {
                                let mut event = osaiku_ui::input::Event::EMPTY;
                                let result = ui_action_take(&mut event);
                                if result == SYSCALL_ERR_MAILBOX_EMPTY {
                                    break;
                                }
                                if result != SYSCALL_OK {
                                    process_exit(55);
                                }
                                if event.target.owner != 0
                                    && (event.target == state.current
                                        || event.target == state.previous)
                                {
                                    changed |= state.app.event(&event);
                                }
                            }
                            let response = state.window.query(&command, changed);
                            if refresh && response.status == DOCUMENT_EDITOR_STATUS_OK {
                                state.model_changed = false;
                                state.dirty |= changed;
                            }
                            response
                        }
                    }
                } else {
                    let state = DocumentEditorStateRecord::canonical_unbound();
                    let (status, reason) = classify_document_editor_command(&command, &state)
                        .unwrap_or((
                            DOCUMENT_EDITOR_STATUS_NOT_OPEN,
                            DOCUMENT_EDITOR_REASON_EDITOR_NOT_OPEN,
                        ));
                    reply(
                        &state,
                        &command,
                        status,
                        DOCUMENT_EDITOR_DECISION_REJECTED,
                        reason,
                    )
                };
                send(sender, &response.encode(), false);
            }
            ApplicationMessage::Other(payload) => {
                let mut response = [0; MAX_IPC_MESSAGE_LEN];
                let length = session.as_mut().and_then(|p| {
                    p.value_mut()
                        .app
                        .host_message(sender, payload, &mut response)
                });
                match length.filter(|n| *n > 0 && *n <= response.len()) {
                    Some(n) => send(sender, &response[..n], true),
                    None => send(sender, APPLICATION_REPLY_INVALID_COMMAND, true),
                }
                clear(&mut response);
            }
        }
        clear(&mut bytes);
    }
}

fn log_present_failure(app: u64, stage: &str, error: &impl core::fmt::Debug) {
    use core::fmt::Write;
    let mut line = osaiku_ui::text_model::Text::EMPTY;
    if writeln!(
        line,
        "application.present declined app={app};stage={stage};error={error:?}"
    )
    .is_ok()
    {
        let _ = crate::log_write(line.as_str());
    }
}

fn send(target: ProcessEndpoint, bytes: &[u8], handoff: bool) {
    if crate::ipc::send_with_backpressure(|| ipc_send(target, bytes), scheduler_yield, monotonic_ms)
        != SYSCALL_OK
    {
        process_exit(32);
    }
    let result = if handoff {
        yield_to(target)
    } else {
        scheduler_yield()
    };
    if result != SYSCALL_OK {
        process_exit(33);
    }
}

fn release(mapping: ProcessSessionMemoryReceipt, cursor: &mut u64) {
    let id = cursor.checked_add(1).unwrap_or_else(|| process_exit(51));
    let request =
        ProcessSessionMemoryRequest::release(id, &mapping).unwrap_or_else(|| process_exit(51));
    let mut receipt = ProcessSessionMemoryReceipt::EMPTY;
    // SAFETY: caller has consumed/retired the page. Control records are on stack.
    let result = unsafe { session_memory_transition(&request, &mut receipt) };
    if result != PROCESS_SESSION_MEMORY_STATUS_OK as isize
        || !receipt.matches_request(&request, mapping.owner_process_id)
        || receipt.decision != PROCESS_SESSION_MEMORY_DECISION_RELEASED
    {
        process_exit(51);
    }
    *cursor = id;
}

fn checkpoint<A: WindowApplication>(session: &mut Option<SessionPage<State<A>>>) {
    let epoch = shutdown_prepare_epoch().unwrap_or_else(|_| process_exit(56));
    if let Some(epoch) = epoch {
        let ready = session
            .as_mut()
            .is_none_or(|p| p.value_mut().app.prepare_shutdown());
        if shutdown_prepare_reply(epoch, ready) != SYSCALL_OK {
            process_exit(56);
        }
    }
}

fn clear(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
}

fn resource_reason(reason: u8) -> Option<u8> {
    Some(match reason {
        PROCESS_SESSION_MEMORY_REASON_PROCESS_QUOTA => DOCUMENT_EDITOR_REASON_SESSION_PROCESS_QUOTA,
        PROCESS_SESSION_MEMORY_REASON_PROTECTED_RESERVE => {
            DOCUMENT_EDITOR_REASON_SESSION_PROTECTED_RESERVE
        }
        PROCESS_SESSION_MEMORY_REASON_TOTAL_EXHAUSTED => {
            DOCUMENT_EDITOR_REASON_SESSION_TOTAL_EXHAUSTED
        }
        PROCESS_SESSION_MEMORY_REASON_FRAGMENTED => DOCUMENT_EDITOR_REASON_SESSION_FRAGMENTED,
        PROCESS_SESSION_MEMORY_REASON_VIRTUAL_RANGE_EXHAUSTED => {
            DOCUMENT_EDITOR_REASON_SESSION_VIRTUAL_RANGE_EXHAUSTED
        }
        PROCESS_SESSION_MEMORY_REASON_MAPPING_FAILED => {
            DOCUMENT_EDITOR_REASON_SESSION_MAPPING_UNAVAILABLE
        }
        PROCESS_SESSION_MEMORY_REASON_TRANSITION_GENERATION_EXHAUSTED => {
            DOCUMENT_EDITOR_REASON_SESSION_TRANSITION_EXHAUSTED
        }
        _ => return None,
    })
}
