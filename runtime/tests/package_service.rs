use osaiku_runtime::package_service::*;
#[test]
fn commands_cannot_carry_consent_or_paths() {
    assert_eq!(core::mem::size_of::<Request>(), 32);
    assert_eq!(core::mem::size_of::<Reply>(), 64);
    for op in [EXECUTE, STATUS, CANCEL, OPENED, OPEN_FAILED] {
        assert!(!Request {
            operation: op,
            ..Request::EMPTY
        }
        .canonical());
        assert!(Request {
            operation: op,
            token: 9,
            ..Request::EMPTY
        }
        .canonical());
    }
    assert!(!Request {
        token: 1,
        ..Request::EMPTY
    }
    .canonical());
    assert!(!Request {
        sequence: 0,
        ..Request::EMPTY
    }
    .canonical());
}
#[test]
fn reply_requires_exact_sequence_token_and_bounded_progress() {
    let r = Request {
        operation: STATUS,
        token: 42,
        ..Request::EMPTY
    };
    let reply = Reply {
        sequence: r.sequence,
        token: 42,
        phase: 2,
        completed: 20,
        total: 21,
        ..Reply::EMPTY
    };
    assert!(reply.valid_for(&r));
    assert!(!Reply { token: 41, ..reply }.valid_for(&r));
    assert!(!Reply {
        sequence: 2,
        ..reply
    }
    .valid_for(&r));
    assert!(!Reply {
        completed: 22,
        ..reply
    }
    .valid_for(&r));
    assert!(!Reply { phase: 6, ..reply }.valid_for(&r));
}
#[test]
fn exchange_refuses_forged_transport_and_malformed_reply() {
    let r = Request::EMPTY;
    assert!(exchange_with(&r, |_, out| {
        *out = Reply {
            sequence: 99,
            ..Reply::EMPTY
        };
        0
    })
    .is_err());
    assert!(exchange_with(&r, |_, _| -1).is_err());
}
