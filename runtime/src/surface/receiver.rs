//! Private compositor cache ingestion. Storage belongs to the server; only a
//! fully read and released snapshot is exposed as a received frame.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveStep {
    Prepared,
    Copied(ReadChunk),
    Complete,
}

/// A complete private copy, not a presentation receipt or input authorization.
/// The compositor must validate its scene/epoch before displaying this content.
pub struct ReceivedFrame<'a> {
    pub snapshot: FrameSnapshot,
    pub metadata: &'a Publication,
    pub pixels: &'a [u8],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Preparing,
    Reading,
    Copied,
    Complete,
    Cancelled,
}

/// Borrows exact-size server-private pixel storage and metadata for one frame.
/// No allocation or automatic retry occurs. Preparation takes at most three
/// syscalls, subsequent steps one bounded READ or RELEASE. Schedule input and
/// other windows between steps. Keep storage private after errors/cancellation;
/// dropping this value neither releases a lease nor certifies partial storage.
#[must_use = "complete or explicitly cancel reception to release the server lease"]
pub struct SurfaceReceiver<'a> {
    snapshot: FrameSnapshot,
    reader: SurfaceReader,
    metadata: &'a mut Publication,
    pixels: &'a mut [u8],
    offset: usize,
    phase: Phase,
}

impl<'a> SurfaceReceiver<'a> {
    pub fn new(
        snapshot: FrameSnapshot,
        metadata: &'a mut Publication,
        pixels: &'a mut [u8],
    ) -> Result<Self, ReadError> {
        let reader = SurfaceReader::from_snapshot(snapshot)?;
        let extent = snapshot
            .frame
            .geometry
            .content_extent()
            .map_err(ReadError::Content)?;
        if pixels.len() as u64 != extent.bytes().map_err(ReadError::Content)? {
            return Err(ReadError::Content(Refusal::Bounds));
        }
        Ok(Self {
            snapshot,
            reader,
            metadata,
            pixels,
            offset: 0,
            phase: Phase::Preparing,
        })
    }

    /// Refusal keeps the exact cursor/phase for retry or explicit cancellation.
    /// A failed RELEASE does not recopy the image or expose an incomplete result.
    pub fn step_with(
        &mut self,
        exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<ReceiveStep, ReadError> {
        match self.phase {
            Phase::Preparing => {
                self.reader.start_with(self.metadata, exchange)?;
                self.phase = Phase::Reading;
                Ok(ReceiveStep::Prepared)
            }
            Phase::Reading => {
                let end = self.offset + (self.pixels.len() - self.offset).min(MAX_TRANSFER);
                let chunk = self
                    .reader
                    .next_with(&mut self.pixels[self.offset..end], exchange)?
                    .ok_or(ReadError::Content(Refusal::Incomplete))?;
                self.offset += chunk.length;
                if self.offset == self.pixels.len() {
                    self.phase = Phase::Copied;
                }
                Ok(ReceiveStep::Copied(chunk))
            }
            Phase::Copied => {
                self.reader.finish_with(exchange)?;
                self.phase = Phase::Complete;
                Ok(ReceiveStep::Complete)
            }
            Phase::Complete => Ok(ReceiveStep::Complete),
            Phase::Cancelled => Err(ReadError::Transport(crate::abi::SYSCALL_ERR_INVALID_STATE)),
        }
    }

    pub fn received(&self) -> Option<ReceivedFrame<'_>> {
        (self.phase == Phase::Complete).then_some(ReceivedFrame {
            snapshot: self.snapshot,
            metadata: self.metadata,
            pixels: self.pixels,
        })
    }

    /// Discard this result and release its lease, if any. Failure preserves the
    /// receiver for retry. Does not scrub an entire potentially 64 MiB image in
    /// one call; incomplete storage remains private and is not a received frame.
    pub fn cancel_with(
        &mut self,
        exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), ReadError> {
        self.reader.cancel_with(exchange)?;
        self.phase = Phase::Cancelled;
        Ok(())
    }
}
