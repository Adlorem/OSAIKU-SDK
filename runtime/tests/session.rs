use osaiku_runtime::session::*;

fn map_request() -> ProcessSessionMemoryRequest {
    ProcessSessionMemoryRequest {
        abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
        operation: PROCESS_SESSION_MEMORY_OPERATION_MAP,
        memory_kind: PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE,
        request_flags: PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL,
        requested_byte_len: 704,
        request_id: 1,
        session_key: 0x51,
        // Independently packed little-endian FNV-1a vector.
        request_hash: 0x755e_fa7e_4426_e17f,
        ..ProcessSessionMemoryRequest::EMPTY
    }
}

fn mapped() -> ProcessSessionMemoryReceipt {
    ProcessSessionMemoryReceipt {
        abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
        status: PROCESS_SESSION_MEMORY_STATUS_OK,
        decision: PROCESS_SESSION_MEMORY_DECISION_MAPPED,
        memory_kind: PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE,
        result_flags: PROCESS_SESSION_MEMORY_RESULT_MAPPED_FLAGS,
        request_id: 1,
        request_hash: map_request().request_hash,
        session_key: 0x51,
        mapping_id: 0x71,
        mapping_generation: 1,
        mapping_hash: 0xe230_b62f_fee2_294e,
        user_virtual_address: 0x4ee0_0000,
        mapped_byte_len: 4096,
        owner_process_id: 8,
        address_space_generation: 1,
        committed_frame_count: 2,
        quota_frame_count: 32,
        available_frame_count: 512,
        protected_reserve_frame_count: 64,
        transition_generation: 1,
        record_hash: 0x756f_dd84_47c5_f64b,
        ..ProcessSessionMemoryReceipt::EMPTY
    }
}

#[test]
fn request_builders_preserve_vectors_and_boundaries_without_truncation() {
    assert_eq!(
        ProcessSessionMemoryRequest::map(1, 0x51, 704),
        Some(map_request())
    );
    for size in [0, 1, 4096, 4097, usize::MAX] {
        assert_eq!(
            ProcessSessionMemoryRequest::map(1, 0x51, size).is_some(),
            (1..=4096).contains(&size)
        );
    }
    assert!(ProcessSessionMemoryRequest::map(0, 0x51, 704).is_none());
    assert!(ProcessSessionMemoryRequest::map(1, 0, 704).is_none());
    assert!(ProcessSessionMemoryRequest::map(u64::MAX, 0x51, 704).is_some());
    let mapping = mapped();
    let before = mapping;
    let release = ProcessSessionMemoryRequest::release(2, &mapping).unwrap();
    assert_eq!(mapping, before);
    assert!(process_session_memory_request_canonical(&release));
    assert_eq!(release.operation, PROCESS_SESSION_MEMORY_OPERATION_RELEASE);
    assert_eq!(release.requested_byte_len, 0);
    assert_eq!(release.session_key, mapping.session_key);
    assert_eq!(
        (
            release.expected_mapping_id,
            release.expected_mapping_generation,
            release.expected_mapping_hash
        ),
        (
            mapping.mapping_id,
            mapping.mapping_generation,
            mapping.mapping_hash
        )
    );
    assert!(ProcessSessionMemoryRequest::release(0, &mapping).is_none());
    let mut corrupt = mapping;
    corrupt.record_hash ^= 1;
    assert!(ProcessSessionMemoryRequest::release(2, &corrupt).is_none());
}

fn released(request: &ProcessSessionMemoryRequest) -> ProcessSessionMemoryReceipt {
    let mut receipt = ProcessSessionMemoryReceipt {
        decision: PROCESS_SESSION_MEMORY_DECISION_RELEASED,
        result_flags: PROCESS_SESSION_MEMORY_RESULT_RELEASED_FLAGS,
        request_id: request.request_id,
        request_hash: request.request_hash,
        user_virtual_address: 0,
        mapped_byte_len: 0,
        ..mapped()
    };
    receipt.record_hash = compute_process_session_memory_receipt_hash(&receipt);
    receipt
}

#[test]
fn map_receipt_matches_only_the_requested_transaction_and_trusted_owner() {
    let request = map_request();
    let receipt = mapped();
    assert!(receipt.matches_request(&request, 8));
    assert!(!receipt.matches_request(&request, 9));
    assert!(!receipt.matches_request(&request, (1 << 32) | 8));
    for field in 0..3 {
        let mut wrong = receipt;
        match field {
            0 => wrong.request_id += 1,
            1 => wrong.request_hash ^= 1,
            _ => wrong.owner_process_id += 1,
        }
        wrong.mapping_hash = compute_process_session_memory_mapping_hash(
            wrong.owner_process_id,
            wrong.address_space_generation,
            wrong.session_key,
            wrong.mapping_id,
            wrong.mapping_generation,
            wrong.user_virtual_address,
            wrong.mapped_byte_len,
        );
        wrong.record_hash = compute_process_session_memory_receipt_hash(&wrong);
        assert!(process_session_memory_receipt_canonical(&wrong));
        assert!(!wrong.matches_request(&request, 8));
    }
    let mut bad_request = request;
    bad_request.request_hash ^= 1;
    assert!(!receipt.matches_request(&bad_request, 8));
}

#[test]
fn release_requires_exact_mapping_and_matching_operation() {
    let request = ProcessSessionMemoryRequest::release(2, &mapped()).unwrap();
    let receipt = released(&request);
    assert!(receipt.matches_request(&request, 8));
    assert!(ProcessSessionMemoryRequest::release(3, &receipt).is_none());
    for field in 0..3 {
        let mut wrong = receipt;
        match field {
            0 => wrong.mapping_id += 1,
            1 => wrong.mapping_generation += 1,
            _ => wrong.mapping_hash ^= 1,
        }
        wrong.record_hash = compute_process_session_memory_receipt_hash(&wrong);
        assert!(process_session_memory_receipt_canonical(&wrong));
        assert!(!wrong.matches_request(&request, 8));
    }
    let mut wrong_operation = mapped();
    wrong_operation.request_id = request.request_id;
    wrong_operation.request_hash = request.request_hash;
    wrong_operation.record_hash = compute_process_session_memory_receipt_hash(&wrong_operation);
    assert!(!wrong_operation.matches_request(&request, 8));
    // Even matching echoes cannot turn a RELEASE result into a MAP result.
    assert!(!released(&map_request()).matches_request(&map_request(), 8));
}

#[test]
fn matching_resource_refusal_grants_no_mapping() {
    let request = map_request();
    let mut refused = ProcessSessionMemoryReceipt {
        status: PROCESS_SESSION_MEMORY_STATUS_RESOURCE_UNAVAILABLE,
        decision: PROCESS_SESSION_MEMORY_DECISION_REJECTED,
        reason: PROCESS_SESSION_MEMORY_REASON_PROCESS_QUOTA,
        result_flags: PROCESS_SESSION_MEMORY_RESULT_REJECTED_FLAGS,
        mapping_id: 0,
        mapping_generation: 0,
        mapping_hash: 0,
        user_virtual_address: 0,
        mapped_byte_len: 0,
        ..mapped()
    };
    refused.record_hash = compute_process_session_memory_receipt_hash(&refused);
    assert!(refused.matches_request(&request, 8));
    assert!(ProcessSessionMemoryRequest::release(2, &refused).is_none());
    refused.session_key += 1;
    refused.record_hash = compute_process_session_memory_receipt_hash(&refused);
    assert!(process_session_memory_receipt_canonical(&refused));
    assert!(!refused.matches_request(&request, 8));
}

#[test]
fn session_native_layout_and_fixed_hash_vectors_are_unchanged() {
    assert_eq!(
        osaiku_runtime::abi::number::PROCESS_SESSION_MEMORY_TRANSITION,
        334
    );
    assert_eq!(core::mem::size_of::<ProcessSessionMemoryRequest>(), 96);
    assert_eq!(core::mem::size_of::<ProcessSessionMemoryReceipt>(), 160);
    assert_eq!(core::mem::align_of::<ProcessSessionMemoryRequest>(), 8);
    assert_eq!(core::mem::align_of::<ProcessSessionMemoryReceipt>(), 8);
    assert_eq!(
        core::mem::offset_of!(ProcessSessionMemoryRequest, request_id),
        16
    );
    assert_eq!(
        core::mem::offset_of!(ProcessSessionMemoryRequest, request_hash),
        88
    );
    assert_eq!(
        core::mem::offset_of!(ProcessSessionMemoryReceipt, user_virtual_address),
        64
    );
    assert_eq!(
        core::mem::offset_of!(ProcessSessionMemoryReceipt, record_hash),
        152
    );
    assert!(process_session_memory_request_canonical(&map_request()));
    assert!(process_session_memory_receipt_canonical(&mapped()));
}

#[test]
fn map_and_release_shapes_preserve_page_bounds_and_exact_mapping() {
    for length in [0, 1, 4096, 4097, u32::MAX] {
        let mut request = map_request();
        request.requested_byte_len = length;
        request.request_hash = compute_process_session_memory_request_hash(&request);
        assert_eq!(
            process_session_memory_request_canonical(&request),
            (1..=4096).contains(&length)
        );
    }
    let receipt = mapped();
    let mut release = ProcessSessionMemoryRequest {
        operation: PROCESS_SESSION_MEMORY_OPERATION_RELEASE,
        requested_byte_len: 0,
        request_id: 2,
        expected_mapping_id: receipt.mapping_id,
        expected_mapping_generation: receipt.mapping_generation,
        expected_mapping_hash: receipt.mapping_hash,
        ..map_request()
    };
    release.request_hash = compute_process_session_memory_request_hash(&release);
    assert!(process_session_memory_request_canonical(&release));
    for field in 0..5 {
        let mut bad = release;
        match field {
            0 => bad.expected_mapping_id = 0,
            1 => bad.expected_mapping_generation = 0,
            2 => bad.expected_mapping_hash = 0,
            3 => bad.requested_byte_len = 1,
            _ => bad.reserved1[3] = 1,
        }
        bad.request_hash = compute_process_session_memory_request_hash(&bad);
        assert!(!process_session_memory_request_canonical(&bad));
    }
}

#[test]
fn receipt_rejects_identity_drift_and_release_carries_no_live_address() {
    let good = mapped();
    for field in 0..7 {
        let mut changed = good;
        match field {
            0 => changed.owner_process_id += 1,
            1 => changed.address_space_generation += 1,
            2 => changed.session_key += 1,
            3 => changed.mapping_id += 1,
            4 => changed.mapping_generation += 1,
            5 => changed.user_virtual_address += 4096,
            _ => changed.mapped_byte_len += 4096,
        }
        // Rehashing the outer record cannot repair stale mapping provenance.
        changed.record_hash = compute_process_session_memory_receipt_hash(&changed);
        assert!(!process_session_memory_receipt_canonical(&changed));
    }
    let mut released = good;
    released.decision = PROCESS_SESSION_MEMORY_DECISION_RELEASED;
    released.result_flags = PROCESS_SESSION_MEMORY_RESULT_RELEASED_FLAGS;
    released.user_virtual_address = 0;
    released.mapped_byte_len = 0;
    released.record_hash = compute_process_session_memory_receipt_hash(&released);
    assert!(process_session_memory_receipt_canonical(&released));
    released.user_virtual_address = good.user_virtual_address;
    released.record_hash = compute_process_session_memory_receipt_hash(&released);
    assert!(!process_session_memory_receipt_canonical(&released));
}
