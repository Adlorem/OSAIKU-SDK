//! Bounded, symmetric shadow/glow lookup tables for a native window style.
use super::geometry::{image_len, valid as ui_window_compositor_bounds_valid, Bounds};
use super::pixels::blend_shape_channel;
use super::style::{PreparedStyle, WindowStyle, MAX_EXTENT, MAX_RADIUS};

pub const EXTENT: u32 = super::frame::STYLE.extent();
const DISTANCE_STEP: u32 = 128;
const LAST_STEP: usize = (MAX_EXTENT * 2) as usize;
const CORNER_SIZE: usize = (MAX_EXTENT + MAX_RADIUS) as usize;
const fn sqrt(value: u32) -> u32 {
    let mut low = 0;
    let mut high = 1 << 16;
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        if mid <= value / mid {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

const fn falloff(distance: u32, extent: u32, maximum: u32) -> u8 {
    let limit = extent * 256;
    if distance >= limit {
        return 0;
    }
    let remaining = limit - distance;
    // A quadratic tail reaches zero smoothly; distances are in 8.8 pixels.
    ((maximum as u64 * remaining as u64 * remaining as u64 + (limit as u64 * limit as u64 / 2))
        / (limit as u64 * limit as u64)) as u8
}

const fn distance_step(distance: u32, extent: u32) -> u8 {
    let step = (distance + DISTANCE_STEP / 2) / DISTANCE_STEP;
    if step >= extent * 2 {
        (extent * 2) as u8
    } else {
        step as u8
    }
}
const fn corner_mask(radius: u32, extent: u32, size: usize) -> [[u8; CORNER_SIZE]; CORNER_SIZE] {
    let mut result = [[0; CORNER_SIZE]; CORNER_SIZE];
    let mut y = 0;
    while y < size {
        let mut x = 0;
        while x < size {
            let dx = ((extent + radius) * 2).saturating_sub(x as u32 * 2 + 1);
            let dy = ((extent + radius) * 2).saturating_sub(y as u32 * 2 + 1);
            let distance = sqrt((dx * dx + dy * dy) * 16384);
            result[y][x] = distance_step(distance.saturating_sub(radius * 256), extent);
            x += 1;
        }
        y += 1;
    }
    result
}
pub(crate) struct Tables {
    rounded: [[u8; CORNER_SIZE]; CORNER_SIZE],
    square: [[u8; CORNER_SIZE]; CORNER_SIZE],
    edge: [u8; MAX_EXTENT as usize],
    channels: [[[u8; 256]; LAST_STEP + 1]; 3],
}
impl Tables {
    pub(crate) const fn new(style: WindowStyle) -> Self {
        let extent = style.extent();
        let mut result = Self {
            rounded: corner_mask(
                style.corner_radius,
                extent,
                (extent + style.corner_radius) as usize,
            ),
            square: corner_mask(0, extent, extent as usize),
            edge: [0; MAX_EXTENT as usize],
            channels: [[[0; 256]; LAST_STEP + 1]; 3],
        };
        let mut x = 0;
        while x < extent {
            result.edge[x as usize] = distance_step((extent - x) * 256 - 128, extent);
            x += 1;
        }
        let mut component = 0;
        while component < 3 {
            let shadow_color = (style.shadow.color >> (component * 8)) as u8;
            let glow_color = (style.glow.color >> (component * 8)) as u8;
            let mut step = 0;
            while step <= (extent * 2) as usize {
                let distance = step as u32 * DISTANCE_STEP;
                let shadow = falloff(distance, style.shadow.extent, style.shadow.opacity as u32);
                let glow = falloff(distance, style.glow.extent, style.glow.opacity as u32);
                let mut channel = 0;
                while channel < 256 {
                    let shaded = blend_shape_channel(shadow_color, channel as u8, shadow);
                    result.channels[component][step][channel] =
                        blend_shape_channel(glow_color, shaded, glow);
                    channel += 1;
                }
                step += 1;
            }
            component += 1;
        }
        result
    }
}

/// Footprint geometry needs only the small style value, not renderer tables.
pub fn footprint(window: Bounds, screen: Bounds) -> Option<Bounds> {
    super::frame::STYLE.footprint(window, screen)
}
pub fn shade(background: u32, window: Bounds, x: u32, y: u32) -> u32 {
    super::style::NATIVE.shade(background, window, x, y)
}

pub fn apply(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    underlay: &[u32],
    underlay_bounds: Bounds,
    underlay_stride: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<()> {
    super::style::NATIVE.apply_shadow(
        image,
        screen,
        window,
        underlay,
        underlay_bounds,
        underlay_stride,
        checkpoint,
    )
}

pub fn apply_clipped(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    underlay: &[u32],
    underlay_bounds: Bounds,
    underlay_stride: u32,
    clip: Bounds,
    checkpoint: &mut impl FnMut(),
) -> Option<()> {
    super::style::NATIVE.apply_shadow_clipped(
        image,
        screen,
        window,
        underlay,
        underlay_bounds,
        underlay_stride,
        clip,
        checkpoint,
    )
}

/// Geometry and mask choice prepared once per pass, outside the pixel loop.
/// A moved window changes only these small coordinates, never the lookup tables.
pub(crate) struct Placement<'a> {
    tables: &'a Tables,
    mask: &'a [[u8; CORNER_SIZE]; CORNER_SIZE],
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    radius: i64,
    extent: i64,
}
impl Placement<'_> {
    #[inline]
    fn step_at(&self, x: u32, y: u32) -> usize {
        let x = i64::from(x);
        let y = i64::from(y);
        let near_x = (x - self.left).min(self.right - x);
        let near_y = (y - self.top).min(self.bottom - y);
        let sx = near_x + self.extent;
        let sy = near_y + self.extent;
        if sx < 0 || sy < 0 {
            return (self.extent * 2) as usize;
        }
        if near_x < self.radius && near_y < self.radius {
            self.mask[sy as usize][sx as usize] as usize
        } else if near_x < 0 {
            self.tables.edge[sx as usize] as usize
        } else if near_y < 0 {
            self.tables.edge[sy as usize] as usize
        } else {
            0
        }
    }
    #[inline]
    pub(crate) fn shade(&self, background: u32, x: u32, y: u32) -> u32 {
        let step = self.step_at(x, y);
        (u32::from(self.tables.channels[2][step][((background >> 16) & 0xff) as usize]) << 16)
            | (u32::from(self.tables.channels[1][step][((background >> 8) & 0xff) as usize]) << 8)
            | u32::from(self.tables.channels[0][step][(background & 0xff) as usize])
    }
}
impl PreparedStyle {
    pub(crate) fn shadow_placement(&self, window: Bounds) -> Placement<'_> {
        let radius = self.style.corner_radius;
        let rounded = window.width >= radius * 2 && window.height >= radius * 2;
        Placement {
            tables: &self.shadow,
            mask: if rounded {
                &self.shadow.rounded
            } else {
                &self.shadow.square
            },
            left: i64::from(window.x),
            top: i64::from(window.y),
            right: i64::from(window.x) + i64::from(window.width) - 1,
            bottom: i64::from(window.y) + i64::from(window.height) - 1,
            radius: if rounded { i64::from(radius) } else { 0 },
            extent: i64::from(self.style.extent()),
        }
    }
    /// Shade an underlay sample at an uncovered halo/corner position. The host
    /// applies window coverage separately; this returns 24-bit XRGB.
    pub fn shade(&self, background: u32, window: Bounds, x: u32, y: u32) -> u32 {
        self.shadow_placement(window).shade(background, x, y)
    }

    /// Visit only the outer halo. Corner coverage uses `shade` in its own pass,
    /// so retained foreground and a previous shadow are never blended twice.
    pub fn apply_shadow(
        &self,
        image: &mut [u32],
        screen: Bounds,
        window: Bounds,
        underlay: &[u32],
        underlay_bounds: Bounds,
        underlay_stride: u32,
        checkpoint: &mut impl FnMut(),
    ) -> Option<()> {
        self.apply_shadow_clipped(
            image,
            screen,
            window,
            underlay,
            underlay_bounds,
            underlay_stride,
            screen,
            checkpoint,
        )
    }

    /// Apply only within `clip`; storage/geometry refusal occurs before writes.
    pub fn apply_shadow_clipped(
        &self,
        image: &mut [u32],
        screen: Bounds,
        window: Bounds,
        underlay: &[u32],
        underlay_bounds: Bounds,
        underlay_stride: u32,
        clip: Bounds,
        checkpoint: &mut impl FnMut(),
    ) -> Option<()> {
        if !ui_window_compositor_bounds_valid(clip)
            || self.style.footprint(window, screen)? != underlay_bounds
            || image.len() != image_len(screen)?
            || underlay_stride < underlay_bounds.width
            || underlay.len()
                < ((underlay_bounds.height - 1) as usize)
                    .checked_mul(underlay_stride as usize)?
                    .checked_add(underlay_bounds.width as usize)?
        {
            return None;
        }
        if self.style.extent() == 0 {
            return Some(());
        }
        let shadow = self.shadow_placement(window);
        let right = window.x + window.width;
        let bottom = window.y + window.height;
        let end = underlay_bounds.x + underlay_bounds.width;
        for y in underlay_bounds.y.max(clip.y)
            ..(underlay_bounds.y + underlay_bounds.height).min(clip.y + clip.height)
        {
            let spans = if y >= window.y && y < bottom {
                [(underlay_bounds.x, window.x), (right, end)]
            } else {
                [(underlay_bounds.x, end), (end, end)]
            };
            for (start, end) in spans {
                let end = end.min(clip.x + clip.width);
                for first in (start.max(clip.x)..end).step_by(1024) {
                    checkpoint();
                    for x in first..(first + 1024).min(end) {
                        let source = (y - underlay_bounds.y) as usize * underlay_stride as usize
                            + (x - underlay_bounds.x) as usize;
                        image[y as usize * screen.width as usize + x as usize] =
                            shadow.shade(underlay[source], x, y);
                    }
                }
            }
        }
        Some(())
    }
}
