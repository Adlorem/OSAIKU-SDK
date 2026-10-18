#![no_std]
#![no_main]

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
compile_error!(
    "Build this example with targets/x86_64-osaiku-user.json; never run it on the host."
);

use osaiku_runtime::{abi::SYSCALL_OK, log_write, process_exit, process_self, scheduler_yield};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if process_self().is_err() {
        process_exit(41);
    }
    if log_write("OSAIKU public native SDK: hello") != SYSCALL_OK {
        process_exit(42);
    }
    if scheduler_yield() != SYSCALL_OK {
        process_exit(43);
    }
    process_exit(0)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    process_exit(30)
}
