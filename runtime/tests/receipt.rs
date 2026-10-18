use osaiku_runtime::window::*;

fn committed() -> UiDocumentWindowPresentationReceipt {
    let mut bytes = [0; 160];
    bytes[..16].copy_from_slice(&[
        0x55, 0x44, 0x4f, 0x43, 10, 0, 0, 0, 0, 2, 0, 26, 255, 0, 0, 0,
    ]);
    for value in 1u64..=16 {
        let offset = 16 + (value as usize - 1) * 8;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[128..136].copy_from_slice(&39u64.to_le_bytes());
    // Independent little-endian/FNV vectors for the existing v2 hash domains.
    bytes[144..152].copy_from_slice(&0xb965_58ef_6f26_8180u64.to_le_bytes());
    bytes[152..160].copy_from_slice(&0x9aac_c6fc_303b_b562u64.to_le_bytes());
    let receipt = UiDocumentWindowPresentationReceipt::decode(&bytes).unwrap();
    assert_eq!(receipt.encode(), bytes);
    receipt
}

fn seal(receipt: &mut UiDocumentWindowPresentationReceipt) {
    receipt.provenance_hash = compute_ui_document_window_presentation_provenance_hash(receipt);
    receipt.record_hash = compute_ui_document_window_presentation_receipt_hash(receipt);
}

#[test]
fn receipt_wire_and_hashes_preserve_the_existing_contract() {
    let receipt = committed();
    assert_eq!(
        core::mem::size_of::<UiDocumentWindowPresentationReceipt>(),
        160
    );
    assert_eq!(
        core::mem::align_of::<UiDocumentWindowPresentationReceipt>(),
        8
    );
    assert_eq!(
        core::mem::offset_of!(UiDocumentWindowPresentationReceipt, source_request_hash),
        16
    );
    assert_eq!(
        core::mem::offset_of!(UiDocumentWindowPresentationReceipt, record_hash),
        152
    );
    assert_eq!(receipt.source_frame_hash, 2);
    assert_eq!(receipt.window_id, 6);
    assert_eq!(receipt.document_revision, 9);
    assert_eq!(receipt.final_visual_generation, 39);
    assert!(ui_document_window_presentation_receipt_canonical(&receipt));
    for len in [0, 159, 161] {
        assert!(UiDocumentWindowPresentationReceipt::decode(&[0; 161][..len]).is_none());
    }
    for offset in 0..160 {
        let mut bytes = receipt.encode();
        bytes[offset] ^= 1;
        let changed = UiDocumentWindowPresentationReceipt::decode(&bytes).unwrap();
        assert!(
            !ui_document_window_presentation_receipt_canonical(&changed),
            "byte {offset}"
        );
    }
}

#[test]
fn committed_generation_range_rejects_overflow_instead_of_accepting_zero() {
    for count in [UI_DOCUMENT_WINDOW_TILE_COUNT, UI_DOCUMENT_WINDOW_CELL_COUNT] {
        let mut receipt = committed();
        receipt.commit_count = count as u8;
        receipt.first_visual_generation = u64::MAX - (count as u64 - 1);
        receipt.final_visual_generation = u64::MAX;
        seal(&mut receipt);
        assert!(ui_document_window_presentation_receipt_canonical(&receipt));
        receipt.first_visual_generation += 1;
        receipt.final_visual_generation = 0;
        seal(&mut receipt);
        assert!(!ui_document_window_presentation_receipt_canonical(&receipt));
        receipt.first_visual_generation = u64::MAX;
        seal(&mut receipt);
        assert!(!ui_document_window_presentation_receipt_canonical(&receipt));
    }
    for count in [0, 1, 25, 27, 51, 53, 255] {
        let mut receipt = committed();
        receipt.commit_count = count;
        receipt.final_visual_generation = receipt.first_visual_generation + count as u64;
        seal(&mut receipt);
        assert!(!ui_document_window_presentation_receipt_canonical(&receipt));
    }
}

#[test]
fn prepared_and_rejected_records_cannot_claim_committed_generations() {
    let mut prepared = UiDocumentWindowPresentationReceipt {
        magic: UI_DOCUMENT_WINDOW_MAGIC,
        abi_version: UI_DOCUMENT_WINDOW_ABI_VERSION,
        status: UI_DOCUMENT_WINDOW_RECEIPT_STATUS_OK,
        presentation_state: UI_DOCUMENT_WINDOW_RECEIPT_STATE_PREPARED,
        receipt_flags: UI_DOCUMENT_WINDOW_RECEIPT_FLAGS_CANONICAL,
        source_request_hash: 23,
        ..UiDocumentWindowPresentationReceipt::EMPTY
    };
    seal(&mut prepared);
    assert!(ui_document_window_presentation_receipt_canonical(&prepared));
    let mut invalid = prepared;
    invalid.source_frame_hash = 1;
    seal(&mut invalid);
    assert!(!ui_document_window_presentation_receipt_canonical(&invalid));
    for (status, reason) in [
        (
            UI_DOCUMENT_WINDOW_RECEIPT_STATUS_INVALID_REQUEST,
            UI_DOCUMENT_WINDOW_RECEIPT_REASON_INVALID_REQUEST,
        ),
        (
            UI_DOCUMENT_WINDOW_RECEIPT_STATUS_INVALID_REQUEST,
            UI_DOCUMENT_WINDOW_RECEIPT_REASON_MANIFEST_MISMATCH,
        ),
        (
            UI_DOCUMENT_WINDOW_RECEIPT_STATUS_INVALID_REQUEST,
            UI_DOCUMENT_WINDOW_RECEIPT_REASON_SEQUENCE_MISMATCH,
        ),
        (
            UI_DOCUMENT_WINDOW_RECEIPT_STATUS_NO_PENDING,
            UI_DOCUMENT_WINDOW_RECEIPT_REASON_NO_PENDING,
        ),
        (
            UI_DOCUMENT_WINDOW_RECEIPT_STATUS_STALE_REQUEST,
            UI_DOCUMENT_WINDOW_RECEIPT_REASON_STALE_PENDING,
        ),
        (
            UI_DOCUMENT_WINDOW_RECEIPT_STATUS_KERNEL_REJECTED,
            UI_DOCUMENT_WINDOW_RECEIPT_REASON_KERNEL_REJECTED,
        ),
    ] {
        let mut rejected = prepared;
        rejected.presentation_state = UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED;
        rejected.status = status;
        rejected.reason = reason;
        seal(&mut rejected);
        assert!(ui_document_window_presentation_receipt_canonical(&rejected));
        rejected.final_visual_generation = 1;
        seal(&mut rejected);
        assert!(!ui_document_window_presentation_receipt_canonical(
            &rejected
        ));
    }
}

#[test]
fn canonical_metadata_is_not_authenticated_presentation() {
    let receipt = committed();
    for offset in [
        0, 4, 12, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 112, 120, 136,
    ] {
        let mut bytes = receipt.encode();
        let width = match offset {
            0 | 4 => 4,
            12 => 2,
            _ => 8,
        };
        bytes[offset..offset + width].fill(0);
        let mut changed = UiDocumentWindowPresentationReceipt::decode(&bytes).unwrap();
        seal(&mut changed);
        assert!(
            !ui_document_window_presentation_receipt_canonical(&changed),
            "field {offset}"
        );
    }
    let mut successor = receipt;
    successor.window_id += 1;
    successor.document_revision += 1;
    seal(&mut successor);
    // Well formed does not establish that this is the caller's actual window.
    // OS frame/manifest checks and kernel sequence authentication are required.
    assert!(ui_document_window_presentation_receipt_canonical(
        &successor
    ));
    assert_ne!(successor.provenance_hash, receipt.provenance_hash);
    assert_ne!(successor.record_hash, receipt.record_hash);
}
