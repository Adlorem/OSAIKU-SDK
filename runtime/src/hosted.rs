//! Retained window-session metadata for a non-editor application hosted by the
//! existing OSAIKU shell. The host authenticates its sender before using these
//! operations. No shell authority, memory mapping or syscall is granted here.
use crate::{compat::*, lifecycle::*, window::*};
mod raster;
mod surface;
pub use raster::{HostedRaster, RasterError};

pub use crate::compat::{
    DocumentEditorCommand as WindowCommand, DocumentEditorReply as WindowReply,
};

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod native;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub use native::{run, run_with_surfaces};

/// A single-window application in an image admitted by the OS deployment policy.
/// State must fit one session page with host metadata. View scratch belongs in
/// bounded process-image storage, never a Document-sized stack temporary.
pub trait WindowApplication: Sized {
    const APP_ID: u64;
    fn new() -> Self;
    /// Poll bounded application-owned work, returning whether its view changed.
    fn poll(&mut self) -> bool {
        false
    }
    /// Relative receive interval while work is active; None sleeps until IPC.
    fn poll_interval_us(&self) -> Option<u64> {
        None
    }
    /// Opt-in bounded observation of this application's own presentation state.
    fn observe_window_state(&self) -> bool {
        false
    }
    /// Kernel-supplied geometry/theme/visibility; not an action authorization.
    fn window_state(&mut self, _state: crate::surface::WindowState) -> bool {
        false
    }
    /// One best-effort cancellation turn before host-authorized retirement.
    fn closing(&mut self) {}
    /// Optional asynchronous service traffic. The app must authenticate the
    /// kernel-supplied endpoint and correlate the entire response. Returning
    /// true consumes the packet; poll() reports any resulting view change.
    fn service_message(&mut self, _sender: crate::process::ProcessEndpoint, _bytes: &[u8]) -> bool {
        false
    }
    /// Events already match the owner and one of the two retained submissions.
    /// Interpret dynamic rows against the event's original revision, not new data.
    fn event(&mut self, event: &osaiku_ui::input::Event) -> bool;
    fn render(&mut self, bytes: &mut [u8]) -> Result<usize, osaiku_ui::Error>;
    /// Called only after the kernel accepts the rendered recipe. Bind its data
    /// snapshot to this exact identity and retain the preceding accepted one
    /// for queued events. Refused renders/submissions never call this method.
    /// Acceptance does not assert that the frame has reached the display.
    fn submitted(&mut self, _identity: osaiku_ui::Identity) {}
    /// Called after rendering, rasterization or content submission is refused.
    /// Discard staged application state; the prior accepted view stays current.
    fn submission_refused(&mut self) {}
    /// Private extension message from the authenticated lifecycle host. The
    /// sender is supplied by kernel IPC. A successful reply only acknowledges
    /// the extension; the host must request ordinary presentation separately.
    /// Return None for unsupported payloads; never grant authority from bytes.
    fn host_message(
        &mut self,
        _sender: crate::process::ProcessEndpoint,
        _bytes: &[u8],
        _reply: &mut [u8],
    ) -> Option<usize> {
        None
    }
    /// True only when all app-owned work/data permits shutdown preparation.
    fn prepare_shutdown(&mut self) -> bool;
}

pub struct WindowSession {
    state: DocumentEditorStateRecord,
}

impl WindowSession {
    /// Construct an empty, clean session for a valid host OPEN exchange.
    pub fn open(command: &WindowCommand) -> Result<Self, WindowReply> {
        let state = DocumentEditorStateRecord::canonical_unbound();
        if let Some((status, reason)) = classify_document_editor_command(command, &state) {
            return Err(reply(
                &state,
                command,
                status,
                DOCUMENT_EDITOR_DECISION_REJECTED,
                reason,
            ));
        }
        if command.command_kind != DOCUMENT_EDITOR_COMMAND_OPEN {
            return Err(reply(
                &state,
                command,
                DOCUMENT_EDITOR_STATUS_INVALID_REQUEST,
                DOCUMENT_EDITOR_DECISION_REJECTED,
                DOCUMENT_EDITOR_REASON_BAD_COMMAND_KIND,
            ));
        }
        let mut state = DocumentEditorStateRecord {
            editor_state: DOCUMENT_EDITOR_STATE_OPEN,
            source_flags: DOCUMENT_EDITOR_SOURCE_APP_LIFECYCLE_CANONICAL,
            boundary_flags: DOCUMENT_EDITOR_BOUNDARY_APP_LIFECYCLE_CANONICAL,
            app_instance_id: command.expected_app_instance_id,
            app_instance_generation: command.expected_app_instance_generation,
            surface_id: command.expected_surface_id,
            surface_generation: command.expected_surface_generation,
            binding_generation: command.expected_binding_generation,
            binding_hash: command.expected_binding_hash,
            document_id: compute_document_editor_document_id(
                command.expected_app_instance_id,
                command.expected_app_instance_generation,
                command.expected_surface_id,
                command.expected_surface_generation,
                command.expected_binding_generation,
                command.expected_binding_hash,
            ),
            document_revision: 1,
            editor_generation: 1,
            semantic_state_generation: 1,
            body_hash: compute_document_editor_body_hash(&[]),
            source_transition_hash: command.command_hash,
            ..state
        };
        state.record_hash = compute_document_editor_state_record_hash(&state);
        Ok(Self { state })
    }

    /// Metadata snapshot, not a copy of the application's state or its ownership.
    pub fn snapshot(&self) -> DocumentEditorStateRecord {
        self.state
    }

    pub fn session_key(&self) -> u64 {
        self.state.document_id
    }

    pub fn opened_reply(&self, command: &WindowCommand) -> WindowReply {
        reply(
            &self.state,
            command,
            DOCUMENT_EDITOR_STATUS_OK,
            DOCUMENT_EDITOR_DECISION_APPLIED,
            DOCUMENT_EDITOR_REASON_NONE,
        )
    }

    /// Check a host query before draining input or changing application data.
    pub fn validate_query(&self, command: &WindowCommand) -> Result<(), WindowReply> {
        if let Some((status, reason)) = classify_document_editor_command(command, &self.state) {
            return Err(reply(
                &self.state,
                command,
                status,
                DOCUMENT_EDITOR_DECISION_REJECTED,
                reason,
            ));
        }
        if command.command_kind != DOCUMENT_EDITOR_COMMAND_QUERY {
            return Err(reply(
                &self.state,
                command,
                DOCUMENT_EDITOR_STATUS_INVALID_REQUEST,
                DOCUMENT_EDITOR_DECISION_REJECTED,
                DOCUMENT_EDITOR_REASON_BAD_COMMAND_KIND,
            ));
        }
        if self.state.editor_generation == u64::MAX {
            return Err(reply(
                &self.state,
                command,
                DOCUMENT_EDITOR_STATUS_STATE_EXHAUSTED,
                DOCUMENT_EDITOR_DECISION_REJECTED,
                DOCUMENT_EDITOR_REASON_STATE_COUNTER_EXHAUSTED,
            ));
        }
        Ok(())
    }

    /// Answer an authenticated query; advance metadata only for a real view
    /// change. Invalid requests never mutate retained session metadata.
    pub fn query(&mut self, command: &WindowCommand, changed: bool) -> WindowReply {
        if let Err(error) = self.validate_query(command) {
            return error;
        }
        if changed && command.expected_editor_generation == 0 {
            self.state.editor_generation += 1;
            self.state.semantic_state_generation = self.state.editor_generation;
            self.state.source_transition_hash = command.command_hash;
            self.state.record_hash = compute_document_editor_state_record_hash(&self.state);
        }
        reply(
            &self.state,
            command,
            DOCUMENT_EDITOR_STATUS_OK,
            DOCUMENT_EDITOR_DECISION_READ_ONLY,
            DOCUMENT_EDITOR_REASON_NONE,
        )
    }

    pub fn matches_present(&self, request: &UiDocumentWindowPresentRequest) -> bool {
        request_matches_window_state(request, &self.state)
    }

    /// Match a validated notice against the retained session. The caller still
    /// validates owner/app identity and notice replay with the lifecycle helper.
    pub fn matches_teardown(&self, notice: &WindowSessionTeardownNotice) -> bool {
        let state = &self.state;
        notice.session_key == state.document_id
            && notice.app_instance_id == state.app_instance_id
            && notice.app_instance_generation == state.app_instance_generation
            && notice.surface_id == state.surface_id
            && notice.surface_generation == state.surface_generation
            && notice.binding_generation == state.binding_generation
            && notice.binding_hash == state.binding_hash
    }
}

fn reply(
    state: &DocumentEditorStateRecord,
    command: &WindowCommand,
    status: u8,
    decision: u8,
    reason: u8,
) -> WindowReply {
    let mut reply = WindowReply {
        abi_version: DOCUMENT_EDITOR_ABI_VERSION,
        status,
        decision,
        reason,
        command_kind: if (1..=6).contains(&command.command_kind) {
            command.command_kind
        } else {
            0
        },
        result_flags: document_editor_result_flags(decision),
        reserved: 0,
        source_command_hash: command.command_hash,
        state: *state,
        reply_hash: 0,
    };
    reply.reply_hash = compute_document_editor_reply_hash(&reply);
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open_command() -> DocumentEditorCommand {
        let mut command = DocumentEditorCommand {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            command_kind: DOCUMENT_EDITOR_COMMAND_OPEN,
            input_origin: DOCUMENT_EDITOR_INPUT_ORIGIN_NONE,
            encoding: DOCUMENT_EDITOR_ENCODING_ASCII,
            payload_len: 0,
            command_flags: DOCUMENT_EDITOR_COMMAND_FLAGS_TRANSPORT,
            reserved_header: [0; 4],
            command_id: 0x101,
            expected_app_instance_id: 0x201,
            expected_app_instance_generation: 1,
            expected_surface_id: 0x301,
            expected_surface_generation: 1,
            expected_binding_generation: 1,
            expected_binding_hash: 0x401,
            ..DocumentEditorCommand::EMPTY
        };
        command.command_hash = compute_document_editor_command_hash(&command);
        command
    }

    fn open_state() -> DocumentEditorStateRecord {
        let command = open_command();
        let mut state = DocumentEditorStateRecord {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            editor_state: DOCUMENT_EDITOR_STATE_OPEN,
            content_state: DOCUMENT_EDITOR_CONTENT_EMPTY,
            dirty_state: DOCUMENT_EDITOR_DIRTY_CLEAN,
            save_state: DOCUMENT_EDITOR_SAVE_NEVER_SAVED,
            encoding: DOCUMENT_EDITOR_ENCODING_ASCII,
            last_input_origin: DOCUMENT_EDITOR_LAST_INPUT_NONE,
            body_len: 0,
            body_capacity: DOCUMENT_EDITOR_BODY_CAPACITY as u8,
            source_flags: DOCUMENT_EDITOR_SOURCE_APP_LIFECYCLE_CANONICAL,
            semantic_flags: DOCUMENT_EDITOR_SEMANTIC_BASE,
            boundary_flags: DOCUMENT_EDITOR_BOUNDARY_APP_LIFECYCLE_CANONICAL,
            app_instance_id: command.expected_app_instance_id,
            app_instance_generation: command.expected_app_instance_generation,
            surface_id: command.expected_surface_id,
            surface_generation: command.expected_surface_generation,
            binding_generation: command.expected_binding_generation,
            binding_hash: command.expected_binding_hash,
            document_id: compute_document_editor_document_id(
                command.expected_app_instance_id,
                command.expected_app_instance_generation,
                command.expected_surface_id,
                command.expected_surface_generation,
                command.expected_binding_generation,
                command.expected_binding_hash,
            ),
            document_revision: 1,
            editor_generation: 1,
            semantic_state_generation: 1,
            body_hash: compute_document_editor_body_hash(&[]),
            source_transition_hash: command.command_hash,
            record_hash: 0,
        };
        state.record_hash = compute_document_editor_state_record_hash(&state);
        state
    }

    fn exact_query(state: &DocumentEditorStateRecord) -> DocumentEditorCommand {
        let mut command = DocumentEditorCommand {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            command_kind: DOCUMENT_EDITOR_COMMAND_QUERY,
            input_origin: DOCUMENT_EDITOR_INPUT_ORIGIN_NONE,
            encoding: DOCUMENT_EDITOR_ENCODING_ASCII,
            command_flags: DOCUMENT_EDITOR_COMMAND_FLAGS_TRANSPORT,
            command_id: 0x102,
            expected_app_instance_id: state.app_instance_id,
            expected_app_instance_generation: state.app_instance_generation,
            expected_surface_id: state.surface_id,
            expected_surface_generation: state.surface_generation,
            expected_binding_generation: state.binding_generation,
            expected_binding_hash: state.binding_hash,
            expected_document_id: state.document_id,
            expected_document_revision: state.document_revision,
            expected_editor_generation: state.editor_generation,
            expected_semantic_state_generation: state.semantic_state_generation,
            expected_body_hash: state.body_hash,
            ..DocumentEditorCommand::EMPTY
        };
        command.command_hash = compute_document_editor_command_hash(&command);
        command
    }

    #[test]
    fn armed_offer_checks_session_before_transport_and_never_retries() {
        use crate::{abi::*, process::*, surface::*};
        let window = WindowSession::open(&open_command()).unwrap_or_else(|_| panic!("open"));
        let state = window.snapshot();
        let mut request = UiDocumentWindowPresentRequest {
            magic: UI_DOCUMENT_WINDOW_MAGIC,
            abi_version: UI_DOCUMENT_WINDOW_ABI_VERSION,
            request_state: UI_DOCUMENT_WINDOW_REQUEST_STATE_PREPARED,
            request_kind: UI_DOCUMENT_WINDOW_REQUEST_KIND_PRESENT,
            request_flags: UI_DOCUMENT_WINDOW_REQUEST_FLAGS_CANONICAL,
            request_id: 41,
            expected_app_id: 7,
            expected_app_instance_id: state.app_instance_id,
            expected_app_instance_generation: state.app_instance_generation,
            expected_window_id: 9,
            expected_window_generation: 2,
            expected_surface_id: state.surface_id,
            expected_surface_generation: state.surface_generation,
            expected_binding_generation: state.binding_generation,
            expected_binding_hash: state.binding_hash,
            expected_binding_record_hash: 1,
            expected_document_id: state.document_id,
            expected_document_revision: state.document_revision,
            expected_editor_generation: state.editor_generation,
            expected_semantic_state_generation: state.semantic_state_generation,
            expected_body_hash: state.body_hash,
            expected_editor_state_hash: state.record_hash,
            ..UiDocumentWindowPresentRequest::EMPTY
        };
        request.request_hash = compute_ui_document_window_present_request_hash(&request);
        let owner = ProcessEndpoint::new(ProcessId(8), 3);
        assert!(window.matches_present(&request));
        for mode in 0..5 {
            let mut bad = request;
            let mut app = 7;
            let mut caller = owner;
            match mode {
                0 => bad.expected_document_revision += 1,
                1 => bad.expected_editor_state_hash += 1,
                2 => bad.request_hash ^= 1,
                3 => app = 8,
                _ => caller = ProcessEndpoint::unbound(owner.process_id),
            }
            if mode < 2 {
                bad.request_hash = compute_ui_document_window_present_request_hash(&bad);
            }
            assert_eq!(
                window.frame_offer_with(&bad, app, caller, |_, _, _| {
                    panic!("invalid session must not reach transport")
                }),
                Err(PublishError::Content(Refusal::Identity))
            );
        }
        for mode in 0..5 {
            let mut calls = 0;
            let result = window.frame_offer_with(&request, 7, owner, |req, bytes, receipt| {
                calls += 1;
                receipt.handle = req.handle;
                receipt.epoch = 6;
                if req.operation == QUERY {
                    assert!(bytes.is_empty());
                    match mode {
                        0 => return SYSCALL_ERR_BUSY,
                        1 => receipt.epoch = 0,
                        2 => receipt.handle.serial = 1,
                        _ => {}
                    }
                } else {
                    assert_eq!(req.operation, FRAME_OFFER);
                    assert_eq!(
                        req.handle.target,
                        Target {
                            owner: owner.encode(),
                            window: 9,
                            incarnation: 2,
                            epoch: 6,
                        }
                    );
                    assert_eq!(
                        (req.handle.revision, req.offset),
                        (41, request.request_hash)
                    );
                    if mode == 3 {
                        return SYSCALL_ERR_STALE_ENDPOINT;
                    }
                    let offer = FrameOffer {
                        geometry: GeometryOffer {
                            version: VERSION,
                            target: req.handle.target,
                            resize_generation: 1,
                            window_width: 2,
                            window_height: 3,
                            content: Rect {
                                x: 0,
                                y: 1,
                                width: 2,
                                height: 2,
                            },
                            frame_layout_hash: 1,
                            theme: 0,
                        },
                        request: RenderRequest {
                            request_id: 41,
                            request_hash: request.request_hash,
                            arm_generation: 4,
                            revision: 5,
                        },
                    };
                    unsafe {
                        core::ptr::write_unaligned(bytes.as_mut_ptr().cast::<FrameOffer>(), offer);
                    }
                }
                SYSCALL_OK
            });
            assert_eq!(calls, if mode < 3 { 1 } else { 2 });
            match mode {
                0 => assert_eq!(result, Err(PublishError::Transport(SYSCALL_ERR_BUSY))),
                1 | 2 => assert_eq!(result, Err(PublishError::Receipt)),
                3 => assert_eq!(
                    result,
                    Err(PublishError::Transport(SYSCALL_ERR_STALE_ENDPOINT))
                ),
                _ => assert_eq!(result.unwrap().request.revision, 5),
            }
        }
        assert_eq!(window.snapshot(), state);
    }

    #[test]
    fn open_matches_existing_host_contract_byte_for_byte() {
        let command = open_command();
        let window = WindowSession::open(&command).unwrap_or_else(|_| panic!("open"));
        assert_eq!(window.snapshot().encode(), open_state().encode());
        assert!(document_editor_reply_canonical(
            &window.opened_reply(&command)
        ));
        let mut corrupt = command;
        corrupt.expected_surface_generation += 1;
        assert!(WindowSession::open(&corrupt).is_err());
    }

    #[test]
    fn exact_queries_are_idempotent_and_refresh_consumes_one_change() {
        let mut window = WindowSession::open(&open_command()).unwrap_or_else(|_| panic!("open"));
        let exact = exact_query(&window.snapshot());
        let initial = window.snapshot();
        for changed in [false, true, true] {
            let reply = window.query(&exact, changed);
            assert_eq!(reply.status, DOCUMENT_EDITOR_STATUS_OK);
            assert_eq!(reply.state, initial);
            assert!(document_editor_reply_canonical(&reply));
        }
        let mut refresh = exact;
        refresh.expected_document_id = 0;
        refresh.expected_document_revision = 0;
        refresh.expected_editor_generation = 0;
        refresh.expected_semantic_state_generation = 0;
        refresh.expected_body_hash = 0;
        refresh.command_hash = compute_document_editor_command_hash(&refresh);
        let updated = window.query(&refresh, true);
        assert_eq!(updated.status, DOCUMENT_EDITOR_STATUS_OK);
        assert_eq!(
            updated.state.editor_generation,
            initial.editor_generation + 1
        );
        assert_eq!(window.query(&refresh, false).state, updated.state);
        let confirmation = exact_query(&updated.state);
        assert_eq!(window.query(&confirmation, true).state, updated.state);
        assert_ne!(
            window.query(&exact, false).status,
            DOCUMENT_EDITOR_STATUS_OK
        );
        assert_eq!(window.snapshot(), updated.state);
    }

    #[test]
    fn exhaustion_is_refused_before_application_event_mutation() {
        let mut window = WindowSession::open(&open_command()).unwrap_or_else(|_| panic!("open"));
        window.state.editor_generation = u64::MAX;
        window.state.semantic_state_generation = u64::MAX;
        window.state.record_hash = compute_document_editor_state_record_hash(&window.state);
        let before = window.snapshot().encode();
        let query = exact_query(&window.snapshot());
        let error = window.validate_query(&query).unwrap_err();
        assert_eq!(error.status, DOCUMENT_EDITOR_STATUS_STATE_EXHAUSTED);
        assert!(document_editor_reply_canonical(&error));
        window.query(&query, true);
        assert_eq!(window.snapshot().encode(), before);
    }
}
