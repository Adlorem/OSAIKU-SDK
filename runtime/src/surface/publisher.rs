//! Cooperative producer: at most one transport call and one raster band per step.
use super::*;
use crate::abi::{SYSCALL_ERR_INVALID_STATE, SYSCALL_ERR_STALE_ENDPOINT, SYSCALL_OK};
use osaiku_ui::{layout::Layout, paint::Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishStep {
    Begun { handle: Handle },
    Uploaded { offset: u64, length: usize },
    Copied { offset: u64, length: usize },
    Sealed { handle: Handle },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Uploading,
    Ready,
    Sealed,
    Cancelled,
    Uncertain,
}
#[derive(Clone, Copy)]
enum Offer<'a> {
    None,
    Geometry(GeometryOffer),
    Frame(FrameOffer),
    Delta(&'a DeltaOffer),
    Stream(FrameOffer, Handle),
}

/// Borrows a frozen scene, metadata and scratch until completion/cancellation.
/// The caller chooses when to step, retry a refused step, or cancel. No hidden
/// retries, waits, allocation or automatic cancellation on Drop. After a corrupt
/// receipt, only cleanup is allowed; never guess whether an upload was accepted.
/// SEAL accepts a snapshot, not scanout or input delivery. Retain the previous
/// visible revision until the compositor's separate presentation acknowledgement.
#[must_use = "seal or cancel the candidate; dropping a publisher does not reclaim it"]
pub struct SurfacePublisher<'a, 'doc, L: AsRef<Layout>> {
    scene: &'a Scene<'doc, L>,
    metadata: &'a mut Publication,
    band: &'a mut [u8],
    request: Request,
    offer: Offer<'a>,
    phase: Phase,
    y: u32,
    tracker: Option<&'a mut DirtyTracker>,
    prepared_band: Option<(u32, dirty::Fingerprint, bool)>,
}

impl<'a, 'doc, L: AsRef<Layout>> SurfacePublisher<'a, 'doc, L> {
    /// Bind the recipe used for this prepared scene before any upload.
    pub fn bind_source_hash(&mut self, hash: u64) -> Result<(), PublishError> {
        if self.phase != Phase::Prepared || hash == 0 {
            return Err(PublishError::Content(Refusal::Identity));
        }
        self.metadata.source_hash = hash;
        Ok(())
    }
    /// Prepare bounded semantics before reserving transport resources. The scene
    /// must already have been laid out in the application's own address space.
    pub fn new(
        scene: &'a Scene<'doc, L>,
        target: Target,
        revision: u64,
        materials: &[Material],
        metadata: &'a mut Publication,
        band: &'a mut [u8],
    ) -> Result<Self, PublishError> {
        let viewport = scene.layout().viewport;
        let extent = Extent {
            width: viewport.width,
            height: viewport.height,
            stride: viewport
                .width
                .checked_mul(4)
                .ok_or(PublishError::Content(Refusal::Bounds))?,
            format: BGRA8_PREMULTIPLIED,
        };
        extent.bytes().map_err(PublishError::Content)?;
        if band.len() > MAX_TRANSFER || band.len() < extent.stride as usize {
            return Err(PublishError::Content(Refusal::Bounds));
        }
        metadata
            .prepare(
                scene,
                Handle {
                    target,
                    serial: 1,
                    revision,
                },
                materials,
            )
            .map_err(PublishError::Content)?;
        band.fill(0);
        Ok(Self {
            scene,
            metadata,
            band,
            request: Request {
                version: REQUEST_VERSION,
                operation: BEGIN,
                handle: Handle {
                    target,
                    serial: 0,
                    revision,
                },
                extent,
                ..Request::default()
            },
            phase: Phase::Prepared,
            offer: Offer::None,
            y: 0,
            tracker: None,
            prepared_band: None,
        })
    }

    /// Use geometry fetched before layout. A stale offer is rejected by the
    /// kernel before pixel allocation; query again and reconstruct the scene.
    pub fn from_offer(
        scene: &'a Scene<'doc, L>,
        offer: GeometryOffer,
        revision: u64,
        materials: &[Material],
        metadata: &'a mut Publication,
        band: &'a mut [u8],
    ) -> Result<Self, PublishError> {
        offer.validate().map_err(PublishError::Content)?;
        let mut producer = Self::new(scene, offer.target, revision, materials, metadata, band)?;
        offer
            .admits(producer.request.extent)
            .map_err(PublishError::Content)?;
        producer.request.operation = BEGIN_OFFER;
        producer.offer = Offer::Geometry(offer);
        Ok(producer)
    }

    /// Bind pixels and semantics to the kernel-issued armed request/revision.
    pub fn from_frame(
        scene: &'a Scene<'doc, L>,
        offer: FrameOffer,
        materials: &[Material],
        metadata: &'a mut Publication,
        band: &'a mut [u8],
    ) -> Result<Self, PublishError> {
        offer.validate().map_err(PublishError::Content)?;
        let mut producer = Self::from_offer(
            scene,
            offer.geometry,
            offer.request.revision,
            materials,
            metadata,
            band,
        )?;
        producer.request.operation = BEGIN_FRAME;
        producer.offer = Offer::Frame(offer);
        Ok(producer)
    }

    /// Prepare a row-delta against an exact immutable base. Geometry and the
    /// armed request are checked again by the host before any allocation.
    pub fn from_delta(
        scene: &'a Scene<'doc, L>,
        delta: &'a DeltaOffer,
        materials: &[Material],
        metadata: &'a mut Publication,
        band: &'a mut [u8],
    ) -> Result<Self, PublishError> {
        delta.validate().map_err(PublishError::Content)?;
        let mut producer = Self::from_frame(scene, delta.frame, materials, metadata, band)?;
        producer.request.operation = BEGIN_DELTA;
        producer.offer = Offer::Delta(delta);
        producer.metadata.damage.fill(Rect::default());
        producer.metadata.damage_count = 0;
        let height = producer.request.extent.height;
        let mut y = 0;
        while y < height {
            if !delta.dirty.contains(y) {
                y += 1;
                continue;
            }
            let start = y;
            while y < height && delta.dirty.contains(y) {
                y += 1;
            }
            producer.add_damage(start, y - start);
        }
        Ok(producer)
    }
    /// The native path renders every transfer band once, including Busy retries.
    pub(crate) fn tracked(
        scene: &'a Scene<'doc, L>,
        frame: FrameOffer,
        base: Option<Handle>,
        tracker: &'a mut DirtyTracker,
        metadata: &'a mut Publication,
        band: &'a mut [u8],
    ) -> Result<Self, PublishError> {
        if band.len() != MAX_TRANSFER {
            return Err(PublishError::Content(Refusal::Bounds));
        }
        let mut producer = Self::from_frame(scene, frame, &[], metadata, band)?;
        if let Some(base) = base {
            DeltaOffer {
                frame,
                base,
                dirty: DirtyRows::EMPTY,
            }
            .validate()
            .map_err(PublishError::Content)?;
            producer.offer = Offer::Stream(frame, base);
            producer.request.operation = BEGIN_STREAM;
            producer.metadata.damage.fill(Rect::default());
            producer.metadata.damage_count = 0;
        }
        producer.tracker = Some(tracker);
        Ok(producer)
    }
    fn add_damage(&mut self, y: u32, height: u32) {
        let n = self.metadata.damage_count as usize;
        if n != 0 {
            let last = &mut self.metadata.damage[n - 1];
            if last.y + last.height == y || n == MAX_DAMAGE {
                last.height = y + height - last.y;
                return;
            }
        }
        self.metadata.damage[n] = Rect {
            x: 0,
            y,
            width: self.request.extent.width,
            height,
        };
        self.metadata.damage_count += 1;
    }
    fn tracked_band(
        &mut self,
        exchange: &mut impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
        receipt: &mut Receipt,
    ) -> Result<PublishStep, PublishError> {
        let stride = self.request.extent.stride as usize;
        let band_rows = (MAX_TRANSFER / stride).min(dirty::ROWS as usize) as u32;
        let index = (self.y / band_rows) as usize;
        let (rows, hash, changed) = if let Some(prepared) = self.prepared_band {
            prepared
        } else {
            let rows = band_rows.min(self.request.extent.height - self.y);
            let bytes = &mut self.band[..rows as usize * stride];
            osaiku_ui::raster::rows(self.scene, self.y, bytes).map_err(PublishError::Content)?;
            let hash = dirty::fingerprint(bytes);
            let changed = !matches!(self.offer, Offer::Stream(..))
                || self.tracker.as_ref().unwrap().changed(index, hash);
            self.prepared_band = Some((rows, hash, changed));
            (rows, hash, changed)
        };
        let length = rows as usize * stride;
        self.request.offset = u64::from(self.y) * stride as u64;
        self.request.operation = if changed { UPLOAD } else { COPY_BASE };
        self.request.server_endpoint = if changed { 0 } else { length as u64 };
        let code = exchange(
            &self.request,
            if changed {
                &mut self.band[..length]
            } else {
                &mut []
            },
            receipt,
        );
        self.request.server_endpoint = 0;
        self.accept(code, receipt)?;
        self.tracker.as_mut().unwrap().record(index, hash);
        if changed && matches!(self.offer, Offer::Stream(..)) {
            self.add_damage(self.y, rows);
        }
        self.prepared_band = None;
        self.y += rows;
        if self.y == self.request.extent.height {
            self.phase = Phase::Ready;
        }
        Ok(if changed {
            PublishStep::Uploaded {
                offset: self.request.offset,
                length,
            }
        } else {
            PublishStep::Copied {
                offset: self.request.offset,
                length,
            }
        })
    }
    fn full_damage(&mut self) {
        self.metadata.damage.fill(Rect::default());
        self.metadata.damage_count = 1;
        self.metadata.damage[0] = Rect {
            x: 0,
            y: 0,
            width: self.request.extent.width,
            height: self.request.extent.height,
        };
    }
    /// One safe fallback after a refused delta BEGIN; no candidate exists yet.
    pub fn fallback_full(&mut self) -> bool {
        if self.phase != Phase::Prepared {
            return false;
        }
        let frame = match self.offer {
            Offer::Delta(delta) => delta.frame,
            Offer::Stream(frame, _) => frame,
            _ => return false,
        };
        self.offer = Offer::Frame(frame);
        self.request.operation = BEGIN_FRAME;
        self.full_damage();
        true
    }

    /// Known transport handle, including after a failed step/cancellation.
    pub fn handle(&self) -> Option<Handle> {
        (self.request.handle.serial != 0).then_some(self.request.handle)
    }

    /// One BEGIN, one row-aligned UPLOAD/COPY_BASE of at most MAX_TRANSFER,
    /// or one SEAL.
    /// A transport refusal leaves the cursor unchanged. Streaming publication
    /// retains a prepared band across Busy so retry does not rasterize twice.
    /// Scratch is cleared at SEAL/CANCEL; every fresh raster initializes its rows.
    pub fn step_with(
        &mut self,
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<PublishStep, PublishError> {
        let mut receipt = Receipt::default();
        match self.phase {
            Phase::Prepared => {
                let code = if let Offer::Delta(delta) = self.offer {
                    let mut delta = *delta;
                    let bytes = unsafe {
                        core::slice::from_raw_parts_mut(
                            (&mut delta as *mut DeltaOffer).cast::<u8>(),
                            core::mem::size_of::<DeltaOffer>(),
                        )
                    };
                    exchange(&self.request, bytes, &mut receipt)
                } else {
                    match self.offer {
                        Offer::Geometry(mut offer) => {
                            let bytes = unsafe {
                                core::slice::from_raw_parts_mut(
                                    (&mut offer as *mut GeometryOffer).cast::<u8>(),
                                    core::mem::size_of::<GeometryOffer>(),
                                )
                            };
                            exchange(&self.request, bytes, &mut receipt)
                        }
                        Offer::Frame(mut offer) => {
                            let bytes = unsafe {
                                core::slice::from_raw_parts_mut(
                                    (&mut offer as *mut FrameOffer).cast::<u8>(),
                                    core::mem::size_of::<FrameOffer>(),
                                )
                            };
                            exchange(&self.request, bytes, &mut receipt)
                        }
                        Offer::None => exchange(&self.request, &mut [], &mut receipt),
                        Offer::Stream(frame, base) => {
                            let mut offer = DeltaOffer {
                                frame,
                                base,
                                dirty: DirtyRows::EMPTY,
                            };
                            let bytes = unsafe {
                                core::slice::from_raw_parts_mut(
                                    (&mut offer as *mut DeltaOffer).cast::<u8>(),
                                    core::mem::size_of::<DeltaOffer>(),
                                )
                            };
                            exchange(&self.request, bytes, &mut receipt)
                        }
                        Offer::Delta(_) => unreachable!(),
                    }
                };
                if code != SYSCALL_OK {
                    return Err(PublishError::Transport(code));
                }
                if receipt.handle.target != self.request.handle.target
                    || receipt.handle.revision != self.request.handle.revision
                    || receipt.handle.serial == 0
                    || receipt.epoch != self.request.handle.target.epoch
                {
                    // There is no trustworthy handle to cancel. Caller must
                    // retire/reconstruct this tenancy, not retry BEGIN.
                    self.phase = Phase::Uncertain;
                    return Err(PublishError::Receipt);
                }
                self.request.handle = receipt.handle;
                self.metadata.handle = receipt.handle;
                self.phase = Phase::Uploading;
                Ok(PublishStep::Begun {
                    handle: receipt.handle,
                })
            }
            Phase::Uploading => {
                if self.tracker.is_some() {
                    return self.tracked_band(&mut exchange, &mut receipt);
                }
                let stride = self.request.extent.stride as usize;
                let mut rows =
                    (self.request.extent.height - self.y).min((self.band.len() / stride) as u32);
                let delta = match self.offer {
                    Offer::Delta(d) => Some(d),
                    _ => None,
                };
                let dirty = delta.is_none_or(|d| d.dirty.contains(self.y));
                if let Some(delta) = delta {
                    rows = (1..rows)
                        .find(|r| delta.dirty.contains(self.y + r) != dirty)
                        .unwrap_or(rows);
                }
                let length = rows as usize * stride;
                if !dirty {
                    self.request.operation = COPY_BASE;
                    self.request.offset = u64::from(self.y) * stride as u64;
                    self.request.server_endpoint = length as u64;
                    let code = exchange(&self.request, &mut [], &mut receipt);
                    self.request.server_endpoint = 0;
                    self.accept(code, &receipt)?;
                    self.y += rows;
                    if self.y == self.request.extent.height {
                        self.phase = Phase::Ready;
                    }
                    return Ok(PublishStep::Copied {
                        offset: self.request.offset,
                        length,
                    });
                }
                self.request.operation = UPLOAD;
                self.request.offset = u64::from(self.y) * stride as u64;
                let rendered =
                    osaiku_ui::raster::rows(self.scene, self.y, &mut self.band[..length]);
                if let Err(error) = rendered {
                    self.band.fill(0);
                    return Err(PublishError::Content(error));
                }
                let code = exchange(&self.request, &mut self.band[..length], &mut receipt);
                if let Err(error) = self.accept(code, &receipt) {
                    self.band.fill(0);
                    return Err(error);
                }
                self.y += rows;
                if self.y == self.request.extent.height {
                    self.phase = Phase::Ready;
                }
                Ok(PublishStep::Uploaded {
                    offset: self.request.offset,
                    length,
                })
            }
            Phase::Ready => {
                self.band.fill(0);
                self.request.operation = SEAL;
                self.request.offset = 0;
                // repr(C) integer-only record; parent's size assertion checks
                // that no uninitialized padding can enter the wire.
                let bytes = unsafe {
                    core::slice::from_raw_parts_mut(
                        (self.metadata as *mut Publication).cast::<u8>(),
                        core::mem::size_of::<Publication>(),
                    )
                };
                let code = exchange(&self.request, bytes, &mut receipt);
                self.accept(code, &receipt)?;
                self.phase = Phase::Sealed;
                Ok(PublishStep::Sealed {
                    handle: self.request.handle,
                })
            }
            Phase::Sealed => Ok(PublishStep::Sealed {
                handle: self.request.handle,
            }),
            Phase::Uncertain => Err(PublishError::Receipt),
            Phase::Cancelled => Err(PublishError::Transport(SYSCALL_ERR_INVALID_STATE)),
        }
    }

    fn accept(&mut self, code: isize, receipt: &Receipt) -> Result<(), PublishError> {
        if code != SYSCALL_OK {
            return Err(PublishError::Transport(code));
        }
        if receipt.handle != self.request.handle
            || receipt.epoch != self.request.handle.target.epoch
        {
            self.phase = Phase::Uncertain;
            return Err(PublishError::Receipt);
        }
        Ok(())
    }

    /// At most one CANCEL. A refusal retains the exact handle for retry. A stale
    /// handle means no candidate remains, e.g. after epoch revocation. Cancel
    /// does not undo a publication already accepted by SEAL.
    pub fn cancel_with(
        &mut self,
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), PublishError> {
        if self.phase == Phase::Sealed {
            return Err(PublishError::Transport(SYSCALL_ERR_INVALID_STATE));
        }
        self.band.fill(0);
        self.prepared_band = None;
        if self.phase == Phase::Cancelled {
            return Ok(());
        }
        if self.phase == Phase::Prepared {
            self.phase = Phase::Cancelled;
            return Ok(());
        }
        if self.handle().is_none() {
            return Err(PublishError::Receipt);
        }
        let mut request = self.request;
        request.operation = CANCEL;
        request.offset = 0;
        let mut receipt = Receipt::default();
        let code = exchange(&request, &mut [], &mut receipt);
        if code != SYSCALL_OK && code != SYSCALL_ERR_STALE_ENDPOINT {
            return Err(PublishError::Cleanup {
                handle: request.handle,
                code,
            });
        }
        if code == SYSCALL_OK {
            self.accept(code, &receipt)?;
        }
        self.phase = Phase::Cancelled;
        Ok(())
    }
}
