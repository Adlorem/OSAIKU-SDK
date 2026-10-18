use osaiku_runtime::lifecycle::*;
use osaiku_runtime::session::*;

fn mapping() -> ProcessSessionMemoryReceipt {
    let mut mapping = ProcessSessionMemoryReceipt {
        abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
        status: PROCESS_SESSION_MEMORY_STATUS_OK,
        decision: PROCESS_SESSION_MEMORY_DECISION_MAPPED,
        memory_kind: PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE,
        result_flags: PROCESS_SESSION_MEMORY_RESULT_MAPPED_FLAGS,
        request_id: 1,
        request_hash: 2,
        session_key: 12,
        mapping_id: 21,
        mapping_generation: 1,
        user_virtual_address: 0x4ee0_0000,
        mapped_byte_len: 4096,
        owner_process_id: 5,
        address_space_generation: 1,
        committed_frame_count: 2,
        quota_frame_count: 32,
        transition_generation: 1,
        ..ProcessSessionMemoryReceipt::EMPTY
    };
    seal_mapping(&mut mapping);
    mapping
}

fn seal_mapping(mapping: &mut ProcessSessionMemoryReceipt) {
    mapping.mapping_hash = compute_process_session_memory_mapping_hash(
        mapping.owner_process_id,
        mapping.address_space_generation,
        mapping.session_key,
        mapping.mapping_id,
        mapping.mapping_generation,
        mapping.user_virtual_address,
        mapping.mapped_byte_len,
    );
    mapping.record_hash = compute_process_session_memory_receipt_hash(mapping);
}

#[test]
fn released_builder_preserves_receipt_bytes_and_never_mutates_inputs() {
    let notice = notice();
    let mapping = mapping();
    let old_notice = notice;
    let old_mapping = mapping;
    for remaining in [0, 1, u64::MAX] {
        let actual = WindowSessionTeardownReceipt::released(&notice, &mapping, remaining).unwrap();
        let mut expected = released();
        expected.released_mapping_hash = mapping.mapping_hash;
        expected.remaining_session_count = remaining;
        seal_receipt(&mut expected);
        assert_eq!(actual.encode(), expected.encode());
        assert!(window_session_teardown_receipt_canonical(&actual));
    }
    assert_eq!(notice, old_notice);
    assert_eq!(mapping, old_mapping);
}

#[test]
fn released_builder_rejects_foreign_or_corrupt_mapping_and_non_map_receipts() {
    let notice = notice();
    for field in 0..4 {
        let mut wrong = mapping();
        match field {
            0 => wrong.owner_process_id += 1,
            1 => wrong.session_key += 1,
            2 => wrong.record_hash ^= 1,
            _ => wrong.mapping_hash ^= 1,
        }
        if field < 2 {
            seal_mapping(&mut wrong);
            assert!(process_session_memory_receipt_canonical(&wrong));
        }
        assert!(WindowSessionTeardownReceipt::released(&notice, &wrong, 0).is_none());
    }
    let mut terminal = mapping();
    terminal.decision = PROCESS_SESSION_MEMORY_DECISION_RELEASED;
    terminal.result_flags = PROCESS_SESSION_MEMORY_RESULT_RELEASED_FLAGS;
    terminal.user_virtual_address = 0;
    terminal.mapped_byte_len = 0;
    terminal.record_hash = compute_process_session_memory_receipt_hash(&terminal);
    assert!(process_session_memory_receipt_canonical(&terminal));
    assert!(WindowSessionTeardownReceipt::released(&notice, &terminal, 0).is_none());
    let mut corrupt_notice = notice;
    corrupt_notice.notice_hash ^= 1;
    assert!(WindowSessionTeardownReceipt::released(&corrupt_notice, &mapping(), 0).is_none());
}

#[test]
fn refusal_builder_preserves_echoes_and_mapping_free_wire_contract() {
    let notice = notice();
    for (status, reason) in [
        (
            WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE,
            WINDOW_SESSION_TEARDOWN_REASON_INVALID_SHAPE,
        ),
        (
            WINDOW_SESSION_TEARDOWN_STATUS_STALE,
            WINDOW_SESSION_TEARDOWN_REASON_REPLAYED_NOTICE,
        ),
        (
            WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE,
            WINDOW_SESSION_TEARDOWN_REASON_IDENTITY_MISMATCH,
        ),
        (
            WINDOW_SESSION_TEARDOWN_STATUS_STALE,
            WINDOW_SESSION_TEARDOWN_REASON_UNKNOWN_SESSION,
        ),
    ] {
        let actual =
            WindowSessionTeardownReceipt::rejected(Some(&notice), status, reason, 2).unwrap();
        let mut expected = released();
        expected.status = status;
        expected.decision = WINDOW_SESSION_TEARDOWN_DECISION_REJECTED;
        expected.reason = reason;
        expected.released_mapping_id = 0;
        expected.released_mapping_generation = 0;
        expected.released_mapping_hash = 0;
        expected.remaining_session_count = 2;
        seal_receipt(&mut expected);
        assert_eq!(actual.encode(), expected.encode());
    }
}

#[test]
fn refusal_builder_handles_missing_echoes_without_truncating_invalid_pid() {
    for notice in [None, Some(WindowSessionTeardownNotice::EMPTY)] {
        let receipt = WindowSessionTeardownReceipt::rejected(notice.as_ref(), 1, 1, 7).unwrap();
        assert!(window_session_teardown_receipt_canonical(&receipt));
        assert_eq!(
            [
                receipt.notice_id,
                receipt.notice_hash,
                receipt.window_id,
                receipt.owner_app_id,
                receipt.owner_process_id,
                receipt.session_key
            ],
            [1; 6]
        );
        assert_eq!(receipt.remaining_session_count, 7);
    }
    let mut bad = notice();
    bad.owner_process_id = u32::MAX as u64 + 1;
    assert!(WindowSessionTeardownReceipt::rejected(Some(&bad), 1, 1, 0).is_none());
    assert!(WindowSessionTeardownReceipt::rejected(None, 0, 1, 0).is_none());
    assert!(WindowSessionTeardownReceipt::rejected(None, 1, 0, 0).is_none());
}

fn notice() -> WindowSessionTeardownNotice {
    let mut bytes = [0; 156];
    bytes[..16].copy_from_slice(&[2, 0, 0, 0, 1, 1, 0, 0, 7, 0, 0, 0, 15, 0, 0, 0]);
    for value in 1u64..=15 {
        let offset = 16 + (value as usize - 1) * 8;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    // Independent fieldwise little-endian FNV-1a vectors for ABI v2.
    bytes[136..144].copy_from_slice(&0xfe86_0b08_adc9_e301u64.to_le_bytes());
    bytes[144..152].copy_from_slice(&0x0283_f1dd_0093_16f0u64.to_le_bytes());
    let notice = WindowSessionTeardownNotice::decode(&bytes).unwrap();
    assert_eq!(notice.encode(), bytes);
    notice
}

fn released() -> WindowSessionTeardownReceipt {
    let mut bytes = [0; 120];
    bytes[..16].copy_from_slice(&[2, 0, 0, 0, 0, 1, 0, 1, 3, 0, 0, 0, 15, 0, 0, 0]);
    for (index, value) in [
        1u64,
        0x0283_f1dd_0093_16f0,
        2,
        4,
        5,
        12,
        21,
        1,
        0x5151,
        1,
        0x37e1_4f6c_6936_9412,
        0x9f1e_1bfe_b1fc_ca4e,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[16 + index * 8..24 + index * 8].copy_from_slice(&value.to_le_bytes());
    }
    let receipt = WindowSessionTeardownReceipt::decode(&bytes).unwrap();
    assert_eq!(receipt.encode(), bytes);
    receipt
}

fn seal_notice(notice: &mut WindowSessionTeardownNotice) {
    notice.provenance_hash = compute_window_session_teardown_notice_provenance_hash(notice);
    notice.notice_hash = compute_window_session_teardown_notice_hash(notice);
}

fn seal_receipt(receipt: &mut WindowSessionTeardownReceipt) {
    receipt.provenance_hash = compute_window_session_teardown_receipt_provenance_hash(receipt);
    receipt.receipt_hash = compute_window_session_teardown_receipt_hash(receipt);
}

fn owned_notice() -> WindowSessionTeardownNotice {
    let mut notice = notice();
    notice.session_key = osaiku_runtime::window::compute_window_document_id(
        notice.app_instance_id,
        notice.app_instance_generation,
        notice.surface_id,
        notice.surface_generation,
        notice.binding_generation,
        notice.binding_hash,
    );
    seal_notice(&mut notice);
    notice
}

#[test]
fn owner_validation_borrows_without_mutation_and_preserves_replay_order() {
    let good = owned_notice();
    let before = good.encode();
    let validated = validate_window_session_teardown_notice(Some(&good), 4, 5, 0).unwrap();
    assert!(core::ptr::eq(validated, &good));
    assert_eq!(good.encode(), before);
    for cursor in [good.notice_id, good.notice_id + 1, u64::MAX] {
        assert_eq!(
            validate_window_session_teardown_notice(Some(&good), 4, 5, cursor),
            Err(TeardownNoticeError::ReplayedNotice)
        );
    }
    let mut last = good;
    last.notice_id = u64::MAX;
    seal_notice(&mut last);
    assert!(validate_window_session_teardown_notice(Some(&last), 4, 5, u64::MAX - 1).is_ok());
    assert_eq!(
        validate_window_session_teardown_notice(Some(&last), 4, 5, u64::MAX),
        Err(TeardownNoticeError::ReplayedNotice)
    );
    // Shape is checked before replay, and replay before owner/key checks.
    let mut malformed = good;
    malformed.reserved0 = 1;
    seal_notice(&mut malformed);
    assert_eq!(
        validate_window_session_teardown_notice(Some(&malformed), 99, 99, u64::MAX),
        Err(TeardownNoticeError::InvalidShape)
    );
    assert_eq!(
        validate_window_session_teardown_notice(None, 4, 5, 0),
        Err(TeardownNoticeError::InvalidShape)
    );
    assert_eq!(
        validate_window_session_teardown_notice(Some(&good), 99, 99, good.notice_id),
        Err(TeardownNoticeError::ReplayedNotice)
    );
}

#[test]
fn owner_validation_rejects_rehashed_misdirection_and_key_substitution() {
    let good = owned_notice();
    for (app, pid) in [(0, 5), (4, 0), (99, 5), (4, 99), (4, (1 << 32) | 5)] {
        assert_eq!(
            validate_window_session_teardown_notice(Some(&good), app, pid, 0),
            Err(TeardownNoticeError::IdentityMismatch)
        );
    }
    for field in 0..9 {
        let mut changed = good;
        match field {
            0 => changed.owner_app_id += 1,
            1 => changed.owner_process_id += 1,
            2 => changed.app_instance_id += 1,
            3 => changed.app_instance_generation += 1,
            4 => changed.surface_id += 1,
            5 => changed.surface_generation += 1,
            6 => changed.binding_generation += 1,
            7 => changed.binding_hash += 1,
            _ => changed.session_key += 1,
        }
        seal_notice(&mut changed);
        assert!(window_session_teardown_notice_canonical(&changed));
        assert_eq!(
            validate_window_session_teardown_notice(Some(&changed), 4, 5, 0),
            Err(TeardownNoticeError::IdentityMismatch)
        );
    }
    let mut unknown_session = good;
    unknown_session.surface_generation += 1;
    unknown_session.session_key = osaiku_runtime::window::compute_window_document_id(
        unknown_session.app_instance_id,
        unknown_session.app_instance_generation,
        unknown_session.surface_id,
        unknown_session.surface_generation,
        unknown_session.binding_generation,
        unknown_session.binding_hash,
    );
    seal_notice(&mut unknown_session);
    // A self-consistent identity can pass preflight without naming a live local
    // session. The application must still locate its exact stored identity.
    assert!(validate_window_session_teardown_notice(Some(&unknown_session), 4, 5, 0).is_ok());
}

#[test]
fn refusal_codes_preserve_the_answered_notice_cursor_policy() {
    for (error, status, reason, consume) in [
        (
            TeardownNoticeError::InvalidShape,
            WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE,
            WINDOW_SESSION_TEARDOWN_REASON_INVALID_SHAPE,
            false,
        ),
        (
            TeardownNoticeError::ReplayedNotice,
            WINDOW_SESSION_TEARDOWN_STATUS_STALE,
            WINDOW_SESSION_TEARDOWN_REASON_REPLAYED_NOTICE,
            false,
        ),
        (
            TeardownNoticeError::IdentityMismatch,
            WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE,
            WINDOW_SESSION_TEARDOWN_REASON_IDENTITY_MISMATCH,
            true,
        ),
    ] {
        assert_eq!(
            (error.status(), error.reason(), error.consumes_notice_id()),
            (status, reason, consume)
        );
    }
}

#[test]
fn explicit_wire_sizes_hashes_and_corruption_detection_are_preserved() {
    let notice = notice();
    let receipt = released();
    assert_eq!(WINDOW_SESSION_TEARDOWN_NOTICE_SIZE, 156);
    assert_eq!(WINDOW_SESSION_TEARDOWN_RECEIPT_SIZE, 120);
    assert_eq!(core::mem::size_of::<WindowSessionTeardownNotice>(), 160);
    assert_eq!(core::mem::size_of::<WindowSessionTeardownReceipt>(), 120);
    assert_eq!(
        core::mem::offset_of!(WindowSessionTeardownNotice, reserved1),
        152
    );
    assert!(window_session_teardown_notice_canonical(&notice));
    assert!(window_session_teardown_receipt_canonical(&receipt));
    assert_eq!(
        compute_window_session_teardown_identity_hash(&notice),
        0xa742_fb9a_d664_6aa9
    );
    for len in [0, 155, 157, 160] {
        assert!(WindowSessionTeardownNotice::decode(&[0; 160][..len]).is_none());
    }
    for len in [0, 119, 121, 160] {
        assert!(WindowSessionTeardownReceipt::decode(&[0; 160][..len]).is_none());
    }
    for offset in 0..156 {
        let mut bytes = notice.encode();
        bytes[offset] ^= 1;
        assert!(!window_session_teardown_notice_canonical(
            &WindowSessionTeardownNotice::decode(&bytes).unwrap()
        ));
    }
    for offset in 0..120 {
        let mut bytes = receipt.encode();
        bytes[offset] ^= 1;
        assert!(!window_session_teardown_receipt_canonical(
            &WindowSessionTeardownReceipt::decode(&bytes).unwrap()
        ));
    }
}

#[test]
fn notice_shape_checks_do_not_authenticate_or_resolve_a_session() {
    for pid in [0, 1, u32::MAX as u64, u32::MAX as u64 + 1] {
        let mut notice = notice();
        notice.owner_process_id = pid;
        seal_notice(&mut notice);
        assert_eq!(
            window_session_teardown_notice_canonical(&notice),
            (1..=u32::MAX as u64).contains(&pid)
        );
    }
    for field in 0..8 {
        let mut notice = notice();
        match field {
            0 => notice.reserved0 = 1,
            1 => notice.reserved1[3] = 1,
            2 => notice.notice_flags |= 1 << 31,
            3 => notice.authority_flags = 0,
            4 => notice.notice_id = 0,
            5 => notice.window_generation = 0,
            6 => notice.binding_hash = 0,
            _ => notice.operation = 0,
        }
        seal_notice(&mut notice);
        assert!(!window_session_teardown_notice_canonical(&notice));
    }
    let mut forged = notice();
    forged.owner_app_id += 1;
    forged.session_key += 1;
    forged.notice_id += 1;
    seal_notice(&mut forged);
    // Anyone can recompute these hashes. The caller must authenticate IPC,
    // compare trusted local identity and derived key, and reject replay IDs.
    assert!(window_session_teardown_notice_canonical(&forged));
}

#[test]
fn release_and_refusal_have_distinct_mapping_claims() {
    for field in 0..3 {
        let mut receipt = released();
        match field {
            0 => receipt.released_mapping_id = 0,
            1 => receipt.released_mapping_generation = 0,
            _ => receipt.released_mapping_hash = 0,
        }
        seal_receipt(&mut receipt);
        assert!(!window_session_teardown_receipt_canonical(&receipt));
    }
    let mut rejected = WindowSessionTeardownReceipt {
        status: WINDOW_SESSION_TEARDOWN_STATUS_STALE,
        decision: WINDOW_SESSION_TEARDOWN_DECISION_REJECTED,
        reason: WINDOW_SESSION_TEARDOWN_REASON_REPLAYED_NOTICE,
        released_mapping_id: 0,
        released_mapping_generation: 0,
        released_mapping_hash: 0,
        ..released()
    };
    seal_receipt(&mut rejected);
    assert!(window_session_teardown_receipt_canonical(&rejected));
    for field in 0..5 {
        let mut receipt = rejected;
        match field {
            0 => receipt.released_mapping_id = 21,
            1 => receipt.released_mapping_generation = 1,
            2 => receipt.released_mapping_hash = 0x5151,
            3 => receipt.status = WINDOW_SESSION_TEARDOWN_STATUS_OK,
            _ => receipt.reason = WINDOW_SESSION_TEARDOWN_REASON_NONE,
        }
        seal_receipt(&mut receipt);
        assert!(!window_session_teardown_receipt_canonical(&receipt));
    }
}
