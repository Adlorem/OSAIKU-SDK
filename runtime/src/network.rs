//! Copy-only, asynchronous IPv4 service ABI. Access requires a policy grant.
pub use osaiku_network_protocol::*;
/// Submit once, then POLL the returned id. BUSY means no request was accepted.
/// CANCEL revokes the connection grant: accepted bytes are never replayed.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(request: &Message) -> Result<Message, i64> {
    let mut bytes = request.encode();
    let code =
        unsafe { crate::raw::syscall(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]) }
            as i64;
    if code != 0 {
        return Err(code);
    }
    Message::decode(&bytes)
}
