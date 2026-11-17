use osaiku_gpu_protocol::measurement::*;
#[test]
fn cumulative_snapshot_retains_bytes_auxiliary_and_rejects_bad_clocks() {
    let c = Counter::new();
    c.record(Some(7), 4096, 2);
    c.record(None, 999, 8);
    c.record(Some(3), 128, 1);
    let s = c.snapshot(0).unwrap();
    assert_eq!(
        (s.count, s.ticks, s.max, s.invalid, s.bytes, s.auxiliary),
        (2, 10, 7, 1, 4224, 3)
    );
    assert_eq!(Snapshot::decode(&s.encode()), Some(s));
    assert!(Snapshot::decode(&s.encode()[..71]).is_none());
    let mut b = s.encode();
    b[8..16].copy_from_slice(&999u64.to_le_bytes());
    assert!(Snapshot::decode(&b).is_none());
}
#[test]
fn overflow_is_invalid_without_wrapping_or_partial_update() {
    let c = Counter::new();
    c.record(Some(u64::MAX), 3, 4);
    c.record(Some(1), 5, 6);
    let s = c.snapshot(1).unwrap();
    assert_eq!(
        (s.count, s.ticks, s.bytes, s.auxiliary, s.invalid),
        (1, u64::MAX, 3, 4, 1)
    );
}
#[test]
fn concurrent_accounting_never_waits_and_reports_skipped_samples() {
    let c = std::sync::Arc::new(Counter::new());
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let c = c.clone();
            std::thread::spawn(move || {
                for _ in 0..10000 {
                    c.record(Some(2), 3, 1);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let s = c.snapshot(2).unwrap();
    assert_eq!(s.count + s.lost, 40000);
    assert_eq!(s.ticks, s.count * 2);
    assert_eq!(s.bytes, s.count * 3);
}
