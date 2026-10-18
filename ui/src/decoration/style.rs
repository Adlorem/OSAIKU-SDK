//! Validated native decoration style. Host policy owns the choice of style;
//! linking this API does not grant an application authority over other windows.
use super::geometry::{valid, Bounds};

pub const MAX_RADIUS: u32 = 32;
pub const MAX_EXTENT: u32 = 32;

/// A symmetric, quadratic falloff outside the window contour, in physical px.
/// Zero extent or opacity disables this layer. Colors are 24-bit RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Falloff {
    pub extent: u32,
    pub color: u32,
    pub opacity: u8,
}
impl Falloff {
    pub const NONE: Self = Self {
        extent: 0,
        color: 0,
        opacity: 0,
    };
    const fn valid(self) -> bool {
        self.extent <= MAX_EXTENT && self.color <= 0xffffff
    }
    const fn extent(self) -> u32 {
        if self.opacity == 0 {
            0
        } else {
            self.extent
        }
    }
}

/// Shared by footprint/damage calculation and the prepared CPU renderer.
/// The one-pixel outline remains separate from the outer shadow and glow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowStyle {
    pub corner_radius: u32,
    pub shadow: Falloff,
    pub glow: Falloff,
    pub border_color: u32,
    pub focus_color: u32,
}
impl WindowStyle {
    pub const NATIVE: Self = Self {
        corner_radius: 12,
        shadow: Falloff {
            extent: 20,
            color: 0,
            opacity: 96,
        },
        glow: Falloff {
            extent: 3,
            color: super::theme::GLOW,
            opacity: 24,
        },
        border_color: super::theme::BORDER,
        focus_color: super::theme::FOCUS,
    };
    pub const fn valid(self) -> bool {
        self.corner_radius <= MAX_RADIUS
            && self.shadow.valid()
            && self.glow.valid()
            && self.border_color <= 0xffffff
            && self.focus_color <= 0xffffff
    }
    /// Outward damage/capture margin. A disabled shadow allocates no halo.
    #[inline]
    pub const fn extent(self) -> u32 {
        let a = self.shadow.extent();
        let b = self.glow.extent();
        if a > b {
            a
        } else {
            b
        }
    }
    pub fn footprint(self, window: Bounds, screen: Bounds) -> Option<Bounds> {
        if !self.valid()
            || screen.x != 0
            || screen.y != 0
            || !valid(window)
            || !valid(screen)
            || window.x.checked_add(window.width)? > screen.width
            || window.y.checked_add(window.height)? > screen.height
        {
            return None;
        }
        let left = window.x.saturating_sub(self.extent());
        let top = window.y.saturating_sub(self.extent());
        let right = (window.x + window.width)
            .saturating_add(self.extent())
            .min(screen.width);
        let bottom = (window.y + window.height)
            .saturating_add(self.extent())
            .min(screen.height);
        Some(Bounds::new(left, top, right - left, bottom - top))
    }
    /// None means square (zero radius or window too small), or invalid geometry.
    pub fn corner_boxes(self, window: Bounds) -> Option<[Bounds; 4]> {
        let r = self.corner_radius;
        if !self.valid()
            || !valid(window)
            || r == 0
            || window.width < 2 * r
            || window.height < 2 * r
        {
            return None;
        }
        let right = window.x.checked_add(window.width)?.checked_sub(r)?;
        let bottom = window.y.checked_add(window.height)?.checked_sub(r)?;
        Some([
            Bounds::new(window.x, window.y, r, r),
            Bounds::new(right, window.y, r, r),
            Bounds::new(window.x, bottom, r, r),
            Bounds::new(right, bottom, r, r),
        ])
    }
}

/// Immutable lookup tables shared by all windows using this style (~60 KiB).
/// Prefer a `static` const initializer. For dynamic themes, prepare once in
/// host-owned storage outside a render/move transaction, then reuse by reference.
/// Do not place this value on a small service stack or prepare it per frame.
pub struct PreparedStyle {
    pub(crate) style: WindowStyle,
    pub(crate) corners: super::corners::Masks,
    pub(crate) shadow: super::shadow::Tables,
}
impl PreparedStyle {
    /// Panics for an invalid style. Validate untrusted settings before preparing.
    /// Const initialization fails at build time for out-of-range presets.
    pub const fn new(style: WindowStyle) -> Self {
        assert!(style.valid(), "invalid window decoration style");
        Self {
            style,
            corners: super::corners::Masks::new(style.corner_radius),
            shadow: super::shadow::Tables::new(style),
        }
    }
    pub const fn style(&self) -> WindowStyle {
        self.style
    }
}

/// The built-in frame's style and renderer use one source of truth.
pub static NATIVE: PreparedStyle = PreparedStyle::new(super::frame::STYLE);
