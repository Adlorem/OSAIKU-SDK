//! Reusable native window chrome and CPU effects. This library has no devices,
//! global state, allocation or OS dependencies. Pixels stay in caller-owned RAM.
//! Hosts retain ownership/admission and publish a complete coherent scene.
//! Cooperative callbacks are host scheduling boundaries, never serialized data.
pub mod corners;
pub mod frame;
pub mod geometry;
pub mod glass;
pub mod pixels;
pub mod shadow;
pub mod style;
pub use style::{Falloff, PreparedStyle, WindowStyle};
mod surface;

/// Built-in native-pixel material. Keeping the original bounded kernels and
/// lookup tables preserves appearance and the retained move/repair fast paths.
pub mod theme {
    use crate::appearance::DARK;
    pub const SURFACE: u32 = DARK.surface;
    pub const TITLE: u32 = DARK.title;
    pub const BORDER: u32 = DARK.border;
    pub const FOCUS: u32 = DARK.focus;
    pub const GLOW: u32 = DARK.glow;
}
