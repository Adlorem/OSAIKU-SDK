use super::*;
use core::ptr::NonNull;

/// Unique owner of a live host-service byte buffer. No implicit syscall
/// or allocator: retire it before explicit RELEASE. A forgotten owner remains
/// charged until process cleanup. Rust borrows cannot survive retirement.
#[must_use]
pub struct SessionBuffer {
    pointer: NonNull<u8>,
    mapping: ProcessSessionMemoryReceipt,
}

impl SessionBuffer {
    /// Validate and adopt zeroed native storage. Checks all bytes in user mode,
    /// where ordinary process preemption remains available.
    ///
    /// # Safety
    /// A valid receipt must describe this caller's actual live MAP allocation,
    /// exclusively transferred here, with no existing values/aliases. Keep the
    /// request, receipt and this owner outside the allocation. No continuation
    /// may access or unmap it while this owner or its borrows are live.
    pub unsafe fn adopt(
        request: &ProcessSessionMemoryRequest,
        mapping: &ProcessSessionMemoryReceipt,
        status: isize,
        owner_pid: u64,
    ) -> Result<Self, SessionPageError> {
        if !matches!(
            request.memory_kind,
            PROCESS_SESSION_MEMORY_KIND_COMPOSITOR_BUFFER
                | PROCESS_SESSION_MEMORY_KIND_ASSISTANT_BUFFER
        ) || status != PROCESS_SESSION_MEMORY_STATUS_OK as isize
            || !mapping.matches_request(request, owner_pid)
            || mapping.decision != PROCESS_SESSION_MEMORY_DECISION_MAPPED
        {
            return Err(SessionPageError::InvalidMapping);
        }
        let address = usize::try_from(mapping.user_virtual_address)
            .map_err(|_| SessionPageError::InvalidMapping)?;
        let length = usize::try_from(mapping.mapped_byte_len)
            .map_err(|_| SessionPageError::InvalidMapping)?;
        address
            .checked_add(length)
            .ok_or(SessionPageError::InvalidMapping)?;
        let pointer = NonNull::new(address as *mut u8).ok_or(SessionPageError::InvalidMapping)?;
        // SAFETY: caller supplies exclusive live memory; metadata bounds checked.
        if unsafe { core::slice::from_raw_parts(pointer.as_ptr(), length) }
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(SessionPageError::NotZeroed);
        }
        Ok(Self {
            pointer,
            mapping: *mapping,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.pointer.as_ptr(),
                self.mapping.mapped_byte_len as usize,
            )
        }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.pointer.as_ptr(),
                self.mapping.mapped_byte_len as usize,
            )
        }
    }

    pub fn mapping(&self) -> &ProcessSessionMemoryReceipt {
        &self.mapping
    }

    /// Scrub while still mapped, end access, and return metadata for RELEASE.
    pub fn retire(self) -> ProcessSessionMemoryReceipt {
        self.mapping
    }
}

impl Drop for SessionBuffer {
    fn drop(&mut self) {
        for byte in self.bytes_mut() {
            unsafe {
                core::ptr::write_volatile(byte, 0);
            }
        }
    }
}
