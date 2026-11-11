//! Nonmodal help: a `ui-tooltip` leaf paints a question mark in normal flow,
//! and its text above the view after a quiet hover or keyboard focus.
use crate::{
    font, input,
    layout::{Layout, Rect},
    state, Document, Kind,
};

pub const ACTION: u32 = crate::symbol("ui-tooltip");
pub const DELAY_MS: u64 = 500;
pub const FONT: u16 = 14;
pub const LINE_HEIGHT: u32 = 20;
pub const PAD: u32 = 12;

/// Application-side transient state. Never persists to disk or holds input.
pub struct State {
    candidate: u32,
    due: Option<u64>,
    visible: bool,
    anchor: Option<(Rect, u64)>,
}
impl State {
    pub const EMPTY: Self = Self {
        candidate: 0,
        due: None,
        visible: false,
        anchor: None,
    };
    pub fn active(&self) -> bool {
        self.candidate != 0
    }
    pub fn cancel(&mut self) -> bool {
        let changed = self.visible;
        *self = Self::EMPTY;
        changed
    }
    /// Call only for authenticated events admitted against a retained view.
    pub fn event(&mut self, e: &input::Event, now: u64) -> bool {
        if e.kind == input::HOVER && e.value == 1 {
            if self.candidate == e.element {
                return false;
            }
            let changed = self.cancel();
            self.candidate = e.element;
            self.due = Some(now.saturating_add(DELAY_MS));
            return changed;
        }
        if matches!(e.kind, input::FOCUS | input::ACTIVATE) && e.action == ACTION {
            let changed = !self.visible || self.candidate != e.element;
            self.candidate = e.element;
            self.due = None;
            self.visible = true;
            self.anchor = None;
            return changed;
        }
        self.cancel()
    }
    pub fn tick(&mut self, now: u64) -> bool {
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            self.visible = true;
            true
        } else {
            false
        }
    }
    /// No periodic wake after the tooltip appears or while nothing is hovered.
    pub fn interval_us(&self, now: u64) -> Option<u64> {
        self.due
            .map(|due| due.saturating_sub(now).saturating_mul(1000).max(1))
    }
    /// Apply to a bound document before encoding/rasterization. Geometry, text,
    /// hiding or a modal menu invalidate help from an earlier presentation.
    pub fn apply(&mut self, view: &mut Document, layout: &Layout) {
        let candidate = view.nodes[..view.len as usize]
            .iter()
            .enumerate()
            .find(|(i, n)| {
                let p = layout.nodes[*i];
                n.kind == Kind::Tooltip
                    && n.id == self.candidate
                    && !p.hidden
                    && !p.disabled
                    && p.clip.width != 0
                    && p.clip.height != 0
                    && layout
                        .modal_popup(view)
                        .is_none_or(|m| Layout::popup_of(view, *i) == Some(m))
            })
            .map(|(i, _)| {
                (
                    layout.nodes[i].clip,
                    crate::wire_hash(view.text_of(i).as_bytes()),
                )
            });
        if candidate.is_none() || self.anchor.is_some_and(|old| Some(old) != candidate) {
            self.cancel();
        } else {
            self.anchor = candidate;
        }
        for n in &mut view.nodes[..view.len as usize] {
            if n.kind == Kind::Tooltip {
                n.state &= !state::SELECTED;
                if self.visible && n.id == self.candidate {
                    n.state |= state::SELECTED;
                }
            }
        }
    }
}

/// Tracks transitions only; repeated motion on the same help icon is silent.
#[derive(Clone, Copy)]
pub struct Hover {
    pub element: u32,
}
impl Hover {
    pub const EMPTY: Self = Self { element: 0 };
    pub fn leave(&mut self, view: &Document, target: crate::Identity) -> Option<input::Event> {
        let element = core::mem::take(&mut self.element);
        if element == 0 {
            return None;
        }
        let event = input::Event {
            target,
            element,
            kind: input::HOVER,
            value: 0,
            action: 0,
        };
        event.validate(target, view).ok().map(|_| event)
    }
    pub fn event(
        &mut self,
        view: &Document,
        layout: &Layout,
        target: crate::Identity,
        point: Option<(i32, i32)>,
    ) -> Option<input::Event> {
        let next = point
            .and_then(|(x, y)| layout.hit(view, x, y))
            .filter(|i| view.nodes[*i].kind == Kind::Tooltip)
            .map_or(0, |i| view.nodes[i].id);
        self.transition(view, target, next)
    }
    /// `next` comes from the trusted presented layout, zero means no help.
    pub fn transition(
        &mut self,
        view: &Document,
        target: crate::Identity,
        next: u32,
    ) -> Option<input::Event> {
        if next == self.element {
            return None;
        }
        let previous = self.element;
        self.element = next;
        let event = input::Event {
            target,
            element: if next == 0 { previous } else { next },
            kind: input::HOVER,
            value: i32::from(next != 0),
            action: 0,
        };
        event.validate(target, view).ok().map(|_| event)
    }
}

/// Help hit testing directly on already validated, sealed semantics. This is
/// the same layer/clip policy as Layout::hit, without expanding or allocating
/// a Layout for each physical mouse sample. Never use unvalidated metadata.
pub fn hit_elements(view: &Document, elements: &[crate::surface::Element], x: i32, y: i32) -> u32 {
    use crate::surface::{DISABLED, HIDDEN};
    let len = view.len as usize;
    if elements.len() != len {
        return 0;
    }
    let contains = |i: usize| {
        let r = elements[i].bounds;
        Rect {
            x: r.x as i32,
            y: r.y as i32,
            width: r.width,
            height: r.height,
        }
        .contains(x, y)
    };
    let open = |i: usize| {
        view.nodes[i].kind == Kind::Popup
            && elements[i].state & HIDDEN == 0
            && elements[i].bounds.width != 0
            && elements[i].bounds.height != 0
    };
    let layer = (0..len).rev().find(|i| open(*i) && contains(*i));
    if layer.is_none()
        && (0..len)
            .any(|i| open(i) && view.nodes[i].action != 0 && elements[i].state & DISABLED == 0)
    {
        return 0;
    }
    (0..len)
        .rev()
        .find(|i| {
            view.nodes[*i].action != 0
                && elements[*i].state & (HIDDEN | DISABLED) == 0
                && contains(*i)
                && Layout::popup_of(view, *i) == layer
                && Some(*i) != layer
        })
        .filter(|i| view.nodes[*i].kind == Kind::Tooltip)
        .map_or(0, |i| view.nodes[i].id)
}

/// Word wrapping shared by overlay measurement and paint, including long words.
#[derive(Clone)]
pub struct Lines<'a> {
    rest: &'a str,
    width: u32,
}
pub fn lines(text: &str, width: u32) -> Lines<'_> {
    Lines { rest: text, width }
}
impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        self.rest = self.rest.trim_start();
        if self.rest.is_empty() {
            return None;
        }
        let mut end = self.rest.len();
        let mut space = 0;
        let mut advance = 0u32;
        for (i, c) in self.rest.char_indices() {
            if c == '\n' {
                end = i;
                break;
            }
            if c.is_whitespace() {
                space = i;
            }
            advance += font::ui_text_glyph(c, FONT).map_or(0, |g| g.advance());
            if (advance + 32) / 64 > self.width {
                end = if space > 0 {
                    space
                } else if i > 0 {
                    i
                } else {
                    c.len_utf8()
                };
                break;
            }
        }
        let (line, rest) = self.rest.split_at(end);
        self.rest = rest;
        Some(line.trim_end())
    }
}
pub fn bounds(anchor: Rect, viewport: Rect, text: &str) -> Rect {
    let mx = 8.min(viewport.width / 2);
    let my = 8.min(viewport.height / 2);
    let width = 360.min(viewport.width.saturating_sub(2 * mx));
    let height = (lines(text, width.saturating_sub(2 * PAD)).count() as u32 * LINE_HEIGHT
        + 2 * PAD)
        .min(viewport.height.saturating_sub(2 * my));
    let x = anchor.x.clamp(
        viewport.x + mx as i32,
        viewport.right() - width as i32 - mx as i32,
    );
    let below = anchor.bottom() + 6;
    let y = if below + height as i32 <= viewport.bottom() - my as i32 {
        below
    } else {
        anchor.y - height as i32 - 6
    };
    Rect {
        x,
        y: y.clamp(
            viewport.y + my as i32,
            viewport.bottom() - height as i32 - my as i32,
        ),
        width,
        height,
    }
}
