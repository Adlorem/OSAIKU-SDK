use osaiku_gpu_runtime::{
    channel_wire::Wire,
    protocol::channel::{op, Message, BYTES, HEADER},
};

#[test]
fn persistent_wire_clears_previous_payload_before_every_request() {
    let mut wire = Wire::EMPTY;
    let mut payload = vec![0xa5; BYTES - HEADER];
    let request = Message {
        length: payload.len() as u64,
        ..Message::new(op::SUBMIT)
    };
    wire.exchange(request, &mut payload, |bytes| {
        assert!(bytes[HEADER..].iter().all(|b| *b == 0xa5));
        0
    })
    .unwrap();
    wire.exchange(Message::new(op::POLL), &mut [], |bytes| {
        assert!(bytes[HEADER..].iter().all(|b| *b == 0));
        0
    })
    .unwrap();
}

#[test]
fn refused_and_malformed_replies_cannot_copy_untrusted_bytes() {
    let mut wire = Wire::EMPTY;
    let mut out = [0x5a; 4];
    assert_eq!(
        wire.exchange(Message::new(op::TAKE), &mut out, |bytes| {
            bytes.fill(0);
            -12
        }),
        Err(-12)
    );
    assert_eq!(out, [0x5a; 4]);
    assert_eq!(
        wire.exchange(Message::new(op::TAKE), &mut out, |bytes| {
            let m = Message {
                length: 5,
                ..Message::new(op::TAKE)
            };
            m.encode(&mut bytes[..HEADER]).unwrap();
            bytes[HEADER..HEADER + 5].fill(9);
            0
        }),
        Err(-5)
    );
    assert_eq!(out, [0x5a; 4]);
    assert_eq!(
        wire.exchange(Message::new(op::POLL), &mut [], |bytes| {
            bytes[0] = 0;
            0
        }),
        Err(-5)
    );
}

#[test]
fn invalid_submit_never_enters_trap_and_take_copies_only_reported_length() {
    let mut wire = Wire::EMPTY;
    assert_eq!(
        wire.exchange(
            Message {
                length: 3,
                ..Message::new(op::SUBMIT)
            },
            &mut [0; 2],
            |_| panic!("invalid request")
        ),
        Err(-1)
    );
    let mut out = [0x5a; 8];
    wire.exchange(Message::new(op::TAKE), &mut out, |bytes| {
        Message {
            length: 3,
            ..Message::new(op::TAKE)
        }
        .encode(&mut bytes[..HEADER])
        .unwrap();
        bytes[HEADER..HEADER + 3].copy_from_slice(&[1, 2, 3]);
        0
    })
    .unwrap();
    assert_eq!(out, [1, 2, 3, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a]);
}
