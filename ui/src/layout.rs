use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            width: (self.right().min(other.right()) - x).max(0) as u32,
            height: (self.bottom().min(other.bottom()) - y).max(0) as u32,
        }
    }
    pub fn right(self) -> i32 {
        self.x.saturating_add(self.width as i32)
    }
    pub fn bottom(self) -> i32 {
        self.y.saturating_add(self.height as i32)
    }
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
    /// The rectangle inside `[top, right, bottom, left]` insets, as returned by
    /// `Document::insets`; empty rather than inverted when they overlap.
    pub fn inset(self, [top, right, bottom, left]: [u32; 4]) -> Self {
        Self {
            x: self.x.saturating_add(left as i32),
            y: self.y.saturating_add(top as i32),
            width: self.width.saturating_sub(left + right),
            height: self.height.saturating_sub(top + bottom),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Placed {
    pub bounds: Rect,
    pub clip: Rect,
    pub content_height: u32,
    pub scroll_y: u32,
    pub hidden: bool,
    pub disabled: bool,
}
#[derive(Clone, Debug)]
pub struct Layout {
    pub nodes: [Placed; MAX_NODES],
    pub len: usize,
    pub viewport: Rect,
}
/// A host-owned viewport for a direct child of a recipe's root. This is useful
/// for native window chrome and content slots; descendants still use the normal
/// CSS layout. Rectangles are local to the host, never framebuffer addresses.
#[derive(Clone, Copy, Debug)]
pub struct HostSlot {
    pub id: u32,
    pub bounds: Rect,
}
/// Per-computation scratch, not retained with the scene. Parent indices precede
/// children in a validated recipe, even when subtrees are not contiguous.
struct Measurements {
    first: [u16; MAX_NODES],
    next: [u16; MAX_NODES],
    height: [u32; MAX_NODES],
}
impl Measurements {
    fn new(view: &Document, width: u32) -> Self {
        let mut result = Self {
            first: [NONE; MAX_NODES],
            next: [NONE; MAX_NODES],
            height: [0; MAX_NODES],
        };
        for i in (1..view.len as usize).rev() {
            let parent = view.nodes[i].parent as usize;
            result.next[i] = result.first[parent];
            result.first[parent] = i as u16;
        }
        // The current subset has single-line text and viewport-relative style
        // breakpoints: intrinsic height does not depend on assigned width.
        // Text wrapping would require width-keyed measurement instead.
        for i in (0..view.len as usize).rev() {
            let n = &view.nodes[i];
            let s = view.resolved_style(i, width);
            let [top, _, bottom, _] = view.insets(s);
            result.height[i] = if n.kind == Kind::Template {
                0
            } else if s.height != u16::MAX {
                s.height as u32
            } else if n.kind == Kind::VirtualList {
                u32::from(view.lists[n.value as usize].item_height) + top + bottom
            } else if !n.kind.container() {
                match n.kind {
                    Kind::Chart => 120,
                    Kind::Separator => 1,
                    Kind::ValueBar => 16,
                    _ => s.font_size as u32 + top + bottom,
                }
            } else {
                let mut count = 0u32;
                let mut content = 0;
                for child in result.children(i) {
                    if view.nodes[child].state & state::HIDDEN != 0 {
                        continue;
                    }
                    count += 1;
                    content = if s.direction == 1 {
                        content.max(result.height[child])
                    } else {
                        content + result.height[child]
                    };
                }
                if s.direction == 0 {
                    content += count.saturating_sub(1) * s.gap as u32;
                }
                content + top + bottom
            };
        }
        result
    }
    fn children(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        core::iter::successors(
            (self.first[i] != NONE).then_some(self.first[i] as usize),
            |i| (self.next[*i] != NONE).then_some(self.next[*i] as usize),
        )
    }
}
const _: () = assert!(core::mem::size_of::<Measurements>() <= 1024);

impl Layout {
    /// Application-owned workspace; initialize statically to avoid copying a
    /// complete layout through a small userspace stack.
    pub const EMPTY: Self = Self {
        nodes: [Placed {
            bounds: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            clip: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            content_height: 0,
            scroll_y: 0,
            hidden: false,
            disabled: false,
        }; MAX_NODES],
        len: 0,
        viewport: Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
    };
    /// Lay out a complete set of host slots. Reject ambiguous, missing, nested
    /// or overflowing placements before returning a scene. Paint, input and
    /// semantics consume the same clipped result, including inherited states.
    pub fn in_slots(
        view: &Document,
        width: u32,
        height: u32,
        slots: &[HostSlot],
    ) -> Result<Self, Error> {
        view.validate()?;
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(Error::Limit);
        }
        let viewport = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let mut seen = [false; MAX_NODES];
        for slot in slots {
            let i = view.nodes[..view.len as usize]
                .iter()
                .position(|n| n.id == slot.id)
                .ok_or(Error::Structure)?;
            let r = slot.bounds;
            if i == 0 || view.nodes[i].parent != 0 || seen[i] {
                return Err(Error::Structure);
            }
            if r.x < 0
                || r.y < 0
                || r.width > width
                || r.height > height
                || r.right() > width as i32
                || r.bottom() > height as i32
            {
                return Err(Error::Limit);
            }
            seen[i] = true;
        }
        if (1..view.len as usize).any(|i| view.nodes[i].parent == 0 && !seen[i]) {
            return Err(Error::Structure);
        }
        let mut result = Self {
            nodes: [Placed::default(); MAX_NODES],
            len: view.len as usize,
            viewport,
        };
        let root = view.nodes[0];
        let hidden = root.state & state::HIDDEN != 0;
        let disabled = root.state & state::DISABLED != 0;
        result.nodes[0] = Placed {
            bounds: viewport,
            clip: viewport,
            hidden,
            disabled,
            ..Placed::default()
        };
        let measurements = Measurements::new(view, width);
        let mut scroll = [0; MAX_NODES];
        for (offset, node) in scroll.iter_mut().zip(view.nodes.iter()) {
            *offset = node.scroll_y;
        }
        for slot in slots {
            let i = view.nodes[..view.len as usize]
                .iter()
                .position(|n| n.id == slot.id)
                .ok_or(Error::Structure)?;
            result.place(
                view,
                &measurements,
                i,
                slot.bounds,
                slot.bounds,
                &scroll,
                hidden,
                disabled,
            );
        }
        Ok(result)
    }
    pub fn compute(
        view: &Document,
        width: u32,
        height: u32,
        scroll: &[u32; MAX_NODES],
    ) -> Result<Self, Error> {
        let mut result = Self::EMPTY;
        Self::compute_into(view, width, height, scroll, &mut result)?;
        Ok(result)
    }
    /// Validate before changing the caller-owned layout. The returned scene
    /// can borrow this storage instead of returning another large value.
    pub fn compute_into(
        view: &Document,
        width: u32,
        height: u32,
        scroll: &[u32; MAX_NODES],
        result: &mut Self,
    ) -> Result<(), Error> {
        view.validate()?;
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(Error::Limit);
        }
        let viewport = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        result.nodes.fill(Placed::default());
        result.len = view.len as usize;
        result.viewport = viewport;
        let measurements = Measurements::new(view, width);
        result.place(
            view,
            &measurements,
            0,
            viewport,
            viewport,
            scroll,
            false,
            false,
        );
        Ok(())
    }
    pub fn style(&self, view: &Document, i: usize) -> Style {
        view.resolved_style(i, self.viewport.width)
    }

    fn width(&self, s: Style, available: u32) -> u32 {
        match s.width {
            i16::MAX => available,
            n if n < 0 => available * u32::from(n.unsigned_abs()) / 100,
            n => n as u32,
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        view: &Document,
        measurements: &Measurements,
        i: usize,
        bounds: Rect,
        clip: Rect,
        scroll: &[u32; MAX_NODES],
        parent_hidden: bool,
        parent_disabled: bool,
    ) {
        let n = &view.nodes[i];
        let s = self.style(view, i);
        let hidden = parent_hidden || n.state & state::HIDDEN != 0 || n.kind == Kind::Template;
        let disabled = parent_disabled || n.state & state::DISABLED != 0;
        let inner = bounds.inset(view.insets(s));
        if n.kind == Kind::VirtualList {
            let content_height = view.lists[n.value as usize].total
                * u32::from(view.lists[n.value as usize].item_height);
            let scroll_y = scroll[i].min(content_height.saturating_sub(inner.height));
            self.nodes[i] = Placed {
                bounds,
                clip: if hidden {
                    Rect::default()
                } else {
                    bounds.intersect(clip)
                },
                content_height,
                scroll_y,
                hidden,
                disabled,
            };
            // Reserve the same eight-pixel track painted by Scene::scrollbar;
            // row text and actions must never sit underneath the scrollbar.
            let rows = Rect {
                width: inner.width.saturating_sub(
                    if content_height > inner.height && inner.width >= 8 {
                        8
                    } else {
                        0
                    },
                ),
                ..inner
            };
            let child_clip = clip.intersect(rows);
            for (slot, child) in measurements.children(i).enumerate() {
                let y = inner.y
                    + ((view.lists[n.value as usize].first + slot as u32)
                        * u32::from(view.lists[n.value as usize].item_height))
                        as i32
                    - scroll_y as i32;
                self.place(
                    view,
                    measurements,
                    child,
                    Rect {
                        x: inner.x,
                        y,
                        width: rows.width,
                        height: u32::from(view.lists[n.value as usize].item_height),
                    },
                    child_clip,
                    scroll,
                    hidden,
                    disabled,
                );
            }
            return;
        }
        let mut count = 0;
        let mut fixed = 0;
        let mut autos = 0;
        let mut content_height = 0;
        let mut growing = 0;
        let mut fixed_height = 0;
        for child in measurements.children(i) {
            if view.nodes[child].state & state::HIDDEN != 0 {
                continue;
            }
            count += 1;
            let cs = self.style(view, child);
            if cs.width == i16::MAX {
                autos += 1;
            } else {
                fixed += self.width(cs, inner.width);
            }
            let h = measurements.height[child];
            if cs.grow {
                growing += 1;
            } else {
                fixed_height += h;
            }
            if s.direction == 1 {
                content_height = content_height.max(h);
            } else {
                content_height += h;
            }
        }
        let gaps = (count as u32).saturating_sub(1) * s.gap as u32;
        if s.direction == 0 {
            content_height += gaps;
        }
        let grown_height =
            inner.height.saturating_sub(fixed_height + gaps) / (growing as u32).max(1);
        if s.direction == 0 && growing != 0 {
            content_height = fixed_height + gaps;
            for child in measurements.children(i) {
                if view.nodes[child].state & state::HIDDEN == 0 && self.style(view, child).grow {
                    let minimum = if self.style(view, child).overflow == 0 {
                        measurements.height[child]
                    } else {
                        0
                    };
                    content_height += grown_height.max(minimum);
                }
            }
        }
        let scroll_y = if s.overflow == 2 {
            scroll[i].min(content_height.saturating_sub(inner.height))
        } else {
            0
        };
        self.nodes[i] = Placed {
            bounds,
            clip: if hidden {
                Rect::default()
            } else {
                bounds.intersect(clip)
            },
            content_height,
            scroll_y,
            hidden,
            disabled,
        };
        let child_clip = if s.overflow != 0 {
            clip.intersect(inner)
        } else {
            clip
        };
        let auto_width = inner.width.saturating_sub(fixed + gaps) / (autos as u32).max(1);
        let align_items = view.decoration(s).align_items;
        let mut x = inner.x;
        let mut y = inner.y - scroll_y as i32;
        for child in measurements.children(i) {
            let cs = self.style(view, child);
            let w = if s.direction == 1 && cs.width == i16::MAX {
                auto_width
            } else {
                self.width(cs, inner.width)
            };
            let h = if s.direction == 0 && cs.grow {
                grown_height.max(if cs.overflow == 0 {
                    measurements.height[child]
                } else {
                    0
                })
            } else {
                measurements.height[child]
            };
            // A row places children on its vertical axis; the compiler admits
            // only start/stretch for columns, whose children span the width.
            let (h, offset) = match (s.direction, align_items) {
                (1, 1) => (h, inner.height.saturating_sub(h) / 2),
                (1, 2) => (h, inner.height.saturating_sub(h)),
                (1, 3) if cs.height == u16::MAX => (inner.height, 0),
                _ => (h, 0),
            };
            self.place(
                view,
                measurements,
                child,
                Rect {
                    x,
                    y: y + offset as i32,
                    width: w,
                    height: h,
                },
                child_clip,
                scroll,
                hidden,
                disabled,
            );
            if view.nodes[child].state & state::HIDDEN != 0 {
                continue;
            }
            if s.direction == 1 {
                x += w as i32 + s.gap as i32;
            } else {
                y += h as i32 + s.gap as i32;
            }
        }
    }
    /// Same clipped geometry as rendering and semantic enumeration.
    pub fn hit(&self, view: &Document, x: i32, y: i32) -> Option<usize> {
        (0..self.len).rev().find(|i| {
            view.nodes[*i].action != 0
                && !self.nodes[*i].disabled
                && !self.nodes[*i].hidden
                && self.nodes[*i].clip.contains(x, y)
        })
    }
    pub fn focus_next(
        &self,
        view: &Document,
        current: Option<usize>,
        backwards: bool,
    ) -> Option<usize> {
        let start = current.unwrap_or(if backwards { 0 } else { self.len - 1 });
        (1..=self.len)
            .map(|offset| {
                if backwards {
                    (start + self.len - offset) % self.len
                } else {
                    (start + offset) % self.len
                }
            })
            .find(|i| {
                view.nodes[*i].action != 0
                    && !self.nodes[*i].hidden
                    && !self.nodes[*i].disabled
                    && self.nodes[*i].clip.width > 0
                    && self.nodes[*i].clip.height > 0
            })
    }
}

impl AsRef<Layout> for Layout {
    fn as_ref(&self) -> &Layout {
        self
    }
}
