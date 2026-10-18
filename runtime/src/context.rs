//! Owner-authorized, bounded image/semantic snapshot transport for agent clients.
pub use osaiku_ui::context::*;
use osaiku_ui::surface::{Rect, Refusal, Target};
pub mod leases;
pub mod system;
pub mod text;
pub const OPEN: u32 = 0;
pub const SEMANTICS: u32 = 1;
pub const PIXELS: u32 = 2;
pub const RELEASE: u32 = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub version: u32,
    pub operation: u32,
    pub token: u64,
    pub target: Target,
    pub revision: u64,
    pub recipient: u64,
    pub crop: Rect,
    pub disclosure: u32,
    pub reserved: u32,
    pub offset: u64,
}
impl Request {
    pub fn open(
        target: Target,
        revision: u64,
        recipient: u64,
        crop: Rect,
        disclosure: u32,
    ) -> Self {
        Self {
            version: VERSION,
            operation: OPEN,
            target,
            revision,
            recipient,
            crop,
            disclosure,
            ..Self::default()
        }
    }
    pub fn token(operation: u32, token: u64, offset: u64) -> Self {
        Self {
            version: VERSION,
            operation,
            token,
            offset,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.version != VERSION || self.reserved != 0 {
            return Err(Refusal::Shape);
        }
        if self.operation == OPEN {
            if !self.target.valid()
                || self.recipient as u32 == 0
                || self.disclosure > CONTENT
                || self.token != 0
                || self.offset != 0
                || self.crop.width == 0
                || self.crop.height == 0
            {
                return Err(Refusal::Identity);
            }
        } else if !matches!(self.operation, SEMANTICS | PIXELS | RELEASE)
            || self.token == 0
            || *self
                != Self::token(
                    self.operation,
                    self.token,
                    if self.operation == PIXELS {
                        self.offset
                    } else {
                        0
                    },
                )
        {
            return Err(Refusal::Shape);
        }
        Ok(())
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Transport(isize),
    Invalid(Refusal),
    Buffer,
    Incomplete,
}
fn bytes<T>(value: &mut T) -> &mut [u8] {
    // Used only with the padding-free integer wire records asserted below.
    unsafe { core::slice::from_raw_parts_mut((value as *mut T).cast(), core::mem::size_of::<T>()) }
}
pub fn open_with(
    request: &Request,
    mut exchange: impl FnMut(&Request, &mut [u8]) -> isize,
) -> Result<Info, Error> {
    request.validate().map_err(Error::Invalid)?;
    if request.operation != OPEN {
        return Err(Error::Invalid(Refusal::Shape));
    }
    let mut info = Info::default();
    let code = exchange(request, bytes(&mut info));
    if code != 0 {
        return Err(Error::Transport(code));
    }
    info.validate().map_err(Error::Invalid)?;
    if info.handle.target != request.target
        || (request.revision != 0 && info.handle.revision != request.revision)
        || info.crop != request.crop
        || info.recipient != request.recipient
        || info.disclosure != request.disclosure
    {
        return Err(Error::Invalid(Refusal::Identity));
    }
    Ok(info)
}
/// Fetch the frozen semantics first. Receiving a token via IPC grants nothing:
/// the kernel additionally authenticates its exact recipient endpoint.
pub fn semantics_with(
    token: u64,
    output: &mut Snapshot,
    mut exchange: impl FnMut(&Request, &mut [u8]) -> isize,
) -> Result<(), Error> {
    let code = exchange(&Request::token(SEMANTICS, token, 0), bytes(output));
    if code != 0 {
        bytes(output).fill(0);
        return Err(Error::Transport(code));
    }
    let result = output.validate().and_then(|()| {
        if output.info.token == token {
            Ok(())
        } else {
            Err(Refusal::Identity)
        }
    });
    if let Err(error) = result {
        bytes(output).fill(0);
        return Err(Error::Invalid(error));
    }
    Ok(())
}
/// Incremental receiver. Do not expose partial pixels to an agent: only finish
/// after all bytes and matching semantics have been received successfully.
pub struct Receiver<'a> {
    snapshot: &'a Snapshot,
    pixels: &'a mut [u8],
    next: u64,
    canceled: bool,
    released: bool,
}
impl<'a> Receiver<'a> {
    pub fn new(snapshot: &'a Snapshot, pixels: &'a mut [u8]) -> Result<Self, Error> {
        snapshot.validate().map_err(Error::Invalid)?;
        if pixels.len() as u64 != snapshot.info.extent.bytes().map_err(Error::Invalid)? {
            return Err(Error::Buffer);
        }
        Ok(Self {
            snapshot,
            pixels,
            next: 0,
            canceled: false,
            released: false,
        })
    }
    pub fn info(&self) -> Info {
        self.snapshot.info
    }
    pub fn next_with(
        &mut self,
        mut exchange: impl FnMut(&Request, &mut [u8]) -> isize,
    ) -> Result<bool, Error> {
        if self.canceled {
            return Err(Error::Incomplete);
        }
        let total = self.pixels.len();
        if self.next as usize == total {
            return Ok(true);
        }
        let end = (self.next as usize + MAX_TRANSFER).min(total);
        let code = exchange(
            &Request::token(PIXELS, self.snapshot.info.token, self.next),
            &mut self.pixels[self.next as usize..end],
        );
        if code != 0 {
            self.pixels.fill(0);
            self.next = 0;
            return Err(Error::Transport(code));
        }
        self.next = end as u64;
        Ok(end == total)
    }
    pub fn finish(&self) -> Result<Context<'_>, Error> {
        if self.canceled || self.pixels.len() as u64 != self.next {
            return Err(Error::Incomplete);
        }
        Ok(Context {
            snapshot: self.snapshot,
            pixels: self.pixels,
        })
    }
    /// Revalidate capture authority immediately before handing a completed image
    /// to a consumer. Use this boundary for model input; `finish` only checks
    /// local transfer completeness and cannot detect a revoked kernel token.
    pub fn finish_with(
        &mut self,
        scratch: &mut Snapshot,
        exchange: impl FnMut(&Request, &mut [u8]) -> isize,
    ) -> Result<Context<'_>, Error> {
        if self.canceled || self.pixels.len() as u64 != self.next {
            return Err(Error::Incomplete);
        }
        let result = semantics_with(self.snapshot.info.token, scratch, exchange).and_then(|()| {
            if scratch == self.snapshot {
                Ok(())
            } else {
                Err(Error::Invalid(Refusal::Stale))
            }
        });
        bytes(scratch).fill(0);
        if let Err(error) = result {
            self.pixels.fill(0);
            self.next = 0;
            self.canceled = true;
            return Err(error);
        }
        self.finish()
    }
    pub fn cancel_with(
        &mut self,
        exchange: impl FnMut(&Request, &mut [u8]) -> isize,
    ) -> Result<(), Error> {
        self.pixels.fill(0);
        self.next = 0;
        self.canceled = true;
        if self.released {
            return Ok(());
        }
        release_with(self.snapshot.info.token, exchange)?;
        self.released = true;
        Ok(())
    }
}
impl Drop for Receiver<'_> {
    fn drop(&mut self) {
        if self.next != self.pixels.len() as u64 {
            self.pixels.fill(0);
        }
    }
}
pub struct Context<'a> {
    pub snapshot: &'a Snapshot,
    pub pixels: &'a [u8],
}
pub fn release_with(
    token: u64,
    mut exchange: impl FnMut(&Request, &mut [u8]) -> isize,
) -> Result<(), Error> {
    match exchange(&Request::token(RELEASE, token, 0), &mut []) {
        0 => Ok(()),
        code => Err(Error::Transport(code)),
    }
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(request: &Request, output: &mut [u8]) -> isize {
    unsafe {
        crate::raw::syscall(
            crate::abi::number::UI_CONTEXT,
            [
                request as *const _ as u64,
                output.as_mut_ptr() as u64,
                output.len() as u64,
                0,
                0,
            ],
        ) as isize
    }
}
const _: () = assert!(core::mem::size_of::<Request>() == 96);
const _: () = assert!(core::mem::size_of::<Info>() == 136);
const _: () = assert!(core::mem::size_of::<Element>() == 64);
const _: () = assert!(core::mem::size_of::<Point>() == 16);
const _: () = assert!(core::mem::size_of::<Snapshot>() == 136 + 16 + 128 * 64 + 8192 + 120 * 16);
