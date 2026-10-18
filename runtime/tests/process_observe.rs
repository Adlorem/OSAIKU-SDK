use osaiku_runtime::process::observe::{Notice, Observations, CAPACITY};
fn key(endpoint: u64, window: u64) -> Notice {
    Notice {
        endpoint,
        window,
        incarnation: 7,
    }
}
#[test]
fn departure_is_retained_until_taken_and_does_not_cross_incarnations() {
    let mut o = Observations::EMPTY;
    let k = key(19 | (3 << 32), 9);
    assert!(o.subscribe(k));
    assert!(!o.departed(19 | (2 << 32), None));
    assert!(!o.pending());
    assert!(o.departed(k.endpoint, None));
    assert!(o.subscribe(k)); // duplicate registration must not erase departure
    assert!(o.pending());
    assert!(!o.departed(k.endpoint, None));
    assert_eq!(o.take(), Some(k));
    assert_eq!(o.take(), None);
}
#[test]
fn window_closure_is_scoped_and_slots_are_reclaimed_without_allocation() {
    let mut o = Observations::EMPTY;
    for i in 0..CAPACITY {
        assert!(o.subscribe(key(19, i as u64 + 1)));
    }
    assert!(!o.subscribe(key(20, 99)));
    assert!(!o.departed(0, Some((2, 6))));
    assert!(o.departed(0, Some((2, 7))));
    assert_eq!(o.take(), Some(key(19, 2)));
    assert!(o.subscribe(key(20, 99)));
    o.remove(key(20, 99));
    assert!(o.subscribe(key(21, 100)));
    assert!(o.departed(19, None));
    let mut count = 0;
    while o.take().is_some() {
        count += 1;
    }
    assert_eq!(count, CAPACITY - 1);
}
#[test]
fn selection_changes_are_coalesced_and_independent_of_process_departure() {
    let mut o = Observations::EMPTY;
    let selection = key(0, 9);
    let process = key(19 | (3 << 32), 9);
    assert!(o.subscribe(selection));
    assert!(o.subscribe(process));
    assert!(o.departed(0, None));
    assert!(!o.departed(0, None));
    assert_eq!(o.take(), Some(selection));
    assert_eq!(o.take(), None);
    // Rearm before querying the full snapshot; later publication remains latched.
    assert!(o.subscribe(selection));
    assert!(o.departed(0, None));
    assert!(o.departed(process.endpoint, None));
    let a = o.take().unwrap();
    let b = o.take().unwrap();
    assert!(a != b && [a, b].contains(&selection) && [a, b].contains(&process));
}
