//! Host-independent frame structure, layout and interaction regions.
use super::geometry::Bounds;
use crate::{
    layout::{HostSlot, Rect},
    paint::Scene,
    symbol, Document,
};
pub const HTML: &str = include_str!("frame.ui.html");
pub const CSS: &str = include_str!("frame.ui.css");
/// Native host decoration accompanying the HTML/CSS frame content. This is
/// host policy, not a property that an untrusted client Document can override.
pub const STYLE: super::WindowStyle = super::WindowStyle::NATIVE;
pub const LEFT: u8 = 1;
pub const RIGHT: u8 = 2;
pub const TOP: u8 = 4;
pub const BOTTOM: u8 = 8;
/// Host-measured native pixel dimensions, without addresses or app identity.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub title_height: u32,
    pub control: Bounds,
    pub inset: Bounds,
}
pub fn scene<'a>(
    doc: &'a Document,
    width: u32,
    height: u32,
    metrics: Metrics,
    title: &'a str,
    icon: &'a str,
    text_width: u32,
    centered: bool,
) -> Option<Scene<'a>> {
    let mut slots = slots(metrics, width, height)?;
    if centered {
        // Centred, but never over the application badge.
        let left = slots[2].bounds.x as u32;
        let right = slots[2].bounds.right();
        let x = (width.saturating_sub(text_width.saturating_add(6)) / 2)
            .max(left)
            .min(right as u32);
        slots[2].bounds.x = x as i32;
        slots[2].bounds.width = (right - x as i32) as u32;
    }
    Scene::in_slots(doc, width, height, &slots)
        .ok()?
        .with_text_binding(symbol("window-title"), title)
        .ok()?
        .with_text_binding(symbol("window-icon"), icon)
        .ok()
}
fn slot(name: &str, x: u32, y: u32, width: u32, height: u32) -> HostSlot {
    HostSlot {
        id: symbol(name),
        bounds: Rect {
            x: x as i32,
            y: y as i32,
            width,
            height,
        },
    }
}
/// The content rectangle in window-local coordinates; no scene/layout work.
/// `title` 0 is an unframed window (a tray flyout): content fills it.
pub fn content_bounds(title: u32, width: u32, height: u32) -> Option<Bounds> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 || title >= height {
        return None;
    }
    Some(Bounds::new(0, title, width, height.checked_sub(title)?))
}
/// How many host slots a frame composes: content, drag, title, close, badge,
/// the twelve resize regions, then minimize, maximize and maximize's mark.
pub const SLOTS: usize = 20;
/// The window controls sit flush against each other from the right edge:
/// minimize, maximize, close. Each is `Metrics::control` wide. A frame too
/// narrow for all of them keeps close alone.
pub const CONTROLS: u32 = 3;
pub fn slots(metrics: Metrics, width: u32, height: u32) -> Option<[HostSlot; SLOTS]> {
    let title = metrics.title_height;
    let control = metrics.control;
    let inset = metrics.inset;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || title == 0
        || title >= height
        || control.width == 0
        || control.height == 0
        || control.height.checked_add(inset.height)? > title
    {
        return None;
    }
    let close_x = width.checked_sub(inset.width.checked_add(control.width)?)?;
    // Minimize and maximize only when all three fit; a narrower frame keeps
    // close alone, and their slots are empty rectangles at close's edge.
    let pair = control.width.checked_mul(2)?;
    let all = close_x >= pair;
    let maximize_x = if all {
        close_x - control.width
    } else {
        close_x
    };
    let minimize_x = if all { close_x - pair } else { close_x };
    let (pair_width, pair_height) = if all {
        (control.width, control.height)
    } else {
        (0, 0)
    };
    // A square application badge centred in the title bar, then the title.
    let icon = (title * 5 / 8).clamp(8, 40).min(title);
    let icon_x = (title - icon) / 2 + 2;
    let title_x = (icon_x + icon + icon / 2).min(minimize_x);
    let mut slots = [slot("content-slot", 0, 0, 0, 0); SLOTS];
    let content = content_bounds(title, width, height)?;
    slots[0] = slot(
        "content-slot",
        content.x,
        content.y,
        content.width,
        content.height,
    );
    slots[1] = slot("drag", 0, 0, width, title);
    slots[2] = slot("title", title_x, 0, minimize_x.checked_sub(title_x)?, title);
    slots[3] = slot(
        "close",
        close_x,
        inset.height,
        control.width,
        control.height,
    );
    slots[4] = slot(
        "window-icon",
        icon_x.min(width),
        (title - icon) / 2,
        icon.min(width.saturating_sub(icon_x)),
        icon,
    );
    for (out, (region, _)) in slots[5..17]
        .iter_mut()
        .zip(resize_regions(width, height, title)?)
    {
        *out = region;
    }
    slots[17] = slot(
        "minimize",
        minimize_x,
        inset.height,
        pair_width,
        pair_height,
    );
    slots[18] = slot(
        "maximize",
        maximize_x,
        inset.height,
        pair_width,
        pair_height,
    );
    // An outlined square, as tall as the minimize dash is wide, centred in
    // the maximize control. Drawn by its border, so no glyph is needed.
    let mark = (pair_height * 9 / 20)
        .max(6)
        .min(pair_width)
        .min(pair_height);
    slots[19] = slot(
        "maximize-mark",
        maximize_x + (pair_width - mark) / 2,
        inset.height + (pair_height - mark) / 2,
        mark,
        mark,
    );
    Some(slots)
}
/// Resize regions are composition slots, shared with the system's size policy.
/// Corner handles are L-shaped, preserving title drag space inside the frame.
pub fn resize_regions(w: u32, h: u32, title: u32) -> Option<[(HostSlot, u8); 12]> {
    if w == 0 || h == 0 || w > 4096 || h > 4096 || title == 0 || title > h {
        return None;
    }
    let b = (title / 6).clamp(3, 8).min(w).min(h);
    let c = (b * 3).min(w / 4).min(h / 4);
    Some([
        (slot("resize-left", 0, c, b, h.checked_sub(2 * c)?), LEFT),
        (
            slot(
                "resize-right",
                w.checked_sub(b)?,
                c,
                b,
                h.checked_sub(2 * c)?,
            ),
            RIGHT,
        ),
        (slot("resize-top", c, 0, w.checked_sub(2 * c)?, b), TOP),
        (
            slot(
                "resize-bottom",
                c,
                h.checked_sub(b)?,
                w.checked_sub(2 * c)?,
                b,
            ),
            BOTTOM,
        ),
        (slot("resize-tl-left", 0, 0, b, c), TOP | LEFT),
        (slot("resize-tl-top", 0, 0, c, b), TOP | LEFT),
        (
            slot("resize-tr-right", w.checked_sub(b)?, 0, b, c),
            TOP | RIGHT,
        ),
        (
            slot("resize-tr-top", w.checked_sub(c)?, 0, c, b),
            TOP | RIGHT,
        ),
        (
            slot("resize-bl-left", 0, h.checked_sub(c)?, b, c),
            BOTTOM | LEFT,
        ),
        (
            slot("resize-bl-bottom", 0, h.checked_sub(b)?, c, b),
            BOTTOM | LEFT,
        ),
        (
            slot(
                "resize-br-right",
                w.checked_sub(b)?,
                h.checked_sub(c)?,
                b,
                c,
            ),
            BOTTOM | RIGHT,
        ),
        (
            slot(
                "resize-br-bottom",
                w.checked_sub(c)?,
                h.checked_sub(b)?,
                c,
                b,
            ),
            BOTTOM | RIGHT,
        ),
    ])
}
