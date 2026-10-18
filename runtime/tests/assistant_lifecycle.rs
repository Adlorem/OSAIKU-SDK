//! Nonvisual transport/lifetime contracts. No UI events, Actions or raster work.
use osaiku_runtime::{
    assistant::{
        evidence::{Audit, MISSING},
        *,
    },
    process::{ProcessEndpoint, ProcessId},
};
fn peer(generation: u32) -> ProcessEndpoint {
    ProcessEndpoint::new(ProcessId(4), generation)
}
fn request(c: &mut Client, now: u64) -> Packet {
    let mut sent = None;
    c.tick(now, |b| {
        sent = Some(Packet::decode(b).unwrap().0);
        Ok(())
    });
    sent.unwrap()
}
fn reply(c: &mut Client, p: Packet, s: Status, g: u32, now: u64) -> bool {
    let mut b = [0; BYTES];
    let n = p.reply(s).encode(&[], &mut b).unwrap();
    c.receive_from(peer(g), &b[..n], now)
}
fn connected() -> Client {
    let mut c = Client::EMPTY;
    c.reset(9, 1).unwrap();
    assert!(!c.endpoint().names_instance());
    assert_eq!(c.progress(), Progress::Connecting);
    let p = request(&mut c, 1);
    assert!(reply(&mut c, p, Status::Ok, 1, 2));
    assert_eq!(c.endpoint(), peer(1));
    c
}
#[test]
fn failed_initial_handshake_never_allows_private_traffic() {
    for status in [
        Status::Unavailable,
        Status::Busy,
        Status::Refused,
        Status::Cancelled,
    ] {
        let mut c = Client::EMPTY;
        c.reset(9, 1).unwrap();
        let p = request(&mut c, 1);
        assert!(reply(&mut c, p, status, 1, 2));
        assert!(!c.endpoint().names_instance());
        assert_eq!(c.begin("private", false), Err(Status::Unavailable));
        assert_eq!(c.begin_diagnostics(), Err(Status::Unavailable));
        c.tick(3, |_| panic!("traffic before acknowledged handshake"));
    }
    for delivered in [false, true] {
        let mut c = Client::EMPTY;
        c.reset(9, 1).unwrap();
        c.tick(1, |_| {
            if delivered {
                Ok(())
            } else {
                Err(SendError::Unavailable)
            }
        });
        c.tick(2001, |_| panic!("unexpected retransmission"));
        assert_eq!(c.begin("private", false), Err(Status::Unavailable));
        c.tick(2002, |_| panic!("unbound private traffic"));
        c.reset(9, 1).unwrap();
        let p = request(&mut c, 2003);
        assert!(reply(&mut c, p, Status::Ok, 2, 2004));
        assert_eq!(c.endpoint(), peer(2));
        assert_eq!(c.begin("explicit retry", false), Ok(()));
    }
}
#[test]
fn repeated_shutdown_poll_does_not_retransmit_or_extend_cancel_deadline() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    request(&mut c, 3);
    c.cancel();
    let p = request(&mut c, 4);
    assert_eq!(p.operation, op::CANCEL);
    assert_eq!(c.progress(), Progress::Cancelling);
    for now in 5..2004 {
        c.cancel();
        c.tick(now, |_| panic!("cancel retransmitted"));
    }
    c.tick(2004, |_| panic!());
    assert_eq!(c.status, Status::ServiceTimeout);
    assert!(!c.active());
}
#[test]
fn cancellation_ack_is_terminal_and_clears_private_text() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    request(&mut c, 3);
    c.cancel();
    let p = request(&mut c, 4);
    assert!(reply(&mut c, p, Status::Cancelled, 1, 5));
    assert_eq!(c.status, Status::Cancelled);
    assert!(!c.active());
    assert!(c.input.as_str().is_empty() && c.output.as_str().is_empty());
}
#[test]
fn backend_replacement_requires_explicit_new_session_and_no_prompt_replay() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    let p = request(&mut c, 3);
    assert!(reply(&mut c, p, Status::Pending, 2, 4));
    assert_eq!(c.status, Status::Stale);
    assert!(!c.active());
    c.tick(5, |_| panic!("replayed prompt"));
    c.reset(9, 1).unwrap();
    assert!(!c.endpoint().names_instance());
    let fresh = request(&mut c, 6);
    assert!(fresh.session > p.session);
    assert!(!reply(&mut c, p, Status::Pending, 1, 7));
    assert!(reply(&mut c, fresh, Status::Ok, 2, 8));
    assert_eq!(c.endpoint(), peer(2));
    assert_eq!(c.status, Status::Ok);
}
#[test]
fn uncorrelated_replacement_packet_cannot_change_the_pinned_peer() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    let p = request(&mut c, 3);
    assert!(!reply(
        &mut c,
        Packet {
            session: p.session + 1,
            ..p
        },
        Status::Pending,
        2,
        4
    ));
    assert!(reply(&mut c, p, Status::Pending, 1, 5));
    assert!(c.active());
}
#[test]
fn clock_loss_is_latched_scrubbed_and_quiet() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    request(&mut c, 3);
    assert!(c.clock_lost());
    assert!(!c.clock_lost());
    assert!(!c.tick(4000, |_| panic!()));
    assert_eq!(c.reset(9, 1), Err(Status::ClockFailure));
    assert_eq!(c.begin("retry", false), Err(Status::ClockFailure));
    assert!(c.input.as_str().is_empty());
    assert!(!c.active());
}
#[test]
fn clock_regression_cannot_restart_a_deadline() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    request(&mut c, 100);
    assert!(c.tick(99, |_| panic!()));
    assert_eq!(c.status, Status::ClockFailure);
    assert!(!c.tick(101, |_| panic!()));
}
#[test]
fn retirement_sends_at_most_one_cancel_and_never_claims_acknowledgement() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    request(&mut c, 3);
    let mut sends = 0;
    c.close(Some(4), |b| {
        assert_eq!(Packet::decode(b).unwrap().0.operation, op::CANCEL);
        sends += 1;
        Err(SendError::Unavailable)
    });
    c.close(Some(5), |_| panic!());
    assert_eq!(sends, 1);
    assert!(!c.active());
    assert!(c.input.as_str().is_empty() && c.output.as_str().is_empty());
}
#[test]
fn busy_oom_offline_and_refusal_terminate_without_retry() {
    for status in [
        Status::Busy,
        Status::OutOfMemory,
        Status::Unavailable,
        Status::Refused,
    ] {
        let mut c = connected();
        c.begin("private", false).unwrap();
        let p = request(&mut c, 3);
        assert!(reply(&mut c, p, status, 1, 4));
        assert_eq!(c.status, status);
        assert!(!c.active());
        c.tick(5, |_| panic!());
        assert!(c.input.as_str().is_empty());
        let audit = Audit {
            packet: p.reply(status),
            flags: MISSING,
            model: 0,
            revision: 0,
            context: p.request,
        };
        assert!(Audit::decode(&audit.encode()).is_some());
    }
    let mut c = connected();
    c.begin("private", false).unwrap();
    c.tick(3, |_| Err(SendError::Unavailable));
    assert_eq!(c.status, Status::Unavailable);
    assert!(!c.active());
}
#[test]
fn progress_names_transport_stage_without_claiming_compute_percentage() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    assert_eq!(c.progress(), Progress::Sending);
    let p = request(&mut c, 3);
    reply(&mut c, p, Status::Pending, 1, 4);
    assert_eq!(c.progress(), Progress::Waiting);
    let p = request(&mut c, 5);
    let mut b = [0; BYTES];
    let n = p.reply(Status::Complete).encode(b"text", &mut b).unwrap();
    assert!(c.receive_from(peer(1), &b[..n], 6));
    assert_eq!(c.progress(), Progress::Receiving);
    let p = request(&mut c, 7);
    reply(&mut c, p, Status::Complete, 1, 8);
    assert_eq!(c.progress(), Progress::Verifying);
    assert_ne!(c.status, Status::Complete);
}

#[test]
fn full_mailbox_during_generation_preserves_request_and_never_replays_an_accepted_send() {
    let mut c = connected();
    c.begin("test", false).unwrap();
    let input = request(&mut c, 10);
    assert!(reply(&mut c, input, Status::Pending, 1, 11));
    let mut rejected = None;
    for now in [12, 62, 112, 1704] {
        assert!(!c.tick(now, |b| {
            let packet = Packet::decode(b).unwrap().0;
            assert_eq!(packet.operation, op::OBSERVE);
            assert_eq!(packet.request, input.request);
            if let Some(previous) = rejected {
                assert_eq!(packet, previous);
            }
            rejected = Some(packet);
            Err(SendError::Full)
        }));
        assert!(c.active());
        assert_eq!(c.status, Status::Pending);
    }
    let sent = request(&mut c, 1750);
    assert_eq!(Some(sent), rejected);
    c.tick(1800, |_| panic!("accepted request must not be replayed"));
    assert!(reply(&mut c, sent, Status::Pending, 1, 1801));
    assert_eq!(request(&mut c, 1802).operation, op::OBSERVE);
}

#[test]
fn full_mailbox_has_a_fixed_send_deadline() {
    let mut c = connected();
    c.begin("test", false).unwrap();
    for now in [10, 60, 1000, 2009] {
        c.tick(now, |_| Err(SendError::Full));
        assert!(c.active());
    }
    c.tick(2010, |_| panic!("expired send"));
    assert_eq!(c.status, Status::ServiceTimeout);
    assert!(!c.active());
}

#[test]
fn retry_after_deadline_renews_handshake_and_sends_only_new_explicit_prompt() {
    let mut c = connected();
    c.begin("old", false).unwrap();
    let initial = request(&mut c, 10);
    c.tick(2010, |_| panic!("mutation replay"));
    assert_eq!(c.status, Status::ServiceTimeout);
    let cancel = request(&mut c, 2011);
    assert_eq!(cancel.operation, op::CANCEL);
    assert!(reply(&mut c, cancel, Status::Cancelled, 1, 2012));
    c.begin("new explicit question", false).unwrap();
    let hello = request(&mut c, 2013);
    assert_eq!(hello.operation, op::NEW);
    assert!(hello.session > initial.session);
    assert!(!reply(&mut c, initial, Status::Pending, 1, 2014));
    assert!(reply(&mut c, hello, Status::Ok, 2, 2015));
    assert_eq!(c.endpoint(), peer(2));
    assert_eq!(request(&mut c, 2016).operation, op::INPUT);
}

#[test]
fn failed_diagnostic_retry_still_uses_diagnostics_after_handshake() {
    let mut c = connected();
    c.begin_diagnostics().unwrap();
    request(&mut c, 10);
    c.tick(2010, |_| panic!());
    assert_eq!(c.status, Status::ServiceTimeout);
    c.begin_diagnostics().unwrap();
    let hello = request(&mut c, 2011);
    assert_eq!(hello.operation, op::NEW);
    assert!(reply(&mut c, hello, Status::Ok, 1, 2012));
    assert_eq!(request(&mut c, 2013).operation, op::DIAGNOSTICS);
}

#[test]
fn lost_generation_reply_cancels_without_replaying_prompt_or_extending_cleanup() {
    let mut c = connected();
    c.begin("test", false).unwrap();
    let input = request(&mut c, 10);
    reply(&mut c, input, Status::Pending, 1, 11);
    let poll = request(&mut c, 12);
    assert!(reply(&mut c, poll, Status::Complete, 1, 13));
    let _evidence = request(&mut c, 14);
    c.tick(2014, |_| {
        panic!("expired metadata request must not be replayed")
    });
    assert_eq!(c.status, Status::ServiceTimeout);
    assert_eq!(c.progress(), Progress::Cancelling);
    assert_eq!(c.reset(9, 1), Err(Status::Busy));
    c.tick(2015, |_| Err(SendError::Full));
    let cancel = request(&mut c, 2063);
    assert_eq!(cancel.operation, op::CANCEL);
    assert_eq!(cancel.request, poll.request);
    assert!(!reply(&mut c, poll, Status::Pending, 1, 2064));
    c.tick(2065, |_| panic!("accepted CANCEL must not be replayed"));
    c.tick(4014, |_| panic!("cleanup expired"));
    assert!(!c.active());
    assert_eq!(c.status, Status::ServiceTimeout);
    assert_eq!(c.elapsed_ms(), 2004);
    c.tick(4015, |_| panic!("automatic question replay"));
}

#[test]
fn failure_projection_rejects_payloads_and_nonfailure_statuses() {
    let r = failure::Record {
        session: 1,
        request: 2,
        window: 3,
        incarnation: 4,
        status: Status::ServiceTimeout,
        operation: op::OBSERVE,
        elapsed_ms: 2000,
    };
    assert_eq!(failure::Record::decode(&r.encode()), Some(r));
    let mut bytes = r.encode();
    bytes[40..48].copy_from_slice(&(Status::Ok as u64).to_le_bytes());
    assert!(failure::Record::decode(&bytes).is_none());
    assert!(failure::Record::decode(&r.encode()[..63]).is_none());
    assert!(!failure::Record {
        operation: op::REPLY,
        ..r
    }
    .valid());
    assert!(!failure::Record { operation: 0, ..r }.valid());
}

#[test]
fn idle_clock_fault_reports_no_request_operation_and_stays_latched() {
    let mut c = connected();
    assert!(!c.active());
    assert!(c.clock_lost());
    let record = c.failure_report().unwrap();
    assert_eq!(record.operation, 0);
    assert_eq!(record.status, Status::ClockFailure);
    assert_eq!(failure::Record::decode(&record.encode()), Some(record));
    assert!(!c.clock_lost());
    assert_eq!(c.failure_report(), Some(record));
    assert_eq!(c.reset(9, 1), Err(Status::ClockFailure));
}
#[test]
fn observed_output_has_no_empty_poll_or_whole_response_timeout() {
    let mut c = connected();
    c.begin("private", false).unwrap();
    let input = request(&mut c, 3);
    assert!(reply(&mut c, input, Status::Pending, 1, 4));
    let credit = request(&mut c, 5);
    assert_eq!(credit.operation, op::OBSERVE);
    for now in [60_001, 300_001, 400_001] {
        c.tick(now, |_| panic!("empty polling"));
        assert!(c.active());
        assert_eq!(c.wake_after_ms(now), None);
    }
    let mut bytes = [0; BYTES];
    let n = credit
        .reply(Status::Pending)
        .encode("część".as_bytes(), &mut bytes)
        .unwrap();
    assert!(c.receive_from(peer(1), &bytes[..n], 400_002));
    c.cancel();
    let cancel = request(&mut c, 400_003);
    assert!(reply(&mut c, cancel, Status::CancelAccepted, 1, 400_004));
    assert!(c.active());
    assert_eq!(c.progress(), Progress::Cancelling);
    assert_eq!(c.output.as_str(), "część");
    c.cancel(); // No second mutation; next frame acknowledges accepted bytes.
    let final_credit = request(&mut c, 400_005);
    assert_eq!(final_credit.operation, op::OBSERVE);
    assert_eq!(final_credit.offset, "część".len() as u64);
    assert!(reply(&mut c, final_credit, Status::Cancelled, 1, 400_006));
    assert!(!c.active());
    assert_eq!(c.output.as_str(), "część");
}
