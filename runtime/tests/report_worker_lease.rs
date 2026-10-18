//! Thread/workspace lifetime contract; no UI or production test hooks.
#[path = "../src/report/metadata/lease.rs"]
mod lease;
use core::cell::Cell;

#[test]
fn abandoned_worker_keeps_ownership_until_join_and_scrub_finish() {
    let lease = lease::Lease::new();
    let acquire = || lease.acquire(|_| panic!("no orphan"), || panic!("no orphan"));
    assert!(acquire());
    assert!(!acquire());
    lease.abandon(7);
    assert!(!lease.acquire(
        |id| {
            assert_eq!(id, 7);
            false
        },
        || panic!("worker is live")
    ));
    let cleared = Cell::new(false);
    assert!(lease.acquire(
        |id| {
            assert_eq!(id, 7);
            assert!(!acquire());
            true
        },
        || {
            assert!(!acquire());
            cleared.set(true);
        }
    ));
    assert!(cleared.get());
    assert!(!acquire());
    lease.release();
    assert!(acquire());
    lease.release();
}
