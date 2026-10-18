//! Application-side, allocation-free rasterization into bounded row bands.
//! No syscall, framebuffer, backdrop or foreign process memory is accessible.
use crate::{
    decoration::pixels::foreground_over,
    font::ui_text_run,
    layout::{Layout, Rect},
    paint::{Painter, Scene},
    surface::{Refusal, MAX_TRANSFER},
};

/// Paint complete rows, starting at `first_y`, into tightly packed BGRA bytes.
/// A caller reuses one private band until every row has been uploaded. All
/// bands borrow the same scene, so layout and text cannot change mid-frame.
/// Reject a malformed band before touching any output byte.
pub fn rows<L: AsRef<Layout>>(
    scene: &Scene<'_, L>,
    first_y: u32,
    bytes: &mut [u8],
) -> Result<(), Refusal> {
    let viewport = scene.layout().viewport;
    let stride = viewport.width as usize * 4;
    if stride == 0 || bytes.is_empty() || bytes.len() > MAX_TRANSFER || bytes.len() % stride != 0 {
        return Err(Refusal::Bounds);
    }
    let height = (bytes.len() / stride) as u32;
    if first_y
        .checked_add(height)
        .is_none_or(|end| end > viewport.height)
    {
        return Err(Refusal::Bounds);
    }
    bytes.fill(0);
    scene.paint(&mut Band {
        bytes,
        stride,
        viewport: Rect {
            x: 0,
            y: first_y as i32,
            width: viewport.width,
            height,
        },
    });
    Ok(())
}

struct Band<'a> {
    bytes: &'a mut [u8],
    stride: usize,
    viewport: Rect,
}
impl Band<'_> {
    fn dot(&mut self, x: i32, y: i32, color: u32, alpha: u8, clip: Rect) {
        if alpha == 0 || !clip.contains(x, y) || !self.viewport.contains(x, y) {
            return;
        }
        let index = (y - self.viewport.y) as usize * self.stride + x as usize * 4;
        let behind = u32::from_le_bytes(self.bytes[index..index + 4].try_into().unwrap());
        let pixel = if alpha == 255 {
            color | 0xff00_0000
        } else {
            foreground_over(color, alpha, behind)
        };
        self.bytes[index..index + 4].copy_from_slice(&pixel.to_le_bytes());
    }
}
impl Painter for Band<'_> {
    fn visible(&self, bounds: Rect) -> bool {
        let r = bounds.intersect(self.viewport);
        r.width != 0 && r.height != 0
    }
    fn fill_alpha(&mut self, rect: Rect, color: u32, alpha: u8, clip: Rect) {
        let rect = rect.intersect(clip).intersect(self.viewport);
        if alpha == 0 || rect.width == 0 || rect.height == 0 {
            return;
        }
        let pixel = (color | 0xff00_0000).to_le_bytes();
        for y in rect.y..rect.bottom() {
            let start = (y - self.viewport.y) as usize * self.stride + rect.x as usize * 4;
            let row = &mut self.bytes[start..start + rect.width as usize * 4];
            for dst in row.chunks_exact_mut(4) {
                if alpha == 255 {
                    dst.copy_from_slice(&pixel);
                } else {
                    let behind = u32::from_le_bytes(dst.try_into().unwrap());
                    dst.copy_from_slice(&foreground_over(color, alpha, behind).to_le_bytes());
                }
            }
        }
    }
    fn text(&mut self, rect: Rect, text: &str, size: u16, color: u32, clip: Rect) {
        let clip = clip.intersect(self.viewport);
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        // Glyphs are pre-rasterized at this exact size; coverage is copied 1:1.
        for (pen, _, glyph) in ui_text_run(text, size) {
            let ink = Rect {
                x: rect.x + pen + glyph.left(),
                y: rect.y + glyph.top() as i32,
                width: glyph.width(),
                height: glyph.height(),
            };
            if rect.x + pen >= clip.right() {
                break;
            }
            let visible = ink.intersect(clip);
            if visible.width == 0 || visible.height == 0 {
                continue;
            }
            for y in visible.y..visible.bottom() {
                let start = (y - self.viewport.y) as usize * self.stride + visible.x as usize * 4;
                let pixels = &mut self.bytes[start..start + visible.width as usize * 4];
                for (x, dst) in (visible.x..).zip(pixels.chunks_exact_mut(4)) {
                    let alpha = glyph.alpha((x - ink.x) as u32, (y - ink.y) as u32);
                    if alpha != 0 {
                        let behind = u32::from_le_bytes(dst.try_into().unwrap());
                        dst.copy_from_slice(&foreground_over(color, alpha, behind).to_le_bytes());
                    }
                }
            }
        }
    }
    fn line(&mut self, from: (i32, i32), to: (i32, i32), color: u32, clip: Rect) {
        let clip = clip.intersect(self.viewport);
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        // Most chart/grid segments do not touch this row band. Reject their
        // bounding box before walking pixels; retain the original endpoints
        // for intersecting segments so Bresenham's coverage is unchanged.
        if from.0.max(to.0) < clip.x
            || from.0.min(to.0) >= clip.right()
            || from.1.max(to.1) < clip.y
            || from.1.min(to.1) >= clip.bottom()
        {
            return;
        }
        let (mut x, mut y) = from;
        let dx = (to.0 - x).abs();
        let dy = -(to.1 - y).abs();
        let sx = if x < to.0 { 1 } else { -1 };
        let sy = if y < to.1 { 1 } else { -1 };
        let mut error = dx + dy;
        // Only bounded SDK scene geometry reaches this private adapter.
        for _ in 0..=dx.max(-dy) {
            self.dot(x, y, color, 255, clip);
            if x == to.0 && y == to.1 {
                break;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_fill_matches_scalar_blend_and_refuses_empty_intersections() {
        let viewport = Rect {
            x: 0,
            y: 4,
            width: 8,
            height: 4,
        };
        for rect in [
            Rect {
                x: -3,
                y: 2,
                width: 9,
                height: 9,
            },
            Rect {
                x: 200,
                y: 4,
                width: 4,
                height: 4,
            },
            Rect {
                x: 2,
                y: 6,
                width: 2,
                height: 2,
            },
        ] {
            for alpha in [0, 37, 255] {
                let mut bytes = [0x40; 128];
                let mut painter = Band {
                    bytes: &mut bytes,
                    stride: 32,
                    viewport,
                };
                painter.fill_alpha(rect, 0x123456, alpha, viewport);
                for y in 4..8 {
                    for x in 0..8 {
                        let expected = if rect.contains(x, y) {
                            foreground_over(0x123456, alpha, 0x40404040)
                        } else {
                            0x40404040
                        };
                        let at = ((y - 4) * 8 + x) as usize * 4;
                        assert_eq!(&bytes[at..at + 4], &expected.to_le_bytes());
                    }
                }
            }
        }
    }

    #[test]
    fn line_band_rejection_preserves_crossing_and_reversed_segment_coverage() {
        let viewport = Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        for (from, to) in [
            ((-8, -10), (20, 30)),
            ((9, 7), (-4, 0)),
            ((0, 2), (7, 2)),
            ((0, 20), (7, 20)),
        ] {
            let mut full = [0; 256];
            Band {
                bytes: &mut full,
                stride: 32,
                viewport,
            }
            .line(from, to, 0xabcdef, viewport);
            for y in [0, 2, 4, 6] {
                let mut band = [0; 64];
                Band {
                    bytes: &mut band,
                    stride: 32,
                    viewport: Rect {
                        y,
                        height: 2,
                        ..viewport
                    },
                }
                .line(from, to, 0xabcdef, viewport);
                assert_eq!(&band, &full[y as usize * 32..(y as usize + 2) * 32]);
            }
        }
    }
}
