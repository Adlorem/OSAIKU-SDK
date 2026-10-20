//! Application-side rasterization and atomic pixel/layout publication.
use crate::{abi::*, surface::*};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use osaiku_ui::{layout::Layout, paint::Scene, Document, Error};

struct Scratch {
    document: Document,
    layout: Layout,
    metadata: Publication,
    band: [u8; MAX_TRANSFER],
    dirty: DirtyTracker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterError {
    View(Error),
    Publish(PublishError),
    Checkpoint(isize),
}

/// Declare as a static: the scratch does not fit the native 16 KiB stack.
/// Guarded access spans decoding, layout and publication, including yields.
/// References never escape; recursive/concurrent use refuses without waiting.
pub struct HostedRaster {
    busy: AtomicBool,
    scratch: UnsafeCell<Scratch>,
}
// All access is serialized by busy; no scratch references escape publish_with.
unsafe impl Sync for HostedRaster {}

impl HostedRaster {
    pub const fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
            scratch: UnsafeCell::new(Scratch {
                document: Document::EMPTY,
                layout: Layout::EMPTY,
                metadata: Publication::EMPTY,
                band: [0; MAX_TRANSFER],
                dirty: DirtyTracker::new(),
            }),
        }
    }

    /// Decode the frozen recipe and seal pixels/semantics for a trusted offer.
    /// The caller authenticates its host and obtains the offer from its session.
    /// Track transfer-sized pixel bands against the last successful seal. Yield after
    /// BEGIN, each 256 KiB scanned/uploaded/copied, and every Busy.
    /// Native timer preemption additionally bounds CPU time between syscalls.
    /// At most eight Busy retries across the entire publication; other errors
    /// stop immediately. A known candidate gets one cancellation attempt.
    /// Cleanup/Receipt errors require caller recovery, never blind resubmission.
    /// SEAL alone does not complete the host transaction or authorize input.
    pub fn publish_with(
        &self,
        offer: FrameOffer,
        wire: &[u8],
        exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
        checkpoint: impl FnMut() -> Result<(), isize>,
    ) -> Result<Handle, RasterError> {
        self.publish_with_images(offer, wire, &[], exchange, checkpoint)
    }

    /// [`Self::publish_with`], drawing the application's own images.
    pub fn publish_with_images(
        &self,
        offer: FrameOffer,
        wire: &[u8],
        images: &'static [osaiku_ui::image::ImageSet],
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
        mut checkpoint: impl FnMut() -> Result<(), isize>,
    ) -> Result<Handle, RasterError> {
        offer
            .validate()
            .map_err(|e| RasterError::Publish(PublishError::Content(e)))?;
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(RasterError::Publish(PublishError::Transport(
                SYSCALL_ERR_BUSY,
            )));
        }
        struct Guard<'a>(&'a AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(&self.busy);
        // SAFETY: exclusive guarded access, including during caller checkpoints.
        let scratch = unsafe { &mut *self.scratch.get() };
        Document::decode_into(wire, &mut scratch.document).map_err(RasterError::View)?;
        let extent = offer
            .geometry
            .content_extent()
            .map_err(|e| RasterError::Publish(PublishError::Content(e)))?;
        let scene = Scene::prepare(
            &scratch.document,
            extent.width,
            extent.height,
            &mut scratch.layout,
        )
        .map_err(RasterError::View)?
        .with_theme(
            osaiku_ui::theme::Theme::from_id(offer.geometry.theme)
                .ok_or(RasterError::View(Error::Value))?,
        )
        .with_images(images);
        // Take before modifying fingerprints: every unsuccessful path loses the
        // base. Geometry includes epoch/incarnation, resize, palette and chrome.
        let previous = scratch.dirty.base.take().filter(|b| {
            b.frame.geometry == offer.geometry && b.handle.revision < offer.request.revision
        });
        let mut producer = SurfacePublisher::tracked(
            &scene,
            offer,
            previous.map(|p| p.handle),
            &mut scratch.dirty,
            &mut scratch.metadata,
            &mut scratch.band,
        )
        .map_err(RasterError::Publish)?;
        producer
            .bind_source_hash(osaiku_ui::wire_hash(wire))
            .map_err(RasterError::Publish)?;
        let mut retries = 8;
        let mut uploaded_since_checkpoint = 0;
        let error = loop {
            match producer.step_with(&mut exchange) {
                Ok(PublishStep::Sealed { handle }) => {
                    drop(producer);
                    scratch.dirty.base = Some(FrameSnapshot {
                        handle,
                        frame: offer,
                    });
                    return Ok(handle);
                }
                Ok(PublishStep::Uploaded { length, .. } | PublishStep::Copied { length, .. }) => {
                    uploaded_since_checkpoint += length;
                    if uploaded_since_checkpoint < 256 * 1024 {
                        continue;
                    }
                }
                Ok(PublishStep::Begun { .. }) => {}
                Err(PublishError::Transport(SYSCALL_ERR_BUSY)) if retries != 0 => retries -= 1,
                Err(PublishError::Transport(SYSCALL_ERR_STALE_ENDPOINT))
                    if producer.fallback_full() =>
                {
                    continue
                }
                Err(error) => break RasterError::Publish(error),
            }
            uploaded_since_checkpoint = 0;
            if let Err(code) = checkpoint() {
                break RasterError::Checkpoint(code);
            }
        };
        if producer.handle().is_some() {
            producer
                .cancel_with(exchange)
                .map_err(RasterError::Publish)?;
        }
        Err(error)
    }
}

impl Default for HostedRaster {
    fn default() -> Self {
        Self::new()
    }
}
