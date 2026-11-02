use osaiku_driver_runtime::{
    controller_io::Platform,
    nvme::dma_region,
    nvme_io::{Gate, Registers, DMA_BYTES},
    nvme_process::*,
    process::Message,
    protocol::{Error, Scope},
};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct State {
    memory: Vec<u8>,
    register_writes: Vec<(u32, u32)>,
    register_reads: Vec<u32>,
    fences: usize,
    now: u64,
    cq_order: Vec<&'static str>,
}
#[derive(Clone)]
struct Recorder(Rc<RefCell<State>>);
impl Platform for Recorder {
    fn read_register(&mut self, offset: u32) -> Result<u32, Error> {
        self.0.borrow_mut().register_reads.push(offset);
        Ok(0x1)
    }
    fn write_register(&mut self, offset: u32, value: u32) -> Result<(), Error> {
        self.0.borrow_mut().register_writes.push((offset, value));
        Ok(())
    }
    fn read_arena(&mut self, o: u64, b: &mut [u8]) -> Result<(), Error> {
        self.0.borrow_mut().cq_order.push(if b.len() == 4 { "phase" } else { "body" });
        b.copy_from_slice(&self.0.borrow().memory[o as usize..o as usize + b.len()]);
        Ok(())
    }
    fn write_arena(&mut self, o: u64, b: &[u8]) -> Result<(), Error> {
        self.0.borrow_mut().memory[o as usize..o as usize + b.len()].copy_from_slice(b);
        Ok(())
    }
    fn fence(&mut self) -> Result<(), Error> {
        self.0.borrow_mut().fences += 1;
        self.0.borrow_mut().cq_order.push("fence");
        Ok(())
    }
    fn now(&mut self) -> u64 {
        self.0.borrow().now
    }
    fn revoke(&mut self) -> bool {
        true
    }
}
fn gate() -> (Gate<Recorder>, Recorder) {
    let r = Recorder(Rc::new(RefCell::new(State {
        memory: vec![0; DMA_BYTES as usize],
        now: 777,
        ..State::default()
    })));
    let g = Gate::new(
        r.clone(),
        Registers {
            aperture: 0x2000,
            stride: 4,
            forbidden: [None, None],
        },
        4096,
        DMA_BYTES,
        u64::MAX,
        1,
    )
    .unwrap();
    (g, r)
}
fn io(sequence: u64, value: u64, length: u64) -> Message {
    let mut m = Message::new(IO, Scope::new(1, 1, 9).unwrap());
    m.endpoint = ENDPOINT;
    m.sequence = sequence;
    m.value = value;
    m.length = length;
    m
}
fn completion(r: &Recorder, queue: usize, slot: usize, cid: u16, phase: bool) {
    let at = dma_region(queue * 2 + 1).unwrap().0 as usize + slot * 16;
    let words = [7u32, 0, (queue as u32) << 16, u32::from(cid) | (u32::from(phase) << 16)];
    for (i, w) in words.into_iter().enumerate() {
        r.0.borrow_mut().memory[at + i * 4..at + i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
}
#[test]
fn gather_wire_round_trips_and_rejects_malformed_runs() {
    let runs = [
        Segment { lba: 8, pages: 3 },
        Segment { lba: (1 << 48) - 8, pages: 1 },
    ];
    let mut c = Channel::new(9);
    assert_eq!(c.submit(1, GATHER, 2, 16384, 4096), Err(Error::Permission));
    let id = c.submit_gather(1, &runs, 4096).unwrap();
    assert!(c.readable());
    let mut payload = [0u8; 256];
    let n = c.fetch_encoded(&mut payload).unwrap();
    assert_eq!(n, BYTES + 16);
    assert!(c.fetch_encoded(&mut payload).is_none());
    let (r, s) = decode_fetch(&payload[..n]).unwrap();
    assert_eq!((r.operation, r.lba, r.bytes, r.buffer), (GATHER, 2, 16384, 4096));
    assert_eq!(s.as_slice(), &runs);
    assert_eq!(c.segments(), &runs);
    for bad in [&payload[..n - 8], &payload[..n + 8], &payload[..BYTES]] {
        assert!(decode_fetch(bad).is_err());
    }
    let mut lying = payload;
    lying[41] = 0x50; // header claims 20480 bytes; the runs carry 16384
    assert_eq!(decode_fetch(&lying[..n]), Err(Error::Range));
    c.complete(Reply {
        generation: 9,
        sequence: id,
        bytes: 16384,
        failed: false,
    })
    .unwrap();
    assert!(c.take(id).unwrap().is_some());
    assert!(c.segments().is_empty());
    assert!(c.idle() && !c.clean());
    let too_long = [Segment { lba: 0, pages: 1 }; MAX_SEGMENTS + 1];
    for bad in [
        &[][..],
        &too_long[..],
        &[Segment { lba: 0, pages: 0 }][..],
        &[Segment { lba: 0, pages: 65 }][..],
        &[Segment { lba: 0, pages: 64 }, Segment { lba: 512, pages: 1 }][..],
        &[Segment { lba: 1 << 48, pages: 1 }][..],
    ] {
        assert!(Segments::new(bad).is_err());
        assert!(c.submit_gather(1, bad, 4096).is_err());
    }
    assert!(c.idle());
}
#[test]
fn submit_and_reap_values_are_bounded() {
    let v = submit_value(1, 3, &[31, 0, 1]).unwrap();
    assert_eq!(parse_submit(v, 3), Ok((1, 3, [31, 0, 1, 0])));
    assert_eq!(parse_submit(v, 2), Err(Error::Range));
    for bad in [
        submit_value(2, 0, &[0]),
        submit_value(0, 64, &[0]),
        submit_value(0, 0, &[64]),
        submit_value(0, 0, &[]),
        submit_value(0, 0, &[0; 5]),
    ] {
        assert_eq!(bad, Err(Error::Range));
    }
    let v = reap_value(1, 31, false, 32, 16).unwrap();
    assert_eq!(parse_reap(v), Ok((1, 31, false, 32, 16)));
    for bad in [
        reap_value(2, 0, true, 2, 1),
        reap_value(0, 2, true, 2, 1),
        reap_value(0, 0, true, 1, 1),
        reap_value(0, 0, true, 65, 1),
        reap_value(0, 0, true, 2, 0),
        reap_value(0, 0, true, 2, 17),
    ] {
        assert_eq!(bad, Err(Error::Range));
    }
    assert_eq!(parse_reap(v | 1 << 20), Err(Error::Range));
    assert_eq!(parse_reap(v | 1 << 40), Err(Error::Range));
}
#[test]
fn gate_submit_copies_entries_then_publishes_and_rings_the_tail() {
    let (mut g, r) = gate();
    let mut m = io(SUBMIT, submit_value(1, 0, &[31, 0]).unwrap(), 128);
    m.payload[..64].fill(0x11);
    m.payload[64..128].fill(0x22);
    let reply = g.exchange(m).unwrap();
    assert_eq!(reply.length, 0);
    assert!(reply.payload.iter().all(|b| *b == 0));
    let sq = dma_region(2).unwrap().0 as usize;
    let s = r.0.borrow();
    assert!(s.memory[sq + 31 * 64..sq + 32 * 64].iter().all(|b| *b == 0x11));
    assert!(s.memory[sq..sq + 64].iter().all(|b| *b == 0x22));
    assert!(s.memory[sq + 64..sq + 31 * 64].iter().all(|b| *b == 0));
    assert_eq!(s.fences, 1);
    assert_eq!(s.register_writes, [(0x1008, 0)]);
    drop(s);
    for (value, length) in [
        (submit_value(1, 0, &[0]).unwrap(), 0),
        (submit_value(1, 0, &[0]).unwrap(), 65),
        (submit_value(1, 0, &[0, 0, 0, 0]).unwrap() | 1 << 56, 256),
    ] {
        assert_eq!(g.exchange(io(SUBMIT, value, length)), Err(Error::Range));
    }
    assert_eq!(r.0.borrow().register_writes.len(), 1);
}
#[test]
fn gate_reap_copies_owned_entries_across_wrap_and_acknowledges_once() {
    let (mut g, r) = gate();
    completion(&r, 1, 30, 4, true);
    completion(&r, 1, 31, 5, true);
    completion(&r, 1, 0, 6, false); // phase flips at the ring wrap
    completion(&r, 1, 1, 9, true); // stale: previous pass of slot 1
    let reply = g
        .exchange(io(REAP, reap_value(1, 30, true, 32, 16).unwrap(), 0))
        .unwrap();
    assert_eq!(reply.length, 48);
    assert_eq!(reply.value, 777);
    assert_eq!(reply.offset, 0);
    let cid = |i: usize| u16::from_le_bytes([reply.payload[i * 16 + 12], reply.payload[i * 16 + 13]]);
    assert_eq!([cid(0), cid(1), cid(2)], [4, 5, 6]);
    assert!(reply.payload[48..].iter().all(|b| *b == 0));
    assert_eq!(r.0.borrow().register_writes, [(0x100c, 1)]);
    assert_eq!(r.0.borrow().cq_order, [
        "phase", "fence", "body", "phase", "fence", "body",
        "phase", "fence", "body", "phase", "fence",
    ]);
    assert!(r.0.borrow().register_reads.is_empty());
    // Nothing owned: no acknowledgement, CSTS returned for fatal-status checks.
    let reply = g
        .exchange(io(REAP, reap_value(1, 1, false, 32, 16).unwrap(), 0))
        .unwrap();
    assert_eq!((reply.length, reply.offset, reply.value), (0, 1, 777));
    assert_eq!(r.0.borrow().register_writes.len(), 1);
    assert_eq!(r.0.borrow().register_reads, [0x1c]);
    // The entry limit bounds a single observation.
    let reply = g
        .exchange(io(REAP, reap_value(1, 30, true, 32, 1).unwrap(), 0))
        .unwrap();
    assert_eq!(reply.length, 16);
    assert_eq!(r.0.borrow().register_writes.last(), Some(&(0x100c, 31)));
    assert_eq!(
        g.exchange(io(REAP, reap_value(0, 0, true, 2, 1).unwrap(), 16)),
        Err(Error::Range)
    );
    assert_eq!(g.exchange(io(REAP, 1 << 40, 0)), Err(Error::Range));
    assert!(g.revoke());
    assert_eq!(
        g.exchange(io(REAP, reap_value(0, 0, true, 2, 1).unwrap(), 0)),
        Err(Error::Stale)
    );
}
#[test]
fn forbidden_doorbell_page_refuses_submission_and_reaping() {
    let r = Recorder(Rc::new(RefCell::new(State {
        memory: vec![0; DMA_BYTES as usize],
        ..State::default()
    })));
    let mut g = Gate::new(
        r.clone(),
        Registers {
            aperture: 0x2000,
            stride: 4,
            forbidden: [Some((0x1000, 0x2000)), None],
        },
        4096,
        DMA_BYTES,
        u64::MAX,
        1,
    )
    .unwrap();
    assert_eq!(
        g.exchange(io(SUBMIT, submit_value(0, 1, &[0]).unwrap(), 64)),
        Err(Error::Permission)
    );
    completion(&r, 0, 0, 1, true);
    assert_eq!(
        g.exchange(io(REAP, reap_value(0, 0, true, 2, 1).unwrap(), 0)),
        Err(Error::Permission)
    );
    let s = r.0.borrow();
    assert!(s.register_writes.is_empty());
    assert!(s.memory[..4096].iter().all(|b| *b == 0));
}
