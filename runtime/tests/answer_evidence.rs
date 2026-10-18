use osaiku_runtime::assistant::{evidence::*, op, Client, Packet, Status, BYTES as WIRE};
use osaiku_runtime::process::{ProcessEndpoint, ProcessId};
#[test]
fn foreign_metadata_does_not_consume_the_outstanding_request() {
    let (mut c, p) = final_client();
    let e = Evidence {
        context: p.request,
        ..Evidence::EMPTY
    };
    let mut out = [0; WIRE];
    for foreign in [
        Packet {
            session: p.session + 1,
            ..p
        },
        Packet {
            incarnation: p.incarnation + 1,
            ..p
        },
        Packet {
            request: p.request + 1,
            ..p
        },
        Packet {
            window: p.window + 1,
            ..p
        },
    ] {
        let n = foreign
            .reply(Status::Complete)
            .encode(&e.encode(), &mut out)
            .unwrap();
        assert!(!c.receive_from(ProcessEndpoint::new(ProcessId(4), 1), &out[..n], 10));
        assert!(c.active());
    }
    reply(&mut c, p, Status::Complete, &e.encode(), 11);
    assert_eq!(c.status, Status::Complete);
}
#[test]
fn extra_packet_expiry_is_rejected_and_expired_source_cannot_complete() {
    let (mut c, p) = final_client();
    let e = Evidence {
        context: p.request,
        ..Evidence::EMPTY
    };
    let mut out = [0; WIRE];
    let mut r = p.reply(Status::Complete);
    r.value |= 200 << 16;
    let n = r.encode(&e.encode(), &mut out).unwrap();
    assert!(c.receive_from(ProcessEndpoint::new(ProcessId(4), 1), &out[..n], 10));
    assert_eq!(c.status, Status::Invalid);
    assert!(!c.output.as_str().is_empty()); // inert historical text; status/evidence still refuse authority
    let (mut c, p) = final_client();
    reply(&mut c, p, Status::Complete, &source().encode(), 200);
    assert_eq!(c.status, Status::Expired);
    assert!(!c.output.as_str().is_empty()); // inert historical text; status/evidence still refuse authority
}
fn source() -> Evidence {
    Evidence {
        context: 2,
        source: 1,
        source_revision: 7,
        expires: 200,
        flags: SOURCE,
        ..Evidence::EMPTY
    }
}
#[test]
fn provenance_rejects_unknown_versions_flags_and_inconsistent_identity() {
    let e = source();
    assert_eq!(Evidence::decode(&e.encode()), Some(e));
    for n in 0..BYTES {
        assert!(Evidence::decode(&e.encode()[..n]).is_none());
    }
    for bad in [
        Evidence { context: 0, ..e },
        Evidence { flags: 128, ..e },
        Evidence {
            source_revision: 0,
            ..e
        },
        Evidence {
            flags: UNSOURCED,
            ..e
        },
        Evidence {
            flags: SOURCE | MODEL,
            ..e
        },
    ] {
        assert!(Evidence::decode(&bad.encode()).is_none());
    }
    let mut b = e.encode();
    b[88] = 2;
    assert!(Evidence::decode(&b).is_none());
}
#[test]
fn model_identity_is_exact_and_independent_from_source_completeness() {
    let e = Evidence {
        model_sha: [3; 32],
        model: 0x100000020,
        model_revision: 7,
        context: 9,
        flags: MODEL | UNSOURCED | TRUNCATED,
        ..Evidence::EMPTY
    };
    assert_eq!(Evidence::decode(&e.encode()), Some(e));
    assert!(!e.references("[[source:1]]", 1));
    assert!(e.references("I am 99% confident", 1)); // Plain text gains no source status.
}
#[test]
fn citations_cannot_expand_scope_or_survive_expiry() {
    let e = source();
    assert!(e.references("a [[source:1]] b [[source:1]]", 199));
    for text in [
        "[[source:2]]",
        "[[source:01]]",
        "[[source:]]",
        "[[source:1]",
        "[[source:18446744073709551616]]",
        "[[source:1]] [[source:3]]",
    ] {
        assert!(!e.references(text, 100), "{text}");
    }
    assert!(!e.references("[[source:1]]", 200));
    for flag in [STALE, CONFLICT, TRUNCATED, MISSING] {
        assert!(!Evidence {
            flags: SOURCE | flag,
            ..e
        }
        .references("[[source:1]]", 100));
    }
}
fn request(c: &mut Client, now: u64) -> Packet {
    let mut packet = None;
    c.tick(now, |b| {
        packet = Some(Packet::decode(b).unwrap().0);
        Ok(())
    });
    packet.unwrap()
}
fn reply(c: &mut Client, p: Packet, status: Status, payload: &[u8], now: u64) {
    let mut b = [0; WIRE];
    let n = p.reply(status).encode(payload, &mut b).unwrap();
    assert!(c.receive_from(ProcessEndpoint::new(ProcessId(4), 1), &b[..n], now));
}
fn final_client() -> (Client, Packet) {
    let mut c = Client::EMPTY;
    c.reset(9, 3).unwrap();
    let p = request(&mut c, 1);
    reply(&mut c, p, Status::Ok, &[], 2);
    c.begin("hi", false).unwrap();
    let p = request(&mut c, 3);
    reply(&mut c, p, Status::Pending, &[], 4);
    let p = request(&mut c, 5);
    reply(&mut c, p, Status::Complete, b"text", 6);
    let p = request(&mut c, 7);
    reply(&mut c, p, Status::Complete, &[], 8);
    assert!(c.active());
    assert_ne!(c.status, Status::Complete);
    let p = request(&mut c, 9);
    assert_eq!(p.operation, op::EVIDENCE);
    (c, p)
}
#[test]
fn client_does_not_complete_until_correlated_provenance_arrives() {
    let (mut c, p) = final_client();
    let e = Evidence {
        context: p.request,
        ..Evidence::EMPTY
    };
    reply(&mut c, p, Status::Complete, &e.encode(), 10);
    assert_eq!(c.status, Status::Complete);
    assert_eq!(c.evidence, e);
    assert_eq!(c.output.as_str(), "text");
    c.begin("next", false).unwrap();
    assert_eq!(c.evidence, Evidence::EMPTY);
}
#[test]
fn changed_context_or_unmatched_source_deadline_clears_output() {
    for e in [
        Evidence {
            context: 999,
            ..Evidence::EMPTY
        },
        source(),
    ] {
        let (mut c, p) = final_client();
        reply(&mut c, p, Status::Complete, &e.encode(), 10);
        assert_eq!(c.status, Status::Invalid);
        assert!(!c.output.as_str().is_empty()); // inert historical text; status/evidence still refuse authority
        assert_ne!(c.evidence.flags & CONFLICT, 0);
    }
}
#[test]
fn missing_metadata_times_out_without_promoting_model_text() {
    let (mut c, _) = final_client();
    c.tick(2010, |_| panic!());
    assert_eq!(c.status, Status::ServiceTimeout);
    assert!(!c.output.as_str().is_empty()); // inert historical text; status/evidence still refuse authority
}
#[test]
fn audit_wire_is_fixed_numeric_metadata_with_no_content_field() {
    let p = Packet {
        operation: op::EVIDENCE,
        session: 1,
        request: 2,
        window: 9,
        incarnation: 3,
        offset: 0,
        value: 0,
    }
    .reply(Status::Complete);
    let a = Audit {
        packet: p,
        flags: UNSOURCED,
        model: 0,
        revision: 0,
        context: 2,
    };
    let b = a.encode();
    let decoded = Audit::decode(&b).unwrap();
    assert_eq!(decoded.packet, p);
    assert_eq!(decoded.context, 2);
    let mut extended = b.to_vec();
    extended.extend_from_slice(b"private prompt");
    assert!(Audit::decode(&extended).is_none());
    for at in [0, 40, 48, 56, 88] {
        let mut bad = b;
        bad[at] = 255;
        assert!(Audit::decode(&bad).is_none());
    }
}
