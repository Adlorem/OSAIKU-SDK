//! Pre-rasterized images for `ui-icon src`. A build tool turns each source
//! (SVG or PNG) into straight ARGB pixels at a fixed set of sizes, so no image
//! decoder or vector rasterizer runs in an application or in the host.

/// One size of an image: straight ARGB rows, alpha in the top byte.
#[derive(Clone, Copy, Debug)]
pub struct Image {
    pub width: u16,
    pub height: u16,
    pub pixels: &'static [u32],
}

impl Image {
    /// The pixel at (x, y) inside the image; transparent outside it.
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        if x >= u32::from(self.width) || y >= u32::from(self.height) {
            return 0;
        }
        self.pixels
            .get(y as usize * usize::from(self.width) + x as usize)
            .copied()
            .unwrap_or(0)
    }
}

/// One named image at every size its build produced, ascending.
#[derive(Clone, Copy, Debug)]
pub struct ImageSet {
    pub name: &'static str,
    pub images: &'static [Image],
}

impl ImageSet {
    /// The largest size that fits inside `width` x `height`; `None` when even
    /// the smallest does not. Images are never scaled at runtime.
    pub fn fit(&self, width: u32, height: u32) -> Option<&'static Image> {
        self.images
            .iter()
            .rev()
            .find(|image| u32::from(image.width) <= width && u32::from(image.height) <= height)
    }
}

/// The default `Painter::image`: one `fill_alpha` per row run of equal pixels
/// inside `at` and `clip`; transparent pixels are skipped.
/// Also usable by GPU painters for private mutable images that must bypass
/// persistent texture caches. The painter retains only bounded drawing work.
pub fn runs<P: crate::paint::Painter + ?Sized>(
    painter: &mut P,
    at: crate::layout::Rect,
    image: &'static Image,
    clip: crate::layout::Rect,
) {
    fill_runs(painter, at, image, None, clip);
}

/// Prefix of an image set drawn as a mask: only its alpha counts, painted in
/// the element's `color`, so one build follows every theme and accent.
pub const TINT_PREFIX: &str = "tint-";

/// `runs` for a mask (a set named `tint-...`): each pixel's alpha, scaled by
/// `alpha`, in `color` (RGB).
pub fn tinted_runs<P: crate::paint::Painter + ?Sized>(
    painter: &mut P,
    at: crate::layout::Rect,
    image: &'static Image,
    color: u32,
    alpha: u8,
    clip: crate::layout::Rect,
) {
    fill_runs(painter, at, image, Some((color & 0x00ff_ffff, alpha)), clip);
}

fn fill_runs<P: crate::paint::Painter + ?Sized>(
    painter: &mut P,
    at: crate::layout::Rect,
    image: &'static Image,
    tint: Option<(u32, u8)>,
    clip: crate::layout::Rect,
) {
    // A mask compares alpha alone: colour differences inside it do not split runs.
    let key = |pixel: u32| if tint.is_some() { pixel >> 24 } else { pixel };
    let area = at.intersect(clip);
    for y in area.y..area.bottom() {
        let row = (y - at.y) as u32;
        let mut x = area.x;
        while x < area.right() {
            let pixel = image.pixel((x - at.x) as u32, row);
            let mut end = x + 1;
            while end < area.right() && key(image.pixel((end - at.x) as u32, row)) == key(pixel) {
                end += 1;
            }
            let (color, alpha) = match tint {
                Some((color, scale)) => (color, ((pixel >> 24) * u32::from(scale) / 255) as u8),
                None => (pixel & 0x00ff_ffff, (pixel >> 24) as u8),
            };
            if alpha != 0 {
                painter.fill_alpha(
                    crate::layout::Rect {
                        x,
                        y,
                        width: (end - x) as u32,
                        height: 1,
                    },
                    color,
                    alpha,
                    clip,
                );
            }
            x = end;
        }
    }
}
