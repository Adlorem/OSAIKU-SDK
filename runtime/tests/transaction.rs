use osaiku_runtime::window::*;

fn request() -> UiDocumentWindowPresentRequest {
    let mut request = UiDocumentWindowPresentRequest {
        magic: UI_DOCUMENT_WINDOW_MAGIC,
        abi_version: UI_DOCUMENT_WINDOW_ABI_VERSION,
        request_state: UI_DOCUMENT_WINDOW_REQUEST_STATE_PREPARED,
        request_kind: UI_DOCUMENT_WINDOW_REQUEST_KIND_PRESENT,
        request_flags: UI_DOCUMENT_WINDOW_REQUEST_FLAGS_CANONICAL,
        request_id: 1,
        expected_app_id: 2,
        expected_app_instance_id: 3,
        expected_app_instance_generation: 4,
        expected_window_id: 5,
        expected_window_generation: 6,
        expected_surface_id: 7,
        expected_surface_generation: 8,
        expected_binding_generation: 9,
        expected_binding_hash: 10,
        expected_binding_record_hash: 11,
        expected_document_id: compute_window_document_id(3, 4, 7, 8, 9, 10),
        expected_document_revision: 13,
        expected_editor_generation: 14,
        expected_semantic_state_generation: 14,
        expected_body_hash: compute_window_body_hash(&[]),
        expected_editor_state_hash: 17,
        ..UiDocumentWindowPresentRequest::EMPTY
    };
    seal(&mut request);
    request
}

fn seal(request: &mut UiDocumentWindowPresentRequest) {
    request.request_hash = compute_ui_document_window_present_request_hash(request);
}

use osaiku_runtime::process::{ProcessEndpoint, ProcessId};

fn owner() -> ProcessEndpoint {
    ProcessEndpoint::new(ProcessId(17), 29)
}

fn seal_status(status: &mut UiDocumentWindowChannelStatusRecord) {
    status.transition_provenance_hash =
        compute_ui_document_window_channel_transition_provenance_hash(status);
    status.record_hash = compute_ui_document_window_channel_status_record_hash(status);
}

fn submitted() -> UiDocumentWindowChannelStatusRecord {
    let request = request();
    let mut status = UiDocumentWindowChannelStatusRecord {
        magic: UI_DOCUMENT_WINDOW_MAGIC,
        abi_version: UI_DOCUMENT_WINDOW_ABI_VERSION,
        channel_state: UI_DOCUMENT_WINDOW_CHANNEL_STATE_SUBMITTED,
        status: UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK,
        reason: UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE,
        profile: UI_DOCUMENT_WINDOW_PROFILE_NORMAL,
        channel_flags: UI_DOCUMENT_WINDOW_CHANNEL_FLAGS_CANONICAL,
        broker_generation: 6,
        request_id: request.request_id,
        request_hash: request.request_hash,
        arm_generation: 5,
        submit_generation: 6,
        initd_process_id: 1,
        document_app_process_id: 17,
        document_app_process_endpoint: owner().encode(),
        document_app_instance_id: request.expected_app_instance_id,
        next_public_generation: 37,
        renderer_process_id: 3,
        ..UiDocumentWindowChannelStatusRecord::EMPTY
    };
    seal_status(&mut status);
    status
}

#[test]
fn accepted_submission_matches_full_owner_and_request() {
    let status = submitted();
    assert!(status.matches_request(&request(), owner()));
    assert!(status.is_submitted_for(&request(), owner()));
    for other in [
        ProcessEndpoint::new(ProcessId(17), 30),
        ProcessEndpoint::new(ProcessId(18), 29),
        ProcessEndpoint::unbound(ProcessId(17)),
        ProcessEndpoint::new(ProcessId::KERNEL, 29),
    ] {
        assert!(!status.matches_request(&request(), other));
    }
    // A changed revision/window must not match even with valid new request hash.
    for field in 0..4 {
        let mut other = request();
        match field {
            0 => other.request_id += 1,
            1 => other.expected_window_generation += 1,
            2 => other.expected_document_revision += 1,
            _ => other.expected_app_instance_id += 1,
        }
        other.expected_document_id = compute_window_document_id(
            other.expected_app_instance_id,
            other.expected_app_instance_generation,
            other.expected_surface_id,
            other.expected_surface_generation,
            other.expected_binding_generation,
            other.expected_binding_hash,
        );
        seal(&mut other);
        assert!(ui_document_window_present_request_well_formed(&other));
        assert!(!status.matches_request(&other, owner()));
    }
}

#[test]
fn resealed_status_cannot_substitute_another_transaction_or_owner() {
    for field in 0..5 {
        let mut other = submitted();
        match field {
            0 => other.request_id += 1,
            1 => other.request_hash ^= 1,
            2 => other.document_app_process_id += 1,
            3 => {
                other.document_app_process_endpoint =
                    ProcessEndpoint::new(ProcessId(17), 30).encode()
            }
            _ => other.document_app_instance_id += 1,
        }
        seal_status(&mut other);
        assert!(ui_document_window_channel_status_record_canonical(&other));
        assert!(!other.is_submitted_for(&request(), owner()));
    }
}

#[test]
fn canonical_other_phases_match_metadata_but_are_not_submission_acknowledgments() {
    for state in [
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_ARMED,
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_TAKEN,
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_COMPLETED,
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_CANCELLED,
    ] {
        let mut other = submitted();
        other.channel_state = state;
        other.broker_generation = 10;
        match state {
            UI_DOCUMENT_WINDOW_CHANNEL_STATE_ARMED => other.submit_generation = 0,
            UI_DOCUMENT_WINDOW_CHANNEL_STATE_TAKEN => other.take_generation = 7,
            UI_DOCUMENT_WINDOW_CHANNEL_STATE_COMPLETED => {
                other.take_generation = 7;
                other.complete_generation = 8;
            }
            _ => {
                other.status = UI_DOCUMENT_WINDOW_CHANNEL_STATUS_DENIED;
                other.reason = UI_DOCUMENT_WINDOW_CHANNEL_REASON_CANCELLED;
                other.complete_generation = 8;
            }
        }
        seal_status(&mut other);
        assert!(ui_document_window_channel_status_record_canonical(&other));
        assert!(other.matches_request(&request(), owner()));
        assert!(!other.is_submitted_for(&request(), owner()));
    }
}

#[test]
fn corrupt_or_out_of_order_records_never_supply_a_submission() {
    let valid = submitted();
    for field in 0..7 {
        let mut other = valid;
        match field {
            0 => other.arm_generation = 0,
            1 => other.submit_generation = other.arm_generation,
            2 => other.take_generation = 7,
            3 => other.complete_generation = 8,
            4 => other.next_public_generation = 0,
            5 => other.channel_flags = 0,
            _ => other.profile = 0,
        }
        seal_status(&mut other);
        assert!(!other.is_submitted_for(&request(), owner()));
    }
    let mut corrupt = valid;
    corrupt.record_hash ^= 1;
    assert!(!corrupt.matches_request(&request(), owner()));
    let mut malformed = request();
    malformed.reserved = 1;
    seal(&mut malformed);
    let mut echo = valid;
    echo.request_hash = malformed.request_hash;
    seal_status(&mut echo);
    assert!(!echo.matches_request(&malformed, owner()));
}
