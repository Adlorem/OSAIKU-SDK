use osaiku_runtime::window::*;

fn completed() -> UiDocumentWindowChannelStatusRecord {
    UiDocumentWindowChannelStatusRecord {
        magic: UI_DOCUMENT_WINDOW_MAGIC,
        abi_version: UI_DOCUMENT_WINDOW_ABI_VERSION,
        channel_state: UI_DOCUMENT_WINDOW_CHANNEL_STATE_COMPLETED,
        status: UI_DOCUMENT_WINDOW_CHANNEL_STATUS_OK,
        reason: UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE,
        profile: UI_DOCUMENT_WINDOW_PROFILE_NORMAL,
        channel_flags: UI_DOCUMENT_WINDOW_CHANNEL_FLAGS_CANONICAL,
        broker_generation: 9,
        request_id: 0x1111_2222_3333_4444,
        request_hash: 0x5555_6666_7777_8888,
        arm_generation: 5,
        submit_generation: 6,
        take_generation: 7,
        complete_generation: 8,
        initd_process_id: 1,
        document_app_process_id: 17,
        document_app_process_endpoint: (29 << 32) | 17,
        document_app_instance_id: 31,
        next_public_generation: 37,
        renderer_process_id: 3,
        transition_provenance_hash: 0x4082_05d1_e4b1_2076,
        record_hash: 0x0a23_7449_d83d_ea6e,
    }
}

fn seal(record: &mut UiDocumentWindowChannelStatusRecord) {
    record.transition_provenance_hash =
        compute_ui_document_window_channel_transition_provenance_hash(record);
    record.record_hash = compute_ui_document_window_channel_status_record_hash(record);
}

#[test]
fn published_status_bytes_and_digests_keep_the_existing_abi() {
    // Fixed ABI v10 vector, independently packed little-endian and FNV-1a hashed.
    let hex = concat!(
        "55444f430a00000004000001ff3f00000900000000000000",
        "444433332222111188887777666655550500000000000000",
        "060000000000000007000000000000000800000000000000",
        "01000000000000001100000000000000110000001d000000",
        "1f0000000000000025000000000000000300000000000000",
        "7620b1e4d10582406eea3dd84974230a"
    );
    let mut bytes = [0; 136];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap();
    }
    let record = completed();
    assert_eq!(
        core::mem::size_of::<UiDocumentWindowChannelStatusRecord>(),
        136
    );
    assert_eq!(
        core::mem::align_of::<UiDocumentWindowChannelStatusRecord>(),
        8
    );
    assert_eq!(record.encode(), bytes);
    assert_eq!(
        UiDocumentWindowChannelStatusRecord::decode(&bytes),
        Some(record)
    );
    assert!(ui_document_window_channel_status_record_canonical(&record));
    assert_eq!(
        UiDocumentWindowChannelStatusRecord::decode(&bytes[..135]),
        None
    );
    assert_eq!(UiDocumentWindowChannelStatusRecord::decode(&[0; 137]), None);
    // Length decoding is deliberately distinct from canonical validation.
    let empty = UiDocumentWindowChannelStatusRecord::decode(&[0; 136]).unwrap();
    assert!(!ui_document_window_channel_status_record_canonical(&empty));
}

#[test]
fn all_wire_corruption_is_rejected_and_identity_fields_are_bound() {
    let record = completed();
    for index in 0..136 {
        let mut bytes = record.encode();
        bytes[index] ^= 1;
        let changed = UiDocumentWindowChannelStatusRecord::decode(&bytes).unwrap();
        assert!(
            !ui_document_window_channel_status_record_canonical(&changed),
            "byte {index}"
        );
    }
    // Zero identities/transaction generation and unknown versions stay invalid
    // even if their hashes are recomputed. Hashes alone confer no authority.
    for index in [4, 11, 12, 16, 72, 80, 88, 96, 104, 112] {
        let mut bytes = record.encode();
        let width = match index {
            4 | 12 => 4,
            11 => 1,
            _ => 8,
        };
        bytes[index..index + width].fill(0);
        let mut changed = UiDocumentWindowChannelStatusRecord::decode(&bytes).unwrap();
        seal(&mut changed);
        assert!(
            !ui_document_window_channel_status_record_canonical(&changed),
            "field {index}"
        );
    }
    let mut successor = record;
    successor.document_app_process_endpoint += 1 << 32;
    successor.document_app_instance_id += 1;
    successor.next_public_generation += 1;
    seal(&mut successor);
    // Canonical describes a well-formed record, not whether it names our window.
    assert!(ui_document_window_channel_status_record_canonical(
        &successor
    ));
    assert_ne!(
        successor.transition_provenance_hash,
        record.transition_provenance_hash
    );
    assert_ne!(successor.record_hash, record.record_hash);
}

#[test]
fn transaction_order_and_terminal_outcomes_remain_distinct() {
    for state in 0..=4 {
        let mut record = completed();
        record.channel_state = state;
        if state < 4 {
            record.complete_generation = 0;
        }
        if state < 3 {
            record.take_generation = 0;
        }
        if state < 2 {
            record.submit_generation = 0;
        }
        if state == 0 {
            record.request_id = 0;
            record.request_hash = 0;
            record.arm_generation = 0;
            record.next_public_generation = 0;
        }
        seal(&mut record);
        assert!(ui_document_window_channel_status_record_canonical(&record));
        if state < 4 {
            record.complete_generation = 8;
        } else {
            record.complete_generation = record.take_generation;
        }
        seal(&mut record);
        assert!(!ui_document_window_channel_status_record_canonical(&record));
    }
    for state in [
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_CANCELLED,
        UI_DOCUMENT_WINDOW_CHANNEL_STATE_REJECTED,
    ] {
        let mut record = completed();
        record.channel_state = state;
        record.status = UI_DOCUMENT_WINDOW_CHANNEL_STATUS_STALE;
        record.reason = UI_DOCUMENT_WINDOW_CHANNEL_REASON_REPLAY;
        seal(&mut record);
        assert!(ui_document_window_channel_status_record_canonical(&record));
        record.reason = UI_DOCUMENT_WINDOW_CHANNEL_REASON_NONE;
        seal(&mut record);
        assert!(!ui_document_window_channel_status_record_canonical(&record));
    }
}

#[test]
fn native_event_layout_keeps_full_source_identity() {
    assert_eq!(osaiku_runtime::abi::number::UI_ACTION_TAKE, 365);
    assert_eq!(core::mem::size_of::<Event>(), 56);
    assert_eq!(core::mem::align_of::<Event>(), 8);
    assert_eq!(core::mem::offset_of!(Event, target), 0);
    assert_eq!(core::mem::offset_of!(Event, element), 40);
    assert_eq!(core::mem::offset_of!(Event, kind), 44);
    assert_eq!(core::mem::offset_of!(Event, value), 48);
    assert_eq!(core::mem::offset_of!(Event, action), 52);
    assert_eq!(core::mem::size_of::<Identity>(), 40);
    assert_eq!(core::mem::offset_of!(Identity, owner), 8);
    assert_eq!(core::mem::offset_of!(Identity, window), 16);
    assert_eq!(core::mem::offset_of!(Identity, incarnation), 24);
    assert_eq!(core::mem::offset_of!(Identity, revision), 32);
}
