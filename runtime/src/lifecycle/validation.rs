use super::*;
use crate::window::compute_window_document_id;

/// Why a notice must be refused before looking up or mutating a local session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TeardownNoticeError {
    InvalidShape,
    ReplayedNotice,
    IdentityMismatch,
}

impl TeardownNoticeError {
    pub const fn status(self) -> u8 {
        match self {
            Self::InvalidShape | Self::IdentityMismatch => {
                WINDOW_SESSION_TEARDOWN_STATUS_INVALID_NOTICE
            }
            Self::ReplayedNotice => WINDOW_SESSION_TEARDOWN_STATUS_STALE,
        }
    }

    pub const fn reason(self) -> u8 {
        match self {
            Self::InvalidShape => WINDOW_SESSION_TEARDOWN_REASON_INVALID_SHAPE,
            Self::ReplayedNotice => WINDOW_SESSION_TEARDOWN_REASON_REPLAYED_NOTICE,
            Self::IdentityMismatch => WINDOW_SESSION_TEARDOWN_REASON_IDENTITY_MISMATCH,
        }
    }

    /// A canonical, fresh but misaddressed notice consumes its ID when refused.
    /// Malformed or replayed notices must not advance or rewind the cursor.
    /// Only apply this after authenticating the lifecycle host at the IPC boundary.
    pub const fn consumes_notice_id(self) -> bool {
        matches!(self, Self::IdentityMismatch)
    }
}

/// Validate a notice against trusted local owner and answered-notice state.
///
/// Checks run in protocol order: canonical shape, replay, owner and derived key.
/// The function has no side effects, allocates nothing, and borrows the input.
/// `None` represents a failed decode. The owner PID is numeric, not an encoded
/// endpoint; obtain it from the process's kernel-issued self endpoint.
///
/// **Authenticate the IPC sender first.** This function does not authenticate
/// unkeyed hashes, establish that a window was destroyed, or authorize unmapping.
/// On success, locate the exact local session using its stored identity and
/// kernel-issued mapping. Unknown sessions still require refusal. Record the
/// notice ID once answered; on error, use `consumes_notice_id` to preserve the
/// protocol's replay policy. Local session mutation and RELEASE remain the
/// caller's responsibility.
pub fn validate_window_session_teardown_notice(
    notice: Option<&WindowSessionTeardownNotice>,
    owner_app_id: u64,
    owner_process_id: u64,
    last_answered_notice_id: u64,
) -> Result<&WindowSessionTeardownNotice, TeardownNoticeError> {
    let notice = notice
        .filter(|notice| window_session_teardown_notice_canonical(notice))
        .ok_or(TeardownNoticeError::InvalidShape)?;
    if notice.notice_id <= last_answered_notice_id {
        return Err(TeardownNoticeError::ReplayedNotice);
    }
    if notice.owner_app_id != owner_app_id
        || notice.owner_process_id != owner_process_id
        || compute_window_document_id(
            notice.app_instance_id,
            notice.app_instance_generation,
            notice.surface_id,
            notice.surface_generation,
            notice.binding_generation,
            notice.binding_hash,
        ) != notice.session_key
    {
        return Err(TeardownNoticeError::IdentityMismatch);
    }
    Ok(notice)
}
