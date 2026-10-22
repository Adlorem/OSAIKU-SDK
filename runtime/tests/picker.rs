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
fn creation_names_are_single_canonical_components_and_write_only() {
    for name in [
        b"".as_slice(),
        b".",
        b"..",
        b"a/b",
        b"a\\b",
        b"C:x",
        b"x.",
        b"x ",
        b"x\n",
        &[255],
        &[b'a'; 65],
    ] {
        assert!(!valid_name(name), "{name:?}");
    }
    let mut r = Request {
        operation: CREATE,
        rights: p::SELECTED_FILE_WRITE,
        length: 5,
        ..open()
    };
    r.bytes[..5].copy_from_slice(b"a.txt");
    assert!(r.canonical());
    assert!(!Request {
        rights: p::SELECTED_FILE_READ,
        ..r
    }
    .canonical());
    assert!(!Request { rights: 3, ..r }.canonical());
    assert!(!Request { token: 1, ..r }.canonical());
    assert!(!Request { offset: 1, ..r }.canonical());
    assert!(Reply {
        sequence: r.sequence,
        token: 4,
        status: 1,
        ..Reply::EMPTY
    }
    .valid_for(&r));
    r.bytes[5] = 1;
    assert!(!r.canonical());
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

#[test]
fn delivered_scope_has_exact_shape_nonzero_fields_and_correlated_acknowledgement() {
    let m = OpenScope {
        sequence: 8,
        window: 2,
        incarnation: 3,
        token: 9,
    };
    assert_eq!(OpenScope::decode(&m.encode()), Some(m));
    assert!(OpenScope::decode(&m.encode()[..39]).is_none());
    for i in 0..5 {
        let mut b = m.encode();
        b[i * 8..i * 8 + 8].fill(0);
        assert!(OpenScope::decode(&b).is_none());
    }
    assert!(m.accepted(&m.reply(true)));
    assert!(!m.accepted(&m.reply(false)));
    assert!(!m.accepted(&OpenScope { sequence: 7, ..m }.reply(true)));
}
#[test]
fn borrowed_workspace_scrubs_partial_payload_on_transport_or_correlation_failure() {
    let request = Request {
        sequence: 9,
        operation: READ,
        window: 2,
        incarnation: 3,
        token: 4,
        length: 4,
        ..Request::EMPTY
    };
    let mut out = Reply::EMPTY;
    for code in [0, -7] {
        assert!(exchange_into_with(&request, &mut out, |_, reply| {
            reply.sequence = 8;
            reply.length = 4;
            reply.bytes[..4].fill(99);
            code
        })
        .is_err());
        assert_eq!(out, Reply::EMPTY);
    }
    exchange_into_with(&request, &mut out, |r, reply| {
        reply.sequence = r.sequence;
        reply.token = r.token;
        reply.length = 4;
        reply.bytes[..4].copy_from_slice(b"data");
        0
    })
    .unwrap();
    assert_eq!(&out.bytes[..4], b"data");
}
