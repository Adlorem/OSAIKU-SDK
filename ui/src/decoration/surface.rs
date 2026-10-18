use super::geometry::{valid as ui_window_compositor_bounds_valid, Bounds};
fn image_len(width: u32, height: u32) -> Option<usize> {
    super::geometry::image_len(Bounds::new(0, 0, width, height))
}
fn image_extent(screen: Bounds, pixels: &[u32]) -> Option<()> {
    (screen.x == 0
        && screen.y == 0
        && ui_window_compositor_bounds_valid(screen)
        && pixels.len() == image_len(screen.width, screen.height)?)
    .then_some(())
}

fn compact_extent(rect: Bounds, stride: u32, pixels: &[u32]) -> Option<()> {
    if !ui_window_compositor_bounds_valid(rect) || stride < rect.width {
        return None;
    }
    let required = (rect.height as usize)
        .checked_sub(1)?
        .checked_mul(stride as usize)?
        .checked_add(rect.width as usize)?;
    (required <= pixels.len()).then_some(())
}

fn rect_in_screen(rect: Bounds, screen: Bounds) -> Option<()> {
    (ui_window_compositor_bounds_valid(rect)
        && rect.x.checked_add(rect.width)? <= screen.width
        && rect.y.checked_add(rect.height)? <= screen.height)
        .then_some(())
}

/// Save a screen rectangle into compact private storage.
pub(crate) fn capture(
    image: &[u32],
    screen: Bounds,
    rect: Bounds,
    compact: &mut [u32],
    compact_stride: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<()> {
    image_extent(screen, image)?;
    rect_in_screen(rect, screen)?;
    compact_extent(rect, compact_stride, compact)?;
    for row in 0..rect.height as usize {
        let source = (rect.y as usize + row) * screen.width as usize + rect.x as usize;
        let target = row * compact_stride as usize;
        for column in (0..rect.width as usize).step_by(1024) {
            let count = 1024.min(rect.width as usize - column);
            compact[target + column..target + column + count]
                .copy_from_slice(&image[source + column..source + column + count]);
            checkpoint();
        }
    }
    Some(())
}
