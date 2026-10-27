//! Experimental immutable pixel transport, independent of the old recipe wire.
mod dirty;
mod window_state;
pub use window_state::{window_state_with, WindowState, WINDOW_STATE};
mod frame;
mod publisher;
pub(crate) use dirty::DirtyTracker;
pub use frame::{frame_offer_with, frame_request_with, frame_snapshot_with};
mod reader;
mod receiver;
mod supervisor;
pub use osaiku_ui::surface::*;
pub use publisher::{PublishStep, SurfacePublisher};
pub use reader::{ReadChunk, ReadError, SurfaceReader};
pub use receiver::{ReceiveStep, ReceivedFrame, SurfaceReceiver};
pub use supervisor::bind_server_with;
pub const QUERY: u32 = 0;
pub const BEGIN: u32 = 1;
pub const UPLOAD: u32 = 2;
pub const SEAL: u32 = 3;
pub const CANCEL: u32 = 4;
pub const READ: u32 = 5;
pub const REVOKE_SERVER: u32 = 6;
pub const BIND_SERVER: u32 = 7;
pub const METADATA: u32 = 8;
pub const ACQUIRE: u32 = 9;
pub const RELEASE: u32 = 10;
pub const QUERY_WORK: u32 = 11;
pub const GEOMETRY: u32 = 12;
pub const BEGIN_OFFER: u32 = 13;
pub const FRAME_OFFER: u32 = 14;
pub const BEGIN_FRAME: u32 = 15;
pub const FRAME_REQUEST: u32 = 16;
pub const FRAME_SNAPSHOT: u32 = 17;
pub const BEGIN_DELTA: u32 = 18;
/// Copy unchanged rows from the admitted base. offset and server_endpoint are
/// the byte offset and length (1..=MAX_TRANSFER); payload must be empty.
pub const COPY_BASE: u32 = 19;
/// Discover dirty rows while uploading. DeltaOffer.dirty must initially be empty.
pub const BEGIN_STREAM: u32 = 20;

/// Request envelope v8 opts into palette IDs with an accent byte. Record
/// layouts retain VERSION (7). The kernel also accepts v7 requests, whose
/// geometry must contain only the original palette IDs 0, 1 or 2.
pub const REQUEST_VERSION: u32 = 8;

/// Fetch authenticated geometry before laying out a frame. No surface is
/// reserved; BEGIN_OFFER rechecks the offer against the current window.
pub fn geometry_with(
    target: Target,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<GeometryOffer, PublishError> {
    let request = Request {
        version: REQUEST_VERSION,
        operation: GEOMETRY,
        handle: Handle {
            target,
            ..Handle::default()
        },
        ..Request::default()
    };
    let mut offer = GeometryOffer::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut offer as *mut GeometryOffer).cast::<u8>(),
            core::mem::size_of::<GeometryOffer>(),
        )
    };
    let mut receipt = Receipt::default();
    let code = exchange(&request, bytes, &mut receipt);
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    offer.validate().map_err(PublishError::Content)?;
    if offer.target != target || receipt.handle != request.handle || receipt.epoch != target.epoch {
        return Err(PublishError::Receipt);
    }
    Ok(offer)
}
const _: () = assert!(core::mem::size_of::<GeometryOffer>() == 80);

/// Read counters for an exact window, or a zero target for the supervisor/server
/// aggregate. These observation calls are excluded from workload counters.
pub fn work_with(
    target: Target,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<WorkSnapshot, PublishError> {
    let request = Request {
        version: VERSION,
        operation: QUERY_WORK,
        handle: Handle {
            target,
            ..Handle::default()
        },
        ..Request::default()
    };
    let mut snapshot = WorkSnapshot::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut snapshot as *mut WorkSnapshot).cast::<u8>(),
            core::mem::size_of::<WorkSnapshot>(),
        )
    };
    let mut receipt = Receipt::default();
    let code = exchange(&request, bytes, &mut receipt);
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    if snapshot.version != VERSION
        || snapshot.reserved != 0
        || snapshot.target != target
        || snapshot.epoch != receipt.epoch
        || receipt.handle != request.handle
    {
        return Err(PublishError::Receipt);
    }
    Ok(snapshot)
}
const _: () = assert!(core::mem::size_of::<WorkSnapshot>() == 16 + 32 + 11 * 8 + 48);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub version: u32,
    pub operation: u32,
    pub handle: Handle,
    pub extent: Extent,
    pub offset: u64,
    pub server_endpoint: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Receipt {
    pub handle: Handle,
    pub epoch: u64,
    pub charged_bytes: u64,
}
/// Invoke with a caller-owned payload (operation-specific cap) and writable receipt.
/// READ is reserved for the supervisor-admitted compositor. Metadata/text is
/// not a capability and no application may bind itself as the compositor.
///
/// # Safety
/// OSAIKU process only. `payload` must remain exclusively accessible for the
/// syscall, including when used as an output buffer for READ.
#[cfg(target_arch = "x86_64")]
pub unsafe fn transition(request: &Request, payload: &mut [u8], receipt: &mut Receipt) -> isize {
    transition_with(
        request,
        payload,
        receipt,
        |request, payload, receipt| unsafe {
            crate::raw::syscall(
                crate::abi::number::UI_SURFACE,
                [
                    request as *const Request as u64,
                    payload.as_mut_ptr() as u64,
                    payload.len() as u64,
                    receipt as *mut Receipt as u64,
                    0,
                ],
            ) as isize
        },
    )
}

/// Native SDK entry point policy, also injectable for host verification. Each
/// new candidate negotiates afresh (no stale device generation cache). This SDK
/// has no Mesa Painter yet, so it honestly advertises no UI_RENDERER capability.
/// GPU-09 will replace that declaration and dispatch to the negotiated Painter.
/// Legacy pixel calls remain usable ONLY after an explicit fallback response.
pub fn transition_with(
    request: &Request,
    payload: &mut [u8],
    receipt: &mut Receipt,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> isize {
    if matches!(
        request.operation,
        BEGIN | BEGIN_OFFER | BEGIN_FRAME | BEGIN_DELTA | BEGIN_STREAM
    ) {
        let selection = crate::graphics::negotiate_with(
            crate::graphics::Negotiation::gpu_default(0),
            &mut exchange,
        );
        match selection {
            Ok(r) if r.renderer == 0 && r.fallback != crate::graphics::fallback::NONE => (),
            Err(PublishError::Transport(code)) => return code,
            _ => return crate::abi::SYSCALL_ERR_INVALID_STATE,
        }
    }
    exchange(request, payload, receipt)
}
const _: () = assert!(core::mem::size_of::<Publication>() <= MAX_METADATA);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishError {
    Content(Refusal),
    Transport(isize),
    Receipt,
    /// Cancellation was refused; retain this handle to retry cleanup or let
    /// window/process teardown reclaim it. Never silently lose a live candidate.
    Cleanup {
        handle: Handle,
        code: isize,
    },
}

/// Rasterize and atomically seal one revision using caller-owned scratch.
/// `exchange` supplies the transport; it must obey the BEGIN/UPLOAD/SEAL
/// protocol. An error after BEGIN triggers one bounded cancellation attempt.
/// The caller must retain the old visible revision until SEAL succeeds.
/// This convenience helper runs the entire frame synchronously. Use
/// SurfacePublisher to schedule input/other work between bounded steps.
/// There are no hidden retries or waits for a refused transition.
pub fn publish_with<L: AsRef<osaiku_ui::layout::Layout>>(
    scene: &osaiku_ui::paint::Scene<'_, L>,
    target: Target,
    revision: u64,
    materials: &[Material],
    metadata: &mut Publication,
    band: &mut [u8],
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<Handle, PublishError> {
    // Preserve the legacy helper's larger-scratch support; the cooperative
    // state machine itself accepts at most one transfer-sized scratch region.
    let usable = band.len().min(MAX_TRANSFER);
    let mut producer = SurfacePublisher::new(
        scene,
        target,
        revision,
        materials,
        metadata,
        &mut band[..usable],
    )?;
    let result = loop {
        match producer.step_with(&mut exchange) {
            Ok(PublishStep::Sealed { handle }) => break Ok(handle),
            Ok(_) => {}
            Err(error) => {
                if producer.handle().is_some() {
                    if let Err(cleanup) = producer.cancel_with(&mut exchange) {
                        break Err(cleanup);
                    }
                }
                break Err(error);
            }
        }
    };
    drop(producer);
    band.fill(0);
    result
}

// Keep byte views above free of uninitialized alignment padding.
const _: () = assert!(
    core::mem::size_of::<Publication>()
        == 8 + 48
            + 8
            + 16
            + 16
            + MAX_DAMAGE * 16
            + MAX_MATERIALS * 24
            + MAX_ELEMENTS * 40
            + MAX_ELEMENTS * 24
            + MAX_LABELS
);
