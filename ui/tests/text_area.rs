#![cfg(feature = "compiler")]
//! `ui-text-area`: the application's shown lines with a caret, a selection
//! and a position strip; editing keys, presses and drags reach it as events.
use osaiku_ui::{
    compiler::compile,
    input::{self, Key, PointerPhase::*},
    paint::{Painter, Scene},
    text_area::{self, edit},
    *,
};

const HTML: &str = r#"<main id="root"><button id="before" action="before">A</button><ui-text-area id="editor" action="editor"></ui-text-area></main>"#;
const CSS: &str = "main { padding:0px; gap:0px; } button { height:30px; } #editor { height:200px; padding:4px; font-size:16px; }";
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
fn area(text: &str, packed: u32, value: u16, focused: bool) -> Document {
    let mut d = compile(HTML, CSS).unwrap();
    let i = at(&d, "editor");
    d.set_text(i, text).unwrap();
    d.nodes[i].scroll_y = packed;
    d.nodes[i].value = value;
    if focused {
        assert!(d.set_focus(symbol("editor")));
    }
    d.validate().unwrap();
    d
}
/// Top left of the text box in window pixels.
fn origin(d: &Document, scene: &Scene<'_, layout::Layout>) -> (i32, i32) {
    let i = at(d, "editor");
    let p = scene.layout().nodes[i];
    let r = p.bounds.inset(d.insets(scene.layout().style(d, i)));
    (r.x, r.y)
}

#[test]
fn the_catalog_has_an_actionable_text_box_with_a_larger_text_bound() {
    let c = catalog::component("ui-text-area").unwrap();
    assert_eq!((c.kind, c.role), (Kind::TextArea, Role::TextBox));
    assert!(catalog::actionable(Kind::TextArea));
    let long = "x".repeat(MAX_AREA_TEXT);
    let d = area(&long, text_area::pack(0, 0, true), 10001, false);
    let mut wire = vec![0; MAX_WIRE];
    let n = d.encode(&mut wire).unwrap();
    let decoded = Document::decode(&wire[..n]).unwrap();
    let i = at(&d, "editor");
    assert_eq!(decoded.nodes[i].kind, Kind::TextArea);
    assert_eq!(decoded.text_of(i).len(), MAX_AREA_TEXT);
    // Other leaves keep their bound, and the area its own.
    let mut d = compile(HTML, CSS).unwrap();
    assert_eq!(d.set_text(at(&d, "before"), &long[..300]), Err(Error::Text));
    assert_eq!(
        d.set_text(at(&d, "editor"), &"x".repeat(MAX_AREA_TEXT + 1)),
        Err(Error::Text)
    );
}

#[test]
fn caret_and_anchor_must_fall_on_shown_character_boundaries() {
    let mut d = area("zażółć", text_area::pack(2, 0, true), 10001, false);
    let i = at(&d, "editor");
    // Inside the two bytes of 'ż'.
    d.nodes[i].scroll_y = text_area::pack(3, 0, true);
    assert_eq!(d.validate(), Err(Error::Value));
    d.nodes[i].scroll_y = text_area::pack(0, 40, true);
    assert_eq!(d.validate(), Err(Error::Value));
    d.nodes[i].scroll_y = text_area::PACKED_MAX + 1;
    assert_eq!(d.validate(), Err(Error::Value));
    // An area never scrolls as a box: its scroll_y is its caret.
    assert!(compile(HTML, "#editor { overflow:auto; }").is_err());
}

#[test]
fn keys_reach_a_focused_area_as_editing_and_text_events() {
    let d = area("ab\ncd", text_area::pack(1, 1, true), 10001, true);
    let scene = Scene::from_document(&d, 300, 300).unwrap();
    let key = |k| input::key_event(&d, &scene, IDENTITY, k);
    let e = key(Key::Edit(edit::LEFT | edit::SHIFT)).unwrap();
    assert_eq!(
        (e.kind, e.element, e.value, e.action),
        (input::EDIT, symbol("editor"), edit::LEFT | edit::SHIFT, 0)
    );
    // Enter breaks the line, Tab inserts a tab, pages move the caret.
    assert_eq!(key(Key::Activate).unwrap().value, edit::NEWLINE);
    assert_eq!(key(Key::Next).unwrap().value, edit::TAB);
    assert_eq!(key(Key::PageDown).unwrap().value, edit::PAGE_DOWN);
    assert_eq!(key(Key::Previous).unwrap().kind, input::FOCUS);
    let space = key(Key::Character(' ')).unwrap();
    assert_eq!((space.kind, space.value), (input::TEXT, ' ' as i32));
    assert_eq!(key(Key::Backspace).unwrap().kind, input::ERASE);
    // Unknown editing values are refused.
    assert!(key(Key::Edit(0)).is_none());
    // Without focus the area takes no editing keys.
    let unfocused = area("ab", text_area::pack(0, 0, true), 10001, false);
    let scene = Scene::from_document(&unfocused, 300, 300).unwrap();
    assert!(input::key_event(&unfocused, &scene, IDENTITY, Key::Edit(edit::LEFT)).is_none());
}

#[test]
fn a_press_puts_the_caret_and_a_drag_selects_until_release() {
    let text = "first line\nsecond";
    let d = area(text, text_area::pack(0, 0, true), 10001, false);
    let scene = Scene::from_document(&d, 300, 300).unwrap();
    let (x0, y0) = origin(&d, &scene);
    let line = text_area::line_height(16) as i32;
    let mut pointer = input::Pointer::EMPTY;
    let press = pointer
        .event(&d, &scene, IDENTITY, Press, x0 - 2, y0 + line + 2)
        .unwrap();
    assert_eq!((press.kind, press.value), (input::CARET, 11));
    assert!(pointer.captured());
    let end = text_area::x_of("second", 6, 16);
    let drag = pointer
        .event(&d, &scene, IDENTITY, Move, x0 + end + 40, y0 + line + 2)
        .unwrap();
    assert_eq!((drag.kind, drag.value), (input::SELECT, text.len() as i32));
    // The same offset is not sent twice; dragging above reaches the first row.
    assert!(pointer
        .event(&d, &scene, IDENTITY, Move, x0 + end + 50, y0 + line + 3)
        .is_none());
    let up = pointer
        .event(&d, &scene, IDENTITY, Move, x0 - 30, y0 - 50)
        .unwrap();
    assert_eq!(up.value, 0);
    assert!(pointer
        .event(&d, &scene, IDENTITY, Release, x0, y0)
        .is_none());
    assert!(!pointer.captured());
    // Offsets outside the shown part never validate.
    let bad = input::Event {
        value: text.len() as i32 + 1,
        ..press
    };
    assert_eq!(bad.validate(IDENTITY, &d), Err(Error::Value));
}

#[test]
fn the_position_strip_sends_values_and_follows_a_drag() {
    let text = "a\nb\nc";
    let d = area(text, text_area::pack(0, 0, true), 0, false);
    let scene = Scene::from_document(&d, 300, 300).unwrap();
    let i = at(&d, "editor");
    let p = scene.layout().nodes[i];
    let content = p.bounds.inset(d.insets(scene.layout().style(&d, i)));
    let g = text_area::geometry(content, 0);
    let track = g.track.unwrap();
    let mut pointer = input::Pointer::EMPTY;
    let press = pointer
        .event(&d, &scene, IDENTITY, Press, track.x + 4, track.y)
        .unwrap();
    assert_eq!(
        (press.kind, press.value, press.action),
        (input::VALUE, 0, symbol("editor"))
    );
    let bottom = pointer
        .event(
            &d,
            &scene,
            IDENTITY,
            Move,
            track.x + 200,
            track.bottom() + 90,
        )
        .unwrap();
    assert_eq!((bottom.kind, bottom.value), (input::VALUE, 10000));
    pointer.event(&d, &scene, IDENTITY, Release, 0, 0);
    // Without a position the strip is not drawn and a press there is text.
    let fits = area(text, text_area::pack(0, 0, true), 10001, false);
    let scene = Scene::from_document(&fits, 300, 300).unwrap();
    let e = input::pointer_event(&fits, &scene, IDENTITY, track.x + 4, track.y).unwrap();
    assert_eq!(e.kind, input::CARET);
}

#[derive(Default)]
struct Record {
    fills: Vec<(layout::Rect, u32, u8)>,
    texts: Vec<(i32, i32, String)>,
}
impl Painter for Record {
    fn fill_alpha(&mut self, rect: layout::Rect, color: u32, alpha: u8, clip: layout::Rect) {
        let r = rect.intersect(clip);
        if r.width != 0 && r.height != 0 {
            self.fills.push((r, color, alpha));
        }
    }
    fn text(&mut self, rect: layout::Rect, text: &str, _: u16, _: u32, _: layout::Rect) {
        self.texts.push((rect.x, rect.y, text.into()));
    }
    fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: layout::Rect) {}
}

#[test]
fn paint_draws_rows_runs_the_selection_and_a_caret_only_with_focus() {
    let text = "ab\tc\nde";
    let line = text_area::line_height(16) as i32;
    for focused in [false, true] {
        let d = area(text, text_area::pack(7, 1, true), 10001, focused);
        let scene = Scene::from_document(&d, 300, 300).unwrap();
        let (x0, y0) = origin(&d, &scene);
        let mut record = Record::default();
        scene.paint(&mut record);
        let rows: Vec<_> = record
            .texts
            .iter()
            .filter(|(x, y, _)| *x >= x0 && *y >= y0)
            .map(|(x, y, t)| (x - x0, (y - y0) / line, t.as_str()))
            .collect();
        let stop = text_area::x_of("ab\tc", 3, 16);
        assert_eq!(rows, [(0, 0, "ab"), (stop, 0, "c"), (0, 1, "de")]);
        // The selection covers from 'b' to the end of the second row.
        let selection = record
            .fills
            .iter()
            .filter(|(r, _, _)| r.y == y0 && r.height == line as u32)
            .count();
        assert_eq!(selection, 1);
        let caret = record.fills.iter().any(|(r, _, _)| {
            r.width == 2 && r.y == y0 + line && r.x == x0 + text_area::x_of("de", 2, 16)
        });
        assert_eq!(caret, focused);
    }
}

#[test]
fn semantics_carry_a_bounded_label_and_no_percent() {
    let long = "ż".repeat(1000);
    let d = area(&long, text_area::pack(0, 0, true), 5000, false);
    let scene = Scene::from_document(&d, 300, 300).unwrap();
    let s = scene
        .semantics()
        .find(|s| s.id == symbol("editor"))
        .unwrap();
    assert!(s.text.len() <= MAX_LEAF_TEXT && s.text.len() >= MAX_LEAF_TEXT - 1);
    assert_eq!(s.value, None);
}
