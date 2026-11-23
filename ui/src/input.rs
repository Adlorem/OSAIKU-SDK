//! Bounded owner event queue. Admission never grants input authority.
use crate::{layout::Rect, Action, Document, Error, Identity};
pub const CAPACITY: usize = 16;

pub const ACTIVATE: u32 = 1;
pub const FOCUS: u32 = 2;
pub const SCROLL: u32 = 3;
pub const TEXT: u32 = 4;
pub const ERASE: u32 = 5;
/// A `ui-slider` position, 0..=10000 basis points, carrying the slider's
/// action. A press sends one; a drag sends one per changed position.
pub const VALUE: u32 = 6;
/// Enter (1) or leave/dismiss (0) a help icon. No motion coordinates or action.
pub const HOVER: u32 = 7;
/// A press on a `ui-text-area`'s lines: the byte offset in its shown part
/// nearest to the pointer. The application moves its caret there and takes
/// focus; the host never edits the text.
pub const CARET: u32 = 8;
/// The pointer dragged over a `ui-text-area` after a press: the offset the
/// selection now runs to from where the press put the caret.
pub const SELECT: u32 = 9;
/// An editing key for the focused `ui-text-area`: `text_area::edit` values.
pub const EDIT: u32 = 10;
/// A keyboard command for a menu item in a closed menu (its accelerator).
/// It carries the item's action; applications treat it as an activation.
pub const COMMAND: u32 = 11;

/// Platform editing commands, by the action a control declares to take one:
/// the keyboard sends `Key::Shortcut(symbol(COMMANDS[i]))`. A visible control
/// with that action takes it, or else a menu item in a closed menu.
pub const COMMANDS: [&str; 15] = [
    "undo",
    "redo",
    "find",
    "replace",
    "go-to-line",
    "new",
    "close-tab",
    "next-tab",
    "previous-tab",
    "find-next",
    "find-previous",
    "insert-date",
    "zoom-in",
    "zoom-out",
    "zoom-reset",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum PointerPhase {
    Press = 0,
    Move = 1,
    Release = 2,
    Cancel = 3,
    Context = 4,
}
impl PointerPhase {
    pub fn decode(value: u64) -> Option<Self> {
        Some(match value {
            0 => Self::Press,
            1 => Self::Move,
            2 => Self::Release,
            3 => Self::Cancel,
            4 => Self::Context,
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
/// A pressed slider follows the pointer until release, through new content
/// revisions of the same window; it reads the slider's current geometry.
/// A press on a text area's lines selects text by the same rule, and one on
/// its position strip moves the strip's thumb.
#[derive(Clone, Copy)]
struct SliderCapture {
    target: Identity,
    element: u32,
    last: i32,
    kind: u32,
}
/// A scroll gesture may follow new content revisions of the same window, but
/// never a new owner/incarnation, another element, or changed track geometry.
pub struct Pointer {
    capture: Option<ScrollCapture>,
    slider: Option<SliderCapture>,
}
impl Pointer {
    pub const EMPTY: Self = Self {
        capture: None,
        slider: None,
    };
    pub fn captured(&self) -> bool {
        self.capture.is_some() || self.slider.is_some()
    }
    pub fn cancel(&mut self) {
        self.capture = None;
        self.slider = None;
    }
    pub fn event(
        &mut self,
        view: &Document,
        scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
        target: Identity,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> Option<Event> {
        if matches!(phase, PointerPhase::Cancel | PointerPhase::Context) {
            self.cancel();
            return None;
        }
        if phase == PointerPhase::Press {
            self.cancel();
            // An open menu takes the press: nothing beneath it scrolls.
            if menu_holds(view, scene, x, y) {
                return pointer_event(view, scene, target, x, y);
            }
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
            let event = pointer_event(view, scene, target, x, y);
            if let Some(e) = event.filter(|e| matches!(e.kind, VALUE | CARET)) {
                self.slider = Some(SliderCapture {
                    target,
                    element: e.element,
                    last: e.value,
                    // A text area's horizontal scrollbar follows the pointer's x.
                    kind: if e.kind == CARET {
                        SELECT
                    } else if e.value >= crate::text_area::HORIZONTAL_SCROLL_EVENT_BASE {
                        SCROLL
                    } else {
                        VALUE
                    },
                });
            }
            return event;
        }
        let result = if self.slider.is_some() {
            self.slide(view, scene, target, x, y)
        } else {
            self.scroll(view, scene, target, y)
        };
        if phase == PointerPhase::Release {
            self.cancel();
        }
        result
    }
    fn slide(
        &mut self,
        view: &Document,
        scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
        target: Identity,
        x: i32,
        y: i32,
    ) -> Option<Event> {
        let mut capture = self.slider.take()?;
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
        let event = match (view.nodes[index].kind, capture.kind) {
            (crate::Kind::TextArea, SELECT) => {
                area_event(view, scene, target, index, x, y, SELECT)?
            }
            (crate::Kind::TextArea, SCROLL) => {
                area_horizontal_scroll_event(view, scene, target, index, x)?
            }
            (crate::Kind::TextArea, _) => area_strip_event(view, scene, target, index, y)?,
            _ => slider_event(view, scene, target, index, x)?,
        };
        // Only a changed position is news to the application.
        let changed = event.value != capture.last;
        capture.last = event.value;
        self.slider = Some(capture);
        changed.then_some(event)
    }
    fn scroll(
        &mut self,
        view: &Document,
        scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
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
    /// A `text_area::edit` value: for a focused text area only, except that
    /// Up and Down also move between the items of an open menu.
    Edit(i32),
    Next,
    Previous,
    Activate,
    PageUp,
    PageDown,
    Character(char),
    Backspace,
    /// Escape: closes the open menu (activates its popup's action).
    Cancel,
    /// A platform command resolves to one enabled, visible action in the
    /// presented scope. It does not bypass the ordinary owner event queue.
    Shortcut(u32),
}

/// Navigation, hit testing and semantics share the presented layout. Scroll
/// events carry its clamped absolute offset, including after a resize.
pub fn key_event(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    key: Key,
) -> Option<Event> {
    let layout = scene.layout();
    let modal = layout.modal_popup(view);
    let focused = view.nodes[..view.len as usize]
        .iter()
        .position(|n| n.state & crate::state::FOCUSED != 0)
        .filter(|i| {
            modal.is_none_or(|m| *i != m && crate::layout::Layout::popup_of(view, *i) == Some(m))
        });
    let mut event = Event {
        target,
        ..Event::EMPTY
    };
    match key {
        Key::Shortcut(action) => {
            if action == 0 {
                return None;
            }
            let mut candidates =
                view.nodes[..view.len as usize]
                    .iter()
                    .enumerate()
                    .filter(|(i, node)| {
                        let placed = &layout.nodes[*i];
                        let visible = placed.bounds.intersect(placed.clip);
                        node.action == action
                            && !placed.hidden
                            && !placed.disabled
                            && visible.width != 0
                            && visible.height != 0
                            && modal.is_none_or(|m| {
                                *i != m && crate::layout::Layout::popup_of(view, *i) == Some(m)
                            })
                    });
            if let Some((_, node)) = candidates.next() {
                if candidates.next().is_some() {
                    // Ambiguous commands must not select an arbitrary recipient.
                    return None;
                }
                event.element = node.id;
                event.kind = ACTIVATE;
                event.action = action;
            } else {
                // A menu item in a closed menu keeps its accelerator. An open
                // menu or dialog holds the keyboard, as it holds the pointer.
                if modal.is_some() {
                    return None;
                }
                let mut items = (0..view.len as usize)
                    .filter(|i| view.nodes[*i].action == action && in_closed_menu(view, *i));
                let index = items.next()?;
                if items.next().is_some() {
                    return None;
                }
                event.element = view.nodes[index].id;
                event.kind = COMMAND;
                event.action = action;
            }
        }
        Key::Edit(value) => {
            let area = focused.filter(|i| view.nodes[*i].kind == crate::Kind::TextArea);
            if let Some(index) = area {
                event.element = view.nodes[index].id;
                event.kind = EDIT;
                event.value = value;
            } else {
                let key = crate::text_area::edit::key(value);
                if modal.is_none() || !matches!(key, 3 | 4) {
                    return None;
                }
                let index = layout.focus_next(view, focused, key == 3)?;
                event.element = view.nodes[index].id;
                event.kind = FOCUS;
            }
        }
        Key::Activate | Key::Next | Key::PageUp | Key::PageDown
            if focused.is_some_and(|i| view.nodes[i].kind == crate::Kind::TextArea) =>
        {
            use crate::text_area::edit;
            event.element = view.nodes[focused?].id;
            event.kind = EDIT;
            event.value = match key {
                Key::Activate => edit::NEWLINE,
                Key::Next => edit::TAB,
                Key::PageUp => edit::PAGE_UP,
                _ => edit::PAGE_DOWN,
            };
        }
        Key::Next | Key::Previous => {
            let index = layout.focus_next(view, focused, matches!(key, Key::Previous))?;
            event.element = view.nodes[index].id;
            event.kind = FOCUS;
            if view.nodes[index].kind == crate::Kind::Tooltip {
                event.action = crate::tooltip::ACTION;
            }
        }
        Key::Character(c)
            if c != ' '
                || focused.is_some_and(|i| {
                    matches!(
                        view.nodes[i].kind,
                        crate::Kind::TextInput | crate::Kind::TextArea
                    )
                }) =>
        {
            event.element = view.nodes[focused?].id;
            event.kind = TEXT;
            event.value = c as i32;
        }
        Key::Backspace => {
            event.element = view.nodes[focused?].id;
            event.kind = ERASE;
        }
        Key::Cancel => {
            if let Some(n) = view.nodes[..view.len as usize]
                .iter()
                .find(|n| n.kind == crate::Kind::Tooltip && n.state & crate::state::SELECTED != 0)
            {
                event.element = n.id;
                event.kind = HOVER;
                event.validate(target, view).ok()?;
                return Some(event);
            }
            let index = layout.modal_popup(view)?;
            event.element = view.nodes[index].id;
            event.kind = ACTIVATE;
            event.action = view.nodes[index].action;
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
            if modal.is_some() {
                return None;
            }
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
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    x: i32,
    y: i32,
) -> Option<Event> {
    let menu = menu_holds(view, scene, x, y);
    for i in (0..view.len as usize).rev().filter(|_| !menu) {
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
    let index = view.nodes[..view.len as usize]
        .iter()
        .position(|n| n.id == element)?;
    if view.nodes[index].kind == crate::Kind::Slider {
        return slider_event(view, scene, target, index, x);
    }
    if view.nodes[index].kind == crate::Kind::TextArea {
        return area_strip_event(view, scene, target, index, y)
            .filter(|_| area_strip_holds(view, scene, index, x, y))
            .or_else(|| {
                area_horizontal_scroll_event(view, scene, target, index, x)
                    .filter(|_| area_horizontal_scrollbar_contains(view, scene, index, x, y))
            })
            .or_else(|| area_event(view, scene, target, index, x, y, CARET));
    }
    // A pointer press enters a text field; only keyboard activation submits
    // its action. Sharing activation here made rename/search fields submit
    // as soon as the user clicked them to edit.
    if view.nodes[..view.len as usize]
        .iter()
        .any(|n| n.id == element && n.kind == crate::Kind::TextInput)
    {
        let event = Event {
            target,
            element,
            kind: FOCUS,
            action: 0,
            value: 0,
        };
        event.validate(target, view).ok()?;
        return Some(event);
    }
    activation(view, target, element)
}
/// The value under `x` on slider `index`, from the geometry it is drawn with.
fn slider_event(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    index: usize,
    x: i32,
) -> Option<Event> {
    let layout = scene.layout();
    let placed = &layout.nodes[index];
    if view.nodes[index].kind != crate::Kind::Slider || placed.hidden || placed.disabled {
        return None;
    }
    let content = placed.bounds.inset(view.insets(layout.style(view, index)));
    let geometry = crate::slider::geometry(content)?;
    let event = Event {
        target,
        element: view.nodes[index].id,
        kind: VALUE,
        value: i32::from(geometry.value(x)),
        action: view.nodes[index].action,
    };
    event.validate(target, view).ok()?;
    Some(event)
}
/// The content box of text area `index` and its strip geometry, while enabled.
fn area_geometry(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    index: usize,
) -> Option<(crate::text_area::Geometry, u16)> {
    let layout = scene.layout();
    let placed = &layout.nodes[index];
    let node = &view.nodes[index];
    if node.kind != crate::Kind::TextArea || placed.hidden || placed.disabled {
        return None;
    }
    let style = layout.style(view, index);
    let content = placed.bounds.inset(view.insets(style));
    Some((
        crate::text_area::geometry_with_scroll(content, node.value, node.scroll_x),
        style.font_size,
    ))
}
fn area_horizontal_scrollbar_contains(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    index: usize,
    x: i32,
    y: i32,
) -> bool {
    area_geometry(view, scene, index)
        .and_then(|(g, _)| g.horizontal_track)
        .is_some_and(|side| {
            Rect {
                y: side.y - 2,
                height: side.height + 2,
                ..side
            }
            .contains(x, y)
        })
}
/// The horizontal scrollbar's value under `x`: a VALUE event at
/// `text_area::HORIZONTAL_SCROLL_EVENT_BASE` plus the position, with the area's action.
fn area_horizontal_scroll_event(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    index: usize,
    x: i32,
) -> Option<Event> {
    let (g, _) = area_geometry(view, scene, index)?;
    let event = Event {
        target,
        element: view.nodes[index].id,
        kind: VALUE,
        value: crate::text_area::HORIZONTAL_SCROLL_EVENT_BASE
            + i32::from(g.horizontal_position(x)?),
        action: view.nodes[index].action,
    };
    event.validate(target, view).ok()?;
    Some(event)
}
fn area_strip_holds(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    index: usize,
    x: i32,
    y: i32,
) -> bool {
    area_geometry(view, scene, index)
        .and_then(|(g, _)| g.track)
        .is_some_and(|track| {
            Rect {
                x: track.x - 2,
                width: track.width + 2,
                ..track
            }
            .contains(x, y)
        })
}
/// The offset under the pointer, clamped to the area's lines.
fn area_event(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    index: usize,
    x: i32,
    y: i32,
    kind: u32,
) -> Option<Event> {
    let (g, size) = area_geometry(view, scene, index)?;
    let text = view.text_of(index);
    let offset = crate::text_area::offset_at(text, size, x - g.text.x, y - g.text.y);
    let event = Event {
        target,
        element: view.nodes[index].id,
        kind,
        value: offset as i32,
        action: 0,
    };
    event.validate(target, view).ok()?;
    Some(event)
}
/// The position strip's value under `y`: a VALUE event with the area's action.
fn area_strip_event(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    index: usize,
    y: i32,
) -> Option<Event> {
    let (g, _) = area_geometry(view, scene, index)?;
    let event = Event {
        target,
        element: view.nodes[index].id,
        kind: VALUE,
        value: i32::from(g.position(y)?),
        action: view.nodes[index].action,
    };
    event.validate(target, view).ok()?;
    Some(event)
}
/// Whether item `index` sits in a closed menu: enabled and not hidden itself,
/// with every hidden ancestor a popup, at least one, and none disabled.
/// Rows of a virtual list are data, never menu items.
fn in_closed_menu(view: &Document, index: usize) -> bool {
    let node = &view.nodes[index];
    if node.action == 0
        || node.state & (crate::state::HIDDEN | crate::state::DISABLED) != 0
        || !crate::catalog::actionable(node.kind)
        || crate::virtual_list::in_template(view, index)
    {
        return false;
    }
    let mut closed = false;
    let mut current = node.parent;
    let mut depth = 0;
    while current != crate::NONE {
        depth += 1;
        if depth > crate::MAX_DEPTH || usize::from(current) >= usize::from(view.len) {
            return false;
        }
        let ancestor = &view.nodes[usize::from(current)];
        if ancestor.state & crate::state::DISABLED != 0 {
            return false;
        }
        if ancestor.state & crate::state::HIDDEN != 0 {
            if ancestor.kind != crate::Kind::Popup {
                return false;
            }
            closed = true;
        }
        current = ancestor.parent;
    }
    closed
}
/// Whether an open popup decides this press: the point is on one, or a menu
/// with a dismiss action is open (then a press elsewhere only closes it).
fn menu_holds(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    x: i32,
    y: i32,
) -> bool {
    let layout = scene.layout();
    layout.over_popup(view, x, y) || layout.modal_popup(view).is_some()
}
fn activation(view: &Document, target: Identity, element: u32) -> Option<Event> {
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
/// The element under a secondary (context) click, ignoring open popups: a
/// second right-click on another item moves the menu there, as on a desktop.
pub fn context_event(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
    x: i32,
    y: i32,
) -> Option<Event> {
    let index = scene.layout().hit_beneath(view, x, y)?;
    activation(view, target, view.nodes[index].id)
}
/// Close the topmost open menu: its popup's action, as a press outside it or
/// Escape would send. None when no popup with an action is open.
pub fn popup_dismiss(
    view: &Document,
    scene: &crate::paint::Scene<'_, impl AsRef<crate::layout::Layout>>,
    target: Identity,
) -> Option<Event> {
    let index = scene.layout().modal_popup(view)?;
    activation(view, target, view.nodes[index].id)
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
    /// The same owner, window and incarnation, and the presented revision.
    fn identity_matches(target: Identity, identity: Identity) -> Result<(), Error> {
        if identity.owner == 0
            || identity.window == 0
            || identity.incarnation == 0
            || (target.owner, target.window, target.incarnation)
                != (identity.owner, identity.window, identity.incarnation)
        {
            return Err(Error::Owner);
        }
        if identity.epoch == 0
            || target.epoch != identity.epoch
            || identity.revision == 0
            || target.revision != identity.revision
        {
            return Err(Error::Stale);
        }
        Ok(())
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
        if self.kind == COMMAND {
            Self::identity_matches(self.target, identity)?;
            let index = view.nodes[..view.len as usize]
                .iter()
                .position(|n| n.id == self.element)
                .ok_or(Error::Stale)?;
            return if self.value == 0
                && self.action != 0
                && view.nodes[index].action == self.action
                && in_closed_menu(view, index)
            {
                Ok(())
            } else {
                Err(Error::Value)
            };
        }
        Self::identity_matches(self.target, identity)?;
        let index = view.nodes[..view.len as usize]
            .iter()
            .position(|n| n.id == self.element)
            .ok_or(Error::Stale)?;
        let node = &view.nodes[index];
        // Only a slider position names its action; other input carries none.
        let action =
            if self.kind == VALUE || (self.kind == FOCUS && node.kind == crate::Kind::Tooltip) {
                node.action
            } else {
                0
            };
        let text_field = matches!(node.kind, crate::Kind::TextInput | crate::Kind::TextArea);
        let focused = node.state & crate::state::FOCUSED != 0;
        let area_offset = |value: i32| {
            node.kind == crate::Kind::TextArea
                && usize::try_from(value).is_ok_and(|at| {
                    let text = view.text_of(index);
                    at <= text.len() && text.is_char_boundary(at)
                })
        };
        if self.action != action {
            return Err(Error::Value);
        }
        match self.kind {
            TEXT if text_field
                && focused
                && char::from_u32(self.value as u32).is_some_and(|c| !c.is_control()) => {}
            ERASE if text_field && focused && self.value == 0 => {}
            CARET | SELECT if area_offset(self.value) => {}
            EDIT if node.kind == crate::Kind::TextArea
                && focused
                && crate::text_area::edit::valid(self.value) => {}
            VALUE
                if node.kind == crate::Kind::TextArea
                    && node.action != 0
                    && node.value <= 10000
                    && (0..=10000).contains(&self.value) => {}
            VALUE
                if node.kind == crate::Kind::TextArea
                    && node.action != 0
                    && crate::text_area::decode_horizontal_scroll(node.scroll_x).is_some()
                    && crate::text_area::horizontal_scroll_event_position(self.value).is_some() => {
            }
            FOCUS if node.action != 0 && self.value == 0 => {}
            HOVER if node.kind == crate::Kind::Tooltip && (0..=1).contains(&self.value) => {}
            VALUE
                if node.kind == crate::Kind::Slider
                    && node.action != 0
                    && (0..=10000).contains(&self.value) => {}
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
    /// Absolute scroll offsets and slider positions coalesce only with the
    /// same trailing event kind for the same element and target.
    /// Full queues otherwise refuse the event; never overwrite an action.
    pub fn push_event(
        &mut self,
        event: Event,
        presented: Identity,
        view: &Document,
    ) -> Result<(), Error> {
        event.validate(presented, view)?;
        if self.len != 0 && matches!(event.kind, SCROLL | VALUE | HOVER | SELECT) {
            let last = &mut self.events[(self.start + self.len - 1) % CAPACITY];
            if last.kind == event.kind
                && last.target == event.target
                && (last.element == event.element || event.kind == HOVER)
            {
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
        self.deliver_event(sources, |_| true)
    }
    /// A failed publication retains the first valid event for retry. Invalid
    /// stale events are discarded; the callback must not reenter this queue.
    pub fn deliver_event(
        &mut self,
        sources: &[(Identity, &Document)],
        publish: impl FnOnce(&Event) -> bool,
    ) -> Option<Event> {
        while self.len != 0 {
            let event = self.events[self.start];
            let valid = sources
                .iter()
                .any(|(identity, view)| event.validate(*identity, view).is_ok());
            if valid {
                if !publish(&event) {
                    return None;
                }
                self.events[self.start] = Event::EMPTY;
                self.start = (self.start + 1) % CAPACITY;
                self.len -= 1;
                return Some(event);
            }
            self.events[self.start] = Event::EMPTY;
            self.start = (self.start + 1) % CAPACITY;
            self.len -= 1;
        }
        None
    }
    pub fn clear(&mut self) {
        *self = Self::EMPTY;
    }
}
