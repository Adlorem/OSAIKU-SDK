#![cfg(feature = "compiler")]
use osaiku_ui::{
    compiler::compile,
    input::{self, Event, Key},
    layout::Rect,
    paint::{Painter, Scene},
    tooltip, *,
};
const ID: Identity = Identity {
    epoch: 1,
    owner: 2,
    window: 3,
    incarnation: 4,
    revision: 5,
};
const HTML: &str = r#"<main id="root"><div id="row"><ui-tooltip id="help">Dłuższa podpowiedź: niższa temperatura nie gwarantuje prawdziwej odpowiedzi.</ui-tooltip><button id="action" action="go">Dalej</button></div></main>"#;
fn doc() -> Document {
    compile(
        HTML,
        "#row { flex-direction:row; height:30px; } button { width:100px; }",
    )
    .unwrap()
}
fn index(d: &Document) -> usize {
    d.nodes[..d.len as usize]
        .iter()
        .position(|n| n.id == symbol("help"))
        .unwrap()
}
fn enter() -> Event {
    Event {
        target: ID,
        element: symbol("help"),
        kind: input::HOVER,
        value: 1,
        action: 0,
    }
}
fn apply(d: &mut Document, state: &mut tooltip::State) {
    let layout = Scene::from_document(d, 400, 180).unwrap().layout().clone();
    state.apply(d, &layout);
}
#[test]
fn wire_and_validation_keep_help_inert_except_for_its_sdk_action() {
    let mut d = doc();
    let mut wire = vec![0; MAX_WIRE];
    let len = d.encode(&mut wire).unwrap();
    assert_eq!(d, Document::decode(&wire[..len]).unwrap());
    enter().validate(ID, &d).unwrap();
    assert!(Event {
        element: symbol("action"),
        ..enter()
    }
    .validate(ID, &d)
    .is_err());
    assert!(Event {
        value: 2,
        ..enter()
    }
    .validate(ID, &d)
    .is_err());
    assert!(enter()
        .validate(Identity { revision: 6, ..ID }, &d)
        .is_err());
    let i = index(&d);
    d.nodes[i].action = symbol("execute");
    assert!(d.validate().is_err());
    assert!(compile(r#"<button id="run" action="ui-tooltip">Run</button>"#, "").is_err());
    assert!(compile(
        r#"<ui-tooltip id="help" bind:selected="open">Help</ui-tooltip>"#,
        ""
    )
    .is_err());
}

#[test]
fn reports_do_not_need_a_business_action_for_help() {
    let manifest = osaiku_ui::report::parse_manifest("report 1 11\n").unwrap();
    osaiku_ui::compiler::compile_report(
        r#"<main id="root"><ui-tooltip id="help">Help text</ui-tooltip></main>"#,
        "",
        &manifest.contract(),
    )
    .unwrap();
}

#[test]
fn hover_coalesces_only_consecutive_hover_events_and_preserves_clicks() {
    let d = doc();
    let mut queue = input::Queue::EMPTY;
    queue.push_event(enter(), ID, &d).unwrap();
    let leave = Event {
        value: 0,
        ..enter()
    };
    queue.push_event(leave, ID, &d).unwrap();
    let click = Event {
        target: ID,
        element: symbol("action"),
        kind: input::ACTIVATE,
        action: symbol("go"),
        value: 0,
    };
    queue.push_event(click, ID, &d).unwrap();
    queue.push_event(enter(), ID, &d).unwrap();
    assert_eq!(queue.take_event(&[(ID, &d)]), Some(leave));
    assert_eq!(queue.take_event(&[(ID, &d)]), Some(click));
    assert_eq!(queue.take_event(&[(ID, &d)]), Some(enter()));
    assert_eq!(queue.take_event(&[(ID, &d)]), None);
}

#[test]
fn compact_hover_hit_testing_matches_scene_geometry_and_popup_occlusion() {
    let html = r#"<main id="root"><div id="row"><ui-tooltip id="help">Help</ui-tooltip><button id="go" action="go">Go</button></div><ui-popup-menu id="menu" action="close"><ui-tooltip id="inner">Menu help</ui-tooltip></ui-popup-menu></main>"#;
    for mode in 0..5 {
        let mut d = compile(
            html,
            "#row { flex-direction:row; height:40px; } #menu { width:100px; padding:8px; }",
        )
        .unwrap();
        let menu = d.nodes[..d.len as usize]
            .iter()
            .position(|n| n.id == symbol("menu"))
            .unwrap();
        d.nodes[menu].set_popup_anchor(100, 50);
        match mode {
            0 => d.nodes[menu].state = state::HIDDEN,
            1 => d.nodes[menu].state = state::DISABLED,
            2 => {}
            3 => d.nodes[menu].action = 0,
            _ => d.nodes[0].state = state::HIDDEN,
        }
        let scene = Scene::from_document(&d, 220, 160).unwrap();
        let elements: Vec<_> = scene
            .semantics()
            .map(|s| surface::Element {
                id: s.id,
                action: s.action,
                state: u32::from(s.state & (state::HIDDEN | state::DISABLED)),
                bounds: surface::Rect {
                    x: s.visible_bounds.x as u32,
                    y: s.visible_bounds.y as u32,
                    width: s.visible_bounds.width,
                    height: s.visible_bounds.height,
                },
                ..surface::Element::default()
            })
            .collect();
        for y in (0..160).step_by(3) {
            for x in (0..220).step_by(3) {
                let expected = scene
                    .layout()
                    .hit(&d, x, y)
                    .filter(|i| d.nodes[*i].kind == Kind::Tooltip)
                    .map_or(0, |i| d.nodes[i].id);
                assert_eq!(
                    tooltip::hit_elements(&d, &elements, x, y),
                    expected,
                    "mode={mode}, ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn tooltip_pixels_do_not_depend_on_transfer_band_boundaries() {
    let mut d = doc();
    let i = index(&d);
    d.nodes[i].state |= state::SELECTED;
    let scene = Scene::from_document(&d, 400, 180).unwrap();
    let render = |rows: usize| {
        let mut bytes = vec![0; 400 * 180 * 4];
        for (i, band) in bytes.chunks_mut(400 * 4 * rows).enumerate() {
            raster::rows(&scene, (i * rows) as u32, band).unwrap();
        }
        bytes
    };
    let maximum = surface::MAX_TRANSFER / (400 * 4);
    assert_eq!(render(maximum), render(32));
    assert_eq!(render(maximum), render(17));
}
#[test]
fn hover_delays_once_then_sleeps_and_leaving_or_clicking_cancels() {
    let mut d = doc();
    let mut state = tooltip::State::EMPTY;
    assert!(!state.event(&enter(), 100));
    assert!(!state.event(&enter(), 400));
    assert_eq!(state.interval_us(400), Some(200_000));
    assert!(!state.tick(599));
    assert!(state.tick(600));
    apply(&mut d, &mut state);
    assert_ne!(d.nodes[index(&d)].state & state::SELECTED, 0);
    assert_eq!(state.interval_us(600), None);
    assert!(state.event(
        &Event {
            value: 0,
            ..enter()
        },
        601
    ));
    apply(&mut d, &mut state);
    assert_eq!(d.nodes[index(&d)].state & state::SELECTED, 0);
    state.event(&enter(), 700);
    state.event(
        &Event {
            kind: input::SCROLL,
            ..enter()
        },
        750,
    );
    assert!(!state.tick(1500));
}
#[test]
fn keyboard_focus_opens_and_escape_closes_without_activating_an_app_command() {
    let mut d = doc();
    let mut state = tooltip::State::EMPTY;
    let scene = Scene::from_document(&d, 400, 180).unwrap();
    let event = input::key_event(&d, &scene, ID, Key::Next).unwrap();
    assert_eq!(
        (event.element, event.action, event.kind),
        (symbol("help"), tooltip::ACTION, input::FOCUS)
    );
    assert!(state.event(&event, 10));
    apply(&mut d, &mut state);
    let scene = Scene::from_document(&d, 400, 180).unwrap();
    let escape = input::key_event(&d, &scene, ID, Key::Cancel).unwrap();
    assert_eq!((escape.kind, escape.value), (input::HOVER, 0));
    assert!(state.event(&escape, 11));
}
#[test]
fn host_hover_emits_only_transitions_and_does_not_hold_underlying_controls() {
    let d = doc();
    let scene = Scene::from_document(&d, 400, 180).unwrap();
    let p = scene.layout().nodes[index(&d)].clip;
    let mut hover = tooltip::Hover::EMPTY;
    assert_eq!(
        hover.event(&d, scene.layout(), ID, Some((p.x + 2, p.y + 2))),
        Some(enter())
    );
    assert_eq!(
        hover.event(&d, scene.layout(), ID, Some((p.x + 3, p.y + 3))),
        None
    );
    assert_eq!(hover.leave(&d, ID).unwrap().value, 0);
    assert_eq!(hover.leave(&d, ID), None);
    let mut d = d;
    let i = index(&d);
    d.nodes[i].state |= state::SELECTED;
    let scene = Scene::from_document(&d, 400, 180).unwrap();
    assert_eq!(scene.layout().modal_popup(&d), None);
    let button = scene
        .semantics()
        .find(|n| n.id == symbol("action"))
        .unwrap()
        .bounds;
    assert_eq!(
        scene.hit(button.x + 2, button.y + 2),
        Some(symbol("action"))
    );
}
#[test]
fn hidden_scrolled_moved_or_reworded_help_does_not_reuse_old_hover() {
    for change in 0..4 {
        let mut d = doc();
        let mut s = tooltip::State::EMPTY;
        s.event(&enter(), 0);
        s.tick(500);
        apply(&mut d, &mut s);
        let i = index(&d);
        match change {
            0 => d.nodes[0].state |= state::HIDDEN,
            1 => d.nodes[0].state |= state::DISABLED,
            2 => {
                d.nodes[0].style.padding = 20;
                d.nodes[0].narrow.padding = 20;
            }
            _ => d.text[d.nodes[i].text_start as usize] = b'X',
        }
        apply(&mut d, &mut s);
        assert_eq!(d.nodes[i].state & state::SELECTED, 0, "change {change}");
        assert_eq!(s.interval_us(600), None);
    }
}
#[derive(Default)]
struct Recorder {
    text: Vec<(Rect, String, Rect)>,
}
impl Painter for Recorder {
    fn fill_alpha(&mut self, _: Rect, _: u32, _: u8, _: Rect) {}
    fn fill(&mut self, _: Rect, _: u32, _: Rect) {}
    fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
    fn text(&mut self, r: Rect, t: &str, _: u16, _: u32, c: Rect) {
        self.text.push((r, t.into(), c));
    }
}
#[test]
fn wrapped_overlay_is_last_inside_viewport_and_does_not_change_layout() {
    let mut d = doc();
    let i = index(&d);
    let layout = Scene::from_document(&d, 400, 180).unwrap().layout().clone();
    let mut p = Recorder::default();
    Scene::from_document(&d, 400, 180).unwrap().paint(&mut p);
    assert!(!p.text.iter().any(|(_, s, _)| s.contains("Dłuższa")));
    d.nodes[i].state |= state::SELECTED;
    let scene = Scene::from_document(&d, 400, 180).unwrap();
    assert_eq!(scene.layout().nodes[i].bounds, layout.nodes[i].bounds);
    let mut p = Recorder::default();
    scene.paint(&mut p);
    let start = p
        .text
        .iter()
        .position(|(_, t, _)| t.starts_with("Dłuższa"))
        .unwrap();
    assert!(start > p.text.iter().position(|(_, t, _)| t == "Dalej").unwrap());
    for (r, t, clip) in &p.text[start..] {
        assert_eq!(r.intersect(*clip), *r);
        assert!(font::ui_font_text_width(t, tooltip::FONT) <= r.width);
    }
    for (x, y) in [(0, 0), (390, 0), (390, 170), (0, 170)] {
        let b = tooltip::bounds(
            Rect {
                x,
                y,
                width: 10,
                height: 10,
            },
            scene.layout().viewport,
            d.text_of(i),
        );
        assert_eq!(b.intersect(scene.layout().viewport), b);
    }
    let text = "żółć bardzo-długie-słowo-bez-spacji 你好";
    let rebuilt = tooltip::lines(text, 15).collect::<Vec<_>>().join("");
    assert_eq!(rebuilt, text.replace(' ', ""));
}
