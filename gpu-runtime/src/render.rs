//! Receiver-side preparation of an immutable drawing batch. No call into Mesa
//! occurs until the whole batch and every asset range have been resolved.
use crate::protocol::{
    buffer::Handle,
    render::{Batch, Draw, Error, Scope, MAX_DRAWS},
};

/// Implemented by the receiver's admitted, read-only buffer mapping owner.
/// The implementation must check the complete handle, caller/session binding
/// and seal before returning bytes, and retain the mapping for the borrow.
/// Unsealed writable or DMA-writable storage must never be returned here.
pub trait Assets {
    fn sealed(&self, handle: Handle) -> Result<&[u8], Error>;
}

pub struct Prepared<'a> {
    batch: Batch<'a>,
    assets: [Option<&'a [u8]>; MAX_DRAWS],
}
impl<'a> Prepared<'a> {
    pub fn resolve(
        bytes: &'a [u8],
        scope: Scope,
        viewport: (u32, u32),
        assets: &'a impl Assets,
    ) -> Result<Self, Error> {
        let batch = Batch::decode(bytes, scope)?;
        if batch.viewport() != viewport {
            return Err(Error::Geometry);
        }
        let mut mapped = [None; MAX_DRAWS];
        for (i, slot) in mapped[..batch.len()].iter_mut().enumerate() {
            let draw = batch.draw(i).ok_or(Error::Length)??;
            if draw.length == 0 {
                continue;
            }
            let buffer = assets.sealed(draw.asset)?;
            let begin = usize::try_from(draw.offset).map_err(|_| Error::Asset)?;
            let end = usize::try_from(draw.offset.checked_add(draw.length).ok_or(Error::Asset)?)
                .map_err(|_| Error::Asset)?;
            *slot = Some(buffer.get(begin..end).ok_or(Error::Asset)?);
        }
        Ok(Self {
            batch,
            assets: mapped,
        })
    }
    pub fn scope(&self) -> Scope {
        self.batch.scope()
    }
    pub fn viewport(&self) -> (u32, u32) {
        self.batch.viewport()
    }
    /// Renderer-private views, valid only while this prepared batch and its
    /// mapping owner are borrowed. There is no output/readback pixel buffer.
    pub fn draws(&self) -> impl Iterator<Item = (Draw, Option<&'a [u8]>)> + '_ {
        (0..self.batch.len()).map(|i| {
            // resolve validated all records over the same immutable borrow.
            (self.batch.draw(i).unwrap().unwrap(), self.assets[i])
        })
    }
}
