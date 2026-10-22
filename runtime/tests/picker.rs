use osaiku_runtime::{launch::permission as p, picker::*};
fn open() -> Request {
    Request {
        window: 2,
        incarnation: 7,
        rights: p::SELECTED_FILE_READ,
        ..Request::EMPTY
    }
}
#[test]
fn open_is_a_request_not_a_grant() {
    let r = open();
    assert!(r.canonical());
    assert!(!Request {
        rights: p::SYSTEM_METRICS_READ,
        ..r
    }
    .canonical());
    assert!(!Request { token: 1, ..r }.canonical());
    assert!(!Request {
        incarnation: 0,
        ..r
    }
    .canonical());
    assert!(!Request { offset: 1, ..r }.canonical());
}
#[test]
fn payload_and_reserved_fields_are_canonical() {
    let mut r = Request {
        operation: REPLACE,
        rights: 0,
        token: 9,
        length: 1,
        ..open()
    };
    r.bytes[0] = 42;
    assert!(r.canonical());
    r.bytes[1] = 42;
    assert!(!r.canonical());
    assert!(!Request { length: 4097, ..r }.canonical());
    let r = Request {
        operation: READ,
        rights: 0,
        token: 9,
        length: 4096,
        ..open()
    };
    assert!(r.canonical());
    assert!(!Request {
        rights: p::SELECTED_FILE_READ,
        ..r
    }
    .canonical());
}
#[test]
fn errors_never_return_private_payload_and_replies_are_correlated() {
    let r = Request {
        operation: READ,
        rights: 0,
        token: 9,
        length: 1,
        ..open()
    };
    let mut out = Reply {
        sequence: 1,
        token: 9,
        length: 1,
        ..Reply::EMPTY
    };
    out.bytes[0] = 42;
    assert!(out.valid_for(&r));
    assert!(!Reply { status: 3, ..out }.valid_for(&r));
    assert!(!Reply { token: 8, ..out }.valid_for(&r));
    assert!(!Reply { sequence: 2, ..out }.valid_for(&r));
    out.bytes[1] = 1;
    assert!(!out.valid_for(&r));
}
