use osaiku_driver_runtime::{network::*, protocol::Error};
fn geometry() -> Geometry {
    Geometry {
        generation: 7,
        capacity: 64,
        slots: 64,
        slot_bytes: 4096,
        mtu: 1500,
    }
}
fn entry(slot: u32) -> Entry {
    Entry {
        slot,
        generation: 7,
        length: 64,
        status: COMPLETED,
    }
}
#[test]
fn snapshot_validation_rejects_stale_bounds_and_reserved_fields() {
    let g = geometry();
    g.validate().unwrap();
    let e = entry(0);
    assert_eq!(Entry::decode(e.encode(), g, RingKind::RxDone), Ok(e));
    for at in [12, 13, 16, 23, 24, 31] {
        let mut b = e.encode();
        b[at] = 1;
        assert_eq!(Entry::decode(b, g, RingKind::RxDone), Err(Error::Reserved));
    }
    assert_eq!(
        Entry::decode(entry(64).encode(), g, RingKind::RxDone),
        Err(Error::Range)
    );
    let mut b = e.encode();
    b[4] = 6;
    assert_eq!(Entry::decode(b, g, RingKind::RxDone), Err(Error::Stale));
    assert_eq!(
        Entry::decode(e.encode(), g, RingKind::RxReturn),
        Err(Error::Range)
    );
}
#[test]
fn untrusted_indices_cannot_invent_capacity() {
    let mut p = Cursor::new(64, true).unwrap();
    assert_eq!(p.available(1), Err(Error::Range));
    for _ in 0..64 {
        p.advance();
    }
    assert_eq!(p.available(0), Ok(0));
    assert_eq!(p.available(65), Err(Error::Range));
    assert_eq!(p.available(64), Ok(64));
    let mut c = Cursor::new(64, false).unwrap();
    assert_eq!(c.available(65), Err(Error::Range));
    assert_eq!(c.available(64), Ok(64));
    for _ in 0..64 {
        c.advance();
    }
    assert_eq!(c.available(64), Ok(0));
    assert_eq!(c.available(63), Err(Error::Range));
}
#[test]
fn duplicate_and_premature_tx_completions_do_not_create_credit() {
    let mut slots = Ownership::<64>::new(geometry(), true).unwrap();
    let e = entry(0);
    assert_eq!(slots.receive(e), Err(Error::State));
    slots.publish(e).unwrap();
    assert_eq!(slots.publish(e), Err(Error::State));
    slots.receive(e).unwrap();
    assert_eq!(slots.receive(e), Err(Error::State));
    slots.close();
    assert_eq!(slots.publish(e), Err(Error::Stale));
}
#[test]
fn full_return_ring_retains_all_credits_until_deadline() {
    let mut q = Returns::<64>::new(64).unwrap();
    for slot in 0..64 {
        q.push(entry(slot), 10, 100).unwrap();
    }
    assert_eq!(q.push(entry(0), 10, 100), Err(Error::Backpressure));
    assert!(!q.expired(109));
    assert!(q.expired(110));
    for slot in 0..64 {
        assert_eq!(q.front(), Some(entry(slot)));
        q.published().unwrap();
    }
    assert_eq!(q.front(), None);
    assert!(!q.expired(111));
}
