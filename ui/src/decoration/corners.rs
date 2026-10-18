//! Rounded coverage and one-pixel outlines, prepared once per native style.
use super::geometry::{image_len, valid, Bounds};
use super::pixels::blend_shape_xrgb;
use super::style::{PreparedStyle, MAX_RADIUS};

/// Built-in frame radius in physical pixels. Custom styles use WindowStyle.
pub const RADIUS: u32 = super::frame::STYLE.corner_radius;
const BORDER: u32 = 1;
const SIZE: usize = (MAX_RADIUS * MAX_RADIUS) as usize;
pub(crate) struct Masks {
    coverage: [u8; SIZE],
    ring: [u8; SIZE],
}
impl Masks {
    pub(crate) const fn new(radius: u32) -> Self {
        let mut masks = Self {
            coverage: [0; SIZE],
            ring: [0; SIZE],
        };
        let mut y = 0;
        while y < radius {
            let mut x = 0;
            while x < radius {
                let mut outer = 0u32;
                let mut inner = 0u32;
                // Most pixels are wholly inside/outside the ring. Only its
                // boundary needs 16x16 samples, even for a maximum-size mask.
                let far_x = (radius - x) * 32 - 1;
                let far_y = (radius - y) * 32 - 1;
                let near_x = (radius - x - 1) * 32 + 1;
                let near_y = (radius - y - 1) * 32 + 1;
                let inner_limit = ((radius - BORDER) * 32).pow(2);
                let outer_limit = (radius * 32).pow(2);
                let inside = far_x * far_x + far_y * far_y <= inner_limit;
                let outside = near_x * near_x + near_y * near_y > outer_limit;
                if inside {
                    outer = 256;
                    inner = 256;
                }
                let mut sy = 0;
                while !inside && !outside && sy < 16 {
                    let mut sx = 0;
                    while sx < 16 {
                        let dx = (x * 32 + sx * 2 + 1) as i32 - (radius * 32) as i32;
                        let dy = (y * 32 + sy * 2 + 1) as i32 - (radius * 32) as i32;
                        let squared = (dx * dx + dy * dy) as u32;
                        if squared <= (radius * 32).pow(2) {
                            outer += 1;
                        }
                        if squared <= ((radius - BORDER) * 32).pow(2) {
                            inner += 1;
                        }
                        sx += 1;
                    }
                    sy += 1;
                }
                let outer = (outer * 255 + 128) / 256;
                let inner = (inner * 255 + 128) / 256;
                let i = (y * radius + x) as usize;
                masks.coverage[i] = outer as u8;
                masks.ring[i] = if outer == 0 {
                    255
                } else {
                    (255 - inner * 255 / outer) as u8
                };
                x += 1;
            }
            y += 1;
        }
        masks
    }
}

/// Built-in style convenience functions. PreparedStyle supplies custom styles.
pub fn frame_surface(
    surface: &mut [u32],
    width: u32,
    height: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<()> {
    super::style::NATIVE.frame_surface(surface, width, height, checkpoint)
}

pub fn frame_surface_rows(
    surface: &mut [u32],
    width: u32,
    height: u32,
    first: u32,
    end: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<()> {
    super::style::NATIVE.frame_surface_rows(surface, width, height, first, end, checkpoint)
}

pub fn apply(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    surface: &[u32],
    underlay: &[u32],
    underlay_bounds: Bounds,
    underlay_stride: u32,
) -> Option<()> {
    super::style::NATIVE.apply_corners(
        image,
        screen,
        window,
        surface,
        underlay,
        underlay_bounds,
        underlay_stride,
    )
}

pub fn apply_clipped(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    surface: &[u32],
    underlay: &[u32],
    underlay_bounds: Bounds,
    underlay_stride: u32,
    clip: Bounds,
) -> Option<()> {
    super::style::NATIVE.apply_corners_clipped(
        image,
        screen,
        window,
        surface,
        underlay,
        underlay_bounds,
        underlay_stride,
        clip,
    )
}

/// The four native corner boxes; None for square/small or invalid windows.
pub fn boxes(window: Bounds) -> Option<[Bounds; 4]> {
    super::frame::STYLE.corner_boxes(window)
}
pub fn rounds(window: Bounds) -> bool {
    boxes(window).is_some()
}

impl PreparedStyle {
    /// Add the one-pixel outline to private foreground. No backdrop enters this
    /// cache. A size change or style change requires rerasterizing the cache.
    pub fn frame_surface(
        &self,
        surface: &mut [u32],
        width: u32,
        height: u32,
        checkpoint: &mut impl FnMut(),
    ) -> Option<()> {
        self.frame_surface_rows(surface, width, height, 0, height, checkpoint)
    }
    pub fn frame_surface_rows(
        &self,
        surface: &mut [u32],
        width: u32,
        height: u32,
        first: u32,
        end: u32,
        checkpoint: &mut impl FnMut(),
    ) -> Option<()> {
        if first > end || end > height {
            return None;
        }
        let radius = self.style.corner_radius;
        let edge = self.style.border_color;
        let light = self.style.focus_color;
        let window = Bounds::new(0, 0, width, height);
        if surface.len() != image_len(window)? {
            return None;
        }
        for y in first..end {
            checkpoint();
            let row = &mut surface[y as usize * width as usize..(y + 1) as usize * width as usize];
            if y < BORDER || y >= height - BORDER {
                let color = if y < BORDER { light } else { edge } | 0xff00_0000;
                for portion in row.chunks_mut(1024) {
                    portion.fill(color);
                    checkpoint();
                }
            } else {
                row[..BORDER as usize].fill(edge | 0xff00_0000);
                row[width as usize - BORDER as usize..].fill(edge | 0xff00_0000);
            }
        }
        let Some(corners) = self.style.corner_boxes(window) else {
            return Some(());
        };
        for (index, corner) in corners.iter().enumerate() {
            for y in first.saturating_sub(corner.y)..radius.min(end.saturating_sub(corner.y)) {
                for x in 0..radius {
                    let sx = if index & 1 != 0 { radius - 1 - x } else { x };
                    let sy = if index & 2 != 0 { radius - 1 - y } else { y };
                    let coverage = self.corners.ring[(sy * radius + sx) as usize];
                    let pixel = &mut surface[((corner.y + y) * width + corner.x + x) as usize];
                    let color = if corner.y + y < BORDER { light } else { edge };
                    *pixel = super::pixels::foreground_over(color, coverage, *pixel);
                }
            }
        }
        Some(())
    }

    /// Blend the four corner masks over the captured underlay and this style's
    /// shadow. Call after placing foreground in `image`; square windows succeed
    /// without writes. The underlay must cover this style's clipped footprint.
    pub fn apply_corners(
        &self,
        image: &mut [u32],
        screen: Bounds,
        window: Bounds,
        surface: &[u32],
        underlay: &[u32],
        underlay_bounds: Bounds,
        underlay_stride: u32,
    ) -> Option<()> {
        self.apply_corners_clipped(
            image,
            screen,
            window,
            surface,
            underlay,
            underlay_bounds,
            underlay_stride,
            screen,
        )
    }

    /// Like `apply_corners`, limited to `clip`, with checked, caller-owned RAM.
    pub fn apply_corners_clipped(
        &self,
        image: &mut [u32],
        screen: Bounds,
        window: Bounds,
        surface: &[u32],
        underlay: &[u32],
        underlay_bounds: Bounds,
        underlay_stride: u32,
        clip: Bounds,
    ) -> Option<()> {
        let radius = self.style.corner_radius;
        if !valid(clip)
            || window.x.checked_add(window.width)? > screen.width
            || window.y.checked_add(window.height)? > screen.height
            || underlay_bounds != self.style.footprint(window, screen)?
            || underlay_stride < underlay_bounds.width
            || surface.len() != (window.width as usize).checked_mul(window.height as usize)?
            || image.len() != image_len(screen)?
            || underlay.len()
                < ((underlay_bounds.height - 1) as usize)
                    .checked_mul(underlay_stride as usize)?
                    .checked_add(underlay_bounds.width as usize)?
        {
            return None;
        }
        let Some(corners) = self.style.corner_boxes(window) else {
            return Some(());
        };
        let shadow = self.shadow_placement(window);
        for (index, corner) in corners.iter().enumerate() {
            // Corner order is top-left, top-right, bottom-left, bottom-right, so
            // the low bit mirrors horizontally and the high bit vertically.
            let mirror_x = index & 1 != 0;
            let mirror_y = index & 2 != 0;
            for row in 0..radius {
                for column in 0..radius {
                    let sample_x = if mirror_x {
                        radius - 1 - column
                    } else {
                        column
                    };
                    let sample_y = if mirror_y { radius - 1 - row } else { row };
                    let coverage = self.corners.coverage[(sample_y * radius + sample_x) as usize];
                    if coverage == u8::MAX {
                        continue;
                    }
                    let x = corner.x + column;
                    let y = corner.y + row;
                    if x < clip.x
                        || y < clip.y
                        || x >= clip.x + clip.width
                        || y >= clip.y + clip.height
                    {
                        continue;
                    }
                    // Read the composed foreground, allowing an optional host
                    // material pass before rounding. Never copy backdrop into cache.
                    let front = *image.get(y as usize * screen.width as usize + x as usize)?;
                    let back = *underlay.get(
                        (y - underlay_bounds.y) as usize * underlay_stride as usize
                            + (x - underlay_bounds.x) as usize,
                    )?;
                    let back = shadow.shade(back, x, y);
                    let target = image.get_mut(y as usize * screen.width as usize + x as usize)?;
                    *target = blend_shape_xrgb(front, back, coverage);
                }
            }
        }
        Some(())
    }
}
