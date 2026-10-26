use osaiku_driver_runtime::{block::Error, nvme_client::*, nvme_process as wire};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct State {
    calls: Vec<(u64, u64, usize)>,
    dead: bool,
    fail: Option<usize>,
    quarantined: usize,
}
#[derive(Clone, Default)]
struct Link(Rc<RefCell<State>>);
impl Transport for Link {
    fn live(&self, g: u64) -> bool {
        g == 9 && !self.0.borrow().dead
    }
    fn exchange(
        &mut self,
        g: u64,
        op: u64,
        lba: u64,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), Error> {
        assert_eq!(g, 9);
        let mut s = self.0.borrow_mut();
        s.calls.push((op, lba, input.len() + output.len()));
        if s.fail == Some(s.calls.len()) {
            return Err(Error::Timeout);
        }
        output.fill(0x73);
        Ok(())
    }
    fn quarantine(&mut self) {
        let mut s = self.0.borrow_mut();
        s.dead = true;
        s.quarantined += 1;
    }
}
fn geometry() -> Geometry {
    Geometry {
        generation: 9,
        namespace: 1,
        blocks: 1024,
        limit: 4096,
        cap: 31 | (1 << 24) | (1 << 37),
        version: 0x10400,
    }
}
#[test]
fn splits_mdts_and_durability_requires_a_flush_receipt() {
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    c.write(8, &[1; 8704], true).unwrap();
    assert_eq!(
        l.0.borrow().calls,
        [
            (wire::WRITE, 8, 4096),
            (wire::WRITE, 16, 4096),
            (wire::WRITE, 24, 512),
            (wire::FLUSH, 0, 0)
        ]
    );
    l.0.borrow_mut().calls.clear();
    let mut out = [0; 8704];
    c.read(8, &mut out).unwrap();
    assert_eq!(out, [0x73; 8704]);
    assert_eq!(
        l.0.borrow().calls,
        [
            (wire::READ, 8, 4096),
            (wire::READ, 16, 4096),
            (wire::READ, 24, 512)
        ]
    );
}
#[test]
fn validates_the_whole_range_before_first_partial_write() {
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    for (lba, n) in [(1020, 4096), (u64::MAX, 512), (0, 513), (0, 0), (0, 262656)] {
        assert_eq!(c.write(lba, &vec![1; n], false), Err(Error::InvalidRange));
    }
    assert!(l.0.borrow().calls.is_empty());
    assert!(c.live());
}
#[test]
fn every_partial_write_or_flush_failure_is_uncertain_and_never_replayed() {
    for cut in 1..=4 {
        let l = Link::default();
        l.0.borrow_mut().fail = Some(cut);
        let mut c = Client::attach(l.clone(), geometry()).unwrap();
        assert_eq!(c.write(0, &[1; 8704], true), Err(Error::UnknownCommit));
        assert_eq!(l.0.borrow().calls.len(), cut);
        assert!(!c.live());
        assert_eq!(c.flush(), Err(Error::Stale));
        assert_eq!(l.0.borrow().calls.len(), cut);
    }
}
#[test]
fn stale_generation_and_failed_read_permanently_close_the_client() {
    for failed_read in [false, true] {
        let l = Link::default();
        let mut c = Client::attach(l.clone(), geometry()).unwrap();
        if failed_read {
            l.0.borrow_mut().fail = Some(1)
        } else {
            l.0.borrow_mut().dead = true
        }
        let mut out = [0x44; 512];
        assert_eq!(
            c.read(0, &mut out),
            Err(if failed_read {
                Error::Timeout
            } else {
                Error::Stale
            })
        );
        assert_eq!(out, [0x44; 512]);
        l.0.borrow_mut().dead = false;
        assert!(!c.live());
        assert!(c.read(0, &mut out).is_err());
    }
}
#[test]
fn identify_and_shutdown_are_bounded_and_shutdown_closes_io() {
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    for cns in 0..=3 {
        c.identify(cns, &mut [0; 4096]).unwrap();
    }
    assert_eq!(c.identify(4, &mut [0; 4096]), Err(Error::InvalidRange));
    c.shutdown().unwrap();
    assert!(!c.live());
    assert!(c.flush().is_err());
    assert_eq!(l.0.borrow().calls.len(), 5);
    assert_eq!(l.0.borrow().calls[4], (wire::SHUTDOWN, 0, 0));
}
#[test]
fn identify_wire_rejects_unbounded_pages_and_selectors() {
    let r = wire::Request {
        generation: 9,
        sequence: 1,
        namespace: 1,
        operation: wire::IDENTIFY,
        lba: 3,
        bytes: 4096,
        buffer: 0x104000,
    };
    assert_eq!(wire::Request::decode(&r.encode()), Ok(r));
    for r in [
        wire::Request { lba: 4, ..r },
        wire::Request { bytes: 512, ..r },
        wire::Request { buffer: 0, ..r },
    ] {
        assert!(wire::Request::decode(&r.encode()).is_err());
    }
}
