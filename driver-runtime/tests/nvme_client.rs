use osaiku_driver_runtime::{block::Error, nvme_client::*, nvme_process as wire};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct State {
    calls: Vec<(u64, u64, usize)>,
    dead: bool,
    fail: Option<usize>,
    die_after: Option<usize>,
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
        if s.die_after == Some(s.calls.len()) {
            s.dead = true;
        }
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
fn one_request_per_arena_buffer_and_durability_uses_fua() {
    // The driver splits by MDTS and keeps those commands in flight together;
    // the client no longer pays a separate round trip per controller limit.
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    c.write(8, &[1; 8704], true).unwrap();
    assert_eq!(l.0.borrow().calls, [(wire::WRITE_FUA, 8, 8704)]);
    l.0.borrow_mut().calls.clear();
    let mut out = vec![0; 262144];
    c.read(8, &mut out).unwrap();
    assert!(out.iter().all(|b| *b == 0x73));
    assert_eq!(l.0.borrow().calls, [(wire::READ, 8, 262144)]);
}
#[test]
fn gathered_runs_are_checked_before_any_submission() {
    let page = [5u8; 4096];
    let pages = [&page; 65];
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    let too_many: Vec<(u64, &[&[u8; 4096]])> = (0..=wire::MAX_SEGMENTS as u64)
        .map(|i| (i * 8, &pages[..1]))
        .collect();
    for runs in [
        &[][..],
        &[(0, &pages[..0])][..],
        &[(1020, &pages[..1])][..],
        &[(u64::MAX, &pages[..1])][..],
        &[(0, &pages[..64]), (600, &pages[..1])][..],
        &too_many[..],
    ] {
        assert_eq!(c.write_runs(runs), Err(Error::InvalidRange));
    }
    assert!(l.0.borrow().calls.is_empty());
    assert!(c.live());
    // A transport without a gathered request writes the same pages in order.
    c.write_runs(&[(16, &pages[..2]), (100, &pages[..1])])
        .unwrap();
    assert_eq!(
        l.0.borrow().calls,
        [
            (wire::WRITE, 16, 4096),
            (wire::WRITE, 24, 4096),
            (wire::WRITE, 100, 4096)
        ]
    );
    l.0.borrow_mut().fail = Some(4);
    assert_eq!(
        c.write_runs(&[(16, &pages[..1])]),
        Err(Error::UnknownCommit)
    );
    assert!(!c.live());
    assert_eq!(c.write_runs(&[(16, &pages[..1])]), Err(Error::Stale));
    assert_eq!(l.0.borrow().calls.len(), 4);
}

#[test]
fn read_scatterlist_validates_all_ranges_and_output_before_io() {
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    for runs in [vec![], vec![(0, 0)], vec![(0, 1), (1020, 1)], vec![(0, usize::MAX)],
        vec![(0, 1); wire::MAX_SEGMENTS + 1]] {
        assert_eq!(c.read_runs(&runs, &mut [0; 8192]), Err(Error::InvalidRange));
    }
    assert!(l.0.borrow().calls.is_empty());
    c.read_runs(&[(100, 1), (8, 1)], &mut [0; 8192]).unwrap();
    assert_eq!(l.0.borrow().calls, [(wire::READ, 100, 4096), (wire::READ, 8, 4096)]);
    l.0.borrow_mut().fail = Some(3);
    assert_eq!(c.read_runs(&[(100, 1)], &mut [0; 4096]), Err(Error::Timeout));
    assert!(!c.live());
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
    for cut in 1..=2 {
        let l = Link::default();
        l.0.borrow_mut().fail = Some(cut);
        let mut c = Client::attach(l.clone(), geometry()).unwrap();
        let result = if cut == 1 {
            c.write(0, &[1; 8704], true)
        } else {
            c.write(0, &[1; 8704], false).and_then(|()| c.flush())
        };
        assert_eq!(result, Err(Error::UnknownCommit));
        assert_eq!(l.0.borrow().calls.len(), cut);
        assert!(!c.live());
        assert_eq!(c.flush(), Err(Error::Stale));
        assert_eq!(l.0.borrow().calls.len(), cut);
    }
}
#[test]
fn owner_loss_during_fua_is_uncertain() {
    for cut in 1..=1 {
        let l = Link::default();
        l.0.borrow_mut().die_after = Some(cut);
        let mut c = Client::attach(l.clone(), geometry()).unwrap();
        assert_eq!(c.write(0, &[1; 8704], true), Err(Error::UnknownCommit));
        assert_eq!(l.0.borrow().calls.len(), cut);
        assert!(!c.live());
        assert_eq!(c.flush(), Err(Error::Stale));
        assert_eq!(l.0.borrow().calls.len(), cut);
    }
}
#[test]
fn loss_before_first_write_keeps_the_definite_no_submission_result() {
    let l = Link::default();
    let mut c = Client::attach(l.clone(), geometry()).unwrap();
    l.0.borrow_mut().dead = true;
    assert_eq!(c.write(0, &[1; 4096], true), Err(Error::Stale));
    assert!(l.0.borrow().calls.is_empty());
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
