//! Pointer-free surface publication metadata. These records describe untrusted
//! client output; only the kernel's authenticated caller/registry grants access.
//! No layout, rasterization, device operation or allocation takes place here.
mod work;
pub use work::{FullFrameWork, WorkCounters, WorkSnapshot};

/// Kernel-issued identity of one armed render transaction, not an input grant.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderRequest {
    pub request_id: u64,
    pub request_hash: u64,
    pub arm_generation: u64,
    pub revision: u64,
}
impl RenderRequest {
    pub fn validate(self) -> Result<(), Refusal> {
        if self.request_id == 0
            || self.request_hash == 0
            || self.arm_generation == 0
            || self.revision == 0
        {
            return Err(Refusal::Identity);
        }
        Ok(())
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameOffer {
    pub geometry: GeometryOffer,
    pub request: RenderRequest,
}
impl FrameOffer {
    pub fn validate(self) -> Result<(), Refusal> {
        self.geometry.validate()?;
        self.request.validate()
    }
}

/// Description of one sealed, request-bound image. Geometry is the stamp saved
/// with that image, not a claim about current window placement or presentation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSnapshot {
    pub handle: Handle,
    pub frame: FrameOffer,
}
impl FrameSnapshot {
    pub fn validate(self) -> Result<(), Refusal> {
        self.frame.validate()?;
        if self.handle.target != self.frame.geometry.target
            || self.handle.serial == 0
            || self.handle.revision != self.frame.request.revision
        {
            return Err(Refusal::Identity);
        }
        Ok(())
    }
}

/// Current admitted content slot in window-local coordinates. Content pixels and
/// semantic bounds start at (0, 0); only the compositor applies the slot offset.
/// Not an allocation, a presentation receipt, or permission to draw chrome.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeometryOffer {
    pub version: u32,
    /// Host-selected palette for this frame; validated again at BEGIN/SEAL.
    pub theme: u32,
    pub target: Target,
    pub resize_generation: u64,
    pub window_width: u32,
    pub window_height: u32,
    pub content: Rect,
    pub frame_layout_hash: u64,
}
impl GeometryOffer {
    pub fn validate(self) -> Result<(), Refusal> {
        if self.version != VERSION
            || crate::theme::Theme::from_id(self.theme).is_none()
            || !self.target.valid()
            || self.resize_generation == 0
            || self.window_width == 0
            || self.window_height == 0
            || self.frame_layout_hash == 0
        {
            return Err(Refusal::Identity);
        }
        if !self.content.within(Extent {
            width: self.window_width,
            height: self.window_height,
            stride: 0,
            format: BGRA8_PREMULTIPLIED,
        }) {
            return Err(Refusal::Bounds);
        }
        self.content_extent()?;
        Ok(())
    }
    pub fn content_extent(self) -> Result<Extent, Refusal> {
        let extent = Extent {
            width: self.content.width,
            height: self.content.height,
            stride: self.content.width.checked_mul(4).ok_or(Refusal::Bounds)?,
            format: BGRA8_PREMULTIPLIED,
        };
        extent.bytes()?;
        Ok(extent)
    }
    pub fn admits(self, extent: Extent) -> Result<(), Refusal> {
        self.validate()?;
        extent.bytes()?;
        if extent != self.content_extent()? {
            return Err(Refusal::Bounds);
        }
        Ok(())
    }
}

pub const VERSION: u32 = 6;
mod delta;
pub use delta::{DeltaOffer, DirtyRows};
pub const MAX_EXTENT: u32 = 4096;
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TRANSFER: usize = 64 * 1024;
/// Metadata is copied atomically; pixel uploads are limited to MAX_TRANSFER.
pub const MAX_METADATA: usize = 32 * 1024;
pub const MAX_OWNER_WINDOWS: usize = 4;
pub const MAX_DAMAGE: usize = 16;
pub const MAX_MATERIALS: usize = 16;
pub const MAX_ELEMENTS: usize = 128;
pub const MAX_LABELS: usize = 8192;
pub const MAX_LABEL: usize = 256;
pub const BGRA8_PREMULTIPLIED: u32 = 1;
pub const HIDDEN: u32 = 1;
pub const DISABLED: u32 = 2;
pub const FOCUSED: u32 = 4;
pub const SELECTED: u32 = 8;
pub const STATE_MASK: u32 = HIDDEN | DISABLED | FOCUSED | SELECTED;
pub const MATERIAL_OPAQUE: u32 = 1;
pub const MATERIAL_TRANSLUCENT: u32 = 2;
pub const MATERIAL_GLASS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Shape,
    Bounds,
    Identity,
    Stale,
    Permission,
    Busy,
    Quota,
    Incomplete,
    Exhausted,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub owner: u64,
    pub window: u64,
    pub incarnation: u64,
    pub epoch: u64,
}
impl Target {
    pub fn valid(self) -> bool {
        // Endpoint format: generation in the high 32 bits, PID in the low 32.
        self.owner as u32 != 0
            && self.owner >> 32 != 0
            && self.window != 0
            && self.incarnation != 0
            && self.epoch != 0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handle {
    pub target: Target,
    pub serial: u64,
    pub revision: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
}

impl Extent {
    pub fn bytes(self) -> Result<u64, Refusal> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_EXTENT
            || self.height > MAX_EXTENT
            || self.format != BGRA8_PREMULTIPLIED
            || self.stride != self.width.checked_mul(4).ok_or(Refusal::Bounds)?
        {
            return Err(Refusal::Bounds);
        }
        let bytes = u64::from(self.stride) * u64::from(self.height);
        if bytes > MAX_BYTES {
            return Err(Refusal::Bounds);
        }
        Ok(bytes)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn within(self, extent: Extent) -> bool {
        self.width != 0
            && self.height != 0
            && self
                .x
                .checked_add(self.width)
                .is_some_and(|r| r <= extent.width)
            && self
                .y
                .checked_add(self.height)
                .is_some_and(|b| b <= extent.height)
    }
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && y >= self.y
            && x.checked_sub(self.x).is_some_and(|v| v < self.width)
            && y.checked_sub(self.y).is_some_and(|v| v < self.height)
    }
    pub fn overlaps(self, other: Self) -> bool {
        u64::from(self.x) < u64::from(other.x) + u64::from(other.width)
            && u64::from(other.x) < u64::from(self.x) + u64::from(self.width)
            && u64::from(self.y) < u64::from(other.y) + u64::from(other.height)
            && u64::from(other.y) < u64::from(self.y) + u64::from(self.height)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub id: u32,
    pub action: u32,
    pub role: u32,
    pub state: u32,
    pub bounds: Rect,
    pub label_offset: u32,
    pub label_len: u32,
}

/// Integer-only client layout, sealed with its pixels and semantic projection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub content_height: u32,
    pub scroll_y: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Material {
    pub bounds: Rect,
    pub kind: u32,
    /// Premultiplied BGRA tint. Glass sampling is private to the compositor.
    pub tint: u32,
}

/// Metadata is uploaded with the pixels and becomes visible in the same commit.
/// Tail entries/bytes must be zero, preventing hidden data in unused fields.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publication {
    pub version: u32,
    pub reserved: u32,
    pub handle: Handle,
    /// Hash of the frozen client recipe accompanying this image; not authority.
    pub source_hash: u64,
    pub extent: Extent,
    pub damage_count: u32,
    pub material_count: u32,
    pub element_count: u32,
    pub label_len: u32,
    pub damage: [Rect; MAX_DAMAGE],
    pub materials: [Material; MAX_MATERIALS],
    pub elements: [Element; MAX_ELEMENTS],
    pub placements: [Placement; MAX_ELEMENTS],
    pub labels: [u8; MAX_LABELS],
}
impl Default for Publication {
    fn default() -> Self {
        Self::EMPTY
    }
}
impl Publication {
    /// Reconstruct only the bounded client-provided layout. No CSS layout or
    /// rasterization runs in the observer/input host. The frozen recipe still
    /// supplies ancestry, chart data and service-validated action identifiers.
    pub fn layout(&self, view: &crate::Document) -> Result<crate::layout::Layout, Refusal> {
        self.validate()?;
        view.validate().map_err(|_| Refusal::Shape)?;
        if self.element_count != u32::from(view.len) || self.source_hash == 0 {
            return Err(Refusal::Identity);
        }
        let mut layout = crate::layout::Layout::EMPTY;
        layout.len = view.len as usize;
        layout.viewport = crate::layout::Rect {
            x: 0,
            y: 0,
            width: self.extent.width,
            height: self.extent.height,
        };
        for i in 0..layout.len {
            let e = self.elements[i];
            let p = self.placements[i];
            let n = &view.nodes[i];
            if e.id != n.id || e.action != n.action || e.role != n.role as u32 {
                return Err(Refusal::Identity);
            }
            let label =
                &self.labels[e.label_offset as usize..(e.label_offset + e.label_len) as usize];
            if label != view.text_of(i).as_bytes() {
                return Err(Refusal::Identity);
            }
            let style = view.resolved_style(i, self.extent.width);
            let [top, _, bottom, _] = view.insets(style);
            let inner_height = p.height.saturating_sub(top + bottom);
            let maximum = if style.overflow == 2 {
                p.content_height.saturating_sub(inner_height)
            } else {
                0
            };
            if p.scroll_y > maximum {
                return Err(Refusal::Bounds);
            }
            let bounds = crate::layout::Rect {
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
            };
            let clip = crate::layout::Rect {
                x: e.bounds.x as i32,
                y: e.bounds.y as i32,
                width: e.bounds.width,
                height: e.bounds.height,
            };
            if clip.width != 0 && clip.height != 0 && bounds.intersect(clip) != clip {
                return Err(Refusal::Bounds);
            }
            layout.nodes[i] = crate::layout::Placed {
                bounds,
                clip,
                content_height: p.content_height,
                scroll_y: p.scroll_y,
                hidden: e.state & HIDDEN != 0,
                disabled: e.state & DISABLED != 0,
            };
        }
        Ok(layout)
    }
    /// Private static workspace initializer; not a valid publication by itself.
    pub const EMPTY: Self = Self {
        version: VERSION,
        reserved: 0,
        handle: Handle {
            target: Target {
                owner: 0,
                window: 0,
                incarnation: 0,
                epoch: 0,
            },
            serial: 0,
            revision: 0,
        },
        source_hash: 0,
        extent: Extent {
            width: 0,
            height: 0,
            stride: 0,
            format: 0,
        },
        damage_count: 0,
        material_count: 0,
        element_count: 0,
        label_len: 0,
        damage: [Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }; MAX_DAMAGE],
        materials: [Material {
            bounds: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            kind: 0,
            tint: 0,
        }; MAX_MATERIALS],
        elements: [Element {
            id: 0,
            action: 0,
            role: 0,
            state: 0,
            bounds: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            label_offset: 0,
            label_len: 0,
        }; MAX_ELEMENTS],
        placements: [Placement {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            content_height: 0,
            scroll_y: 0,
        }; MAX_ELEMENTS],
        labels: [0; MAX_LABELS],
    };
    /// Build the current transport's flat accessibility/action projection from
    /// the same immutable scene that supplies pixels. This experimental record
    /// carries placements and scroll extents; chart series and ancestry remain
    /// in the frozen recipe authenticated by source_hash.
    pub fn prepare<L: AsRef<crate::layout::Layout>>(
        &mut self,
        scene: &crate::paint::Scene<'_, L>,
        handle: Handle,
        materials: &[Material],
    ) -> Result<(), Refusal> {
        let viewport = scene.layout().viewport;
        self.version = VERSION;
        self.reserved = 0;
        self.handle = handle;
        self.source_hash = 0;
        self.extent = Extent {
            width: viewport.width,
            height: viewport.height,
            stride: viewport.width * 4,
            format: BGRA8_PREMULTIPLIED,
        };
        self.damage_count = 1;
        self.material_count = 0;
        self.element_count = 0;
        self.label_len = 0;
        self.damage.fill(Rect::default());
        self.materials.fill(Material::default());
        self.elements.fill(Element::default());
        self.placements.fill(Placement::default());
        self.labels.fill(0);
        self.damage[0] = Rect {
            x: 0,
            y: 0,
            width: viewport.width,
            height: viewport.height,
        };
        if materials.len() > MAX_MATERIALS {
            return Err(Refusal::Bounds);
        }
        self.materials[..materials.len()].copy_from_slice(materials);
        self.material_count = materials.len() as u32;
        for semantic in scene.semantics() {
            let i = self.element_count as usize;
            let start = self.label_len as usize;
            let end = start
                .checked_add(semantic.text.len())
                .ok_or(Refusal::Bounds)?;
            if i == MAX_ELEMENTS || end > MAX_LABELS {
                return Err(Refusal::Bounds);
            }
            let clip = semantic.visible_bounds;
            let invisible =
                semantic.state & crate::state::HIDDEN != 0 || clip.width == 0 || clip.height == 0;
            let disabled = semantic.state & crate::state::DISABLED != 0;
            let focused = !invisible && !disabled && semantic.state & crate::state::FOCUSED != 0;
            self.elements[i] = Element {
                id: semantic.id,
                action: semantic.action,
                role: semantic.role as u32,
                state: if invisible { HIDDEN } else { 0 }
                    | if disabled { DISABLED } else { 0 }
                    | if focused { FOCUSED } else { 0 }
                    | if semantic.state & crate::state::SELECTED != 0 {
                        SELECTED
                    } else {
                        0
                    },
                bounds: if invisible {
                    Rect::default()
                } else {
                    Rect {
                        x: u32::try_from(clip.x).map_err(|_| Refusal::Bounds)?,
                        y: u32::try_from(clip.y).map_err(|_| Refusal::Bounds)?,
                        width: clip.width,
                        height: clip.height,
                    }
                },
                label_offset: start as u32,
                label_len: semantic.text.len() as u32,
            };
            let placed = scene.layout().nodes[i];
            self.placements[i] = Placement {
                x: placed.bounds.x,
                y: placed.bounds.y,
                width: placed.bounds.width,
                height: placed.bounds.height,
                content_height: placed.content_height,
                scroll_y: placed.scroll_y,
            };
            self.labels[start..end].copy_from_slice(semantic.text.as_bytes());
            self.label_len = end as u32;
            self.element_count += 1;
        }
        self.validate()
    }

    pub fn validate(&self) -> Result<(), Refusal> {
        let zero_rect = Rect::default();
        self.extent.bytes()?;
        if self.version != VERSION
            || self.reserved != 0
            || !self.handle.target.valid()
            || self.handle.serial == 0
            || self.handle.revision == 0
            || self.damage_count as usize > MAX_DAMAGE
            || self.material_count as usize > MAX_MATERIALS
            || self.element_count as usize > MAX_ELEMENTS
            || self.label_len as usize > MAX_LABELS
        {
            return Err(Refusal::Shape);
        }
        let damage = self.damage_count as usize;
        let materials = self.material_count as usize;
        let elements = self.element_count as usize;
        let labels = self.label_len as usize;
        if self.damage[..damage].iter().any(|r| !r.within(self.extent))
            || self.damage[damage..].iter().any(|r| *r != zero_rect)
            || self.materials[materials..]
                .iter()
                .any(|r| *r != Material::default())
            || self.elements[elements..]
                .iter()
                .any(|r| *r != Element::default())
            || self.placements[elements..]
                .iter()
                .any(|p| *p != Placement::default())
            || self.labels[labels..].iter().any(|b| *b != 0)
        {
            return Err(Refusal::Shape);
        }
        let labels = core::str::from_utf8(&self.labels[..labels]).map_err(|_| Refusal::Shape)?;
        let mut focused = false;
        for (i, e) in self.elements[..elements].iter().enumerate() {
            let p = self.placements[i];
            // Coordinates are untrusted. Keep all later SDK hit/scroll arithmetic
            // far from signed overflow, including off-viewport descendants.
            const LIMIT: u32 = 16 * 1024 * 1024;
            if p.x.unsigned_abs() > LIMIT
                || p.y.unsigned_abs() > LIMIT
                || p.width > LIMIT
                || p.height > LIMIT
                || p.content_height > LIMIT
                || p.scroll_y > LIMIT
            {
                return Err(Refusal::Bounds);
            }
            let start = e.label_offset as usize;
            let end = start
                .checked_add(e.label_len as usize)
                .ok_or(Refusal::Bounds)?;
            if e.id == 0
                || e.role > crate::Role::TextBox as u32
                || e.state & !STATE_MASK != 0
                || e.label_len as usize > MAX_LABEL
                || labels.get(start..end).is_none()
                || self.elements[..i].iter().any(|old| old.id == e.id)
                || (e.bounds != zero_rect && !e.bounds.within(self.extent))
                || (e.state & HIDDEN == 0 && !e.bounds.within(self.extent))
            {
                return Err(Refusal::Shape);
            }
            if e.state & FOCUSED != 0 {
                if focused || e.state & (HIDDEN | DISABLED) != 0 || e.action == 0 {
                    return Err(Refusal::Shape);
                }
                focused = true;
            }
        }
        for (i, m) in self.materials[..materials].iter().enumerate() {
            if !m.bounds.within(self.extent)
                || !matches!(
                    m.kind,
                    MATERIAL_OPAQUE | MATERIAL_TRANSLUCENT | MATERIAL_GLASS
                )
                || !premultiplied(m.tint)
                || (m.kind == MATERIAL_OPAQUE && m.tint >> 24 != 255)
                || self.materials[..i]
                    .iter()
                    .any(|old| old.bounds.overlaps(m.bounds))
            {
                return Err(Refusal::Shape);
            }
        }
        Ok(())
    }

    /// Candidate metadata must be validated before use. This performs only a
    /// bounded rectangle lookup; it cannot recreate a client layout in kernel.
    pub fn hit(&self, x: u32, y: u32) -> Option<&Element> {
        self.elements
            .get(..self.element_count as usize)?
            .iter()
            .rev()
            .find(|e| {
                e.action != 0 && e.state & (HIDDEN | DISABLED) == 0 && e.bounds.contains(x, y)
            })
    }
}

pub fn premultiplied(pixel: u32) -> bool {
    let alpha = pixel >> 24;
    pixel & 255 <= alpha && (pixel >> 8) & 255 <= alpha && (pixel >> 16) & 255 <= alpha
}
