//! Reserved replies: requests are admitted only after reserving independent
//! kernel storage. Tokens never wrap or identify a successor transaction.
pub const SYSCALL: u64 = 415;
pub const REQUEST: u64 = 0;
pub const REPLY: u64 = 1;
pub const TAKE: u64 = 2;
pub const CANCEL: u64 = 3;
pub const CURRENT: u64 = 4;
/// The reservation was cancelled, collected, or its peer died. Terminal;
/// never retry this token and never fall back to ordinary IPC.
pub const CLOSED: isize = -16;
pub const CAPACITY: usize = 16;

/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn request(peer: crate::process::ProcessEndpoint, bytes: &[u8]) -> isize {
    unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                REQUEST,
                peer.encode(),
                bytes.as_ptr() as u64,
                bytes.len() as u64,
                0,
            ],
        ) as isize
    }
}
/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn reply(peer: crate::process::ProcessEndpoint, token: u64, bytes: &[u8]) -> isize {
    unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                REPLY,
                peer.encode(),
                bytes.as_ptr() as u64,
                bytes.len() as u64,
                token,
            ],
        ) as isize
    }
}
/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn take(token: u64, bytes: &mut [u8]) -> isize {
    unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                TAKE,
                token,
                bytes.as_mut_ptr() as u64,
                bytes.len() as u64,
                0,
            ],
        ) as isize
    }
}
/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn cancel(token: u64) -> isize {
    unsafe { crate::raw::syscall(SYSCALL, [CANCEL, token, 0, 0, 0]) as isize }
}
/// Main application-thread adapter. Kernel sideband is replaced only when
/// that thread receives another message, never when a request is merely queued.
/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn current(peer: crate::process::ProcessEndpoint) -> isize {
    unsafe { crate::raw::syscall(SYSCALL, [CURRENT, peer.encode(), 0, 0, 0]) as isize }
}

/// Adapter for existing sequential application loops. Explicit `reply` keeps
/// the exact terminal status, including CLOSED. The application may end that
/// cancelled exchange without exiting; this adapter never changes it to OK
/// or resends it onto the ordinary mailbox.
/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn send_application(peer: crate::process::ProcessEndpoint, bytes: &[u8]) -> isize {
    let token = if peer.process_id.0 == 1 {
        unsafe { current(peer) }
    } else {
        0
    };
    unsafe { send_with_token(peer, bytes, token, 3, token.max(0) as u64) }
}
/// # Safety
/// Call only in an OSAIKU user process implementing reserved-reply ABI 415.
#[cfg(target_arch = "x86_64")]
pub unsafe fn send_application_with_metadata(
    peer: crate::process::ProcessEndpoint,
    bytes: &[u8],
    kind: u64,
    id: u64,
) -> isize {
    let token = if peer.process_id.0 == 1 {
        unsafe { current(peer) }
    } else {
        0
    };
    unsafe { send_with_token(peer, bytes, token, kind, id) }
}
#[cfg(target_arch = "x86_64")]
unsafe fn send_with_token(
    peer: crate::process::ProcessEndpoint,
    bytes: &[u8],
    token: isize,
    kind: u64,
    id: u64,
) -> isize {
    use crate::abi::*;
    let status = if token > 0 {
        unsafe { reply(peer, token as u64, bytes) }
    } else if token == 0 {
        unsafe { crate::raw::ipc_send(peer, bytes) }
    } else {
        token
    };
    if status != SYSCALL_OK {
        let report = crate::ipc::SendReport {
            status,
            last_send: if token < 0 { 0 } else { status },
            stage: if token < 0 {
                "reply-context"
            } else if token > 0 {
                "reserved-reply"
            } else {
                "send"
            },
            attempts: 1,
            elapsed_ms: None,
        };
        for part in 0..2 {
            let mut line = osaiku_ui::text_model::Text::EMPTY;
            if report
                .diagnostic(&mut line, part, peer.encode(), kind, id)
                .is_ok()
            {
                unsafe {
                    crate::raw::syscall(
                        number::LOG_WRITE,
                        [
                            line.as_str().as_ptr() as u64,
                            line.as_str().len() as u64,
                            0,
                            0,
                            0,
                        ],
                    );
                }
            }
        }
    }
    status
}
