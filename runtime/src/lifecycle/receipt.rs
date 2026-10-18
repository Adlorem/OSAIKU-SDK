use super::*;
use crate::session::{
    process_session_memory_receipt_canonical, ProcessSessionMemoryReceipt,
    PROCESS_SESSION_MEMORY_DECISION_MAPPED, PROCESS_SESSION_MEMORY_STATUS_OK,
};

impl WindowSessionTeardownReceipt {
    /// Build the diagnostic response after successfully releasing a session.
    ///
    /// `mapping` is the retained kernel-issued MAP receipt, copied out before
    /// unmapping. Its owner and session must match the canonical notice. This
    /// function checks metadata only: it neither releases memory nor proves
    /// release, sender authority or freshness. The host must still confirm the
    /// kernel registry. Call only after successful RELEASE and end all accesses
    /// to the old page before that syscall.
    pub fn released(
        notice: &WindowSessionTeardownNotice,
        mapping: &ProcessSessionMemoryReceipt,
        remaining_session_count: u64,
    ) -> Option<Self> {
        if !window_session_teardown_notice_canonical(notice)
            || !process_session_memory_receipt_canonical(mapping)
            || mapping.status != PROCESS_SESSION_MEMORY_STATUS_OK
            || mapping.decision != PROCESS_SESSION_MEMORY_DECISION_MAPPED
            || mapping.owner_process_id != notice.owner_process_id
            || mapping.session_key != notice.session_key
        {
            return None;
        }
        seal(Self {
            abi_version: WINDOW_SESSION_TEARDOWN_ABI_VERSION,
            status: WINDOW_SESSION_TEARDOWN_STATUS_OK,
            decision: WINDOW_SESSION_TEARDOWN_DECISION_RELEASED,
            reason: WINDOW_SESSION_TEARDOWN_REASON_NONE,
            operation: WINDOW_SESSION_TEARDOWN_OPERATION_RELEASE,
            receipt_flags: WINDOW_SESSION_TEARDOWN_RECEIPT_FLAGS_CANONICAL,
            authority_flags: WINDOW_SESSION_TEARDOWN_AUTHORITY_CANONICAL,
            notice_id: notice.notice_id,
            notice_hash: notice.notice_hash,
            window_id: notice.window_id,
            owner_app_id: notice.owner_app_id,
            owner_process_id: notice.owner_process_id,
            session_key: notice.session_key,
            released_mapping_id: mapping.mapping_id,
            released_mapping_generation: mapping.mapping_generation,
            released_mapping_hash: mapping.mapping_hash,
            remaining_session_count,
            ..Self::EMPTY
        })
    }

    /// Build a refusal without claiming any released mapping.
    ///
    /// A missing or malformed notice is allowed so the caller can report a
    /// decode/validation failure. Zero echo fields become the existing v2
    /// placeholder 1; all nonzero fields are preserved, never truncated. If
    /// these fields or the supplied status/reason cannot form a canonical
    /// receipt, return None. This does not update the answered-notice cursor
    /// or send a reply. Match responses against the original authenticated
    /// transaction; placeholders and hashes grant no authority.
    pub fn rejected(
        notice: Option<&WindowSessionTeardownNotice>,
        status: u8,
        reason: u8,
        remaining_session_count: u64,
    ) -> Option<Self> {
        seal(Self {
            abi_version: WINDOW_SESSION_TEARDOWN_ABI_VERSION,
            status,
            decision: WINDOW_SESSION_TEARDOWN_DECISION_REJECTED,
            reason,
            operation: WINDOW_SESSION_TEARDOWN_OPERATION_RELEASE,
            receipt_flags: WINDOW_SESSION_TEARDOWN_RECEIPT_FLAGS_CANONICAL,
            authority_flags: WINDOW_SESSION_TEARDOWN_AUTHORITY_CANONICAL,
            notice_id: nonzero_echo(notice.map_or(0, |notice| notice.notice_id)),
            notice_hash: nonzero_echo(notice.map_or(0, |notice| notice.notice_hash)),
            window_id: nonzero_echo(notice.map_or(0, |notice| notice.window_id)),
            owner_app_id: nonzero_echo(notice.map_or(0, |notice| notice.owner_app_id)),
            owner_process_id: nonzero_echo(notice.map_or(0, |notice| notice.owner_process_id)),
            session_key: nonzero_echo(notice.map_or(0, |notice| notice.session_key)),
            remaining_session_count,
            ..Self::EMPTY
        })
    }
}

fn nonzero_echo(value: u64) -> u64 {
    if value == 0 {
        1
    } else {
        value
    }
}

fn seal(mut receipt: WindowSessionTeardownReceipt) -> Option<WindowSessionTeardownReceipt> {
    receipt.provenance_hash = compute_window_session_teardown_receipt_provenance_hash(&receipt);
    receipt.receipt_hash = compute_window_session_teardown_receipt_hash(&receipt);
    window_session_teardown_receipt_canonical(&receipt).then_some(receipt)
}
