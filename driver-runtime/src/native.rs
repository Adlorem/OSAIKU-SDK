//! Native control, shared CPU pages and sequenced process events.
//! Only compiled for the freestanding x86_64 target.
use crate::process::{Message, BYTES, SYSCALL};
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};
static CALLS: AtomicU64 = AtomicU64::new(0);
/// Cumulative calls through this process runtime, including waits and logging.
pub fn syscall_count() -> u64 {
    CALLS.load(Ordering::Relaxed)
}
fn invoke(number: u64, args: [u64; 5]) -> isize {
    CALLS.fetch_add(1, Ordering::Relaxed);
    let result: u64;
    // All pointers are private, live borrows owned by the wrappers below.
    // No nomem flag: a trap copies data and may reschedule before returning.
    unsafe {
        asm!("int 0x80",inlateout("rax") number=>result,
        in("rdi") args[0],in("rsi") args[1],in("rdx") args[2],in("r8") args[3],in("r9") args[4],clobber_abi("C"));
    }
    result as isize
}
pub fn direct(
    message: &crate::protocol::direct::Message,
) -> Result<crate::protocol::direct::Message, isize> {
    use crate::protocol::direct::{Message, BYTES, SYSCALL};
    let mut bytes = message.encode();
    let code = invoke(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]);
    if code != 0 {
        return Err(code);
    }
    Message::decode(&bytes).map_err(|_| -5)
}
pub fn network_channel(
    w: crate::network_channel::Wire,
) -> Result<crate::network_channel::Wire, isize> {
    use crate::network_channel::{Wire, BYTES, SYSCALL};
    let mut bytes = w.encode();
    let code = invoke(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]);
    if code != 0 {
        return Err(code);
    }
    Wire::decode(&bytes).map_err(|_| -5)
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
    wait_for(100_000);
}
pub fn clock_millis() -> Result<u64, isize> {
    let code = invoke(380, [0; 5]);
    if code < 0 {
        Err(code)
    } else {
        Ok(code as u64)
    }
}
/// Microsecond deadline, bounded by the owner's heartbeat window.
pub fn wait_for(microseconds: u64) {
    let mut bytes = [0u8; 16];
    let mut endpoint = [0u32; 2];
    let _ = invoke(
        359,
        [
            bytes.as_mut_ptr() as u64,
            16,
            endpoint.as_mut_ptr() as u64,
            microseconds.clamp(1, 100_000),
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

pub fn shared(
    request: crate::protocol::shared::Request,
) -> Result<crate::protocol::shared::Request, isize> {
    use crate::protocol::shared::{Request, BYTES, SYSCALL};
    let mut bytes = request.encode();
    let code = invoke(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]);
    if code != 0 {
        return Err(code);
    }
    Request::decode(&bytes).map_err(|_| -5)
}

pub fn event(
    operation: u64,
    id: u64,
    generation: u64,
    last_seen: u64,
    timeout_ms: u64,
) -> Result<u64, isize> {
    let code = invoke(
        crate::protocol::event::SYSCALL,
        [operation, id, generation, last_seen, timeout_ms],
    );
    if code < 0 {
        Err(code)
    } else {
        Ok(code as u64)
    }
}
