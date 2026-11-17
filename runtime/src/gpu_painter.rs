//! Bounded application Painter commands. Native dispatch owns EGL on one worker.
//! No pixels are read back; static glyph/image bytes remain valid through dispatch.
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use osaiku_ui::{
    font::ui_text_run,
    layout::Rect,
    paint::{Painter, RoundedShape},
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Command {
    pub op: u32,
    pub color: u32,
    pub rect: [i32; 4],
    pub clip: [i32; 4],
    pub shape: [i32; 4],
    pub params: [u32; 8],
    pub data: *const u8,
    pub len: usize,
    pub result: [u64; 4],
}
impl Command {
    pub const EMPTY: Self = Self {
        op: 0,
        color: 0,
        rect: [0; 4],
        clip: [0; 4],
        shape: [0; 4],
        params: [0; 8],
        data: core::ptr::null(),
        len: 0,
        result: [0; 4],
    };
}
pub type Dispatch = unsafe extern "C" fn(*mut Command) -> i32;
static DISPATCH: AtomicUsize = AtomicUsize::new(0);
static APPLICATION: AtomicU32 = AtomicU32::new(0);
static VOLATILE_IMAGES: AtomicBool = AtomicBool::new(false);
/// Opt this process into drawing mutable/private images without texture caching.
/// Call before the first frame; static application assets retain the fast default.
pub fn volatile_images() { VOLATILE_IMAGES.store(true, Ordering::Release); }
/// Diagnostic label only; the kernel supplies the authenticated owner and seals.
pub fn label(application:u32) { APPLICATION.store(application,Ordering::Release); }
pub fn application()->u32 { APPLICATION.load(Ordering::Acquire) }
/// Installed once by the linked, process-private graphics runtime before events.
pub fn install(dispatch: Dispatch) {
    DISPATCH.store(dispatch as usize, Ordering::Release);
}
pub fn available() -> bool {
    DISPATCH.load(Ordering::Acquire) != 0
}
pub fn call(command: &mut Command) -> Result<(), u32> {
    let p = DISPATCH.load(Ordering::Acquire);
    if p == 0 {
        return Err(19);
    }
    let f: Dispatch = unsafe { core::mem::transmute(p) };
    match unsafe { f(command) } {
        0 => Ok(()),
        e => Err(e as u32),
    }
}
fn rect(r: Rect) -> [i32; 4] {
    [r.x, r.y, r.width as i32, r.height as i32]
}
pub const COMMAND_CAPACITY: usize = 32;
/// Frame lifecycle has the same ordered channel as drawing, but occupies a
/// complete batch. No renderer-private result or pointer is a request field.
pub fn encode_control(scope: osaiku_gpu_protocol::render::Scope, width: u32, height: u32,
    operation: u32, out: &mut [u8]) -> Result<usize, osaiku_gpu_protocol::render::Error> {
    use osaiku_gpu_protocol::render::{self, Draw, Error, NO_ASSET};
    if !matches!(operation, 1 | 2 | 7 | 8 | 10) { return Err(Error::Operation); }
    let draw = Draw { operation, color: 0,
        rect: if operation == 1 { [0, 0, width as i32, height as i32] } else { [0; 4] },
        clip: [0; 4], shape: [0; 4], params: [0; 8], asset: NO_ASSET, offset: 0, length: 0 };
    render::encode(scope, width, height, &[draw], out)
}
/// Encode one drawing batch after its assets have been registered as immutable
/// buffers. `seal` translates a process-local source to a kernel-bound handle;
/// it must retain that seal until the receiver finishes the batch and release
/// any newly retained seals if encoding fails. Native
/// pointers and result words never enter the wire representation.
pub fn encode_batch(
    scope: osaiku_gpu_protocol::render::Scope,
    width: u32,
    height: u32,
    commands: &[Command],
    out: &mut [u8],
    seal: impl FnMut(&Command) -> Result<(osaiku_gpu_protocol::buffer::Handle, u64), osaiku_gpu_protocol::render::Error>,
) -> Result<usize, osaiku_gpu_protocol::render::Error> {
    let mut draws = EMPTY_DRAWS;
    encode_batch_with_scratch(scope, width, height, commands, out, &mut draws, seal)
}
pub const EMPTY_DRAWS: [osaiku_gpu_protocol::render::Draw; osaiku_gpu_protocol::render::MAX_DRAWS] = [
    osaiku_gpu_protocol::render::Draw { operation: 0, color: 0, rect: [0; 4], clip: [0; 4],
        shape: [0; 4], params: [0; 8], asset: osaiku_gpu_protocol::render::NO_ASSET, offset: 0, length: 0 };
    osaiku_gpu_protocol::render::MAX_DRAWS];
/// Native clients provide process-owned storage instead of nesting another
/// full batch on the 16 KiB application stack. Output remains atomic on failure.
pub fn encode_batch_with_scratch(
    scope: osaiku_gpu_protocol::render::Scope,
    width: u32, height: u32, commands: &[Command], out: &mut [u8],
    draws: &mut [osaiku_gpu_protocol::render::Draw; osaiku_gpu_protocol::render::MAX_DRAWS],
    mut seal: impl FnMut(&Command) -> Result<(osaiku_gpu_protocol::buffer::Handle, u64), osaiku_gpu_protocol::render::Error>,
) -> Result<usize, osaiku_gpu_protocol::render::Error> {
    use osaiku_gpu_protocol::render::{self, Draw, Error, NO_ASSET};
    if commands.is_empty() || commands.len() > render::MAX_DRAWS
        || out.len() < render::HEADER_BYTES + commands.len() * render::DRAW_BYTES {
        return Err(Error::Length);
    }
    for (c, draw) in commands.iter().zip(draws.iter_mut()) {
        if !(3..=6).contains(&c.op) || c.result != [0; 4] {
            return Err(Error::Operation);
        }
        let (asset, offset) = if matches!(c.op, 4 | 6) {
            if c.data.is_null() || c.len == 0 { return Err(Error::Asset); }
            seal(c)?
        } else {
            if !c.data.is_null() || c.len != 0 { return Err(Error::Asset); }
            (NO_ASSET, 0)
        };
        *draw = Draw { operation: c.op, color: c.color, rect: c.rect, clip: c.clip,
            shape: c.shape, params: c.params, asset, offset, length: c.len as u64 };
    }
    render::encode(scope, width, height, &draws[..commands.len()], out)
}
pub struct GpuPainter<'a, F: FnMut(&mut Command) -> Result<(), u32>> {
    submit: F,
    viewport: Rect,
    error: Option<u32>,
    commands: &'a mut [Command; COMMAND_CAPACITY],
    count: usize,
}
impl<'a, F: FnMut(&mut Command) -> Result<(), u32>> GpuPainter<'a, F> {
    /// Borrow process-owned scratch. Keeping this array in the painter creates
    /// multiple 4352-byte stack copies during construction, retry and finish.
    pub fn new(viewport: Rect, commands: &'a mut [Command; COMMAND_CAPACITY], submit: F) -> Self {
        Self {
            submit,
            viewport,
            error: None,
            commands,
            count: 0,
        }
    }
    pub fn finish(mut self) -> Result<(), u32> {
        self.flush();
        self.error.map_or(Ok(()), Err)
    }
    fn flush(&mut self) {
        if self.count == 0 || self.error.is_some() {
            return;
        }
        let mut c = Command {
            op: 9,
            data: self.commands.as_ptr().cast(),
            len: self.count * core::mem::size_of::<Command>(),
            ..Command::EMPTY
        };
        if let Err(e) = (self.submit)(&mut c) {
            self.error = Some(e);
        }
        self.count = 0;
    }
    fn emit(&mut self, mut c: Command, clip: Rect) {
        let clip = clip.intersect(self.viewport);
        if self.error.is_some() || clip.width == 0 || clip.height == 0 {
            return;
        }
        c.clip = rect(clip);
        if self.count == COMMAND_CAPACITY {
            self.flush();
        }
        if self.error.is_none() {
            self.commands[self.count] = c;
            self.count += 1;
        }
    }
}
impl<F: FnMut(&mut Command) -> Result<(), u32>> Painter for GpuPainter<'_, F> {
    fn visible(&self, bounds: Rect) -> bool {
        let r = bounds.intersect(self.viewport);
        r.width != 0 && r.height != 0
    }
    fn fill_alpha(&mut self, r: Rect, color: u32, alpha: u8, clip: Rect) {
        if alpha != 0 {
            self.emit(
                Command {
                    op: 3,
                    color,
                    rect: rect(r),
                    params: [alpha.into(), 0, 0, 0, 0, 0, 0, 0],
                    ..Command::EMPTY
                },
                clip,
            );
        }
    }
    fn text(&mut self, r: Rect, text: &str, size: u16, color: u32, clip: Rect) {
        for (pen, _, glyph) in ui_text_run(text, size) {
            let ink = Rect {
                x: r.x + pen + glyph.left(),
                y: r.y + glyph.top() as i32,
                width: glyph.width(),
                height: glyph.height(),
            };
            if r.x + pen >= clip.right() {
                break;
            }
            if ink.width == 0 || ink.height == 0 || !self.visible(ink) {
                continue;
            }
            let bytes = glyph.packed_coverage();
            self.emit(
                Command {
                    op: 4,
                    color,
                    rect: rect(ink),
                    data: bytes.as_ptr(),
                    len: bytes.len(),
                    params: [255, size.into(), 1, 0, 0, 0, 0, 0],
                    ..Command::EMPTY
                },
                clip,
            );
        }
    }
    fn line(&mut self, from: (i32, i32), to: (i32, i32), color: u32, clip: Rect) {
        // Geometry, not CPU rasterization: emit Bresenham's exact pixel quads.
        let (mut x, mut y) = from;
        let dx = (to.0 - x).abs();
        let dy = -(to.1 - y).abs();
        if dx.max(-dy) > 8192 {
            self.error = Some(22);
            return;
        }
        let sx = if x < to.0 { 1 } else { -1 };
        let sy = if y < to.1 { 1 } else { -1 };
        let mut error = dx + dy;
        for _ in 0..=dx.max(-dy) {
            self.fill(
                Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
                color,
                clip,
            );
            if (x, y) == to {
                break;
            }
            let e = 2 * error;
            if e >= dy {
                error += dy;
                x += sx;
            }
            if e <= dx {
                error += dx;
                y += sy;
            }
        }
    }
    fn fill_shape(&mut self, area: Rect, shape: RoundedShape, color: u32, clip: Rect) {
        let (r, radius, widths, alpha) = shape.gpu_parts();
        let w = widths.unwrap_or([0; 4]);
        self.emit(
            Command {
                op: 5,
                color,
                rect: rect(area),
                shape: rect(r),
                params: [
                    alpha.into(),
                    radius.into(),
                    u32::from(widths.is_some()),
                    w[0].into(),
                    w[1].into(),
                    w[2].into(),
                    w[3].into(),
                    0,
                ],
                ..Command::EMPTY
            },
            clip,
        );
    }
    fn image(&mut self, at: Rect, image: &'static osaiku_ui::image::Image, clip: Rect) {
        if VOLATILE_IMAGES.load(Ordering::Acquire) {
            osaiku_ui::image::runs(self, at, image, clip);
            return;
        }
        self.emit(
            Command {
                op: 6,
                rect: rect(at),
                data: image.pixels.as_ptr().cast(),
                len: core::mem::size_of_val(image.pixels),
                params: [255, image.width.into(), image.height.into(), 0, 0, 0, 0, 0],
                ..Command::EMPTY
            },
            clip,
        );
    }
}

/// Distinct operation: legacy pixel messages keep their original meaning.
pub const BEGIN_GPU: u32 = 25;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GpuOffer {
    pub frame: crate::surface::FrameOffer,
    pub data: u64,
    pub fence: u64,
    pub stride: u64,
    pub format: u64,
}
