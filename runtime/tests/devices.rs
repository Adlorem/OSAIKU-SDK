use osaiku_runtime::devices::*;
#[test]
fn proposal_is_a_preview_bound_to_exact_state_and_never_an_execution_grant() {
    let r = Record {
        device: 3,
        connection: 7,
        state: state::READY,
        active: 5,
        flags: flag::PROTECTED,
        ..Record::EMPTY
    }
    .seal();
    assert!(r.proposal(PROPOSE_DISABLE, r.stamp + 1).is_none());
    assert!(r.proposal(99, r.stamp).is_none());
    let p = r.proposal(PROPOSE_DISABLE, r.stamp).unwrap();
    assert_eq!(p.state, r.state);
    assert_eq!(p.endpoint, r.endpoint);
    assert_eq!(p.flags, flag::PROTECTED | flag::CONFIRMATION);
    let changed = Record { connection: 8, ..r }.seal();
    assert!(changed.proposal(PROPOSE_DISABLE, r.stamp).is_none());
}
#[test]
fn event_log_is_bounded_and_reports_cursor_loss_without_partial_pages() {
    let mut log = Log::NEW;
    for i in 0..EVENTS + 3 {
        log.push(1, 2, i as u64, EventKind::Ready).unwrap();
    }
    let mut out = [None; 4];
    assert_eq!(log.page(0, &mut out), None);
    assert_eq!(log.page(3, &mut out), Some(4));
    assert_eq!(out[0].unwrap().sequence, 4);
    assert_eq!(log.page(log.sequence() + 1, &mut out), None);
}
