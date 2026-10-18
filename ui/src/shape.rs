//! Rounded box coverage for backgrounds, borders, markers and chart areas.
//! Integer-only and allocation-free. Shapes reach adapters as bounded
//! rectangles through `Painter::fill_alpha`, and the cells an arc crosses
//! through `Painter::fill_shape`, so every adapter that keeps the default
//! methods shows the same pixels.
use crate::{layout::Rect, paint::Painter};

/// Subsamples per axis for a pixel that a corner arc crosses.
const SAMPLES: u64 = 8;
/// Larger radii cannot occur inside the 4096-pixel layout bound.
const MAX_RADIUS: u32 = 2048;

#[derive(Clone, Copy)]
struct Shape {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    /// Elliptical (horizontal, vertical) radii: top-left, top-right,
    /// bottom-right, bottom-left. Zero on either axis is a square corner.
    radii: [(u32, u32); 4],
}

impl Shape {
    fn corner(&self, x: i64, y: i64) -> Option<(u32, u32, u64, u64)> {
        let [tl, tr, br, bl] = self
            .radii
            .map(|(rx, ry)| (rx, ry, i64::from(rx), i64::from(ry)));
        // Distance in whole pixels from the arc's centre to the pixel's
        // nearer edge, measured towards the box corner.
        let found = if x < self.x0 + tl.2 && y < self.y0 + tl.3 {
            (tl, self.x0 + tl.2 - 1 - x, self.y0 + tl.3 - 1 - y)
        } else if x >= self.x1 - tr.2 && y < self.y0 + tr.3 {
            (tr, x - (self.x1 - tr.2), self.y0 + tr.3 - 1 - y)
        } else if x >= self.x1 - br.2 && y >= self.y1 - br.3 {
            (br, x - (self.x1 - br.2), y - (self.y1 - br.3))
        } else if x < self.x0 + bl.2 && y >= self.y1 - bl.3 {
            (bl, self.x0 + bl.2 - 1 - x, y - (self.y1 - bl.3))
        } else {
            return None;
        };
        let ((rx, ry, ..), u, v) = found;
        Some((rx, ry, u as u64, v as u64))
    }

    /// Coverage of pixel (x, y), 0..=255.
    fn coverage(&self, x: i64, y: i64) -> u32 {
        if x < self.x0 || x >= self.x1 || y < self.y0 || y >= self.y1 {
            return 0;
        }
        self.corner(x, y)
            .map_or(255, |(rx, ry, u, v)| ellipse(rx, ry, u, v))
    }

    /// Row and column edges: every place where coverage may change.
    fn edges(&self, vertical: bool, out: &mut [i64; 16], len: &mut usize) {
        let (start, end) = if vertical {
            (self.y0, self.y1)
        } else {
            (self.x0, self.x1)
        };
        let mut push = |value: i64| {
            out[*len] = value;
            *len += 1;
        };
        push(start);
        push(end);
        for (i, (rx, ry)) in self.radii.into_iter().enumerate() {
            let (radius, near_start) = if vertical {
                (ry, i < 2)
            } else {
                (rx, i == 0 || i == 3)
            };
            push(if near_start {
                start + i64::from(radius)
            } else {
                end - i64::from(radius)
            });
        }
    }
}

/// Coverage of the pixel whose nearer edge lies (u, v) pixels from the centre
/// of an ellipse with radii (rx, ry). Only pixels the arc crosses are sampled.
fn ellipse(rx: u32, ry: u32, u: u64, v: u64) -> u32 {
    let scale = 2 * SAMPLES;
    let (rx, ry) = (u64::from(rx) * scale, u64::from(ry) * scale);
    let inside = |x: u64, y: u64| x * x * ry * ry + y * y * rx * rx <= rx * rx * ry * ry;
    let (u, v) = (u * scale, v * scale);
    if inside(u + scale, v + scale) {
        return 255;
    }
    if !inside(u, v) {
        return 0;
    }
    let mut count = 0;
    for j in 0..SAMPLES {
        for i in 0..SAMPLES {
            count += u64::from(inside(u + 2 * i + 1, v + 2 * j + 1));
        }
    }
    ((count * 255 + SAMPLES * SAMPLES / 2) / (SAMPLES * SAMPLES)) as u32
}

fn sorted(values: &mut [i64; 32], len: usize) -> usize {
    let values = &mut values[..len];
    values.sort_unstable();
    let mut unique = 0;
    for i in 0..values.len() {
        if unique == 0 || values[i] != values[unique - 1] {
            values[unique] = values[i];
            unique += 1;
        }
    }
    unique
}

/// A rounded box, filled or as its border ring, with the alpha it is painted
/// at. `alpha` gives the exact value the default painter methods emit for a
/// pixel, so a recipe planner can retain the shape instead of its runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundedShape {
    rect: Rect,
    /// Already clamped to half the shorter side.
    radius: u16,
    /// Border widths (top, right, bottom, left) of a ring; `None` fills.
    widths: Option<[u16; 4]>,
    alpha: u8,
}

impl RoundedShape {
    /// `None` when nothing would be painted: a ring with no border.
    fn new(rect: Rect, radius: u32, widths: Option<[u32; 4]>, alpha: u8) -> Option<Self> {
        let radius = radius
            .min(rect.width / 2)
            .min(rect.height / 2)
            .min(MAX_RADIUS);
        let widths = match widths {
            Some([0, 0, 0, 0]) => return None,
            // Wider borders cannot fit the 4096-pixel layout bound.
            Some(widths) => Some(widths.map(|w| w.min(u32::from(u16::MAX)) as u16)),
            None => None,
        };
        Some(Self {
            rect,
            radius: radius as u16,
            widths,
            alpha,
        })
    }
    /// The same shape `dx`, `dy` pixels further right and down.
    pub fn translated(self, dx: i32, dy: i32) -> Self {
        Self {
            rect: Rect {
                x: self.rect.x.saturating_add(dx),
                y: self.rect.y.saturating_add(dy),
                ..self.rect
            },
            ..self
        }
    }
    /// The painted alpha of pixel (x, y), 0..=255.
    pub fn alpha(&self, x: i32, y: i32) -> u8 {
        let (x, y) = (i64::from(x), i64::from(y));
        let mut coverage = self.outer().coverage(x, y);
        if let Some(inner) = self.inner() {
            coverage = coverage.saturating_sub(inner.coverage(x, y));
        }
        ((coverage * u32::from(self.alpha) + 127) / 255) as u8
    }
    fn outer(&self) -> Shape {
        let radius = u32::from(self.radius);
        Shape {
            x0: i64::from(self.rect.x),
            y0: i64::from(self.rect.y),
            x1: i64::from(self.rect.x) + i64::from(self.rect.width),
            y1: i64::from(self.rect.y) + i64::from(self.rect.height),
            radii: [(radius, radius); 4],
        }
    }
    /// The hole of a ring; `None` for a fill, or for a border that meets
    /// itself and so covers the whole box.
    fn inner(&self) -> Option<Shape> {
        let [top, right, bottom, left] = self.widths?.map(u32::from);
        let outer = self.outer();
        let inset = |width: u32| u32::from(self.radius).saturating_sub(width);
        let shape = Shape {
            x0: outer.x0 + i64::from(left),
            y0: outer.y0 + i64::from(top),
            x1: outer.x1 - i64::from(right),
            y1: outer.y1 - i64::from(bottom),
            radii: [
                (inset(left), inset(top)),
                (inset(right), inset(top)),
                (inset(right), inset(bottom)),
                (inset(left), inset(bottom)),
            ],
        };
        (shape.x0 < shape.x1 && shape.y0 < shape.y1).then_some(shape)
    }
}

/// Fill a box with a uniform corner `radius`, or with `widths` (top, right,
/// bottom, left) paint only its border ring. CSS semantics: the radius is
/// clamped to half the shorter side, and the ring's inner corners keep the
/// outer centres (inner radius = outer radius - adjacent border width).
///
/// The box is cut at every edge where coverage may change. A cell no arc
/// crosses has one alpha and becomes one rectangle; a cell an arc crosses goes
/// to `Painter::fill_shape`.
pub(crate) fn rounded<P: Painter + ?Sized>(
    painter: &mut P,
    rect: Rect,
    radius: u32,
    widths: Option<[u32; 4]>,
    color: u32,
    alpha: u8,
    clip: Rect,
) {
    let area = rect.intersect(clip);
    if alpha == 0 || area.width == 0 || area.height == 0 {
        return;
    }
    let Some(shape) = RoundedShape::new(rect, radius, widths, alpha) else {
        return;
    };
    let (outer, inner) = (shape.outer(), shape.inner());
    if shape.radius == 0 && inner.is_none() {
        painter.fill_alpha(rect, color, alpha, clip);
        return;
    }
    let mut rows = [0; 32];
    let mut columns = [0; 32];
    let (mut row_len, mut column_len) = (0, 0);
    for shape in [Some(outer), inner].into_iter().flatten() {
        let mut edges = [0; 16];
        let mut len = 0;
        shape.edges(true, &mut edges, &mut len);
        rows[row_len..row_len + len].copy_from_slice(&edges[..len]);
        row_len += len;
        len = 0;
        shape.edges(false, &mut edges, &mut len);
        columns[column_len..column_len + len].copy_from_slice(&edges[..len]);
        column_len += len;
    }
    let row_len = sorted(&mut rows, row_len);
    let column_len = sorted(&mut columns, column_len);
    let (top, bottom) = (i64::from(area.y), i64::from(area.bottom()));
    let (left, right) = (i64::from(area.x), i64::from(area.right()));
    for pair in rows[..row_len].windows(2) {
        let (first, end) = (pair[0].max(top), pair[1].min(bottom));
        if first >= end {
            continue;
        }
        let band = Rect {
            y: first as i32,
            height: (end - first) as u32,
            ..area
        };
        if !painter.visible(band) {
            continue;
        }
        let flush = |painter: &mut P, run: Option<(i64, i64, u8)>| {
            if let Some((start, stop, value)) = run.filter(|run| run.2 != 0) {
                painter.fill_alpha(
                    Rect {
                        x: start as i32,
                        width: (stop - start) as u32,
                        ..band
                    },
                    color,
                    value,
                    clip,
                );
            }
        };
        let mut run: Option<(i64, i64, u8)> = None;
        for pair in columns[..column_len].windows(2) {
            let (x0, x1) = (pair[0].max(left), pair[1].min(right));
            if x0 >= x1 {
                continue;
            }
            // The edges include every corner's bounds, so a cell lies wholly
            // inside or outside each corner region.
            if outer.corner(x0, first).is_some()
                || inner.is_some_and(|s| s.corner(x0, first).is_some())
            {
                flush(painter, run.take());
                let cell = Rect {
                    x: x0 as i32,
                    width: (x1 - x0) as u32,
                    ..band
                };
                painter.fill_shape(cell, shape, color, clip);
                continue;
            }
            let value = shape.alpha(x0 as i32, first as i32);
            run = match run {
                Some((start, stop, previous)) if stop == x0 && previous == value => {
                    Some((start, x1, value))
                }
                pending => {
                    flush(painter, pending);
                    Some((x0, x1, value))
                }
            };
        }
        flush(painter, run);
    }
}

/// The default `Painter::fill_shape`: one `fill_alpha` per row run of equal
/// alpha inside `area`; zero alpha is skipped.
pub(crate) fn runs<P: Painter + ?Sized>(
    painter: &mut P,
    area: Rect,
    shape: RoundedShape,
    color: u32,
    clip: Rect,
) {
    let area = area.intersect(clip);
    for y in area.y..area.bottom() {
        let mut run: Option<(i32, i32, u8)> = None;
        // One step past the last column flushes the final run.
        for x in area.x..=area.right() {
            let value = (x < area.right()).then(|| shape.alpha(x, y));
            match run {
                Some((start, stop, previous)) if Some(previous) == value && stop == x => {
                    run = Some((start, x + 1, previous));
                }
                pending => {
                    if let Some((start, stop, previous)) = pending.filter(|run| run.2 != 0) {
                        painter.fill_alpha(
                            Rect {
                                x: start,
                                y,
                                width: (stop - start) as u32,
                                height: 1,
                            },
                            color,
                            previous,
                            clip,
                        );
                    }
                    run = value.map(|value| (x, x + 1, value));
                }
            }
        }
    }
}
