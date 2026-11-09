use osaiku_driver_runtime::{
    network::*,
    network_client::Consumer,
    network_driver::{Device, Driver},
    network_ring::Ring,
    protocol::Error,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
#[repr(C, align(64))]
struct Line(AtomicU32);
#[repr(C, align(4096))]
struct Entries([AtomicU32; 512]);
struct Queue {
    entries: Entries,
    prod: Line,
    cons: Line,
    work: Line,
    space: Line,
}
impl Queue {
    fn new() -> Self {
        Self {
            entries: Entries([const { AtomicU32::new(0) }; 512]),
            prod: Line(AtomicU32::new(0)),
            cons: Line(AtomicU32::new(0)),
            work: Line(AtomicU32::new(0)),
            space: Line(AtomicU32::new(0)),
        }
    }
    unsafe fn producer(&self) -> Ring {
        unsafe {
            Ring::mapped(
                self.entries.0.as_ptr() as *mut u8,
                &self.prod.0 as *const _ as *mut u32,
                &self.cons.0 as *const _ as *const u32,
                &self.space.0 as *const _ as *mut u32,
                &self.work.0 as *const _ as *const u32,
                64,
                true,
            )
            .unwrap()
        }
    }
    unsafe fn consumer(&self) -> Ring {
        unsafe {
            Ring::mapped(
                self.entries.0.as_ptr() as *mut u8,
                &self.cons.0 as *const _ as *mut u32,
                &self.prod.0 as *const _ as *const u32,
                &self.work.0 as *const _ as *mut u32,
                &self.space.0 as *const _ as *const u32,
                64,
                false,
            )
            .unwrap()
        }
    }
}
#[repr(C, align(4096))]
struct Data([u8; 64 * 4096]);
fn geometry() -> Geometry {
    Geometry {
        generation: 1,
        capacity: 64,
        slots: 64,
        slot_bytes: 4096,
        mtu: 1500,
    }
}
#[derive(Default)]
struct Nic {
    rx: std::collections::VecDeque<(u32, u32)>,
    tx: std::collections::VecDeque<(u32, u32, u16)>,
    returned: Vec<u32>,
    sent: Vec<(u32, u32)>,
}
struct Backend(Rc<RefCell<Nic>>);
impl Device for Backend {
    fn receive(&mut self) -> Result<Option<(u32, u32)>, Error> {
        Ok(self.0.borrow_mut().rx.pop_front())
    }
    fn recycle_rx(&mut self, slot: u32) -> Result<(), Error> {
        self.0.borrow_mut().returned.push(slot);
        Ok(())
    }
    fn transmit(&mut self, slot: u32, n: u32) -> Result<(), Error> {
        let mut d = self.0.borrow_mut();
        d.sent.push((slot, n));
        d.tx.push_back((slot, n, COMPLETED));
        Ok(())
    }
    fn complete_tx(&mut self) -> Result<Option<(u32, u32, u16)>, Error> {
        Ok(self.0.borrow_mut().tx.pop_front())
    }
}
#[test]
fn full_l2_batches_preserve_rx_copy_tx_credits_and_returns() {
    let rx = Box::new(Queue::new());
    let ret = Box::new(Queue::new());
    let tx = Box::new(Queue::new());
    let done = Box::new(Queue::new());
    let mut rd = Box::new(Data([0; 64 * 4096]));
    let mut td = Box::new(Data([0; 64 * 4096]));
    let nic = Rc::new(RefCell::new(Nic::default()));
    for slot in 0..64u32 {
        rd.0[slot as usize * 4096..slot as usize * 4096 + 64].fill(slot as u8);
        nic.borrow_mut().rx.push_back((slot, 64));
    }
    let mut driver = Driver::<_, 64>::new(
        Backend(nic.clone()),
        geometry(),
        unsafe { rx.producer() },
        unsafe { ret.consumer() },
        unsafe { tx.consumer() },
        unsafe { done.producer() },
    )
    .unwrap();
    let mut client = unsafe {
        Consumer::<64>::mapped(
            geometry(),
            rx.consumer(),
            ret.producer(),
            tx.producer(),
            done.consumer(),
            rd.0.as_ptr(),
            td.0.as_mut_ptr(),
        )
        .unwrap()
    };
    for i in 0..64u8 {
        client.transmit(&[i; 64]).unwrap();
    }
    assert_eq!(client.transmit(&[0; 64]), Err(Error::Backpressure));
    assert_eq!(driver.poll(1, 100, || {}), Ok(128));
    let mut scratch = [0u8; 4096];
    let mut copies = Vec::new();
    assert_eq!(
        client.poll(2, 100, &mut scratch, |b| copies.push(b.to_vec()), || {}),
        Ok(64)
    );
    rd.0.fill(255);
    assert_eq!(copies[2], [2; 64]);
    driver.poll(3, 100, || {}).unwrap();
    client.poll(4, 100, &mut scratch, |_| {}, || {}).unwrap();
    assert_eq!(nic.borrow().returned.len(), 64);
    assert_eq!(nic.borrow().sent.len(), 64);
    for i in 0..64u8 {
        client.transmit(&[i; 64]).unwrap();
    }
    assert_eq!(client.transmit(&[0; 64]), Err(Error::Backpressure));
    client.close();
    assert_eq!(client.transmit(&[0; 64]), Err(Error::Stale));
}

struct Event(Mutex<u64>, Condvar);

fn consumer_fixture() -> (Consumer<64>, [Box<Queue>; 4], [Box<Data>; 2]) {
    let queues = core::array::from_fn(|_| Box::new(Queue::new()));
    let mut data = core::array::from_fn(|_| Box::new(Data([0; 64 * 4096])));
    let consumer = unsafe {
        Consumer::mapped(
            geometry(),
            queues[0].consumer(),
            queues[1].producer(),
            queues[2].producer(),
            queues[3].consumer(),
            data[0].0.as_ptr(),
            data[1].0.as_mut_ptr(),
        )
    }
    .unwrap();
    (consumer, queues, data)
}

#[test]
fn duplicate_completion_cannot_mint_a_credit_or_release_an_arena() {
    let (mut client, queues, _data) = consumer_fixture();
    client.transmit(&[1; 64]).unwrap();
    let mut producer = unsafe { queues[3].producer() };
    let done = Entry {
        slot: 0,
        generation: 1,
        length: 64,
        status: COMPLETED,
    };
    producer.push(done).unwrap();
    producer.push(done).unwrap();
    assert_eq!(
        client.poll(1, 100, &mut [0; 4096], |_| {}, || {}),
        Err(Error::Range)
    );
    // One validated completion permits exactly one reuse, not two. The shared
    // submit queue itself also remains bounded after the malicious completion.
    let mut submissions = unsafe { queues[2].consumer() };
    submissions.pop().unwrap();
    for _ in 0..64 {
        client.transmit(&[1; 64]).unwrap();
    }
    assert_eq!(client.transmit(&[1; 64]), Err(Error::Backpressure));
    assert_eq!(client.counters.invalid, 1);
}

#[test]
fn duplicate_rx_is_not_parsed_twice_and_pending_return_survives_batch_error() {
    let (mut client, queues, _data) = consumer_fixture();
    let mut producer = unsafe { queues[0].producer() };
    let entry = Entry {
        slot: 0,
        generation: 1,
        length: 64,
        status: COMPLETED,
    };
    producer.push(entry).unwrap();
    producer.push(entry).unwrap();
    let mut seen = 0;
    assert_eq!(
        client.poll(1, 100, &mut [0; 4096], |_| seen += 1, || {}),
        Err(Error::State)
    );
    assert_eq!(seen, 1);
    client
        .poll(2, 100, &mut [0; 4096], |_| panic!("duplicate RX"), || {})
        .unwrap();
    let mut returns = unsafe { queues[1].consumer() };
    let (b, _) = returns.pop().unwrap();
    assert_eq!(
        Entry::decode(b, geometry(), RingKind::RxReturn)
            .unwrap()
            .slot,
        0
    );
    assert_eq!(returns.available(), Ok(0));
}

#[test]
fn hostile_rewrite_and_stale_generation_close_channel_before_any_parser_call() {
    let (mut client, queues, _data) = consumer_fixture();
    let mut producer = unsafe { queues[0].producer() };
    producer
        .push(Entry {
            slot: 0,
            generation: 1,
            length: 64,
            status: COMPLETED,
        })
        .unwrap();
    // Peer rewrites metadata after publication. We validate a private snapshot
    // before calculating any data address or invoking a parser.
    queues[0].entries.0[0].store(u32::MAX.to_le(), Ordering::Relaxed);
    assert_eq!(
        client.poll(1, 100, &mut [0; 4096], |_| panic!("bad slot"), || {}),
        Err(Error::Range)
    );
    producer
        .push(Entry {
            slot: 0,
            generation: 2,
            length: 64,
            status: COMPLETED,
        })
        .unwrap();
    assert_eq!(
        client.poll(2, 100, &mut [0; 4096], |_| panic!("stale RX"), || {}),
        Err(Error::Stale)
    );
    assert_eq!(client.transmit(&[0; 64]), Err(Error::Stale));
    let before = queues[0].work.0.load(Ordering::Relaxed);
    client.disarm();
    assert_eq!(queues[0].work.0.load(Ordering::Relaxed), before);
}
impl Event {
    fn notify(&self) {
        let mut seq = self.0.lock().unwrap();
        *seq += 1;
        self.1.notify_one();
    }
    fn wait(&self, last: &mut u64) {
        let lock = self.0.lock().unwrap();
        let (lock, result) = self
            .1
            .wait_timeout_while(lock, Duration::from_secs(5), |s| *s == *last)
            .unwrap();
        assert!(!result.timed_out(), "lost wakeup");
        *last = *lock;
    }
}
#[test]
fn two_cpu_style_producer_and_waiter_do_not_lose_wakeup_or_space_credit() {
    let q = Arc::new(Queue::new());
    let work = Arc::new(Event(Mutex::new(0), Condvar::new()));
    let space = Arc::new(Event(Mutex::new(0), Condvar::new()));
    let producer = {
        let q = q.clone();
        let work = work.clone();
        let space = space.clone();
        std::thread::spawn(move || {
            let mut ring = unsafe { q.producer() };
            let mut last = 0;
            for i in 0..100_000u32 {
                loop {
                    if ring.available().unwrap() != 0 {
                        let hint = ring
                            .push(Entry {
                                slot: i % 64,
                                generation: 1,
                                length: i,
                                status: 0,
                            })
                            .unwrap();
                        if hint {
                            work.notify();
                        }
                        break;
                    }
                    if ring.arm().unwrap() {
                        space.wait(&mut last);
                    }
                    ring.disarm();
                }
            }
        })
    };
    let consumer = std::thread::spawn(move || {
        let mut ring = unsafe { q.consumer() };
        let mut last = 0;
        for i in 0..100_000u32 {
            loop {
                if ring.available().unwrap() != 0 {
                    let (b, hint) = ring.pop().unwrap();
                    assert_eq!(u32::from_le_bytes(b[8..12].try_into().unwrap()), i);
                    if hint {
                        space.notify();
                    }
                    break;
                }
                if ring.arm().unwrap() {
                    work.wait(&mut last);
                }
                ring.disarm();
            }
        }
        assert_eq!(q.prod.0.load(Ordering::Acquire), 100_000);
        assert_eq!(q.cons.0.load(Ordering::Acquire), 100_000);
    });
    producer.join().unwrap();
    consumer.join().unwrap();
}
