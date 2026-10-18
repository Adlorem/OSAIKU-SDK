//! Existing renderer-sequence receipt contract. Counts are ABI v10 constants,
//! not a public API for choosing a renderer or drawing tiles from an application.
//! Canonical form binds metadata; kernel sequence/owner authentication is separate.
use super::{
    domain_hash, fold_bytes, fold_u16, fold_u32, fold_u64, fold_u8, get_u16, get_u32, get_u64,
    nonzero_hash, put_u16, put_u32, put_u64, UI_DOCUMENT_WINDOW_ABI_VERSION,
    UI_DOCUMENT_WINDOW_MAGIC,
};

pub const UI_DOCUMENT_WINDOW_CELL_COUNT: usize = 52;
pub const UI_DOCUMENT_WINDOW_TILE_COUNT: usize = 26;
pub const UI_DOCUMENT_WINDOW_PRESENTATION_RECEIPT_SIZE: usize = 160;

pub const UI_DOCUMENT_WINDOW_RECEIPT_STATUS_OK: u8 = 0;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATUS_INVALID_REQUEST: u8 = 1;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATUS_NO_PENDING: u8 = 2;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATUS_STALE_REQUEST: u8 = 3;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATUS_KERNEL_REJECTED: u8 = 4;

pub const UI_DOCUMENT_WINDOW_RECEIPT_STATE_EMPTY: u8 = 0;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATE_PREPARED: u8 = 1;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATE_COMMITTED: u8 = 2;
pub const UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED: u8 = 3;

pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_NONE: u8 = 0;
pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_INVALID_REQUEST: u8 = 1;
pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_NO_PENDING: u8 = 2;
pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_STALE_PENDING: u8 = 3;
pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_KERNEL_REJECTED: u8 = 4;
pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_MANIFEST_MISMATCH: u8 = 5;
pub const UI_DOCUMENT_WINDOW_RECEIPT_REASON_SEQUENCE_MISMATCH: u8 = 6;

pub const UI_DOCUMENT_WINDOW_RECEIPT_METADATA_ONLY: u16 = 1 << 0;
pub const UI_DOCUMENT_WINDOW_RECEIPT_NO_BODY: u16 = 1 << 1;
pub const UI_DOCUMENT_WINDOW_RECEIPT_NO_TITLE_PREVIEW: u16 = 1 << 2;
pub const UI_DOCUMENT_WINDOW_RECEIPT_NO_PIXELS: u16 = 1 << 3;
pub const UI_DOCUMENT_WINDOW_RECEIPT_ONE_SHOT_REQUEST: u16 = 1 << 4;
pub const UI_DOCUMENT_WINDOW_RECEIPT_KERNEL_BOUND: u16 = 1 << 5;
pub const UI_DOCUMENT_WINDOW_RECEIPT_PROVENANCE_BOUND: u16 = 1 << 6;
pub const UI_DOCUMENT_WINDOW_RECEIPT_RENDERER_BROKER_SCOPED: u16 = 1 << 7;
pub const UI_DOCUMENT_WINDOW_RECEIPT_FLAGS_CANONICAL: u16 = UI_DOCUMENT_WINDOW_RECEIPT_METADATA_ONLY
    | UI_DOCUMENT_WINDOW_RECEIPT_NO_BODY
    | UI_DOCUMENT_WINDOW_RECEIPT_NO_TITLE_PREVIEW
    | UI_DOCUMENT_WINDOW_RECEIPT_NO_PIXELS
    | UI_DOCUMENT_WINDOW_RECEIPT_ONE_SHOT_REQUEST
    | UI_DOCUMENT_WINDOW_RECEIPT_KERNEL_BOUND
    | UI_DOCUMENT_WINDOW_RECEIPT_PROVENANCE_BOUND
    | UI_DOCUMENT_WINDOW_RECEIPT_RENDERER_BROKER_SCOPED;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiDocumentWindowPresentationReceipt {
    pub magic: u32,
    pub abi_version: u32,
    pub status: u8,
    pub presentation_state: u8,
    pub reason: u8,
    pub commit_count: u8,
    pub receipt_flags: u16,
    pub reserved: [u8; 2],
    pub source_request_hash: u64,
    pub source_frame_hash: u64,
    pub source_editor_state_hash: u64,
    pub source_body_hash: u64,
    pub app_instance_id: u64,
    pub window_id: u64,
    pub surface_id: u64,
    pub document_id: u64,
    pub document_revision: u64,
    pub manifest_hash: u64,
    pub sequence_id: u64,
    pub presentation_start_commit_hash: u64,
    pub presentation_final_commit_hash: u64,
    pub first_visual_generation: u64,
    pub final_visual_generation: u64,
    pub kernel_sequence_status_hash: u64,
    pub provenance_hash: u64,
    pub record_hash: u64,
}

impl UiDocumentWindowPresentationReceipt {
    pub const EMPTY: Self = Self {
        magic: 0,
        abi_version: 0,
        status: UI_DOCUMENT_WINDOW_RECEIPT_STATUS_INVALID_REQUEST,
        presentation_state: UI_DOCUMENT_WINDOW_RECEIPT_STATE_EMPTY,
        reason: UI_DOCUMENT_WINDOW_RECEIPT_REASON_NONE,
        commit_count: 0,
        receipt_flags: 0,
        reserved: [0; 2],
        source_request_hash: 0,
        source_frame_hash: 0,
        source_editor_state_hash: 0,
        source_body_hash: 0,
        app_instance_id: 0,
        window_id: 0,
        surface_id: 0,
        document_id: 0,
        document_revision: 0,
        manifest_hash: 0,
        sequence_id: 0,
        presentation_start_commit_hash: 0,
        presentation_final_commit_hash: 0,
        first_visual_generation: 0,
        final_visual_generation: 0,
        kernel_sequence_status_hash: 0,
        provenance_hash: 0,
        record_hash: 0,
    };

    pub fn encode(&self) -> [u8; UI_DOCUMENT_WINDOW_PRESENTATION_RECEIPT_SIZE] {
        let mut out = [0u8; UI_DOCUMENT_WINDOW_PRESENTATION_RECEIPT_SIZE];
        put_u32(&mut out, 0, self.magic);
        put_u32(&mut out, 4, self.abi_version);
        out[8] = self.status;
        out[9] = self.presentation_state;
        out[10] = self.reason;
        out[11] = self.commit_count;
        put_u16(&mut out, 12, self.receipt_flags);
        out[14..16].copy_from_slice(&self.reserved);
        let fields = [
            self.source_request_hash,
            self.source_frame_hash,
            self.source_editor_state_hash,
            self.source_body_hash,
            self.app_instance_id,
            self.window_id,
            self.surface_id,
            self.document_id,
            self.document_revision,
            self.manifest_hash,
            self.sequence_id,
            self.presentation_start_commit_hash,
            self.presentation_final_commit_hash,
            self.first_visual_generation,
            self.final_visual_generation,
            self.kernel_sequence_status_hash,
            self.provenance_hash,
            self.record_hash,
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put_u64(&mut out, 16 + index * 8, value);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != UI_DOCUMENT_WINDOW_PRESENTATION_RECEIPT_SIZE {
            return None;
        }
        let mut reserved = [0u8; 2];
        reserved.copy_from_slice(&bytes[14..16]);
        Some(Self {
            magic: get_u32(bytes, 0),
            abi_version: get_u32(bytes, 4),
            status: bytes[8],
            presentation_state: bytes[9],
            reason: bytes[10],
            commit_count: bytes[11],
            receipt_flags: get_u16(bytes, 12),
            reserved,
            source_request_hash: get_u64(bytes, 16),
            source_frame_hash: get_u64(bytes, 24),
            source_editor_state_hash: get_u64(bytes, 32),
            source_body_hash: get_u64(bytes, 40),
            app_instance_id: get_u64(bytes, 48),
            window_id: get_u64(bytes, 56),
            surface_id: get_u64(bytes, 64),
            document_id: get_u64(bytes, 72),
            document_revision: get_u64(bytes, 80),
            manifest_hash: get_u64(bytes, 88),
            sequence_id: get_u64(bytes, 96),
            presentation_start_commit_hash: get_u64(bytes, 104),
            presentation_final_commit_hash: get_u64(bytes, 112),
            first_visual_generation: get_u64(bytes, 120),
            final_visual_generation: get_u64(bytes, 128),
            kernel_sequence_status_hash: get_u64(bytes, 136),
            provenance_hash: get_u64(bytes, 144),
            record_hash: get_u64(bytes, 152),
        })
    }
}

pub fn compute_ui_document_window_presentation_provenance_hash(
    receipt: &UiDocumentWindowPresentationReceipt,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.ui-document-window.presentation-provenance.v2");
    hash = fold_u8(hash, receipt.commit_count);
    hash = fold_u64(hash, receipt.source_request_hash);
    hash = fold_u64(hash, receipt.source_frame_hash);
    hash = fold_u64(hash, receipt.source_editor_state_hash);
    hash = fold_u64(hash, receipt.source_body_hash);
    hash = fold_u64(hash, receipt.app_instance_id);
    hash = fold_u64(hash, receipt.window_id);
    hash = fold_u64(hash, receipt.surface_id);
    hash = fold_u64(hash, receipt.document_id);
    hash = fold_u64(hash, receipt.document_revision);
    hash = fold_u64(hash, receipt.manifest_hash);
    hash = fold_u64(hash, receipt.sequence_id);
    hash = fold_u64(hash, receipt.presentation_start_commit_hash);
    hash = fold_u64(hash, receipt.presentation_final_commit_hash);
    hash = fold_u64(hash, receipt.first_visual_generation);
    hash = fold_u64(hash, receipt.final_visual_generation);
    nonzero_hash(fold_u64(hash, receipt.kernel_sequence_status_hash))
}

pub fn compute_ui_document_window_presentation_receipt_hash(
    receipt: &UiDocumentWindowPresentationReceipt,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.ui-document-window.presentation-receipt.v2");
    hash = fold_u32(hash, receipt.magic);
    hash = fold_u32(hash, receipt.abi_version);
    hash = fold_u8(hash, receipt.status);
    hash = fold_u8(hash, receipt.presentation_state);
    hash = fold_u8(hash, receipt.reason);
    hash = fold_u8(hash, receipt.commit_count);
    hash = fold_u16(hash, receipt.receipt_flags);
    hash = fold_bytes(hash, &receipt.reserved);
    hash = fold_u64(hash, receipt.source_request_hash);
    hash = fold_u64(hash, receipt.source_frame_hash);
    hash = fold_u64(hash, receipt.source_editor_state_hash);
    hash = fold_u64(hash, receipt.source_body_hash);
    hash = fold_u64(hash, receipt.app_instance_id);
    hash = fold_u64(hash, receipt.window_id);
    hash = fold_u64(hash, receipt.surface_id);
    hash = fold_u64(hash, receipt.document_id);
    hash = fold_u64(hash, receipt.document_revision);
    hash = fold_u64(hash, receipt.manifest_hash);
    hash = fold_u64(hash, receipt.sequence_id);
    hash = fold_u64(hash, receipt.presentation_start_commit_hash);
    hash = fold_u64(hash, receipt.presentation_final_commit_hash);
    hash = fold_u64(hash, receipt.first_visual_generation);
    hash = fold_u64(hash, receipt.final_visual_generation);
    hash = fold_u64(hash, receipt.kernel_sequence_status_hash);
    nonzero_hash(fold_u64(hash, receipt.provenance_hash))
}

pub fn ui_document_window_presentation_receipt_canonical(
    receipt: &UiDocumentWindowPresentationReceipt,
) -> bool {
    if receipt.magic != UI_DOCUMENT_WINDOW_MAGIC
        || receipt.abi_version != UI_DOCUMENT_WINDOW_ABI_VERSION
        || receipt.receipt_flags != UI_DOCUMENT_WINDOW_RECEIPT_FLAGS_CANONICAL
        || receipt.reserved != [0; 2]
        || !receipt_status_reason_canonical(
            receipt.status,
            receipt.presentation_state,
            receipt.reason,
        )
        || receipt.provenance_hash
            != compute_ui_document_window_presentation_provenance_hash(receipt)
        || receipt.record_hash == 0
        || receipt.record_hash != compute_ui_document_window_presentation_receipt_hash(receipt)
    {
        return false;
    }

    match receipt.presentation_state {
        UI_DOCUMENT_WINDOW_RECEIPT_STATE_PREPARED => {
            receipt.commit_count == 0
                && receipt.source_request_hash != 0
                && receipt_fields_after_request_zero(receipt)
        }
        UI_DOCUMENT_WINDOW_RECEIPT_STATE_COMMITTED => {
            matches!(
                receipt.commit_count as usize,
                UI_DOCUMENT_WINDOW_TILE_COUNT | UI_DOCUMENT_WINDOW_CELL_COUNT
            ) && receipt.source_request_hash != 0
                && receipt.source_frame_hash != 0
                && receipt.source_editor_state_hash != 0
                && receipt.source_body_hash != 0
                && receipt.app_instance_id != 0
                && receipt.window_id != 0
                && receipt.surface_id != 0
                && receipt.document_id != 0
                && receipt.document_revision != 0
                && receipt.manifest_hash != 0
                && receipt.sequence_id != 0
                && receipt.presentation_final_commit_hash != 0
                && receipt.first_visual_generation != 0
                && receipt
                    .first_visual_generation
                    .checked_add(receipt.commit_count as u64 - 1)
                    .is_some_and(|last| receipt.final_visual_generation == last)
                && receipt.kernel_sequence_status_hash != 0
        }
        UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED => {
            receipt.commit_count == 0
                && receipt.manifest_hash == 0
                && receipt.sequence_id == 0
                && receipt.presentation_start_commit_hash == 0
                && receipt.presentation_final_commit_hash == 0
                && receipt.first_visual_generation == 0
                && receipt.final_visual_generation == 0
        }
        _ => false,
    }
}

fn receipt_status_reason_canonical(status: u8, state: u8, reason: u8) -> bool {
    match status {
        UI_DOCUMENT_WINDOW_RECEIPT_STATUS_OK => {
            matches!(
                state,
                UI_DOCUMENT_WINDOW_RECEIPT_STATE_PREPARED
                    | UI_DOCUMENT_WINDOW_RECEIPT_STATE_COMMITTED
            ) && reason == UI_DOCUMENT_WINDOW_RECEIPT_REASON_NONE
        }
        UI_DOCUMENT_WINDOW_RECEIPT_STATUS_INVALID_REQUEST => {
            state == UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED
                && matches!(
                    reason,
                    UI_DOCUMENT_WINDOW_RECEIPT_REASON_INVALID_REQUEST
                        | UI_DOCUMENT_WINDOW_RECEIPT_REASON_MANIFEST_MISMATCH
                        | UI_DOCUMENT_WINDOW_RECEIPT_REASON_SEQUENCE_MISMATCH
                )
        }
        UI_DOCUMENT_WINDOW_RECEIPT_STATUS_NO_PENDING => {
            state == UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED
                && reason == UI_DOCUMENT_WINDOW_RECEIPT_REASON_NO_PENDING
        }
        UI_DOCUMENT_WINDOW_RECEIPT_STATUS_STALE_REQUEST => {
            state == UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED
                && reason == UI_DOCUMENT_WINDOW_RECEIPT_REASON_STALE_PENDING
        }
        UI_DOCUMENT_WINDOW_RECEIPT_STATUS_KERNEL_REJECTED => {
            state == UI_DOCUMENT_WINDOW_RECEIPT_STATE_REJECTED
                && reason == UI_DOCUMENT_WINDOW_RECEIPT_REASON_KERNEL_REJECTED
        }
        _ => false,
    }
}

fn receipt_fields_after_request_zero(receipt: &UiDocumentWindowPresentationReceipt) -> bool {
    receipt.source_frame_hash == 0
        && receipt.source_editor_state_hash == 0
        && receipt.source_body_hash == 0
        && receipt.app_instance_id == 0
        && receipt.window_id == 0
        && receipt.surface_id == 0
        && receipt.document_id == 0
        && receipt.document_revision == 0
        && receipt.manifest_hash == 0
        && receipt.sequence_id == 0
        && receipt.presentation_start_commit_hash == 0
        && receipt.presentation_final_commit_hash == 0
        && receipt.first_visual_generation == 0
        && receipt.final_visual_generation == 0
        && receipt.kernel_sequence_status_hash == 0
}
