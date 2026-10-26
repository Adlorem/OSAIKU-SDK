use osaiku_driver_runtime::hid::{self, Decoder, KeyEvent, Poll};
fn event(usage: u8, pressed: bool) -> Poll {
    Poll::Event(KeyEvent { usage, pressed })
}
#[test]
fn mouse_exact_boot_report_signed_bounds_and_buttons() {
    assert!(hid::mouse(&[]).is_none());
    assert!(hid::mouse(&[0; 4]).is_none());
    let p = hid::mouse(&[255, 128, 127]).unwrap();
    assert_eq!((p.dx, p.dy, p.buttons), (-128, 127, 7));
}
#[test]
fn keyboard_diff_modifiers_duplicates_release_and_no_repeat() {
    let mut d = Decoder::new();
    let r = d.bytes(&[2, 0, 4, 4, 0, 0, 0, 0]);
    assert_eq!(r[0], event(0xe1, true));
    assert_eq!(r[1], event(4, true));
    assert_eq!(r[2], Poll::NoEvent);
    assert!(d
        .bytes(&[2, 0, 4, 0, 0, 0, 0, 0])
        .iter()
        .all(|e| *e == Poll::NoEvent));
    let r = d.bytes(&[0; 8]);
    assert_eq!(r[0], event(0xe1, false));
    assert_eq!(r[1], event(4, false));
}
#[test]
fn invalid_rollover_and_short_require_neutral() {
    for r in [
        vec![0; 7],
        vec![0, 0, 1, 0, 0, 0, 0, 0],
        vec![0, 0, 0xe0, 0, 0, 0, 0, 0],
    ] {
        let mut d = Decoder::new();
        d.bytes(&[0, 0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(d.bytes(&r)[0], Poll::Lost);
        assert_eq!(d.bytes(&[0, 0, 5, 0, 0, 0, 0, 0])[0], Poll::NoEvent);
        d.bytes(&[0; 8]);
        assert_eq!(d.bytes(&[0, 0, 5, 0, 0, 0, 0, 0])[0], event(5, true));
    }
}
#[test]
fn physical_event_wire_rejects_reserved_bits() {
    for usage in 0..=255 {
        for pressed in [false, true] {
            let e = event(usage, pressed);
            assert_eq!(Poll::decode(e.encode()), Some(e));
        }
    }
    assert_eq!(Poll::decode(0x4104), None);
    assert_eq!(Poll::decode(0x8004), None);
}
#[test]
fn input_operations_require_explicit_owner_admission() {
    use osaiku_driver_runtime::{
        process::*,
        protocol::{Error, Scope},
    };
    let scope = Scope::new(1, 2, 3).unwrap();
    let mut i = Instance::admitted(Role::Driver, scope, 1, 1, 0);
    i.authorize(Role::Driver, &Message::new(op::HELLO, scope), 1)
        .unwrap();
    i.authorize(Role::Driver, &Message::new(op::READY, scope), 2)
        .unwrap();
    let m = Message::new(op::HID_READ, scope);
    assert_eq!(i.authorize(Role::Driver, &m, 3), Err(Error::Permission));
    i.admit_hid();
    assert!(i.authorize(Role::Driver, &m, 4).is_ok());
    assert_eq!(i.authorize(Role::Manager, &m, 5), Err(Error::Permission));
    let mut stale = m;
    stale.release = 2;
    assert_eq!(i.authorize(Role::Driver, &stale, 6), Err(Error::Permission));
}

#[test]
fn keyboard_oem_reserved_byte_is_ignored() {
    let mut d = Decoder::new();
    assert_eq!(d.bytes(&[0, 255, 4, 0, 0, 0, 0, 0])[0], event(4, true));
    assert_eq!(d.bytes(&[0, 1, 0, 0, 0, 0, 0, 0])[0], event(4, false));
}
