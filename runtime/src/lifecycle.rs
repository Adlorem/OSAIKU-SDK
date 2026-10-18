// Copyright (c) OSAIKU contributors.

//! Application-facing session teardown messages, ABI v2.
//!
//! A notice reports a compositor-accepted window destroy. It does not authorize
//! arbitrary unmapping or process exit. Decode checks length only; canonical
//! validators check metadata and unkeyed hashes, not the sender, freshness,
//! ownership, or the session key's derivation. Before release, authenticate the
//! IPC sender, reject replayed notice IDs, check the local owner/window/session
//! identity, and use the exact kernel-issued mapping receipt.
//!
//! Receipts are diagnostic self-reports. The system independently checks the
//! kernel's mapping registry. Serialize with encode/decode: the notice's native
//! struct has trailing padding and is NOT its 156-byte wire representation.

const AUDIT_JOURNAL_FNV1A_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const AUDIT_JOURNAL_FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

mod receipt;
mod validation;
pub use validation::{validate_window_session_teardown_notice, TeardownNoticeError};

pub const WINDOW_SESSION_TEARDOWN_ABI_VERSION: u32 = 2;

pub const WINDOW_SESSION_TEARDOWN_NOTICE_SIZE: usize = 156;

pub const WINDOW_SESSION_TEARDOWN_RECEIPT_SIZE: usize = 120;

pub const WINDOW_SESSION_TEARDOWN_NOTICE_STATE_READY: u8 = 1;
pub const WINDOW_SESSION_TEARDOWN_OPERATION_RELEASE: u8 = 1;

pub const WINDOW_SESSION_TEARDOWN_NOTICE_METADATA_ONLY: u32 = 1 << 0;
pub const WINDOW_SESSION_TEARDOWN_NOTICE_WINDOW_DESTROYED: u32 = 1 << 1;
pub const WINDOW_SESSION_TEARDOWN_NOTICE_IDENTITY_BOUND: u32 = 1 << 2;
pub const WINDOW_SESSION_TEARDOWN_NOTICE_FLAGS_CANONICAL: u32 =
    WINDOW_SESSION_TEARDOWN_NOTICE_METADATA_ONLY
        | WINDOW_SESSION_TEARDOWN_NOTICE_WINDOW_DESTROYED
        | WINDOW_SESSION_TEARDOWN_NOTICE_IDENTITY_BOUND;

/// What initd declares it is *not* doing by sending this.
pub const WINDOW_SESSION_TEARDOWN_AUTHORITY_NOTICE_ONLY: u32 = 1 << 0;
pub const WINDOW_SESSION_TEARDOWN_AUTHORITY_NO_APP_EXIT: u32 = 1 << 1;
pub const WINDOW_SESSION_TEARDOWN_AUTHORITY_NO_OTHER_SESSION: u32 = 1 << 2;
pub const WINDOW_SESSION_TEARDOWN_AUTHORITY_NO_CONTENT_READ: u32 = 1 << 3;
pub const WINDOW_SESSION_TEARDOWN_AUTHORITY_CANONICAL: u32 =
    WINDOW_SESSION_TEARDOWN_AUTHORITY_NOTICE_ONLY
        | WINDOW_SESSION_TEARDOWN_AUTHORITY_NO_APP_EXIT
        | WINDOW_SESSION_TEARDOWN_AUTHORITY_NO_OTHER_SESSION
        | WINDOW_SESSION_TEARDOWN_AUTHORITY_NO_CONTENT_READ;

pub const WINDOW_SESSION_TEARDOWN_RECEIPT_METADATA_ONLY: u32 = 1 << 0;
pub const WINDOW_SESSION_TEARDOWN_RECEIPT_NO_PARTIAL_MUTATION: u32 = 1 << 1;
pub const WINDOW_SESSION_TEARDOWN_RECEIPT_FLAGS_CANONICAL: u32 =
    WINDOW_SESSION_TEARDOWN_RECEIPT_METADATA_ONLY
        | WINDOW_SESSION_TEARDOWN_RECEIPT_NO_PARTIAL_MUTATION;

pub const WINDOW_SESSION_TEARDOWN_STATUS_OK: u8 = 0;
pub const WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE: u8 = 1;
pub const WINDOW_SESSION_TEARDOWN_STATUS_STALE: u8 = 2;
pub const WINDOW_SESSION_TEARDOWN_STATUS_UNAVAILABLE: u8 = 3;

pub const WINDOW_SESSION_TEARDOWN_DECISION_RELEASED: u8 = 1;
pub const WINDOW_SESSION_TEARDOWN_DECISION_REJECTED: u8 = 2;

pub const WINDOW_SESSION_TEARDOWN_REASON_NONE: u8 = 0;
pub const WINDOW_SESSION_TEARDOWN_REASON_INVALID_SHAPE: u8 = 1;
/// A notice id at or below the last one answered. A teardown happens once.
pub const WINDOW_SESSION_TEARDOWN_REASON_REPLAYED_NOTICE: u8 = 2;
/// The application holds no session under that identity.
pub const WINDOW_SESSION_TEARDOWN_REASON_UNKNOWN_SESSION: u8 = 3;
/// The session key does not follow from the identity fields beside it.
pub const WINDOW_SESSION_TEARDOWN_REASON_IDENTITY_MISMATCH: u8 = 4;

/// What initd sends the owning application after an accepted `DESTROY`.
///
/// This is a statement, not a request: the window is already gone. What the
/// application decides is whether the identity names a session it holds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSessionTeardownNotice {
    pub abi_version: u32,
    pub notice_state: u8,
    pub operation: u8,
    pub reason: u8,
    pub reserved0: u8,
    pub notice_flags: u32,
    pub authority_flags: u32,
    pub notice_id: u64,
    pub window_id: u64,
    pub window_generation: u64,
    pub owner_app_id: u64,
    /// Numeric PID, not an encoded ProcessEndpoint. Authenticate the sender
    /// and track endpoint generations separately at the IPC boundary.
    pub owner_process_id: u64,
    pub app_instance_id: u64,
    pub app_instance_generation: u64,
    pub surface_id: u64,
    pub surface_generation: u64,
    pub binding_generation: u64,
    pub binding_hash: u64,
    /// The key the session was mapped under, derived from the six identity
    /// fields above. Carried so the application can check the identity against
    /// the key rather than trusting either one alone.
    pub session_key: u64,
    /// The compositor's state after the destroy this notice reports.
    pub compositor_generation: u64,
    pub compositor_record_hash: u64,
    pub transition_receipt_hash: u64,
    pub provenance_hash: u64,
    pub notice_hash: u64,
    pub reserved1: [u8; 4],
}

impl WindowSessionTeardownNotice {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        notice_state: 0,
        operation: 0,
        reason: 0,
        reserved0: 0,
        notice_flags: 0,
        authority_flags: 0,
        notice_id: 0,
        window_id: 0,
        window_generation: 0,
        owner_app_id: 0,
        owner_process_id: 0,
        app_instance_id: 0,
        app_instance_generation: 0,
        surface_id: 0,
        surface_generation: 0,
        binding_generation: 0,
        binding_hash: 0,
        session_key: 0,
        compositor_generation: 0,
        compositor_record_hash: 0,
        transition_receipt_hash: 0,
        provenance_hash: 0,
        notice_hash: 0,
        reserved1: [0; 4],
    };

    pub fn encode(&self) -> [u8; WINDOW_SESSION_TEARDOWN_NOTICE_SIZE] {
        let mut out = [0u8; WINDOW_SESSION_TEARDOWN_NOTICE_SIZE];
        put_u32(&mut out, 0, self.abi_version);
        out[4] = self.notice_state;
        out[5] = self.operation;
        out[6] = self.reason;
        out[7] = self.reserved0;
        put_u32(&mut out, 8, self.notice_flags);
        put_u32(&mut out, 12, self.authority_flags);
        for (index, field) in self.fields().into_iter().enumerate() {
            put_u64(&mut out, 16 + index * 8, field);
        }
        out[152..156].copy_from_slice(&self.reserved1);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != WINDOW_SESSION_TEARDOWN_NOTICE_SIZE {
            return None;
        }
        let mut reserved1 = [0u8; 4];
        reserved1.copy_from_slice(&bytes[152..156]);
        Some(Self {
            abi_version: get_u32(bytes, 0),
            notice_state: bytes[4],
            operation: bytes[5],
            reason: bytes[6],
            reserved0: bytes[7],
            notice_flags: get_u32(bytes, 8),
            authority_flags: get_u32(bytes, 12),
            notice_id: get_u64(bytes, 16),
            window_id: get_u64(bytes, 24),
            window_generation: get_u64(bytes, 32),
            owner_app_id: get_u64(bytes, 40),
            owner_process_id: get_u64(bytes, 48),
            app_instance_id: get_u64(bytes, 56),
            app_instance_generation: get_u64(bytes, 64),
            surface_id: get_u64(bytes, 72),
            surface_generation: get_u64(bytes, 80),
            binding_generation: get_u64(bytes, 88),
            binding_hash: get_u64(bytes, 96),
            session_key: get_u64(bytes, 104),
            compositor_generation: get_u64(bytes, 112),
            compositor_record_hash: get_u64(bytes, 120),
            transition_receipt_hash: get_u64(bytes, 128),
            provenance_hash: get_u64(bytes, 136),
            notice_hash: get_u64(bytes, 144),
            reserved1,
        })
    }

    fn fields(&self) -> [u64; 17] {
        [
            self.notice_id,
            self.window_id,
            self.window_generation,
            self.owner_app_id,
            self.owner_process_id,
            self.app_instance_id,
            self.app_instance_generation,
            self.surface_id,
            self.surface_generation,
            self.binding_generation,
            self.binding_hash,
            self.session_key,
            self.compositor_generation,
            self.compositor_record_hash,
            self.transition_receipt_hash,
            self.provenance_hash,
            self.notice_hash,
        ]
    }
}

/// What the application reports after handling the notice. The kernel-owned
/// session registry, not this self-report, is the release authority.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSessionTeardownReceipt {
    pub abi_version: u32,
    pub status: u8,
    pub decision: u8,
    pub reason: u8,
    pub operation: u8,
    pub receipt_flags: u32,
    pub authority_flags: u32,
    pub notice_id: u64,
    pub notice_hash: u64,
    pub window_id: u64,
    pub owner_app_id: u64,
    pub owner_process_id: u64,
    pub session_key: u64,
    pub released_mapping_id: u64,
    pub released_mapping_generation: u64,
    pub released_mapping_hash: u64,
    pub remaining_session_count: u64,
    pub provenance_hash: u64,
    pub receipt_hash: u64,
    pub reserved: [u8; 8],
}

impl WindowSessionTeardownReceipt {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        status: WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE,
        decision: WINDOW_SESSION_TEARDOWN_DECISION_REJECTED,
        reason: WINDOW_SESSION_TEARDOWN_REASON_INVALID_SHAPE,
        operation: 0,
        receipt_flags: 0,
        authority_flags: 0,
        notice_id: 0,
        notice_hash: 0,
        window_id: 0,
        owner_app_id: 0,
        owner_process_id: 0,
        session_key: 0,
        released_mapping_id: 0,
        released_mapping_generation: 0,
        released_mapping_hash: 0,
        remaining_session_count: 0,
        provenance_hash: 0,
        receipt_hash: 0,
        reserved: [0; 8],
    };

    pub fn encode(&self) -> [u8; WINDOW_SESSION_TEARDOWN_RECEIPT_SIZE] {
        let mut out = [0u8; WINDOW_SESSION_TEARDOWN_RECEIPT_SIZE];
        put_u32(&mut out, 0, self.abi_version);
        out[4] = self.status;
        out[5] = self.decision;
        out[6] = self.reason;
        out[7] = self.operation;
        put_u32(&mut out, 8, self.receipt_flags);
        put_u32(&mut out, 12, self.authority_flags);
        for (index, field) in self.fields().into_iter().enumerate() {
            put_u64(&mut out, 16 + index * 8, field);
        }
        out[112..120].copy_from_slice(&self.reserved);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != WINDOW_SESSION_TEARDOWN_RECEIPT_SIZE {
            return None;
        }
        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&bytes[112..120]);
        Some(Self {
            abi_version: get_u32(bytes, 0),
            status: bytes[4],
            decision: bytes[5],
            reason: bytes[6],
            operation: bytes[7],
            receipt_flags: get_u32(bytes, 8),
            authority_flags: get_u32(bytes, 12),
            notice_id: get_u64(bytes, 16),
            notice_hash: get_u64(bytes, 24),
            window_id: get_u64(bytes, 32),
            owner_app_id: get_u64(bytes, 40),
            owner_process_id: get_u64(bytes, 48),
            session_key: get_u64(bytes, 56),
            released_mapping_id: get_u64(bytes, 64),
            released_mapping_generation: get_u64(bytes, 72),
            released_mapping_hash: get_u64(bytes, 80),
            remaining_session_count: get_u64(bytes, 88),
            provenance_hash: get_u64(bytes, 96),
            receipt_hash: get_u64(bytes, 104),
            reserved,
        })
    }

    fn fields(&self) -> [u64; 12] {
        [
            self.notice_id,
            self.notice_hash,
            self.window_id,
            self.owner_app_id,
            self.owner_process_id,
            self.session_key,
            self.released_mapping_id,
            self.released_mapping_generation,
            self.released_mapping_hash,
            self.remaining_session_count,
            self.provenance_hash,
            self.receipt_hash,
        ]
    }
}

/// The identity the session was opened under, folded on its own.
///
/// This binds owner, six identity fields and the supplied session key into the
/// notice provenance. It does not derive or validate that key. Compare it with
/// `window::compute_window_document_id` and the trusted local session separately.
pub fn compute_window_session_teardown_identity_hash(notice: &WindowSessionTeardownNotice) -> u64 {
    let mut hash = domain_hash(b"osaiku.window-session-teardown.identity.v2");
    hash = fold_u64(hash, notice.owner_app_id);
    hash = fold_u64(hash, notice.owner_process_id);
    hash = fold_u64(hash, notice.app_instance_id);
    hash = fold_u64(hash, notice.app_instance_generation);
    hash = fold_u64(hash, notice.surface_id);
    hash = fold_u64(hash, notice.surface_generation);
    hash = fold_u64(hash, notice.binding_generation);
    hash = fold_u64(hash, notice.binding_hash);
    nonzero(fold_u64(hash, notice.session_key))
}

pub fn compute_window_session_teardown_notice_provenance_hash(
    notice: &WindowSessionTeardownNotice,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.window-session-teardown.notice-provenance.v2");
    hash = fold_u64(hash, notice.notice_id);
    hash = fold_u64(hash, notice.window_id);
    hash = fold_u64(hash, notice.window_generation);
    hash = fold_u64(hash, notice.owner_app_id);
    hash = fold_u64(hash, notice.owner_process_id);
    hash = fold_u64(hash, notice.compositor_generation);
    hash = fold_u64(hash, notice.compositor_record_hash);
    hash = fold_u64(hash, notice.transition_receipt_hash);
    nonzero(fold_u64(
        hash,
        compute_window_session_teardown_identity_hash(notice),
    ))
}

pub fn compute_window_session_teardown_notice_hash(notice: &WindowSessionTeardownNotice) -> u64 {
    let mut hash = domain_hash(b"osaiku.window-session-teardown.notice.v2");
    hash = fold_u32(hash, notice.abi_version);
    hash = fold_u8(hash, notice.notice_state);
    hash = fold_u8(hash, notice.operation);
    hash = fold_u8(hash, notice.reason);
    hash = fold_u8(hash, notice.reserved0);
    hash = fold_u32(hash, notice.notice_flags);
    hash = fold_u32(hash, notice.authority_flags);
    hash = fold_u64(hash, notice.notice_id);
    hash = fold_u64(hash, notice.window_id);
    hash = fold_u64(hash, notice.window_generation);
    hash = fold_u64(hash, notice.owner_app_id);
    hash = fold_u64(hash, notice.owner_process_id);
    hash = fold_u64(hash, notice.app_instance_id);
    hash = fold_u64(hash, notice.app_instance_generation);
    hash = fold_u64(hash, notice.surface_id);
    hash = fold_u64(hash, notice.surface_generation);
    hash = fold_u64(hash, notice.binding_generation);
    hash = fold_u64(hash, notice.binding_hash);
    hash = fold_u64(hash, notice.session_key);
    hash = fold_u64(hash, notice.compositor_generation);
    hash = fold_u64(hash, notice.compositor_record_hash);
    hash = fold_u64(hash, notice.transition_receipt_hash);
    hash = fold_u64(hash, notice.provenance_hash);
    nonzero(fold(hash, &notice.reserved1))
}

pub fn compute_window_session_teardown_receipt_provenance_hash(
    receipt: &WindowSessionTeardownReceipt,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.window-session-teardown.receipt-provenance.v2");
    hash = fold_u8(hash, receipt.status);
    hash = fold_u8(hash, receipt.decision);
    hash = fold_u8(hash, receipt.reason);
    hash = fold_u64(hash, receipt.notice_id);
    hash = fold_u64(hash, receipt.notice_hash);
    hash = fold_u64(hash, receipt.owner_app_id);
    hash = fold_u64(hash, receipt.owner_process_id);
    nonzero(fold_u64(hash, receipt.session_key))
}

pub fn compute_window_session_teardown_receipt_hash(receipt: &WindowSessionTeardownReceipt) -> u64 {
    let mut hash = domain_hash(b"osaiku.window-session-teardown.receipt.v2");
    hash = fold_u32(hash, receipt.abi_version);
    hash = fold_u8(hash, receipt.status);
    hash = fold_u8(hash, receipt.decision);
    hash = fold_u8(hash, receipt.reason);
    hash = fold_u8(hash, receipt.operation);
    hash = fold_u32(hash, receipt.receipt_flags);
    hash = fold_u32(hash, receipt.authority_flags);
    hash = fold_u64(hash, receipt.notice_id);
    hash = fold_u64(hash, receipt.notice_hash);
    hash = fold_u64(hash, receipt.window_id);
    hash = fold_u64(hash, receipt.owner_app_id);
    hash = fold_u64(hash, receipt.owner_process_id);
    hash = fold_u64(hash, receipt.session_key);
    hash = fold_u64(hash, receipt.released_mapping_id);
    hash = fold_u64(hash, receipt.released_mapping_generation);
    hash = fold_u64(hash, receipt.released_mapping_hash);
    hash = fold_u64(hash, receipt.remaining_session_count);
    hash = fold_u64(hash, receipt.provenance_hash);
    nonzero(fold(hash, &receipt.reserved))
}

pub fn window_session_teardown_notice_canonical(notice: &WindowSessionTeardownNotice) -> bool {
    notice.abi_version == WINDOW_SESSION_TEARDOWN_ABI_VERSION
        && notice.notice_state == WINDOW_SESSION_TEARDOWN_NOTICE_STATE_READY
        && notice.operation == WINDOW_SESSION_TEARDOWN_OPERATION_RELEASE
        && notice.reason == WINDOW_SESSION_TEARDOWN_REASON_NONE
        && notice.reserved0 == 0
        && notice.reserved1 == [0; 4]
        && notice.notice_flags == WINDOW_SESSION_TEARDOWN_NOTICE_FLAGS_CANONICAL
        && notice.authority_flags == WINDOW_SESSION_TEARDOWN_AUTHORITY_CANONICAL
        && notice.notice_id != 0
        && notice.window_id != 0
        && notice.window_generation != 0
        && notice.owner_app_id != 0
        && notice.owner_process_id != 0
        && notice.owner_process_id <= u32::MAX as u64
        && notice.app_instance_id != 0
        && notice.app_instance_generation != 0
        && notice.surface_id != 0
        && notice.surface_generation != 0
        && notice.binding_generation != 0
        && notice.binding_hash != 0
        && notice.session_key != 0
        && notice.compositor_generation != 0
        && notice.compositor_record_hash != 0
        && notice.transition_receipt_hash != 0
        && notice.provenance_hash == compute_window_session_teardown_notice_provenance_hash(notice)
        && notice.notice_hash == compute_window_session_teardown_notice_hash(notice)
}

pub fn window_session_teardown_receipt_canonical(receipt: &WindowSessionTeardownReceipt) -> bool {
    receipt.abi_version == WINDOW_SESSION_TEARDOWN_ABI_VERSION
        && receipt.operation == WINDOW_SESSION_TEARDOWN_OPERATION_RELEASE
        && receipt.receipt_flags == WINDOW_SESSION_TEARDOWN_RECEIPT_FLAGS_CANONICAL
        && receipt.authority_flags == WINDOW_SESSION_TEARDOWN_AUTHORITY_CANONICAL
        && receipt.notice_id != 0
        && receipt.notice_hash != 0
        && receipt.window_id != 0
        && receipt.owner_app_id != 0
        && receipt.owner_process_id != 0
        && receipt.owner_process_id <= u32::MAX as u64
        && receipt.session_key != 0
        && receipt.reserved == [0; 8]
        && receipt.provenance_hash
            == compute_window_session_teardown_receipt_provenance_hash(receipt)
        && receipt.receipt_hash == compute_window_session_teardown_receipt_hash(receipt)
        && match receipt.decision {
            WINDOW_SESSION_TEARDOWN_DECISION_RELEASED => {
                receipt.status == WINDOW_SESSION_TEARDOWN_STATUS_OK
                    && receipt.reason == WINDOW_SESSION_TEARDOWN_REASON_NONE
                    && receipt.released_mapping_id != 0
                    && receipt.released_mapping_generation != 0
                    && receipt.released_mapping_hash != 0
            }
            WINDOW_SESSION_TEARDOWN_DECISION_REJECTED => {
                receipt.status != WINDOW_SESSION_TEARDOWN_STATUS_OK
                    && receipt.reason != WINDOW_SESSION_TEARDOWN_REASON_NONE
                    // Nothing was released, so no mapping is named.
                    && receipt.released_mapping_id == 0
                    && receipt.released_mapping_generation == 0
                    && receipt.released_mapping_hash == 0
            }
            _ => false,
        }
}

fn domain_hash(domain: &[u8]) -> u64 {
    fold(AUDIT_JOURNAL_FNV1A_OFFSET, domain)
}

fn fold(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(AUDIT_JOURNAL_FNV1A_PRIME);
    }
    hash
}

fn fold_u8(hash: u64, value: u8) -> u64 {
    fold(hash, &[value])
}

fn fold_u32(hash: u64, value: u32) -> u64 {
    fold(hash, &value.to_le_bytes())
}

fn fold_u64(hash: u64, value: u64) -> u64 {
    fold(hash, &value.to_le_bytes())
}

fn nonzero(hash: u64) -> u64 {
    if hash == 0 {
        AUDIT_JOURNAL_FNV1A_PRIME
    } else {
        hash
    }
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
    let mut value = [0u8; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}
