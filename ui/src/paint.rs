//! Raster adapters implement these bounded primitives. They do not know app
//! labels, monitor offsets or metric types. Every operation carries its clip.
pub use crate::shape::RoundedShape;
use crate::{
    layout::{Layout, Rect},
    *,
};

pub trait Painter {
    /// Coarse rejection before expanding a node into glyphs/chart segments.
    /// Painters still clip individual primitives to their exact coverage.
    fn visible(&self, _bounds: Rect) -> bool {
        true
    }
    /// RGB with independent coverage; adapters compose into retained alpha.
    fn fill_alpha(&mut self, rect: Rect, color: u32, alpha: u8, clip: Rect);
    fn fill(&mut self, rect: Rect, color: u32, clip: Rect) {
        self.fill_alpha(rect, color, 255, clip);
    }
    fn text(&mut self, rect: Rect, text: &str, size: u16, color: u32, clip: Rect);
    fn line(&mut self, from: (i32, i32), to: (i32, i32), color: u32, clip: Rect);
    /// Anti-aliased box with a uniform corner radius, clamped to half the
    /// shorter side. The default emits bounded `fill_alpha` rectangles; an
    /// override must produce the same coverage.
    fn fill_rounded(&mut self, rect: Rect, radius: u32, color: u32, alpha: u8, clip: Rect) {
        crate::shape::rounded(self, rect, radius, None, color, alpha, clip);
    }
    /// The border ring of the same box; `widths` are top, right, bottom, left.
    /// Inner corners follow the outer ones, as in CSS.
    fn stroke_rounded(
        &mut self,
        rect: Rect,
        radius: u32,
        widths: [u32; 4],
        color: u32,
        alpha: u8,
        clip: Rect,
    ) {
        crate::shape::rounded(self, rect, radius, Some(widths), color, alpha, clip);
    }
    /// The part `area` of a rounded box where an arc crosses every row. The
    /// default emits one `fill_alpha` per run of equal alpha; a recipe planner
    /// may keep `shape` and evaluate `RoundedShape::alpha` for each pixel when
    /// it composes, which gives the same pixels from one primitive.
    fn fill_shape(&mut self, area: Rect, shape: RoundedShape, color: u32, clip: Rect) {
        crate::shape::runs(self, area, shape, color, clip);
    }
    /// A pre-rasterized image at `at`, which has exactly its size. The default
    /// emits one `fill_alpha` per row run of equal pixels; a recipe planner may
    /// keep the image and compose its pixels later, with the same result.
    fn image(&mut self, at: Rect, image: &'static crate::image::Image, clip: Rect) {
        crate::image::runs(self, at, image, clip);
    }
}

/// A background, marker or bar: square corners keep the single rectangle
/// every adapter has always received.
fn fill_box(
    painter: &mut (impl Painter + ?Sized),
    rect: Rect,
    radius: u8,
    color: u32,
    alpha: u8,
    clip: Rect,
) {
    if radius == 0 {
        painter.fill_alpha(rect, color, alpha, clip);
    } else {
        painter.fill_rounded(rect, radius.into(), color, alpha, clip);
    }
}

/// Bound to a layout produced for this exact immutable recipe. Hosts must keep
/// the pair together through admission, rasterization and presentation.
pub struct Scene<'a, L = Layout> {
    view: &'a Document,
    layout: L,
    text_bindings: [(u32, &'a str); 8],
    text_binding_len: usize,
    theme: theme::Theme,
    images: &'a [crate::image::ImageSet],
}
pub struct Semantic<'a> {
    pub id: u32,
    pub parent: Option<u32>,
    pub kind: Kind,
    pub role: Role,
    pub text: &'a str,
    pub value: Option<u16>,
    pub series: &'a [Point],
    pub state: u8,
    pub action: u32,
    pub bounds: Rect,
    pub visible_bounds: Rect,
    pub clipped: bool,
}
pub struct Scrollbar {
    pub track: Rect,
    pub thumb: Rect,
    pub maximum: u32,
}
impl<'a> Scene<'a> {
    /// Use an already validated immutable publication layout.
    pub fn from_layout(view: &'a Document, layout: Layout) -> Result<Self, Error> {
        if layout.len != view.len as usize || layout.len > MAX_NODES {
            return Err(Error::Limit);
        }
        Ok(Self {
            view,
            layout,
            text_bindings: [(0, ""); 8],
            text_binding_len: 0,
            theme: theme::Theme::Dark,
            images: &[],
        })
    }
    /// Compose host-owned slots without creating a second paint/input layout.
    pub fn in_slots(
        view: &'a Document,
        width: u32,
        height: u32,
        slots: &[layout::HostSlot],
    ) -> Result<Self, Error> {
        Ok(Self {
            view,
            layout: Layout::in_slots(view, width, height, slots)?,
            text_bindings: [(0, ""); 8],
            text_binding_len: 0,
            theme: theme::Theme::Dark,
            images: &[],
        })
    }
    /// Native hosts use offsets from the immutable submitted recipe.
    pub fn from_document(view: &'a Document, width: u32, height: u32) -> Result<Self, Error> {
        let mut scroll = [0; MAX_NODES];
        for (offset, node) in scroll.iter_mut().zip(view.nodes.iter()) {
            *offset = node.scroll_y;
        }
        Self::new(view, width, height, &scroll)
    }
    pub fn new(
        view: &'a Document,
        width: u32,
        height: u32,
        scroll: &[u32; MAX_NODES],
    ) -> Result<Self, Error> {
        Ok(Self {
            view,
            layout: Layout::compute(view, width, height, scroll)?,
            text_bindings: [(0, ""); 8],
            text_binding_len: 0,
            theme: theme::Theme::Dark,
            images: &[],
        })
    }
}
impl<'a> Scene<'a, &'a Layout> {
    /// Compute in private caller-owned storage and retain a shared borrow of
    /// the exact recipe/layout pair for pixels, semantics and hit testing.
    pub fn prepare(
        view: &'a Document,
        width: u32,
        height: u32,
        workspace: &'a mut Layout,
    ) -> Result<Self, Error> {
        let mut scroll = [0; MAX_NODES];
        for (offset, node) in scroll.iter_mut().zip(view.nodes.iter()) {
            *offset = node.scroll_y;
        }
        Layout::compute_into(view, width, height, &scroll, workspace)?;
        Ok(Self {
            view,
            layout: workspace,
            text_bindings: [(0, ""); 8],
            text_binding_len: 0,
            theme: theme::Theme::Dark,
            images: &[],
        })
    }
}
impl<'a, L: AsRef<Layout>> Scene<'a, L> {
    /// Select the host's palette for this immutable frame. Layout and semantic
    /// identities stay unchanged; no global mutable renderer state is used.
    pub fn with_theme(mut self, theme: theme::Theme) -> Self {
        self.theme = theme;
        self
    }
    /// The host's images, found by a `ui-icon src` name. An icon whose name is
    /// missing, or whose smallest size does not fit, keeps the square mark.
    pub fn with_images(mut self, images: &'a [crate::image::ImageSet]) -> Self {
        self.images = images;
        self
    }

    /// Bind immutable host text without copying a Document-sized recipe. The
    /// current single-line text subset has no text-dependent intrinsic metrics.
    /// Both paint and semantics read this same bounded binding; data is never
    /// interpreted as markup. Use Document::bind_into for transported updates.
    pub fn with_text_binding(mut self, binding: u32, text: &'a str) -> Result<Self, Error> {
        if text.len() > MAX_LEAF_TEXT {
            return Err(Error::Text);
        }
        if binding == 0
            || !self.view.nodes[..self.view.len as usize]
                .iter()
                .any(|n| n.bindings[0] == binding)
        {
            return Err(Error::Binding);
        }
        if self.text_bindings[..self.text_binding_len]
            .iter()
            .any(|(id, _)| *id == binding)
        {
            return Err(Error::Binding);
        }
        *self
            .text_bindings
            .get_mut(self.text_binding_len)
            .ok_or(Error::Limit)? = (binding, text);
        self.text_binding_len += 1;
        Ok(self)
    }
    fn text_of(&self, index: usize) -> &str {
        let binding = self.view.nodes[index].bindings[0];
        self.text_bindings[..self.text_binding_len]
            .iter()
            .find(|(id, _)| *id == binding)
            .map_or_else(|| self.view.text_of(index), |(_, text)| *text)
    }
    pub fn layout(&self) -> &Layout {
        self.layout.as_ref()
    }
    pub fn hit(&self, x: i32, y: i32) -> Option<u32> {
        self.layout()
            .hit(self.view, x, y)
            .map(|i| self.view.nodes[i].id)
    }
    pub fn semantics(&self) -> impl Iterator<Item = Semantic<'_>> {
        (0..self.layout().len).map(|i| {
            let n = &self.view.nodes[i];
            let p = &self.layout().nodes[i];
            Semantic {
                id: n.id,
                parent: (n.parent != NONE).then(|| self.view.nodes[n.parent as usize].id),
                kind: n.kind,
                role: n.role,
                text: self.text_of(i),
                value: (n.value <= 10000).then_some(n.value),
                series: &self.view.points
                    [n.series_start as usize..(n.series_start + n.series_len) as usize],
                state: n.state
                    | if p.hidden { state::HIDDEN } else { 0 }
                    | if p.disabled { state::DISABLED } else { 0 },
                action: n.action,
                bounds: p.bounds,
                visible_bounds: p.clip,
                clipped: p.bounds != p.clip,
            }
        })
    }
    pub fn paint(&self, painter: &mut impl Painter) {
        for i in 0..self.layout().len {
            let n = &self.view.nodes[i];
            let p = &self.layout().nodes[i];
            if p.hidden
                || p.clip.width == 0
                || p.clip.height == 0
                || !painter.visible(p.bounds.intersect(p.clip))
            {
                continue;
            }
            let mut s = self.layout().style(self.view, i);
            let mut decoration = self.view.decoration(s);
            s.color = self.theme.color(s.color_token, s.color) & 0x00ff_ffff;
            s.background = self.theme.color(s.background_token, s.background);
            decoration.border_color = self
                .theme
                .color(decoration.border_token, decoration.border_color);
            if s.fill {
                fill_box(
                    painter,
                    p.bounds,
                    decoration.radius,
                    s.background & 0x00ff_ffff,
                    (s.background >> 24) as u8,
                    p.clip,
                );
            }
            if decoration.border_width != 0 {
                let (color, alpha) = if decoration.border_current_color {
                    (s.color, 255)
                } else {
                    (
                        decoration.border_color & 0x00ff_ffff,
                        (decoration.border_color >> 24) as u8,
                    )
                };
                painter.stroke_rounded(
                    p.bounds,
                    decoration.radius.into(),
                    decoration.border_widths(),
                    color,
                    alpha,
                    p.clip,
                );
            }
            let ring = n.state & state::FOCUSED != 0 && !decoration.outline_none;
            if ring && decoration.radius != 0 {
                painter.stroke_rounded(
                    p.bounds,
                    decoration.radius.into(),
                    [1; 4],
                    s.color,
                    255,
                    p.clip,
                );
            } else if ring {
                let r = p.bounds;
                for edge in [
                    Rect { height: 1, ..r },
                    Rect {
                        y: r.bottom() - 1,
                        height: 1,
                        ..r
                    },
                    Rect { width: 1, ..r },
                    Rect {
                        x: r.right() - 1,
                        width: 1,
                        ..r
                    },
                ] {
                    painter.fill(edge, s.color, p.clip);
                }
            }
            let r = p.bounds.inset(self.view.insets(s));
            let color = s.color;
            match n.kind {
                Kind::Text | Kind::Button | Kind::Cell | Kind::TextInput => {
                    let text = self.text_of(i);
                    let at = aligned(r, text, s, decoration);
                    painter.text(at, text, s.font_size, color, p.clip)
                }
                Kind::Separator => painter.fill(Rect { height: 1, ..r }, color, p.clip),
                Kind::Icon => {
                    let name = self.text_of(i);
                    let image = self
                        .images
                        .iter()
                        .find(|set| !name.is_empty() && set.name == name)
                        .and_then(|set| set.fit(r.width, r.height));
                    if let Some(image) = image {
                        // Centred in the content box, at its built size.
                        let (w, h) = (u32::from(image.width), u32::from(image.height));
                        let at = Rect {
                            x: r.x + ((r.width - w) / 2) as i32,
                            y: r.y + ((r.height - h) / 2) as i32,
                            width: w,
                            height: h,
                        };
                        painter.image(at, image, p.clip);
                    } else {
                        fill_box(
                            painter,
                            Rect {
                                width: r.width.min(12),
                                height: r.height.min(12),
                                ..r
                            },
                            decoration.radius,
                            color,
                            255,
                            p.clip,
                        )
                    }
                }
                Kind::ValueBar => {
                    // A styled background is the track; otherwise the built-in one.
                    if !s.fill {
                        fill_box(
                            painter,
                            r,
                            decoration.radius,
                            self.theme.color(theme::GRID, 0) & 0x00ff_ffff,
                            255,
                            p.clip,
                        );
                    }
                    if n.value <= 10000 {
                        fill_box(
                            painter,
                            Rect {
                                width: r.width * n.value as u32 / 10000,
                                ..r
                            },
                            decoration.radius,
                            color,
                            255,
                            p.clip,
                        );
                    } else {
                        painter.text(r, "--", s.font_size, color, p.clip);
                    }
                }
                Kind::Chart => self.chart(painter, n, r, color, decoration.fill_opacity, p.clip),
                _ => {}
            }
        }
        // Tracks overlay descendants; painting them during parent traversal
        // allowed a child surface to hide the only pointer scroll control.
        // They take the `ui-value-bar` look: a rounded track and accent thumb.
        for i in 0..self.layout().len {
            if let Some(bar) = self.scrollbar(i) {
                let clip = self.layout().nodes[i].clip;
                if !painter.visible(bar.track.intersect(clip)) {
                    continue;
                }
                let track = self.theme.color(theme::TRACK, 0);
                let radius = (bar.track.width / 2) as u8;
                fill_box(
                    painter,
                    bar.track,
                    radius,
                    track & 0x00ff_ffff,
                    (track >> 24) as u8,
                    clip,
                );
                fill_box(
                    painter,
                    bar.thumb,
                    radius,
                    self.theme.color(theme::ACCENT, 0) & 0x00ff_ffff,
                    255,
                    clip,
                );
            }
        }
    }
    pub fn scrollbar(&self, index: usize) -> Option<Scrollbar> {
        let p = self.layout().nodes.get(index)?;
        let s = self.layout().style(self.view, index);
        let content = p.bounds.inset(self.view.insets(s));
        let height = content.height;
        let width = content.width;
        if s.overflow != 2
            || p.hidden
            || p.disabled
            || height == 0
            || width < 8
            || p.content_height <= height
        {
            return None;
        }
        let track = Rect {
            x: content.right() - 8,
            y: content.y,
            width: 8,
            height,
        };
        let thumb_height = ((u64::from(height) * u64::from(height) / u64::from(p.content_height))
            as u32)
            .max(8)
            .min(height);
        let maximum = p.content_height - height;
        let offset =
            (u64::from(height - thumb_height) * u64::from(p.scroll_y) / u64::from(maximum)) as i32;
        Some(Scrollbar {
            track,
            thumb: Rect {
                y: track.y + offset,
                height: thumb_height,
                ..track
            },
            maximum,
        })
    }
    fn chart(
        &self,
        painter: &mut impl Painter,
        node: &Node,
        r: Rect,
        color: u32,
        fill_opacity: u8,
        clip: Rect,
    ) {
        if r.width < 48 || r.height < 32 {
            return;
        }
        let graph = Rect {
            x: r.x + 36,
            y: r.y + 6,
            width: r.width - 36,
            height: r.height - 22,
        };
        let clip = clip.intersect(r);
        for (tick, label) in ["100%", "75%", "50%", "25%", "0%"].iter().enumerate() {
            let y = graph.y + (tick as u32 * (graph.height - 1) / 4) as i32;
            painter.line(
                (graph.x, y),
                (graph.right() - 1, y),
                self.theme.color(theme::GRID, 0) & 0x00ff_ffff,
                clip,
            );
            painter.text(
                Rect {
                    x: r.x,
                    y: y - 6,
                    width: 35,
                    height: 12,
                },
                label,
                12,
                self.theme.color(theme::MUTED, 0) & 0x00ff_ffff,
                clip,
            );
        }
        let points = &self.view.points
            [node.series_start as usize..(node.series_start + node.series_len) as usize];
        let Some(first) = points.first() else {
            return;
        };
        let last = points.last().unwrap();
        let duration = last.time_ms.saturating_sub(first.time_ms);
        time_axis(painter, graph, duration, clip, self.theme);
        let plot_clip = clip.intersect(graph);
        let position = |point: &Point| {
            (point.value <= 10000).then(|| {
                let x = if duration == 0 {
                    graph.right() - 1
                } else {
                    graph.x
                        + ((point.time_ms - first.time_ms) as u128 * (graph.width - 1) as u128
                            / duration as u128) as i32
                };
                let y =
                    graph.bottom() - 1 - ((graph.height - 1) * point.value as u32 / 10000) as i32;
                (x, y)
            })
        };
        if fill_opacity != 0 {
            let mut positions = [None; 60];
            for (slot, point) in positions.iter_mut().zip(points) {
                *slot = position(point);
            }
            area(
                painter,
                &positions[..points.len()],
                graph.bottom(),
                color,
                fill_opacity,
                plot_clip,
            );
        }
        let mut previous = None;
        for point in points {
            let Some((x, y)) = position(point) else {
                previous = None;
                continue;
            };
            if let Some(from) = previous {
                painter.line(from, (x, y), color, plot_clip);
            }
            painter.fill(
                Rect {
                    x: x - 1,
                    y: y - 1,
                    width: 3,
                    height: 3,
                },
                color,
                plot_clip,
            );
            previous = Some((x, y));
        }
    }
}

/// One line of text in the content box: CSS text-align across it and
/// vertical-align within its height. Text wider than the box keeps its start;
/// a line box taller than the content box overflows both edges when centred,
/// as in CSS, and the node clip bounds what is drawn.
fn aligned(r: Rect, text: &str, s: Style, decoration: Decoration) -> Rect {
    let size = u32::from(s.font_size);
    // Left alignment does not use the advance. Avoid a second glyph walk for
    // each visible text node in every raster band.
    let width = if s.text_align == 0 {
        0
    } else {
        crate::font::ui_font_text_width(text, s.font_size)
    };
    let x = match s.text_align {
        1 => r.width.saturating_sub(width) / 2,
        2 => r.width.saturating_sub(width),
        _ => 0,
    };
    let spare = r.height as i32 - size as i32;
    let y = match decoration.vertical_align {
        1 => spare.div_euclid(2),
        2 => spare,
        _ => 0,
    };
    Rect {
        x: r.x + x as i32,
        y: r.y + y,
        width: r.width - x,
        height: (r.height as i32 - y).max(0) as u32,
    }
}

/// The area under each run of consecutive available samples, down to the
/// plot baseline. An unavailable sample breaks the area as it breaks the line.
/// Rows below every vertex share one set of spans, painted as one rectangle each.
fn area<P: Painter>(
    painter: &mut P,
    positions: &[Option<(i32, i32)>],
    bottom: i32,
    color: u32,
    alpha: u8,
    clip: Rect,
) {
    let segments = || {
        positions
            .windows(2)
            .filter_map(|pair| Some((pair[0]?, pair[1]?)))
    };
    let Some(top) = segments().map(|(a, b)| a.1.min(b.1)).min() else {
        return;
    };
    let deepest = segments().map(|(a, b)| a.1.max(b.1)).max().unwrap_or(top);
    let (first, last) = (top.max(clip.y), bottom.min(clip.bottom()));
    let rows = |painter: &mut P, y: i32, height: i32| {
        let band = Rect {
            x: clip.x,
            y,
            width: clip.width,
            height: height as u32,
        };
        if !painter.visible(band) {
            return;
        }
        let mut emit = |run: Option<(i32, i32)>| {
            if let Some((start, end)) = run {
                let span = Rect {
                    x: start,
                    width: (end - start + 1) as u32,
                    ..band
                };
                painter.fill_alpha(span, color, alpha, clip);
            }
        };
        let mut run: Option<(i32, i32)> = None;
        for ((xa, ya), (xb, yb)) in segments() {
            let Some((start, end)) = covered((xa, ya), (xb, yb), y) else {
                continue;
            };
            run = match run {
                Some((pending, stop)) if start <= stop + 1 => Some((pending, stop.max(end))),
                pending => {
                    emit(pending);
                    Some((start, end))
                }
            };
        }
        emit(run);
    };
    for y in first..deepest.min(last) {
        rows(painter, y, 1);
    }
    let uniform = deepest.max(first);
    if uniform < last {
        rows(painter, uniform, last - uniform);
    }
}

/// Columns of one segment whose line point lies at or above row `y`.
fn covered((xa, ya): (i32, i32), (xb, yb): (i32, i32), y: i32) -> Option<(i32, i32)> {
    if y >= ya.max(yb) {
        return Some((xa, xb));
    }
    if y < ya.min(yb) {
        return None;
    }
    // The row crosses the segment strictly between its endpoint rows.
    let (dx, dy) = (i64::from(xb - xa), i64::from(yb - ya));
    if dx == 0 {
        Some((xa, xa))
    } else if dy > 0 {
        Some((xa, xa + (i64::from(y - ya) * dx / dy) as i32))
    } else {
        let (numerator, denominator) = (i64::from(ya - y) * dx, -dy);
        Some((
            xa + ((numerator + denominator - 1) / denominator) as i32,
            xb,
        ))
    }
}

// ASCII labels use conservative 12px cells at 12px font size. Reserve whole
// labels before painting: adapters need not provide font metrics, and narrow
// charts omit intermediate/oldest labels rather than overlap or truncate them.
fn time_axis<P: Painter>(
    painter: &mut P,
    graph: Rect,
    duration: u64,
    clip: Rect,
    theme: theme::Theme,
) {
    let latest = SecondsAgo::new(0);
    if graph.width < latest.width() {
        return;
    }
    let right = graph.right() - latest.width() as i32;
    let tick = |painter: &mut P, x, label_x, label: &SecondsAgo| {
        painter.line(
            (x, graph.y),
            (x, graph.bottom() - 1),
            theme.color(theme::GRID, 0) & 0x00ff_ffff,
            clip,
        );
        let bounds = Rect {
            x: label_x,
            y: graph.bottom() + 4,
            width: label.width(),
            height: 12,
        };
        painter.text(
            bounds,
            label.text(),
            12,
            theme.color(theme::MUTED, 0) & 0x00ff_ffff,
            clip.intersect(bounds),
        );
    };
    tick(painter, graph.right() - 1, right, &latest);
    if duration == 0 {
        return;
    }
    let oldest = SecondsAgo::new(duration);
    let left_end = graph.x + oldest.width() as i32;
    if left_end + 8 > right {
        return;
    }
    tick(painter, graph.x, graph.x, &oldest);
    let age = duration / 2;
    if age == 0 {
        return;
    }
    let middle = SecondsAgo::new(age);
    // Position the tick at the labelled millisecond, including odd durations.
    let x = graph.x
        + (u128::from(duration - age) * u128::from(graph.width - 1) / u128::from(duration)) as i32;
    let label_x = x - middle.width() as i32 / 2;
    if label_x >= left_end + 8 && label_x + middle.width() as i32 + 8 <= right {
        tick(painter, x, label_x, &middle);
    }
}

struct SecondsAgo {
    // Full u64 milliseconds: "-18446744073709551.615 s".
    bytes: [u8; 24],
    len: usize,
}
impl SecondsAgo {
    fn new(ms: u64) -> Self {
        let mut label = Self {
            bytes: [0; 24],
            len: 0,
        };
        if ms != 0 {
            label.push(b'-');
        }
        let mut seconds = ms / 1000;
        let mut digits = [0; 20];
        let mut count = 0;
        loop {
            digits[count] = b'0' + (seconds % 10) as u8;
            count += 1;
            seconds /= 10;
            if seconds == 0 {
                break;
            }
        }
        for digit in digits[..count].iter().rev() {
            label.push(*digit);
        }
        let fraction = ms % 1000;
        if fraction != 0 {
            label.push(b'.');
            label.push(b'0' + (fraction / 100) as u8);
            if fraction % 100 != 0 {
                label.push(b'0' + (fraction / 10 % 10) as u8);
                if fraction % 10 != 0 {
                    label.push(b'0' + (fraction % 10) as u8);
                }
            }
        }
        label.push(b' ');
        label.push(b's');
        label
    }
    fn push(&mut self, byte: u8) {
        self.bytes[self.len] = byte;
        self.len += 1;
    }
    fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
    fn width(&self) -> u32 {
        self.len as u32 * 12
    }
}
