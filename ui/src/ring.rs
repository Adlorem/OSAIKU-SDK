//! `ui-ring`: a percent drawn as an arc round a circular track, clockwise
//! from the top, in the largest circle that fits the content box. The ring is
//! a tenth of the diameter thick. Integer-only and allocation-free: coverage
//! is sampled 8x8 where an edge crosses a pixel and reaches every adapter as
//! one `Painter::fill_alpha` per run of equal alpha in a row.
use crate::{layout::Rect, paint::Painter};

/// Subsamples per axis where an edge crosses a pixel.
const SAMPLES: i64 = 8;
/// Sub-pixel units per pixel: sample centres are odd multiples.
const UNIT: i64 = 2 * SAMPLES;
/// Fixed point of the arc's end direction.
const ONE: i64 = 1 << 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// The ring's square, centred in the content box.
    pub outer: Rect,
    pub thickness: u32,
}

/// None when the content box is too small for a ring.
pub fn geometry(content: Rect) -> Option<Geometry> {
    let d = content.width.min(content.height);
    if d < 8 {
        return None;
    }
    Some(Geometry {
        outer: Rect {
            x: content.x + ((content.width - d) / 2) as i32,
            y: content.y + ((content.height - d) / 2) as i32,
            width: d,
            height: d,
        },
        thickness: (d / 10).max(3),
    })
}

/// sin of `a` ten-thousandths of a turn, times `ONE` (Bhaskara's
/// approximation: within 0.2% of a unit, a tenth of a pixel at radius 64).
fn sin(a: i64) -> i64 {
    let a = a.rem_euclid(10_000);
    if a >= 5_000 {
        return -sin(a - 5_000);
    }
    let p = a * (5_000 - a);
    4 * p * ONE / (31_250_000 - p)
}

/// The part of the ring from the top clockwise through `sweep`
/// ten-thousandths of a turn (10000 is the whole circle).
#[derive(Clone, Copy)]
struct Arc {
    cx: i64,
    cy: i64,
    inner: i64,
    outer: i64,
    sweep: i64,
    /// Direction of the arc's end from the centre, `ONE` long.
    end: (i64, i64),
}

impl Arc {
    fn new(g: Geometry, sweep: u16) -> Self {
        let d = i64::from(g.outer.width);
        let t = i64::from(g.thickness);
        let sweep = i64::from(sweep.min(10_000));
        Self {
            cx: i64::from(g.outer.x) * UNIT + d * UNIT / 2,
            cy: i64::from(g.outer.y) * UNIT + d * UNIT / 2,
            inner: ((d - 2 * t) * UNIT / 2).pow(2),
            outer: (d * UNIT / 2).pow(2),
            sweep,
            end: (sin(sweep), -sin(sweep + 2_500)),
        }
    }

    /// Whether the direction (dx, dy) from the centre lies in the sweep.
    fn within(&self, dx: i64, dy: i64) -> bool {
        if self.sweep >= 10_000 {
            return true;
        }
        if self.sweep == 0 {
            return false;
        }
        // Screen y grows downward: positive cross products turn clockwise.
        let cross = |a: (i64, i64), b: (i64, i64)| a.0 * b.1 - a.1 * b.0;
        let v = (dx, dy);
        if self.sweep <= 5_000 {
            dx >= 0 && cross(v, self.end) >= 0
        } else {
            !(cross(self.end, v) > 0 && dx < 0)
        }
    }

    fn inside(&self, x: i64, y: i64) -> bool {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let r = dx * dx + dy * dy;
        r >= self.inner && r <= self.outer && self.within(dx, dy)
    }

    /// Coverage of pixel (x, y), 0..=255.
    fn coverage(&self, x: i64, y: i64) -> u32 {
        let (x0, y0) = (x * UNIT, y * UNIT);
        let (x1, y1) = (x0 + UNIT, y0 + UNIT);
        // Nearest and farthest points of the pixel from the centre.
        let near = |lo: i64, hi: i64, c: i64| (lo.max(c.min(hi)) - c).pow(2);
        let far = |lo: i64, hi: i64, c: i64| (lo - c).pow(2).max((hi - c).pow(2));
        let nearest = near(x0, x1, self.cx) + near(y0, y1, self.cy);
        let farthest = far(x0, x1, self.cx) + far(y0, y1, self.cy);
        if nearest > self.outer || farthest < self.inner {
            return 0;
        }
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
            .map(|(cx, cy)| self.within(cx - self.cx, cy - self.cy));
        let radial = nearest >= self.inner && farthest <= self.outer;
        // Within a pixel and a half of either edge of the sweep, sample:
        // a thin arc can cross a pixel without containing a corner.
        let (mx, my) = (x0 + UNIT / 2 - self.cx, y0 + UNIT / 2 - self.cy);
        let near_edge = self.sweep > 0
            && self.sweep < 10_000
            && (mx.abs() < UNIT * 3 / 2
                || (mx * self.end.1 - my * self.end.0).abs() < UNIT * 3 / 2 * ONE);
        if radial && !near_edge && corners.iter().all(|c| *c) {
            return 255;
        }
        if radial && !near_edge && corners.iter().all(|c| !*c) {
            return 0;
        }
        let mut hits = 0;
        for j in 0..SAMPLES {
            for i in 0..SAMPLES {
                if self.inside(x0 + 2 * i + 1, y0 + 2 * j + 1) {
                    hits += 1;
                }
            }
        }
        (hits * 255 / (SAMPLES * SAMPLES)) as u32
    }

    fn fill(
        &self,
        painter: &mut (impl Painter + ?Sized),
        g: Geometry,
        color: u32,
        alpha: u8,
        clip: Rect,
    ) {
        let area = g.outer.intersect(clip);
        for y in area.y..area.bottom() {
            if !painter.visible(Rect {
                y,
                height: 1,
                ..area
            }) {
                continue;
            }
            let mut x = area.x;
            while x < area.right() {
                let c = self.coverage(x.into(), y.into());
                let mut end = x + 1;
                while end < area.right() && self.coverage(end.into(), y.into()) == c {
                    end += 1;
                }
                if c != 0 {
                    painter.fill_alpha(
                        Rect {
                            x,
                            y,
                            width: (end - x) as u32,
                            height: 1,
                        },
                        color,
                        (c * u32::from(alpha) / 255) as u8,
                        clip,
                    );
                }
                x = end;
            }
        }
    }
}

/// Paint the track, then the arc for `value` (0..=10000; anything above is
/// unavailable and shows the track alone). Colours are RGB.
pub fn paint(
    painter: &mut (impl Painter + ?Sized),
    content: Rect,
    value: u16,
    track: (u32, u8),
    color: u32,
    alpha: u8,
    clip: Rect,
) {
    let Some(g) = geometry(content) else {
        return;
    };
    Arc::new(g, 10_000).fill(painter, g, track.0, track.1, clip);
    if value <= 10_000 && value != 0 {
        Arc::new(g, value).fill(painter, g, color, alpha, clip);
    }
}
