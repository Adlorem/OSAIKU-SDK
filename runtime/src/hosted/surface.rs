use super::WindowSession;
use crate::{abi::SYSCALL_OK, process::*, surface::*, window::UiDocumentWindowPresentRequest};

impl WindowSession {
    /// Resolve a render request against this retained session, then fetch the
    /// current epoch and armed frame offer (at most two transport calls).
    /// `app_id` and `owner` come from the kernel launch context;
    /// authenticate the IPC sender before calling. No allocation, reservation,
    /// retry, session mutation or broker completion occurs here. A subsequent
    /// BEGIN_FRAME/SEAL revalidates any intervening geometry/request changes.
    pub fn frame_offer_with(
        &self,
        request: &UiDocumentWindowPresentRequest,
        app_id: u64,
        owner: ProcessEndpoint,
        mut exchange: impl FnMut(&Request, &mut [u8], &mut Receipt) -> isize,
    ) -> Result<FrameOffer, PublishError> {
        if app_id == 0
            || request.expected_app_id != app_id
            || owner.process_id == ProcessId::KERNEL
            || !owner.names_instance()
            || !self.matches_present(request)
        {
            return Err(PublishError::Content(Refusal::Identity));
        }
        let query = Request {
            version: VERSION,
            operation: QUERY,
            ..Request::default()
        };
        let mut receipt = Receipt::default();
        let code = exchange(&query, &mut [], &mut receipt);
        if code != SYSCALL_OK {
            return Err(PublishError::Transport(code));
        }
        if receipt.epoch == 0 || receipt.handle != query.handle {
            return Err(PublishError::Receipt);
        }
        frame_offer_with(
            Target {
                owner: owner.encode(),
                window: request.expected_window_id,
                incarnation: request.expected_window_generation,
                epoch: receipt.epoch,
            },
            request.request_id,
            request.request_hash,
            exchange,
        )
    }
}
