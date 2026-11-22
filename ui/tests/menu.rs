#![cfg(feature = "compiler")]
//! The menu bar: titles open popups beneath them; items keep accelerators.
use osaiku_ui::{
    compiler::compile,
    input::{self, Key},
    layout::Layout,
    menu::{self, Menu, Outcome},
    paint::Scene,
    text_model::{self, Model, Text},
    *,
};

const HTML: &str = r#"<main id="root"><nav class="menu-bar" id="menu-bar"><button class="menu-title" id="menu-file" action="menu-file" bind:selected="file-menu-open">Plik</button><button class="menu-title" id="menu-edit" action="menu-edit" bind:selected="edit-menu-open">Edycja</button></nav><ui-text-area id="editor" action="editor"></ui-text-area><button id="undo-button" action="undo-visible" bind:hidden="undo-button-hidden">Cofnij</button><ui-popup-menu class="menu-popup" id="file-menu" action="menu-close" bind:hidden="file-menu-hidden"><ui-list-item class="menu-entry" id="item-save" action="save"><span class="menu-entry-label" id="item-save-label">Zapisz</span><span class="menu-entry-keys" id="item-save-keys">Ctrl+S</span></ui-list-item><ui-list-item class="menu-entry" id="item-off" action="find" bind:disabled="off"><span class="menu-entry-label" id="item-off-label">Off</span></ui-list-item></ui-popup-menu><ui-popup-menu class="menu-popup" id="edit-menu" action="menu-close" bind:hidden="edit-menu-hidden"><ui-list-item class="menu-entry" id="item-undo" action="undo"><span class="menu-entry-label" id="item-undo-label">Cofnij</span></ui-list-item><ui-list-item class="menu-entry" id="item-redo" action="redo"><span class="menu-entry-label" id="item-redo-label">Ponów</span></ui-list-item></ui-popup-menu></main>"#;
const CSS: &str = "main { padding:0px; } #editor { height:120px; }";
const MENUS: [Menu; 2] = [Menu::named("file"), Menu::named("edit")];
const IDENTITY: Identity = Identity {
    epoch: 1,
    owner: 8,
    window: 4,
    incarnation: 2,
    revision: 7,
};

struct App(menu::State);
impl Model for App {
    fn text(&self, _: u32, _: &mut Text) -> core::fmt::Result {
        Err(core::fmt::Error)
    }
    fn flag(&self, key: u32) -> Option<bool> {
        if key == symbol("off") || key == symbol("undo-button-hidden") {
            return Some(true);
        }
        self.0.flag(&MENUS, key)
    }
}
fn bound(state: menu::State) -> Document {
    let template = compile(HTML, CSS).unwrap();
    let mut d = Document::EMPTY;
    text_model::bind_into(&template, &App(state), &mut d).unwrap();
    d
}
fn press(state: &mut menu::State, action: &str) -> Outcome {
    let event = input::Event {
        target: IDENTITY,
        element: symbol(action),
        kind: input::ACTIVATE,
        value: 0,
        action: symbol(action),
    };
    state.event(&MENUS, &event)
}

#[test]
fn a_title_toggles_its_menu_and_switches_from_another() {
    let mut state = menu::State::CLOSED;
    assert_eq!(press(&mut state, "menu-file"), Outcome::Changed);
    assert_eq!(state.open(), Some(0));
    assert_eq!(state.flag(&MENUS, symbol("file-menu-hidden")), Some(false));
    assert_eq!(state.flag(&MENUS, symbol("edit-menu-hidden")), Some(true));
    assert_eq!(state.flag(&MENUS, symbol("file-menu-open")), Some(true));
    assert_eq!(press(&mut state, "menu-edit"), Outcome::Changed);
    assert_eq!(state.open(), Some(1));
    assert_eq!(press(&mut state, "menu-edit"), Outcome::Changed);
    assert_eq!(state.open(), None);
    // Choosing an item closes the menu and reports its action.
    press(&mut state, "menu-file");
    assert_eq!(press(&mut state, "save"), Outcome::Command(symbol("save")));
    assert_eq!(state.open(), None);
    // The dismissal closes; other input is the application's.
    press(&mut state, "menu-file");
    assert_eq!(press(&mut state, "menu-close"), Outcome::Changed);
    assert_eq!(press(&mut state, "menu-close"), Outcome::Ignored);
    assert_eq!(press(&mut state, "editor"), Outcome::Ignored);
}

#[test]
fn the_open_menu_is_placed_under_its_title() {
    let mut state = menu::State::CLOSED;
    press(&mut state, "menu-edit");
    let mut d = bound(state);
    let mut layout = Layout::EMPTY;
    Layout::compute_into(&d, 400, 300, &[0; MAX_NODES], &mut layout).unwrap();
    state.place(&MENUS, &mut d, &layout);
    let title = layout.nodes[d
        .nodes
        .iter()
        .position(|n| n.id == symbol("menu-edit"))
        .unwrap()]
    .bounds;
    let popup = d
        .nodes
        .iter()
        .position(|n| n.id == symbol("edit-menu"))
        .unwrap();
    assert_eq!(
        d.nodes[popup].popup_anchor(),
        Some((title.x as u32, title.bottom() as u32))
    );
    let scene = Scene::from_document(&d, 400, 300).unwrap();
    let placed = scene.layout().nodes[popup].bounds;
    assert_eq!((placed.x, placed.y), (title.x, title.bottom()));
}

#[test]
fn accelerators_reach_items_in_closed_menus_as_commands() {
    let d = bound(menu::State::CLOSED);
    let scene = Scene::from_document(&d, 400, 300).unwrap();
    let key = |name: &str| input::key_event(&d, &scene, IDENTITY, Key::Shortcut(symbol(name)));
    let save = key("save").unwrap();
    assert_eq!(
        (save.kind, save.element, save.action),
        (input::COMMAND, symbol("item-save"), symbol("save"))
    );
    let mut state = menu::State::CLOSED;
    assert_eq!(state.event(&MENUS, &save), Outcome::Command(symbol("save")));
    // A disabled item, or one no menu lists, takes nothing.
    assert!(key("find").is_none());
    assert!(key("print").is_none());
    // A visible control with the action is preferred: none here is visible.
    assert_eq!(key("undo").unwrap().kind, input::COMMAND);
    // A forged command for a visible control or a disabled item is refused.
    let forged = input::Event {
        element: symbol("editor"),
        action: symbol("editor"),
        ..save
    };
    assert!(forged.validate(IDENTITY, &d).is_err());
    let disabled = input::Event {
        element: symbol("item-off"),
        action: symbol("find"),
        ..save
    };
    assert!(disabled.validate(IDENTITY, &d).is_err());
}

#[test]
fn an_open_menu_holds_accelerators_to_its_own_items_and_arrows_move_in_it() {
    let mut state = menu::State::CLOSED;
    press(&mut state, "menu-edit");
    let mut d = bound(state);
    let mut layout = Layout::EMPTY;
    Layout::compute_into(&d, 400, 300, &[0; MAX_NODES], &mut layout).unwrap();
    state.place(&MENUS, &mut d, &layout);
    let scene = Scene::from_document(&d, 400, 300).unwrap();
    let key = |k| input::key_event(&d, &scene, IDENTITY, k);
    let undo = key(Key::Shortcut(symbol("undo"))).unwrap();
    assert_eq!(
        (undo.kind, undo.element),
        (input::ACTIVATE, symbol("item-undo"))
    );
    // Another menu's item waits until this one closes.
    assert!(key(Key::Shortcut(symbol("save"))).is_none());
    let down = key(Key::Edit(osaiku_ui::text_area::edit::DOWN)).unwrap();
    assert_eq!(
        (down.kind, down.element),
        (input::FOCUS, symbol("item-undo"))
    );
}
