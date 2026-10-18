//! Caller-owned session memory contract: one zeroed, writable, non-executable
//! page per admitted mapping. The kernel selects the virtual address and owns
//! quotas, request ordering, mapping identity and release/scrubbing policy.
//! Hashes bind metadata; they do not prove ownership or make an address safe to
//! dereference. The native transition is unsafe because RELEASE unmaps memory.

mod buffer;
mod page;
pub use buffer::SessionBuffer;
mod request;
pub use page::{SessionPage, SessionPageError};

pub(crate) fn transition_arguments(
    request: &ProcessSessionMemoryRequest,
    receipt: &mut ProcessSessionMemoryReceipt,
) -> [u64; 5] {
    [
        request as *const ProcessSessionMemoryRequest as u64,
        receipt as *mut ProcessSessionMemoryReceipt as u64,
        0,
        0,
        0,
    ]
}

pub const PROCESS_SESSION_MEMORY_ABI_VERSION: u32 = 1;
pub const PROCESS_SESSION_MEMORY_REQUEST_SIZE: usize = 96;
pub const PROCESS_SESSION_MEMORY_RECEIPT_SIZE: usize = 160;
pub const PROCESS_SESSION_MEMORY_PAGE_SIZE: u32 = 4096;

pub const PROCESS_SESSION_MEMORY_OPERATION_MAP: u8 = 1;
pub const PROCESS_SESSION_MEMORY_OPERATION_RELEASE: u8 = 2;
pub const PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE: u8 = 1;
/// Compositor-private contiguous storage; only the admitted compositor role may
/// request it. Applications keep the original one-page contract.
pub const PROCESS_SESSION_MEMORY_KIND_COMPOSITOR_BUFFER: u8 = 2;
pub const PROCESS_SESSION_MEMORY_BUFFER_LIMIT: u32 = 64 * 1024 * 1024;
/// Private tokenizer/controller storage; admitted only for the host assistant service.
/// This wire kind grants no application allocation authority.
pub const PROCESS_SESSION_MEMORY_KIND_ASSISTANT_BUFFER: u8 = 3;
pub const PROCESS_SESSION_MEMORY_ASSISTANT_LIMIT: u32 = 8 * 1024 * 1024;

pub const fn session_memory_kind_limit(kind: u8) -> u32 {
    match kind {
        PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE => PROCESS_SESSION_MEMORY_PAGE_SIZE,
        PROCESS_SESSION_MEMORY_KIND_COMPOSITOR_BUFFER => PROCESS_SESSION_MEMORY_BUFFER_LIMIT,
        PROCESS_SESSION_MEMORY_KIND_ASSISTANT_BUFFER => PROCESS_SESSION_MEMORY_ASSISTANT_LIMIT,
        _ => 0,
    }
}

pub const PROCESS_SESSION_MEMORY_REQUEST_CALLER_SCOPED: u16 = 1 << 0;
pub const PROCESS_SESSION_MEMORY_REQUEST_PRIVATE: u16 = 1 << 1;
pub const PROCESS_SESSION_MEMORY_REQUEST_WRITABLE: u16 = 1 << 2;
pub const PROCESS_SESSION_MEMORY_REQUEST_NO_EXECUTE: u16 = 1 << 3;
pub const PROCESS_SESSION_MEMORY_REQUEST_ZEROED: u16 = 1 << 4;
pub const PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL: u16 =
    PROCESS_SESSION_MEMORY_REQUEST_CALLER_SCOPED
        | PROCESS_SESSION_MEMORY_REQUEST_PRIVATE
        | PROCESS_SESSION_MEMORY_REQUEST_WRITABLE
        | PROCESS_SESSION_MEMORY_REQUEST_NO_EXECUTE
        | PROCESS_SESSION_MEMORY_REQUEST_ZEROED;

pub const PROCESS_SESSION_MEMORY_STATUS_OK: u8 = 0;
pub const PROCESS_SESSION_MEMORY_STATUS_INVALID_REQUEST: u8 = 1;
pub const PROCESS_SESSION_MEMORY_STATUS_STALE_REQUEST: u8 = 2;
pub const PROCESS_SESSION_MEMORY_STATUS_RESOURCE_UNAVAILABLE: u8 = 3;

pub const PROCESS_SESSION_MEMORY_DECISION_NONE: u8 = 0;
pub const PROCESS_SESSION_MEMORY_DECISION_MAPPED: u8 = 1;
pub const PROCESS_SESSION_MEMORY_DECISION_RELEASED: u8 = 2;
pub const PROCESS_SESSION_MEMORY_DECISION_REJECTED: u8 = 3;

pub const PROCESS_SESSION_MEMORY_REASON_NONE: u8 = 0;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_ABI_VERSION: u8 = 1;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_OPERATION: u8 = 2;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_MEMORY_KIND: u8 = 3;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_FLAGS: u8 = 4;
pub const PROCESS_SESSION_MEMORY_REASON_ZERO_REQUEST_ID: u8 = 5;
pub const PROCESS_SESSION_MEMORY_REASON_ZERO_SESSION_KEY: u8 = 6;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_BYTE_LENGTH: u8 = 7;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_MAPPING_IDENTITY: u8 = 8;
pub const PROCESS_SESSION_MEMORY_REASON_NONZERO_RESERVED: u8 = 9;
pub const PROCESS_SESSION_MEMORY_REASON_BAD_REQUEST_HASH: u8 = 10;
pub const PROCESS_SESSION_MEMORY_REASON_REQUEST_ID_NOT_NEXT: u8 = 11;
pub const PROCESS_SESSION_MEMORY_REASON_MAPPING_NOT_FOUND: u8 = 12;
pub const PROCESS_SESSION_MEMORY_REASON_MAPPING_IDENTITY_MISMATCH: u8 = 13;
pub const PROCESS_SESSION_MEMORY_REASON_PROCESS_QUOTA: u8 = 14;
pub const PROCESS_SESSION_MEMORY_REASON_PROTECTED_RESERVE: u8 = 15;
pub const PROCESS_SESSION_MEMORY_REASON_TOTAL_EXHAUSTED: u8 = 16;
pub const PROCESS_SESSION_MEMORY_REASON_FRAGMENTED: u8 = 17;
pub const PROCESS_SESSION_MEMORY_REASON_VIRTUAL_RANGE_EXHAUSTED: u8 = 18;
pub const PROCESS_SESSION_MEMORY_REASON_MAPPING_FAILED: u8 = 19;
pub const PROCESS_SESSION_MEMORY_REASON_SESSION_ALREADY_MAPPED: u8 = 20;
pub const PROCESS_SESSION_MEMORY_REASON_TRANSITION_GENERATION_EXHAUSTED: u8 = 21;

pub const PROCESS_SESSION_MEMORY_RESULT_CALLER_SCOPED: u32 = 1 << 0;
pub const PROCESS_SESSION_MEMORY_RESULT_PROCESS_PRIVATE: u32 = 1 << 1;
pub const PROCESS_SESSION_MEMORY_RESULT_ZEROED: u32 = 1 << 2;
pub const PROCESS_SESSION_MEMORY_RESULT_WRITABLE: u32 = 1 << 3;
pub const PROCESS_SESSION_MEMORY_RESULT_NO_EXECUTE: u32 = 1 << 4;
pub const PROCESS_SESSION_MEMORY_RESULT_KERNEL_SELECTED: u32 = 1 << 5;
pub const PROCESS_SESSION_MEMORY_RESULT_RUNTIME_CHARGED: u32 = 1 << 6;
pub const PROCESS_SESSION_MEMORY_RESULT_NO_PHYSICAL_ADDRESS: u32 = 1 << 7;
pub const PROCESS_SESSION_MEMORY_RESULT_SCRUBBED: u32 = 1 << 8;
pub const PROCESS_SESSION_MEMORY_RESULT_NO_MAPPING: u32 = 1 << 9;
pub const PROCESS_SESSION_MEMORY_RESULT_NO_MUTATION: u32 = 1 << 10;
pub const PROCESS_SESSION_MEMORY_RESULT_MAPPED_FLAGS: u32 =
    PROCESS_SESSION_MEMORY_RESULT_CALLER_SCOPED
        | PROCESS_SESSION_MEMORY_RESULT_PROCESS_PRIVATE
        | PROCESS_SESSION_MEMORY_RESULT_ZEROED
        | PROCESS_SESSION_MEMORY_RESULT_WRITABLE
        | PROCESS_SESSION_MEMORY_RESULT_NO_EXECUTE
        | PROCESS_SESSION_MEMORY_RESULT_KERNEL_SELECTED
        | PROCESS_SESSION_MEMORY_RESULT_RUNTIME_CHARGED
        | PROCESS_SESSION_MEMORY_RESULT_NO_PHYSICAL_ADDRESS;
pub const PROCESS_SESSION_MEMORY_RESULT_RELEASED_FLAGS: u32 =
    PROCESS_SESSION_MEMORY_RESULT_CALLER_SCOPED
        | PROCESS_SESSION_MEMORY_RESULT_PROCESS_PRIVATE
        | PROCESS_SESSION_MEMORY_RESULT_RUNTIME_CHARGED
        | PROCESS_SESSION_MEMORY_RESULT_NO_PHYSICAL_ADDRESS
        | PROCESS_SESSION_MEMORY_RESULT_SCRUBBED
        | PROCESS_SESSION_MEMORY_RESULT_NO_MAPPING;
pub const PROCESS_SESSION_MEMORY_RESULT_REJECTED_FLAGS: u32 =
    PROCESS_SESSION_MEMORY_RESULT_CALLER_SCOPED
        | PROCESS_SESSION_MEMORY_RESULT_NO_PHYSICAL_ADDRESS
        | PROCESS_SESSION_MEMORY_RESULT_NO_MAPPING
        | PROCESS_SESSION_MEMORY_RESULT_NO_MUTATION;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessSessionMemoryRequest {
    pub abi_version: u32,
    pub operation: u8,
    pub memory_kind: u8,
    pub request_flags: u16,
    pub requested_byte_len: u32,
    pub reserved0: u32,
    pub request_id: u64,
    pub session_key: u64,
    pub expected_mapping_id: u64,
    pub expected_mapping_generation: u64,
    pub expected_mapping_hash: u64,
    pub reserved1: [u64; 4],
    pub request_hash: u64,
}

impl ProcessSessionMemoryRequest {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        operation: 0,
        memory_kind: 0,
        request_flags: 0,
        requested_byte_len: 0,
        reserved0: 0,
        request_id: 0,
        session_key: 0,
        expected_mapping_id: 0,
        expected_mapping_generation: 0,
        expected_mapping_hash: 0,
        reserved1: [0; 4],
        request_hash: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessSessionMemoryReceipt {
    pub abi_version: u32,
    pub status: u8,
    pub decision: u8,
    pub reason: u8,
    pub memory_kind: u8,
    pub result_flags: u32,
    pub reserved0: u32,
    pub request_id: u64,
    pub request_hash: u64,
    pub session_key: u64,
    pub mapping_id: u64,
    pub mapping_generation: u64,
    pub mapping_hash: u64,
    pub user_virtual_address: u64,
    pub mapped_byte_len: u64,
    pub owner_process_id: u64,
    pub address_space_generation: u64,
    pub committed_frame_count: u64,
    pub quota_frame_count: u64,
    pub available_frame_count: u64,
    pub protected_reserve_frame_count: u64,
    pub transition_generation: u64,
    pub reserved1: u64,
    pub reserved2: u64,
    pub record_hash: u64,
}

impl ProcessSessionMemoryReceipt {
    pub const EMPTY: Self = Self {
        abi_version: 0,
        status: PROCESS_SESSION_MEMORY_STATUS_INVALID_REQUEST,
        decision: PROCESS_SESSION_MEMORY_DECISION_NONE,
        reason: PROCESS_SESSION_MEMORY_REASON_NONE,
        memory_kind: 0,
        result_flags: 0,
        reserved0: 0,
        request_id: 0,
        request_hash: 0,
        session_key: 0,
        mapping_id: 0,
        mapping_generation: 0,
        mapping_hash: 0,
        user_virtual_address: 0,
        mapped_byte_len: 0,
        owner_process_id: 0,
        address_space_generation: 0,
        committed_frame_count: 0,
        quota_frame_count: 0,
        available_frame_count: 0,
        protected_reserve_frame_count: 0,
        transition_generation: 0,
        reserved1: 0,
        reserved2: 0,
        record_hash: 0,
    };
}

pub fn compute_process_session_memory_request_hash(request: &ProcessSessionMemoryRequest) -> u64 {
    let mut hash = domain_hash(b"osaiku.process-session-memory.request.v1");
    hash = fold_u32(hash, request.abi_version);
    hash = fold_u8(hash, request.operation);
    hash = fold_u8(hash, request.memory_kind);
    hash = fold_u16(hash, request.request_flags);
    hash = fold_u32(hash, request.requested_byte_len);
    hash = fold_u32(hash, request.reserved0);
    hash = fold_u64(hash, request.request_id);
    hash = fold_u64(hash, request.session_key);
    hash = fold_u64(hash, request.expected_mapping_id);
    hash = fold_u64(hash, request.expected_mapping_generation);
    hash = fold_u64(hash, request.expected_mapping_hash);
    for value in request.reserved1 {
        hash = fold_u64(hash, value);
    }
    hash
}

pub fn process_session_memory_request_canonical(request: &ProcessSessionMemoryRequest) -> bool {
    if request.abi_version != PROCESS_SESSION_MEMORY_ABI_VERSION
        || session_memory_kind_limit(request.memory_kind) == 0
        || request.request_flags != PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL
        || request.request_id == 0
        || request.session_key == 0
        || request.reserved0 != 0
        || request.reserved1 != [0; 4]
        || request.request_hash != compute_process_session_memory_request_hash(request)
    {
        return false;
    }
    match request.operation {
        PROCESS_SESSION_MEMORY_OPERATION_MAP => {
            request.requested_byte_len != 0
                && request.requested_byte_len <= session_memory_kind_limit(request.memory_kind)
                && request.expected_mapping_id == 0
                && request.expected_mapping_generation == 0
                && request.expected_mapping_hash == 0
        }
        PROCESS_SESSION_MEMORY_OPERATION_RELEASE => {
            request.requested_byte_len == 0
                && request.expected_mapping_id != 0
                && request.expected_mapping_generation != 0
                && request.expected_mapping_hash != 0
        }
        _ => false,
    }
}

pub fn compute_process_session_memory_mapping_hash(
    owner_process_id: u64,
    address_space_generation: u64,
    session_key: u64,
    mapping_id: u64,
    mapping_generation: u64,
    user_virtual_address: u64,
    mapped_byte_len: u64,
) -> u64 {
    let mut hash = domain_hash(b"osaiku.process-session-memory.mapping.v1");
    hash = fold_u64(hash, owner_process_id);
    hash = fold_u64(hash, address_space_generation);
    hash = fold_u64(hash, session_key);
    hash = fold_u64(hash, mapping_id);
    hash = fold_u64(hash, mapping_generation);
    hash = fold_u64(hash, user_virtual_address);
    fold_u64(hash, mapped_byte_len)
}

pub fn compute_process_session_memory_receipt_hash(receipt: &ProcessSessionMemoryReceipt) -> u64 {
    let mut hash = domain_hash(b"osaiku.process-session-memory.receipt.v1");
    hash = fold_u32(hash, receipt.abi_version);
    hash = fold_u8(hash, receipt.status);
    hash = fold_u8(hash, receipt.decision);
    hash = fold_u8(hash, receipt.reason);
    hash = fold_u8(hash, receipt.memory_kind);
    hash = fold_u32(hash, receipt.result_flags);
    hash = fold_u32(hash, receipt.reserved0);
    hash = fold_u64(hash, receipt.request_id);
    hash = fold_u64(hash, receipt.request_hash);
    hash = fold_u64(hash, receipt.session_key);
    hash = fold_u64(hash, receipt.mapping_id);
    hash = fold_u64(hash, receipt.mapping_generation);
    hash = fold_u64(hash, receipt.mapping_hash);
    hash = fold_u64(hash, receipt.user_virtual_address);
    hash = fold_u64(hash, receipt.mapped_byte_len);
    hash = fold_u64(hash, receipt.owner_process_id);
    hash = fold_u64(hash, receipt.address_space_generation);
    hash = fold_u64(hash, receipt.committed_frame_count);
    hash = fold_u64(hash, receipt.quota_frame_count);
    hash = fold_u64(hash, receipt.available_frame_count);
    hash = fold_u64(hash, receipt.protected_reserve_frame_count);
    hash = fold_u64(hash, receipt.transition_generation);
    hash = fold_u64(hash, receipt.reserved1);
    fold_u64(hash, receipt.reserved2)
}

pub fn process_session_memory_receipt_canonical(receipt: &ProcessSessionMemoryReceipt) -> bool {
    if receipt.abi_version != PROCESS_SESSION_MEMORY_ABI_VERSION
        || session_memory_kind_limit(receipt.memory_kind) == 0
        || receipt.owner_process_id == 0
        || receipt.quota_frame_count == 0
        || receipt.committed_frame_count > receipt.quota_frame_count
        || receipt.transition_generation == 0
        || receipt.reserved0 != 0
        || receipt.reserved1 != 0
        || receipt.reserved2 != 0
        || receipt.record_hash != compute_process_session_memory_receipt_hash(receipt)
    {
        return false;
    }
    match receipt.decision {
        PROCESS_SESSION_MEMORY_DECISION_MAPPED => {
            receipt.request_id != 0
                && receipt.request_hash != 0
                && receipt.session_key != 0
                && receipt.status == PROCESS_SESSION_MEMORY_STATUS_OK
                && receipt.reason == PROCESS_SESSION_MEMORY_REASON_NONE
                && receipt.result_flags == PROCESS_SESSION_MEMORY_RESULT_MAPPED_FLAGS
                && receipt.mapping_id != 0
                && receipt.mapping_generation != 0
                && receipt.mapping_hash
                    == compute_process_session_memory_mapping_hash(
                        receipt.owner_process_id,
                        receipt.address_space_generation,
                        receipt.session_key,
                        receipt.mapping_id,
                        receipt.mapping_generation,
                        receipt.user_virtual_address,
                        receipt.mapped_byte_len,
                    )
                && receipt.user_virtual_address != 0
                && receipt.user_virtual_address % PROCESS_SESSION_MEMORY_PAGE_SIZE as u64 == 0
                && receipt.mapped_byte_len != 0
                && receipt.mapped_byte_len
                    <= u64::from(session_memory_kind_limit(receipt.memory_kind))
                && receipt.mapped_byte_len % u64::from(PROCESS_SESSION_MEMORY_PAGE_SIZE) == 0
                && receipt
                    .user_virtual_address
                    .checked_add(receipt.mapped_byte_len)
                    .is_some()
        }
        PROCESS_SESSION_MEMORY_DECISION_RELEASED => {
            receipt.request_id != 0
                && receipt.request_hash != 0
                && receipt.session_key != 0
                && receipt.status == PROCESS_SESSION_MEMORY_STATUS_OK
                && receipt.reason == PROCESS_SESSION_MEMORY_REASON_NONE
                && receipt.result_flags == PROCESS_SESSION_MEMORY_RESULT_RELEASED_FLAGS
                && receipt.mapping_id != 0
                && receipt.mapping_generation != 0
                && receipt.mapping_hash != 0
                && receipt.user_virtual_address == 0
                && receipt.mapped_byte_len == 0
        }
        PROCESS_SESSION_MEMORY_DECISION_REJECTED => {
            receipt.status != PROCESS_SESSION_MEMORY_STATUS_OK
                && receipt.reason != PROCESS_SESSION_MEMORY_REASON_NONE
                && receipt.result_flags == PROCESS_SESSION_MEMORY_RESULT_REJECTED_FLAGS
                && receipt.mapping_id == 0
                && receipt.mapping_generation == 0
                && receipt.mapping_hash == 0
                && receipt.user_virtual_address == 0
                && receipt.mapped_byte_len == 0
        }
        _ => false,
    }
}

fn domain_hash(domain: &[u8]) -> u64 {
    fold_bytes(0xcbf2_9ce4_8422_2325, domain)
}

fn fold_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn fold_u8(hash: u64, value: u8) -> u64 {
    fold_bytes(hash, &[value])
}

fn fold_u16(hash: u64, value: u16) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

fn fold_u32(hash: u64, value: u32) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

fn fold_u64(hash: u64, value: u64) -> u64 {
    fold_bytes(hash, &value.to_le_bytes())
}

const _: [(); PROCESS_SESSION_MEMORY_REQUEST_SIZE] =
    [(); core::mem::size_of::<ProcessSessionMemoryRequest>()];
const _: [(); PROCESS_SESSION_MEMORY_RECEIPT_SIZE] =
    [(); core::mem::size_of::<ProcessSessionMemoryReceipt>()];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_keeps_control_buffers_and_unused_arguments() {
        let request = ProcessSessionMemoryRequest::EMPTY;
        let mut receipt = ProcessSessionMemoryReceipt::EMPTY;
        let output = &mut receipt as *mut ProcessSessionMemoryReceipt as u64;
        assert_eq!(
            transition_arguments(&request, &mut receipt),
            [
                &request as *const ProcessSessionMemoryRequest as u64,
                output,
                0,
                0,
                0,
            ]
        );
        assert_eq!(receipt, ProcessSessionMemoryReceipt::EMPTY);
    }
}
