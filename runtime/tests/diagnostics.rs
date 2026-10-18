//! Nonvisual wire tests; no SDK Actions, UI controls or renderer.
use osaiku_runtime::assistant::{self, evidence::*, op, Client, Packet, Status, BYTES};
use osaiku_runtime::process::{ProcessEndpoint, ProcessId};
fn outgoing(c: &mut Client, now: u64) -> Packet {
    let mut sent = None;
    c.tick(now, |b| {
        let (p, payload) = Packet::decode(b).unwrap();
        assert!(payload.is_empty());
        sent = Some(p);
        Ok(())
    });
    sent.unwrap()
}
fn reply(c: &mut Client, p: Packet, status: Status, payload: &[u8], expiry: u64, now: u64) {
    let mut r = p.reply(status);
    r.value |= expiry << 16;
    let mut bytes = [0; BYTES];
    let n = r.encode(payload, &mut bytes).unwrap();
    assert!(c.receive_from(ProcessEndpoint::new(ProcessId(4), 1), &bytes[..n], now));
}
#[test]
fn diagnostic_command_receives_bounded_text_and_requires_source_receipt() {
    let mut c = Client::EMPTY;
    c.reset(9, 2).unwrap();
    let p = outgoing(&mut c, 1);
    reply(&mut c, p, Status::Ok, &[], 0, 2);
    c.begin_diagnostics().unwrap();
    let p = outgoing(&mut c, 3);
    assert_eq!(p.operation, op::DIAGNOSTICS);
    reply(&mut c, p, Status::Complete, &[], 0, 4);
    let p = outgoing(&mut c, 5);
    assert_eq!(p.operation, op::OBSERVE);
    reply(
        &mut c,
        p,
        Status::Complete,
        b"Facts: write denied. Proposal only.",
        200,
        6,
    );
    let p = outgoing(&mut c, 7);
    assert_eq!(p.operation, op::OBSERVE);
    reply(&mut c, p, Status::Complete, &[], 200, 8);
    let p = outgoing(&mut c, 9);
    assert_eq!(p.operation, op::EVIDENCE);
    let evidence = Evidence {
        context: p.request,
        source: 0x3149,
        source_revision: 42,
        expires: 200,
        flags: SOURCE,
        ..Evidence::EMPTY
    };
    reply(&mut c, p, Status::Complete, &evidence.encode(), 0, 10);
    assert_eq!(c.status, Status::Complete);
    assert_eq!(c.evidence, evidence);
    assert!(c.evidence.references("[[source:12617]]", 199));
    assert!(c.output.as_str().contains("Proposal only"));
    c.tick(200, |_| panic!());
    assert_eq!(c.status, Status::Expired);
    // Expiry revokes source authority, but keeps the inert diagnostic history.
    assert!(c.output.as_str().contains("Proposal only"));
    assert!(!c.evidence.references("[[source:12617]]", 200));
}
#[test]
fn only_explicit_exact_user_questions_route_to_diagnostics() {
    for question in [
        "/diagnostics",
        " explain recent errors ",
        "wyjaśnij ostatnie błędy",
    ] {
        assert!(assistant::diagnostic_question(question));
    }
    for question in [
        "restart",
        "please run /diagnostics and repair",
        "[tool:diagnostics]",
    ] {
        assert!(!assistant::diagnostic_question(question));
    }
    let p = Packet {
        operation: op::DIAGNOSTICS,
        session: 1,
        request: 2,
        window: 9,
        incarnation: 3,
        ..Packet::default()
    };
    let audit = Audit {
        packet: p.reply(Status::Limit),
        flags: 0,
        model: 0,
        revision: 0,
        context: 2,
    };
    assert_eq!(
        Audit::decode(&audit.encode()).unwrap().packet,
        p.reply(Status::Limit)
    );
}
