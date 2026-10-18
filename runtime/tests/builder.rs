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

#[test]
fn ui_frame_binds_exact_request_and_bounded_recipe_without_inline_body() {
    let request = request();
    assert!(ui_document_window_present_request_well_formed(&request));
    for len in [1, 160, osaiku_ui::MAX_WIRE] {
        let bytes = vec![0xab; len];
        let frame = UiDocumentWindowContentFrame::for_ui(&request, &bytes).unwrap();
        assert_eq!(frame.app_view_kind, 4);
        assert_eq!(frame.app_instance_id, request.expected_app_instance_id);
        assert_eq!(frame.window_id, request.expected_window_id);
        assert_eq!(frame.surface_id, request.expected_surface_id);
        assert_eq!(
            frame.surface_generation,
            request.expected_surface_generation
        );
        assert_eq!(frame.document_revision, request.expected_document_revision);
        assert_eq!(frame.editor_state_hash, request.expected_editor_state_hash);
        assert_eq!(frame.source_request_hash, request.request_hash);
        assert_eq!(frame.app_view_hash, osaiku_ui::wire_hash(&bytes));
        assert_eq!(
            frame.record_hash,
            compute_ui_document_window_content_frame_record_hash(&frame)
        );
        assert_eq!(
            (
                frame.body_len,
                frame.content_state,
                frame.dirty_state,
                frame.save_state
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(frame.body, [0; UI_DOCUMENT_WINDOW_BODY_CAPACITY]);
        assert_eq!(frame.assistant_report, 0);
        assert_eq!(frame.reserved_header, [0; 4]);
        let changed = vec![0xac; len];
        let next = UiDocumentWindowContentFrame::for_ui(&request, &changed).unwrap();
        assert_ne!(next.app_view_hash, frame.app_view_hash);
        assert_ne!(next.record_hash, frame.record_hash);
    }
    assert!(UiDocumentWindowContentFrame::for_ui(&request, &[]).is_none());
    assert!(
        UiDocumentWindowContentFrame::for_ui(&request, &vec![0; osaiku_ui::MAX_WIRE + 1]).is_none()
    );
}

#[test]
fn malformed_request_and_nonempty_editor_body_cannot_build_ui_frame() {
    let original = request();
    for offset in [
        0, 4, 8, 9, 10, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 136, 144,
    ] {
        let mut bytes = original.encode();
        let width = match offset {
            0 | 4 => 4,
            8 | 9 => 1,
            10 => 2,
            _ => 8,
        };
        bytes[offset..offset + width].fill(0);
        let mut bad = UiDocumentWindowPresentRequest::decode(&bytes).unwrap();
        seal(&mut bad);
        assert!(
            !ui_document_window_present_request_well_formed(&bad),
            "field {offset}"
        );
        assert!(UiDocumentWindowContentFrame::for_ui(&bad, b"wire").is_none());
    }
    let mut bad = original;
    bad.reserved = 1;
    seal(&mut bad);
    assert!(UiDocumentWindowContentFrame::for_ui(&bad, b"wire").is_none());
    bad = original;
    bad.expected_body_hash = compute_window_body_hash(b"private document");
    seal(&mut bad);
    assert!(ui_document_window_present_request_well_formed(&bad));
    assert!(UiDocumentWindowContentFrame::for_ui(&bad, b"wire").is_none());
    bad = original;
    bad.request_hash ^= 1;
    assert!(UiDocumentWindowContentFrame::for_ui(&bad, b"wire").is_none());
    bad = original;
    bad.expected_document_id ^= 1;
    seal(&mut bad);
    assert!(UiDocumentWindowContentFrame::for_ui(&bad, b"wire").is_none());
}

#[test]
fn metadata_validation_does_not_admit_an_app_or_parse_a_recipe() {
    let mut request = request();
    request.expected_app_id = u64::MAX;
    seal(&mut request);
    assert!(ui_document_window_present_request_well_formed(&request));
    let invalid_recipe = b"not a versioned UI document";
    assert!(osaiku_ui::Document::decode(invalid_recipe).is_err());
    // The builder hashes bounded bytes. Application authorization, armed-request
    // matching and full recipe validation must still happen at kernel admission.
    assert!(UiDocumentWindowContentFrame::for_ui(&request, invalid_recipe).is_some());
}
