//! Compatibility envelope used by the current OSAIKU window host.
//! The historic document names and wire ABI remain stable. Metadata/codecs
//! grant no storage or editing permission. New non-editor SDK apps should use
//! the bounded hosted-session API rather than implement this envelope by hand.
//!
//!
//! The document application exclusively owns the retained working body and all
//! mutable editor state. This ABI carries bounded edit payloads into the app
//! and metadata-only replies back over the existing IPC boundary. Replies
//! expose hashes and semantic state, never document body, preview, title,
//! pixels, or observation data.
//!
//! All wire codecs and hashes are field-wise and little-endian. FNV-1a is used
//! only for deterministic binding and corruption detection; it is not
//! authentication, confidentiality, authorization, or a signature.

const AUDIT_JOURNAL_FNV1A_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const AUDIT_JOURNAL_FNV1A_PRIME: u64 = 0x100_0000_01b3;

// Version 5 adds a canonical resource-refusal reply for OPEN and permits an
// identity-bound QUERY to omit document freshness when discovering a newly
// created surface. Record layouts and hash field order are unchanged.
pub const DOCUMENT_EDITOR_ABI_VERSION: u32 = 5;
pub use crate::window::{
    compute_window_body_hash as compute_document_editor_body_hash,
    compute_window_document_id as compute_document_editor_document_id,
};

pub const DOCUMENT_EDITOR_BODY_CAPACITY: usize = crate::window::UI_DOCUMENT_WINDOW_BODY_CAPACITY;
pub const DOCUMENT_EDITOR_COMMAND_SIZE: usize = 160;
pub const DOCUMENT_EDITOR_STATE_RECORD_SIZE: usize = 128;
pub const DOCUMENT_EDITOR_REPLY_SIZE: usize = 160;

pub const DOCUMENT_EDITOR_COMMAND_OPEN: u8 = 1;
pub const DOCUMENT_EDITOR_COMMAND_QUERY: u8 = 2;
pub const DOCUMENT_EDITOR_COMMAND_REPLACE: u8 = 3;
pub const DOCUMENT_EDITOR_COMMAND_APPEND: u8 = 4;
pub const DOCUMENT_EDITOR_COMMAND_BACKSPACE: u8 = 5;
pub const DOCUMENT_EDITOR_COMMAND_CLEAR: u8 = 6;
pub const DOCUMENT_EDITOR_COMMAND_UNKNOWN: u8 = 0;

pub const DOCUMENT_EDITOR_INPUT_ORIGIN_NONE: u8 = 0;
pub const DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE: u8 = 3;

pub const DOCUMENT_EDITOR_ENCODING_ASCII: u8 = 1;

pub const DOCUMENT_EDITOR_COMMAND_FLAG_EXACT_TARGET: u32 = 1 << 0;
pub const DOCUMENT_EDITOR_COMMAND_FLAG_LOCAL_COMPATIBILITY_INPUT: u32 = 1 << 2;
pub const DOCUMENT_EDITOR_COMMAND_FLAGS_TRANSPORT: u32 = DOCUMENT_EDITOR_COMMAND_FLAG_EXACT_TARGET;
pub const DOCUMENT_EDITOR_COMMAND_FLAGS_LOCAL_COMPATIBILITY_EDIT: u32 =
    DOCUMENT_EDITOR_COMMAND_FLAG_EXACT_TARGET
        | DOCUMENT_EDITOR_COMMAND_FLAG_LOCAL_COMPATIBILITY_INPUT;

pub const DOCUMENT_EDITOR_STATUS_OK: u8 = 0;
pub const DOCUMENT_EDITOR_STATUS_INVALID_REQUEST: u8 = 1;
pub const DOCUMENT_EDITOR_STATUS_NOT_OPEN: u8 = 2;
pub const DOCUMENT_EDITOR_STATUS_ALREADY_OPEN: u8 = 3;
pub const DOCUMENT_EDITOR_STATUS_STALE_TARGET: u8 = 4;
pub const DOCUMENT_EDITOR_STATUS_STALE_DOCUMENT: u8 = 5;
pub const DOCUMENT_EDITOR_STATUS_CAPACITY_EXCEEDED: u8 = 6;
pub const DOCUMENT_EDITOR_STATUS_STATE_EXHAUSTED: u8 = 7;
pub const DOCUMENT_EDITOR_STATUS_RESOURCE_UNAVAILABLE: u8 = 8;

pub const DOCUMENT_EDITOR_DECISION_READ_ONLY: u8 = 1;
pub const DOCUMENT_EDITOR_DECISION_APPLIED: u8 = 2;
pub const DOCUMENT_EDITOR_DECISION_NO_OP: u8 = 3;
pub const DOCUMENT_EDITOR_DECISION_REJECTED: u8 = 4;

pub const DOCUMENT_EDITOR_REASON_NONE: u8 = 0;
pub const DOCUMENT_EDITOR_REASON_BAD_ABI_VERSION: u8 = 1;
pub const DOCUMENT_EDITOR_REASON_BAD_COMMAND_KIND: u8 = 2;
pub const DOCUMENT_EDITOR_REASON_BAD_COMMAND_FLAGS: u8 = 3;
pub const DOCUMENT_EDITOR_REASON_BAD_INPUT_ORIGIN: u8 = 4;
pub const DOCUMENT_EDITOR_REASON_BAD_ENCODING: u8 = 5;
pub const DOCUMENT_EDITOR_REASON_BAD_COMMAND_SHAPE: u8 = 6;
pub const DOCUMENT_EDITOR_REASON_BAD_INPUT_EVIDENCE: u8 = 7;
pub const DOCUMENT_EDITOR_REASON_NONZERO_RESERVED: u8 = 8;
pub const DOCUMENT_EDITOR_REASON_ZERO_COMMAND_ID: u8 = 9;
pub const DOCUMENT_EDITOR_REASON_BAD_COMMAND_HASH: u8 = 10;
pub const DOCUMENT_EDITOR_REASON_PAYLOAD_TOO_LARGE: u8 = 11;
pub const DOCUMENT_EDITOR_REASON_EDITOR_NOT_OPEN: u8 = 12;
pub const DOCUMENT_EDITOR_REASON_EDITOR_ALREADY_OPEN: u8 = 13;
pub const DOCUMENT_EDITOR_REASON_APP_INSTANCE_ID_MISMATCH: u8 = 14;
pub const DOCUMENT_EDITOR_REASON_APP_INSTANCE_GENERATION_MISMATCH: u8 = 15;
pub const DOCUMENT_EDITOR_REASON_SURFACE_ID_MISMATCH: u8 = 16;
pub const DOCUMENT_EDITOR_REASON_SURFACE_GENERATION_MISMATCH: u8 = 17;
pub const DOCUMENT_EDITOR_REASON_BINDING_GENERATION_MISMATCH: u8 = 18;
pub const DOCUMENT_EDITOR_REASON_BINDING_HASH_MISMATCH: u8 = 19;
pub const DOCUMENT_EDITOR_REASON_DOCUMENT_ID_MISMATCH: u8 = 20;
pub const DOCUMENT_EDITOR_REASON_DOCUMENT_REVISION_MISMATCH: u8 = 21;
pub const DOCUMENT_EDITOR_REASON_EDITOR_GENERATION_MISMATCH: u8 = 22;
pub const DOCUMENT_EDITOR_REASON_SEMANTIC_GENERATION_MISMATCH: u8 = 23;
pub const DOCUMENT_EDITOR_REASON_BODY_HASH_MISMATCH: u8 = 24;
pub const DOCUMENT_EDITOR_REASON_RESULT_CAPACITY_EXCEEDED: u8 = 25;
pub const DOCUMENT_EDITOR_REASON_UNCHANGED_REPLACEMENT: u8 = 26;
pub const DOCUMENT_EDITOR_REASON_EMPTY_BACKSPACE: u8 = 27;
pub const DOCUMENT_EDITOR_REASON_EMPTY_CLEAR: u8 = 28;
pub const DOCUMENT_EDITOR_REASON_STATE_COUNTER_EXHAUSTED: u8 = 29;
pub const DOCUMENT_EDITOR_REASON_SESSION_PROCESS_QUOTA: u8 = 30;
pub const DOCUMENT_EDITOR_REASON_SESSION_PROTECTED_RESERVE: u8 = 31;
pub const DOCUMENT_EDITOR_REASON_SESSION_TOTAL_EXHAUSTED: u8 = 32;
pub const DOCUMENT_EDITOR_REASON_SESSION_FRAGMENTED: u8 = 33;
pub const DOCUMENT_EDITOR_REASON_SESSION_VIRTUAL_RANGE_EXHAUSTED: u8 = 34;
pub const DOCUMENT_EDITOR_REASON_SESSION_MAPPING_UNAVAILABLE: u8 = 35;
pub const DOCUMENT_EDITOR_REASON_SESSION_TRANSITION_EXHAUSTED: u8 = 36;

pub const DOCUMENT_EDITOR_STATE_UNBOUND: u8 = 0;
pub const DOCUMENT_EDITOR_STATE_OPEN: u8 = 1;
pub const DOCUMENT_EDITOR_CONTENT_EMPTY: u8 = 0;
pub const DOCUMENT_EDITOR_CONTENT_PRESENT: u8 = 1;
pub const DOCUMENT_EDITOR_DIRTY_CLEAN: u8 = 0;
pub const DOCUMENT_EDITOR_DIRTY_DIRTY: u8 = 1;
pub const DOCUMENT_EDITOR_SAVE_NEVER_SAVED: u8 = 0;
pub const DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT: u8 = 1;
pub const DOCUMENT_EDITOR_SAVE_VOLATILE_STALE: u8 = 2;
pub const DOCUMENT_EDITOR_SAVE_DURABLE_CURRENT: u8 = 3;
pub const DOCUMENT_EDITOR_SAVE_DURABLE_STALE: u8 = 4;
pub const DOCUMENT_EDITOR_SAVE_STORAGE_UNAVAILABLE: u8 = 5;
pub const DOCUMENT_EDITOR_SAVE_AUTH_FAILED: u8 = 6;
pub const DOCUMENT_EDITOR_SAVE_RECOVERY_REQUIRED: u8 = 7;
pub const DOCUMENT_EDITOR_SAVE_INCOMPATIBLE: u8 = 8;
pub const DOCUMENT_EDITOR_SAVE_STALE_TARGET: u8 = 9;
pub const DOCUMENT_EDITOR_SAVE_EMPTY_VOLUME: u8 = 10;
pub const DOCUMENT_EDITOR_SAVE_CORRUPT: u8 = 11;
pub const DOCUMENT_EDITOR_SAVE_UNKNOWN_COMMIT: u8 = 12;
pub const DOCUMENT_EDITOR_SAVE_FILE_CONFLICT: u8 = 13;
pub const DOCUMENT_EDITOR_SAVE_NO_SPACE: u8 = 14;
pub const DOCUMENT_EDITOR_SAVE_PERMISSION: u8 = 15;
pub const DOCUMENT_EDITOR_SAVE_NAME_ENTRY: u8 = 16;
pub const DOCUMENT_EDITOR_OPEN_NAME_ENTRY: u8 = 17;

pub fn document_editor_storage_refused_state(value: u8) -> bool {
    matches!(
        value,
        DOCUMENT_EDITOR_SAVE_STORAGE_UNAVAILABLE
            | DOCUMENT_EDITOR_SAVE_AUTH_FAILED
            | DOCUMENT_EDITOR_SAVE_RECOVERY_REQUIRED
            | DOCUMENT_EDITOR_SAVE_INCOMPATIBLE
            | DOCUMENT_EDITOR_SAVE_STALE_TARGET
            | DOCUMENT_EDITOR_SAVE_EMPTY_VOLUME
            | DOCUMENT_EDITOR_SAVE_CORRUPT
            | DOCUMENT_EDITOR_SAVE_UNKNOWN_COMMIT
            | DOCUMENT_EDITOR_SAVE_FILE_CONFLICT
            | DOCUMENT_EDITOR_SAVE_NO_SPACE
            | DOCUMENT_EDITOR_SAVE_PERMISSION
    )
}

pub fn document_editor_durable_state(save_state: u8) -> bool {
    matches!(
        save_state,
        DOCUMENT_EDITOR_SAVE_DURABLE_CURRENT | DOCUMENT_EDITOR_SAVE_DURABLE_STALE
    )
}
pub const DOCUMENT_EDITOR_LAST_INPUT_NONE: u8 = DOCUMENT_EDITOR_INPUT_ORIGIN_NONE;

pub const DOCUMENT_EDITOR_SOURCE_APP_PRIVATE_MEMORY: u32 = 1 << 0;
pub const DOCUMENT_EDITOR_SOURCE_VOLATILE_PROCESS_STATE: u32 = 1 << 2;
pub const DOCUMENT_EDITOR_SOURCE_TYPED_INPUTD_IPC: u32 = 1 << 3;
pub const DOCUMENT_EDITOR_SOURCE_UNBOUND_CANONICAL: u32 =
    DOCUMENT_EDITOR_SOURCE_APP_PRIVATE_MEMORY | DOCUMENT_EDITOR_SOURCE_VOLATILE_PROCESS_STATE;
pub const DOCUMENT_EDITOR_SOURCE_LOCAL_COMPATIBILITY_CANONICAL: u32 =
    DOCUMENT_EDITOR_SOURCE_APP_PRIVATE_MEMORY
        | DOCUMENT_EDITOR_SOURCE_TYPED_INPUTD_IPC
        | DOCUMENT_EDITOR_SOURCE_VOLATILE_PROCESS_STATE;
pub const DOCUMENT_EDITOR_SOURCE_APP_LIFECYCLE_CANONICAL: u32 =
    DOCUMENT_EDITOR_SOURCE_APP_PRIVATE_MEMORY | DOCUMENT_EDITOR_SOURCE_VOLATILE_PROCESS_STATE;

pub const DOCUMENT_EDITOR_SEMANTIC_HUMAN_OWNED: u32 = 1 << 0;
pub const DOCUMENT_EDITOR_SEMANTIC_BOUNDED_WORKING_STATE: u32 = 1 << 1;
pub const DOCUMENT_EDITOR_SEMANTIC_HASH_ONLY_PROJECTION: u32 = 1 << 2;
pub const DOCUMENT_EDITOR_SEMANTIC_USER_SPACE_AUTONOMOUS: u32 = 1 << 3;
pub const DOCUMENT_EDITOR_SEMANTIC_STABLE_SURFACE_BINDING: u32 = 1 << 4;
pub const DOCUMENT_EDITOR_SEMANTIC_LOCAL_COMPATIBILITY_INPUT_HONEST: u32 = 1 << 6;
pub const DOCUMENT_EDITOR_SEMANTIC_BASE: u32 = DOCUMENT_EDITOR_SEMANTIC_HUMAN_OWNED
    | DOCUMENT_EDITOR_SEMANTIC_BOUNDED_WORKING_STATE
    | DOCUMENT_EDITOR_SEMANTIC_HASH_ONLY_PROJECTION
    | DOCUMENT_EDITOR_SEMANTIC_USER_SPACE_AUTONOMOUS
    | DOCUMENT_EDITOR_SEMANTIC_STABLE_SURFACE_BINDING;

pub const DOCUMENT_EDITOR_BOUNDARY_NO_BODY_IPC_PROJECTION: u32 = 1 << 0;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_PREVIEW_DISCLOSURE: u32 = 1 << 1;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_AI_MEDIATION: u32 = 1 << 2;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_AI_SUPERVISION: u32 = 1 << 3;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_AI_APPROVAL: u32 = 1 << 4;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_AI_LEASE: u32 = 1 << 5;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_AI_TICKET: u32 = 1 << 6;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_AI_OBSERVATION: u32 = 1 << 7;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_MODEL_EXECUTION: u32 = 1 << 8;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_TOOL_EXECUTION: u32 = 1 << 9;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_SHELL: u32 = 1 << 10;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_NETWORK: u32 = 1 << 11;
pub const DOCUMENT_EDITOR_BOUNDARY_TICKET_GATED_VOLATILE_SAVE: u32 = 1 << 12;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_GENERIC_STORAGE: u32 = 1 << 13;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_DURABLE_SAVE: u32 = 1 << 14;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_EXTERNAL_CONTROL: u32 = 1 << 15;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_INPUT_INJECTION: u32 = 1 << 16;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_LIVE_INPUT_CLAIM: u32 = 1 << 17;
pub const DOCUMENT_EDITOR_BOUNDARY_INPUTD_BROKER_ONLY: u32 = 1 << 18;
pub const DOCUMENT_EDITOR_BOUNDARY_LOCAL_COMPATIBILITY_INPUT_ONLY: u32 = 1 << 19;
pub const DOCUMENT_EDITOR_BOUNDARY_NO_PHYSICAL_INPUT_CLAIM: u32 = 1 << 20;
pub const DOCUMENT_EDITOR_BOUNDARY_UNBOUND_CANONICAL: u32 =
    DOCUMENT_EDITOR_BOUNDARY_NO_BODY_IPC_PROJECTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_PREVIEW_DISCLOSURE
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_MEDIATION
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_SUPERVISION
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_APPROVAL
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_LEASE
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_TICKET
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_OBSERVATION
        | DOCUMENT_EDITOR_BOUNDARY_NO_MODEL_EXECUTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_TOOL_EXECUTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_SHELL
        | DOCUMENT_EDITOR_BOUNDARY_NO_NETWORK
        | DOCUMENT_EDITOR_BOUNDARY_TICKET_GATED_VOLATILE_SAVE
        | DOCUMENT_EDITOR_BOUNDARY_NO_GENERIC_STORAGE
        | DOCUMENT_EDITOR_BOUNDARY_NO_DURABLE_SAVE
        | DOCUMENT_EDITOR_BOUNDARY_NO_EXTERNAL_CONTROL
        | DOCUMENT_EDITOR_BOUNDARY_NO_INPUT_INJECTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_LIVE_INPUT_CLAIM;
pub const DOCUMENT_EDITOR_BOUNDARY_LOCAL_COMPATIBILITY_CANONICAL: u32 =
    DOCUMENT_EDITOR_BOUNDARY_NO_BODY_IPC_PROJECTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_PREVIEW_DISCLOSURE
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_MEDIATION
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_SUPERVISION
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_APPROVAL
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_LEASE
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_TICKET
        | DOCUMENT_EDITOR_BOUNDARY_NO_AI_OBSERVATION
        | DOCUMENT_EDITOR_BOUNDARY_NO_MODEL_EXECUTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_TOOL_EXECUTION
        | DOCUMENT_EDITOR_BOUNDARY_NO_SHELL
        | DOCUMENT_EDITOR_BOUNDARY_NO_NETWORK
        | DOCUMENT_EDITOR_BOUNDARY_TICKET_GATED_VOLATILE_SAVE
        | DOCUMENT_EDITOR_BOUNDARY_NO_GENERIC_STORAGE
        | DOCUMENT_EDITOR_BOUNDARY_NO_DURABLE_SAVE
        | DOCUMENT_EDITOR_BOUNDARY_NO_EXTERNAL_CONTROL
        | DOCUMENT_EDITOR_BOUNDARY_NO_INPUT_INJECTION
        | DOCUMENT_EDITOR_BOUNDARY_INPUTD_BROKER_ONLY
        | DOCUMENT_EDITOR_BOUNDARY_LOCAL_COMPATIBILITY_INPUT_ONLY
        | DOCUMENT_EDITOR_BOUNDARY_NO_PHYSICAL_INPUT_CLAIM;
pub const DOCUMENT_EDITOR_BOUNDARY_APP_LIFECYCLE_CANONICAL: u32 =
    DOCUMENT_EDITOR_BOUNDARY_UNBOUND_CANONICAL;

pub const DOCUMENT_EDITOR_RESULT_STATE_INCLUDED: u32 = 1 << 0;
pub const DOCUMENT_EDITOR_RESULT_METADATA_ONLY: u32 = 1 << 1;
pub const DOCUMENT_EDITOR_RESULT_READ_ONLY: u32 = 1 << 2;
pub const DOCUMENT_EDITOR_RESULT_APPLIED: u32 = 1 << 3;
pub const DOCUMENT_EDITOR_RESULT_NO_OP: u32 = 1 << 4;
pub const DOCUMENT_EDITOR_RESULT_REJECTED: u32 = 1 << 5;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentEditorCommand {
    pub abi_version: u32,
    pub command_kind: u8,
    pub input_origin: u8,
    pub encoding: u8,
    pub payload_len: u8,
    pub command_flags: u32,
    pub reserved_header: [u8; 4],
    pub command_id: u64,
    pub expected_app_instance_id: u64,
    pub expected_app_instance_generation: u64,
    pub expected_surface_id: u64,
    pub expected_surface_generation: u64,
    pub expected_binding_generation: u64,
    pub expected_binding_hash: u64,
    pub expected_document_id: u64,
    pub expected_document_revision: u64,
    pub expected_editor_generation: u64,
    pub expected_semantic_state_generation: u64,
    pub expected_body_hash: u64,
    pub input_evidence_hash: u64,
    pub payload: [u8; DOCUMENT_EDITOR_BODY_CAPACITY],
    pub command_hash: u64,
}

impl DocumentEditorCommand {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        command_kind: 0,
        input_origin: 0,
        encoding: 0,
        payload_len: 0,
        command_flags: 0,
        reserved_header: [0; 4],
        command_id: 0,
        expected_app_instance_id: 0,
        expected_app_instance_generation: 0,
        expected_surface_id: 0,
        expected_surface_generation: 0,
        expected_binding_generation: 0,
        expected_binding_hash: 0,
        expected_document_id: 0,
        expected_document_revision: 0,
        expected_editor_generation: 0,
        expected_semantic_state_generation: 0,
        expected_body_hash: 0,
        input_evidence_hash: 0,
        payload: [0; DOCUMENT_EDITOR_BODY_CAPACITY],
        command_hash: 0,
    };

    pub fn encode(&self) -> [u8; DOCUMENT_EDITOR_COMMAND_SIZE] {
        let mut out = [0u8; DOCUMENT_EDITOR_COMMAND_SIZE];
        put_u32(&mut out, 0, self.abi_version);
        out[4] = self.command_kind;
        out[5] = self.input_origin;
        out[6] = self.encoding;
        out[7] = self.payload_len;
        put_u32(&mut out, 8, self.command_flags);
        out[12..16].copy_from_slice(&self.reserved_header);
        let fields = [
            self.command_id,
            self.expected_app_instance_id,
            self.expected_app_instance_generation,
            self.expected_surface_id,
            self.expected_surface_generation,
            self.expected_binding_generation,
            self.expected_binding_hash,
            self.expected_document_id,
            self.expected_document_revision,
            self.expected_editor_generation,
            self.expected_semantic_state_generation,
            self.expected_body_hash,
            self.input_evidence_hash,
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put_u64(&mut out, 16 + index * 8, value);
        }
        out[120..152].copy_from_slice(&self.payload);
        put_u64(&mut out, 152, self.command_hash);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != DOCUMENT_EDITOR_COMMAND_SIZE {
            return None;
        }
        let mut reserved_header = [0u8; 4];
        reserved_header.copy_from_slice(&bytes[12..16]);
        let mut payload = [0u8; DOCUMENT_EDITOR_BODY_CAPACITY];
        payload.copy_from_slice(&bytes[120..152]);
        Some(Self {
            abi_version: get_u32(bytes, 0),
            command_kind: bytes[4],
            input_origin: bytes[5],
            encoding: bytes[6],
            payload_len: bytes[7],
            command_flags: get_u32(bytes, 8),
            reserved_header,
            command_id: get_u64(bytes, 16),
            expected_app_instance_id: get_u64(bytes, 24),
            expected_app_instance_generation: get_u64(bytes, 32),
            expected_surface_id: get_u64(bytes, 40),
            expected_surface_generation: get_u64(bytes, 48),
            expected_binding_generation: get_u64(bytes, 56),
            expected_binding_hash: get_u64(bytes, 64),
            expected_document_id: get_u64(bytes, 72),
            expected_document_revision: get_u64(bytes, 80),
            expected_editor_generation: get_u64(bytes, 88),
            expected_semantic_state_generation: get_u64(bytes, 96),
            expected_body_hash: get_u64(bytes, 104),
            input_evidence_hash: get_u64(bytes, 112),
            payload,
            command_hash: get_u64(bytes, 152),
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentEditorStateRecord {
    pub abi_version: u32,
    pub editor_state: u8,
    pub content_state: u8,
    pub dirty_state: u8,
    pub save_state: u8,
    pub encoding: u8,
    pub last_input_origin: u8,
    pub body_len: u8,
    pub body_capacity: u8,
    pub source_flags: u32,
    pub semantic_flags: u32,
    pub boundary_flags: u32,
    pub app_instance_id: u64,
    pub app_instance_generation: u64,
    pub surface_id: u64,
    pub surface_generation: u64,
    pub binding_generation: u64,
    pub binding_hash: u64,
    pub document_id: u64,
    pub document_revision: u64,
    pub editor_generation: u64,
    pub semantic_state_generation: u64,
    pub body_hash: u64,
    pub source_transition_hash: u64,
    pub record_hash: u64,
}

impl DocumentEditorStateRecord {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        editor_state: DOCUMENT_EDITOR_STATE_UNBOUND,
        content_state: DOCUMENT_EDITOR_CONTENT_EMPTY,
        dirty_state: DOCUMENT_EDITOR_DIRTY_CLEAN,
        save_state: DOCUMENT_EDITOR_SAVE_NEVER_SAVED,
        encoding: 0,
        last_input_origin: DOCUMENT_EDITOR_LAST_INPUT_NONE,
        body_len: 0,
        body_capacity: 0,
        source_flags: 0,
        semantic_flags: 0,
        boundary_flags: 0,
        app_instance_id: 0,
        app_instance_generation: 0,
        surface_id: 0,
        surface_generation: 0,
        binding_generation: 0,
        binding_hash: 0,
        document_id: 0,
        document_revision: 0,
        editor_generation: 0,
        semantic_state_generation: 0,
        body_hash: 0,
        source_transition_hash: 0,
        record_hash: 0,
    };

    pub fn canonical_unbound() -> Self {
        let mut record = Self {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            editor_state: DOCUMENT_EDITOR_STATE_UNBOUND,
            content_state: DOCUMENT_EDITOR_CONTENT_EMPTY,
            dirty_state: DOCUMENT_EDITOR_DIRTY_CLEAN,
            save_state: DOCUMENT_EDITOR_SAVE_NEVER_SAVED,
            encoding: DOCUMENT_EDITOR_ENCODING_ASCII,
            last_input_origin: DOCUMENT_EDITOR_LAST_INPUT_NONE,
            body_len: 0,
            body_capacity: DOCUMENT_EDITOR_BODY_CAPACITY as u8,
            source_flags: DOCUMENT_EDITOR_SOURCE_UNBOUND_CANONICAL,
            semantic_flags: DOCUMENT_EDITOR_SEMANTIC_BASE,
            boundary_flags: DOCUMENT_EDITOR_BOUNDARY_UNBOUND_CANONICAL,
            ..Self::EMPTY
        };
        record.record_hash = compute_document_editor_state_record_hash(&record);
        record
    }

    pub fn encode(&self) -> [u8; DOCUMENT_EDITOR_STATE_RECORD_SIZE] {
        let mut out = [0u8; DOCUMENT_EDITOR_STATE_RECORD_SIZE];
        put_u32(&mut out, 0, self.abi_version);
        out[4] = self.editor_state;
        out[5] = self.content_state;
        out[6] = self.dirty_state;
        out[7] = self.save_state;
        out[8] = self.encoding;
        out[9] = self.last_input_origin;
        out[10] = self.body_len;
        out[11] = self.body_capacity;
        put_u32(&mut out, 12, self.source_flags);
        put_u32(&mut out, 16, self.semantic_flags);
        put_u32(&mut out, 20, self.boundary_flags);
        let fields = [
            self.app_instance_id,
            self.app_instance_generation,
            self.surface_id,
            self.surface_generation,
            self.binding_generation,
            self.binding_hash,
            self.document_id,
            self.document_revision,
            self.editor_generation,
            self.semantic_state_generation,
            self.body_hash,
            self.source_transition_hash,
            self.record_hash,
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put_u64(&mut out, 24 + index * 8, value);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != DOCUMENT_EDITOR_STATE_RECORD_SIZE {
            return None;
        }
        Some(Self {
            abi_version: get_u32(bytes, 0),
            editor_state: bytes[4],
            content_state: bytes[5],
            dirty_state: bytes[6],
            save_state: bytes[7],
            encoding: bytes[8],
            last_input_origin: bytes[9],
            body_len: bytes[10],
            body_capacity: bytes[11],
            source_flags: get_u32(bytes, 12),
            semantic_flags: get_u32(bytes, 16),
            boundary_flags: get_u32(bytes, 20),
            app_instance_id: get_u64(bytes, 24),
            app_instance_generation: get_u64(bytes, 32),
            surface_id: get_u64(bytes, 40),
            surface_generation: get_u64(bytes, 48),
            binding_generation: get_u64(bytes, 56),
            binding_hash: get_u64(bytes, 64),
            document_id: get_u64(bytes, 72),
            document_revision: get_u64(bytes, 80),
            editor_generation: get_u64(bytes, 88),
            semantic_state_generation: get_u64(bytes, 96),
            body_hash: get_u64(bytes, 104),
            source_transition_hash: get_u64(bytes, 112),
            record_hash: get_u64(bytes, 120),
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentEditorReply {
    pub abi_version: u32,
    pub status: u8,
    pub decision: u8,
    pub reason: u8,
    pub command_kind: u8,
    pub result_flags: u32,
    pub reserved: u32,
    pub source_command_hash: u64,
    pub state: DocumentEditorStateRecord,
    pub reply_hash: u64,
}

impl DocumentEditorReply {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        status: DOCUMENT_EDITOR_STATUS_INVALID_REQUEST,
        decision: DOCUMENT_EDITOR_DECISION_REJECTED,
        reason: DOCUMENT_EDITOR_REASON_NONE,
        command_kind: 0,
        result_flags: 0,
        reserved: 0,
        source_command_hash: 0,
        state: DocumentEditorStateRecord::EMPTY,
        reply_hash: 0,
    };

    pub fn encode(&self) -> [u8; DOCUMENT_EDITOR_REPLY_SIZE] {
        let mut out = [0u8; DOCUMENT_EDITOR_REPLY_SIZE];
        put_u32(&mut out, 0, self.abi_version);
        out[4] = self.status;
        out[5] = self.decision;
        out[6] = self.reason;
        out[7] = self.command_kind;
        put_u32(&mut out, 8, self.result_flags);
        put_u32(&mut out, 12, self.reserved);
        put_u64(&mut out, 16, self.source_command_hash);
        out[24..152].copy_from_slice(&self.state.encode());
        put_u64(&mut out, 152, self.reply_hash);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != DOCUMENT_EDITOR_REPLY_SIZE {
            return None;
        }
        Some(Self {
            abi_version: get_u32(bytes, 0),
            status: bytes[4],
            decision: bytes[5],
            reason: bytes[6],
            command_kind: bytes[7],
            result_flags: get_u32(bytes, 8),
            reserved: get_u32(bytes, 12),
            source_command_hash: get_u64(bytes, 16),
            state: DocumentEditorStateRecord::decode(&bytes[24..152])?,
            reply_hash: get_u64(bytes, 152),
        })
    }
}

const _: [(); DOCUMENT_EDITOR_COMMAND_SIZE] = [(); core::mem::size_of::<DocumentEditorCommand>()];
const _: [(); DOCUMENT_EDITOR_STATE_RECORD_SIZE] =
    [(); core::mem::size_of::<DocumentEditorStateRecord>()];
const _: [(); DOCUMENT_EDITOR_REPLY_SIZE] = [(); core::mem::size_of::<DocumentEditorReply>()];

pub fn compute_document_editor_command_hash(command: &DocumentEditorCommand) -> u64 {
    let mut hash = domain_hash(b"osaiku.document-editor.command.v1");
    hash = fold_u32(hash, command.abi_version);
    hash = fold_u8(hash, command.command_kind);
    hash = fold_u8(hash, command.input_origin);
    hash = fold_u8(hash, command.encoding);
    hash = fold_u8(hash, command.payload_len);
    hash = fold_u32(hash, command.command_flags);
    hash = fold_bytes(hash, &command.reserved_header);
    hash = fold_u64(hash, command.command_id);
    hash = fold_u64(hash, command.expected_app_instance_id);
    hash = fold_u64(hash, command.expected_app_instance_generation);
    hash = fold_u64(hash, command.expected_surface_id);
    hash = fold_u64(hash, command.expected_surface_generation);
    hash = fold_u64(hash, command.expected_binding_generation);
    hash = fold_u64(hash, command.expected_binding_hash);
    hash = fold_u64(hash, command.expected_document_id);
    hash = fold_u64(hash, command.expected_document_revision);
    hash = fold_u64(hash, command.expected_editor_generation);
    hash = fold_u64(hash, command.expected_semantic_state_generation);
    hash = fold_u64(hash, command.expected_body_hash);
    hash = fold_u64(hash, command.input_evidence_hash);
    fold_bytes(hash, &command.payload)
}

pub fn document_editor_command_hash_valid(command: &DocumentEditorCommand) -> bool {
    command.command_hash != 0
        && compute_document_editor_command_hash(command) == command.command_hash
}

pub fn compute_document_editor_state_record_hash(record: &DocumentEditorStateRecord) -> u64 {
    let mut hash = domain_hash(b"osaiku.document-editor.state.v1");
    hash = fold_u32(hash, record.abi_version);
    hash = fold_u8(hash, record.editor_state);
    hash = fold_u8(hash, record.content_state);
    hash = fold_u8(hash, record.dirty_state);
    hash = fold_u8(hash, record.save_state);
    hash = fold_u8(hash, record.encoding);
    hash = fold_u8(hash, record.last_input_origin);
    hash = fold_u8(hash, record.body_len);
    hash = fold_u8(hash, record.body_capacity);
    hash = fold_u32(hash, record.source_flags);
    hash = fold_u32(hash, record.semantic_flags);
    hash = fold_u32(hash, record.boundary_flags);
    hash = fold_u64(hash, record.app_instance_id);
    hash = fold_u64(hash, record.app_instance_generation);
    hash = fold_u64(hash, record.surface_id);
    hash = fold_u64(hash, record.surface_generation);
    hash = fold_u64(hash, record.binding_generation);
    hash = fold_u64(hash, record.binding_hash);
    hash = fold_u64(hash, record.document_id);
    hash = fold_u64(hash, record.document_revision);
    hash = fold_u64(hash, record.editor_generation);
    hash = fold_u64(hash, record.semantic_state_generation);
    hash = fold_u64(hash, record.body_hash);
    fold_u64(hash, record.source_transition_hash)
}

pub fn document_editor_state_record_hash_valid(record: &DocumentEditorStateRecord) -> bool {
    record.record_hash != 0
        && compute_document_editor_state_record_hash(record) == record.record_hash
}

pub fn document_editor_state_record_canonical(record: &DocumentEditorStateRecord) -> bool {
    let local_compatibility_input =
        record.last_input_origin == DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE;
    let expected_source = if local_compatibility_input {
        DOCUMENT_EDITOR_SOURCE_LOCAL_COMPATIBILITY_CANONICAL
    } else if record.editor_state == DOCUMENT_EDITOR_STATE_OPEN {
        DOCUMENT_EDITOR_SOURCE_APP_LIFECYCLE_CANONICAL
    } else {
        DOCUMENT_EDITOR_SOURCE_UNBOUND_CANONICAL
    };
    let mut expected_boundary = if local_compatibility_input {
        DOCUMENT_EDITOR_BOUNDARY_LOCAL_COMPATIBILITY_CANONICAL
    } else if record.editor_state == DOCUMENT_EDITOR_STATE_OPEN {
        DOCUMENT_EDITOR_BOUNDARY_APP_LIFECYCLE_CANONICAL
    } else {
        DOCUMENT_EDITOR_BOUNDARY_UNBOUND_CANONICAL
    };
    if document_editor_durable_state(record.save_state) {
        expected_boundary &= !(DOCUMENT_EDITOR_BOUNDARY_NO_DURABLE_SAVE
            | DOCUMENT_EDITOR_BOUNDARY_TICKET_GATED_VOLATILE_SAVE);
    }
    if record.abi_version != DOCUMENT_EDITOR_ABI_VERSION
        || record.encoding != DOCUMENT_EDITOR_ENCODING_ASCII
        || !(document_editor_storage_refused_state(record.save_state)
            || matches!(
                record.save_state,
                DOCUMENT_EDITOR_SAVE_NAME_ENTRY | DOCUMENT_EDITOR_OPEN_NAME_ENTRY
            )
            || matches!(
                record.save_state,
                DOCUMENT_EDITOR_SAVE_NEVER_SAVED
                    | DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT
                    | DOCUMENT_EDITOR_SAVE_VOLATILE_STALE
                    | DOCUMENT_EDITOR_SAVE_DURABLE_CURRENT
                    | DOCUMENT_EDITOR_SAVE_DURABLE_STALE
            ))
        || record.body_capacity != DOCUMENT_EDITOR_BODY_CAPACITY as u8
        || record.source_flags != expected_source
        || record.boundary_flags != expected_boundary
        || record.body_len > record.body_capacity
        || !document_editor_state_record_hash_valid(record)
    {
        return false;
    }

    let expected_semantic = match record.last_input_origin {
        DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE => {
            DOCUMENT_EDITOR_SEMANTIC_BASE
                | DOCUMENT_EDITOR_SEMANTIC_LOCAL_COMPATIBILITY_INPUT_HONEST
        }
        _ => DOCUMENT_EDITOR_SEMANTIC_BASE,
    };
    if record.semantic_flags != expected_semantic {
        return false;
    }

    match record.editor_state {
        DOCUMENT_EDITOR_STATE_UNBOUND => {
            record.content_state == DOCUMENT_EDITOR_CONTENT_EMPTY
                && record.dirty_state == DOCUMENT_EDITOR_DIRTY_CLEAN
                && record.save_state == DOCUMENT_EDITOR_SAVE_NEVER_SAVED
                && record.last_input_origin == DOCUMENT_EDITOR_LAST_INPUT_NONE
                && record.body_len == 0
                && editor_identity_fields_zero(record)
        }
        DOCUMENT_EDITOR_STATE_OPEN => {
            matches!(
                record.content_state,
                DOCUMENT_EDITOR_CONTENT_EMPTY | DOCUMENT_EDITOR_CONTENT_PRESENT
            ) && matches!(
                record.dirty_state,
                DOCUMENT_EDITOR_DIRTY_CLEAN | DOCUMENT_EDITOR_DIRTY_DIRTY
            ) && matches!(
                record.last_input_origin,
                DOCUMENT_EDITOR_LAST_INPUT_NONE
                    | DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE
            ) && ((record.body_len == 0 && record.content_state == DOCUMENT_EDITOR_CONTENT_EMPTY)
                || (record.body_len != 0
                    && record.content_state == DOCUMENT_EDITOR_CONTENT_PRESENT))
                && record.app_instance_id != 0
                && record.app_instance_generation != 0
                && record.surface_id != 0
                && record.surface_generation != 0
                && record.binding_generation != 0
                && record.binding_hash != 0
                && record.document_id != 0
                && record.document_id
                    == compute_document_editor_document_id(
                        record.app_instance_id,
                        record.app_instance_generation,
                        record.surface_id,
                        record.surface_generation,
                        record.binding_generation,
                        record.binding_hash,
                    )
                && record.document_revision != 0
                && record.editor_generation != 0
                && record.semantic_state_generation != 0
                && record.editor_generation == record.semantic_state_generation
                && record.editor_generation >= record.document_revision
                && record.body_hash != 0
                && (record.body_len != 0
                    || record.body_hash == compute_document_editor_body_hash(&[]))
                && record.source_transition_hash != 0
                && ((record.dirty_state == DOCUMENT_EDITOR_DIRTY_CLEAN
                    && record.save_state == DOCUMENT_EDITOR_SAVE_NEVER_SAVED
                    && record.content_state == DOCUMENT_EDITOR_CONTENT_EMPTY
                    && record.last_input_origin == DOCUMENT_EDITOR_LAST_INPUT_NONE)
                    || (record.dirty_state == DOCUMENT_EDITOR_DIRTY_DIRTY
                        && matches!(
                            record.save_state,
                            DOCUMENT_EDITOR_SAVE_NEVER_SAVED
                                | DOCUMENT_EDITOR_SAVE_VOLATILE_STALE
                                | DOCUMENT_EDITOR_SAVE_DURABLE_STALE
                        )
                        && matches!(
                            record.last_input_origin,
                            DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE
                        )
                        && record.document_revision > 1)
                    || (record.dirty_state == DOCUMENT_EDITOR_DIRTY_CLEAN
                        && record.save_state == DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT
                        && matches!(
                            record.last_input_origin,
                            DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE
                        )
                        && record.document_revision > 1)
                    || (record.dirty_state == DOCUMENT_EDITOR_DIRTY_CLEAN
                        && record.save_state == DOCUMENT_EDITOR_SAVE_DURABLE_CURRENT)
                    || (matches!(
                        record.save_state,
                        DOCUMENT_EDITOR_SAVE_NAME_ENTRY | DOCUMENT_EDITOR_OPEN_NAME_ENTRY
                    ) && record.dirty_state == DOCUMENT_EDITOR_DIRTY_DIRTY
                        && record.document_revision > 1)
                    || (document_editor_storage_refused_state(record.save_state)
                        // Open may fail after a filename prompt while keeping
                        // the previous clean body. Clean describes unmodified
                        // content; it does not assert storage success/durability.
                        && (record.dirty_state == DOCUMENT_EDITOR_DIRTY_CLEAN
                            || (record.dirty_state == DOCUMENT_EDITOR_DIRTY_DIRTY
                                && record.document_revision > 1))))
        }
        _ => false,
    }
}

pub fn compute_document_editor_reply_hash(reply: &DocumentEditorReply) -> u64 {
    let mut hash = domain_hash(b"osaiku.document-editor.reply.v1");
    hash = fold_u32(hash, reply.abi_version);
    hash = fold_u8(hash, reply.status);
    hash = fold_u8(hash, reply.decision);
    hash = fold_u8(hash, reply.reason);
    hash = fold_u8(hash, reply.command_kind);
    hash = fold_u32(hash, reply.result_flags);
    hash = fold_u32(hash, reply.reserved);
    hash = fold_u64(hash, reply.source_command_hash);
    fold_bytes(hash, &reply.state.encode())
}

pub fn document_editor_reply_hash_valid(reply: &DocumentEditorReply) -> bool {
    reply.reply_hash != 0 && compute_document_editor_reply_hash(reply) == reply.reply_hash
}

pub fn document_editor_reply_canonical(reply: &DocumentEditorReply) -> bool {
    let command_kind_canonical = if reply.command_kind == DOCUMENT_EDITOR_COMMAND_UNKNOWN {
        reply.decision == DOCUMENT_EDITOR_DECISION_REJECTED
            && reply.status == DOCUMENT_EDITOR_STATUS_INVALID_REQUEST
    } else {
        document_editor_command_kind_valid(reply.command_kind)
    };
    let resource_refusal_shape_valid = reply.status != DOCUMENT_EDITOR_STATUS_RESOURCE_UNAVAILABLE
        || (reply.command_kind == DOCUMENT_EDITOR_COMMAND_OPEN
            && reply.state == DocumentEditorStateRecord::canonical_unbound());
    reply.abi_version == DOCUMENT_EDITOR_ABI_VERSION
        && reply.reserved == 0
        && command_kind_canonical
        && resource_refusal_shape_valid
        && document_editor_state_record_canonical(&reply.state)
        && document_editor_reply_status_canonical(reply)
        && document_editor_reply_hash_valid(reply)
}

pub fn document_editor_result_flags(decision: u8) -> u32 {
    let common = DOCUMENT_EDITOR_RESULT_STATE_INCLUDED | DOCUMENT_EDITOR_RESULT_METADATA_ONLY;
    common
        | match decision {
            DOCUMENT_EDITOR_DECISION_READ_ONLY => DOCUMENT_EDITOR_RESULT_READ_ONLY,
            DOCUMENT_EDITOR_DECISION_APPLIED => DOCUMENT_EDITOR_RESULT_APPLIED,
            DOCUMENT_EDITOR_DECISION_NO_OP => DOCUMENT_EDITOR_RESULT_NO_OP,
            DOCUMENT_EDITOR_DECISION_REJECTED => DOCUMENT_EDITOR_RESULT_REJECTED,
            _ => 0,
        }
}

pub fn classify_document_editor_command(
    command: &DocumentEditorCommand,
    state: &DocumentEditorStateRecord,
) -> Option<(u8, u8)> {
    if command.abi_version != DOCUMENT_EDITOR_ABI_VERSION {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_ABI_VERSION);
    }
    if !document_editor_command_kind_valid(command.command_kind) {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_COMMAND_KIND);
    }

    let edit = document_editor_command_is_edit(command.command_kind);
    let expected_flags = match (command.command_kind, command.input_origin) {
        (_, DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE) if edit => {
            DOCUMENT_EDITOR_COMMAND_FLAGS_LOCAL_COMPATIBILITY_EDIT
        }
        (DOCUMENT_EDITOR_COMMAND_OPEN | DOCUMENT_EDITOR_COMMAND_QUERY, _) => {
            DOCUMENT_EDITOR_COMMAND_FLAGS_TRANSPORT
        }
        _ => 0,
    };
    if command.command_flags != expected_flags {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_COMMAND_FLAGS);
    }

    let origin_valid = match command.command_kind {
        DOCUMENT_EDITOR_COMMAND_QUERY => command.input_origin == DOCUMENT_EDITOR_INPUT_ORIGIN_NONE,
        DOCUMENT_EDITOR_COMMAND_OPEN => command.input_origin == DOCUMENT_EDITOR_INPUT_ORIGIN_NONE,
        _ if edit => {
            command.input_origin == DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE
        }
        _ => false,
    };
    if !origin_valid {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_INPUT_ORIGIN);
    }
    if command.encoding != DOCUMENT_EDITOR_ENCODING_ASCII {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_ENCODING);
    }
    if !document_editor_command_shape_valid(command) {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_COMMAND_SHAPE);
    }
    if edit && command.payload_len as usize <= DOCUMENT_EDITOR_BODY_CAPACITY {
        let payload = &command.payload[..command.payload_len as usize];
        let expected_evidence = compute_document_editor_local_compatibility_input_evidence_hash(
            command.command_kind,
            payload,
        );
        if command.input_evidence_hash != expected_evidence {
            return invalid(DOCUMENT_EDITOR_REASON_BAD_INPUT_EVIDENCE);
        }
    }
    if command.reserved_header != [0; 4] {
        return invalid(DOCUMENT_EDITOR_REASON_NONZERO_RESERVED);
    }
    if command.command_id == 0 {
        return invalid(DOCUMENT_EDITOR_REASON_ZERO_COMMAND_ID);
    }
    if !document_editor_command_hash_valid(command) {
        return invalid(DOCUMENT_EDITOR_REASON_BAD_COMMAND_HASH);
    }
    if command.payload_len as usize > DOCUMENT_EDITOR_BODY_CAPACITY {
        return Some((
            DOCUMENT_EDITOR_STATUS_CAPACITY_EXCEEDED,
            DOCUMENT_EDITOR_REASON_PAYLOAD_TOO_LARGE,
        ));
    }

    if command.command_kind == DOCUMENT_EDITOR_COMMAND_OPEN {
        if state.editor_state == DOCUMENT_EDITOR_STATE_OPEN {
            return Some((
                DOCUMENT_EDITOR_STATUS_ALREADY_OPEN,
                DOCUMENT_EDITOR_REASON_EDITOR_ALREADY_OPEN,
            ));
        }
    } else if command.command_kind != DOCUMENT_EDITOR_COMMAND_QUERY
        && state.editor_state != DOCUMENT_EDITOR_STATE_OPEN
    {
        return Some((
            DOCUMENT_EDITOR_STATUS_NOT_OPEN,
            DOCUMENT_EDITOR_REASON_EDITOR_NOT_OPEN,
        ));
    }

    if command.command_kind == DOCUMENT_EDITOR_COMMAND_OPEN {
        return None;
    }

    if command.expected_app_instance_id != state.app_instance_id {
        return stale_target(DOCUMENT_EDITOR_REASON_APP_INSTANCE_ID_MISMATCH);
    }
    if command.expected_app_instance_generation != state.app_instance_generation {
        return stale_target(DOCUMENT_EDITOR_REASON_APP_INSTANCE_GENERATION_MISMATCH);
    }
    if command.expected_surface_id != state.surface_id {
        return stale_target(DOCUMENT_EDITOR_REASON_SURFACE_ID_MISMATCH);
    }
    if command.expected_surface_generation != state.surface_generation {
        return stale_target(DOCUMENT_EDITOR_REASON_SURFACE_GENERATION_MISMATCH);
    }
    if command.expected_binding_generation != state.binding_generation {
        return stale_target(DOCUMENT_EDITOR_REASON_BINDING_GENERATION_MISMATCH);
    }
    if command.expected_binding_hash != state.binding_hash {
        return stale_target(DOCUMENT_EDITOR_REASON_BINDING_HASH_MISMATCH);
    }
    if command.command_kind == DOCUMENT_EDITOR_COMMAND_QUERY
        && command.expected_document_id == 0
        && command.expected_document_revision == 0
        && command.expected_editor_generation == 0
        && command.expected_semantic_state_generation == 0
        && command.expected_body_hash == 0
    {
        return None;
    }
    if command.expected_document_id != state.document_id {
        return stale_document(DOCUMENT_EDITOR_REASON_DOCUMENT_ID_MISMATCH);
    }
    if command.expected_document_revision != state.document_revision {
        return stale_document(DOCUMENT_EDITOR_REASON_DOCUMENT_REVISION_MISMATCH);
    }
    if command.expected_editor_generation != state.editor_generation {
        return stale_document(DOCUMENT_EDITOR_REASON_EDITOR_GENERATION_MISMATCH);
    }
    if command.expected_semantic_state_generation != state.semantic_state_generation {
        return stale_document(DOCUMENT_EDITOR_REASON_SEMANTIC_GENERATION_MISMATCH);
    }
    if command.expected_body_hash != state.body_hash {
        return stale_document(DOCUMENT_EDITOR_REASON_BODY_HASH_MISMATCH);
    }
    None
}

pub fn compute_document_editor_local_compatibility_input_evidence_hash(
    command_kind: u8,
    payload: &[u8],
) -> u64 {
    // Stable wire ABI v1 identity retained across the public symbol rename.
    let mut hash = domain_hash(b"osaiku.document-editor.live-emulated-local-input.v1");
    hash = fold_u8(hash, command_kind);
    hash = fold_u64(hash, payload.len() as u64);
    nonzero_hash(fold_bytes(hash, payload))
}

pub fn document_editor_ascii_payload_valid(payload: &[u8]) -> bool {
    payload
        .iter()
        .all(|byte| matches!(*byte, b'\n' | b'\r' | b'\t' | 0x20..=0x7e))
}

pub fn document_editor_command_is_edit(command_kind: u8) -> bool {
    matches!(
        command_kind,
        DOCUMENT_EDITOR_COMMAND_REPLACE
            | DOCUMENT_EDITOR_COMMAND_APPEND
            | DOCUMENT_EDITOR_COMMAND_BACKSPACE
            | DOCUMENT_EDITOR_COMMAND_CLEAR
    )
}

fn document_editor_command_kind_valid(command_kind: u8) -> bool {
    matches!(
        command_kind,
        DOCUMENT_EDITOR_COMMAND_OPEN
            | DOCUMENT_EDITOR_COMMAND_QUERY
            | DOCUMENT_EDITOR_COMMAND_REPLACE
            | DOCUMENT_EDITOR_COMMAND_APPEND
            | DOCUMENT_EDITOR_COMMAND_BACKSPACE
            | DOCUMENT_EDITOR_COMMAND_CLEAR
    )
}

fn document_editor_command_shape_valid(command: &DocumentEditorCommand) -> bool {
    let payload_len = command.payload_len as usize;
    if payload_len <= DOCUMENT_EDITOR_BODY_CAPACITY
        && command.payload[payload_len..].iter().any(|byte| *byte != 0)
    {
        return false;
    }

    let target_fields = [
        command.expected_app_instance_id,
        command.expected_app_instance_generation,
        command.expected_surface_id,
        command.expected_surface_generation,
        command.expected_binding_generation,
        command.expected_binding_hash,
    ];
    let required_target_present = command.expected_app_instance_id != 0
        && command.expected_app_instance_generation != 0
        && command.expected_surface_id != 0
        && command.expected_surface_generation != 0
        && command.expected_binding_generation != 0
        && command.expected_binding_hash != 0;
    let document_fields = [
        command.expected_document_id,
        command.expected_document_revision,
        command.expected_editor_generation,
        command.expected_semantic_state_generation,
        command.expected_body_hash,
    ];
    match command.command_kind {
        DOCUMENT_EDITOR_COMMAND_OPEN => {
            required_target_present
                && payload_len == 0
                && command.input_evidence_hash == 0
                && document_fields.iter().all(|value| *value == 0)
        }
        DOCUMENT_EDITOR_COMMAND_QUERY => {
            payload_len == 0
                && command.input_evidence_hash == 0
                && ((target_fields.iter().all(|value| *value == 0)
                    && document_fields.iter().all(|value| *value == 0))
                    || (required_target_present
                        && (document_fields.iter().all(|value| *value == 0)
                            || document_fields.iter().all(|value| *value != 0))))
        }
        DOCUMENT_EDITOR_COMMAND_REPLACE => {
            required_target_present
                && document_fields.iter().all(|value| *value != 0)
                && (payload_len > DOCUMENT_EDITOR_BODY_CAPACITY
                    || document_editor_ascii_payload_valid(&command.payload[..payload_len]))
        }
        DOCUMENT_EDITOR_COMMAND_APPEND => {
            required_target_present
                && document_fields.iter().all(|value| *value != 0)
                && (payload_len > DOCUMENT_EDITOR_BODY_CAPACITY
                    || (payload_len != 0
                        && document_editor_ascii_payload_valid(&command.payload[..payload_len])))
        }
        DOCUMENT_EDITOR_COMMAND_BACKSPACE | DOCUMENT_EDITOR_COMMAND_CLEAR => {
            required_target_present
                && payload_len == 0
                && document_fields.iter().all(|value| *value != 0)
        }
        _ => false,
    }
}

fn document_editor_reply_status_canonical(reply: &DocumentEditorReply) -> bool {
    let expected_flags = document_editor_result_flags(reply.decision);
    if reply.result_flags != expected_flags {
        return false;
    }
    match reply.decision {
        DOCUMENT_EDITOR_DECISION_READ_ONLY => {
            reply.status == DOCUMENT_EDITOR_STATUS_OK
                && reply.reason == DOCUMENT_EDITOR_REASON_NONE
                && reply.command_kind == DOCUMENT_EDITOR_COMMAND_QUERY
        }
        DOCUMENT_EDITOR_DECISION_APPLIED => {
            reply.status == DOCUMENT_EDITOR_STATUS_OK
                && reply.reason == DOCUMENT_EDITOR_REASON_NONE
                && reply.command_kind != DOCUMENT_EDITOR_COMMAND_QUERY
                && reply.source_command_hash != 0
                && reply.source_command_hash == reply.state.source_transition_hash
        }
        DOCUMENT_EDITOR_DECISION_NO_OP => {
            reply.status == DOCUMENT_EDITOR_STATUS_OK
                && matches!(
                    (reply.command_kind, reply.reason),
                    (
                        DOCUMENT_EDITOR_COMMAND_REPLACE,
                        DOCUMENT_EDITOR_REASON_UNCHANGED_REPLACEMENT
                    ) | (
                        DOCUMENT_EDITOR_COMMAND_BACKSPACE,
                        DOCUMENT_EDITOR_REASON_EMPTY_BACKSPACE
                    ) | (
                        DOCUMENT_EDITOR_COMMAND_CLEAR,
                        DOCUMENT_EDITOR_REASON_EMPTY_CLEAR
                    )
                )
        }
        DOCUMENT_EDITOR_DECISION_REJECTED => {
            matches!(
                (reply.status, reply.reason),
                (
                    DOCUMENT_EDITOR_STATUS_INVALID_REQUEST,
                    DOCUMENT_EDITOR_REASON_BAD_ABI_VERSION
                        | DOCUMENT_EDITOR_REASON_BAD_COMMAND_KIND
                        | DOCUMENT_EDITOR_REASON_BAD_COMMAND_FLAGS
                        | DOCUMENT_EDITOR_REASON_BAD_INPUT_ORIGIN
                        | DOCUMENT_EDITOR_REASON_BAD_ENCODING
                        | DOCUMENT_EDITOR_REASON_BAD_COMMAND_SHAPE
                        | DOCUMENT_EDITOR_REASON_BAD_INPUT_EVIDENCE
                        | DOCUMENT_EDITOR_REASON_NONZERO_RESERVED
                        | DOCUMENT_EDITOR_REASON_ZERO_COMMAND_ID
                        | DOCUMENT_EDITOR_REASON_BAD_COMMAND_HASH
                ) | (
                    DOCUMENT_EDITOR_STATUS_NOT_OPEN,
                    DOCUMENT_EDITOR_REASON_EDITOR_NOT_OPEN
                ) | (
                    DOCUMENT_EDITOR_STATUS_ALREADY_OPEN,
                    DOCUMENT_EDITOR_REASON_EDITOR_ALREADY_OPEN
                ) | (
                    DOCUMENT_EDITOR_STATUS_STALE_TARGET,
                    DOCUMENT_EDITOR_REASON_APP_INSTANCE_ID_MISMATCH
                        | DOCUMENT_EDITOR_REASON_APP_INSTANCE_GENERATION_MISMATCH
                        | DOCUMENT_EDITOR_REASON_SURFACE_ID_MISMATCH
                        | DOCUMENT_EDITOR_REASON_SURFACE_GENERATION_MISMATCH
                        | DOCUMENT_EDITOR_REASON_BINDING_GENERATION_MISMATCH
                        | DOCUMENT_EDITOR_REASON_BINDING_HASH_MISMATCH
                ) | (
                    DOCUMENT_EDITOR_STATUS_STALE_DOCUMENT,
                    DOCUMENT_EDITOR_REASON_DOCUMENT_ID_MISMATCH
                        | DOCUMENT_EDITOR_REASON_DOCUMENT_REVISION_MISMATCH
                        | DOCUMENT_EDITOR_REASON_EDITOR_GENERATION_MISMATCH
                        | DOCUMENT_EDITOR_REASON_SEMANTIC_GENERATION_MISMATCH
                        | DOCUMENT_EDITOR_REASON_BODY_HASH_MISMATCH
                ) | (
                    DOCUMENT_EDITOR_STATUS_CAPACITY_EXCEEDED,
                    DOCUMENT_EDITOR_REASON_PAYLOAD_TOO_LARGE
                        | DOCUMENT_EDITOR_REASON_RESULT_CAPACITY_EXCEEDED
                ) | (
                    DOCUMENT_EDITOR_STATUS_STATE_EXHAUSTED,
                    DOCUMENT_EDITOR_REASON_STATE_COUNTER_EXHAUSTED
                ) | (
                    DOCUMENT_EDITOR_STATUS_RESOURCE_UNAVAILABLE,
                    DOCUMENT_EDITOR_REASON_SESSION_PROCESS_QUOTA
                        | DOCUMENT_EDITOR_REASON_SESSION_PROTECTED_RESERVE
                        | DOCUMENT_EDITOR_REASON_SESSION_TOTAL_EXHAUSTED
                        | DOCUMENT_EDITOR_REASON_SESSION_FRAGMENTED
                        | DOCUMENT_EDITOR_REASON_SESSION_VIRTUAL_RANGE_EXHAUSTED
                        | DOCUMENT_EDITOR_REASON_SESSION_MAPPING_UNAVAILABLE
                        | DOCUMENT_EDITOR_REASON_SESSION_TRANSITION_EXHAUSTED
                )
            )
        }
        _ => false,
    }
}

fn editor_identity_fields_zero(record: &DocumentEditorStateRecord) -> bool {
    record.app_instance_id == 0
        && record.app_instance_generation == 0
        && record.surface_id == 0
        && record.surface_generation == 0
        && record.binding_generation == 0
        && record.binding_hash == 0
        && record.document_id == 0
        && record.document_revision == 0
        && record.editor_generation == 0
        && record.semantic_state_generation == 0
        && record.body_hash == 0
        && record.source_transition_hash == 0
}

fn invalid(reason: u8) -> Option<(u8, u8)> {
    Some((DOCUMENT_EDITOR_STATUS_INVALID_REQUEST, reason))
}

fn stale_target(reason: u8) -> Option<(u8, u8)> {
    Some((DOCUMENT_EDITOR_STATUS_STALE_TARGET, reason))
}

fn stale_document(reason: u8) -> Option<(u8, u8)> {
    Some((DOCUMENT_EDITOR_STATUS_STALE_DOCUMENT, reason))
}

fn domain_hash(domain: &[u8]) -> u64 {
    fold_bytes(AUDIT_JOURNAL_FNV1A_OFFSET, domain)
}

fn nonzero_hash(hash: u64) -> u64 {
    if hash == 0 {
        1
    } else {
        hash
    }
}

fn fold_u8(hash: u64, value: u8) -> u64 {
    fold_bytes(hash, &[value])
}

fn fold_u32(hash: u64, value: u32) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

fn fold_u64(hash: u64, value: u64) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

fn fold_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(AUDIT_JOURNAL_FNV1A_PRIME);
    }
    hash
}

fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
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
    fn cancelled_empty_prompt_and_failed_open_preserve_clean_content() {
        let mut state = open_state();
        state.document_revision = 10;
        state.editor_generation = 12;
        state.semantic_state_generation = 12;
        state.record_hash = compute_document_editor_state_record_hash(&state);
        assert!(document_editor_state_record_canonical(&state));
        for body in [b"".as_slice(), b"old document".as_slice()] {
            state.body_len = body.len() as u8;
            state.body_hash = compute_document_editor_body_hash(body);
            state.content_state = if body.is_empty() {
                DOCUMENT_EDITOR_CONTENT_EMPTY
            } else {
                DOCUMENT_EDITOR_CONTENT_PRESENT
            };
            for error in [
                DOCUMENT_EDITOR_SAVE_STORAGE_UNAVAILABLE,
                DOCUMENT_EDITOR_SAVE_INCOMPATIBLE,
                DOCUMENT_EDITOR_SAVE_FILE_CONFLICT,
                DOCUMENT_EDITOR_SAVE_STALE_TARGET,
            ] {
                state.save_state = error;
                state.record_hash = compute_document_editor_state_record_hash(&state);
                assert!(document_editor_state_record_canonical(&state));
            }
        }
        // A successful durable save still cannot describe dirty contents.
        state.save_state = DOCUMENT_EDITOR_SAVE_DURABLE_CURRENT;
        state.dirty_state = DOCUMENT_EDITOR_DIRTY_DIRTY;
        state.record_hash = compute_document_editor_state_record_hash(&state);
        assert!(!document_editor_state_record_canonical(&state));
    }

    fn local_compatibility_append(state: &DocumentEditorStateRecord) -> DocumentEditorCommand {
        let payload = b"x";
        let mut command = exact_query(state);
        command.command_kind = DOCUMENT_EDITOR_COMMAND_APPEND;
        command.input_origin = DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE;
        command.command_flags = DOCUMENT_EDITOR_COMMAND_FLAGS_LOCAL_COMPATIBILITY_EDIT;
        command.payload_len = payload.len() as u8;
        command.payload[..payload.len()].copy_from_slice(payload);
        command.input_evidence_hash =
            compute_document_editor_local_compatibility_input_evidence_hash(
                command.command_kind,
                payload,
            );
        command.command_hash = compute_document_editor_command_hash(&command);
        command
    }

    #[test]
    fn exact_layouts_and_wire_round_trips() {
        assert_eq!(
            core::mem::size_of::<DocumentEditorCommand>(),
            DOCUMENT_EDITOR_COMMAND_SIZE
        );
        assert_eq!(
            core::mem::size_of::<DocumentEditorStateRecord>(),
            DOCUMENT_EDITOR_STATE_RECORD_SIZE
        );
        assert_eq!(
            core::mem::size_of::<DocumentEditorReply>(),
            DOCUMENT_EDITOR_REPLY_SIZE
        );

        let command = open_command();
        let encoded = command.encode();
        assert_eq!(DocumentEditorCommand::decode(&encoded), Some(command));
        assert_eq!(&encoded[120..152], &[0; DOCUMENT_EDITOR_BODY_CAPACITY]);

        let state = open_state();
        assert_eq!(
            DocumentEditorStateRecord::decode(&state.encode()),
            Some(state)
        );

        let mut reply = DocumentEditorReply {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            status: DOCUMENT_EDITOR_STATUS_OK,
            decision: DOCUMENT_EDITOR_DECISION_READ_ONLY,
            reason: DOCUMENT_EDITOR_REASON_NONE,
            command_kind: DOCUMENT_EDITOR_COMMAND_QUERY,
            result_flags: document_editor_result_flags(DOCUMENT_EDITOR_DECISION_READ_ONLY),
            reserved: 0,
            source_command_hash: exact_query(&state).command_hash,
            state,
            reply_hash: 0,
        };
        reply.reply_hash = compute_document_editor_reply_hash(&reply);
        assert_eq!(DocumentEditorReply::decode(&reply.encode()), Some(reply));
        assert!(document_editor_reply_canonical(&reply));
    }

    #[test]
    fn canonical_unbound_and_active_states_are_distinct() {
        let unbound = DocumentEditorStateRecord::canonical_unbound();
        assert!(document_editor_state_record_canonical(&unbound));
        let active = open_state();
        assert!(document_editor_state_record_canonical(&active));

        let mut leaked_identity = unbound;
        leaked_identity.document_id = 1;
        leaked_identity.record_hash = compute_document_editor_state_record_hash(&leaked_identity);
        assert!(!document_editor_state_record_canonical(&leaked_identity));

        let mut impossible_save = unbound;
        impossible_save.save_state = DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT;
        impossible_save.record_hash = compute_document_editor_state_record_hash(&impossible_save);
        assert!(!document_editor_state_record_canonical(&impossible_save));
    }

    #[test]
    fn unbound_query_is_read_only_and_open_is_exact_lifecycle_setup() {
        let state = DocumentEditorStateRecord::canonical_unbound();
        let query = exact_query(&state);
        assert_eq!(query.input_origin, DOCUMENT_EDITOR_INPUT_ORIGIN_NONE);
        assert_eq!(classify_document_editor_command(&query, &state), None);

        let open = open_command();
        assert_eq!(classify_document_editor_command(&open, &state), None);
    }

    #[test]
    fn evidence_valid_edit_still_requires_an_open_document() {
        let state = DocumentEditorStateRecord::canonical_unbound();
        let mut command = local_compatibility_append(&state);
        command.expected_app_instance_id = 1;
        command.expected_app_instance_generation = 1;
        command.expected_surface_id = 1;
        command.expected_surface_generation = 1;
        command.expected_binding_generation = 1;
        command.expected_binding_hash = 1;
        command.expected_document_id = 1;
        command.expected_document_revision = 1;
        command.expected_editor_generation = 1;
        command.expected_semantic_state_generation = 1;
        command.expected_body_hash = 1;
        command.command_hash = compute_document_editor_command_hash(&command);
        assert_eq!(
            classify_document_editor_command(&command, &state),
            Some((
                DOCUMENT_EDITOR_STATUS_NOT_OPEN,
                DOCUMENT_EDITOR_REASON_EDITOR_NOT_OPEN
            ))
        );
    }

    #[test]
    fn volatile_save_states_preserve_document_revision_relations() {
        let mut dirty = open_state();
        dirty.content_state = DOCUMENT_EDITOR_CONTENT_PRESENT;
        dirty.dirty_state = DOCUMENT_EDITOR_DIRTY_DIRTY;
        dirty.last_input_origin = DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE;
        dirty.source_flags = DOCUMENT_EDITOR_SOURCE_LOCAL_COMPATIBILITY_CANONICAL;
        dirty.body_len = 1;
        dirty.semantic_flags = DOCUMENT_EDITOR_SEMANTIC_BASE
            | DOCUMENT_EDITOR_SEMANTIC_LOCAL_COMPATIBILITY_INPUT_HONEST;
        dirty.boundary_flags = DOCUMENT_EDITOR_BOUNDARY_LOCAL_COMPATIBILITY_CANONICAL;
        dirty.document_revision = 2;
        dirty.editor_generation = 2;
        dirty.semantic_state_generation = 2;
        dirty.body_hash = compute_document_editor_body_hash(b"x");
        dirty.source_transition_hash = 71;
        dirty.record_hash = compute_document_editor_state_record_hash(&dirty);
        assert!(document_editor_state_record_canonical(&dirty));

        let mut saved = dirty;
        saved.dirty_state = DOCUMENT_EDITOR_DIRTY_CLEAN;
        saved.save_state = DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT;
        saved.editor_generation = 3;
        saved.semantic_state_generation = 3;
        saved.source_transition_hash = 72;
        saved.record_hash = compute_document_editor_state_record_hash(&saved);
        assert!(document_editor_state_record_canonical(&saved));
        assert_eq!(saved.document_revision, dirty.document_revision);
        assert_eq!(saved.body_hash, dirty.body_hash);

        let mut stale = saved;
        stale.dirty_state = DOCUMENT_EDITOR_DIRTY_DIRTY;
        stale.save_state = DOCUMENT_EDITOR_SAVE_VOLATILE_STALE;
        stale.document_revision = 3;
        stale.editor_generation = 4;
        stale.semantic_state_generation = 4;
        stale.source_transition_hash = 73;
        stale.record_hash = compute_document_editor_state_record_hash(&stale);
        assert!(document_editor_state_record_canonical(&stale));

        let mut impossible = stale;
        impossible.save_state = DOCUMENT_EDITOR_SAVE_VOLATILE_CURRENT;
        impossible.record_hash = compute_document_editor_state_record_hash(&impossible);
        assert!(!document_editor_state_record_canonical(&impossible));
    }

    #[test]
    fn hashes_cover_wire_fields_and_detect_tampering() {
        let command = open_command();
        let state = open_state();
        assert_ne!(command.command_hash, 0);
        assert_ne!(state.document_id, 0);
        assert_ne!(state.body_hash, 0);
        assert_ne!(state.record_hash, 0);

        let mut command_tamper = command;
        command_tamper.payload[31] = 1;
        assert!(!document_editor_command_hash_valid(&command_tamper));

        let mut state_tamper = state;
        state_tamper.boundary_flags ^= DOCUMENT_EDITOR_BOUNDARY_NO_AI_MEDIATION;
        assert!(!document_editor_state_record_hash_valid(&state_tamper));

        let query = exact_query(&state);
        let mut reply = DocumentEditorReply {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            status: DOCUMENT_EDITOR_STATUS_OK,
            decision: DOCUMENT_EDITOR_DECISION_READ_ONLY,
            reason: DOCUMENT_EDITOR_REASON_NONE,
            command_kind: DOCUMENT_EDITOR_COMMAND_QUERY,
            result_flags: document_editor_result_flags(DOCUMENT_EDITOR_DECISION_READ_ONLY),
            reserved: 0,
            source_command_hash: query.command_hash,
            state,
            reply_hash: 0,
        };
        reply.reply_hash = compute_document_editor_reply_hash(&reply);
        assert_ne!(query.command_hash, 0);
        assert_ne!(reply.reply_hash, 0);
        reply.state.body_len = 1;
        assert!(!document_editor_reply_hash_valid(&reply));
    }

    #[test]
    fn malformed_and_stale_classification_order_is_deterministic() {
        let state = open_state();
        let mut command = exact_query(&state);
        command.expected_app_instance_id ^= 1;
        command.reserved_header[0] = 1;
        command.command_hash = 0;
        assert_eq!(
            classify_document_editor_command(&command, &state),
            invalid(DOCUMENT_EDITOR_REASON_NONZERO_RESERVED)
        );

        command.reserved_header = [0; 4];
        assert_eq!(
            classify_document_editor_command(&command, &state),
            invalid(DOCUMENT_EDITOR_REASON_BAD_COMMAND_HASH)
        );

        command.command_hash = compute_document_editor_command_hash(&command);
        assert_eq!(
            classify_document_editor_command(&command, &state),
            stale_target(DOCUMENT_EDITOR_REASON_APP_INSTANCE_ID_MISMATCH)
        );

        command.expected_app_instance_id = state.app_instance_id;
        command.expected_document_revision += 1;
        command.command_hash = compute_document_editor_command_hash(&command);
        assert_eq!(
            classify_document_editor_command(&command, &state),
            stale_document(DOCUMENT_EDITOR_REASON_DOCUMENT_REVISION_MISMATCH)
        );
    }

    #[test]
    fn oversized_payload_is_classified_after_a_valid_hash() {
        let state = open_state();
        let mut command = exact_query(&state);
        command.command_kind = DOCUMENT_EDITOR_COMMAND_REPLACE;
        command.input_origin = DOCUMENT_EDITOR_INPUT_ORIGIN_LOCAL_COMPATIBILITY_DEVICE;
        command.command_flags = DOCUMENT_EDITOR_COMMAND_FLAGS_LOCAL_COMPATIBILITY_EDIT;
        command.payload_len = 33;
        command.input_evidence_hash = 1;
        command.command_hash = compute_document_editor_command_hash(&command);
        assert_eq!(
            classify_document_editor_command(&command, &state),
            Some((
                DOCUMENT_EDITOR_STATUS_CAPACITY_EXCEEDED,
                DOCUMENT_EDITOR_REASON_PAYLOAD_TOO_LARGE
            ))
        );
    }

    #[test]
    fn local_compatibility_input_evidence_and_ascii_are_bound() {
        let payload = b"local input\n";
        let hash = compute_document_editor_local_compatibility_input_evidence_hash(
            DOCUMENT_EDITOR_COMMAND_APPEND,
            payload,
        );
        assert_ne!(hash, 0);
        assert!(document_editor_ascii_payload_valid(payload));
        assert!(!document_editor_ascii_payload_valid(&[0, 0xff]));
        assert_ne!(
            hash,
            compute_document_editor_local_compatibility_input_evidence_hash(
                DOCUMENT_EDITOR_COMMAND_REPLACE,
                payload
            )
        );
    }

    #[test]
    fn malformed_unknown_command_has_a_canonical_rejection_reply() {
        let state = DocumentEditorStateRecord::canonical_unbound();
        let mut reply = DocumentEditorReply {
            abi_version: DOCUMENT_EDITOR_ABI_VERSION,
            status: DOCUMENT_EDITOR_STATUS_INVALID_REQUEST,
            decision: DOCUMENT_EDITOR_DECISION_REJECTED,
            reason: DOCUMENT_EDITOR_REASON_BAD_ABI_VERSION,
            command_kind: DOCUMENT_EDITOR_COMMAND_UNKNOWN,
            result_flags: document_editor_result_flags(DOCUMENT_EDITOR_DECISION_REJECTED),
            reserved: 0,
            source_command_hash: 0,
            state,
            reply_hash: 0,
        };
        reply.reply_hash = compute_document_editor_reply_hash(&reply);
        assert!(document_editor_reply_canonical(&reply));
    }
}
