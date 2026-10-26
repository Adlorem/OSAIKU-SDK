#![no_std]
#![no_main]
use core::panic::PanicInfo;
use osaiku_driver_runtime::{hid, native, process::op};
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut identity = native::identity().unwrap_or_else(|_| native::exit(1));
    if identity.endpoint != hid::Kind::Mouse as u64 {
        native::exit(2);
    }
    identity.value = 0;
    native::exchange(&identity).unwrap_or_else(|_| native::exit(3));
    identity.operation = op::READY;
    native::exchange(&identity).unwrap_or_else(|_| native::exit(4));

    loop {
        identity.operation = op::HEARTBEAT;
        native::exchange(&identity).unwrap_or_else(|_| native::exit(5));
        // Bound each drain so sustained input cannot starve the heartbeat.
        for _ in 0..32 {
            identity.operation = op::HID_READ;
            let report = native::exchange(&identity).unwrap_or_else(|_| native::exit(6));
            if report.sequence == 0 {
                break;
            }
            let mut reply = identity;
            reply.operation = op::HID_INPUT;
            reply.sequence = report.sequence;
            match hid::mouse(&report.payload[..report.length as usize]) {
                Some(p) => {
                    reply.length = 6;
                    reply.payload[..2].copy_from_slice(&p.dx.to_le_bytes());
                    reply.payload[2..4].copy_from_slice(&p.dy.to_le_bytes());
                    reply.payload[4] = p.buttons;
                }
                None => {
                    reply.length = 0;
                    reply.value = 1;
                }
            }
            native::exchange(&reply).unwrap_or_else(|_| native::exit(7));
        }
        native::wait();
    }
}
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    native::exit(8)
}
