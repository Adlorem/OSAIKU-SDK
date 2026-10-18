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
pub(crate) fn runs<P: crate::paint::Painter + ?Sized>(
    painter: &mut P,
    at: crate::layout::Rect,
    image: &'static Image,
    clip: crate::layout::Rect,
) {
    let area = at.intersect(clip);
    for y in area.y..area.bottom() {
        let row = (y - at.y) as u32;
        let mut x = area.x;
        while x < area.right() {
            let pixel = image.pixel((x - at.x) as u32, row);
            let mut end = x + 1;
            while end < area.right() && image.pixel((end - at.x) as u32, row) == pixel {
                end += 1;
            }
            let alpha = (pixel >> 24) as u8;
            if alpha != 0 {
                painter.fill_alpha(
                    crate::layout::Rect {
                        x,
                        y,
                        width: (end - x) as u32,
                        height: 1,
                    },
                    pixel & 0x00ff_ffff,
                    alpha,
                    clip,
                );
            }
            x = end;
        }
    }
}
