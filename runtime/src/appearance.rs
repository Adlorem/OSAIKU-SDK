//! The desktop background the compositor paints under every window: one of
//! the SDK's background presets, optionally with a picture over it, and how
//! much of it shows through the taskbar; and the base theme (Dark or Light)
//! and accent colour of the shell, the window frames and the applications. A process granted DesktopAppearanceWrite
//! may change them, and the setting lasts until restart.
//!
//! A picture travels as straight ARGB rows of exactly the screen's size, the
//! caller having decoded and fitted it: BEGIN reserves them, ROWS
//! copies some, COMMIT hands the complete picture to the compositor. The
//! kernel decodes nothing. A transparent pixel shows the preset beneath.
//!
//! `SYSCALL`: RDI the [`Request`], RSI the [`Reply`], and for ROWS RDX the
//! rows and R8 their byte count (at most [`CHUNK`]).
pub const SYSCALL: u64 = 391;
pub const VERSION: u32 = 3;
/// Largest ROWS transfer.
pub const CHUNK: usize = 64 * 1024;
/// Largest screen a picture is kept for, per side.
pub const MAX_SIDE: u32 = 4096;

pub mod operation {
    /// The current background and the screen's size.
    pub const QUERY: u32 = 1;
    /// Show `preset`, taskbar step `shelf`, base theme `base` and accent
    /// `accent`; keep the current picture over the preset when `keep_picture`.
    pub const PRESET: u32 = 2;
    /// Reserve a picture of the screen's size, to go over `preset`, with
    /// taskbar step `shelf` and base theme `base` once committed.
    pub const BEGIN: u32 = 3;
    /// Copy `rows` rows from `first_row` into the reserved picture.
    pub const ROWS: u32 = 4;
    /// Show the reserved picture once every row has been copied.
    pub const COMMIT: u32 = 5;
    /// Drop the reserved picture.
    pub const CANCEL: u32 = 6;
}
pub mod status {
    pub const OK: u32 = 0;
    pub const INVALID: u32 = 1;
    /// No picture is reserved by this caller, or the screen changed size.
    pub const STALE: u32 = 2;
    /// The picture does not fit in memory.
    pub const NO_MEMORY: u32 = 3;
    pub const BUSY: u32 = 4;
}
/// How a compositor background job paints: `parameters[1]` of the job.
pub mod mode {
    /// `parameters[0]` as one colour; the boot-time desktop.
    pub const SOLID: u64 = 0;
    /// Preset `parameters[2]`, a colour or a vertical gradient.
    pub const PRESET: u64 = 1;
    /// The job's rows of the picture over preset `parameters[2]`.
    pub const PICTURE: u64 = 2;
}
// Every mode: `parameters[3]` is the taskbar's transparency step.

/// A preset index the SDK defines.
pub fn preset_valid(preset: u64) -> bool {
    preset < osaiku_ui::appearance::BACKGROUNDS.len() as u64
}
/// A taskbar transparency step the SDK defines; 0 is opaque.
pub fn shelf_valid(shelf: u64) -> bool {
    shelf < osaiku_ui::appearance::SHELF_TRANSPARENCY.len() as u64
}
/// A base theme index the SDK defines: 0 Dark, 1 Light.
pub fn base_valid(base: u64) -> bool {
    u32::try_from(base).is_ok_and(|base| osaiku_ui::appearance::Base::from_index(base).is_some())
}
/// An accent the SDK defines (`osaiku_ui::appearance::ACCENTS`).
pub fn accent_valid(accent: u64) -> bool {
    accent < osaiku_ui::appearance::ACCENTS.len() as u64
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub version: u32,
    pub operation: u32,
    pub sequence: u64,
    pub preset: u32,
    pub keep_picture: u32,
    pub first_row: u32,
    pub rows: u32,
    pub shelf: u32,
    /// `osaiku_ui::appearance::Base::index`.
    pub base: u32,
    /// `osaiku_ui::appearance::ACCENTS` index.
    pub accent: u32,
    /// Zero; keeps the layout free of padding.
    pub reserved: u32,
}
const _: () = assert!(core::mem::size_of::<Request>() == 48);
impl Request {
    pub const EMPTY: Self = Self {
        version: VERSION,
        operation: 0,
        sequence: 0,
        preset: 0,
        keep_picture: 0,
        first_row: 0,
        rows: 0,
        shelf: 0,
        base: 0,
        accent: 0,
        reserved: 0,
    };
    pub fn canonical(&self) -> bool {
        let look = |s: &Self| {
            preset_valid(u64::from(s.preset))
                && shelf_valid(u64::from(s.shelf))
                && base_valid(u64::from(s.base))
                && accent_valid(u64::from(s.accent))
        };
        let no_look = |s: &Self| {
            s.preset == 0 && s.shelf == 0 && s.keep_picture == 0 && s.base == 0 && s.accent == 0
        };
        let no_rows = |s: &Self| s.first_row == 0 && s.rows == 0;
        self.version == VERSION
            && self.reserved == 0
            && self.sequence != 0
            && match self.operation {
                operation::QUERY | operation::COMMIT | operation::CANCEL => {
                    no_look(self) && no_rows(self)
                }
                operation::PRESET => look(self) && self.keep_picture <= 1 && no_rows(self),
                operation::BEGIN => look(self) && self.keep_picture == 0 && no_rows(self),
                operation::ROWS => {
                    no_look(self)
                        && self.rows != 0
                        && self.first_row < MAX_SIDE
                        && self.rows <= MAX_SIDE - self.first_row
                }
                _ => false,
            }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub version: u32,
    pub status: u32,
    pub sequence: u64,
    /// The preset shown, or to be shown once the compositor next paints.
    pub preset: u32,
    /// 1 when a picture is shown over it.
    pub picture: u32,
    /// The screen; a picture has exactly this size.
    pub width: u32,
    pub height: u32,
    /// The taskbar's transparency step.
    pub shelf: u32,
    /// The base theme: `osaiku_ui::appearance::Base::index`.
    pub base: u32,
    /// The accent: `osaiku_ui::appearance::ACCENTS` index.
    pub accent: u32,
    /// Zero; keeps the layout free of padding.
    pub reserved: u32,
}
const _: () = assert!(core::mem::size_of::<Reply>() == 48);
impl Reply {
    pub const EMPTY: Self = Self {
        version: VERSION,
        status: status::INVALID,
        sequence: 0,
        preset: 0,
        picture: 0,
        width: 0,
        height: 0,
        shelf: 0,
        base: 0,
        accent: 0,
        reserved: 0,
    };
}

/// A transport success still needs the exact request's receipt. No retry is
/// implicit: callers retain their sequence and decide how to handle BUSY.
pub fn exchange_with(
    request: &Request,
    data: &[u8],
    mut exchange: impl FnMut(&Request, &[u8], &mut Reply) -> isize,
) -> Result<Reply, isize> {
    use crate::abi::*;
    if !request.canonical()
        || (request.operation == operation::ROWS && (data.is_empty() || data.len() > CHUNK))
        || (request.operation != operation::ROWS && !data.is_empty())
    {
        return Err(SYSCALL_ERR_INVALID_STATE);
    }
    let mut reply = Reply::EMPTY;
    let code = exchange(request, data, &mut reply);
    if code != 0 {
        return Err(code);
    }
    if reply.version != VERSION
        || reply.reserved != 0
        || reply.sequence != request.sequence
        || reply.status > status::BUSY
        || (reply.status == status::OK
            && (!preset_valid(reply.preset as u64)
                || !base_valid(reply.base as u64)
                || !accent_valid(reply.accent as u64)
                || !shelf_valid(reply.shelf as u64)
                || reply.picture > 1
                || !(1..=MAX_SIDE).contains(&reply.width)
                || !(1..=MAX_SIDE).contains(&reply.height)))
    {
        return Err(SYSCALL_ERR_INVALID_STATE);
    }
    Ok(reply)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(request: &Request, data: &[u8]) -> Result<Reply, isize> {
    exchange_with(request, data, |request, data, reply| unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                request as *const Request as u64,
                reply as *mut Reply as u64,
                data.as_ptr() as u64,
                data.len() as u64,
                0,
            ],
        ) as isize
    })
}
