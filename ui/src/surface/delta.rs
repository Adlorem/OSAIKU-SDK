//! Version 5 incremental frames use full-width dirty rows. Every output byte
//! must be either uploaded or copied from the exact immutable base handle.
use super::*;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRows(pub [u64; MAX_EXTENT as usize / 64]);
impl DirtyRows {
    pub const EMPTY: Self = Self([0; MAX_EXTENT as usize / 64]);
    pub fn contains(&self, row: u32) -> bool {
        row < MAX_EXTENT && self.0[row as usize / 64] & (1 << (row % 64)) != 0
    }
    pub fn insert(&mut self, start: u32, end: u32) -> Result<(), Refusal> {
        if start > end || end > MAX_EXTENT {
            return Err(Refusal::Bounds);
        }
        for row in start..end {
            self.0[row as usize / 64] |= 1 << (row % 64);
        }
        Ok(())
    }
    pub fn validate(&self, height: u32) -> Result<(), Refusal> {
        if height == 0 || height > MAX_EXTENT || (height..MAX_EXTENT).any(|y| self.contains(y)) {
            return Err(Refusal::Bounds);
        }
        Ok(())
    }
}
impl Default for DirtyRows {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeltaOffer {
    pub frame: FrameOffer,
    pub base: Handle,
    pub dirty: DirtyRows,
}
impl DeltaOffer {
    pub fn validate(&self) -> Result<(), Refusal> {
        self.frame.validate()?;
        if self.base.target != self.frame.geometry.target
            || self.base.serial == 0
            || self.base.revision == 0
            || self.base.revision >= self.frame.request.revision
        {
            return Err(Refusal::Identity);
        }
        self.dirty.validate(self.frame.geometry.content.height)
    }
}
const _: () = assert!(core::mem::size_of::<DeltaOffer>() == 672);
