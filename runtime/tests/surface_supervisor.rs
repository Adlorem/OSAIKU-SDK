use osaiku_runtime::{abi::*, process::*, surface::*};

fn server() -> ProcessEndpoint {
    ProcessEndpoint::new(ProcessId(3), 7)
}

#[test]
fn binds_exact_named_instance_in_observed_epoch() {
    let mut calls = 0;
    assert_eq!(
        bind_server_with(server(), |request, payload, receipt| {
            assert!(payload.is_empty());
            assert_eq!(request.version, VERSION);
            assert_eq!(
                request.operation,
                if calls == 0 { QUERY } else { BIND_SERVER }
            );
            assert_eq!(
                request.server_endpoint,
                if calls == 0 { 0 } else { server().encode() }
            );
            assert_eq!(request.handle.target.epoch, if calls == 0 { 0 } else { 11 });
            *receipt = Receipt {
                handle: request.handle,
                epoch: 11,
                ..Receipt::default()
            };
            calls += 1;
            SYSCALL_OK
        }),
        Ok(11)
    );
    assert_eq!(calls, 2);
}

#[test]
fn rejects_unnamed_or_kernel_identity_without_a_syscall() {
    for endpoint in [
        ProcessEndpoint::unbound(ProcessId(3)),
        ProcessEndpoint::new(ProcessId::KERNEL, 1),
    ] {
        assert_eq!(
            bind_server_with(endpoint, |_, _, _| panic!("invalid identity")),
            Err(PublishError::Content(Refusal::Identity))
        );
    }
}

#[test]
fn rejects_invalid_query_receipt_before_binding() {
    for bad_handle in [false, true] {
        let mut calls = 0;
        assert_eq!(
            bind_server_with(server(), |_, _, receipt| {
                calls += 1;
                receipt.epoch = if bad_handle { 11 } else { 0 };
                receipt.handle.serial = u64::from(bad_handle);
                SYSCALL_OK
            }),
            Err(PublishError::Receipt)
        );
        assert_eq!(calls, 1);
    }
}

#[test]
fn validates_bind_echo_and_epoch_without_retrying() {
    for bad_handle in [false, true] {
        let mut calls = 0;
        assert_eq!(
            bind_server_with(server(), |request, _, receipt| {
                calls += 1;
                receipt.handle = request.handle;
                receipt.epoch = 11;
                if request.operation == BIND_SERVER {
                    if bad_handle {
                        receipt.handle.serial = 1;
                    } else {
                        receipt.epoch = 12;
                    }
                }
                SYSCALL_OK
            }),
            Err(PublishError::Receipt)
        );
        assert_eq!(calls, 2);
    }
}

#[test]
fn transport_failure_never_triggers_retry_or_revoke() {
    for fail_at in [QUERY, BIND_SERVER] {
        for code in [
            SYSCALL_ERR_PERMISSION_DENIED,
            SYSCALL_ERR_STALE_ENDPOINT,
            SYSCALL_ERR_INVALID_STATE,
        ] {
            let mut calls = 0;
            assert_eq!(
                bind_server_with(server(), |request, _, receipt| {
                    calls += 1;
                    assert!(matches!(request.operation, QUERY | BIND_SERVER));
                    receipt.handle = request.handle;
                    receipt.epoch = 11;
                    if request.operation == fail_at {
                        code
                    } else {
                        SYSCALL_OK
                    }
                }),
                Err(PublishError::Transport(code))
            );
            assert_eq!(calls, if fail_at == QUERY { 1 } else { 2 });
        }
    }
}
