//! Bounded CPU backdrop filtering. Reads only the private, cursor-free underlay.
use super::geometry::{
    image_len, intersection, valid as ui_window_compositor_bounds_valid, Bounds,
};
use super::pixels::blend_declared_xrgb;

pub const RADIUS: u32 = 8;
pub const REPAIR_CAPACITY: usize = 12;
const DIAMETER: usize = (RADIUS * 2 + 1) as usize;
const STRIPE: usize = 64;
const OPACITY: u8 = 200;

/// Optional blur needs its own capture margin even when the native frame's
/// shadow/glow are disabled. Opaque hosts need only WindowStyle::footprint.
pub fn footprint(window: Bounds, screen: Bounds) -> Option<Bounds> {
    let shadow = super::shadow::footprint(window, screen)?;
    Some(super::geometry::union(
        shadow,
        expand(window, RADIUS, screen)?,
    ))
}

pub fn material(tint: u32, backdrop: u32) -> u32 {
    blend_declared_xrgb(tint, backdrop, OPACITY)
}

/// A repaired region contains complete footprints of every window it touches.
/// This deliberately conservative closure keeps blur taps away from stale
/// upper layers and preserves a coherent retained image outside the repair.
pub fn repair_region(
    mut dirty: Bounds,
    screen: Bounds,
    windows: impl Iterator<Item = Bounds> + Clone,
) -> Option<Bounds> {
    image_len(screen)?;
    if intersection(dirty, screen) != Some(dirty) {
        return None;
    }
    for _ in 0..=windows.clone().count() {
        let before = dirty;
        for window in windows.clone() {
            let footprint = footprint(window, screen)?;
            if intersection(dirty, footprint).is_some() {
                dirty = super::geometry::union(dirty, footprint);
            }
        }
        if dirty == before {
            return Some(dirty);
        }
    }
    None
}

pub fn uniform_color(
    pixels: &[u32],
    bounds: Bounds,
    stride: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<u32> {
    uniform_region(
        pixels,
        Bounds::new(0, 0, bounds.width, bounds.height),
        stride,
        checkpoint,
    )
}

fn uniform_region(
    pixels: &[u32],
    local: Bounds,
    stride: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<u32> {
    if !ui_window_compositor_bounds_valid(local) || stride < local.x.checked_add(local.width)? {
        return None;
    }
    let first = *pixels.get(local.y as usize * stride as usize + local.x as usize)?;
    for y in local.y..local.y.checked_add(local.height)? {
        let start = (y as usize)
            .checked_mul(stride as usize)?
            .checked_add(local.x as usize)?;
        for portion in pixels
            .get(start..start.checked_add(local.width as usize)?)?
            .chunks(1024)
        {
            checkpoint();
            if portion.iter().any(|p| *p != first) {
                return None;
            }
        }
    }
    Some(first)
}

/// Composite premultiplied foreground over the filtered and tinted backdrop.
/// Opaque text and image pixels retain their original RGB values.
fn over_glass(cached: u32, glass: u32) -> u32 {
    let transparency = 255 - (cached >> 24) as u8;
    (cached & 0x00ff_ffff) + blend_declared_xrgb(glass, 0, transparency)
}

/// One separable 17x17 box filter, evaluated with sliding column sums in 64px
/// stripes. Scratch is under 1KiB; no allocation, variable-radius loops per
/// output pixel, framebuffer reads or modifications to the saved backdrop.
/// Returns the uniform backdrop colour when translation-invariant skip is safe.
pub fn compose(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    title_height: u32,
    surface: &[u32],
    underlay: &[u32],
    saved: Bounds,
    stride: u32,
    checkpoint: &mut impl FnMut(),
) -> Option<Option<u32>> {
    compose_region(
        image,
        screen,
        window,
        title_height,
        surface,
        underlay,
        saved,
        stride,
        window,
        true,
        checkpoint,
    )
}

/// Only write the intersection with clip. The caller reconstructs a radius-wide
/// backdrop halo first. Do not scan the whole saved underlay for uniformity.
pub fn compose_clipped(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    title_height: u32,
    surface: &[u32],
    underlay: &[u32],
    saved: Bounds,
    stride: u32,
    clip: Bounds,
    checkpoint: &mut impl FnMut(),
) -> Option<()> {
    compose_region(
        image,
        screen,
        window,
        title_height,
        surface,
        underlay,
        saved,
        stride,
        clip,
        false,
        checkpoint,
    )
    .map(|_| ())
}

fn compose_region(
    image: &mut [u32],
    screen: Bounds,
    window: Bounds,
    title_height: u32,
    surface: &[u32],
    underlay: &[u32],
    saved: Bounds,
    stride: u32,
    clip: Bounds,
    check_uniform: bool,
    checkpoint: &mut impl FnMut(),
) -> Option<Option<u32>> {
    if !ui_window_compositor_bounds_valid(clip)
        || screen.x != 0
        || screen.y != 0
        || !ui_window_compositor_bounds_valid(screen)
        || intersection(window, screen) != Some(window)
        || saved != footprint(window, screen)?
        || stride < saved.width
        || title_height > window.height
        || image.len() != image_len(screen)?
        || surface.len() != (window.width as usize).checked_mul(window.height as usize)?
        || underlay.len()
            < (saved.height as usize - 1)
                .checked_mul(stride as usize)?
                .checked_add(saved.width as usize)?
    {
        return None;
    }
    let Some(patch) = intersection(window, clip) else {
        return Some(None);
    };
    let x0 = patch.x - window.x;
    let y0 = patch.y - window.y;
    let tint = |y| {
        if y < title_height {
            super::theme::TITLE
        } else {
            super::theme::SURFACE
        }
    };
    let uniform = if check_uniform {
        uniform_color(underlay, saved, stride, checkpoint)
    } else {
        // Partial repairs capture only this patch and its blur halo. Testing
        // precisely those taps avoids filtering a flat desktop without reading
        // stale scratch outside the captured region.
        let taps = intersection(expand(patch, RADIUS, screen)?, saved)?;
        uniform_region(
            underlay,
            Bounds::new(taps.x - saved.x, taps.y - saved.y, taps.width, taps.height),
            stride,
            checkpoint,
        )
    };
    if let Some(backdrop) = uniform {
        for y in y0..y0 + patch.height {
            let base = tint(y);
            let glass = material(base, backdrop);
            for first in (x0..x0 + patch.width).step_by(1024) {
                checkpoint();
                for x in first..(first + 1024).min(x0 + patch.width) {
                    image[((window.y + y) * screen.width + window.x + x) as usize] =
                        over_glass(surface[(y * window.width + x) as usize], glass);
                }
            }
        }
        return Some(uniform);
    }
    let sample = |x: i64, y: i64| {
        let x = x.clamp(i64::from(saved.x), i64::from(saved.x + saved.width - 1)) as u32;
        let y = y.clamp(i64::from(saved.y), i64::from(saved.y + saved.height - 1)) as u32;
        underlay[((y - saved.y) * stride + x - saved.x) as usize]
    };
    let mut columns = [[0u32; 3]; STRIPE + DIAMETER - 1];
    for stripe in (x0 as usize..(x0 + patch.width) as usize).step_by(STRIPE) {
        let count = STRIPE.min((x0 + patch.width) as usize - stripe);
        let left = i64::from(window.x) + stripe as i64 - i64::from(RADIUS);
        columns.fill([0; 3]);
        for (x, column) in columns[..count + DIAMETER - 1].iter_mut().enumerate() {
            checkpoint();
            for dy in -(RADIUS as i64)..=i64::from(RADIUS) {
                let pixel = sample(left + x as i64, i64::from(patch.y) + dy);
                for channel in 0..3 {
                    column[channel] += (pixel >> (channel * 8)) & 255;
                }
            }
        }
        for y in y0..y0 + patch.height {
            checkpoint();
            let mut sum = [0; 3];
            for column in &columns[..DIAMETER] {
                for channel in 0..3 {
                    sum[channel] += column[channel];
                }
            }
            let base = tint(y);
            for x in 0..count {
                let mut blurred = 0;
                for channel in 0..3 {
                    let divisor = (DIAMETER * DIAMETER) as u32;
                    blurred |= ((sum[channel] + divisor / 2) / divisor) << (channel * 8);
                }
                let local_x = stripe + x;
                image[(window.y + y) as usize * screen.width as usize
                    + window.x as usize
                    + local_x] = over_glass(
                    surface[y as usize * window.width as usize + local_x],
                    material(base, blurred),
                );
                if x + 1 < count {
                    for channel in 0..3 {
                        sum[channel] =
                            sum[channel] - columns[x][channel] + columns[x + DIAMETER][channel];
                    }
                }
            }
            if y + 1 < y0 + patch.height {
                for (x, column) in columns[..count + DIAMETER - 1].iter_mut().enumerate() {
                    let above =
                        sample(left + x as i64, i64::from(window.y + y) - i64::from(RADIUS));
                    let below = sample(
                        left + x as i64,
                        i64::from(window.y + y) + i64::from(RADIUS) + 1,
                    );
                    for channel in 0..3 {
                        column[channel] = column[channel] - ((above >> (channel * 8)) & 255)
                            + ((below >> (channel * 8)) & 255);
                    }
                }
            }
        }
    }
    Some(None)
}

/// A small repair borrows the unused tail of the workspace underlay to preserve
/// the old scene outside its output. No allocation and no screen-sized copy.
/// If a large window leaves no room, use the conservative full-footprint path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartialRepair {
    pub output: Bounds,
    pub work: Bounds,
    pub backup_offset: usize,
    layers: usize,
}
pub fn expand(rect: Bounds, radius: u32, screen: Bounds) -> Option<Bounds> {
    image_len(screen)?;
    if !ui_window_compositor_bounds_valid(rect) {
        return None;
    }
    let x = rect.x.saturating_sub(radius);
    let y = rect.y.saturating_sub(radius);
    let right = rect
        .x
        .checked_add(rect.width)?
        .saturating_add(radius)
        .min(screen.width);
    let bottom = rect
        .y
        .checked_add(rect.height)?
        .saturating_add(radius)
        .min(screen.height);
    (right > x && bottom > y).then_some(Bounds::new(x, y, right - x, bottom - y))
}
impl PartialRepair {
    pub fn plan(
        dirty: Bounds,
        screen: Bounds,
        windows: impl Iterator<Item = Bounds> + Clone,
        stride: u32,
        underlay_len: usize,
    ) -> Option<Self> {
        let repair = Self::layout(dirty, screen, windows, stride)?;
        let pixels = repair.pixels()?;
        // Avoid a costly partial pass for repairs that already cover most of the screen.
        if pixels > (screen.width as usize).checked_mul(screen.height as usize)? / 2
            || repair.backup_offset.checked_add(pixels)? > underlay_len
        {
            return None;
        }
        Some(repair)
    }

    /// Output, work area and capture extent, without the size limits. The
    /// backup offset is the capture extent: the first underlay word the
    /// per-layer captures never write.
    fn layout(
        dirty: Bounds,
        screen: Bounds,
        windows: impl Iterator<Item = Bounds> + Clone,
        stride: u32,
    ) -> Option<Self> {
        image_len(screen)?;
        if intersection(dirty, screen) != Some(dirty) {
            return None;
        }
        let layers = windows.clone().count();
        let mut output = dirty;
        let mut backup_offset = 0;
        for window in windows {
            // A changed lower pixel can affect upper glass within one radius.
            if let Some(affected) = intersection(expand(output, RADIUS, screen)?, window) {
                output = super::geometry::union(output, affected);
            }
            let saved = footprint(window, screen)?;
            if stride < saved.width {
                return None;
            }
            let used = (saved.height as usize - 1)
                .checked_mul(stride as usize)?
                .checked_add(saved.width as usize)?;
            backup_offset = backup_offset.max(used);
        }
        let work = expand(
            output,
            RADIUS.checked_mul(u32::try_from(layers).ok()?)?,
            screen,
        )?;
        Some(Self {
            output,
            work,
            backup_offset,
            layers,
        })
    }

    fn pixels(&self) -> Option<usize> {
        (self.work.width as usize).checked_mul(self.work.height as usize)
    }
    pub fn layer_clip(&self, index: usize, screen: Bounds) -> Option<Bounds> {
        expand(
            self.output,
            RADIUS.checked_mul(
                u32::try_from(self.layers.checked_sub(index.checked_add(1)?)?).ok()?,
            )?,
            screen,
        )
    }
    pub fn save(
        &self,
        image: &[u32],
        screen: Bounds,
        backup: &mut [u32],
        checkpoint: &mut impl FnMut(),
    ) -> Option<()> {
        super::surface::capture(
            image,
            screen,
            self.work,
            backup,
            self.work.width,
            checkpoint,
        )
    }
    pub fn restore_halo(
        &self,
        image: &mut [u32],
        screen: Bounds,
        backup: &[u32],
        checkpoint: &mut impl FnMut(),
    ) -> Option<()> {
        if image.len() != image_len(screen)?
            || intersection(self.work, screen) != Some(self.work)
            || intersection(self.output, self.work) != Some(self.output)
            || backup.len() < (self.work.width as usize).checked_mul(self.work.height as usize)?
        {
            return None;
        }
        for y in self.work.y..self.work.y + self.work.height {
            for first in (self.work.x..self.work.x + self.work.width).step_by(1024) {
                checkpoint();
                for x in first..(first + 1024).min(self.work.x + self.work.width) {
                    if x >= self.output.x
                        && x < self.output.x + self.output.width
                        && y >= self.output.y
                        && y < self.output.y + self.output.height
                    {
                        continue;
                    }
                    image[(y * screen.width + x) as usize] =
                        backup[((y - self.work.y) * self.work.width + x - self.work.x) as usize];
                }
            }
        }
        Some(())
    }
}

/// Separate partial repairs for damage regions that stay apart after the blur
/// closure. Regions whose work areas (output plus one radius per layer) meet
/// are merged and replanned until every work area is disjoint, so no repair
/// reads or writes another's pixels, and each saves its own halo in the
/// underlay tail after the shared capture extent. The size limits of a single
/// repair apply to the sum. Distant edits no longer repair their bounding box.
pub struct RepairSet {
    repairs: [PartialRepair; REPAIR_CAPACITY],
    count: usize,
}
impl RepairSet {
    pub fn plan(
        dirty: &[Bounds],
        screen: Bounds,
        windows: impl Iterator<Item = Bounds> + Clone,
        stride: u32,
        underlay_len: usize,
    ) -> Option<Self> {
        let mut regions = [Bounds::empty(); REPAIR_CAPACITY];
        let count = dirty.len();
        if count == 0 || count > regions.len() {
            return None;
        }
        regions[..count].copy_from_slice(dirty);
        let mut set = Self {
            repairs: [PartialRepair {
                output: Bounds::empty(),
                work: Bounds::empty(),
                backup_offset: 0,
                layers: 0,
            }; REPAIR_CAPACITY],
            count,
        };
        loop {
            for (repair, region) in set.repairs.iter_mut().zip(&regions[..set.count]) {
                *repair = PartialRepair::layout(*region, screen, windows.clone(), stride)?;
            }
            let repairs = &set.repairs[..set.count];
            let meeting = (0..set.count).find_map(|a| {
                (a + 1..set.count)
                    .find(|&b| intersection(repairs[a].work, repairs[b].work).is_some())
                    .map(|b| (a, b))
            });
            let Some((a, b)) = meeting else { break };
            regions[a] = super::geometry::union(regions[a], regions[b]);
            set.count -= 1;
            regions[b] = regions[set.count];
        }
        // The capture extent depends only on the windows: equal for every repair.
        let mut offset = set.repairs[0].backup_offset;
        let mut pixels = 0usize;
        for repair in &mut set.repairs[..set.count] {
            let work = repair.pixels()?;
            repair.backup_offset = offset;
            offset = offset.checked_add(work)?;
            pixels = pixels.checked_add(work)?;
        }
        if pixels > (screen.width as usize).checked_mul(screen.height as usize)? / 2
            || offset > underlay_len
        {
            return None;
        }
        Some(set)
    }

    pub fn repairs(&self) -> &[PartialRepair] {
        &self.repairs[..self.count]
    }

    /// Underlay words the per-layer captures may use; backups follow.
    pub fn capture_len(&self) -> usize {
        self.repairs[0].backup_offset
    }

    pub fn output_bounds(&self) -> Bounds {
        self.repairs()[1..]
            .iter()
            .fold(self.repairs[0].output, |bounds, repair| {
                super::geometry::union(bounds, repair.output)
            })
    }
}
