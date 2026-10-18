use super::*;

impl ProcessSessionMemoryRequest {
    /// Build host-assistant buffer metadata. The host enforces the service role.
    pub fn map_assistant_buffer(
        request_id: u64,
        session_key: u64,
        byte_len: usize,
    ) -> Option<Self> {
        if byte_len == 0 || byte_len > PROCESS_SESSION_MEMORY_ASSISTANT_LIMIT as usize {
            return None;
        }
        seal(Self {
            abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
            operation: PROCESS_SESSION_MEMORY_OPERATION_MAP,
            memory_kind: PROCESS_SESSION_MEMORY_KIND_ASSISTANT_BUFFER,
            request_flags: PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL,
            requested_byte_len: byte_len as u32,
            request_id,
            session_key,
            ..Self::EMPTY
        })
    }

    /// Request compositor-private storage, rounded up by the kernel to 4 KiB.
    /// This constructs metadata only; it grants no allocation authority.
    pub fn map_buffer(request_id: u64, session_key: u64, byte_len: usize) -> Option<Self> {
        if byte_len == 0 || byte_len > PROCESS_SESSION_MEMORY_BUFFER_LIMIT as usize {
            return None;
        }
        seal(Self {
            abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
            operation: PROCESS_SESSION_MEMORY_OPERATION_MAP,
            memory_kind: PROCESS_SESSION_MEMORY_KIND_COMPOSITOR_BUFFER,
            request_flags: PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL,
            requested_byte_len: byte_len as u32,
            request_id,
            session_key,
            ..Self::EMPTY
        })
    }
    /// Build a bounded MAP request without allocating or advancing a cursor.
    /// `byte_len` must be 1..=4096; larger usize values are never truncated.
    /// Request sequencing and the session key's derivation remain the caller's
    /// responsibility. A valid request does not guarantee resource admission.
    pub fn map(request_id: u64, session_key: u64, byte_len: usize) -> Option<Self> {
        if byte_len == 0 || byte_len > PROCESS_SESSION_MEMORY_PAGE_SIZE as usize {
            return None;
        }
        seal(Self {
            abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
            operation: PROCESS_SESSION_MEMORY_OPERATION_MAP,
            memory_kind: PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE,
            request_flags: PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL,
            requested_byte_len: byte_len as u32,
            request_id,
            session_key,
            ..Self::EMPTY
        })
    }

    /// Build RELEASE for an exact retained MAP receipt. No syscall is performed.
    /// The receipt must come from the kernel and still identify a live mapping;
    /// canonical metadata/hashes cannot establish that. Before the unsafe
    /// transition, end every access/reference and destroy live values in the
    /// page. Keep the receipt and request buffers outside the page being released.
    pub fn release(request_id: u64, mapping: &ProcessSessionMemoryReceipt) -> Option<Self> {
        if !process_session_memory_receipt_canonical(mapping)
            || mapping.decision != PROCESS_SESSION_MEMORY_DECISION_MAPPED
        {
            return None;
        }
        seal(Self {
            abi_version: PROCESS_SESSION_MEMORY_ABI_VERSION,
            operation: PROCESS_SESSION_MEMORY_OPERATION_RELEASE,
            memory_kind: mapping.memory_kind,
            request_flags: PROCESS_SESSION_MEMORY_REQUEST_FLAGS_CANONICAL,
            request_id,
            session_key: mapping.session_key,
            expected_mapping_id: mapping.mapping_id,
            expected_mapping_generation: mapping.mapping_generation,
            expected_mapping_hash: mapping.mapping_hash,
            ..Self::EMPTY
        })
    }
}

impl ProcessSessionMemoryReceipt {
    /// Match canonical response metadata to a canonical request and trusted PID.
    /// The PID is numeric, obtained from the kernel-issued self endpoint. Also
    /// check the native syscall result and expected success/refusal status before
    /// acting: a matching refusal does not grant an address. This helper performs
    /// no syscall, authentication, pointer dereference, retry or cursor update.
    pub fn matches_request(&self, request: &ProcessSessionMemoryRequest, owner_pid: u64) -> bool {
        if !process_session_memory_request_canonical(request)
            || !process_session_memory_receipt_canonical(self)
            || self.owner_process_id != owner_pid
            || self.request_id != request.request_id
            || self.request_hash != request.request_hash
            || self.session_key != request.session_key
            || self.memory_kind != request.memory_kind
        {
            return false;
        }
        match self.decision {
            PROCESS_SESSION_MEMORY_DECISION_MAPPED => {
                request.operation == PROCESS_SESSION_MEMORY_OPERATION_MAP
                    && self.mapped_byte_len
                        == u64::from(request.requested_byte_len)
                            .div_ceil(u64::from(PROCESS_SESSION_MEMORY_PAGE_SIZE))
                            * u64::from(PROCESS_SESSION_MEMORY_PAGE_SIZE)
            }
            PROCESS_SESSION_MEMORY_DECISION_RELEASED => {
                request.operation == PROCESS_SESSION_MEMORY_OPERATION_RELEASE
                    && self.mapping_id == request.expected_mapping_id
                    && self.mapping_generation == request.expected_mapping_generation
                    && self.mapping_hash == request.expected_mapping_hash
            }
            PROCESS_SESSION_MEMORY_DECISION_REJECTED => true,
            _ => false,
        }
    }
}

fn seal(mut request: ProcessSessionMemoryRequest) -> Option<ProcessSessionMemoryRequest> {
    request.request_hash = compute_process_session_memory_request_hash(&request);
    process_session_memory_request_canonical(&request).then_some(request)
}
