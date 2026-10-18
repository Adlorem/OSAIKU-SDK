use osaiku_runtime::{
    application::{ApplicationHost, ApplicationMessage},
    process::{ProcessEndpoint, ProcessId},
    report::{transfer::*, ReportView, Surface},
};
use osaiku_ui::{
    compiler::compile,
    report::{host::Error, parse_manifest},
    *,
};
const PRODUCER: ProcessEndpoint = ProcessEndpoint::new(ProcessId(1), 1);
const RECEIVER: ProcessEndpoint = ProcessEndpoint::new(ProcessId(19), 4);
const HOST: ApplicationHost = ApplicationHost::FixedService(ProcessId(1));
const ID: Identity = Identity {
    epoch: 2,
    owner: RECEIVER.encode(),
    window: 6,
    incarnation: 3,
    revision: 7,
};
fn packet(kind: u16, request: u64, total: u32) -> Packet<'static> {
    Packet {
        kind,
        request,
        target: ID,
        offset: if kind == FINISH { total } else { 0 },
        total,
        bytes: &[],
    }
}
fn wire(markup: &str) -> Vec<u8> {
    let doc = compile(markup, "").unwrap();
    let mut b = vec![0; MAX_WIRE];
    let n = doc.encode(&mut b).unwrap();
    b.truncate(n);
    b
}

#[test]
fn codecs_refuse_malformed_lengths_padding_and_never_alias_lifecycle_commands() {
    for kind in [BEGIN, CHUNK, FINISH, CANCEL] {
        let mut p = packet(kind, 1, 5);
        if kind == CHUNK {
            p.bytes = b"abcde";
        }
        let b = p.encode().unwrap();
        assert_eq!(Packet::decode(&b), Some(p));
        assert_eq!(
            HOST.classify(PRODUCER, &b),
            Ok(ApplicationMessage::Other(&b))
        );
        assert!(HOST.classify(RECEIVER, &b).is_err());
        for len in 0..b.len() {
            assert!(Packet::decode(&b[..len]).is_none());
        }
        for offset in [0, 8, 9, 12, 15] {
            let mut bad = b;
            bad[offset] ^= 128;
            assert!(Packet::decode(&bad).is_none());
        }
        if kind != CHUNK {
            let mut bad = b;
            bad[151] = 1;
            assert!(Packet::decode(&bad).is_none());
        }
    }
    for total in [0, u32::MAX, MAX_WIRE as u32 + 1] {
        assert!(packet(BEGIN, 1, total).encode().is_none());
    }
    let mut p = packet(CHUNK, 1, 5);
    p.offset = u32::MAX;
    p.bytes = b"abc";
    assert!(p.encode().is_none());
    assert!(!Reply::for_packet(packet(BEGIN, 1, 5), Status::Receiving, 0).matches(p));
}

#[test]
fn fragmented_upload_requires_exact_sender_offsets_and_complete_data() {
    let mut inbox = Box::new(Inbox::EMPTY);
    let bytes = vec![42; CHUNK_BYTES * 2 + 1];
    let mut upload = Upload::new(ID, 1, &bytes).unwrap();
    while !upload.staged() {
        let p = upload.packet().unwrap();
        let reply = inbox.receive(p, ID, |b| b == bytes);
        let before = upload.packet().unwrap();
        assert_eq!(
            upload.acknowledge(PRODUCER, &reply.encode()),
            Err(UploadError::Sender)
        );
        assert_eq!(upload.packet().unwrap(), before);
        upload.acknowledge(RECEIVER, &reply.encode()).unwrap();
        if p.kind == CHUNK {
            assert_eq!(inbox.receive(p, ID, |_| panic!()).status, Status::Stale);
        }
    }
    assert_eq!(inbox.definition(b"old"), bytes);
    assert!(inbox.staged());
    assert_eq!(
        inbox.receive(packet(BEGIN, 2, 1), ID, |_| panic!()).status,
        Status::Busy
    );
    inbox.commit();
    assert!(!inbox.staged());
    assert_eq!(inbox.definition(b"old"), bytes);
    assert_eq!(
        inbox.receive(packet(BEGIN, 1, 1), ID, |_| panic!()).status,
        Status::Stale
    );
    assert_eq!(
        inbox.receive(packet(BEGIN, 2, 5), ID, |_| panic!()).status,
        Status::Receiving
    );
    assert_eq!(
        inbox.receive(packet(FINISH, 2, 5), ID, |_| panic!()).status,
        Status::Stale
    );
    let mut wrong = packet(CANCEL, 2, 5);
    wrong.target.incarnation += 1;
    assert_eq!(inbox.receive(wrong, ID, |_| panic!()).status, Status::Stale);
    assert_eq!(
        inbox.receive(packet(CANCEL, 2, 5), ID, |_| panic!()).status,
        Status::Cancelled
    );
    assert_eq!(inbox.definition(b"old"), bytes);
}

#[test]
fn rejected_wire_and_intervening_publication_preserve_the_accepted_definition() {
    let mut inbox = Box::new(Inbox::EMPTY);
    let run = |inbox: &mut Inbox, id, valid| {
        let mut upload = Upload::new(ID, id, b"new").unwrap();
        loop {
            let p = upload.packet().unwrap();
            let reply = inbox.receive(p, ID, |_| valid);
            if p.kind == FINISH {
                return reply.status;
            }
            upload.acknowledge(RECEIVER, &reply.encode()).unwrap();
        }
    };
    assert_eq!(run(&mut inbox, 1, false), Status::Rejected);
    assert_eq!(inbox.definition(b"fallback"), b"fallback");
    assert_eq!(run(&mut inbox, 2, true), Status::Staged);
    inbox.discard();
    assert_eq!(inbox.definition(b"fallback"), b"fallback");
    assert_eq!(run(&mut inbox, 3, true), Status::Staged);
    inbox.commit();
    assert_eq!(inbox.definition(b"fallback"), b"new");
    inbox.receive(packet(BEGIN, 4, 5), ID, |_| panic!());
    inbox.commit(); // A normal publication invalidates a partial transfer.
    assert_eq!(
        inbox.receive(packet(FINISH, 4, 5), ID, |_| panic!()).status,
        Status::Stale
    );
    inbox.close();
    assert_eq!(inbox.definition(b"fallback"), b"fallback");
}

#[test]
fn runtime_replacement_is_contract_checked_authenticated_and_transactional() {
    static VIEW: ReportView = ReportView::new();
    let s = VIEW.open(ID.owner, 1, Surface::Window).unwrap();
    let registration =
        parse_manifest("report 1 10\nsource local record 1\nfield value local text").unwrap();
    let contract = registration.contract();
    let original = wire(r#"<p id="root">original</p>"#);
    let changed = wire(r#"<p id="root" bind:text="value"></p>"#);
    let unknown = wire(r#"<p id="root" bind:text="private"></p>"#);
    struct Model;
    impl Bindings for Model {
        fn get(&self, key: u32) -> Option<Value<'_>> {
            assert_eq!(key, symbol("value"));
            Some(Value::Text("bound externally"))
        }
    }
    let mut output = vec![0; MAX_WIRE];
    let (token, _) = VIEW
        .prepare(s, 0, &contract, &original, &Model, None, &mut output)
        .unwrap();
    VIEW.commit(token, ID).unwrap();
    let upload = |request, definition: &[u8]| {
        let mut sender = Upload::new(ID, request, definition).unwrap();
        loop {
            let p = sender.packet().unwrap();
            let b = p.encode().unwrap();
            assert_eq!(
                VIEW.receive_definition(s, HOST, RECEIVER, &contract, &b),
                Err(Error::Owner)
            );
            let reply = VIEW
                .receive_definition(s, HOST, PRODUCER, &contract, &b)
                .unwrap()
                .unwrap();
            if p.kind == FINISH {
                return reply.status;
            }
            sender.acknowledge(RECEIVER, &reply.encode()).unwrap();
        }
    };
    assert_eq!(upload(1, &unknown), Status::Rejected);
    assert_eq!(upload(2, &changed), Status::Staged);
    let (token, n) = VIEW
        .prepare(s, 1, &contract, &original, &Model, None, &mut output)
        .unwrap();
    assert_eq!(
        Document::decode(&output[..n]).unwrap().text_of(0),
        "bound externally"
    );
    let cancel = packet(CANCEL, 2, changed.len() as u32).encode().unwrap();
    assert_eq!(
        VIEW.receive_definition(s, HOST, PRODUCER, &contract, &cancel)
            .unwrap()
            .unwrap()
            .status,
        Status::Busy
    );
    VIEW.discard(token).unwrap();
    let (token, n) = VIEW
        .prepare(s, 1, &contract, &original, &Model, None, &mut output)
        .unwrap();
    assert_eq!(
        Document::decode(&output[..n]).unwrap().text_of(0),
        "original"
    );
    VIEW.discard(token).unwrap();
    assert_eq!(upload(3, &changed), Status::Staged);
    assert!(VIEW
        .prepare(s, 1, &contract, &original, &Model, None, &mut output[..1])
        .is_err());
    assert_eq!(upload(4, &changed), Status::Staged);
    let (token, _) = VIEW
        .prepare(s, 1, &contract, &original, &Model, None, &mut output)
        .unwrap();
    let next = Identity {
        revision: ID.revision + 1,
        ..ID
    };
    VIEW.commit(token, next).unwrap();
    let stale = packet(BEGIN, 5, changed.len() as u32).encode().unwrap();
    assert_eq!(
        VIEW.receive_definition(s, HOST, PRODUCER, &contract, &stale)
            .unwrap()
            .unwrap()
            .status,
        Status::Stale
    );
    let (token, n) = VIEW
        .prepare(s, 2, &contract, &original, &Model, None, &mut output)
        .unwrap();
    assert_eq!(
        Document::decode(&output[..n]).unwrap().text_of(0),
        "bound externally"
    );
    VIEW.discard(token).unwrap();
    VIEW.close(s).unwrap();
    let s = VIEW.open(ID.owner, 1, Surface::Window).unwrap();
    let (token, n) = VIEW
        .prepare(s, 0, &contract, &original, &Model, None, &mut output)
        .unwrap();
    assert_eq!(
        Document::decode(&output[..n]).unwrap().text_of(0),
        "original"
    );
    VIEW.discard(token).unwrap();
    VIEW.close(s).unwrap();
}
