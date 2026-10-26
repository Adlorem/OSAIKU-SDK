use osaiku_driver_protocol::*;

#[test]
fn cancel_and_submit_cannot_bypass_feature_negotiation() {
    let mut p = packet();
    p[8] = Kind::Cancel as u8;
    assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Features));
    p[56] = CANCEL as u8;
    assert!(Header::decode(session(), scope(), &p).is_ok());
    p = packet();
    p[56] = 0;
    assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Features));
}

#[test]
fn transfer_checks_scope_length_overflow_and_deadline() {
    let t = Transfer {
        buffer: ResourceHandle {
            scope: scope(),
            id: ResourceId::new(1).unwrap(),
        },
        range: BufferRange {
            offset: 16,
            bytes: 32,
        },
        deadline: 10,
    };
    let bytes = t.encode().unwrap();
    assert_eq!(Transfer::decode(scope(), &bytes), Ok(t));
    assert_eq!(
        Transfer::decode(Scope::new(7, 12, 13).unwrap(), &bytes),
        Err(Error::Stale)
    );
    assert_eq!(Transfer::decode(scope(), &bytes[..55]), Err(Error::Size));
    for bad in [
        Transfer { deadline: 0, ..t },
        Transfer {
            range: BufferRange {
                offset: u64::MAX,
                bytes: 2,
            },
            ..t
        },
        Transfer {
            range: BufferRange {
                offset: 0,
                bytes: MAX_BUFFER + 1,
            },
            ..t
        },
    ] {
        assert!(bad.encode().is_err());
    }
}

#[test]
fn completion_preserves_uncertain_write_and_rejects_unknown_status() {
    let c = Completion {
        actual: 4096,
        outcome: Outcome::UncertainWrite,
    };
    assert_eq!(Completion::decode(&c.encode()), Ok(c));
    let mut bytes = c.encode();
    bytes[8] = 255;
    assert_eq!(Completion::decode(&bytes), Err(Error::Kind));
    bytes = c.encode();
    bytes[15] = 1;
    assert_eq!(Completion::decode(&bytes), Err(Error::Reserved));
}

fn scope() -> Scope {
    Scope::new(7, 11, 13).unwrap()
}
fn offer() -> Offer {
    Offer {
        version: Version {
            major: MAJOR,
            minor: MINOR,
        },
        supported: KNOWN_FEATURES,
        required: DEADLINE,
    }
}
fn session() -> Negotiated {
    negotiate(offer(), offer()).unwrap()
}
fn packet() -> Vec<u8> {
    let mut out = vec![0; MAX_MESSAGE];
    let n = Header {
        kind: Kind::Submit,
        scope: scope(),
        request: RequestId::new(17).unwrap(),
        features: DEADLINE,
    }
    .encode(session(), b"abc", &mut out)
    .unwrap();
    out.truncate(n);
    out
}

#[test]
fn fixed_little_endian_vector_and_round_trip() {
    let p = packet();
    assert_eq!(
        &p[..16],
        &[79, 68, 82, 86, 1, 0, 0, 0, 2, 0, 0, 0, 67, 0, 0, 0]
    );
    assert_eq!(&p[16..24], &7u64.to_le_bytes());
    assert_eq!(&p[40..48], &17u64.to_le_bytes());
    let (h, payload) = Header::decode(session(), scope(), &p).unwrap();
    assert_eq!(h.kind, Kind::Submit);
    assert_eq!(payload, b"abc");
}
#[test]
fn every_truncation_and_trailing_data_is_rejected() {
    let p = packet();
    for n in 0..p.len() {
        assert!(Header::decode(session(), scope(), &p[..n]).is_err());
    }
    let mut extra = p;
    extra.push(0);
    assert_eq!(Header::decode(session(), scope(), &extra), Err(Error::Size));
}
#[test]
fn reserved_flags_and_unknown_kind_fail_closed() {
    for at in [10, 11, 52, 53, 54, 55] {
        let mut p = packet();
        p[at] = 1;
        assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Reserved));
    }
    let mut p = packet();
    p[8] = 99;
    assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Kind));
}
#[test]
fn malformed_lengths_and_unknown_version_are_rejected() {
    for at in [12, 13, 14, 15, 48, 49, 50, 51] {
        let mut p = packet();
        p[at] ^= 255;
        assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Size));
    }
    for at in [0, 4, 6] {
        let mut p = packet();
        p[at] ^= 1;
        assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Version));
    }
}
#[test]
fn zero_and_stale_identities_are_not_authority() {
    for at in [16, 24, 32, 40] {
        let mut p = packet();
        p[at..at + 8].fill(0);
        assert_eq!(
            Header::decode(session(), scope(), &p),
            Err(Error::InvalidId)
        );
    }
    for s in [
        Scope::new(8, 11, 13),
        Scope::new(7, 12, 13),
        Scope::new(7, 11, 14),
    ] {
        assert_eq!(
            Header::decode(session(), s.unwrap(), &packet()),
            Err(Error::Stale)
        );
    }
}
#[test]
fn unknown_optional_features_are_ignored_but_required_features_refused() {
    let mut peer = offer();
    peer.supported |= 1 << 63;
    assert_eq!(negotiate(offer(), peer).unwrap().features, KNOWN_FEATURES);
    peer.required |= 1 << 63;
    assert_eq!(negotiate(offer(), peer), Err(Error::Features));
    peer = offer();
    peer.supported = 0;
    assert_eq!(negotiate(offer(), peer), Err(Error::Features));
}
#[test]
fn newer_minor_downgrades_and_other_major_refuses() {
    let mut peer = offer();
    peer.version.minor = 7;
    assert_eq!(negotiate(offer(), peer).unwrap().version.minor, 0);
    peer.version.major = 2;
    assert_eq!(negotiate(offer(), peer), Err(Error::Version));
}
#[test]
fn handshake_has_exact_size_and_reserved_policy() {
    let bytes = offer().encode();
    assert_eq!(Offer::decode(&bytes), Ok(offer()));
    for n in 0..32 {
        assert_eq!(Offer::decode(&bytes[..n]), Err(Error::Size));
    }
    let mut bytes = bytes;
    bytes[31] = 1;
    assert_eq!(Offer::decode(&bytes), Err(Error::Reserved));
}
#[test]
fn unnegotiated_message_feature_is_rejected() {
    let mut p = packet();
    p[63] = 128;
    assert_eq!(Header::decode(session(), scope(), &p), Err(Error::Features));
    let mut s = session();
    s.features = 0;
    assert_eq!(Header::decode(s, scope(), &packet()), Err(Error::Features));
}
#[test]
fn encoder_refuses_oversize_before_mutation() {
    let (h, _) = Header::decode(session(), scope(), &packet()).unwrap();
    let mut out = [0x55; MAX_MESSAGE];
    assert_eq!(
        h.encode(session(), &[0; MAX_PAYLOAD + 1], &mut out),
        Err(Error::Size)
    );
    assert!(out.iter().all(|b| *b == 0x55));
    assert_eq!(
        h.encode(session(), b"abc", &mut out[..HEADER_BYTES]),
        Err(Error::Size)
    );
}
#[test]
fn sequence_exhaustion_is_permanent() {
    let mut s = Sequence::after(u64::MAX - 1);
    assert_eq!(s.issue(), Ok(u64::MAX));
    assert_eq!(s.issue(), Err(Error::Exhausted));
    assert_eq!(s.issue(), Err(Error::Exhausted));
}
#[test]
fn resource_handle_wire_and_overflow_safe_ranges() {
    let h = ResourceHandle {
        scope: scope(),
        id: ResourceId::new(9).unwrap(),
    };
    assert_eq!(ResourceHandle::decode(&h.encode()), Ok(h));
    assert_eq!(ResourceHandle::decode(&[0; 32]), Err(Error::InvalidId));
    assert_eq!(ResourceHandle::decode(&[0; 31]), Err(Error::Size));
    assert_eq!(
        BufferRange {
            offset: u64::MAX,
            bytes: 2
        }
        .validate(u64::MAX),
        Err(Error::Range)
    );
    assert_eq!(
        BufferRange {
            offset: 0,
            bytes: 0
        }
        .validate(4096),
        Err(Error::Range)
    );
    assert_eq!(
        BufferRange {
            offset: 4095,
            bytes: 1
        }
        .validate(4096),
        Ok(())
    );
}
