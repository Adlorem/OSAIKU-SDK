use super::*;
use crate::process::ProcessEndpoint;

/// Bind a named server instance to the currently observed transport epoch.
/// Kernel policy requires the supervisor as caller and an admitted live server.
/// Two bounded exchanges, no retry or implicit revocation. A racing retirement
/// or existing binding is returned to the caller for lifecycle handling.
pub fn bind_server_with(
    server: ProcessEndpoint,
    mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
) -> Result<u64, PublishError> {
    if server.process_id == crate::process::ProcessId::KERNEL || !server.names_instance() {
        return Err(PublishError::Content(Refusal::Identity));
    }
    let mut request = Request {
        version: VERSION,
        operation: QUERY,
        ..Request::default()
    };
    let mut receipt = Receipt::default();
    let result = exchange(&request, &mut [], &mut receipt);
    if result != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(result));
    }
    if receipt.epoch == 0 || receipt.handle != request.handle {
        return Err(PublishError::Receipt);
    }
    let epoch = receipt.epoch;
    request.operation = BIND_SERVER;
    request.handle.target.epoch = epoch;
    request.server_endpoint = server.encode();
    receipt = Receipt::default();
    let result = exchange(&request, &mut [], &mut receipt);
    if result != crate::abi::SYSCALL_OK {
        return Err(PublishError::Transport(result));
    }
    if receipt.epoch != epoch || receipt.handle != request.handle {
        return Err(PublishError::Receipt);
    }
    Ok(epoch)
}
