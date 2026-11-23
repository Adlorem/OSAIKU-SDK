#![cfg(feature = "compiler")]
//! `ui-switch`: an on/off switch in the theme accent, on while selected.
use osaiku_ui::{
    compiler::compile,
    input::{self, Key},
    layout::Rect,
    paint::Scene,
    text_model::{self, Model, Text},
    theme::Theme,
    *,
};

const HTML: &str = r#"<main id="root"><ui-switch id="wrap" action="wrap" bind:selected="wrap-on"></ui-switch><ui-list-item id="entry" action="entry"><span id="label">Zawijanie</span><ui-switch id="shown" bind:selected="wrap-on"></ui-switch></ui-list-item></main>"#;
const CSS: &str = "main { padding:0px; gap:8px; } ui-list-item { flex-direction:row; height:32px; gap:8px; } #label { width:120px; }";
const IDENTITY: Identity = Identity {
    epoch: 1,
    owner: 8,
    window: 4,
    incarnation: 2,
    revision: 7,
};
const W: u32 = 240;
const H: u32 = 80;

struct On(bool);
impl Model for On {
    fn text(&self, _: u32, _: &mut Text) -> core::fmt::Result {
        Err(core::fmt::Error)
    }
    fn flag(&self, key: u32) -> Option<bool> {
        (key == symbol("wrap-on")).then_some(self.0)
    }
}
fn at(d: &Document, id: &str) -> usize {
    d.nodes[..d.len as usize]
        .iter()
        .position(|n| n.id == symbol(id))
        .unwrap()
}
fn document(on: bool) -> Box<Document> {
    let template = compile(HTML, CSS).unwrap();
    let mut d = Box::new(Document::EMPTY);
    text_model::bind_into(&template, &On(on), &mut d).unwrap();
    d
}
/// The painted BGRA pixel at (x, y).
fn pixel(scene: &Scene<'_, layout::Layout>, x: i32, y: i32) -> u32 {
    let mut px = vec![0u8; (W * 4) as usize];
    raster::rows(scene, y as u32, &mut px).unwrap();
    let p = &px[x as usize * 4..x as usize * 4 + 4];
    u32::from_le_bytes([p[0], p[1], p[2], 0])
}
fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
}

#[test]
fn the_catalog_has_a_switch_that_acts_like_a_button() {
    let c = catalog::component("ui-switch").unwrap();
    assert_eq!((c.kind, c.role), (Kind::Switch, Role::Button));
    assert!(catalog::actionable(Kind::Switch));
    let d = document(true);
    let i = at(&d, "wrap");
    assert_eq!(d.nodes[i].state & state::SELECTED, state::SELECTED);
    let mut wire = vec![0; MAX_WIRE];
    let n = d.encode(&mut wire).unwrap();
    let decoded = Document::decode(&wire[..n]).unwrap();
    assert_eq!(
        (decoded.nodes[i].kind, decoded.nodes[i].role),
        (Kind::Switch, Role::Button)
    );
    // 44x24 unless CSS says otherwise.
    let scene = Scene::from_document(&d, W, H).unwrap();
    let b = scene.layout().nodes[i].bounds;
    assert_eq!((b.width, b.height), (44, 24));
}

#[test]
fn on_is_the_desktop_accent_at_the_right_and_off_a_grey_knob_at_the_left() {
    for accent in [None, Some(0), Some(1), Some(2), Some(3)] {
        for theme in [Theme::Dark, Theme::Light] {
            let accent_rgb = theme.accented(theme::ACCENT, 0, accent) & 0x00ff_ffff;
            let muted = theme.accented(theme::MUTED, 0, accent) & 0x00ff_ffff;
            for on in [false, true] {
                let d = document(on);
                let scene = Scene::from_document(&d, W, H)
                    .unwrap()
                    .with_theme(theme)
                    .with_accent(accent);
                let i = at(&d, "wrap");
                let g = switch::geometry(scene.layout().nodes[i].bounds).unwrap();
                let (x, y) = centre(g.knob(on));
                assert_eq!(pixel(&scene, x, y), if on { accent_rgb } else { muted });
                // The other end shows the track, not a knob.
                let (x, y) = centre(g.knob(!on));
                let track = pixel(&scene, x, y);
                assert_ne!(track, accent_rgb);
                assert_ne!(track, muted);
            }
        }
    }
}

#[test]
fn a_press_toggles_through_the_action_and_a_switch_in_an_entry_lets_the_entry_act() {
    let d = document(false);
    let scene = Scene::from_document(&d, W, H).unwrap();
    let (x, y) = centre(scene.layout().nodes[at(&d, "wrap")].bounds);
    let e = input::pointer_event(&d, &scene, IDENTITY, x, y).unwrap();
    assert_eq!(
        (e.kind, e.element, e.action),
        (input::ACTIVATE, symbol("wrap"), symbol("wrap"))
    );
    // Without an action the switch only shows the entry's state.
    let (x, y) = centre(scene.layout().nodes[at(&d, "shown")].bounds);
    let e = input::pointer_event(&d, &scene, IDENTITY, x, y).unwrap();
    assert_eq!((e.element, e.action), (symbol("entry"), symbol("entry")));
    // Keyboard: Tab reaches the switch with an action and Enter activates it.
    let mut d = document(false);
    d.set_focus(symbol("entry"));
    let scene = Scene::from_document(&d, W, H).unwrap();
    let back = input::key_event(&d, &scene, IDENTITY, Key::Previous).unwrap();
    assert_eq!(back.element, symbol("wrap"));
    d.set_focus(symbol("wrap"));
    let scene = Scene::from_document(&d, W, H).unwrap();
    let enter = input::key_event(&d, &scene, IDENTITY, Key::Activate).unwrap();
    assert_eq!(
        (enter.kind, enter.element, enter.action),
        (input::ACTIVATE, symbol("wrap"), symbol("wrap"))
    );
}

#[test]
fn a_disabled_switch_is_faded_and_sends_nothing() {
    let mut d = document(true);
    let i = at(&d, "wrap");
    let lit = {
        let scene = Scene::from_document(&d, W, H)
            .unwrap()
            .with_theme(Theme::Dark);
        let g = switch::geometry(scene.layout().nodes[i].bounds).unwrap();
        let (x, y) = centre(g.on);
        pixel(&scene, x, y)
    };
    d.nodes[i].state |= state::DISABLED;
    let scene = Scene::from_document(&d, W, H)
        .unwrap()
        .with_theme(Theme::Dark);
    let g = switch::geometry(scene.layout().nodes[i].bounds).unwrap();
    let (x, y) = centre(g.on);
    assert_ne!(pixel(&scene, x, y), lit);
    assert!(input::pointer_event(&d, &scene, IDENTITY, x, y).is_none());
}
