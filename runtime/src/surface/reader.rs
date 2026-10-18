//! Incremental server-side reads. Each step performs at most one bounded copy;
//! callers can service input/cancellation between steps instead of blocking on
//! an entire window. A completed read is not an acknowledgement of scanout.
use super::*;
use crate::abi::{SYSCALL_ERR_INVALID_STATE, SYSCALL_OK};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Content(Refusal),
    Transport(isize),
    Receipt,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadChunk {
    pub offset: u64,
    pub length: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Leased,
    Ready,
    Complete,
    Released,
}

/// Keeps one publication leased while a window server copies it into private
/// composition storage. Keep the metadata and pixels private until every step
/// and finish succeeds. A failure retains the reader so cancel can release it;
/// dropping the Rust value does not perform a syscall or release a live lease.
#[must_use = "finish or cancel the surface read to release its server lease"]
pub struct SurfaceReader {
    handle: Handle,
    phase: Phase,
    offset: u64,
    bytes: u64,
    expected_request: Option<RenderRequest>,
    expected_extent: Option<Extent>,
}
impl SurfaceReader {
    pub fn new(handle: Handle) -> Result<Self, ReadError> {
        if !handle.target.valid() || handle.serial == 0 || handle.revision == 0 {
            return Err(ReadError::Content(Refusal::Identity));
        }
        Ok(Self {
            handle,
            phase: Phase::New,
            offset: 0,
            bytes: 0,
            expected_request: None,
            expected_extent: None,
        })
    }
    /// Read only a publication bound to this exact broker transaction. Obtain
    /// `expected` from authenticated transaction state, never from app labels.
    /// Acquiring the lease precedes checking the retained binding, so a client
    /// cannot replace the pixels between that check and the final read.
    pub fn for_frame(handle: Handle, expected: RenderRequest) -> Result<Self, ReadError> {
        expected.validate().map_err(ReadError::Content)?;
        if expected.revision != handle.revision {
            return Err(ReadError::Content(Refusal::Identity));
        }
        let mut reader = Self::new(handle)?;
        reader.expected_request = Some(expected);
        Ok(reader)
    }
    pub fn handle(&self) -> Handle {
        self.handle
    }
    /// Consume a kernel-returned descriptor, checking its request under the
    /// lease and the metadata extent against the saved content geometry.
    /// The caller still checks relevance to its broker transaction and current
    /// scene; this descriptor is not a visibility or input authorization.
    pub fn from_snapshot(snapshot: FrameSnapshot) -> Result<Self, ReadError> {
        snapshot.validate().map_err(ReadError::Content)?;
        let mut reader = Self::for_frame(snapshot.handle, snapshot.frame.request)?;
        reader.expected_extent = Some(
            snapshot
                .frame
                .geometry
                .content_extent()
                .map_err(ReadError::Content)?,
        );
        Ok(reader)
    }
    /// Acquire, verify the request binding for for_frame readers, then fetch
    /// immutable metadata: at most three syscalls (two for unbound readers).
    /// Refusals can be retried without acquiring another lease. No pixel reads
    /// are allowed until all checks succeed. Preparation failure clears metadata;
    /// an out-of-sequence call leaves it untouched. Retain this reader for explicit
    /// retry/cancellation, including bad receipts.
    pub fn start_with(
        &mut self,
        metadata: &mut Publication,
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), ReadError> {
        if !matches!(self.phase, Phase::New | Phase::Leased) {
            return Err(ReadError::Transport(SYSCALL_ERR_INVALID_STATE));
        }
        clear_metadata(metadata);
        let result = self.start_inner(metadata, &mut exchange);
        if result.is_err() {
            clear_metadata(metadata);
        }
        result
    }
    fn start_inner(
        &mut self,
        metadata: &mut Publication,
        exchange: &mut impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), ReadError> {
        if self.phase == Phase::New {
            // Track acquisition even when its returned receipt is malformed,
            // so the exact requested handle remains available for cancellation.
            let request = self.request(ACQUIRE);
            let mut receipt = Receipt::default();
            let code = exchange(&request, &mut [], &mut receipt);
            if code != SYSCALL_OK {
                return Err(ReadError::Transport(code));
            }
            self.phase = Phase::Leased;
            self.check_receipt(&receipt)?;
        }
        if let Some(expected) = self.expected_request {
            let mut binding = RenderRequest::default();
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    (&mut binding as *mut RenderRequest).cast::<u8>(),
                    core::mem::size_of::<RenderRequest>(),
                )
            };
            self.exchange(FRAME_REQUEST, bytes, exchange)?;
            binding.validate().map_err(ReadError::Content)?;
            if binding != expected {
                return Err(ReadError::Content(Refusal::Stale));
            }
        }
        // repr(C) integer-only record; super's size assertion checks padding.
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                (metadata as *mut Publication).cast::<u8>(),
                core::mem::size_of::<Publication>(),
            )
        };
        self.exchange(METADATA, bytes, exchange)?;
        metadata.validate().map_err(ReadError::Content)?;
        if metadata.handle != self.handle {
            return Err(ReadError::Receipt);
        }
        if self
            .expected_extent
            .is_some_and(|extent| extent != metadata.extent)
        {
            return Err(ReadError::Content(Refusal::Bounds));
        }
        self.bytes = metadata.extent.bytes().map_err(ReadError::Content)?;
        self.phase = Phase::Ready;
        Ok(())
    }
    /// Read one pixel-aligned chunk, no more than MAX_TRANSFER. On refusal the
    /// cursor stays put and output scratch is cleared; retry or cancel explicitly.
    pub fn next_with(
        &mut self,
        scratch: &mut [u8],
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<Option<ReadChunk>, ReadError> {
        if self.phase == Phase::Complete {
            return Ok(None);
        }
        if self.phase != Phase::Ready {
            return Err(ReadError::Transport(SYSCALL_ERR_INVALID_STATE));
        }
        let available = scratch.len() / 4 * 4;
        if available == 0 || scratch.len() > MAX_TRANSFER {
            return Err(ReadError::Content(Refusal::Bounds));
        }
        let length = (self.bytes - self.offset).min(available as u64) as usize;
        scratch.fill(0);
        if let Err(error) = self.exchange(READ, &mut scratch[..length], &mut exchange) {
            scratch.fill(0);
            return Err(error);
        }
        let chunk = ReadChunk {
            offset: self.offset,
            length,
        };
        self.offset += length as u64;
        if self.offset == self.bytes {
            self.phase = Phase::Complete;
        }
        Ok(Some(chunk))
    }
    pub fn finish_with(
        &mut self,
        exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), ReadError> {
        if self.phase != Phase::Complete {
            return Err(ReadError::Content(Refusal::Incomplete));
        }
        self.cancel_with(exchange)
    }
    /// Cancellation/release failure leaves this handle available for retry.
    /// Window/process teardown and supervisor revocation also revoke the lease.
    pub fn cancel_with(
        &mut self,
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), ReadError> {
        if !matches!(self.phase, Phase::New | Phase::Released) {
            self.exchange(RELEASE, &mut [], &mut exchange)?;
        }
        self.phase = Phase::Released;
        Ok(())
    }
    fn request(&self, operation: u32) -> Request {
        Request {
            version: VERSION,
            operation,
            handle: self.handle,
            offset: if operation == READ { self.offset } else { 0 },
            ..Request::default()
        }
    }
    fn check_receipt(&self, receipt: &Receipt) -> Result<(), ReadError> {
        if receipt.handle != self.handle || receipt.epoch != self.handle.target.epoch {
            Err(ReadError::Receipt)
        } else {
            Ok(())
        }
    }
    fn exchange(
        &self,
        operation: u32,
        bytes: &mut [u8],
        exchange: &mut impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<(), ReadError> {
        let mut receipt = Receipt::default();
        let code = exchange(&self.request(operation), bytes, &mut receipt);
        if code != SYSCALL_OK {
            return Err(ReadError::Transport(code));
        }
        self.check_receipt(&receipt)
    }
}

fn clear_metadata(metadata: &mut Publication) {
    // All fields are integer/byte records; zero is an invalid, inert publication.
    // Write in place rather than materializing a large temporary on a user stack.
    unsafe { core::ptr::write_bytes(metadata as *mut Publication, 0, 1) };
}
