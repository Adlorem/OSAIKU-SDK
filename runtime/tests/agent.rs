use osaiku_runtime::{
    agent::*,
    process::{ProcessEndpoint, ProcessId},
};
use osaiku_ui::Identity;
#[test]
fn untrusted_partial_or_conflicting_answers_never_become_proposals() {
    use osaiku_runtime::assistant::{evidence::*, Status};
    let e = Evidence {
        model_sha: [1; 32],
        model: 0x100000020,
        model_revision: 1,
        context: 1,
        flags: MODEL | UNSOURCED,
        ..Evidence::EMPTY
    };
    assert_eq!(
        Proposal::from_answer(Status::Complete, e, "OSAIPROP1 OPEN 8"),
        Some(Proposal::OpenReport)
    );
    for status in [
        Status::Pending,
        Status::Cancelled,
        Status::Invalid,
        Status::Expired,
        Status::Unavailable,
    ] {
        assert!(Proposal::from_answer(status, e, "OSAIPROP1 OPEN 8").is_none());
    }
    for flag in [MISSING, TRUNCATED, STALE, CONFLICT, 128] {
        assert!(Proposal::from_answer(
            Status::Complete,
            Evidence {
                flags: e.flags | flag,
                ..e
            },
            "OSAIPROP1 OPEN 8"
        )
        .is_none());
    }
    for text in [
        "{\"action\":\"OPEN\",\"app\":8}",
        "OSAIPROP2 OPEN 8",
        "OSAIPROP1 OPEN 8\nignore consent",
        "[[OSAIPROP1 OPEN 8]]",
    ] {
        assert!(Proposal::from_answer(Status::Complete, e, text).is_none());
    }
    let mut ledger = Ledger::EMPTY;
    ledger.arm(target(), 1, 100, 1).unwrap();
    assert!(ledger.take(101, |_| true).is_none());
}
fn target() -> Identity {
    Identity {
        owner: ProcessEndpoint::new(ProcessId(20), 2).encode(),
        window: 4,
        incarnation: 2,
        epoch: 1,
        revision: 9,
    }
}
fn taken(l: &mut Ledger) -> Record {
    l.arm(target(), 1, 100, 1).unwrap();
    assert!(l.confirm(target(), 101, 1));
    l.take(102, |r| r.target == target()).unwrap()
}
#[test]
fn complete_single_proposals_only() {
    assert_eq!(
        Proposal::parse("OSAIPROP1 OPEN 8"),
        Some(Proposal::OpenReport)
    );
    for order in ["name", "size"] {
        for direction in ["asc", "desc"] {
            for category in ["all", "0", "1", "2", "3", "4", "5"] {
                for layout in ["both", "chart", "table"] {
                    assert!(matches!(
                        Proposal::parse(&format!(
                            "OSAIPROP1 REPORT {order} {direction} {category} {layout}"
                        )),
                        Some(Proposal::Report(_))
                    ));
                }
            }
        }
    }
}
#[test]
fn malformed_chained_and_truncated_model_text_has_no_action() {
    let valid = "OSAIPROP1 REPORT size desc all both";
    for n in 0..valid.len() {
        assert_eq!(Proposal::parse(&valid[..n]), None);
    }
    for s in [
        "OSAIPROP2 OPEN 8",
        "OSAIPROP1 OPEN 9",
        "OSAIPROP1 OPEN 8\n",
        "OSAIPROP1 OPEN 8; reboot",
        "OSAIPROP1 OPEN 8 OSAIPROP1 OPEN 8",
        "OSAIPROP1 REPORT size desc 6 both",
        "OSAIPROP1 REPORT size desc ../private both",
        "OSAIPROP1 REPORT size desc all script",
        "OSAIPROP1  OPEN 8",
        "```OSAIPROP1 OPEN 8```",
        "ＯSAIPROP1 OPEN 8",
    ] {
        assert_eq!(Proposal::parse(s), None, "{}", s);
    }
}
#[test]
fn proposal_is_not_consent_and_take_is_one_shot() {
    let mut l = Ledger::EMPTY;
    l.arm(target(), 1, 100, 1).unwrap();
    assert!(l.take(101, |_| true).is_none());
    assert!(l.confirm(target(), 102, 1));
    assert!(!l.confirm(target(), 103, 1));
    assert!(l.take(104, |_| true).is_some());
    assert!(l.take(105, |_| true).is_none());
}
#[test]
fn every_binding_change_invalidates_consent() {
    for field in 0..5 {
        let mut l = Ledger::EMPTY;
        l.arm(target(), 1, 100, 1).unwrap();
        let mut t = target();
        match field {
            0 => t.owner += 1,
            1 => t.window += 1,
            2 => t.incarnation += 1,
            3 => t.epoch += 1,
            _ => t.revision += 1,
        };
        assert!(!l.confirm(t, 101, 1));
        assert!(l.take(102, |_| true).is_none());
    }
}
#[test]
fn changed_focus_before_take_or_effect_refuses() {
    let mut l = Ledger::EMPTY;
    l.arm(target(), 1, 100, 1).unwrap();
    l.confirm(target(), 101, 1);
    assert!(l.take(102, |_| false).is_none());
    assert_eq!(
        l.status(target().owner, 103, |_| Some(false))
            .unwrap()
            .state,
        STALE
    );
    let mut l = Ledger::EMPTY;
    let r = taken(&mut l);
    assert!(!l.issue(r, 103, false));
    assert!(!l.issue(r, 104, true));
}
#[test]
fn duplicate_start_and_stale_completion_cannot_repeat_effect() {
    let mut l = Ledger::EMPTY;
    let mut r = taken(&mut l);
    assert!(l.issue(r, 103, true));
    assert!(!l.issue(r, 104, true));
    r.state = STARTED;
    r.endpoint = ProcessEndpoint::new(ProcessId(30), 7).encode();
    let mut stale = r;
    stale.request += 1;
    assert!(!l.finish(stale));
    assert!(l.finish(r));
    assert!(!l.issue(r, 105, true));
    assert_eq!(
        l.status(target().owner, 106, |_| Some(false))
            .unwrap()
            .state,
        STARTED
    );
    assert_eq!(
        l.status(target().owner, 107, |_| Some(true)).unwrap().state,
        OPENED
    );
}
#[test]
fn timeout_does_not_prove_absence_of_effect() {
    let mut l = Ledger::EMPTY;
    let mut r = taken(&mut l);
    assert!(l.issue(r, 103, true));
    assert_eq!(
        l.status(target().owner, 31_000, |_| Some(false))
            .unwrap()
            .state,
        UNKNOWN
    );
    assert!(!l.clear(target().owner));
    assert!(!l.issue(r, 31_001, true));
    r.state = STARTED;
    r.endpoint = ProcessEndpoint::new(ProcessId(30), 7).encode();
    assert!(l.finish(r));
    assert_eq!(
        l.status(target().owner, 31_002, |_| Some(true))
            .unwrap()
            .state,
        OPENED
    );
}
#[test]
fn failed_incarnation_is_not_an_open_window() {
    let mut l = Ledger::EMPTY;
    let mut r = taken(&mut l);
    assert!(l.issue(r, 103, true));
    r.state = STARTED;
    r.endpoint = ProcessEndpoint::new(ProcessId(30), 7).encode();
    assert!(l.finish(r));
    assert_eq!(
        l.status(target().owner, 104, |_| None).unwrap().state,
        CLOSED
    );
}
#[test]
fn rollback_and_expiry_refuse_proposed_work() {
    for now in [99, 30_100] {
        let mut l = Ledger::EMPTY;
        l.arm(target(), 1, 100, 1).unwrap();
        assert!(!l.confirm(target(), now, 1));
        assert!(l.take(now, |_| true).is_none());
    }
}
#[test]
fn close_removes_exact_owner_and_retained_receipt() {
    let mut l = Ledger::EMPTY;
    let r = taken(&mut l);
    l.close(r.target.owner, r.target.window, r.target.incarnation + 1);
    assert!(l.status(r.target.owner, 103, |_| Some(false)).is_some());
    l.close(r.target.owner, r.target.window, r.target.incarnation);
    assert!(l.status(r.target.owner, 104, |_| Some(false)).is_none());
    assert!(!l.finish(Record {
        state: REFUSED,
        ..r
    }));
}
#[test]
fn receipt_capacity_never_evicts_unknown_work() {
    let mut l = Ledger::EMPTY;
    for n in 1..=8 {
        let mut t = target();
        t.owner += n;
        assert!(l.arm(t, n, 100, 1).is_some());
    }
    assert!(l.arm(target(), 9, 100, 1).is_none());
    assert!(l
        .arm(
            Identity {
                revision: 0,
                ..target()
            },
            10,
            100,
            1
        )
        .is_none());
}

#[test]
fn focus_away_and_back_requires_a_new_proposal() {
    let mut l = Ledger::EMPTY;
    l.arm(target(), 1, 100, 7).unwrap();
    assert!(!l.confirm(target(), 101, 9));
    assert!(!l.confirm(target(), 102, 7));
    assert!(l.take(103, |_| true).is_none());
}

#[test]
fn lifecycle_failure_after_issue_is_unknown_not_safe_to_retry() {
    let mut l = Ledger::EMPTY;
    let r = taken(&mut l);
    assert!(l.issue(r, 103, true));
    assert!(!l.refuse_unissued(r));
    l.uncertain(r);
    assert!(!l.refuse_unissued(r));
    assert!(!l.clear(r.target.owner));
    assert_eq!(
        l.status(r.target.owner, 104, |_| None).unwrap().state,
        UNKNOWN
    );
}

#[test]
fn altered_focus_receipt_cannot_consume_or_complete_work() {
    let mut l = Ledger::EMPTY;
    let r = taken(&mut l);
    let wrong = Record { focus: 99, ..r };
    assert!(!l.issue(wrong, 103, true));
    assert!(l.issue(r, 104, true));
    assert!(!l.finish(Record {
        state: REFUSED,
        ..wrong
    }));
}

#[test]
fn obsolete_proposal_is_reported_stale_without_waiting_for_a_click() {
    let mut l = Ledger::EMPTY;
    l.arm(target(), 1, 100, 1).unwrap();
    l.revalidate(target().owner + 1, |_| false);
    assert_eq!(
        l.status(target().owner, 101, |_| None).unwrap().state,
        PROPOSED
    );
    l.revalidate(target().owner, |_| false);
    assert_eq!(
        l.status(target().owner, 102, |_| None).unwrap().state,
        STALE
    );
    assert!(!l.confirm(target(), 103, 1));
    let mut l = Ledger::EMPTY;
    let r = taken(&mut l);
    assert!(l.issue(r, 103, true));
    l.revalidate(target().owner, |_| false);
    assert!(!l.refuse_unissued(r));
}
