use osaiku_driver_runtime::{controller_io as xhci, nvme_io as nvme, protocol::Error};
use std::{cell::RefCell, rc::Rc};
struct State { memory: Vec<u8>, calls: usize, valid: bool }
#[derive(Clone)]
struct Platform(Rc<RefCell<State>>);
impl xhci::Platform for Platform {
    fn read_register(&mut self, _: u32) -> Result<u32, Error> { unreachable!() }
    fn write_register(&mut self, _: u32, _: u32) -> Result<(), Error> { unreachable!() }
    fn read_arena(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        let mut s = self.0.borrow_mut(); s.calls += 1;
        if !s.valid { return Err(Error::Permission); }
        bytes.copy_from_slice(&s.memory[offset as usize..offset as usize + bytes.len()]); Ok(())
    }
    fn write_arena(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        let mut s = self.0.borrow_mut(); s.calls += 1;
        if !s.valid { return Err(Error::Permission); }
        s.memory[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes); Ok(())
    }
    fn fence(&mut self) -> Result<(), Error> { unreachable!() }
    fn now(&mut self) -> u64 { 0 }
    fn revoke(&mut self) -> bool { false }
}
trait CopyGate {
    fn read(&mut self, a: u64, b: &mut [u8]) -> Result<(), Error>;
    fn write(&mut self, a: u64, b: &[u8]) -> Result<(), Error>;
    fn revoke(&mut self) -> bool;
}
macro_rules! gate {
    ($kind:ty) => { impl CopyGate for $kind {
        fn read(&mut self, a: u64, b: &mut [u8]) -> Result<(), Error> { self.owner_read(a,b) }
        fn write(&mut self, a: u64, b: &[u8]) -> Result<(), Error> { self.owner_write(a,b) }
        fn revoke(&mut self) -> bool { self.revoke() }
    } };
}
gate!(xhci::Gate<Platform>);
gate!(nvme::Gate<Platform>);
fn exercise(mut gate: impl CopyGate, state: Rc<RefCell<State>>, size: u64, limit: usize) {
    let mut out = [0; 4096];
    gate.write(8192, &[0xa5; 4096]).unwrap();
    gate.read(8192, &mut out).unwrap();
    assert_eq!(out, [0xa5; 4096]);
    assert_eq!(state.borrow().calls, 2); // one platform validation per whole page
    assert!(state.borrow().memory[..4096].iter().all(|b| *b == 0));
    assert!(state.borrow().memory[8192..].iter().all(|b| *b == 0));
    for (at, len) in [(0, 1), (4095, 1), (4096+size-1, 2), (u64::MAX, 1), (4096, 0), (4096, limit + 1)] {
        assert_eq!(gate.read(at, &mut vec![0; len]), Err(Error::Range));
        assert_eq!(gate.write(at, &vec![0; len]), Err(Error::Range));
    }
    assert_eq!(state.borrow().calls, 2);
    // A grant/epoch change or hardware protection failure between copies is
    // observed on the next call, even though the owner's Gate remains open.
    state.borrow_mut().valid = false;
    out.fill(0x77);
    assert_eq!(gate.read(8192, &mut out), Err(Error::Permission));
    assert_eq!(out, [0x77; 4096]);
    assert_eq!(gate.write(8192, &[1; 4096]), Err(Error::Permission));
    assert_eq!(state.borrow().calls, 4);
    assert!(!gate.revoke()); // failed hardware stop still closes CPU access
    state.borrow_mut().valid = true;
    assert_eq!(gate.read(8192, &mut out), Err(Error::Stale));
    assert_eq!(gate.write(8192, &[1; 4096]), Err(Error::Stale));
    assert_eq!(state.borrow().calls, 4);
}
fn platform(bytes: u64) -> Platform {
    Platform(Rc::new(RefCell::new(State { memory: vec![0; bytes as usize], calls: 0, valid: true })))
}
#[test]
fn xhci_owner_copy_remains_bounded_and_revocable() {
    let p = platform(xhci::DMA_BYTES); let s = p.0.clone();
    let g = xhci::Gate::new(p, xhci::Registers { aperture: 65536, operational: 0x80,
        runtime: 0x2000, doorbells: 0x3000, ports: 4, slots: 8, forbidden: [None; 2] },
        4096, xhci::DMA_BYTES).unwrap();
    exercise(g, s, xhci::DMA_BYTES, xhci::OWNER_COPY_BYTES);
}
#[test]
fn nvme_owner_copy_remains_bounded_and_revocable() {
    let p = platform(nvme::DMA_BYTES); let s = p.0.clone();
    let g = nvme::Gate::new(p, nvme::Registers { aperture: 8192, stride: 4, forbidden: [None; 2] },
        4096, nvme::DMA_BYTES, u64::MAX, 1).unwrap();
    exercise(g, s, nvme::DMA_BYTES, nvme::OWNER_COPY_BYTES);
}

#[test]
fn nvme_bulk_owner_copy_checks_each_batch_and_leaves_neighbors_untouched() {
    let p = platform(nvme::DMA_BYTES);
    let s = p.0.clone();
    let mut g = nvme::Gate::new(p, nvme::Registers { aperture: 8192, stride: 4, forbidden: [None; 2] },
        4096, nvme::DMA_BYTES, u64::MAX, 1).unwrap();
    let offset = osaiku_driver_runtime::nvme::dma_region(5).unwrap().0;
    let bytes: Vec<u8> = (0..nvme::OWNER_COPY_BYTES).map(|n| (n % 251) as u8).collect();
    let mut out = vec![0; bytes.len()];
    g.owner_write(4096 + offset, &bytes).unwrap();
    g.owner_read(4096 + offset, &mut out).unwrap();
    assert_eq!(out, bytes);
    assert_eq!(s.borrow().calls, 2);
    assert!(s.borrow().memory[..offset as usize].iter().all(|b| *b == 0));
    assert!(s.borrow().memory[offset as usize + bytes.len()..].iter().all(|b| *b == 0));
    s.borrow_mut().valid = false;
    assert_eq!(g.owner_write(4096 + offset + bytes.len() as u64, &bytes), Err(Error::Permission));
    assert!(s.borrow().memory[offset as usize + bytes.len()..].iter().all(|b| *b == 0));
}

#[test]
fn xhci_16k_copy_checks_protection_again_and_preserves_adjacent_buffers() {
    let p = platform(xhci::DMA_BYTES);
    let s = p.0.clone();
    let mut g = xhci::Gate::new(p, xhci::Registers { aperture: 65536, operational: 0x80,
        runtime: 0x2000, doorbells: 0x3000, ports: 4, slots: 8, forbidden: [None; 2] },
        4096, xhci::DMA_BYTES).unwrap();
    let address = xhci::xhci_buffer(4096, 8, 3);
    let offset = (address - 4096) as usize;
    let bytes: Vec<u8> = (0..xhci::XHCI_TRANSFER_BYTES).map(|n| (n % 251) as u8).collect();
    let mut out = vec![0; bytes.len()];
    g.owner_write(address, &bytes).unwrap();
    g.owner_read(address, &mut out).unwrap();
    assert_eq!(out, bytes);
    assert_eq!(s.borrow().calls, 2);
    assert!(s.borrow().memory[..offset].iter().all(|b| *b == 0));
    assert!(s.borrow().memory[offset + bytes.len()..].iter().all(|b| *b == 0));
    s.borrow_mut().valid = false;
    out.fill(0x77);
    assert_eq!(g.owner_read(address, &mut out), Err(Error::Permission));
    assert!(out.iter().all(|b| *b == 0x77));
    assert_eq!(g.owner_write(address, &vec![1; bytes.len()]), Err(Error::Permission));
    assert_eq!(&s.borrow().memory[offset..offset + bytes.len()], &bytes);
}
