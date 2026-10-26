use osaiku_driver_runtime::{
    ps2::{response, valid_response, Clock, Commands, Lamps},
    ps2_pointer::Event,
};
#[test]
fn controller_reset_and_unrequested_arguments_are_denied() {
    let mut c = Commands::new();
    for b in [0xfe, 0xd1, 0xad, 0xae, 0xa7, 0xa8, 0x60, 0x20, 0xf5, 3] {
        assert!(!c.admit(b, 0));
    }
    assert!(c.admit(0xe8, 0));
    assert!(!c.admit(3, 1));
    c.receive(0xfa, 2);
    assert!(c.admit(3, 3));
    c.receive(0xfa, 4);
    assert!(c.admit(0xf4, 5));
}
#[test]
fn resend_preserves_argument_boundary_and_timeout_requires_reset() {
    let mut c = Commands::new();
    assert!(c.admit(0xe8, 0));
    c.receive(0xfa, 1);
    assert!(c.admit(3, 2));
    c.receive(0xfe, 3);
    assert!(c.admit(3, 4));
    c.receive(0xfa, 205);
    assert!(!c.admit(0xf4, 206));
    assert!(c.admit(0xff, 207));
    c.receive(0xfa, 208);
    assert!(!c.admit(0xe8, 209));
    c.receive(0xaa, 210);
    assert!(!c.admit(0xe8, 211));
    c.receive(0, 212);
    assert!(c.admit(0xe8, 213));
}
#[test]
fn lost_owner_is_replaced_only_after_a_fresh_complete_reset() {
    let mut old = Commands::new();
    assert!(old.admit(0xe8, 0));
    let mut replacement = Commands::new();
    assert!(replacement.admit(0xff, 1));
    replacement.receive(0xfa, 2);
    replacement.receive(0x08, 3);
    assert!(!replacement.admit(3, 4));
    assert!(!replacement.admit(0xe8, 4));
    replacement.receive(0xaa, 5);
    replacement.receive(0, 6);
    assert!(replacement.admit(0xe8, 7));
}
#[test]
fn led_exchange_routes_only_responses_and_bounds_resends() {
    let mut l = Lamps::new();
    l.request(true);
    assert_eq!(l.next(0), Some(0xed));
    assert!(!l.receive(0x1e));
    assert!(l.receive(0xfe));
    assert_eq!(l.next(1), Some(0xed));
    l.receive(0xfa);
    assert_eq!(l.next(2), Some(4));
    assert_eq!(l.next(3), None);
    l.receive(0xfa);
    assert_eq!(l.wait_ms(4), None);
    l.request(false);
    assert_eq!(l.next(5), Some(0xed));
    l.receive(0xfa);
    assert_eq!(l.next(6), Some(0));
    l.receive(0xfa);
    assert_eq!(l.wait_ms(7), None);
}
#[test]
fn silent_lamp_quarantines_ambiguous_boundary_without_blocking_keyboard() {
    let mut l = Lamps::new();
    l.request(true);
    assert_eq!(l.next(0), Some(0xed));
    assert_eq!(l.next(201), None);
    l.request(false);
    l.request(true);
    assert_eq!(l.next(500), None);
    assert!(!l.receive(0x1e));
    assert_eq!(l.wait_ms(501), None);
    l.receive(0xfa);
    assert_eq!(l.next(600), None);
}
#[test]
fn pointer_wire_preserves_ps2_nine_bit_motion_and_rejects_authority_extensions() {
    let b = response(Event::Motion {
        dx: -255,
        dy: 255,
        buttons: 7,
    });
    assert!(valid_response(&b));
    let mut bad = b;
    bad[4] = 8;
    assert!(!valid_response(&bad));
    bad = b;
    bad[7] = 1;
    assert!(!valid_response(&bad));
    assert!(valid_response(&response(Event::Restarted(0xff))));
    assert!(!valid_response(&response(Event::Send(0xd1))));
    let mut c = Clock::new();
    assert_eq!(c.accept(u32::MAX - 1), u32::MAX as u64 - 1);
    assert_eq!(c.accept(3), (1u64 << 32) + 3);
}

#[test]
fn untrusted_client_cannot_flood_device_commands_or_reset_forever() {
    let mut c = Commands::new();
    for n in 0..32 {
        assert!(c.admit(0xf4, n * 2));
        c.receive(0xfa, n * 2 + 1);
    }
    assert!(!c.admit(0xf4, 100));
    assert!(c.admit(0xf4, 1000));
    c.receive(0xfa, 1001);
    for n in 0..3 {
        assert!(c.admit(0xff, 2000 + n * 10));
        c.receive(0xfa, 2001 + n * 10);
        c.receive(0xaa, 2002 + n * 10);
        c.receive(0, 2003 + n * 10);
    }
    assert!(!c.admit(0xff, 9000));
}

#[test]
fn absent_empty_and_damaged_channels_are_distinct() {
    use osaiku_driver_runtime::ps2::output;
    assert_eq!(output(0xff), None);
    assert_eq!(output(0), None);
    assert_eq!(output(0x20), None);
    assert_eq!(output(1), Some((false, false)));
    assert_eq!(output(0x21), Some((true, false)));
    assert_eq!(output(0x81), Some((false, true)));
    assert_eq!(output(0x61), Some((true, true)));
}

#[test]
fn exhausted_argument_resend_never_replays_a_new_led_command() {
    let mut l = Lamps::new();
    l.request(true);
    l.next(0);
    l.receive(0xfa);
    assert_eq!(l.next(1), Some(4));
    for now in [2, 3] {
        l.receive(0xfe);
        assert_eq!(l.next(now), Some(4));
    }
    l.receive(0xfe);
    l.request(false);
    assert_eq!(l.next(4), None);
    assert_eq!(l.wait_ms(4), None);
}

#[test]
fn replacement_receives_complete_extended_sequence_without_phantom_left_control() {
    use osaiku_driver_runtime::{
        hid::{KeyEvent, Poll},
        ps2::{keyboard, KeyboardFrames},
        set1::Decoder,
    };
    let mut frames = KeyboardFrames::new();
    assert_eq!(frames.accept(0xe0, false, 1), None);
    // Owner changes here; the controller retains framing, never semantic keys.
    let mut replacement = Decoder::new();
    let record = frames.accept(0x1d, false, 2).unwrap();
    assert_eq!(
        keyboard(&mut replacement, &record),
        Poll::Event(KeyEvent {
            usage: 0xe4,
            pressed: true
        })
    );
    for byte in [0xe1, 0x1d, 0x45, 0xe1, 0x9d] {
        assert_eq!(frames.accept(byte, false, 3), None);
    }
    assert_eq!(
        keyboard(&mut replacement, &frames.accept(0xc5, false, 4).unwrap()),
        Poll::Ignored
    );
    assert_eq!(frames.accept(0xe0, false, 5), None);
    assert_eq!(
        keyboard(&mut replacement, &frames.accept(0x1d, false, 600).unwrap()),
        Poll::Lost
    );
    assert_eq!(
        keyboard(&mut replacement, &[8, 0, 0, 0, 0, 0, 0, 0]),
        Poll::Lost
    );
}
