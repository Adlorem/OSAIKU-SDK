//! Bounded software resource policy, independent of PCI ID and panel model.
//! These are admission ceilings, not preallocated physical backing.
pub const GEM_PAGES: usize = 69_632; // 272 MiB (includes the third scanout) across all GEM owners
pub const CLIENT_PAGES: usize = 8192; // ordinary clients retain their quota
pub const VM_TABLES: usize = 256;
pub const DMA_PAGES: usize = GEM_PAGES + 4096; // eight VMs, scratch and engine state
pub const ALLOCATION_SLOTS: usize = DMA_PAGES.div_ceil(32);
pub const DMA_MAPS: usize = DMA_PAGES * 2; // independent aliases retain separate pins
pub const CACHE_BYTES: u64 = 64 * 1024 * 1024;
pub const MESA_BYTES: u64 = 32 * 1024 * 1024;
// Each Mesa process has 256 mmap descriptors. The broker serves eight VMs;
// ordinary clients must leave one complete descriptor budget for compositord.
pub const CPU_MAPS_PER_OWNER: usize = 256;
pub const CPU_MAPS: usize = 8 * CPU_MAPS_PER_OWNER;

/// Four retained RGBA scenes and three scanouts, plus bounded foreground and
/// decoration caches and Mesa BOs. Align textures conservatively for tiling.
/// Unsupported working sets fail before starting Mesa, rather than consuming
/// all RAM or silently clipping their quota. Actual allocation still charges
/// each page and preserves the allocator's protected recovery reserve.
pub fn compositor_pages(width: u64, height: u64, stride: u64) -> Option<usize> {
    if width == 0 || height == 0 || width > 4096 || height > 4096
        || stride < width * 4 || stride % 4 != 0 || stride > 65536 {
        return None;
    }
    let texture = (width * 4).div_ceil(512) * 512 * height.div_ceil(32) * 32;
    let scanout = (stride * height).div_ceil(4096) * 4096;
    let bytes = texture * 4 + scanout * 3 + CACHE_BYTES * 2 + MESA_BYTES;
    let pages = usize::try_from(bytes.div_ceil(4096)).ok()?;
    (pages <= GEM_PAGES).then_some(pages)
}
