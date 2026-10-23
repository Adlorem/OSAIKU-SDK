use crate::status_snapshot::Snapshot;
use crate::{abi::*, process::ProcessEndpoint, window_status::*};

static SNAPSHOT: Snapshot = Snapshot::new();
#[repr(C, align(4096))]
struct Stack([u8; 8192]);
// Only the kernel-created continuation uses this stack after registration.
static mut STACK: Stack = Stack([0; 8192]);

pub fn register() {
    let _ = unsafe {
        crate::raw::syscall(
            360,
            [
                entry as *const () as u64,
                core::ptr::addr_of_mut!(STACK) as u64,
                8192,
                0,
                0,
            ],
        )
    };
}

pub fn publish(identity: osaiku_ui::Identity, length: u64, save: u64) {
    SNAPSHOT.publish(
        identity.window,
        identity.incarnation,
        identity.revision,
        length,
        save,
    );
}
pub fn invalidate() {
    SNAPSHOT.publish(0, 0, 0, 0, 0);
}
pub fn query(sender: ProcessEndpoint, bytes: &[u8]) -> bool {
    let Some(m) = Message::decode(bytes) else {
        return false;
    };
    if m.kind != QUERY || sender.process_id.0 != 4 {
        return false;
    }
    // Kernel IPC authorizes the exact one-shot exchange before delivery.
    if let Some((length, save)) = SNAPSHOT.read(m.window, m.generation, m.revision) {
        let _ = crate::ipc_send(
            sender,
            &Message {
                kind: METADATA,
                length,
                save,
                ..m
            }
            .encode(),
        );
    }
    true
}

extern "C" fn entry(
    request: u64,
    owner: u64,
    window: u64,
    generation: u64,
    revision: u64,
    donor: u64,
) -> ! {
    if let Some((length, save)) = SNAPSHOT.read(window, generation, revision) {
        let reply = Message {
            kind: METADATA,
            request,
            owner: ProcessEndpoint::decode(owner),
            window,
            generation,
            revision,
            length,
            save,
            result: 0,
        };
        for _ in 0..4 {
            if crate::ipc_send(ProcessEndpoint::decode(donor), &reply.encode())
                != SYSCALL_ERR_MAILBOX_FULL
            {
                break;
            }
            let _ = crate::scheduler_yield();
        }
    }
    // The main task's continuation is restored by the kernel. Returning on
    // this stack would have no caller, so an unexpected refusal yields safely.
    loop {
        let _ = unsafe { crate::raw::syscall(361, [0; 5]) };
        let _ = crate::scheduler_yield();
    }
}
