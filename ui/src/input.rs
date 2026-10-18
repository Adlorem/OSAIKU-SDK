//! Bounded owner event queue. Admission never grants input authority.
use crate::{Action, Document, Error, Identity};
pub const CAPACITY: usize = 16;

pub const ACTIVATE: u32 = 1;
pub const FOCUS: u32 = 2;
pub const SCROLL: u32 = 3;
pub const TEXT: u32 = 4;
pub const ERASE: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum PointerPhase {
    Press = 0,
    Move = 1,
    Release = 2,
    Cancel = 3,
}
impl PointerPhase {
    pub fn decode(value: u64) -> Option<Self> {
        Some(match value {
            0 => Self::Press,
            1 => Self::Move,
            2 => Self::Release,
            3 => Self::Cancel,
            _ => return None,
        })
    }
}
#[derive(Clone, Copy)]
struct ScrollCapture {
    target: Identity,
    element: u32,
    track: crate::layout::Rect,
    grab: i32,
}
/// A scroll gesture may follow new content revisions of the same window, but
/// never a new owner/incarnation, another element, or changed track geometry.
pub struct Pointer {
    capture: Option<ScrollCapture>,
}
impl Pointer {
    pub const EMPTY: Self = Self { capture: None };
    pub fn captured(&self) -> bool {
        self.capture.is_some()
    }
    pub fn cancel(&mut self) {
        self.capture = None;
    }
    pub fn event(
        &mut self,
        view: &Document,
        scene: &crate::paint::Scene<'_>,
        target: Identity,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> Option<Event> {
        if phase == PointerPhase::Cancel {
            self.cancel();
            return None;
        }
        if phase == PointerPhase::Press {
            self.cancel();
            for i in (0..view.len as usize).rev() {
                let Some(bar) = scene.scrollbar(i) else {
                    continue;
                };
                if !bar
                    .track
                    .intersect(scene.layout().nodes[i].clip)
                    .contains(x, y)
                {
                    continue;
                }
                self.capture = Some(ScrollCapture {
                    target,
                    element: view.nodes[i].id,
                    track: bar.track,
                    grab: if bar.thumb.contains(x, y) {
                        y - bar.thumb.y
                    } else {
                        bar.thumb.height as i32 / 2
                    },
                });
                return self.scroll(view, scene, target, y);
            }
            return pointer_event(view, scene, target, x, y);
        }
        let result = self.scroll(view, scene, target, y);
        if phase == PointerPhase::Release {
            self.cancel();
        }
        result
    }
    fn scroll(
        &mut self,
        view: &Document,
        scene: &crate::paint::Scene<'_>,
        target: Identity,
        y: i32,
    ) -> Option<Event> {
        let capture = self.capture.take()?;
        if (
            capture.target.epoch,
            capture.target.owner,
            capture.target.window,
            capture.target.incarnation,
        ) != (
            target.epoch,
            target.owner,
            target.window,
            target.incarnation,
        ) {
            return None;
        }
        let index = view.nodes[..view.len as usize]
            .iter()
            .position(|n| n.id == capture.element)?;
        let bar = scene.scrollbar(index)?;
        if bar.track != capture.track
            || bar.track.intersect(scene.layout().nodes[index].clip) != bar.track
        {
            return None;
        }
        let travel = bar.track.height - bar.thumb.height;
        let position = (i64::from(y) - i64::from(bar.track.y) - i64::from(capture.grab))
            .clamp(0, i64::from(travel)) as u64;
        let event = Event {
            target,
            element: capture.element,
            kind: SCROLL,
            action: 0,
            value: (position * u64::from(bar.maximum) / u64::from(travel.max(1))).min(1_048_576)
                as i32,
        };
        event.validate(target, view).ok()?;
        self.capture = Some(capture);
        Some(event)
    }
}

#[derive(Clone, Copy)]
pub enum Key {
    Next,
    Previous,
    Activate,
    PageUp,
    PageDown,
    Character(char),
    Backspace,
}

/// Navigation, hit testing and semantics share the presented layout. Scroll
/// events carry its clamped absolute offset, including after a resize.
pub fn key_event(
    view: &Document,
    scene: &crate::paint::Scene<'_>,
    target: Identity,
    key: Key,
) -> Option<Event> {
    let layout = scene.layout();
    let focused = view.nodes[..view.len as usize]
        .iter()
        .position(|n| n.state & crate::state::FOCUSED != 0);
    let mut event = Event {
        target,
        ..Event::EMPTY
    };
    match key {
        Key::Next | Key::Previous => {
            let index = layout.focus_next(view, focused, matches!(key, Key::Previous))?;
            event.element = view.nodes[index].id;
            event.kind = FOCUS;
        }
        Key::Character(c)
            if c != ' '
                || focused.is_some_and(|i| view.nodes[i].kind == crate::Kind::TextInput) =>
        {
            event.element = view.nodes[focused?].id;
            event.kind = TEXT;
            event.value = c as i32;
        }
        Key::Backspace => {
            event.element = view.nodes[focused?].id;
            event.kind = ERASE;
        }
        Key::Activate | Key::Character(_) => {
            let index = focused?;
            let placed = &layout.nodes[index];
            if placed.hidden || placed.disabled || placed.clip.width == 0 || placed.clip.height == 0
            {
                return None;
            }
            event.element = view.nodes[index].id;
            event.kind = ACTIVATE;
            event.action = view.nodes[index].action;
        }
        Key::PageUp | Key::PageDown => {
            let scrollable = |i: &usize| {
                layout.style(view, *i).overflow == 2
                    && !layout.nodes[*i].hidden
                    && !layout.nodes[*i].disabled
                    && layout.nodes[*i].clip.width != 0
                    && layout.nodes[*i].clip.height != 0
                    && scene.scrollbar(*i).is_some()
            };
            // Prefer the focused item's nearest scroll owner. Without one,
            // start with an inner viewport, not an unrelated outer panel.
            let mut ancestor = focused;
            let mut owner = None;
            while let Some(i) = ancestor {
                if scrollable(&i) {
                    owner = Some(i);
                    break;
                }
                ancestor =
                    (view.nodes[i].parent != crate::NONE).then_some(view.nodes[i].parent as usize);
            }
            let index = owner.or_else(|| (0..layout.len).rev().find(scrollable))?;
            let placed = &layout.nodes[index];
            let max = scene.scrollbar(index)?.maximum;
            let step = placed.bounds.height.max(1);
            event.element = view.nodes[index].id;
            event.kind = SCROLL;
            event.value = if matches!(key, Key::PageUp) {
                placed.scroll_y.saturating_sub(step)
            } else {
                placed.scroll_y.saturating_add(step).min(max)
            }
            .min(1_048_576) as i32;
        }
    }
    event.validate(target, view).ok()?;
    Some(event)
}
/// A track press moves the thumb center to the pointer. No application ID or
/// metric-specific hit coordinates are interpreted by the host.
pub fn pointer_event(
    view: &Document,
    scene: &crate::paint::Scene<'_>,
    target: Identity,
    x: i32,
    y: i32,
) -> Option<Event> {
    for i in (0..view.len as usize).rev() {
        if let Some(bar) = scene.scrollbar(i) {
            if bar
                .track
                .intersect(scene.layout().nodes[i].clip)
                .contains(x, y)
            {
                let travel = bar.track.height - bar.thumb.height;
                let position = (y - bar.track.y - bar.thumb.height as i32 / 2).max(0) as u32;
                let value = (u64::from(position.min(travel)) * u64::from(bar.maximum)
                    / u64::from(travel.max(1)))
                .min(1_048_576) as i32;
                let event = Event {
                    target,
                    element: view.nodes[i].id,
                    kind: SCROLL,
                    value,
                    action: 0,
                };
                event.validate(target, view).ok()?;
                return Some(event);
            }
        }
    }
    let element = scene.hit(x, y)?;
    let node = view.nodes[..view.len as usize]
        .iter()
        .find(|n| n.id == element)?;
    let event = Event::activation(Action {
        target,
        element,
        action: node.action,
    });
    event.validate(target, view).ok()?;
    Some(event)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub target: Identity,
    pub element: u32,
    pub kind: u32,
    pub value: i32,
    pub action: u32,
}
impl Event {
    pub const EMPTY: Self = Self {
        target: Action::EMPTY.target,
        element: 0,
        kind: 0,
        value: 0,
        action: 0,
    };
    pub fn activation(a: Action) -> Self {
        Self {
            target: a.target,
            element: a.element,
            kind: ACTIVATE,
            value: 0,
            action: a.action,
        }
    }
    pub fn validate(self, identity: Identity, view: &Document) -> Result<(), Error> {
        if self.kind == ACTIVATE {
            if self.value != 0 {
                return Err(Error::Value);
            }
            return Action {
                target: self.target,
                element: self.element,
                action: self.action,
            }
            .validate(identity, view);
        }
        view.validate()?;
        if identity.owner == 0
            || identity.window == 0
            || identity.incarnation == 0
            || (
                self.target.owner,
                self.target.window,
                self.target.incarnation,
            ) != (identity.owner, identity.window, identity.incarnation)
        {
            return Err(Error::Owner);
        }
        if identity.epoch == 0
            || self.target.epoch != identity.epoch
            || identity.revision == 0
            || self.target.revision != identity.revision
        {
            return Err(Error::Stale);
        }
        let index = view.nodes[..view.len as usize]
            .iter()
            .position(|n| n.id == self.element)
            .ok_or(Error::Stale)?;
        let node = &view.nodes[index];
        if self.action != 0 {
            return Err(Error::Value);
        }
        match self.kind {
            TEXT if node.kind == crate::Kind::TextInput
                && node.state & crate::state::FOCUSED != 0
                && char::from_u32(self.value as u32).is_some_and(|c| !c.is_control()) => {}
            ERASE
                if node.kind == crate::Kind::TextInput
                    && node.state & crate::state::FOCUSED != 0
                    && self.value == 0 => {}
            FOCUS if node.action != 0 && self.value == 0 => {}
            SCROLL
                if (view.resolved_style(index, u32::MAX).overflow == 2
                    || view.resolved_style(index, 0).overflow == 2)
                    && (0..=1_048_576).contains(&self.value) => {}
            _ => return Err(Error::Value),
        }
        let mut current = node;
        loop {
            if current.state & (crate::state::DISABLED | crate::state::HIDDEN) != 0 {
                return Err(Error::Disabled);
            }
            if current.parent == crate::NONE {
                break;
            }
            current = &view.nodes[current.parent as usize];
        }
        Ok(())
    }
}
const _: () = assert!(core::mem::size_of::<Event>() == 56);

pub struct Queue {
    events: [Event; CAPACITY],
    start: usize,
    len: usize,
}
impl Queue {
    pub const EMPTY: Self = Self {
        events: [Event::EMPTY; CAPACITY],
        start: 0,
        len: 0,
    };
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Absolute scroll offsets coalesce only with the same trailing scroll.
    /// Full queues otherwise refuse the event; never overwrite an action.
    pub fn push_event(
        &mut self,
        event: Event,
        presented: Identity,
        view: &Document,
    ) -> Result<(), Error> {
        event.validate(presented, view)?;
        if self.len != 0 && event.kind == SCROLL {
            let last = &mut self.events[(self.start + self.len - 1) % CAPACITY];
            if last.kind == SCROLL && last.target == event.target && last.element == event.element {
                *last = event;
                return Ok(());
            }
        }
        if self.len == CAPACITY {
            return Err(Error::Limit);
        }
        self.events[(self.start + self.len) % CAPACITY] = event;
        self.len += 1;
        Ok(())
    }
    /// Revalidate against the exact retained source, never reinterpret an old
    /// element ID in a replacement document. Hosts may retain the preceding
    /// presented recipe while its already admitted input drains.
    pub fn take_event(&mut self, sources: &[(Identity, &Document)]) -> Option<Event> {
        while self.len != 0 {
            let event = core::mem::replace(&mut self.events[self.start], Event::EMPTY);
            self.start = (self.start + 1) % CAPACITY;
            self.len -= 1;
            if sources
                .iter()
                .any(|(identity, view)| event.validate(*identity, view).is_ok())
            {
                return Some(event);
            }
        }
        None
    }
    pub fn clear(&mut self) {
        *self = Self::EMPTY;
    }
}
