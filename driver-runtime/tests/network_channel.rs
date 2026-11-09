use osaiku_driver_runtime::{
    network::{Entry, COMPLETED},
    network_channel::*,
    protocol::Error,
};
use std::alloc::{alloc_zeroed, dealloc, Layout};
struct Object(*mut u8);
impl Object {
    fn layout() -> Layout {
        Layout::from_size_align(PAGES as usize * 4096, 4096).unwrap()
    }
    fn new() -> Self {
        let p = unsafe { alloc_zeroed(Self::layout()) };
        assert!(!p.is_null());
        Self(p)
    }
    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0, Self::layout().size()) }
    }
}
impl Drop for Object {
    fn drop(&mut self) {
        unsafe { dealloc(self.0, Self::layout()) };
    }
}
#[test]
fn broker_layout_pairs_all_four_rings_and_only_writes_own_object() {
    let d = Object::new();
    let c = Object::new();
    let receipt = Wire {
        id: 7,
        generation: 3,
        own: d.0 as u64,
        peer: c.0 as u64,
        ..Wire::new(op::ATTACH)
    };
    let mut driver = unsafe { rings(receipt, true).unwrap() };
    let mut consumer = unsafe {
        rings(
            Wire {
                own: receipt.peer,
                peer: receipt.own,
                ..receipt
            },
            false,
        )
        .unwrap()
    };
    for i in 0..4 {
        let (producer, reader, peer, own) = if matches!(i, 0 | 3) {
            (&mut driver[i], &mut consumer[i], &c, &d)
        } else {
            (&mut consumer[i], &mut driver[i], &d, &c)
        };
        let peer_before = peer.bytes().to_vec();
        for slot in 0..64 {
            producer
                .push(Entry {
                    slot,
                    generation: 3,
                    length: 64,
                    status: COMPLETED,
                })
                .unwrap();
        }
        assert_eq!(peer.bytes(), peer_before);
        assert_eq!(producer.available().unwrap(), 0);
        let own_before = own.bytes().to_vec();
        for slot in 0..64 {
            let (bytes, _) = reader.pop().unwrap();
            assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), slot);
        }
        assert_eq!(own.bytes(), own_before);
        assert_eq!(producer.available().unwrap(), 64);
    }
}
#[test]
fn wire_rejects_reserved_bytes_unknown_operations_and_generation_overflow() {
    let w = Wire {
        generation: 1,
        ..Wire::new(op::OPEN)
    };
    assert_eq!(Wire::decode(&w.encode()), Ok(w));
    // Byte 75 now identifies Ethernet (0) or Wi-Fi (1); padding starts at 76.
    for medium in [0, 1] {
        let receipt = Wire { medium, ..w };
        assert_eq!(receipt.encode()[75], medium);
        assert_eq!(Wire::decode(&receipt.encode()), Ok(receipt));
    }
    for at in [74, 75] {
        for value in [2, 255] {
            let mut b = w.encode();
            b[at] = value;
            assert_eq!(Wire::decode(&b), Err(Error::Reserved));
        }
    }
    for at in 76..BYTES {
        let mut b = w.encode();
        b[at] = 1;
        assert_eq!(Wire::decode(&b), Err(Error::Reserved));
    }
    let mut b = w.encode();
    b[8] = 7;
    assert_eq!(Wire::decode(&b), Err(Error::Kind));
    assert_eq!(
        Wire {
            generation: u32::MAX as u64 + 1,
            ..w
        }
        .geometry(),
        Err(Error::Range)
    );
}
