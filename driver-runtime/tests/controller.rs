use osaiku_driver_runtime::{
    controller::*,
    controller_io::*,
    process::{Instance, Message, Role},
    protocol::{Error, Scope},
};
fn request(generation: u64) -> Request {
    Request {
        sequence: 1,
        generation,
        operation: request::NOOP,
        slot: 0,
        endpoint: 0,
        port: 0,
        speed: 0,
        kind: 0,
        interval: 0,
        packet: 0,
        length: 0,
        setup: [0; 8],
        timeout: 1000,
    }
}
#[test]
fn wire_canonical_and_generation_checked() {
    let r = request(9);
    assert_eq!(Request::decode(&r.encode()).unwrap(), r);
    let mut b = r.encode();
    b[63] = 1;
    assert_eq!(Request::decode(&b), Err(Error::Reserved));
    b[63] = 0;
    b[8..16].fill(0);
    assert_eq!(Request::decode(&b), Err(Error::Range));
}

#[test]
fn evaluate_ep0_is_an_address_free_generation_bound_request() {
    let mut r = request(9);
    r.operation = request::EVALUATE_EP0;
    r.slot = 2;
    r.packet = 64;
    assert_eq!(Request::decode(&r.encode()), Ok(r));
    let mut channel = Channel::new(9);
    channel.submit(r).unwrap();
    assert_eq!(channel.fetch().unwrap().packet, 64);
    channel.fail();
    assert_eq!(channel.submit(r), Err(Error::Stale));
    r.operation = 12;
    assert_eq!(Request::decode(&r.encode()), Err(Error::Range));
}
#[test]
fn crash_invalidates_all_outstanding_receipts_and_children() {
    let mut c = Channel::new(9);
    let first = c.submit(request(9)).unwrap();
    c.submit(request(9)).unwrap();
    let r = c.fetch().unwrap();
    assert_eq!(r.sequence, first);
    c.fail();
    assert!(!c.active());
    assert!(c.fetch().is_none());
    assert_eq!(c.take(first), Err(Error::Stale));
    assert_eq!(c.submit(request(9)), Err(Error::Stale));
}
#[test]
fn shared_reset_clients_boot_medium_and_no_other_input_stage_restart() {
    let mut c = Channel::new(1);
    assert_eq!(c.update(), Update::Restart);
    c.dependencies(0, false, true);
    assert_eq!(c.update(), Update::Live);
    for (clients, boot, input) in [(1, false, true), (0, true, true), (0, false, false)] {
        c.dependencies(clients, boot, input);
        assert_eq!(c.update(), Update::Restart);
    }
    c.dependencies(0, false, true);
    c.submit(request(1)).unwrap();
    assert_eq!(c.update(), Update::Restart);
}
#[test]
fn foreign_unsolicited_and_duplicate_completions_rejected() {
    let mut c = Channel::new(1);
    let id = c.submit(request(1)).unwrap();
    let mut r = Reply {
        sequence: id,
        generation: 1,
        slot: 0,
        endpoint: 0,
        code: 1,
        port: 0,
        actual: 0,
    };
    assert_eq!(c.complete(r), Err(Error::Permission));
    c.fetch();
    r.generation = 2;
    assert_eq!(c.complete(r), Err(Error::Stale));
    r.generation = 1;
    c.complete(r).unwrap();
    assert_eq!(c.complete(r), Err(Error::Permission));
    assert_eq!(c.take(id).unwrap(), Some(r));
    assert_eq!(c.take(id), Err(Error::Stale));
}
#[test]
fn queue_backpressure_never_overwrites() {
    let mut c = Channel::new(3);
    for _ in 0..QUEUE {
        c.submit(request(3)).unwrap();
    }
    assert_eq!(c.submit(request(3)), Err(Error::Backpressure));
    for i in 1..=QUEUE {
        assert_eq!(c.fetch().unwrap().sequence, i as u64);
    }
}
struct Hardware {
    memory: Vec<u8>,
    stopped: bool,
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
        1
    }
    fn revoke(&mut self) -> bool {
        self.stopped
    }
}
fn gate() -> Gate<Hardware> {
    Gate::new(
        Hardware {
            memory: vec![0; DMA_BYTES as usize],
            stopped: false,
        },
        Registers {
            aperture: 0x4000,
            operational: 0x40,
            runtime: 0x1000,
            doorbells: 0x2000,
            ports: 8,
            slots: 8,
            forbidden: [Some((0x3000, 0x4000)), None],
        },
        4096,
        DMA_BYTES,
    )
    .unwrap()
}
fn message(op: u64, value: u64, n: u64) -> Message {
    let mut m = Message::new(IO, Scope::new(1, 1, 1).unwrap());
    m.sequence = op;
    m.value = value;
    m.length = n;
    m
}
#[test]
fn arena_and_register_grants_cannot_escape_or_alias_msix() {
    let mut g = gate();
    assert_eq!(
        g.exchange(message(WRITE_REGISTER, 0x3000, 4)),
        Err(Error::Permission)
    );
    assert_eq!(
        g.exchange(message(WRITE_REGISTER, 0x18, 4)),
        Err(Error::Permission)
    );
    assert_eq!(g.exchange(message(READ_ARENA, 0, 4)), Err(Error::Range));
    assert_eq!(
        g.exchange(message(READ_ARENA, 4096 + DMA_BYTES - 2, 4)),
        Err(Error::Range)
    );
    let mut m = message(WRITE_ARENA, 4096, 4);
    m.payload[..4].copy_from_slice(&[1, 2, 3, 4]);
    g.exchange(m).unwrap();
    assert_eq!(
        &g.exchange(message(READ_ARENA, 4096, 4)).unwrap().payload[..4],
        &[1, 2, 3, 4]
    );
    assert!(!g.revoke());
    assert_eq!(g.exchange(message(CLOCK, 0, 0)), Err(Error::Stale));
}
#[test]
fn controller_operations_require_platform_admission() {
    let scope = Scope::new(1, 1, 1).unwrap();
    let mut i = Instance::admitted(Role::Driver, scope, 1, 6, 0);
    let mut m = Message::new(2, scope);
    m.endpoint = 6;
    i.authorize(Role::Driver, &m, 0).unwrap();
    m.operation = IO;
    assert_eq!(i.authorize(Role::Driver, &m, 0), Err(Error::Permission));
    i.admit_controller();
    i.authorize(Role::Driver, &m, 0).unwrap();
}

#[test]
fn recycled_queue_slots_preserve_receipt_order() {
    let mut c = Channel::new(1);
    let first = c.submit(request(1)).unwrap();
    let second = c.submit(request(1)).unwrap();
    assert_eq!(c.fetch().unwrap().sequence, first);
    c.complete(Reply {
        sequence: first,
        generation: 1,
        slot: 0,
        endpoint: 0,
        code: 1,
        port: 0,
        actual: 0,
    })
    .unwrap();
    c.take(first).unwrap();
    let third = c.submit(request(1)).unwrap();
    assert_eq!(c.fetch().unwrap().sequence, second);
    assert_eq!(c.fetch().unwrap().sequence, third);
}
#[test]
fn completion_cannot_change_target_or_exceed_requested_payload() {
    let mut c = Channel::new(1);
    let mut q = request(1);
    q.operation = request::TRANSFER;
    q.slot = 2;
    q.endpoint = 3;
    q.length = 8;
    let sequence = c.submit(q).unwrap();
    c.fetch().unwrap();
    let reply = Reply {
        sequence,
        generation: 1,
        slot: 2,
        endpoint: 3,
        code: 1,
        port: 0,
        actual: 8,
    };
    for r in [
        Reply { slot: 3, ..reply },
        Reply {
            endpoint: 5,
            ..reply
        },
        Reply { actual: 9, ..reply },
        Reply { port: 1, ..reply },
    ] {
        assert_eq!(c.complete(r), Err(Error::Permission));
    }
    c.complete(reply).unwrap();
    assert_eq!(c.take(sequence).unwrap(), Some(reply));
}
