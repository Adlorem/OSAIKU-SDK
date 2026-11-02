//! Kernel ownership removes message transport, not aperture or lease checks.
use osaiku_driver_runtime::{controller_io as xhci, nvme_io as nvme, protocol::Error};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct State {
    calls: usize,
    denied: bool,
}
#[derive(Clone)]
struct Platform(Rc<RefCell<State>>);
impl Platform {
    fn access(&self) -> Result<(), Error> {
        let mut s = self.0.borrow_mut();
        s.calls += 1;
        if s.denied {
            Err(Error::Permission)
        } else {
            Ok(())
        }
    }
}
impl xhci::Platform for Platform {
    fn read_register(&mut self, _: u32) -> Result<u32, Error> {
        self.access().map(|_| 42)
    }
    fn write_register(&mut self, _: u32, _: u32) -> Result<(), Error> {
        self.access()
    }
    fn read_arena(&mut self, _: u64, b: &mut [u8]) -> Result<(), Error> {
        self.access()?;
        b.fill(42);
        Ok(())
    }
    fn write_arena(&mut self, _: u64, _: &[u8]) -> Result<(), Error> {
        self.access()
    }
    fn zero_arena(&mut self, _: u64, _: usize) -> Result<(), Error> {
        self.access()
    }
    fn fence(&mut self) -> Result<(), Error> {
        self.access()
    }
    fn now(&mut self) -> u64 {
        123
    }
    fn revoke(&mut self) -> bool {
        false
    }
}
#[test]
fn nvme_direct_path_keeps_msix_exclusion_epoch_check_and_failed_stop_closure() {
    let s = Rc::new(RefCell::new(State::default()));
    let mut gate = nvme::Gate::new(
        Platform(s.clone()),
        nvme::Registers {
            aperture: 8192,
            stride: 4,
            forbidden: [Some((4096, 8192)), None],
        },
        4096,
        nvme::DMA_BYTES,
        u64::MAX,
        1,
    )
    .unwrap();
    assert_eq!(gate.read_register(0), Ok(42));
    assert_eq!(gate.write_register(0x14, 1), Ok(()));
    assert_eq!(gate.fence(), Ok(()));
    for offset in [1, 0x1000, 8192, u32::MAX] {
        assert!(gate.read_register(offset).is_err());
        assert!(gate.write_register(offset, 0).is_err());
    }
    assert!(gate.write_register(0, 0).is_err());
    assert_eq!(s.borrow().calls, 3);
    s.borrow_mut().denied = true;
    assert!(gate.read_register(0).is_err());
    assert!(gate.write_register(0x14, 1).is_err());
    assert!(gate.fence().is_err());
    assert!(!gate.revoke());
    s.borrow_mut().denied = false;
    let before = s.borrow().calls;
    assert!(gate.read_register(0).is_err());
    assert!(gate.write_register(0x14, 1).is_err());
    assert!(gate.owner_write(4096, &[0; 16]).is_err());
    assert!(gate.fence().is_err());
    assert_eq!(s.borrow().calls, before);
}
#[test]
fn xhci_direct_path_keeps_irq_page_exclusion_zero_bounds_and_failed_stop_closure() {
    let s = Rc::new(RefCell::new(State::default()));
    let mut gate = xhci::Gate::new(
        Platform(s.clone()),
        xhci::Registers {
            aperture: 65536,
            operational: 0x80,
            runtime: 0x2000,
            doorbells: 0x3000,
            ports: 4,
            slots: 8,
            forbidden: [Some((0x3000, 0x4000)), None],
        },
        4096,
        xhci::DMA_BYTES,
    )
    .unwrap();
    assert_eq!(gate.read_register(0), Ok(42));
    assert_eq!(gate.write_register(0x80, 1), Ok(()));
    assert_eq!(gate.owner_zero(4096, 4096), Ok(()));
    assert_eq!(gate.fence(), Ok(()));
    for offset in [1, 0x3000, 65536, u32::MAX] {
        assert!(gate.read_register(offset).is_err());
        assert!(gate.write_register(offset, 0).is_err());
    }
    for (at, bytes) in [
        (4096, 4097),
        (4095, 1),
        (4096 + xhci::DMA_BYTES - 1, 2),
        (u64::MAX, 1),
    ] {
        assert!(gate.owner_zero(at, bytes).is_err());
    }
    assert_eq!(s.borrow().calls, 4);
    s.borrow_mut().denied = true;
    assert!(gate.owner_zero(4096, 4096).is_err());
    assert!(gate.read_register(0).is_err());
    assert!(!gate.revoke());
    s.borrow_mut().denied = false;
    let before = s.borrow().calls;
    assert!(gate.owner_zero(4096, 4096).is_err());
    assert!(gate.write_register(0x80, 1).is_err());
    assert!(gate.fence().is_err());
    assert_eq!(s.borrow().calls, before);
}
