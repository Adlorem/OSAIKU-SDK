use osaiku_driver_protocol::*;

#[test]
fn standalone_consumer_parses_public_bytes_and_rejects_stale_instance() {
    let scope = Scope::new(1, 2, 3).unwrap();
    let session = Negotiated {
        version: Version {
            major: MAJOR,
            minor: MINOR,
        },
        features: KNOWN_FEATURES,
    };
    let payload = Transfer {
        buffer: ResourceHandle {
            scope,
            id: ResourceId::new(4).unwrap(),
        },
        range: BufferRange {
            offset: 0,
            bytes: 512,
        },
        deadline: 10,
    }
    .encode()
    .unwrap();
    let mut message = [0; MAX_MESSAGE];
    let n = Header {
        kind: Kind::Submit,
        scope,
        request: RequestId::new(1).unwrap(),
        features: DEADLINE,
    }
    .encode(session, &payload, &mut message)
    .unwrap();
    assert_eq!(
        driver_kit_client::receive(scope, session, &message[..n]),
        Ok(1)
    );
    assert_eq!(
        driver_kit_client::receive(Scope::new(1, 2, 4).unwrap(), session, &message[..n]),
        Err(Error::Stale)
    );
}
