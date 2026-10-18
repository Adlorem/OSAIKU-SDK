use osaiku_runtime::window::*;

// ABI codec vectors, not admitted application transactions. Admission additionally
// requires authenticated lifecycle state and matching editor/binding provenance.
#[test]
fn request_wire_hash_and_decline_keep_the_original_protocol() {
    let mut wire = [0u8; 160];
    wire[..16].copy_from_slice(&[
        0x55, 0x44, 0x4f, 0x43, 10, 0, 0, 0, 1, 1, 127, 0, 0, 0, 0, 0,
    ]);
    for value in 1u64..=17 {
        let offset = 16 + (value as usize - 1) * 8;
        wire[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    // Independently packed and hashed with FNV-1a, including the original domain.
    let hash = 0x2e33_03da_fb49_5fc3u64;
    wire[152..].copy_from_slice(&hash.to_le_bytes());
    let request = UiDocumentWindowPresentRequest::decode(&wire).unwrap();
    assert_eq!(core::mem::size_of::<UiDocumentWindowPresentRequest>(), 160);
    assert_eq!(core::mem::align_of::<UiDocumentWindowPresentRequest>(), 8);
    assert_eq!(request.expected_app_instance_id, 3);
    assert_eq!(request.expected_window_id, 5);
    assert_eq!(request.expected_surface_generation, 8);
    assert_eq!(request.expected_editor_state_hash, 17);
    assert_eq!(request.encode(), wire);
    assert_eq!(
        compute_ui_document_window_present_request_hash(&request),
        hash
    );
    assert_eq!(request.request_hash, hash);
    for offset in 0..152 {
        let mut changed = wire;
        changed[offset] ^= 1;
        let changed = UiDocumentWindowPresentRequest::decode(&changed).unwrap();
        assert_ne!(
            compute_ui_document_window_present_request_hash(&changed),
            hash
        );
    }
    assert!(UiDocumentWindowPresentRequest::decode(&wire[..159]).is_none());
    assert!(UiDocumentWindowPresentRequest::decode(&[0; 161]).is_none());
    let decline = ui_content_declined(&request);
    assert_eq!(&decline[..8], b"OSUINO01");
    assert_eq!(&decline[8..16], &1u64.to_le_bytes());
    assert_eq!(&decline[16..], &hash.to_le_bytes());
    let mut next = request;
    next.request_id += 1;
    next.request_hash = compute_ui_document_window_present_request_hash(&next);
    assert_ne!(ui_content_declined(&next), decline);
}

#[test]
fn content_frame_keeps_inline_body_and_hash_order_beyond_ipc_limit() {
    let mut wire = [0u8; 168];
    wire[..24].copy_from_slice(&[
        0x55, 0x44, 0x4f, 0x43, 10, 0, 0, 0, 1, 1, 32, 1, 1, 0, 127, 63, 255, 255, 0, 0, 0, 0, 0, 0,
    ]);
    for value in 1u64..=12 {
        let offset = 24 + (value as usize - 1) * 8;
        wire[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    for index in 0..32 {
        wire[120 + index] = index as u8;
    }
    // Frame hashing deliberately folds app_view_hash before body, unlike encoding.
    let hash = 0xae57_df0d_3caf_d6c9u64;
    wire[160..].copy_from_slice(&hash.to_le_bytes());
    let frame = UiDocumentWindowContentFrame::decode(&wire).unwrap();
    assert_eq!(core::mem::size_of::<UiDocumentWindowContentFrame>(), 168);
    assert_eq!(core::mem::align_of::<UiDocumentWindowContentFrame>(), 8);
    assert_eq!(
        core::mem::offset_of!(UiDocumentWindowContentFrame, body),
        120
    );
    assert_eq!(
        core::mem::offset_of!(UiDocumentWindowContentFrame, record_hash),
        160
    );
    assert_eq!(frame.app_instance_id, 1);
    assert_eq!(frame.source_request_hash, 12);
    assert_eq!(frame.body[31], 31);
    assert_eq!(frame.encode(), wire);
    assert_eq!(
        compute_ui_document_window_content_frame_record_hash(&frame),
        hash
    );
    assert_eq!(frame.record_hash, hash);
    for offset in 0..160 {
        let mut changed = wire;
        changed[offset] ^= 1;
        let changed = UiDocumentWindowContentFrame::decode(&changed).unwrap();
        assert_ne!(
            compute_ui_document_window_content_frame_record_hash(&changed),
            hash
        );
    }
    for len in [0, 160, 167] {
        assert!(UiDocumentWindowContentFrame::decode(&wire[..len]).is_none());
    }
    assert!(UiDocumentWindowContentFrame::decode(&[0; 169]).is_none());
    assert_eq!(UiDocumentWindowContentFrame::EMPTY.encode(), [0; 168]);
    assert_eq!(osaiku_runtime::abi::number::UI_CONTENT_SUBMIT, 363);
}
