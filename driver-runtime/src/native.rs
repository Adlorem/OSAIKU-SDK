//! Native copy transport. Only compiled for the freestanding x86_64 target.
use crate::process::{Message, BYTES, SYSCALL};
use core::arch::asm;
fn invoke(number: u64, args: [u64; 5]) -> isize {
    let result: u64;
    // All pointers are private, live borrows owned by the wrappers below.
    // No nomem flag: a trap copies data and may reschedule before returning.
    unsafe {
        asm!("int 0x80",inlateout("rax") number=>result,
        in("rdi") args[0],in("rsi") args[1],in("rdx") args[2],in("r8") args[3],in("r9") args[4],clobber_abi("C"));
    }
    result as isize
}
pub fn identity() -> Result<Message, isize> {
    let mut bytes = [0; BYTES];
    let code = invoke(SYSCALL, [2, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]);
    if code != 0 {
        return Err(code);
    }
    Message::decode(&bytes).map_err(|_| -5)
}
pub fn exchange(message: &Message) -> Result<Message, isize> {
    let mut bytes = message.encode();
    let code = invoke(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]);
    if code != 0 {
        return Err(code);
    }
    Message::decode(&bytes).map_err(|_| -5)
}
/// Event-driven wait with a bounded deadline; unsolicited messages confer no authority.
pub fn wait() {
    let mut bytes = [0u8; 16];
    let mut endpoint = [0u32; 2];
    let _ = invoke(
        359,
        [
            bytes.as_mut_ptr() as u64,
            16,
            endpoint.as_mut_ptr() as u64,
            100_000,
            0,
        ],
    );
}
pub fn log(text: &str) -> Result<(), isize> {
    if text.len() > 160 {
        return Err(-1);
    }
    let code = invoke(0, [text.as_ptr() as u64, text.len() as u64, 0, 0, 0]);
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}
pub fn exit(code: u64) -> ! {
    unsafe {
        asm!("int 0x80",in("rax") 1u64,in("rdi") code,clobber_abi("C"),options(noreturn));
    }
}
