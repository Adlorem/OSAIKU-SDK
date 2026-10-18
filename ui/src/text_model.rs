//! Bounded formatted text bindings for application views and trusted dialogs.
use core::fmt::{self, Write};
pub struct Text {
    bytes: [u8; 256],
    len: usize,
}
impl Text {
    pub const EMPTY: Self = Self {
        bytes: [0; 256],
        len: 0,
    };
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}
impl Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.len + s.len() > self.bytes.len() {
            return Err(fmt::Error);
        }
        self.bytes[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}
pub trait Model {
    fn text(&self, key: u32, out: &mut Text) -> fmt::Result;
    fn flag(&self, key: u32) -> Option<bool>;
}

/// Verify a text-only recipe against an authoritative template and model.
/// Focus and scrolling may vary; labels, actions, layout, visibility and enabled
/// state cannot be rewritten by an untrusted transport. No allocation is used.
pub fn matches(template: &crate::Document, document: &crate::Document, model: &impl Model) -> bool {
    if template.validate().is_err()
        || document.validate().is_err()
        || template.len != document.len
        || template.point_len != 0
        || document.point_len != 0
        || template.state_style_len != document.state_style_len
        || template.state_styles != document.state_styles
        || template.decoration_len != document.decoration_len
        || template.decorations != document.decorations
        || template.list_len != document.list_len
        || template.lists != document.lists
    {
        return false;
    }
    let mut text = Text::EMPTY;
    for i in 0..template.len as usize {
        let mut expected = template.nodes[i];
        let actual = document.nodes[i];
        if expected.bindings[1] != 0 || expected.bindings[2] != 0 {
            return false;
        }
        text.clear();
        let label = if expected.bindings[0] == 0 {
            template.text_of(i)
        } else {
            if model.text(expected.bindings[0], &mut text).is_err() {
                return false;
            }
            text.as_str()
        };
        if label != document.text_of(i) {
            return false;
        }
        expected.text_start = actual.text_start;
        expected.text_len = actual.text_len;
        for (slot, flag) in [
            (3, crate::state::HIDDEN),
            (4, crate::state::DISABLED),
            (5, crate::state::SELECTED),
        ] {
            if expected.bindings[slot] != 0 {
                let Some(value) = model.flag(expected.bindings[slot]) else {
                    return false;
                };
                expected.state = (expected.state & !flag) | if value { flag } else { 0 };
            }
        }
        expected.state |= actual.state & crate::state::FOCUSED;
        if expected.style.overflow == 2 || expected.narrow.overflow == 2 {
            expected.scroll_y = actual.scroll_y;
        }
        if expected != actual {
            return false;
        }
    }
    true
}

/// Bind text and boolean state without a Document-sized stack temporary.
pub fn bind_into(
    template: &crate::Document,
    model: &impl Model,
    out: &mut crate::Document,
) -> Result<(), crate::Error> {
    template.validate()?;
    if template.point_len != 0 {
        return Err(crate::Error::Binding);
    }
    out.nodes.copy_from_slice(&template.nodes);
    out.state_styles.copy_from_slice(&template.state_styles);
    out.state_style_len = template.state_style_len;
    out.decorations.copy_from_slice(&template.decorations);
    out.decoration_len = template.decoration_len;
    out.lists.copy_from_slice(&template.lists);
    out.list_len = template.list_len;
    out.len = template.len;
    out.point_len = 0;
    out.points.fill(crate::Point::EMPTY);
    out.text_len = 0;
    out.text.fill(0);
    let mut text = Text::EMPTY;
    for i in 0..template.len as usize {
        let node = &mut out.nodes[i];
        if node.bindings[1] != 0 || node.bindings[2] != 0 {
            return Err(crate::Error::Binding);
        }
        text.clear();
        let label = if node.bindings[0] == 0 {
            template.text_of(i)
        } else {
            model
                .text(node.bindings[0], &mut text)
                .map_err(|_| crate::Error::Binding)?;
            text.as_str()
        };
        let start = out.text_len as usize;
        if start + label.len() > out.text.len() {
            return Err(crate::Error::Limit);
        }
        out.text[start..start + label.len()].copy_from_slice(label.as_bytes());
        node.text_start = out.text_len;
        node.text_len = label.len() as u16;
        out.text_len += node.text_len;
        for (slot, flag) in [
            (3, crate::state::HIDDEN),
            (4, crate::state::DISABLED),
            (5, crate::state::SELECTED),
        ] {
            if node.bindings[slot] != 0 {
                let value = model
                    .flag(node.bindings[slot])
                    .ok_or(crate::Error::Binding)?;
                node.state = (node.state & !flag) | if value { flag } else { 0 };
            }
        }
    }
    out.validate()
}
