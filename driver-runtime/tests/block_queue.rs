use osaiku_driver_runtime::{
    block::*,
    block_queue::{Durability, Outcome, Queue as Async},
};
fn geometry() -> Geometry {
    Geometry {
        identity: Identity {
            endpoint: 3,
            backend_generation: 1,
            device_generation: 1,
            namespace_generation: 1,
            media_generation: 1,
            target_hash: 5,
        },
        backend: BackendKind::UsbBot,
        backend_version: 1,
        block_bytes: 512,
        physical_write_bytes: None,
        namespace_blocks: 1000,
        region: Region {
            start: Lba(10),
            blocks: 100,
        },
        max_blocks: 8,
        max_bytes: 4096,
        buffer_alignment: 1,
        lba_alignment: 1,
        queue_depth: 2,
        capabilities: Capabilities {
            read_only: false,
            removable: true,
            volatile_write_cache: true,
            flush: true,
            discard: false,
            fua: false,
        },
        state: State::Ready,
    }
}
fn request(id: u64, op: Operation) -> Request {
    let data = matches!(op, Operation::Read | Operation::Write);
    Request {
        identity: geometry().identity,
        id,
        operation: op,
        range: Region {
            start: Lba(0),
            blocks: u64::from(data),
        },
        buffer_token: if data { id } else { 0 },
        direction: if op == Operation::Read {
            Direction::IntoBuffer
        } else if op == Operation::Write {
            Direction::FromBuffer
        } else {
            Direction::None
        },
        ordering_domain: 1,
        predecessor: 0,
        timeout_ticks: 10,
        ticket: if matches!(op, Operation::Write | Operation::Flush) {
            7
        } else {
            0
        },
        hash: 0,
    }
    .seal()
}
fn grant() -> Option<Region> {
    Some(Region {
        start: Lba(0),
        blocks: 10,
    })
}
#[test]
fn copies_before_return_and_orders_durability_barrier() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let mut bytes = [9; 512];
    let t = q
        .submit(request(1, Operation::Write), &bytes, grant(), 0)
        .unwrap();
    bytes.fill(0);
    let d = q.dispatch(0).unwrap().unwrap();
    assert_eq!(d.physical.start, Lba(10));
    let mut got = [0; 512];
    q.copy_write(t, 0, &mut got).unwrap();
    assert_eq!(got, [9; 512]);
    assert_eq!(
        q.submit(request(2, Operation::Flush), &[], grant(), 0),
        Err(Error::NotReady)
    );
    q.complete(t, Ok(())).unwrap();
    assert_eq!(
        q.take(t, &mut []).unwrap(),
        Outcome::Complete(Durability::DeviceCache)
    );
    assert!(q.dirty());
    let f = q
        .submit(request(2, Operation::Flush), &[], grant(), 1)
        .unwrap();
    q.dispatch(1).unwrap();
    q.complete(f, Ok(())).unwrap();
    assert_eq!(
        q.take(f, &mut []).unwrap(),
        Outcome::Complete(Durability::Stable)
    );
    assert!(!q.dirty());
}
#[test]
fn rejected_ranges_never_reach_backend() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let mut r = request(1, Operation::Write);
    r.range.start = Lba(10);
    assert_eq!(
        q.submit(r.seal(), &[0; 512], grant(), 0),
        Err(Error::Permission)
    );
    assert!(q.dispatch(0).unwrap().is_none());
    assert_eq!(
        q.submit(request(1, Operation::Write), &[0; 512], None, 0),
        Err(Error::Permission)
    );
}
#[test]
fn detach_write_and_flush_are_uncertain_and_not_replayed() {
    for op in [Operation::Write, Operation::Flush] {
        let mut q = Async::<2, 4096>::new(geometry()).unwrap();
        let data = if op == Operation::Write {
            &[1; 512][..]
        } else {
            &[]
        };
        let t = q.submit(request(1, op), data, grant(), 0).unwrap();
        q.dispatch(0).unwrap();
        q.lost();
        assert_eq!(q.outcome(t).unwrap(), Some(Outcome::UnknownCommit));
        assert_eq!(q.dispatch(0).err(), Some(Error::Quarantined));
        assert_eq!(q.take(t, &mut []).unwrap(), Outcome::UnknownCommit);
        assert_eq!(q.pending(), 1);
        q.transport_stopped();
        assert_eq!(q.pending(), 0);
    }
}
#[test]
fn cancellation_before_dispatch_is_known_and_after_is_uncertain() {
    for sent in [false, true] {
        let mut q = Async::<2, 4096>::new(geometry()).unwrap();
        let t = q
            .submit(request(1, Operation::Write), &[0; 512], grant(), 0)
            .unwrap();
        if sent {
            q.dispatch(0).unwrap();
        }
        q.cancel(t).unwrap();
        assert_eq!(
            q.take(t, &mut []).unwrap(),
            if sent {
                Outcome::UnknownCommit
            } else {
                Outcome::Failed(Error::Cancelled)
            }
        );
    }
}
#[test]
fn read_fragments_are_ordered_and_short_read_cannot_succeed() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let t = q.submit(request(1, Operation::Read), &[], None, 0).unwrap();
    q.dispatch(0).unwrap();
    assert_eq!(q.copy_read(t, 1, &[0; 128]), Err(Error::Stale));
    q.copy_read(t, 0, &[7; 128]).unwrap();
    q.complete(t, Ok(())).unwrap();
    assert_eq!(q.take(t, &mut []).unwrap(), Outcome::Failed(Error::Length));
}
#[test]
fn successful_read_copies_only_after_complete() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let t = q.submit(request(1, Operation::Read), &[], None, 0).unwrap();
    q.dispatch(0).unwrap();
    q.copy_read(t, 0, &[4; 512]).unwrap();
    let mut out = [0; 512];
    assert_eq!(q.take(t, &mut out), Err(Error::NotReady));
    q.complete(t, Ok(())).unwrap();
    q.take(t, &mut out).unwrap();
    assert_eq!(out, [4; 512]);
}
#[test]
fn old_generation_and_late_completion_are_rejected() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let mut r = request(1, Operation::Read);
    r.identity.media_generation += 1;
    assert_eq!(q.submit(r.seal(), &[], None, 0), Err(Error::Stale));
    let t = q.submit(request(1, Operation::Read), &[], None, 0).unwrap();
    q.dispatch(0).unwrap();
    q.expire(10);
    assert_eq!(q.complete(t, Ok(())), Err(Error::Stale));
    assert_eq!(q.outcome(t).unwrap(), Some(Outcome::Failed(Error::Timeout)));
}
#[test]
fn transport_error_on_sent_write_is_unknown() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let t = q
        .submit(request(1, Operation::Write), &[0; 512], grant(), 0)
        .unwrap();
    q.dispatch(0).unwrap();
    q.complete(t, Err(Error::Transport)).unwrap();
    assert_eq!(q.outcome(t).unwrap(), Some(Outcome::UnknownCommit));
}
#[test]
fn power_quiesce_stops_admission() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    q.quiesce();
    assert_eq!(
        q.submit(request(1, Operation::Write), &[0; 512], grant(), 0),
        Err(Error::Quarantined)
    );
}
#[test]
fn full_queue_backpressure_and_monotonic_ids() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    q.submit(request(1, Operation::Read), &[], None, 0).unwrap();
    q.submit(request(2, Operation::Read), &[], None, 0).unwrap();
    assert_eq!(
        q.submit(request(3, Operation::Read), &[], None, 0),
        Err(Error::Resources)
    );
    q.lost();
    assert_eq!(q.dispatch(0).err(), Some(Error::Quarantined));
}

#[test]
fn detach_read_keeps_partial_data_private_and_rejects_late_completion() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let t = q.submit(request(1, Operation::Read), &[], None, 0).unwrap();
    q.dispatch(0).unwrap();
    q.copy_read(t, 0, &[8; 128]).unwrap();
    q.lost();
    let mut output = [0xa5; 512];
    assert_eq!(q.take(t, &mut output), Ok(Outcome::Failed(Error::Removed)));
    assert_eq!(output, [0xa5; 512]);
    assert_eq!(q.copy_read(t, 128, &[8; 384]), Err(Error::Stale));
    assert_eq!(q.complete(t, Ok(())), Err(Error::Stale));
    q.transport_stopped();
    let mut replacement = geometry();
    replacement.identity.media_generation += 1;
    let mut q = Async::<2, 4096>::new(replacement).unwrap();
    assert_eq!(
        q.submit(request(1, Operation::Read), &[], None, 0),
        Err(Error::Stale)
    );
}

#[test]
fn flush_timeout_retains_dirty_state_and_blocks_replay() {
    let mut q = Async::<2, 4096>::new(geometry()).unwrap();
    let t = q
        .submit(request(1, Operation::Write), &[1; 512], grant(), 0)
        .unwrap();
    q.dispatch(0).unwrap();
    q.complete(t, Ok(())).unwrap();
    q.take(t, &mut []).unwrap();
    let t = q
        .submit(request(2, Operation::Flush), &[], grant(), 1)
        .unwrap();
    q.dispatch(1).unwrap();
    q.expire(11);
    assert_eq!(q.outcome(t).unwrap(), Some(Outcome::UnknownCommit));
    assert!(q.dirty());
    assert_eq!(
        q.submit(request(3, Operation::Flush), &[], grant(), 12),
        Err(Error::Quarantined)
    );
}
