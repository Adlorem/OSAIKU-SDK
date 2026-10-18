use osaiku_runtime::session::*;
use std::{
    alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout},
    cell::Cell,
    rc::Rc,
};

// Real aligned storage supplies the lifetime contract of MAP without invoking
// native traps on the host. No references to it exist while SessionPage owns it.
struct Backing(*mut u8);
impl Backing {
    fn new() -> Self {
        let layout = Layout::from_size_align(4096, 4096).unwrap();
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        Self(ptr)
    }
    fn receipt(&self, request: &ProcessSessionMemoryRequest) -> ProcessSessionMemoryReceipt {
        let mut receipt = ProcessSessionMemoryReceipt {
            abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
            status: PROCESS_SESSION_MEMORY_STATUS_OK,
            decision: PROCESS_SESSION_MEMORY_DECISION_MAPPED,
            memory_kind: PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE,
            result_flags: PROCESS_SESSION_MEMORY_RESULT_MAPPED_FLAGS,
            request_id: request.request_id,
            request_hash: request.request_hash,
            session_key: request.session_key,
            mapping_id: 0x71,
            mapping_generation: 1,
            user_virtual_address: self.0 as u64,
            mapped_byte_len: 4096,
            owner_process_id: 8,
            address_space_generation: 1,
            committed_frame_count: 1,
            quota_frame_count: 32,
            available_frame_count: 512,
            protected_reserve_frame_count: 64,
            transition_generation: 1,
            ..ProcessSessionMemoryReceipt::EMPTY
        };
        receipt.mapping_hash = compute_process_session_memory_mapping_hash(
            8,
            1,
            request.session_key,
            0x71,
            1,
            self.0 as u64,
            4096,
        );
        receipt.record_hash = compute_process_session_memory_receipt_hash(&receipt);
        assert!(receipt.matches_request(request, 8));
        receipt
    }
    // Call only before adoption or after the unique owner has been consumed.
    unsafe fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0, 4096) }
    }
}
impl Drop for Backing {
    fn drop(&mut self) {
        unsafe { dealloc(self.0, Layout::from_size_align(4096, 4096).unwrap()) };
    }
}

struct Tracked {
    value: u64,
    drops: Rc<Cell<usize>>,
}
impl Drop for Tracked {
    fn drop(&mut self) {
        assert_eq!(self.value, 99, "value must be dropped before scrubbing");
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn retirement_ends_value_lifetime_and_preserves_release_identity() {
    let backing = Backing::new();
    let request = ProcessSessionMemoryRequest::map(1, 0x51, size_of::<Tracked>()).unwrap();
    let receipt = backing.receipt(&request);
    let drops = Rc::new(Cell::new(0));
    let mut page = unsafe {
        SessionPage::adopt(
            &request,
            &receipt,
            0,
            8,
            Tracked {
                value: 42,
                drops: drops.clone(),
            },
        )
    }
    .unwrap();
    assert_eq!(page.value().value, 42);
    page.value_mut().value = 99;
    assert_eq!(page.mapping(), &receipt);
    assert_eq!(drops.get(), 0);
    let mapping = page.retire();
    assert_eq!(drops.get(), 1);
    assert_eq!(mapping, receipt);
    assert!(unsafe { backing.bytes() }.iter().all(|b| *b == 0));
    let release = ProcessSessionMemoryRequest::release(2, &mapping).unwrap();
    assert_eq!(release.expected_mapping_id, receipt.mapping_id);
    assert_eq!(
        release.expected_mapping_generation,
        receipt.mapping_generation
    );
    assert_eq!(release.expected_mapping_hash, receipt.mapping_hash);
}

#[test]
fn drop_scrubs_the_full_page_without_freeing_the_backing_allocation() {
    let backing = Backing::new();
    let request = ProcessSessionMemoryRequest::map(1, 0x51, 4096).unwrap();
    let receipt = backing.receipt(&request);
    let page = unsafe { SessionPage::adopt(&request, &receipt, 0, 8, [0xa5u8; 4096]) }.unwrap();
    assert_eq!(page.value()[4095], 0xa5);
    drop(page);
    assert!(unsafe { backing.bytes() }.iter().all(|b| *b == 0));
    // Backing::drop still deallocates the live mapping; the owner did no syscall.
}

#[test]
fn wrong_status_owner_request_or_hash_never_initializes_the_page() {
    let backing = Backing::new();
    let request = ProcessSessionMemoryRequest::map(1, 0x51, 8).unwrap();
    let receipt = backing.receipt(&request);
    for case in 0..4 {
        let mut actual = receipt;
        let mut status = 0;
        let mut owner = 8;
        match case {
            0 => status = -1,
            1 => owner = 9,
            2 => {
                actual.request_id += 1;
                actual.record_hash = compute_process_session_memory_receipt_hash(&actual);
            }
            _ => actual.record_hash ^= 1,
        }
        let result = unsafe { SessionPage::adopt(&request, &actual, status, owner, 99u64) };
        assert!(matches!(result, Err(SessionPageError::InvalidMapping)));
        assert!(unsafe { backing.bytes() }.iter().all(|b| *b == 0));
    }
}

#[test]
fn dirty_page_is_refused_unchanged_and_unadopted_value_is_dropped() {
    let backing = Backing::new();
    let request = ProcessSessionMemoryRequest::map(1, 0x51, size_of::<Tracked>()).unwrap();
    let receipt = backing.receipt(&request);
    unsafe {
        backing.0.add(4095).write(1);
    }
    let drops = Rc::new(Cell::new(0));
    let result = unsafe {
        SessionPage::adopt(
            &request,
            &receipt,
            0,
            8,
            Tracked {
                value: 99,
                drops: drops.clone(),
            },
        )
    };
    assert!(matches!(result, Err(SessionPageError::NotZeroed)));
    assert_eq!(drops.get(), 1);
    let bytes = unsafe { backing.bytes() };
    assert!(bytes[..4095].iter().all(|b| *b == 0));
    assert_eq!(bytes[4095], 1);
}

#[test]
fn layout_and_requested_capacity_are_checked_before_initialization() {
    let backing = Backing::new();
    let request = ProcessSessionMemoryRequest::map(1, 0x51, 1).unwrap();
    let receipt = backing.receipt(&request);
    assert!(matches!(
        unsafe { SessionPage::adopt(&request, &receipt, 0, 8, ()) },
        Err(SessionPageError::InvalidLayout)
    ));
    assert!(matches!(
        unsafe { SessionPage::adopt(&request, &receipt, 0, 8, [0u8; 4097]) },
        Err(SessionPageError::InvalidLayout)
    ));
    assert!(matches!(
        unsafe { SessionPage::adopt(&request, &receipt, 0, 8, 42u64) },
        Err(SessionPageError::InvalidMapping)
    ));
    assert!(unsafe { backing.bytes() }.iter().all(|b| *b == 0));
}
