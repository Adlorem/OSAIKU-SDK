//! A menu bar: titles in a row, each opening one `ui-popup-menu` beneath it.
//!
//! The markup, for a menu named `file`:
//!
//! ```html
//! <nav class="menu-bar" id="menu-bar">
//!   <button class="menu-title" id="menu-file" action="menu-file" bind:selected="file-menu-open">Plik</button>
//! </nav>
//! <ui-popup-menu class="menu-popup" id="file-menu" action="menu-close" bind:hidden="file-menu-hidden">
//!   <ui-list-item class="menu-entry" id="item-save" action="save">
//!     <span class="menu-entry-label" id="item-save-label">Zapisz</span>
//!     <span class="menu-entry-keys" id="item-save-keys">Ctrl+S</span>
//!   </ui-list-item>
//! </ui-popup-menu>
//! ```
//!
//! The application keeps one [`State`], answers the two bindings through
//! [`State::flag`] and places the open menu with [`State::place`] once it has
//! laid out the bound document. Items keep their accelerators while their
//! menu is closed: the keyboard's command reaches them as `input::COMMAND`,
//! which [`State::event`] reports like a click.
use crate::{input, layout::Layout, symbol, Document};

/// FNV-1a over consecutive parts: `symbol` of their concatenation.
const fn symbol_of(parts: &[&str]) -> u32 {
    let mut hash = 2166136261u32;
    let mut p = 0;
    while p < parts.len() {
        let bytes = parts[p].as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            hash = (hash ^ bytes[i] as u32).wrapping_mul(16777619);
            i += 1;
        }
        p += 1;
    }
    hash
}

/// One menu of the bar, by the names the markup above derives from `name`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    /// Id and action of the title button: `menu-NAME`.
    pub title: u32,
    /// Id of the popup: `NAME-menu`.
    pub popup: u32,
    /// `bind:hidden` of the popup: `NAME-menu-hidden`.
    pub hidden: u32,
    /// `bind:selected` of the title: `NAME-menu-open`.
    pub open: u32,
}
impl Menu {
    pub const fn named(name: &str) -> Self {
        Self {
            title: symbol_of(&["menu-", name]),
            popup: symbol_of(&[name, "-menu"]),
            hidden: symbol_of(&[name, "-menu-hidden"]),
            open: symbol_of(&[name, "-menu-open"]),
        }
    }
}
/// The popups' shared dismissal action: a press outside the open menu, or
/// Escape, activates it.
pub const CLOSE: u32 = symbol("menu-close");

/// What an event meant for the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not the bar's: the application handles the event itself.
    Ignored,
    /// A menu opened, switched or closed; nothing else to do but redraw.
    Changed,
    /// An item was chosen, by click or accelerator: do its action. An open
    /// menu has been closed.
    Command(u32),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    open: Option<u8>,
}
impl State {
    pub const CLOSED: Self = Self { open: None };
    pub fn open(&self) -> Option<usize> {
        self.open.map(usize::from)
    }
    pub fn close(&mut self) -> bool {
        self.open.take().is_some()
    }
    /// Interpret an activation or accelerator. Only activations and
    /// commands are considered; other input is `Ignored`.
    pub fn event(&mut self, menus: &[Menu], event: &input::Event) -> Outcome {
        match event.kind {
            input::COMMAND if event.action != 0 => {
                self.open = None;
                Outcome::Command(event.action)
            }
            input::ACTIVATE => {
                if let Some(i) = menus.iter().position(|m| m.title == event.action) {
                    self.open = if self.open() == Some(i) {
                        None
                    } else {
                        Some(i as u8)
                    };
                    return Outcome::Changed;
                }
                if event.action == CLOSE {
                    return if self.close() {
                        Outcome::Changed
                    } else {
                        Outcome::Ignored
                    };
                }
                if self.open.is_some() && event.action != 0 {
                    self.open = None;
                    return Outcome::Command(event.action);
                }
                Outcome::Ignored
            }
            _ => Outcome::Ignored,
        }
    }
    /// The bar's bindings: each popup hidden unless open, its title selected
    /// while open. None for any other key.
    pub fn flag(&self, menus: &[Menu], key: u32) -> Option<bool> {
        menus.iter().enumerate().find_map(|(i, m)| {
            let open = self.open() == Some(i);
            if key == m.hidden {
                Some(!open)
            } else if key == m.open {
                Some(open)
            } else {
                None
            }
        })
    }
    /// Anchor the open menu's popup at the bottom left of its title, from a
    /// layout of this same bound document. Layout keeps it in the viewport.
    pub fn place(&self, menus: &[Menu], view: &mut Document, layout: &Layout) {
        let Some(menu) = self.open().and_then(|i| menus.get(i)) else {
            return;
        };
        let nodes = &view.nodes[..usize::from(view.len)];
        let title = nodes.iter().position(|n| n.id == menu.title);
        let popup = nodes.iter().position(|n| n.id == menu.popup);
        if let (Some(title), Some(popup)) = (title, popup) {
            let bounds = layout.nodes[title].bounds;
            view.nodes[popup]
                .set_popup_anchor(bounds.x.max(0) as u32, bounds.bottom().max(0) as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_follow_the_markup_convention() {
        let m = Menu::named("file");
        assert_eq!(m.title, symbol("menu-file"));
        assert_eq!(m.popup, symbol("file-menu"));
        assert_eq!(m.hidden, symbol("file-menu-hidden"));
        assert_eq!(m.open, symbol("file-menu-open"));
    }
}
