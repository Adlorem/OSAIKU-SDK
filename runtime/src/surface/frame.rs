use super::*;

/// Fetch only the currently armed transaction named by the application's
/// authenticated incoming request. No state is consumed or reserved.
pub fn frame_offer_with(
    target: Target,
    request_id: u64,
    request_hash: u64,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<FrameOffer, PublishError> {
    let request = Request {
        version: VERSION,
        operation: FRAME_OFFER,
        handle: Handle {
            target,
            revision: request_id,
            serial: 0,
        },
        offset: request_hash,
        ..Request::default()
    };
    let mut offer = FrameOffer::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut offer as *mut FrameOffer).cast::<u8>(),
            core::mem::size_of::<FrameOffer>(),
        )
    };
    let mut receipt = Receipt::default();
    let code = exchange(&request, bytes, &mut receipt);
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    offer.validate().map_err(PublishError::Content)?;
    if offer.geometry.target != target
        || offer.request.request_id != request_id
        || offer.request.request_hash != request_hash
        || receipt.handle != request.handle
        || receipt.epoch != target.epoch
    {
        return Err(PublishError::Receipt);
    }
    Ok(offer)
}

/// Read the kernel-retained request binding of a sealed publication. Only its
/// owner or the admitted server can observe it. Unbound surfaces are refused.
pub fn frame_request_with(
    handle: Handle,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<RenderRequest, PublishError> {
    let request = Request {
        version: VERSION,
        operation: FRAME_REQUEST,
        handle,
        ..Request::default()
    };
    let mut binding = RenderRequest::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut binding as *mut RenderRequest).cast::<u8>(),
            core::mem::size_of::<RenderRequest>(),
        )
    };
    let mut receipt = Receipt::default();
    let code = exchange(&request, bytes, &mut receipt);
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    binding.validate().map_err(PublishError::Content)?;
    if binding.revision != handle.revision
        || receipt.handle != handle
        || receipt.epoch != handle.target.epoch
    {
        return Err(PublishError::Receipt);
    }
    Ok(binding)
}
const _: () = assert!(core::mem::size_of::<FrameOffer>() == 112);
const _: () = assert!(core::mem::size_of::<RenderRequest>() == 32);

/// Find the currently sealed bound publication of an exact window tenancy.
/// Only its owner or the admitted server can query it. This does not acquire a
/// lease: a later SurfaceReader::from_snapshot may refuse a superseded handle.
pub fn frame_snapshot_with(
    target: Target,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<FrameSnapshot, PublishError> {
    let request = Request {
        version: VERSION,
        operation: FRAME_SNAPSHOT,
        handle: Handle {
            target,
            ..Handle::default()
        },
        ..Request::default()
    };
    let mut snapshot = FrameSnapshot::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut snapshot as *mut FrameSnapshot).cast::<u8>(),
            core::mem::size_of::<FrameSnapshot>(),
        )
    };
    let mut receipt = Receipt::default();
    let code = exchange(&request, bytes, &mut receipt);
    if code != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(code));
    }
    snapshot.validate().map_err(PublishError::Content)?;
    if snapshot.handle.target != target
        || receipt.handle != snapshot.handle
        || receipt.epoch != target.epoch
    {
        return Err(PublishError::Receipt);
    }
    Ok(snapshot)
}
const _: () = assert!(core::mem::size_of::<FrameSnapshot>() == 160);
