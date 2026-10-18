//! Bounded application control-message routing, without allocation or syscalls.
//! The legacy wire strings remain stable; they apply to all built-in apps.
use crate::{
    abi::MAX_IPC_MESSAGE_LEN,
    lifecycle::WINDOW_SESSION_TEARDOWN_NOTICE_SIZE,
    process::{ProcessEndpoint, ProcessId},
    window::{
        UI_DOCUMENT_WINDOW_ABI_VERSION, UI_DOCUMENT_WINDOW_MAGIC,
        UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE,
    },
};

pub const APPLICATION_COMMAND_REPORT: &[u8] = b"document_app|command|report\n";
pub const APPLICATION_COMMAND_EXIT: &[u8] = b"document_app|command|exit\n";
pub const APPLICATION_REPLY_RUNNING: &[u8] = b"document_app|reply|running\n";
pub const APPLICATION_REPLY_INVALID_COMMAND: &[u8] = b"document_app|reply|invalid_command\n";

/// Trusted local policy for the lifecycle host, never learned from payload bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationHost {
    /// Only for a system service whose PID cannot be reused (currently initd
    /// in the built-in adapters). Selecting this policy does not establish that
    /// a PID is fixed; the caller must know that from its trusted boot contract.
    FixedService(ProcessId),
    /// For a reusable process slot, match the full nonzero-generation endpoint.
    Instance(ProcessEndpoint),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationMessageError {
    InvalidLength,
    InvalidSender,
}

/// Borrowed routing result, not canonical/authorized window or session metadata.
/// Decode and validate Present/Teardown against trusted local state before acting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationMessage<'a> {
    Report,
    Exit,
    Present(&'a [u8]),
    Teardown(&'a [u8]),
    /// App-specific messages and unsupported presentation versions are left
    /// untouched for the adapter's existing decoder or invalid-command reply.
    Other(&'a [u8]),
}

impl ApplicationHost {
    /// Route one received message after checking its length and sender policy.
    ///
    /// `sender` must be the endpoint returned by kernel IPC, not supplied by the
    /// message. Empty/oversized messages are refused before sender checks, as in
    /// the existing adapters. Kernel PID zero and unbound Instance policies are
    /// never accepted. FixedService preserves the service PID-only policy.
    /// There is no receive, reply, retry, session mutation or scheduling here.
    pub fn classify<'a>(
        self,
        sender: ProcessEndpoint,
        bytes: &'a [u8],
    ) -> Result<ApplicationMessage<'a>, ApplicationMessageError> {
        if bytes.is_empty() || bytes.len() > MAX_IPC_MESSAGE_LEN {
            return Err(ApplicationMessageError::InvalidLength);
        }
        let authorized = match self {
            Self::FixedService(pid) => pid != ProcessId::KERNEL && sender.process_id == pid,
            Self::Instance(endpoint) => {
                endpoint.process_id != ProcessId::KERNEL
                    && endpoint.names_instance()
                    && endpoint == sender
            }
        };
        if !authorized {
            return Err(ApplicationMessageError::InvalidSender);
        }
        Ok(if bytes == APPLICATION_COMMAND_REPORT {
            ApplicationMessage::Report
        } else if bytes == APPLICATION_COMMAND_EXIT {
            ApplicationMessage::Exit
        } else if bytes.len() == UI_DOCUMENT_WINDOW_PRESENT_REQUEST_SIZE
            && bytes[..4] == UI_DOCUMENT_WINDOW_MAGIC.to_le_bytes()
            && bytes[4..8] == UI_DOCUMENT_WINDOW_ABI_VERSION.to_le_bytes()
        {
            ApplicationMessage::Present(bytes)
        } else if bytes.len() == WINDOW_SESSION_TEARDOWN_NOTICE_SIZE {
            ApplicationMessage::Teardown(bytes)
        } else {
            ApplicationMessage::Other(bytes)
        })
    }
}
