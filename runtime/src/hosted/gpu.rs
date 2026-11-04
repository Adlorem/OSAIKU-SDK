use super::RasterError;
use crate::{
    abi::*,
    gpu_painter::{self as gpu, Command, GpuPainter},
    graphics::{self, Negotiation},
    surface::*,
};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
static FAILED: AtomicBool = AtomicBool::new(false);
static RECOVERED: AtomicBool = AtomicBool::new(false);
static FAILURE: AtomicU32 = AtomicU32::new(0);
static DEVICE: AtomicU64 = AtomicU64::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);
fn log(reason: u32, frames: u64, glyphs: u64, images: u64) {
    #[cfg(target_os = "none")]
    {
        use core::fmt::Write;
        struct Log {
            b: [u8; 160],
            n: usize,
        }
        impl Write for Log {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let end = self.n + s.len();
                self.b
                    .get_mut(self.n..end)
                    .ok_or(core::fmt::Error)?
                    .copy_from_slice(s.as_bytes());
                self.n = end;
                Ok(())
            }
        }
        let mut l = Log { b: [0; 160], n: 0 };
        let _=writeln!(l,"sdk-gpu:v1;app={};renderer={};reason={reason};frames={frames};glyph_uploads={glyphs};image_uploads={images};readback=0",gpu::application(),u8::from(reason==0));
        if let Ok(s) = core::str::from_utf8(&l.b[..l.n]) {
            crate::log_write(s);
        }
    }
    #[cfg(not(target_os = "none"))]
    let _ = (reason, frames, glyphs, images);
}
fn bytes<T>(value: &mut T) -> &mut [u8] {
    unsafe { core::slice::from_raw_parts_mut((value as *mut T).cast(), core::mem::size_of::<T>()) }
}
pub(super) fn publish<L: AsRef<osaiku_ui::layout::Layout>>(
    scene: &osaiku_ui::paint::Scene<'_, L>,
    offer: FrameOffer,
    wire: &[u8],
    metadata: &mut Publication,
    commands: &mut [Command; gpu::COMMAND_CAPACITY],
    exchange: &mut impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    checkpoint: &mut impl FnMut() -> Result<(), isize>,
) -> Result<Option<Handle>, RasterError> {
    if !gpu::available() {
        return Ok(None);
    }
    let mut query = Negotiation::gpu_default(graphics::capability::UI_RENDERER);
    if FAILED.load(Ordering::Acquire) {
        query.policy = graphics::policy::DIAGNOSTIC_CPU;
    }
    let selected = graphics::negotiate_with(query, &mut *exchange).map_err(RasterError::Publish)?;
    if selected.renderer == 0 {
        log(
            if FAILED.load(Ordering::Acquire) {
                FAILURE.load(Ordering::Acquire)
            } else {
                selected.fallback as u32
            },
            0,
            0,
            0,
        );
        return Ok(None);
    }
    let previous = DEVICE.swap(selected.device, Ordering::AcqRel);
    let generation = GENERATION.swap(selected.generation, Ordering::AcqRel);
    if previous != 0 && (previous != selected.device || generation != selected.generation) {
        if let Err(e) = gpu::call(&mut Command {
            op: 8,
            ..Command::EMPTY
        }) {
            FAILURE.store(e, Ordering::Release);
            FAILED.store(true, Ordering::Release);
            log(e, 0, 0, 0);
            return Ok(None);
        }
    }
    let viewport = scene.layout().viewport;
    let mut begin = Command {
        op: 1,
        rect: [0, 0, viewport.width as i32, viewport.height as i32],
        ..Command::EMPTY
    };
    let mut render = || {
        gpu::call(&mut begin)?;
        let mut painter = GpuPainter::new(viewport, commands, gpu::call);
        scene.paint(&mut painter);
        painter.finish()?;
        let mut done = Command {
            op: 2,
            result: [offer.geometry.target.owner, 0, 0, 0],
            ..Command::EMPTY
        };
        gpu::call(&mut done)?;
        Ok::<_, u32>(done.result)
    };
    let mut rendered = render();
    if rendered.is_err() && !RECOVERED.swap(true, Ordering::AcqRel) {
        log(rendered.as_ref().err().copied().unwrap_or(5), 0, 0, 0);
        if gpu::call(&mut Command {
            op: 8,
            ..Command::EMPTY
        })
        .is_ok()
        {
            rendered = render();
        }
    }
    let result = match rendered {
        Ok(r) => r,
        Err(e) => {
            // Stop calling a potentially poisoned context. CPU event loop stays alive.
            let _ = gpu::call(&mut Command {
                op: 8,
                ..Command::EMPTY
            });
            FAILURE.store(e, Ordering::Release);
            FAILED.store(true, Ordering::Release);
            log(e, 0, 0, 0);
            return Ok(None);
        }
    };
    let mut gpu_offer = GpuOffer {
        frame: offer,
        data: result[0],
        fence: result[1],
        stride: result[2],
        format: result[3],
    };
    let mut request = Request {
        version: REQUEST_VERSION,
        operation: BEGIN_GPU,
        handle: Handle {
            target: offer.geometry.target,
            serial: 0,
            revision: offer.request.revision,
        },
        extent: offer
            .geometry
            .content_extent()
            .map_err(|e| RasterError::Publish(PublishError::Content(e)))?,
        ..Request::default()
    };
    let mut handle = None;
    let publication = (|| {
        let mut receipt = Receipt::default();
        let mut retries = 32;
        loop {
            let code = exchange(&request, bytes(&mut gpu_offer), &mut receipt);
            if code == SYSCALL_ERR_BUSY && retries > 0 {
                retries -= 1;
                checkpoint().map_err(RasterError::Checkpoint)?;
                continue;
            }
            if code != SYSCALL_OK {
                return Err(RasterError::Publish(PublishError::Transport(code)));
            }
            break;
        }
        handle = Some(receipt.handle);
        if receipt.epoch != offer.geometry.target.epoch
            || receipt.handle.target != request.handle.target
            || receipt.handle.revision != request.handle.revision
            || receipt.handle.serial == 0
        {
            return Err(RasterError::Publish(PublishError::Receipt));
        }
        request.handle = receipt.handle;
        metadata
            .prepare(scene, request.handle, &[])
            .map_err(|e| RasterError::Publish(PublishError::Content(e)))?;
        metadata.source_hash = osaiku_ui::wire_hash(wire);
        request.operation = SEAL;
        let code = exchange(&request, bytes(metadata), &mut receipt);
        if code != SYSCALL_OK {
            return Err(RasterError::Publish(PublishError::Transport(code)));
        }
        if receipt.handle != request.handle || receipt.epoch != request.handle.target.epoch {
            return Err(RasterError::Publish(PublishError::Receipt));
        }
        Ok(Some(request.handle))
    })();
    if publication.is_err() {
        if let Some(h) = handle {
            request.operation = CANCEL;
            request.handle = h;
            let code = exchange(&request, &mut [], &mut Receipt::default());
            if code != SYSCALL_OK {
                return Err(RasterError::Publish(PublishError::Cleanup {
                    handle: h,
                    code,
                }));
            }
        }
    }
    let _ = gpu::call(&mut Command {
        op: 7,
        ..Command::EMPTY
    });
    if publication.is_ok() {
        let mut stats = Command {
            op: 10,
            ..Command::EMPTY
        };
        if gpu::call(&mut stats).is_ok() {
            log(0, stats.result[0], stats.result[1], stats.result[2]);
        }
    }
    publication
}
