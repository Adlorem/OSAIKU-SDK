//! Transport work counters, not latency measurements. Counters saturate and
//! explicitly flag saturation so a comparison cannot mistake wrapping for work.
use super::{Extent, Refusal, Target, MAX_TRANSFER};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkCounters {
    pub dispatched_calls: u64,
    pub refused_calls: u64,
    pub upload_calls: u64,
    pub read_calls: u64,
    pub uploaded_pixel_bytes: u64,
    pub read_pixel_bytes: u64,
    /// All actual payload copies, including user/scratch and scratch/storage.
    /// Excludes fixed request/receipt records and failed pointer validation.
    pub payload_copy_bytes: u64,
    pub publications: u64,
    pub cancellations: u64,
    pub lease_stalls: u64,
    pub saturated: u64,
}
impl WorkCounters {
    pub const EMPTY: Self = Self {
        dispatched_calls: 0,
        refused_calls: 0,
        upload_calls: 0,
        read_calls: 0,
        uploaded_pixel_bytes: 0,
        read_pixel_bytes: 0,
        payload_copy_bytes: 0,
        publications: 0,
        cancellations: 0,
        lease_stalls: 0,
        saturated: 0,
    };
    pub fn add(&mut self, delta: Self) {
        self.saturated |= delta.saturated;
        macro_rules! add { ($($field:ident),*) => { $(
            match self.$field.checked_add(delta.$field) {
                Some(value) => self.$field = value,
                None => { self.$field = u64::MAX; self.saturated = 1; }
            }
        )* }; }
        add!(
            dispatched_calls,
            refused_calls,
            upload_calls,
            read_calls,
            uploaded_pixel_bytes,
            read_pixel_bytes,
            payload_copy_bytes,
            publications,
            cancellations,
            lease_stalls
        );
    }
}

/// Statistics queries do not contribute to dispatched work counters. Global
/// counters include retired windows; per-window counters end with that tenancy.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkSnapshot {
    pub version: u32,
    pub reserved: u32,
    pub epoch: u64,
    /// Zero for a supervisor/server-only aggregate, exact tenancy otherwise.
    pub target: Target,
    pub counters: WorkCounters,
    pub current_pixel_bytes: u64,
    pub peak_pixel_bytes: u64,
    /// Nonzero if accounting could not acquire its guard; reject comparisons.
    pub incomplete: u64,
    /// Global supervisor/server snapshot only; includes rounded metadata/header
    /// storage, separately budgeted from live pixel charges.
    pub spare_bytes: u64,
    pub backing_reuses: u64,
    pub backing_allocations: u64,
}

/// Calculated work for the current full-frame producer and max-sized reader.
/// Row-aligned uploads may need more calls than arbitrary byte-chunk reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullFrameWork {
    pub pixel_bytes: u64,
    pub upload_calls: u64,
    pub read_calls: u64,
    pub pixel_copy_bytes: u64,
}
impl FullFrameWork {
    pub fn calculate(extent: Extent) -> Result<Self, Refusal> {
        let pixel_bytes = extent.bytes()?;
        let rows = MAX_TRANSFER as u64 / u64::from(extent.stride);
        if rows == 0 {
            return Err(Refusal::Bounds);
        }
        Ok(Self {
            pixel_bytes,
            upload_calls: u64::from(extent.height).div_ceil(rows),
            read_calls: pixel_bytes.div_ceil(MAX_TRANSFER as u64),
            pixel_copy_bytes: pixel_bytes * 4,
        })
    }
}
