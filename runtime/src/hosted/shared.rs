//! CPU rendering directly into a retained two-buffer window pool. The kernel
//! revokes write access at seal; old displayed generations remain pinned.
use super::RasterError;
use crate::{abi::*, surface::*, window_buffer::Buffer};
struct Slot {
    target: Target,
    buffer: Buffer,
    used: u64,
}
pub(super) struct Pool {
    slots: [Option<Slot>; 8],
    clock: u64,
}
impl Pool {
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; 8],
            clock: 0,
        }
    }
    fn acquire(&mut self, target: Target, bytes: usize) -> Result<&mut Buffer, isize> {
        self.clock = self.clock.saturating_add(1);
        let mut chosen = None;
        // Resize retires obsolete producer slots immediately. Displayed pins
        // keep old pixels alive, but unused old sizes must not exhaust quota.
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|s| s.target == target && s.buffer.len() != bytes)
            {
                let old = slot.take().unwrap();
                if let Err(buffer) = old.buffer.release() {
                    *slot = Some(Slot { buffer, ..old });
                    return Err(-5);
                }
            }
        }
        for i in 0..self.slots.len() {
            if let Some(s) = self.slots[i]
                .as_mut()
                .filter(|s| s.target == target && s.buffer.len() == bytes)
            {
                match s.buffer.write() {
                    Ok(()) => {
                        chosen = Some(i);
                        break;
                    }
                    Err(SYSCALL_ERR_BUSY) => (),
                    Err(e) => return Err(e),
                }
            }
        }
        let n = if let Some(n) = chosen {
            n
        } else {
            if self
                .slots
                .iter()
                .flatten()
                .filter(|s| s.target == target && s.buffer.len() == bytes)
                .count()
                >= 2
            {
                return Err(SYSCALL_ERR_BUSY);
            }
            let n = self
                .slots
                .iter()
                .position(Option::is_none)
                .or_else(|| {
                    self.slots
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, s)| s.as_ref().unwrap().used)
                        .map(|(i, _)| i)
                })
                .ok_or(-10isize)?;
            if let Some(old) = self.slots[n].take() {
                if let Err(buffer) = old.buffer.release() {
                    self.slots[n] = Some(Slot { buffer, ..old });
                    return Err(-5);
                }
            }
            self.slots[n] = Some(Slot {
                target,
                buffer: Buffer::create(bytes)?,
                used: self.clock,
            });
            n
        };
        let s = self.slots[n].as_mut().unwrap();
        s.used = self.clock;
        Ok(&mut s.buffer)
    }
}
fn data<T>(v: &mut T) -> &mut [u8] {
    unsafe { core::slice::from_raw_parts_mut((v as *mut T).cast(), core::mem::size_of::<T>()) }
}
pub(super) fn publish<L: AsRef<osaiku_ui::layout::Layout>>(
    pool: &mut Pool,
    scene: &osaiku_ui::paint::Scene<'_, L>,
    offer: FrameOffer,
    wire: &[u8],
    metadata: &mut Publication,
    tracker: &mut DirtyTracker,
    exchange: &mut impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    checkpoint: &mut impl FnMut() -> Result<(), isize>,
) -> Result<Handle, RasterError> {
    let error = |e| RasterError::Publish(PublishError::Transport(e));
    let extent = offer
        .geometry
        .content_extent()
        .map_err(|e| RasterError::Publish(PublishError::Content(e)))?;
    let previous = tracker.base.take().filter(|p| {
        p.frame.geometry == offer.geometry && p.handle.revision < offer.request.revision
    });
    let buffer = pool
        .acquire(
            offer.geometry.target,
            extent
                .bytes()
                .map_err(|e| RasterError::Publish(PublishError::Content(e)))? as usize,
        )
        .map_err(error)?;
    metadata
        .prepare(
            scene,
            Handle {
                target: offer.geometry.target,
                revision: offer.request.revision,
                serial: 1,
            },
            &[],
        )
        .map_err(|e| RasterError::Publish(PublishError::Content(e)))?;
    metadata.source_hash = osaiku_ui::wire_hash(wire);
    metadata.damage.fill(Rect::default());
    metadata.damage_count = 0;
    let mut timing = crate::gpu_timing::Accumulator::EMPTY;
    let stride = extent.stride as usize;
    let rows = (MAX_TRANSFER / stride).min(crate::surface::dirty::ROWS as usize);
    for (index, y) in (0..extent.height as usize).step_by(rows).enumerate() {
        let count = rows.min(extent.height as usize - y);
        let bytes = &mut buffer.bytes_mut()[y * stride..(y + count) * stride];
        let start = crate::gpu_timing::stamp();
        osaiku_ui::raster::rows(scene, y as u32, bytes)
            .map_err(|e| RasterError::Publish(PublishError::Content(e)))?;
        timing.add(start, crate::gpu_timing::stamp(), bytes.len());
        let hash = crate::surface::dirty::fingerprint(bytes);
        if previous.is_none() || tracker.changed(index, hash) {
            let n = metadata.damage_count as usize;
            if n > 0
                && (metadata.damage[n - 1].y + metadata.damage[n - 1].height == y as u32
                    || n == MAX_DAMAGE)
            {
                metadata.damage[n - 1].height = (y + count) as u32 - metadata.damage[n - 1].y;
            } else {
                metadata.damage[n] = Rect {
                    x: 0,
                    y: y as u32,
                    width: extent.width,
                    height: count as u32,
                };
                metadata.damage_count += 1;
            }
        }
        tracker.record(index, hash);
        checkpoint().map_err(RasterError::Checkpoint)?;
    }
    let h = buffer.seal().map_err(error)?;
    let mut shared = SharedOffer {
        frame: offer,
        buffer: h,
        base: previous.map_or(Handle::default(), |p| p.handle),
    };
    let mut request = Request {
        version: REQUEST_VERSION,
        operation: BEGIN_SHARED,
        handle: Handle {
            target: offer.geometry.target,
            serial: 0,
            revision: offer.request.revision,
        },
        extent,
        ..Request::default()
    };
    let mut receipt = Receipt::default();
    let mut retries = 32;
    loop {
        let code = exchange(&request, data(&mut shared), &mut receipt);
        if code == SYSCALL_ERR_STALE_ENDPOINT && shared.base != Handle::default() {
            shared.base = Handle::default();
            metadata.damage.fill(Rect::default());
            metadata.damage[0] = Rect {
                x: 0,
                y: 0,
                width: extent.width,
                height: extent.height,
            };
            metadata.damage_count = 1;
            continue;
        }
        if code == SYSCALL_ERR_BUSY && retries > 0 {
            retries -= 1;
            checkpoint().map_err(RasterError::Checkpoint)?;
            continue;
        }
        if code != SYSCALL_OK {
            return Err(error(code));
        }
        break;
    }
    if receipt.epoch != offer.geometry.target.epoch
        || receipt.handle.target != request.handle.target
        || receipt.handle.revision != request.handle.revision
        || receipt.handle.serial == 0
    {
        return Err(RasterError::Publish(PublishError::Receipt));
    }
    request.handle = receipt.handle;
    metadata.handle = receipt.handle;
    request.operation = SEAL;
    let result = loop {
        let code = exchange(&request, data(metadata), &mut receipt);
        if code == SYSCALL_ERR_BUSY && retries > 0 {
            retries -= 1;
            if let Err(e) = checkpoint() {
                break Err(RasterError::Checkpoint(e));
            }
            continue;
        }
        if code != SYSCALL_OK {
            break Err(error(code));
        }
        if receipt.handle != request.handle || receipt.epoch != offer.geometry.target.epoch {
            break Err(RasterError::Publish(PublishError::Receipt));
        }
        break Ok(request.handle);
    };
    if result.is_err() {
        request.operation = CANCEL;
        let code = exchange(&request, &mut [], &mut receipt);
        if code != SYSCALL_OK && code != SYSCALL_ERR_STALE_ENDPOINT {
            return Err(RasterError::Publish(PublishError::Cleanup {
                handle: request.handle,
                code,
            }));
        }
    }
    if let Ok(handle) = result {
        tracker.base = Some(FrameSnapshot {
            handle,
            frame: offer,
        });
        crate::gpu_timing::emit(timing.record(0));
    }
    result
}
