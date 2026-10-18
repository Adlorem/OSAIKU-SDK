use osaiku_runtime::{
    files::{self, operation as op, status, FileReply, FileRequest, ROOT},
    report::metadata::{category, Order, Scan, Scope, Sort, State, CAPACITY, DIRECTORIES},
};

#[derive(Clone)]
struct Object {
    id: u64,
    parent: u64,
    name: String,
    size: Option<u64>,
}
fn file(id: u64, parent: u64, name: &str, size: u64) -> Object {
    Object {
        id,
        parent,
        name: name.into(),
        size: Some(size),
    }
}
fn reply(r: &FileRequest, status: u64) -> FileReply {
    FileReply {
        sequence: r.sequence,
        status,
        catalog_revision: 7,
        mount_generation: 9,
        ..FileReply::EMPTY
    }
}
fn serve(r: &FileRequest, objects: &[Object]) -> FileReply {
    assert!(r.canonical());
    assert_eq!((r.window_id, r.window_generation), (21, 3));
    assert!(files::metadata_operation(r.operation));
    if r.catalog_revision != 0 {
        assert_eq!((r.catalog_revision, r.mount_generation), (7, 9));
    }
    let object = if r.operation == op::LIST_V2 {
        assert_eq!(r.list_session(), Some(3));
        objects
            .iter()
            .filter(|o| o.parent == r.parent)
            .nth(r.object as usize)
    } else {
        assert_eq!(r.expected_version, 4);
        objects.iter().find(|o| o.id == r.object)
    };
    let Some(o) = object else {
        return reply(r, status::END);
    };
    let mut out = FileReply {
        object: o.id,
        object_version: 4,
        parent: o.parent,
        kind: if o.size.is_some() { 1 } else { 2 },
        name_len: o.name.len() as u64,
        ..reply(r, status::OK)
    };
    out.name[..o.name.len()].copy_from_slice(o.name.as_bytes());
    if r.operation == op::LIST_V2 {
        out.set_list_cursor(r.object + 1);
    } else {
        out.set_logical_bytes(o.size.unwrap());
    }
    out
}
fn started() -> Scan {
    let mut scan = Scan::EMPTY;
    scan.start(Scope {
        window: 21,
        incarnation: 3,
    });
    scan
}
fn finish(scan: &mut Scan, mut fetch: impl FnMut(&FileRequest) -> FileReply) -> usize {
    let mut calls = 0;
    for _ in 0..2050 {
        scan.step(|r| {
            calls += 1;
            fetch(r)
        });
        if scan.state != State::Running {
            return calls;
        }
    }
    panic!("query exceeded work bound")
}

#[test]
fn traverses_directories_and_uses_stat_logical_size_without_reading_content() {
    let objects = [
        file(2, ROOT, "a.PDF", 1u64 << 40),
        Object {
            id: 3,
            parent: ROOT,
            name: "nested".into(),
            size: None,
        },
        file(4, 3, "photo.png", 300),
        file(5, 3, "empty", 0),
    ];
    let mut scan = started();
    assert_eq!(finish(&mut scan, |r| serve(r, &objects)), 9);
    assert_eq!(
        (scan.state, scan.len, scan.revision, scan.mount),
        (State::Complete, 3, 7, 9)
    );
    assert_eq!(
        scan.totals(),
        [Some(1 << 40), Some(300), Some(0), Some(0), Some(0), Some(0)]
    );
}

#[test]
fn revision_or_mount_change_discards_all_previous_values() {
    let objects = [file(2, ROOT, "a.txt", 23), file(3, ROOT, "b.txt", 42)];
    for change_mount in [false, true] {
        let mut scan = started();
        for _ in 0..2 {
            scan.step(|r| serve(r, &objects));
        }
        assert_eq!(scan.len, 1);
        scan.step(|r| {
            let mut out = serve(r, &objects);
            if change_mount {
                out.mount_generation += 1
            } else {
                out.catalog_revision += 1
            };
            out
        });
        assert_eq!((scan.state, scan.len), (State::Stale, 0));
        assert!(scan.entries.iter().all(|e| e.object == 0));
    }
}

#[test]
fn service_conflict_and_stale_never_leave_mixed_snapshot() {
    for code in [status::STALE, status::CONFLICT] {
        let mut scan = started();
        let objects = [file(2, ROOT, "a", 1)];
        for _ in 0..2 {
            scan.step(|r| serve(r, &objects));
        }
        scan.step(|r| reply(r, code));
        assert_eq!((scan.state, scan.len), (State::Stale, 0));
    }
}

#[test]
fn malformed_sequence_version_cursor_name_and_parent_are_rejected() {
    for case in 0..7 {
        let mut scan = started();
        scan.step(|r| {
            let mut out = serve(r, &[file(2, ROOT, "a", 1)]);
            match case {
                0 => out.sequence += 1,
                1 => out.version += 1,
                2 => out.set_list_cursor(0),
                3 => out.name_len = 65,
                4 => out.name[63] = 1,
                5 => out.parent += 1,
                _ => out.name_len = 0,
            };
            out
        });
        assert_eq!((scan.state, scan.len), (State::Invalid, 0));
    }
}

#[test]
fn stat_must_match_enumerated_object_and_version() {
    let objects = [file(2, ROOT, "a", 1)];
    for case in 0..4 {
        let mut scan = started();
        scan.step(|r| serve(r, &objects));
        scan.step(|r| {
            let mut out = serve(r, &objects);
            match case {
                0 => out.object += 1,
                1 => out.object_version += 1,
                2 => out.parent += 1,
                _ => out.kind = 2,
            };
            out
        });
        assert_eq!((scan.state, scan.len), (State::Invalid, 0));
    }
}

#[test]
fn missing_or_overflowed_sizes_are_unknown_not_zero() {
    let mut scan = started();
    let objects = [
        file(2, ROOT, "a.txt", u64::MAX),
        file(3, ROOT, "b.txt", 1),
        file(4, ROOT, "c.jpg", 42),
    ];
    finish(&mut scan, |r| {
        let mut out = serve(r, &objects);
        if r.operation == op::STAT_V2 && r.object == 4 {
            out.content_len = 0;
            out.content.fill(0);
        }
        out
    });
    assert_eq!(scan.state, State::Partial);
    assert_eq!(scan.totals()[..2], [None, None]);
    assert_eq!(scan.entries[2].bytes, None);
}

#[test]
fn cancel_erases_results_and_stops_all_service_calls() {
    let mut scan = started();
    for _ in 0..2 {
        scan.step(|r| serve(r, &[file(2, ROOT, "a", 1)]));
    }
    scan.cancel();
    scan.step(|_| panic!("canceled query called storage"));
    assert_eq!((scan.state, scan.len), (State::Canceled, 0));
    scan.start(Scope {
        window: 0,
        incarnation: 1,
    });
    scan.step(|_| panic!("invalid scope called storage"));
    assert_eq!(scan.state, State::Invalid);
}

#[test]
fn busy_is_retried_with_a_finite_limit() {
    let mut scan = started();
    assert_eq!(finish(&mut scan, |r| reply(r, status::BUSY)), 100);
    assert_eq!(scan.state, State::Unavailable);
    let mut scan = started();
    let objects = [file(2, ROOT, "a", 1)];
    let mut busy = true;
    finish(&mut scan, |r| {
        if busy {
            busy = false;
            reply(r, status::BUSY)
        } else {
            serve(r, &objects)
        }
    });
    assert_eq!((scan.state, scan.len), (State::Complete, 1));
}

#[test]
fn global_step_bound_survives_periodic_busy_responses() {
    let mut scan = started();
    let objects: Vec<_> = (2..258).map(|id| file(id, ROOT, "a", 1)).collect();
    let mut count = 0;
    assert_eq!(
        finish(&mut scan, |r| {
            count += 1;
            if count % 90 != 0 {
                reply(r, status::BUSY)
            } else {
                serve(r, &objects)
            }
        }),
        2048
    );
    assert_eq!(scan.state, State::Partial);
}

#[test]
fn file_and_directory_caps_return_explicit_partial_results() {
    let mut scan = started();
    let objects: Vec<_> = (2..CAPACITY as u64 + 3)
        .map(|id| file(id, ROOT, "a", 1))
        .collect();
    finish(&mut scan, |r| serve(r, &objects));
    assert_eq!(
        (scan.state, scan.len, scan.omitted),
        (State::Partial, CAPACITY, 1)
    );
    let mut scan = started();
    let objects: Vec<_> = (2..DIRECTORIES as u64 + 2)
        .map(|id| Object {
            id,
            parent: ROOT,
            name: "dir".into(),
            size: None,
        })
        .collect();
    finish(&mut scan, |r| serve(r, &objects));
    assert_eq!((scan.state, scan.omitted), (State::Partial, 1));
}

#[test]
fn directory_cycles_and_duplicate_objects_are_invalid() {
    for objects in [
        vec![Object {
            id: ROOT,
            parent: ROOT,
            name: "loop".into(),
            size: None,
        }],
        vec![file(2, ROOT, "a", 1), file(2, ROOT, "b", 2)],
    ] {
        let mut scan = started();
        finish(&mut scan, |r| serve(r, &objects));
        assert_eq!((scan.state, scan.len), (State::Invalid, 0));
    }
}

#[test]
fn failure_after_some_results_is_partial_and_empty_catalog_is_complete() {
    let mut scan = started();
    for _ in 0..2 {
        scan.step(|r| serve(r, &[file(2, ROOT, "a", 1)]));
    }
    scan.step(|r| reply(r, status::PERMISSION));
    assert_eq!((scan.state, scan.len), (State::Partial, 1));
    let mut scan = started();
    finish(&mut scan, |r| reply(r, status::END));
    assert_eq!((scan.state, scan.len), (State::Complete, 0));
}

#[test]
fn sort_filter_and_reconciliation_use_stable_object_keys() {
    let mut scan = started();
    let objects = [
        file(8, ROOT, "z.txt", 5),
        file(2, ROOT, "a.TXT", 5),
        file(7, ROOT, "b.png", 3),
    ];
    finish(&mut scan, |r| serve(r, &objects));
    let mut order = Order::EMPTY;
    order.rebuild(&scan, None, "", Sort::Name, false);
    assert_eq!(&order.indices[..order.len], &[1, 2, 0]);
    assert_eq!(order.index_of(&scan, 8), Some(2));
    order.rebuild(&scan, None, "", Sort::Size, true);
    assert_eq!(&order.indices[..order.len], &[0, 1, 2]);
    assert_eq!(order.index_of(&scan, 8), Some(0));
    order.rebuild(&scan, Some(0), "TXT", Sort::Name, false);
    assert_eq!(&order.indices[..order.len], &[1, 0]);
    order.rebuild(&scan, Some(1), "a", Sort::Name, false);
    assert_eq!(order.len, 0);
}

#[test]
fn categories_are_filename_rules_and_metadata_policy_is_read_only() {
    for (name, c) in [
        ("a.PDF", 0),
        ("photo.JPEG", 1),
        ("movie.mp4", 2),
        ("backup.gz", 3),
        ("main.rs", 4),
        ("unknown", 5),
    ] {
        assert_eq!(category(name), c);
    }
    for code in 0..100 {
        assert_eq!(
            files::metadata_operation(code),
            matches!(code, op::LIST_V2 | op::STAT_V2)
        );
    }
    assert_eq!(core::mem::size_of::<FileReply>(), 416);
    assert_eq!(core::mem::size_of::<FileRequest>(), 416);
}
