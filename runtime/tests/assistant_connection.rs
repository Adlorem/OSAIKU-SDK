//! Nonvisual readiness, identity and replay contracts. No UI tests.
use osaiku_runtime::{
    assistant::{connection::*, op, Packet, Status, BYTES},
    process::{ProcessEndpoint, ProcessId},
};
fn scope() -> Scope {
    Scope {
        server: ProcessEndpoint::new(ProcessId(4), 1),
        session: 1,
        window: 9,
        incarnation: 2,
        model: (3 << 32) | 19,
    }
}
fn query(c: &mut Client, now: u64) -> Packet {
    let mut packet = None;
    c.tick(scope(), now, true, |_, bytes| {
        packet = Some(Packet::decode(bytes).unwrap().0);
        true
    });
    packet.unwrap()
}
fn response(p: Packet, state: State) -> Vec<u8> {
    let r = Record {
        model: p.value,
        state,
        reason: match state {
            State::Ready => Status::Ok,
            State::Retry => Status::StartupTimeout,
            State::Preparing => Status::Preparing,
            _ => Status::Busy,
        },
        elapsed_ms: 120_000,
        attempts: 40,
    };
    let mut bytes = [0; BYTES];
    let mut payload = [0; 48];
    payload[..40].copy_from_slice(&r.encode());
    payload[40..].copy_from_slice(&1u64.to_le_bytes());
    let n = p.reply(Status::Ok).encode(&payload, &mut bytes).unwrap();
    bytes[..n].to_vec()
}
#[test]
fn readiness_response_requires_exact_model_session_request_and_server() {
    let mut c = Client::EMPTY;
    let p = query(&mut c, 1);
    let good = response(p, State::Ready);
    assert!(!c.receive(ProcessEndpoint::new(ProcessId(4), 2), &good, 2));
    for wrong in [
        Packet { session: 2, ..p },
        Packet {
            request: p.request + 1,
            ..p
        },
        Packet {
            value: p.value + (1 << 32),
            ..p
        },
        Packet { offset: 1, ..p },
    ] {
        assert!(!c.receive(scope().server, &response(wrong, State::Ready), 2));
    }
    assert!(c.receive(scope().server, &good, 2));
    assert_eq!(c.record.state, State::Ready);
    assert!(!c.retry_available());
}
#[test]
fn reconnect_requires_explicit_retryable_state_and_is_never_replayed() {
    let mut c = Client::EMPTY;
    c.reconnect();
    let p = query(&mut c, 1);
    assert_eq!(p.offset, 0);
    assert!(c.receive(scope().server, &response(p, State::Retry), 2));
    assert!(c.retry_available());
    c.reconnect();
    let retry = query(&mut c, 3);
    assert_eq!(retry.offset, 1);
    assert_eq!(retry.operation, op::CONNECTION);
    c.tick(scope(), 1000, true, |_, _| panic!("replayed request"));
    c.tick(scope(), 2003, true, |_, _| panic!("replayed request"));
    assert_eq!(c.status, Status::ServiceTimeout);
    c.tick(scope(), 3003, true, |_, _| {
        panic!("lost reply cannot start periodic queries")
    });
    assert_eq!(c.wake_after_ms(3003), None);
    assert!(c.session_retry_available());
    // An explicitly renewed NEW session permits a fresh status subscription.
    let mut next = None;
    c.tick(
        Scope {
            session: 2,
            ..scope()
        },
        3004,
        true,
        |_, bytes| {
            next = Some(Packet::decode(bytes).unwrap().0);
            true
        },
    );
    let next = next.unwrap();
    assert_eq!(next.offset, 0);
    assert!(next.request > retry.request);
    assert!(!c.receive(scope().server, &response(retry, State::Ready), 3005));
}
#[test]
fn new_model_or_window_session_invalidates_pending_readiness_and_retry() {
    let mut c = Client::EMPTY;
    let p = query(&mut c, 1);
    let other = Scope {
        session: 2,
        model: scope().model + (1 << 32),
        ..scope()
    };
    c.tick(other, 2, false, |_, _| panic!("active conversation"));
    assert!(!c.receive(scope().server, &response(p, State::Ready), 3));
    assert_eq!(c.record, Record::EMPTY);
    assert!(!c.retry_available());
}
#[test]
fn full_mailbox_and_clock_failure_remain_bounded() {
    let mut c = Client::EMPTY;
    for now in [1, 1000, 2000] {
        c.tick(scope(), now, true, |_, _| false);
    }
    c.tick(scope(), 2001, true, |_, _| panic!("expired unsent request"));
    assert_eq!(c.status, Status::ServiceTimeout);
    c.tick(scope(), 2000, true, |_, _| panic!("clock failure"));
    assert_eq!(c.status, Status::ClockFailure);
    c.tick(scope(), 5000, true, |_, _| panic!("latched clock failure"));
    assert_eq!(c.status, Status::ClockFailure);
}
#[test]
fn malformed_readiness_never_exposes_ready_or_retry() {
    let mut c = Client::EMPTY;
    let p = query(&mut c, 1);
    let mut bytes = response(p, State::Ready);
    // Ready cannot carry a busy/failure reason.
    bytes[64 + 16..64 + 24].copy_from_slice(&(Status::Busy as u64).to_le_bytes());
    assert!(!c.receive(scope().server, &bytes, 2));
    assert_eq!(c.record, Record::EMPTY);
    assert!(!c.retry_available());
}
#[test]
fn startup_failure_observation_uses_negotiation_duration_and_is_not_duplicated() {
    let mut c = Client::EMPTY;
    let p = query(&mut c, 1);
    assert!(c.receive(scope().server, &response(p, State::Retry), 2));
    let failure = c.failure_report().unwrap();
    assert_eq!(failure.elapsed_ms, 120_000);
    assert_eq!(failure.operation, op::CONNECTION);
    assert_eq!(failure.status, Status::StartupTimeout);
    assert_eq!(
        osaiku_runtime::assistant::failure::Record::decode(&failure.encode()),
        Some(failure)
    );
    c.tick(scope(), 1002, true, |_, _| {
        panic!("steady subscription must stay quiet")
    });
    assert!(!c.receive(scope().server, &response(p, State::Retry), 1003));
    assert_eq!(c.failure_report(), Some(failure));
    c.reconnect();
    let p = query(&mut c, 1004);
    assert!(c.receive(scope().server, &response(p, State::Preparing), 1005));
    assert_eq!(c.failure_report(), None);
    let mut invalid = failure;
    invalid.operation = op::INPUT;
    assert!(!invalid.valid());
}
#[test]
fn replaced_controller_session_exposes_explicit_renewal_without_resending_a_question() {
    let mut c = Client::EMPTY;
    let p = query(&mut c, 1);
    let mut bytes = [0; BYTES];
    let n = p.reply(Status::Stale).encode(&[], &mut bytes).unwrap();
    assert!(c.receive(scope().server, &bytes[..n], 2));
    assert!(c.session_retry_available());
    assert!(!c.retry_available());
    let mut conversation = osaiku_runtime::assistant::Client::EMPTY;
    conversation
        .reset(scope().window, scope().incarnation)
        .unwrap();
    conversation.tick(3, |bytes| {
        let (request, payload) = Packet::decode(bytes).unwrap();
        assert_eq!(request.operation, op::NEW);
        assert!(payload.is_empty());
        Ok(())
    });
    c.tick(
        Scope {
            session: 2,
            ..scope()
        },
        3,
        false,
        |_, _| panic!("during handshake"),
    );
    assert!(!c.session_retry_available());
}
fn connect(conversation: &mut osaiku_runtime::assistant::Client, now: u64) {
    let mut request = None;
    conversation.tick(now, |bytes| {
        let (packet, body) = Packet::decode(bytes).unwrap();
        assert_eq!(packet.operation, op::NEW);
        assert!(body.is_empty());
        request = Some(packet);
        Ok(())
    });
    let mut out = [0; BYTES];
    let n = request
        .unwrap()
        .reply(Status::Ok)
        .encode(&[], &mut out)
        .unwrap();
    assert!(conversation.receive_from(scope().server, &out[..n], now + 1));
}
fn stale(c: &mut Client, scope: Scope, now: u64) {
    let mut query = None;
    c.tick(scope, now, true, |_, bytes| {
        query = Some(Packet::decode(bytes).unwrap().0);
        true
    });
    let mut out = [0; BYTES];
    let n = query
        .unwrap()
        .reply(Status::Stale)
        .encode(&[], &mut out)
        .unwrap();
    assert!(c.receive(scope.server, &out[..n], now + 1));
}
#[test]
fn activation_session_renews_once_and_only_new_is_sent_across_unbound_handshake() {
    let mut c = Client::EMPTY;
    let mut conversation = osaiku_runtime::assistant::Client::EMPTY;
    conversation
        .reset(scope().window, scope().incarnation)
        .unwrap();
    connect(&mut conversation, 1);
    stale(&mut c, scope(), 10);
    assert!(!c.renew_session(&mut conversation, 100));
    assert!(c.renew_session(&mut conversation, 1011));
    assert!(conversation.active());
    assert!(conversation.input.as_str().is_empty());
    assert!(conversation.output.as_str().is_empty());
    c.tick(
        Scope {
            server: conversation.endpoint(),
            session: conversation.session,
            ..scope()
        },
        1011,
        false,
        |_, _| panic!("handshake"),
    );
    connect(&mut conversation, 1012);
    let next = Scope {
        session: conversation.session,
        ..scope()
    };
    stale(&mut c, next, 1020);
    assert!(!c.renew_session(&mut conversation, 2021));
    assert_eq!(c.wake_after_ms(2021), None);
    for now in [2021, 60_000, 400_000] {
        c.tick(next, now, true, |_, _| {
            panic!("exhausted renewal must stay quiet")
        });
    }
    assert!(!conversation.active());
    // New model incarnation gets its own single control-only renewal.
    stale(
        &mut c,
        Scope {
            model: scope().model + (1 << 32),
            ..next
        },
        400_010,
    );
    assert!(c.renew_session(&mut conversation, 401_011));
}
#[test]
fn automatic_renewal_never_replays_active_input_or_responds_to_missing_replies() {
    let mut c = Client::EMPTY;
    let mut conversation = osaiku_runtime::assistant::Client::EMPTY;
    conversation
        .reset(scope().window, scope().incarnation)
        .unwrap();
    connect(&mut conversation, 1);
    stale(&mut c, scope(), 10);
    conversation.begin("hello", false).unwrap();
    assert!(!c.renew_session(&mut conversation, 1011));
    assert_eq!(conversation.input.as_str(), "hello");
    conversation.close(Some(1012), |_| Ok(()));
    c = Client::EMPTY;
    let _ = query(&mut c, 1020);
    c.tick(scope(), 3020, true, |_, _| panic!("expired query"));
    assert_eq!(c.status, Status::ServiceTimeout);
    assert!(!c.renew_session(&mut conversation, 4021));
    c.clock_lost();
    assert!(!c.renew_session(&mut conversation, 5021));
}
#[test]
fn service_loss_allows_explicit_handshake_but_never_replays_a_question() {
    let mut c = Client::EMPTY;
    let p = query(&mut c, 1);
    assert!(c.receive(scope().server, &response(p, State::Ready), 2));
    let mut conversation = osaiku_runtime::assistant::Client::EMPTY;
    conversation
        .reset(scope().window, scope().incarnation)
        .unwrap();
    connect(&mut conversation, 3);
    c.service_lost();
    assert!(c.session_retry_available());
    assert_eq!(c.wake_after_ms(4), None);
    assert!(!c.renew_session(&mut conversation, 4000));
    assert!(!conversation.active());
}
