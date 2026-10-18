//! IPC/catalog contracts only: no controls, layout or rendering.
use osaiku_runtime::report::transfer::*;
use osaiku_ui::Identity;
const ID: Identity = Identity {
    epoch: 1,
    owner: 7,
    window: 2,
    incarnation: 1,
    revision: 4,
};
#[test]
fn catalog_requires_registered_content_exact_target_and_fresh_request() {
    let mut inbox = Box::new(Inbox::EMPTY);
    let wire = [42u8; 12814];
    let key = osaiku_ui::wire_hash(&wire).to_le_bytes();
    let mut packet = Packet {
        kind: CATALOG,
        request: 1,
        target: ID,
        offset: 0,
        total: wire.len() as u32,
        bytes: &key,
    };
    let encoded = packet.encode().unwrap();
    assert_eq!(Packet::decode(&encoded), Some(packet));
    let rejected = inbox.receive_catalog(packet, ID, &[], |_| {
        panic!("unregistered content reached validator")
    });
    assert_eq!(rejected.status, Status::Rejected);
    packet.request = 2;
    assert_eq!(
        inbox
            .receive_catalog(packet, Identity { revision: 5, ..ID }, &[&wire], |_| true)
            .status,
        Status::Stale
    );
    assert_eq!(
        inbox
            .receive_catalog(packet, ID, &[&wire], |_| false)
            .status,
        Status::Rejected
    );
    packet.request = 3;
    let reply = inbox.receive_catalog(packet, ID, &[&wire], |b| b == wire);
    assert!(reply.matches(packet));
    assert_eq!(inbox.definition(&[]), wire);
    assert!(inbox.staged());
    inbox.commit();
    assert_eq!(
        inbox.receive_catalog(packet, ID, &[&wire], |_| true).status,
        Status::Stale
    );
    assert_eq!(inbox.definition(&[]), wire);
}
#[test]
fn catalog_never_overwrites_an_active_transfer_or_accepts_malformed_keys() {
    let mut inbox = Box::new(Inbox::EMPTY);
    let wire = [8; 3];
    let key = osaiku_ui::wire_hash(&wire).to_le_bytes();
    let begin = Packet {
        kind: BEGIN,
        request: 1,
        target: ID,
        offset: 0,
        total: 3,
        bytes: &[],
    };
    assert_eq!(inbox.receive(begin, ID, |_| true).status, Status::Receiving);
    let mut p = Packet {
        kind: CATALOG,
        request: 2,
        bytes: &key,
        ..begin
    };
    assert_eq!(
        inbox.receive_catalog(p, ID, &[&wire], |_| true).status,
        Status::Busy
    );
    p.bytes = &[0; 7];
    assert!(p.encode().is_none());
    p.bytes = &key;
    p.offset = 1;
    assert!(p.encode().is_none());
}
