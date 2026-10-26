use osaiku_driver_runtime::{
    controller_io::Platform,
    nvme_io::{Gate, Registers, DMA_BYTES},
    nvme_process::*,
    process::{Instance, Message, Role},
    protocol::{Error, Scope},
};
fn request() -> Request {
    Request {
        generation: 9,
        sequence: 1,
        namespace: 1,
        operation: WRITE,
        lba: 8,
        bytes: 512,
        buffer: 4096,
    }
}
#[test]
fn wire_rejects_reserved_overflow_and_control_payloads() {
    let r = request();
    assert_eq!(Request::decode(&r.encode()), Ok(r));
    let mut b = r.encode();
    b[63] = 1;
    assert_eq!(Request::decode(&b), Err(Error::Reserved));
    for r in [
        Request { lba: u64::MAX, ..r },
        Request { bytes: 513, ..r },
        Request { bytes: 262656, ..r },
        Request { buffer: 0, ..r },
        Request {
            operation: FLUSH,
            ..r
        },
        Request { namespace: 0, ..r },
    ] {
        assert!(Request::decode(&r.encode()).is_err());
    }
    let mut b = Reply {
        generation: 9,
        sequence: 1,
        bytes: 512,
        failed: false,
    }
    .encode();
    b[24] = 2;
    assert_eq!(Reply::decode(&b), Err(Error::Reserved));
}
#[test]
fn receipts_require_exact_generation_sequence_length_and_dispatch() {
    let mut c = Channel::new(9);
    let id = c.submit(1, READ, 8, 512, 4096).unwrap();
    let r = Reply {
        generation: 9,
        sequence: id,
        bytes: 512,
        failed: false,
    };
    assert_eq!(c.complete(r), Err(Error::Stale));
    assert_eq!(c.submit(1, FLUSH, 0, 0, 0), Err(Error::Backpressure));
    c.fetch().unwrap();
    assert!(c.fetch().is_none());
    assert_eq!(c.take(id), Ok(None));
    for bad in [
        Reply { generation: 8, ..r },
        Reply {
            sequence: id + 1,
            ..r
        },
        Reply { bytes: 0, ..r },
    ] {
        assert_eq!(c.complete(bad), Err(Error::Stale));
    }
    c.complete(r).unwrap();
    assert_eq!(c.complete(r), Err(Error::Stale));
    assert!(!c.idle());
    assert_eq!(c.take(id), Ok(Some(r)));
    assert!(c.clean());
    assert_eq!(c.take(id), Err(Error::Stale));
}
#[test]
fn write_stays_dirty_until_successful_flush_and_receipt_consumption() {
    let mut c = Channel::new(9);
    let id = c.submit(1, WRITE, 8, 512, 4096).unwrap();
    c.fetch();
    c.complete(Reply {
        generation: 9,
        sequence: id,
        bytes: 512,
        failed: false,
    })
    .unwrap();
    c.take(id).unwrap();
    assert!(c.idle());
    assert!(!c.clean());
    let id = c.submit(1, FLUSH, 0, 0, 0).unwrap();
    c.fetch();
    c.complete(Reply {
        generation: 9,
        sequence: id,
        bytes: 0,
        failed: false,
    })
    .unwrap();
    assert!(!c.clean());
    c.take(id).unwrap();
    assert!(c.clean());
}
#[test]
fn crash_and_failed_completion_never_replay_or_release_an_uncertain_write() {
    for failed in [false, true] {
        let mut c = Channel::new(9);
        let id = c.submit(1, WRITE, 8, 512, 4096).unwrap();
        c.fetch();
        if failed {
            assert_eq!(
                c.complete(Reply {
                    generation: 9,
                    sequence: id,
                    bytes: 0,
                    failed: true
                }),
                Err(Error::State)
            );
        } else {
            c.fail();
        }
        assert!(!c.active());
        assert!(!c.idle());
        assert!(!c.clean());
        assert!(c.fetch().is_none());
        assert_eq!(c.take(id), Err(Error::State));
        assert_eq!(c.submit(1, READ, 8, 512, 4096), Err(Error::State));
    }
}
struct Hardware {
    memory: Vec<u8>,
}
impl Platform for Hardware {
    fn read_register(&mut self, _: u32) -> Result<u32, Error> {
        Ok(123)
    }
    fn write_register(&mut self, _: u32, _: u32) -> Result<(), Error> {
        Ok(())
    }
    fn read_arena(&mut self, o: u64, b: &mut [u8]) -> Result<(), Error> {
        b.copy_from_slice(&self.memory[o as usize..o as usize + b.len()]);
        Ok(())
    }
    fn write_arena(&mut self, o: u64, b: &[u8]) -> Result<(), Error> {
        self.memory[o as usize..o as usize + b.len()].copy_from_slice(b);
        Ok(())
    }
    fn fence(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn now(&mut self) -> u64 {
        1234
    }
    fn revoke(&mut self) -> bool {
        false
    }
}
fn gate(forbidden: [Option<(u32, u32)>; 2]) -> Gate<Hardware> {
    Gate::new(
        Hardware {
            memory: vec![0; DMA_BYTES as usize],
        },
        Registers {
            aperture: 0x4000,
            stride: 4,
            forbidden,
        },
        4096,
        DMA_BYTES,
        u64::MAX,
        1,
    )
    .unwrap()
}
fn message(action: u64, address: u64, length: u64) -> Message {
    let mut m = Message::new(IO, Scope::new(1, 1, 9).unwrap());
    m.endpoint = ENDPOINT;
    m.sequence = action;
    m.value = address;
    m.length = length;
    m
}
#[test]
fn register_filter_excludes_msix_reserved_offsets_and_other_endpoints() {
    let mut g = gate([Some((0x1008, 0x1018)), None]);
    for o in [0x1000, 0x1004, 0x1008, 0x100c, 0x18, 0x38, 0x4000, 1] {
        assert_eq!(
            g.exchange(message(WRITE_REGISTER, o, 4)),
            Err(Error::Permission)
        );
    }
    assert_eq!(g.exchange(message(READ_REGISTER, 8, 4)).unwrap().value, 123);
    let mut m = message(READ_REGISTER, 8, 4);
    m.endpoint = 6;
    assert_eq!(g.exchange(m), Err(Error::Range));
}
#[test]
fn arena_is_exact_and_failed_revocation_keeps_access_closed() {
    let mut g = gate([None, None]);
    for (address, len) in [
        (0, 4),
        (4095, 4),
        (4096 + DMA_BYTES - 2, 4),
        (4096, 257),
        (u64::MAX, 4),
        (4096, 0),
    ] {
        assert_eq!(
            g.exchange(message(READ_ARENA, address, len)),
            Err(Error::Range)
        );
    }
    let mut m = message(WRITE_ARENA, 4096 + DMA_BYTES - 4, 4);
    m.payload[..4].copy_from_slice(&[1, 2, 3, 4]);
    g.exchange(m).unwrap();
    assert_eq!(
        &g.exchange(message(READ_ARENA, m.value, 4)).unwrap().payload[..4],
        &[1, 2, 3, 4]
    );
    let grant = g.exchange(message(GRANT, 0, 0)).unwrap();
    assert_eq!(grant.length, 32);
    assert_eq!(&grant.payload[8..16], &DMA_BYTES.to_le_bytes());
    assert!(!g.revoke());
    assert_eq!(g.exchange(message(CLOCK, 0, 0)), Err(Error::Stale));
}
#[test]
fn trusted_start_budget_is_bounded_and_not_renewable() {
    let scope = Scope::new(1, 1, 9).unwrap();
    let mut i = Instance::admitted(Role::Driver, scope, 1, 7, 100);
    assert!(i.admit_controller_start_budget(70_001).is_err());
    i.admit_controller_start_budget(70_000).unwrap();
    assert!(i.admit_controller_start_budget(70_000).is_err());
    assert!(!i.poll(70_099, true));
    assert!(i.poll(70_100, true));
}
