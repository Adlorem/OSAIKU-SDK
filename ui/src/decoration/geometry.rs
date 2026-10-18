//! Checked, nonnegative native-pixel rectangles; no display authority.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Bounds {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn empty() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

pub fn valid(bounds: Bounds) -> bool {
    bounds.width != 0
        && bounds.height != 0
        && bounds.x.checked_add(bounds.width).is_some()
        && bounds.y.checked_add(bounds.height).is_some()
}

/// Maximum RAM image handled by a single effect pass (64 MiB of XRGB).
/// Aggregate allocation and window ownership remain the host's responsibility.
pub const MAX_IMAGE_PIXELS: usize = 16 * 1024 * 1024;
pub(crate) fn image_len(screen: Bounds) -> Option<usize> {
    if screen.x != 0 || screen.y != 0 || !valid(screen) {
        return None;
    }
    let len = (screen.width as usize).checked_mul(screen.height as usize)?;
    (len <= MAX_IMAGE_PIXELS).then_some(len)
}

pub fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.x.checked_add(a.width)?.min(b.x.checked_add(b.width)?);
    let bottom = a.y.checked_add(a.height)?.min(b.y.checked_add(b.height)?);
    (right > x && bottom > y).then(|| Bounds::new(x, y, right - x, bottom - y))
}
pub(crate) fn union(a: Bounds, b: Bounds) -> Bounds {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Bounds::new(
        x,
        y,
        (a.x + a.width).max(b.x + b.width) - x,
        (a.y + a.height).max(b.y + b.height) - y,
    )
}
