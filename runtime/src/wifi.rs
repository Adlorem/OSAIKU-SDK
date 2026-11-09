//! Authorized Wi-Fi control. Connection is independent of an application's
//! IPv4 grant; successful association does not grant Internet access.
pub use osaiku_wifi_protocol::*;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(request: &Message) -> Result<Message, i64> {
    use zeroize::Zeroize;
    let mut bytes = request.encode();
    let code =
        unsafe { crate::raw::syscall(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]) }
            as i64;
    let result = if code == 0 {
        Message::decode(&bytes)
    } else {
        Err(code)
    };
    bytes.zeroize();
    result
}
