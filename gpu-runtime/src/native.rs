//! Native GPU control transport. All bytes are copied by the syscall before
//! return, with no retained user pointer and no kernel-source dependency.
use crate::{
    process::Transport,
    protocol::control::{Message, BYTES, SYSCALL},
};
pub struct Native;
unsafe impl crate::upload::Transport for Native {
    fn exchange(
        &mut self,
        request: osaiku_driver_protocol::shared::Request,
    ) -> Result<osaiku_driver_protocol::shared::Request, isize> {
        use osaiku_driver_protocol::shared::{Request, BYTES, SYSCALL};
        let mut bytes = request.encode();
        let r = invoke(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0]);
        if r != 0 {
            return Err(r);
        }
        Request::decode(&bytes).map_err(|_| -5)
    }
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), isize> {
        // Upload validated the complete kernel mapping receipt. This native
        // transport is never mixed with another transport's address space.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        }
        Ok(())
    }
}
fn invoke(number: u64, args: [u64; 5]) -> isize {
    let result: u64;
    unsafe {
        core::arch::asm!("int 0x80", inlateout("rax") number => result,
        in("rdi") args[0], in("rsi") args[1], in("rdx") args[2],
        in("r8") args[3], in("r9") args[4], clobber_abi("C"));
    }
    result as isize
}
pub fn clock_millis() -> Result<u64, isize> {
    let n = invoke(380, [0; 5]);
    if n < 0 {
        Err(n)
    } else {
        Ok(n as u64)
    }
}
/// Wake hints have no authority. Every received event is followed by a fresh
/// bounded channel/RPC observation and a supervisor heartbeat.
pub fn wait_for(microseconds: u64) {
    let mut message = [0u8; 16];
    let mut sender = [0u32; 2];
    let _ = invoke(
        359,
        [
            message.as_mut_ptr() as u64,
            16,
            sender.as_mut_ptr() as u64,
            microseconds.clamp(1, 100_000),
            0,
        ],
    );
}
pub fn log(text: &str) -> Result<(), isize> {
    log_bytes(text.as_bytes())
}
pub fn log_bytes(text: &[u8]) -> Result<(), isize> {
    if text.len() > 160 {
        return Err(-1);
    }
    match invoke(0, [text.as_ptr() as u64, text.len() as u64, 0, 0, 0]) {
        0 => Ok(()),
        n => Err(n),
    }
}
pub fn exit(code: u64) -> ! {
    unsafe {
        core::arch::asm!("int 0x80", in("rax") 1u64, in("rdi") code,
        clobber_abi("C"), options(noreturn));
    }
}
impl crate::backend::Transport for Native {
    fn exchange(&mut self, packet: &mut [u8]) -> Result<(), isize> {
        use crate::protocol::backend::{HEADER, MAX_BYTES, SYSCALL};
        if !(HEADER..=MAX_BYTES).contains(&packet.len()) {
            return Err(-1);
        }
        let result: u64;
        unsafe {
            core::arch::asm!("int 0x80", inlateout("rax") SYSCALL => result,
            in("rdi") 0u64, in("rsi") packet.as_mut_ptr() as u64,
            in("rdx") packet.len() as u64, in("r8") 0u64, in("r9") 0u64, clobber_abi("C"));
        }
        if result == 0 {
            Ok(())
        } else {
            Err(result as isize)
        }
    }
}
impl crate::channel::Transport for Native {
    fn exchange(
        &mut self,
        request: crate::protocol::channel::Message,
        payload: &mut [u8],
    ) -> Result<crate::protocol::channel::Message, isize> {
        use crate::protocol::channel::{BUSY, BYTES, SYSCALL};
        use core::{
            cell::UnsafeCell,
            sync::atomic::{AtomicBool, Ordering},
        };
        struct Scratch(UnsafeCell<crate::channel_wire::Wire>);
        unsafe impl Sync for Scratch {}
        static WIRE: Scratch = Scratch(UnsafeCell::new(crate::channel_wire::Wire::EMPTY));
        static LOCK: AtomicBool = AtomicBool::new(false);
        if LOCK
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(BUSY);
        }
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                LOCK.store(false, Ordering::Release);
            }
        }
        let _guard = Guard;
        // Process-private ownership covers every trap/scheduling boundary.
        // Concurrent callers receive BUSY without touching the borrowed bytes.
        unsafe {
            (&mut *WIRE.0.get()).exchange(request, payload, |bytes| {
                invoke(SYSCALL, [0, bytes.as_mut_ptr() as u64, BYTES as u64, 0, 0])
            })
        }
    }
}
impl crate::platform::Transport for Native {
    fn exchange(
        &mut self,
        request: crate::protocol::platform::Message,
    ) -> Result<crate::protocol::platform::Message, isize> {
        use crate::protocol::platform::{Message, BYTES, SYSCALL};
        let mut bytes = request.encode();
        let result: u64;
        unsafe {
            core::arch::asm!("int 0x80", inlateout("rax") SYSCALL => result,
                in("rdi") 0u64, in("rsi") bytes.as_mut_ptr() as u64,
                in("rdx") BYTES as u64, in("r8") 0u64, in("r9") 0u64,
                clobber_abi("C"));
        }
        if result != 0 {
            return Err(result as isize);
        }
        Message::decode(&bytes).map_err(|_| -5)
    }
}
impl Transport for Native {
    fn exchange(&mut self, request: Message) -> Result<Message, isize> {
        let mut bytes = request.encode();
        let result: u64;
        unsafe {
            core::arch::asm!("int 0x80", inlateout("rax") SYSCALL => result,
                in("rdi") 0u64, in("rsi") bytes.as_mut_ptr() as u64,
                in("rdx") BYTES as u64, in("r8") 0u64, in("r9") 0u64,
                clobber_abi("C"));
        }
        if result != 0 {
            return Err(result as isize);
        }
        Message::decode(&bytes).map_err(|_| -5)
    }
}
