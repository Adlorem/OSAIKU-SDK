//! Shared graphics buffer records. Values alone never confer authority.
pub const MAX_BUFFER_BYTES: u64 = 256 * 1024 * 1024;
pub mod access {
    pub const CPU_READ: u32 = 1;
    pub const CPU_WRITE: u32 = 2;
    pub const GPU_READ: u32 = 4;
    pub const GPU_WRITE: u32 = 8;
    pub const SCANOUT: u32 = 16;
    pub const ALL: u32 = 31;
}
pub mod kind {
    pub const BUFFER: u32 = 1;
    pub const CONTEXT: u32 = 2;
    pub const JOB: u32 = 3;
    pub const SYNC: u32 = 4;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Version,
    Shape,
    Bounds,
    Permission,
    Stale,
    Busy,
    Exhausted,
    Order,
    Timeout,
    Cancelled,
    Dependency,
    Quarantined,
}

/// B,G,R,A bytes, premultiplied alpha, UNORM; the X variant is opaque B,G,R,X.
/// These are DRM fourcc values, not a promise of any backend's scanout support.
pub const ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
pub const XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
pub const LINEAR: u64 = 0;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BufferDesc {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub access: u32,
    pub stride: u64,
    pub bytes: u64,
    pub modifier: u64,
}
impl BufferDesc {
    pub fn validate(self) -> Result<(), Error> {
        if !matches!(self.format, ARGB8888 | XRGB8888)
            || self.modifier != LINEAR
            || self.access == 0
            || self.access & !access::ALL != 0
        {
            return Err(Error::Shape);
        }
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || self.stride % 4 != 0
            || self.stride < u64::from(self.width) * 4
            || self.stride.checked_mul(self.height.into()) != Some(self.bytes)
            || self.bytes > MAX_BUFFER_BYTES
        {
            return Err(Error::Bounds);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handle {
    /// Encoded process endpoint, including its nonzero generation.
    pub owner: u64,
    pub device: u64,
    pub generation: u64,
    /// Never reused, including when a slot is recycled.
    pub serial: u64,
    pub kind: u32,
    pub reserved: u32,
}
