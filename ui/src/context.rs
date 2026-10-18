//! Versioned, read-only context from one committed window publication.
//! Client-content coordinates exclude chrome, other windows and host overlays.
use crate::surface::{Extent, Handle, Publication, Rect, Refusal, HIDDEN};
pub const VERSION: u32 = 1;
pub const STRUCTURE: u32 = 0;
pub const CONTENT: u32 = 1;
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;
pub const MAX_TRANSFER: usize = 64 * 1024;
pub const CLIPPED: u32 = 1;
pub const REDACTED: u32 = 2;
pub const CLIENT_CONTENT: u32 = 1;
pub const HOST_OVERLAYS_EXCLUDED: u32 = 2;
pub const OWNER_AUTHORIZED: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub version: u32,
    pub disclosure: u32,
    pub token: u64,
    pub recipient: u64,
    pub handle: Handle,
    pub frame_generation: u64,
    pub source_hash: u64,
    pub app_id: u64,
    /// Crop in the original immutable client image.
    pub crop: Rect,
    pub extent: Extent,
    pub flags: u32,
    pub reserved: u32,
}
impl Info {
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.version != VERSION
            || self.disclosure > CONTENT
            || self.token == 0
            || self.recipient as u32 == 0
            // Fixed service endpoints may start at generation zero. The kernel
            // checks the exact live generation; dynamic unbound IDs are refused
            // by endpoint resolution. Wire shape alone never grants authority.
            || !self.handle.target.valid()
            || self.handle.revision == 0
            || self.handle.serial == 0
            || self.frame_generation == 0
            || self.source_hash == 0
            || self.app_id == 0
            || self.reserved != 0
            || self.flags != CLIENT_CONTENT | HOST_OVERLAYS_EXCLUDED | OWNER_AUTHORIZED
        {
            return Err(Refusal::Identity);
        }
        if self.extent.bytes()? > MAX_PIXELS
            || self.extent.width != self.crop.width
            || self.extent.height != self.crop.height
            || !self.crop.within(Extent {
                width: crate::surface::MAX_EXTENT,
                height: crate::surface::MAX_EXTENT,
                ..self.extent
            })
        {
            return Err(Refusal::Bounds);
        }
        Ok(())
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub id: u32,
    pub parent: u32,
    pub kind: u32,
    pub role: u32,
    pub state: u32,
    pub action: u32,
    pub bounds: Rect,
    pub flags: u32,
    pub value: u32,
    pub label_offset: u32,
    pub label_len: u32,
    pub point_offset: u32,
    pub point_len: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub timestamp: u64,
    pub value: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub info: Info,
    pub count: u32,
    pub label_len: u32,
    pub point_len: u32,
    pub reserved: u32,
    pub elements: [Element; crate::MAX_NODES],
    pub labels: [u8; crate::MAX_TEXT],
    pub points: [Point; crate::MAX_POINTS],
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            info: Info::default(),
            count: 0,
            label_len: 0,
            point_len: 0,
            reserved: 0,
            elements: [Element::default(); crate::MAX_NODES],
            labels: [0; crate::MAX_TEXT],
            points: [Point::default(); crate::MAX_POINTS],
        }
    }
}
fn intersection(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect {
        x,
        y,
        width: (u64::from(a.x) + u64::from(a.width))
            .min(u64::from(b.x) + u64::from(b.width))
            .saturating_sub(u64::from(x)) as u32,
        height: (u64::from(a.y) + u64::from(a.height))
            .min(u64::from(b.y) + u64::from(b.height))
            .saturating_sub(u64::from(y)) as u32,
    }
}
impl Snapshot {
    /// Metadata and pixels must be obtained from the SAME retained publication.
    /// Never emits offscreen labels, clipped text, unrevealed values or hidden nodes.
    pub fn project(
        &mut self,
        info: Info,
        source: &Publication,
        view: &crate::Document,
    ) -> Result<(), Refusal> {
        info.validate()?;
        source.layout(view)?;
        if info.handle != source.handle
            || info.source_hash != source.source_hash
            || !info.crop.within(source.extent)
        {
            return Err(Refusal::Identity);
        }
        self.info = info;
        self.count = 0;
        self.label_len = 0;
        self.point_len = 0;
        self.reserved = 0;
        self.elements.fill(Element::default());
        self.labels.fill(0);
        self.points.fill(Point::default());
        for i in 0..source.element_count as usize {
            let e = source.elements[i];
            let n = view.nodes[i];
            let p = source.placements[i];
            let clip = intersection(e.bounds, info.crop);
            if e.state & HIDDEN != 0 || clip.width == 0 || clip.height == 0 {
                continue;
            }
            let full = p.x >= 0
                && p.y >= 0
                && clip
                    == Rect {
                        x: p.x as u32,
                        y: p.y as u32,
                        width: p.width,
                        height: p.height,
                    };
            let covered = source.elements[i + 1..source.element_count as usize]
                .iter()
                .any(|later| later.state & HIDDEN == 0 && later.bounds.overlaps(clip));
            let disclose = info.disclosure == CONTENT && full && !covered;
            let mut out = Element {
                id: e.id,
                parent: 0,
                kind: n.kind as u32,
                role: e.role,
                state: e.state,
                action: e.action,
                bounds: Rect {
                    x: clip.x - info.crop.x,
                    y: clip.y - info.crop.y,
                    width: clip.width,
                    height: clip.height,
                },
                flags: if full { 0 } else { CLIPPED } | if disclose { 0 } else { REDACTED },
                value: 10001,
                label_offset: self.label_len,
                point_offset: 0,
                ..Element::default()
            };
            if disclose {
                out.value = u32::from(n.value);
                let text = &source.labels
                    [e.label_offset as usize..(e.label_offset + e.label_len) as usize];
                let start = self.label_len as usize;
                self.labels
                    .get_mut(start..start + text.len())
                    .ok_or(Refusal::Bounds)?
                    .copy_from_slice(text);
                out.label_len = e.label_len;
                self.label_len += e.label_len;
                out.point_offset = u32::from(n.series_start);
                out.point_len = u32::from(n.series_len);
                for (j, p) in view.points
                    [n.series_start as usize..(n.series_start + n.series_len) as usize]
                    .iter()
                    .enumerate()
                {
                    *self
                        .points
                        .get_mut(n.series_start as usize + j)
                        .ok_or(Refusal::Bounds)? = Point {
                        timestamp: p.time_ms,
                        value: u32::from(p.value),
                        reserved: 0,
                    };
                }
                if out.point_len != 0 {
                    self.point_len = self.point_len.max(out.point_offset + out.point_len);
                } else {
                    out.point_offset = 0;
                }
            }
            self.elements[self.count as usize] = out;
            self.count += 1;
        }
        // Only refer to parents actually present in the disclosed crop.
        for i in 0..self.count as usize {
            let node = view.nodes[..view.len as usize]
                .iter()
                .find(|n| n.id == self.elements[i].id)
                .ok_or(Refusal::Identity)?;
            if node.parent != crate::NONE {
                let parent = view.nodes[node.parent as usize].id;
                if self.elements[..self.count as usize]
                    .iter()
                    .any(|e| e.id == parent)
                {
                    self.elements[i].parent = parent;
                }
            }
        }
        self.validate()
    }
    pub fn validate(&self) -> Result<(), Refusal> {
        self.info.validate()?;
        if self.count as usize > crate::MAX_NODES
            || self.label_len as usize > crate::MAX_TEXT
            || self.point_len as usize > crate::MAX_POINTS
            || self.reserved != 0
        {
            return Err(Refusal::Bounds);
        }
        let mut labels = 0u32;
        for (i, e) in self.elements[..self.count as usize].iter().enumerate() {
            let label_end = e
                .label_offset
                .checked_add(e.label_len)
                .ok_or(Refusal::Bounds)?;
            let point_end = e
                .point_offset
                .checked_add(e.point_len)
                .ok_or(Refusal::Bounds)?;
            if e.id == 0
                || e.label_offset != labels
                || e.kind > crate::Kind::Template as u32
                || e.role > crate::Role::TextBox as u32
                || e.state & !crate::surface::STATE_MASK != 0
                || e.state & HIDDEN != 0
                || e.label_len > crate::MAX_LEAF_TEXT as u32
                || e.point_len > 60
                || self.elements[..i].iter().any(|p| p.id == e.id)
                || !e.bounds.within(self.info.extent)
                || label_end > self.label_len
                || point_end > self.point_len
                || e.value > 10001
                || e.flags & !(CLIPPED | REDACTED) != 0
                || (e.parent != 0 && !self.elements[..i].iter().any(|p| p.id == e.parent))
                || ((e.flags & REDACTED != 0 || self.info.disclosure == STRUCTURE)
                    && (e.label_len != 0 || e.point_len != 0 || e.value != 10001))
                || core::str::from_utf8(&self.labels[e.label_offset as usize..label_end as usize])
                    .is_err()
            {
                return Err(Refusal::Shape);
            }
            labels = label_end;
        }
        if labels != self.label_len {
            return Err(Refusal::Shape);
        }
        if self.elements[self.count as usize..]
            .iter()
            .any(|e| *e != Element::default())
            || self.labels[self.label_len as usize..]
                .iter()
                .any(|b| *b != 0)
            || self.points[self.point_len as usize..]
                .iter()
                .any(|p| *p != Point::default())
            || self.points[..self.point_len as usize]
                .iter()
                .any(|p| p.value > 10001 || p.reserved != 0)
        {
            return Err(Refusal::Shape);
        }
        for (i, p) in self.points[..self.point_len as usize].iter().enumerate() {
            if !self.elements[..self.count as usize].iter().any(|e| {
                i >= e.point_offset as usize && i < (e.point_offset + e.point_len) as usize
            }) && *p != Point::default()
            {
                return Err(Refusal::Shape);
            }
        }
        Ok(())
    }
}
/// Copy a packed crop band; structure-only disclosure returns opaque black.
pub fn pixels(
    info: &Info,
    source_extent: Extent,
    source: &[u8],
    offset: u64,
    out: &mut [u8],
) -> Result<(), Refusal> {
    info.validate()?;
    if out.is_empty()
        || out.len() > MAX_TRANSFER
        || offset % 4 != 0
        || out.len() % 4 != 0
        || offset
            .checked_add(out.len() as u64)
            .is_none_or(|end| end > info.extent.bytes().unwrap_or(0))
        || !info.crop.within(source_extent)
        || source.len() as u64 != source_extent.bytes()?
    {
        return Err(Refusal::Bounds);
    }
    if info.disclosure == STRUCTURE {
        for pixel in out.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[0, 0, 0, 255]);
        }
        return Ok(());
    }
    let stride = info.extent.stride as usize;
    let mut written = 0;
    while written < out.len() {
        let index = offset as usize + written;
        let row = index / stride;
        let col = index % stride;
        let count = (stride - col).min(out.len() - written);
        let start = (info.crop.y as usize + row) * source_extent.stride as usize
            + info.crop.x as usize * 4
            + col;
        out[written..written + count].copy_from_slice(&source[start..start + count]);
        written += count;
    }
    Ok(())
}
