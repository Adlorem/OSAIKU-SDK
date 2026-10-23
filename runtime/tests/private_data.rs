use osaiku_runtime::{packages::status, private_data::*};
fn read() -> Request {
    let mut r = Request {
        sequence: 4,
        operation: READ,
        length: 64,
        ..Request::EMPTY
    };
    r.name[..8].copy_from_slice(b"settings");
    r
}
#[test]
fn private_names_are_flat_and_requests_have_no_authority_fields() {
    assert!(read().canonical());
    for name in ["", ".", "..", "../secret", "a/b", "a\\b", "C:secret", "x\n"] {
        let mut r = read();
        r.name.fill(0);
        r.name[..name.len()].copy_from_slice(name.as_bytes());
        assert!(!r.canonical(), "{name}");
    }
    let mut r = read();
    r.name[20] = b'x';
    assert!(!r.canonical());
    r = read();
    r.bytes[0] = 1;
    assert!(!r.canonical());
    r = read();
    r.length = u64::MAX;
    assert!(!r.canonical());
    r = read();
    r.offset = u64::MAX;
    assert!(!r.canonical());
}
#[test]
fn replacement_is_atomic_bounded_and_canonical_even_for_empty_file() {
    let mut r = read();
    r.operation = REPLACE;
    r.length = 0;
    assert!(r.canonical());
    r.bytes[0] = 7;
    assert!(!r.canonical());
    r.length = 1;
    assert!(r.canonical());
    r.offset = 1;
    assert!(!r.canonical());
    r.offset = 0;
    r.length = CAPACITY as u64;
    assert!(r.canonical());
}
#[test]
fn replies_reject_cross_sequence_oversize_error_payload_and_nonzero_tail() {
    let r = read();
    let mut reply = Reply {
        sequence: r.sequence,
        status: status::OK,
        length: 3,
        ..Reply::EMPTY
    };
    reply.bytes[..3].copy_from_slice(b"abc");
    assert!(reply.valid_for(&r));
    for invalid in [
        Reply {
            sequence: 5,
            ..reply
        },
        Reply {
            length: 65,
            ..reply
        },
        Reply {
            length: u64::MAX,
            ..reply
        },
        Reply {
            status: status::DENIED,
            ..reply
        },
        Reply { length: 2, ..reply },
    ] {
        assert!(!invalid.valid_for(&r));
    }
    assert!(exchange_with(&r, |_, out| {
        *out = reply;
        0
    })
    .is_ok());
    assert!(exchange_with(&r, |_, _| 0).is_err());
    assert_eq!(exchange_with(&r, |_, _| -7), Err(-7));
    let replacement = Request {
        operation: REPLACE,
        ..r
    };
    assert!(!reply.valid_for(&replacement));
}
