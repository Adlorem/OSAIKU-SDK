use osaiku_runtime::packages::*;
#[test]
fn requests_do_not_accept_authority_or_unbound_pages() {
    let mut r = Request {
        operation: operation::STATUS,
        sequence: 1,
        ..Request::EMPTY
    };
    assert!(r.canonical());
    r.operation_id[0] = 1;
    assert!(!r.canonical());
    r.operation_id = [0; 32];
    r.operation = operation::ITEM;
    assert!(!r.canonical());
    r.mount = 3;
    r.catalog = 4;
    assert!(r.canonical());
    r.index = MAX_ITEMS;
    assert!(!r.canonical());
    r.index = 0;
    r.operation = 100; // There is no public Approve opcode.
    assert!(!r.canonical());
}
#[test]
fn replies_bind_sequence_mount_revision_and_zero_unused_fields() {
    let r = Request {
        operation: operation::STATUS,
        sequence: 7,
        ..Request::EMPTY
    };
    let p = Reply {
        sequence: 7,
        status: status::OK,
        mount: 2,
        catalog: 3,
        policy_epoch: 4,
        ..Reply::EMPTY
    };
    assert!(p.valid_for(&r));
    for bad in [
        Reply { sequence: 8, ..p },
        Reply { running: 1, ..p },
        Reply {
            policy_epoch: 0,
            ..p
        },
        Reply { outcome: 2, ..p },
    ] {
        assert!(!bad.valid_for(&r));
    }
    assert!(read_with(&r, |_, out| {
        *out = p;
        0
    })
    .is_ok());
    assert!(read_with(&r, |_, _| 0).is_err());
    assert_eq!(read_with(&r, |_, _| -8), Err(-8));
}

#[test]
fn end_of_catalog_is_bound_to_the_same_snapshot() {
    let request = Request {
        operation: operation::ITEM,
        sequence: 2,
        mount: 9,
        catalog: 10,
        index: 1,
        ..Request::EMPTY
    };
    let reply = Reply {
        sequence: 2,
        status: status::END,
        mount: 9,
        catalog: 10,
        policy_epoch: 3,
        index: 1,
        ..Reply::EMPTY
    };
    assert!(reply.valid_for(&request));
    for wrong in [
        Reply { mount: 8, ..reply },
        Reply {
            catalog: 11,
            ..reply
        },
        Reply {
            policy_epoch: 0,
            ..reply
        },
        Reply {
            status: status::FAILED,
            ..reply
        },
    ] {
        assert!(!wrong.valid_for(&request));
    }
}
