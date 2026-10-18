use osaiku_runtime::{abi::*, process::*};

#[test]
fn endpoint_layout_and_round_trip_preserve_process_and_incarnation() {
    assert_eq!(core::mem::size_of::<ProcessEndpoint>(), 8);
    assert_eq!(core::mem::align_of::<ProcessEndpoint>(), 4);
    for pid in [0, 1, 0x8000_0000, u32::MAX] {
        for generation in [0, 1, 0x8000_0000, u32::MAX] {
            let endpoint = ProcessEndpoint::new(ProcessId(pid), generation);
            assert_eq!(endpoint.encode().to_le_bytes()[..4], pid.to_le_bytes());
            assert_eq!(
                endpoint.encode().to_le_bytes()[4..],
                generation.to_le_bytes()
            );
            assert_eq!(ProcessEndpoint::decode(endpoint.encode()), endpoint);
            assert_eq!(endpoint.names_instance(), generation != 0);
        }
    }
    assert!(!ProcessEndpoint::unbound(ProcessId(7)).names_instance());
}

#[test]
fn public_native_abi_keeps_existing_gate_numbers_bounds_and_errors() {
    assert_eq!(SYSCALL_VECTOR, 0x80);
    assert_eq!(number::LOG_WRITE, 0);
    assert_eq!(number::PROCESS_EXIT, 1);
    assert_eq!(number::SCHEDULER_YIELD, 22);
    assert_eq!(number::PROCESS_SELF_ENDPOINT, 336);
    assert_eq!((MAX_LOG_WRITE_LEN, MAX_IPC_MESSAGE_LEN), (160, 160));
    assert_eq!(SYSCALL_OK, 0);
    assert_eq!(SYSCALL_ERR_STALE_ENDPOINT, -12);
    assert_eq!(SYSCALL_ERR_BUSY, -13);
    assert_eq!(SYSCALL_ERR_TIMEOUT, -14);
}
