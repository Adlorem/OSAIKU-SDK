#![cfg(feature = "compiler")]
//! `ui-slider`: a press or drag sends a VALUE event; the thumb shows `bind:value`.
use osaiku_ui::{
    compiler::compile,
    input::{self, Key, PointerPhase::*},
    paint::Scene,
    *,
};

const HTML: &str = r#"<main id="root"><button id="before" action="before">A</button><ui-slider id="level" action="level" bind:value="level-value"></ui-slider><button id="after" action="after">B</button></main>"#;
const CSS: &str =
    "main { padding:0px; gap:0px; } button { height:30px; } #level { height:24px; padding:0px; }";
const IDENTITY: Identity = Identity {
    epoch: 1,
    owner: 8,
    window: 4,
    incarnation: 2,
    revision: 7,
};

fn at(d: &Document, id: &str) -> usize {
    d.nodes[..d.len as usize]
        .iter()
        .position(|n| n.id == symbol(id))
        .unwrap()
}
fn slider(value: u16) -> Document {
    let mut d = compile(HTML, CSS).unwrap();
    let i = at(&d, "level");
    d.nodes[i].value = value;
    d
}
/// The slider's thumb travel in window pixels: (start, end, y).
fn travel(d: &Document, scene: &Scene<'_, layout::Layout>) -> (i32, i32, i32) {
    let p = scene.layout().nodes[at(d, "level")];
    let g = osaiku_ui::slider::geometry(p.bounds).unwrap();
    (g.start, g.start + g.travel as i32, p.bounds.y + 10)
}

#[test]
fn the_catalog_has_a_slider_with_an_action_and_a_percent_binding() {
    let c = catalog::component("ui-slider").unwrap();
    assert_eq!((c.kind, c.role), (Kind::Slider, Role::Slider));
    assert!(catalog::actionable(Kind::Slider));
    assert!(catalog::binding("bind:value")
        .unwrap()
        .accepts(Kind::Slider));
    let d = slider(2500);
    let i = at(&d, "level");
    assert_eq!(d.nodes[i].action, symbol("level"));
    // The wire carries the new kind and role.
    let mut wire = vec![0; MAX_WIRE];
    let n = d.encode(&mut wire).unwrap();
    let decoded = Document::decode(&wire[..n]).unwrap();
    assert_eq!(decoded.nodes[i].kind, Kind::Slider);
    assert_eq!(decoded.nodes[i].role, Role::Slider);
}

#[test]
fn a_press_sends_the_position_under_the_pointer() {
    let d = slider(0);
    let scene = Scene::from_document(&d, 220, 100).unwrap();
    let (start, end, y) = travel(&d, &scene);
    // Under the thumb's half beyond either end, the value is that end.
    for (x, expected) in [
        (start - 5, 0),
        (start, 0),
        ((start + end) / 2, 5000),
        (end, 10000),
        (end + 5, 10000),
    ] {
        let e = input::pointer_event(&d, &scene, IDENTITY, x, y).unwrap();
        assert_eq!(
            (e.kind, e.element, e.action, e.value),
            (input::VALUE, symbol("level"), symbol("level"), expected),
            "x={x}"
        );
    }
}

#[test]
fn a_drag_follows_the_pointer_outside_the_slider_until_release() {
    let d = slider(0);
    let scene = Scene::from_document(&d, 220, 100).unwrap();
    let (start, end, y) = travel(&d, &scene);
    let mut pointer = input::Pointer::EMPTY;
    let press = pointer
        .event(&d, &scene, IDENTITY, Press, start, y)
        .unwrap();
    assert_eq!((press.kind, press.value), (input::VALUE, 0));
    assert!(pointer.captured());
    let middle = pointer
        .event(&d, &scene, IDENTITY, Move, (start + end) / 2, y + 300)
        .unwrap();
    assert_eq!(middle.value, 5000);
    // An unchanged position is not sent again.
    assert!(pointer
        .event(&d, &scene, IDENTITY, Move, (start + end) / 2, y)
        .is_none());
    // A newer revision of the same window keeps the gesture.
    let newer = Identity {
        revision: 8,
        ..IDENTITY
    };
    let far = pointer
        .event(&d, &scene, newer, Move, end + 500, -40)
        .unwrap();
    assert_eq!((far.value, far.target), (10000, newer));
    assert!(pointer
        .event(&d, &scene, newer, Release, end + 500, y)
        .is_none());
    assert!(!pointer.captured());
    assert!(pointer.event(&d, &scene, newer, Move, start, y).is_none());
    // Another incarnation never inherits the gesture.
    pointer
        .event(&d, &scene, IDENTITY, Press, start, y)
        .unwrap();
    let other = Identity {
        incarnation: 3,
        ..IDENTITY
    };
    assert!(pointer.event(&d, &scene, other, Move, end, y).is_none());
    assert!(!pointer.captured());
}

#[test]
fn a_disabled_slider_sends_nothing_and_tab_does_not_stop_on_a_slider() {
    let mut d = slider(5000);
    let i = at(&d, "level");
    d.nodes[i].state |= state::DISABLED;
    let scene = Scene::from_document(&d, 220, 100).unwrap();
    let (start, end, y) = travel(&d, &scene);
    assert!(input::pointer_event(&d, &scene, IDENTITY, (start + end) / 2, y).is_none());
    let mut pointer = input::Pointer::EMPTY;
    assert!(pointer
        .event(&d, &scene, IDENTITY, Press, start, y)
        .is_none());
    assert!(!pointer.captured());

    let mut d = slider(5000);
    d.set_focus(symbol("before"));
    let scene = Scene::from_document(&d, 220, 100).unwrap();
    let next = input::key_event(&d, &scene, IDENTITY, Key::Next).unwrap();
    assert_eq!(next.element, symbol("after"));
}

#[test]
fn value_events_are_checked_and_coalesce_in_the_queue() {
    let d = slider(5000);
    let event = |element: &str, value: i32, action: &str| input::Event {
        target: IDENTITY,
        element: symbol(element),
        kind: input::VALUE,
        value,
        action: if action.is_empty() { 0 } else { symbol(action) },
    };
    assert!(event("level", 10000, "level")
        .validate(IDENTITY, &d)
        .is_ok());
    for refused in [
        event("level", 10001, "level"),
        event("level", -1, "level"),
        event("level", 5000, ""),
        event("level", 5000, "before"),
        event("before", 5000, "before"),
    ] {
        assert!(refused.validate(IDENTITY, &d).is_err(), "{refused:?}");
    }
    let mut queue = input::Queue::EMPTY;
    for value in [1000, 2000, 3000] {
        queue
            .push_event(event("level", value, "level"), IDENTITY, &d)
            .unwrap();
    }
    let taken = queue.take_event(&[(IDENTITY, &d)]).unwrap();
    assert_eq!(taken.value, 3000);
    assert!(queue.take_event(&[(IDENTITY, &d)]).is_none());
}

#[test]
fn the_thumb_is_drawn_where_a_press_reads_the_same_value() {
    let content = layout::Rect {
        x: 10,
        y: 4,
        width: 230,
        height: 18,
    };
    let g = osaiku_ui::slider::geometry(content).unwrap();
    // Two thirds of the box: the box stays the press target.
    assert_eq!(g.knob, 12);
    assert_eq!(g.track.height, 4);
    for value in [0u16, 1, 2500, 5000, 7300, 9999, 10000] {
        let thumb = g.thumb(value);
        assert!(thumb.x >= content.x && thumb.right() <= content.right());
        let back = g.value(g.centre(value));
        // One pixel of travel is 10000/218 basis points.
        assert!(
            back.abs_diff(value) <= 10000 / g.travel as u16,
            "{value} -> {back}"
        );
    }
    assert!(osaiku_ui::slider::geometry(layout::Rect {
        width: 12,
        ..content
    })
    .is_none());
}
