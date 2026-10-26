#![no_std]
#![no_main]
//! Guest-only malicious/failure fixture; never linked into a production image.
use core::panic::PanicInfo;
use osaiku_driver_runtime::{
    native,
    process::{op, Message, SLOTS},
};
fn check(ok: bool) {
    if !ok {
        let _ = native::log("drv06.fixture: FAILED\n");
        native::exit(90);
    }
}
fn send(m: Message) -> Message {
    native::exchange(&m).unwrap_or_else(|_| {
        check(false);
        native::exit(91)
    })
}
fn request(identity: Message, operation: u64) -> Message {
    let mut m = Message::new(operation, identity.scope);
    m.release = identity.release;
    m.endpoint = identity.endpoint;
    m
}
fn log(s: &str) {
    check(native::log(s).is_ok());
}
#[no_mangle]
pub extern "C" fn _start() -> ! {
    #[cfg(feature = "oversize")]
    unsafe {
        core::ptr::write_volatile(
            core::hint::black_box(core::ptr::addr_of_mut!(OVERSIZE)).cast::<u8>(),
            1,
        );
    }

    let identity = native::identity().unwrap();
    if identity.value == 2 {
        driver(identity)
    }
    let mut m = request(identity, op::HELLO);
    send(m);
    m.operation = op::READY;
    send(m);
    m.operation = op::RECONCILE;
    send(m);
    m.operation = op::SNAPSHOT;
    let page = send(m);
    check(page.length >= 48);
    let mut start = request(identity, op::START);
    start.length = 16;
    start.payload[..16].copy_from_slice(&page.payload[..16]);
    for attempt in 0..3 {
        if attempt == 2 {
            for _ in 0..30 {
                send(request(identity, op::HEARTBEAT));
                native::wait();
            }
        }
        let child = send(start);
        check(child.value != 0);
        let mut finished = false;
        for _ in 0..100 {
            send(request(identity, op::HEARTBEAT));
            native::wait();
            let status = send(request(identity, op::STATUS));
            if status.payload[2] != 0 && status.payload[3] == attempt + 1 && status.payload[11] == 0
            {
                finished = true;
                break;
            }
        }
        check(finished);
    }
    check(native::exchange(&start).is_err());
    log("drv06.fixture: restart-limit and retained-failure PASS\n");
    check(page.length >= 96);
    start.payload[..16].copy_from_slice(&page.payload[48..64]);
    for attempt in 0..2 {
        send(start);
        let mut finished = false;
        for _ in 0..150 {
            send(request(identity, op::HEARTBEAT));
            native::wait();
            let status = send(request(identity, op::STATUS));
            if status.payload[4] != 0 && status.payload[5] == attempt + 1 && status.payload[12] == 0
            {
                finished = true;
                break;
            }
        }
        check(finished);
    }
    log("drv06.fixture: flood and start-timeout PASS\n");
    log("drv06.fixture: complete PASS\n");
    loop {
        send(request(identity, op::HEARTBEAT));
        native::wait();
    }
}
fn driver(identity: Message) -> ! {
    if identity.scope.instance.get() == 6 {
        log("drv06.fixture: start-timeout-loop\n");
        loop {
            core::hint::spin_loop();
        }
    }
    // Monotonic instance IDs: manager=1, driver attempts=2,3,4.
    if identity.scope.instance.get() == 2 {
        log("drv06.fixture: death-starting\n");
        native::exit(11);
    }
    send(request(identity, op::HELLO));
    if identity.scope.instance.get() == 3 {
        log("drv06.fixture: death-initializing\n");
        native::exit(12);
    }
    send(request(identity, op::READY));
    if identity.scope.instance.get() == 5 {
        log("drv06.fixture: flood-loop\n");
        loop {
            let _ = native::exchange(&request(identity, op::HEARTBEAT));
        }
    }
    let mut wrong = request(identity, op::SUBMIT);
    wrong.release += 1;
    check(native::exchange(&wrong).is_err());
    let mut wrong = request(identity, op::SUBMIT);
    wrong.endpoint += 1;
    check(native::exchange(&wrong).is_err());
    let mut wrong = request(identity, op::SUBMIT);
    wrong.scope = osaiku_driver_runtime::protocol::Scope::new(99, 99, 99).unwrap();
    check(native::exchange(&wrong).is_err());
    check(native::exchange(&request(identity, 99)).is_err());
    for number in [334, 347, 368, 370, 399] {
        check(raw(number, [0; 5]) == -8);
    }
    let mut malformed = request(identity, op::SUBMIT).encode();
    malformed[100] = 1;
    check(raw(400, [0, malformed.as_mut_ptr() as u64, 384, 0, 0]) < 0);
    let mut ids = [0; SLOTS];
    for id in &mut ids {
        *id = send(request(identity, op::SUBMIT)).sequence;
    }
    check(native::exchange(&request(identity, op::SUBMIT)).is_err());
    for (index, id) in ids.into_iter().enumerate() {
        let mut take = request(identity, if index % 2 == 0 { op::TAKE } else { op::CANCEL });
        take.sequence = id;
        let result = send(take);
        check(result.length == if index % 2 == 0 { 16 } else { 0 });
        check(native::exchange(&take).is_err());
    }
    let mut forged = request(identity, op::TAKE);
    forged.sequence = u64::MAX;
    check(native::exchange(&forged).is_err());
    log("drv06.fixture: copy-queue forged-completion role-scope boundaries PASS\n");
    log("drv06.fixture: watchdog-loop\n");
    loop {
        core::hint::spin_loop();
    }
}
fn raw(number: u64, args: [u64; 5]) -> isize {
    let out: u64;
    unsafe {
        core::arch::asm!("int 0x80",inlateout("rax") number=>out,in("rdi")args[0],in("rsi")args[1],in("rdx")args[2],in("r8")args[3],in("r9")args[4],clobber_abi("C"));
    }
    out as isize
}
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    let _ = native::log("drv06.fixture: PANIC\n");
    native::exit(99)
}

#[cfg(feature = "oversize")]
static mut OVERSIZE: [u8; 300 * 1024] = [0; 300 * 1024];
