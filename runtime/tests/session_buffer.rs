use osaiku_runtime::session::*;
use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};

struct Backing {
    pointer: *mut u8,
    layout: Layout,
}
impl Backing {
    fn new(bytes: usize) -> Self {
        let layout = Layout::from_size_align(bytes, 4096).unwrap();
        let pointer = unsafe { alloc_zeroed(layout) };
        if pointer.is_null() {
            handle_alloc_error(layout);
        }
        Self { pointer, layout }
    }
    fn receipt(&self, request: &ProcessSessionMemoryRequest) -> ProcessSessionMemoryReceipt {
        let mut receipt = ProcessSessionMemoryReceipt {
            abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
            status: PROCESS_SESSION_MEMORY_STATUS_OK,
            decision: PROCESS_SESSION_MEMORY_DECISION_MAPPED,
            memory_kind: request.memory_kind,
            result_flags: PROCESS_SESSION_MEMORY_RESULT_MAPPED_FLAGS,
            request_id: request.request_id,
            request_hash: request.request_hash,
            session_key: request.session_key,
            mapping_id: 77,
            mapping_generation: 1,
            user_virtual_address: self.pointer as u64,
            mapped_byte_len: self.layout.size() as u64,
            owner_process_id: 3,
            address_space_generation: 1,
            committed_frame_count: self.layout.size() as u64 / 4096 + 1,
            quota_frame_count: 32768,
            transition_generation: 1,
            ..ProcessSessionMemoryReceipt::EMPTY
        };
        receipt.mapping_hash = compute_process_session_memory_mapping_hash(
            3,
            1,
            request.session_key,
            77,
            1,
            receipt.user_virtual_address,
            receipt.mapped_byte_len,
        );
        receipt.record_hash = compute_process_session_memory_receipt_hash(&receipt);
        receipt
    }
}
impl Drop for Backing {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.pointer, self.layout);
        }
    }
}

#[test]
fn owns_and_scrubs_every_page_and_preserves_release_kind() {
    let backing = Backing::new(3 * 1024 * 1024 + 4096);
    let request = ProcessSessionMemoryRequest::map_buffer(1, 77, 3 * 1024 * 1024 + 1).unwrap();
    let receipt = backing.receipt(&request);
    let mut buffer = unsafe { SessionBuffer::adopt(&request, &receipt, 0, 3) }.unwrap();
    assert_eq!(buffer.bytes().len(), backing.layout.size());
    buffer.bytes_mut().fill(0x73);
    let mapping = buffer.retire();
    assert!(
        unsafe { std::slice::from_raw_parts(backing.pointer, backing.layout.size()) }
            .iter()
            .all(|v| *v == 0)
    );
    let release = ProcessSessionMemoryRequest::release(2, &mapping).unwrap();
    assert_eq!(
        release.memory_kind,
        PROCESS_SESSION_MEMORY_KIND_COMPOSITOR_BUFFER
    );
    assert_eq!(release.expected_mapping_hash, receipt.mapping_hash);
}

#[test]
fn dirty_tail_refused_without_scrubbing() {
    let backing = Backing::new(3 * 4096);
    unsafe {
        backing.pointer.add(backing.layout.size() - 1).write(9);
    }
    let request = ProcessSessionMemoryRequest::map_buffer(1, 77, backing.layout.size()).unwrap();
    let receipt = backing.receipt(&request);
    assert!(matches!(
        unsafe { SessionBuffer::adopt(&request, &receipt, 0, 3) },
        Err(SessionPageError::NotZeroed)
    ));
    assert_eq!(
        unsafe { backing.pointer.add(backing.layout.size() - 1).read() },
        9
    );
}

#[test]
fn rejects_wrong_owner_status_kind_and_oversized_receipt() {
    let backing = Backing::new(8192);
    let request = ProcessSessionMemoryRequest::map_buffer(1, 77, 4097).unwrap();
    let receipt = backing.receipt(&request);
    for (status, owner) in [(-8, 3), (0, 8)] {
        assert!(matches!(
            unsafe { SessionBuffer::adopt(&request, &receipt, status, owner) },
            Err(SessionPageError::InvalidMapping)
        ));
    }
    assert!(matches!(
        unsafe { SessionPage::adopt(&request, &receipt, 0, 3, 0u8) },
        Err(SessionPageError::InvalidMapping)
    ));
    let short = ProcessSessionMemoryRequest::map_buffer(2, 78, 1).unwrap();
    let oversized = backing.receipt(&short);
    assert!(process_session_memory_receipt_canonical(&oversized));
    assert!(!oversized.matches_request(&short, 3));
}

#[test]
fn kind_specific_limits_preserve_application_page_contract() {
    assert!(ProcessSessionMemoryRequest::map_assistant_buffer(1, 1, 0).is_none());
    assert!(ProcessSessionMemoryRequest::map_assistant_buffer(1, 1, usize::MAX).is_none());
    assert!(ProcessSessionMemoryRequest::map_assistant_buffer(
        1,
        1,
        PROCESS_SESSION_MEMORY_ASSISTANT_LIMIT as usize
    )
    .is_some());
    assert!(ProcessSessionMemoryRequest::map_assistant_buffer(
        1,
        1,
        PROCESS_SESSION_MEMORY_ASSISTANT_LIMIT as usize + 1
    )
    .is_none());
    assert!(ProcessSessionMemoryRequest::map(1, 1, 4097).is_none());
    assert!(ProcessSessionMemoryRequest::map_buffer(1, 1, 0).is_none());
    assert!(ProcessSessionMemoryRequest::map_buffer(1, 1, usize::MAX).is_none());
    assert!(ProcessSessionMemoryRequest::map_buffer(
        1,
        1,
        PROCESS_SESSION_MEMORY_BUFFER_LIMIT as usize
    )
    .is_some());
    assert!(ProcessSessionMemoryRequest::map_buffer(
        1,
        1,
        PROCESS_SESSION_MEMORY_BUFFER_LIMIT as usize + 1
    )
    .is_none());
}

#[test]
fn assistant_buffer_adoption_and_retirement_preserve_exact_kind() {
    let backing = Backing::new(5 * 1024 * 1024);
    let request =
        ProcessSessionMemoryRequest::map_assistant_buffer(1, 77, backing.layout.size()).unwrap();
    let receipt = backing.receipt(&request);
    let mut buffer = unsafe { SessionBuffer::adopt(&request, &receipt, 0, 3) }.unwrap();
    buffer.bytes_mut().fill(17);
    let mapping = buffer.retire();
    assert!(
        unsafe { std::slice::from_raw_parts(backing.pointer, backing.layout.size()) }
            .iter()
            .all(|b| *b == 0)
    );
    assert_eq!(
        ProcessSessionMemoryRequest::release(2, &mapping)
            .unwrap()
            .memory_kind,
        PROCESS_SESSION_MEMORY_KIND_ASSISTANT_BUFFER
    );
}
