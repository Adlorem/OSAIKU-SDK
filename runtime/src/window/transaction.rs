use super::*;
use crate::process::{ProcessEndpoint, ProcessId};

impl UiDocumentWindowChannelStatusRecord {
    /// Match canonical transaction metadata against a request and a trusted
    /// owner instance. This does not authenticate IPC, validate OS admission,
    /// select a successful phase or prove that any pixels have been presented.
    pub fn matches_request(
        &self,
        request: &UiDocumentWindowPresentRequest,
        owner: ProcessEndpoint,
    ) -> bool {
        owner.process_id != ProcessId::KERNEL
            && owner.names_instance()
            && ui_document_window_present_request_well_formed(request)
            && ui_document_window_channel_status_record_canonical(self)
            && self.request_id == request.request_id
            && self.request_hash == request.request_hash
            && self.document_app_process_id == u64::from(owner.process_id.0)
            && self.document_app_process_endpoint == owner.encode()
            && self.document_app_instance_id == request.expected_app_instance_id
    }

    /// Accept only the submitted phase for this request and owner. Canonical
    /// SUBMITTED requires successful admission, ordered arm/submit generations,
    /// a nonzero publication generation, and no take/complete generation yet.
    /// Call only after checking the submission syscall result or authenticating
    /// the IPC sender. The view may still be awaiting renderer/compositor work.
    pub fn is_submitted_for(
        &self,
        request: &UiDocumentWindowPresentRequest,
        owner: ProcessEndpoint,
    ) -> bool {
        self.channel_state == UI_DOCUMENT_WINDOW_CHANNEL_STATE_SUBMITTED
            && self.matches_request(request, owner)
    }
}

/// Match public request metadata to retained compatibility state; OS admission and sender checks are separate.
pub fn request_matches_window_state(
    request: &UiDocumentWindowPresentRequest,
    editor: &crate::compat::DocumentEditorStateRecord,
) -> bool {
    use crate::compat::*;
    ui_document_window_present_request_well_formed(request)
        && document_editor_state_record_canonical(editor)
        && editor.editor_state == DOCUMENT_EDITOR_STATE_OPEN
        && request.expected_app_instance_id == editor.app_instance_id
        && request.expected_app_instance_generation == editor.app_instance_generation
        && request.expected_surface_id == editor.surface_id
        && request.expected_surface_generation == editor.surface_generation
        && request.expected_binding_generation == editor.binding_generation
        && request.expected_binding_hash == editor.binding_hash
        && request.expected_document_id == editor.document_id
        && request.expected_document_revision == editor.document_revision
        && request.expected_editor_generation == editor.editor_generation
        && request.expected_semantic_state_generation == editor.semantic_state_generation
        && request.expected_body_hash == editor.body_hash
        && request.expected_editor_state_hash == editor.record_hash
}
