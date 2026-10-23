use osaiku_runtime::package_service::*;
#[test]
fn commands_cannot_carry_consent_or_paths() {
    assert_eq!(core::mem::size_of::<Request>(), 56);
    assert_eq!(core::mem::size_of::<Reply>(), 96);
    for op in [EXECUTE, STATUS, CANCEL, CHOOSER_FAILED, RETURNED] {
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
    let bind = Request {
        operation: CHOOSER_BOUND,
        token: 5,
        endpoint: 9,
        window: 3,
        incarnation: 2,
        ..Request::EMPTY
    };
    assert!(bind.canonical());
    assert!(!Request { window: 0, ..bind }.canonical());
    assert!(!Request {
        incarnation: 0,
        ..bind
    }
    .canonical());
    assert!(!Request {
        operation: EXECUTE,
        ..bind
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

#[test]
fn host_handshake_is_versioned_and_endpoint_bound() {
    let ready = Request {
        operation: HOST_READY,
        token: 9,
        endpoint: 10,
        ..Request::EMPTY
    };
    assert!(ready.canonical());
    for op in [OPENED, OPEN_FAILED] {
        assert!(Request {
            operation: op,
            ..ready
        }
        .canonical());
        assert!(!Request {
            operation: op,
            endpoint: 0,
            ..ready
        }
        .canonical());
    }
    assert!(!Request {
        endpoint: 0,
        ..ready
    }
    .canonical());
    assert!(!Request { window: 4, ..ready }.canonical());
    assert!(!Request {
        version: 2,
        ..ready
    }
    .canonical());
    assert!(!Request {
        operation: HOST_FAILED,
        ..ready
    }
    .canonical());
    assert!(Request {
        operation: HOST_FAILED,
        endpoint: 0,
        ..ready
    }
    .canonical());
    let reply = Reply {
        sequence: ready.sequence,
        token: ready.token,
        purpose: 2,
        ..Reply::EMPTY
    };
    assert!(reply.valid_for(&ready));
    assert!(!Reply {
        purpose: 3,
        ..reply
    }
    .valid_for(&ready));
}
