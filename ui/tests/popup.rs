#![cfg(feature = "compiler")]
//! `ui-popup-menu`: a menu drawn above the document at an anchor point.
use osaiku_ui::{
    compiler::compile,
    input::{self, Key},
    layout::Rect,
    paint::{Painter, Scene},
    text_model::{self, Model, Text},
    *,
};

const HTML: &str = r#"<main id="root"><section id="list"><button id="row-a" action="open-a">A</button><button id="row-b" action="open-b">B</button><section id="tall"><p id="filler">.</p></section></section><ui-popup-menu id="menu" action="close" bind:hidden="closed"><ui-list-item id="cut" action="cut"><span id="cut-label">Wytnij</span></ui-list-item><ui-list-item id="copy" action="copy"><span id="copy-label">Kopiuj</span></ui-list-item></ui-popup-menu></main>"#;
const CSS: &str = "main { padding:0px; } #list { padding:10px; gap:4px; overflow:auto; height:200px; } button { height:30px; } #tall { height:600px; } #menu { width:160px; padding:4px; gap:2px; border-radius:6px; background-color:theme(card); } ui-list-item { height:28px; padding:4px; }";

fn at(d: &Document, id: &str) -> usize {
    d.nodes[..d.len as usize]
        .iter()
        .position(|n| n.id == symbol(id))
        .unwrap()
}
fn menu(open: bool, x: u32, y: u32) -> Document {
    let mut d = compile(HTML, CSS).unwrap();
    let m = at(&d, "menu");
    d.nodes[m].set_popup_anchor(x, y);
    if !open {
        d.nodes[m].state |= state::HIDDEN;
    }
    d
}
const IDENTITY: Identity = Identity {
    epoch: 1,
    owner: 8,
    window: 4,
    incarnation: 2,
    revision: 7,
};

#[test]
fn a_popup_takes_no_space_and_opens_at_its_anchor_inside_the_viewport() {
    let closed = menu(false, 0, 0);
    let open = menu(true, 100, 50);
    let a = Scene::from_document(&closed, 400, 300).unwrap();
    let b = Scene::from_document(&open, 400, 300).unwrap();
    // Siblings keep their places whether the menu is open or not.
    for id in ["list", "row-a", "row-b"] {
        assert_eq!(
            a.layout().nodes[at(&closed, id)].bounds,
            b.layout().nodes[at(&open, id)].bounds
        );
    }
    let m = b.layout().nodes[at(&open, "menu")];
    // 2 items of 28 px, one 2 px gap, 4 px padding on both sides.
    assert_eq!(
        m.bounds,
        Rect {
            x: 100,
            y: 50,
            width: 160,
            height: 66
        }
    );
    assert_eq!(m.clip, m.bounds);
    // Near the right/bottom edges it opens to the left of / above the point.
    let edge = menu(true, 390, 290);
    let s = Scene::from_document(&edge, 400, 300).unwrap();
    let r = s.layout().nodes[at(&edge, "menu")].bounds;
    assert_eq!((r.x, r.y), (390 - 160, 290 - 66));
    // A point larger than the viewport is clamped, never placed outside.
    let far = menu(true, 4096, 4096);
    let s = Scene::from_document(&far, 400, 300).unwrap();
    let r = s.layout().nodes[at(&far, "menu")].bounds;
    assert!(r.right() <= 400 && r.bottom() <= 300 && r.x >= 0 && r.y >= 0);
}

#[test]
fn an_open_menu_holds_the_pointer_and_closes_on_a_press_elsewhere() {
    let d = menu(true, 20, 12);
    let s = Scene::from_document(&d, 400, 300).unwrap();
    let cut = s.layout().nodes[at(&d, "cut")].clip;
    // Over the menu: its item, never the row underneath.
    assert_eq!(s.hit(cut.x + 2, cut.y + 2), Some(symbol("cut")));
    // Its padding is part of the menu but activates nothing.
    let m = s.layout().nodes[at(&d, "menu")].bounds;
    assert_eq!(s.hit(m.x + 1, m.y + 1), None);
    // Elsewhere the menu itself answers: its action dismisses it.
    let row_b = s.layout().nodes[at(&d, "row-b")].clip;
    assert_eq!(s.hit(row_b.right() - 2, row_b.y + 2), Some(symbol("menu")));
    let event = input::pointer_event(&d, &s, IDENTITY, row_b.right() - 2, row_b.y + 2).unwrap();
    assert_eq!(
        (event.element, event.action),
        (symbol("menu"), symbol("close"))
    );
    // A context click names what lies beneath, so a second right-click
    // retargets the menu; Escape and popup_dismiss close it.
    let under = input::context_event(&d, &s, IDENTITY, row_b.right() - 2, row_b.y + 2).unwrap();
    assert_eq!(under.action, symbol("open-b"));
    let under = input::context_event(&d, &s, IDENTITY, cut.x + 2, cut.y + 2);
    assert_ne!(under.map(|e| e.action), Some(symbol("cut")));
    let close = input::popup_dismiss(&d, &s, IDENTITY).unwrap();
    assert_eq!(close.action, symbol("close"));
    let escape = input::key_event(&d, &s, IDENTITY, Key::Cancel).unwrap();
    assert_eq!(
        (escape.kind, escape.action),
        (input::ACTIVATE, symbol("close"))
    );
    // No scroll gesture starts under an open menu, not even on the track.
    let list = at(&d, "list");
    let bar = s.scrollbar(list).unwrap();
    let mut pointer = input::Pointer::EMPTY;
    let pressed = pointer.event(
        &d,
        &s,
        IDENTITY,
        input::PointerPhase::Press,
        bar.track.x + 2,
        bar.track.bottom() - 4,
    );
    assert_eq!(pressed.map(|e| e.kind), Some(input::ACTIVATE));
    assert!(!pointer.captured());
    // Focus cycles among the menu's items only.
    let next = s.layout().focus_next(&d, None, false).unwrap();
    let after = s.layout().focus_next(&d, Some(next), false).unwrap();
    for i in [next, after] {
        assert!(["cut", "copy"].iter().any(|id| d.nodes[i].id == symbol(id)));
    }
}

#[test]
fn a_hidden_menu_changes_nothing() {
    let d = menu(false, 20, 12);
    let s = Scene::from_document(&d, 400, 300).unwrap();
    let row_a = s.layout().nodes[at(&d, "row-a")].clip;
    assert_eq!(s.hit(row_a.x + 2, row_a.y + 2), Some(symbol("row-a")));
    assert!(input::popup_dismiss(&d, &s, IDENTITY).is_none());
    assert!(input::key_event(&d, &s, IDENTITY, Key::Cancel).is_none());
}

#[derive(Default)]
struct Fills(Vec<(Rect, u32, u8)>);
impl Painter for Fills {
    fn fill_alpha(&mut self, rect: Rect, color: u32, alpha: u8, _: Rect) {
        self.0.push((rect, color, alpha));
    }
    fn text(&mut self, _: Rect, _: &str, _: u16, _: u32, _: Rect) {}
    fn line(&mut self, _: (i32, i32), _: (i32, i32), _: u32, _: Rect) {}
}

#[test]
fn the_menu_and_its_shadow_are_painted_above_the_document() {
    let d = menu(true, 20, 12);
    let s = Scene::from_document(&d, 400, 300).unwrap();
    let mut fills = Fills::default();
    s.paint(&mut fills);
    let m = s.layout().nodes[at(&d, "menu")].bounds;
    // The list's scrollbar thumb (accent) comes before any menu pixel.
    let accent = theme::Theme::Dark.color(theme::ACCENT, 0) & 0xffffff;
    let thumb = fills.0.iter().rposition(|f| f.1 == accent).unwrap();
    let shadow = fills
        .0
        .iter()
        .position(|f| f.1 == 0 && f.2 < 40 && f.0.y > m.y - 8)
        .unwrap();
    assert!(thumb < shadow);
    let card = theme::Theme::Dark.color(4, 0) & 0xffffff;
    let body = fills.0.iter().rposition(|f| f.1 == card).unwrap();
    assert!(shadow < body);
}

#[test]
fn popups_cannot_scroll_or_live_in_list_rows() {
    let scroll = compile(HTML, &format!("{CSS} #menu {{ overflow:auto; }}"));
    assert!(scroll.unwrap_err().message.contains("cannot scroll"));
    let in_row = compile(
        r#"<main id="r"><ui-virtual-list id="l" source="s"><template><ui-list-item id="i" action="a"><ui-popup-menu id="m" action="c"></ui-popup-menu></ui-list-item></template></ui-virtual-list></main>"#,
        "",
    );
    assert!(in_row.is_err());
    // The anchor survives the wire.
    let d = menu(true, 1234, 567);
    let mut wire = vec![0; MAX_WIRE];
    let len = d.encode(&mut wire).unwrap();
    let back = Document::decode(&wire[..len]).unwrap();
    assert_eq!(
        back.nodes[at(&back, "menu")].popup_anchor(),
        Some((1234, 567))
    );
    let mut bad = d.clone();
    let m = at(&bad, "menu");
    bad.nodes[m].scroll_y = 5000;
    assert!(bad.validate().is_err());
}

struct Progress;
impl Model for Progress {
    fn text(&self, _: u32, _: &mut Text) -> core::fmt::Result {
        Err(core::fmt::Error)
    }
    fn flag(&self, _: u32) -> Option<bool> {
        None
    }
    fn value(&self, key: u32) -> Option<Option<u16>> {
        (key == symbol("done")).then_some(Some(4250))
    }
}

#[test]
fn text_models_bind_value_bars() {
    let t = compile(
        r#"<main id="r"><ui-value-bar id="bar" bind:value="done"></ui-value-bar></main>"#,
        "",
    )
    .unwrap();
    let mut d = Document::EMPTY;
    text_model::bind_into(&t, &Progress, &mut d).unwrap();
    assert_eq!(d.nodes[at(&d, "bar")].value, 4250);
    assert!(text_model::matches(&t, &d, &Progress));
    d.nodes[1].value = 4251;
    assert!(!text_model::matches(&t, &d, &Progress));
    let unknown = compile(
        r#"<main id="r"><ui-value-bar id="bar" bind:value="other"></ui-value-bar></main>"#,
        "",
    )
    .unwrap();
    assert!(text_model::bind_into(&unknown, &Progress, &mut d).is_err());
}
