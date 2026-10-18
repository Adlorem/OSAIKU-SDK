use super::*;
use core::{mem, ptr::NonNull};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPageError {
    InvalidLayout,
    InvalidMapping,
    NotZeroed,
}

/// Unique owner of one initialized value in an admitted session page.
///
/// Adoption is unsafe; subsequent access is bounded by Rust borrows. This type
/// is neither Copy nor Clone. Normal destruction drops the value and scrubs the
/// entire page, but never performs a syscall. Use `retire` before the explicit
/// RELEASE transition; simply dropping/forgetting the owner leaves the mapping
/// allocated until process cleanup. Keep this handle outside its owned page.
#[must_use = "Retire the page and explicitly release its mapping."]
pub struct SessionPage<T> {
    pointer: NonNull<T>,
    mapping: ProcessSessionMemoryReceipt,
}

impl<T> SessionPage<T> {
    /// Validate a successful MAP response, verify the whole page is zero and
    /// initialize its value. Errors never initialize or scrub the mapping.
    /// The supplied value is dropped normally on error. No syscall or retry.
    ///
    /// # Safety
    /// For a response that passes status/metadata validation, its address must
    /// denote the live, writable allocation from this caller's kernel MAP:
    /// one full page, with no live values, references, aliases or other owner.
    /// Transfer exclusive access to this handle and keep the allocation live
    /// until the handle is retired/dropped (including its value's destructor).
    /// No external continuation may access or unmap the page during that time.
    /// Keep request/receipt control buffers outside the transferred page.
    /// Metadata/hashes alone cannot prove these lifetime/ownership conditions.
    /// Invalid status/metadata is rejected before accessing the address.
    pub unsafe fn adopt(
        request: &ProcessSessionMemoryRequest,
        mapping: &ProcessSessionMemoryReceipt,
        syscall_status: isize,
        owner_pid: u64,
        value: T,
    ) -> Result<Self, SessionPageError> {
        let size = mem::size_of::<T>();
        let page_size = PROCESS_SESSION_MEMORY_PAGE_SIZE as usize;
        if size == 0 || size > page_size || mem::align_of::<T>() > page_size {
            return Err(SessionPageError::InvalidLayout);
        }
        if request.memory_kind != PROCESS_SESSION_MEMORY_KIND_APPLICATION_STATE
            || syscall_status != PROCESS_SESSION_MEMORY_STATUS_OK as isize
            || !mapping.matches_request(request, owner_pid)
            || mapping.decision != PROCESS_SESSION_MEMORY_DECISION_MAPPED
            || (request.requested_byte_len as usize) < size
        {
            return Err(SessionPageError::InvalidMapping);
        }
        let address = usize::try_from(mapping.user_virtual_address)
            .map_err(|_| SessionPageError::InvalidMapping)?;
        address
            .checked_add(page_size)
            .ok_or(SessionPageError::InvalidMapping)?;
        let pointer = NonNull::new(address as *mut T).ok_or(SessionPageError::InvalidMapping)?;
        // SAFETY: caller transfers the whole live page; canonical MAP verifies
        // its size and page alignment, sufficient for the checked T layout.
        let bytes = unsafe { core::slice::from_raw_parts(address as *const u8, page_size) };
        if bytes.iter().any(|byte| *byte != 0) {
            return Err(SessionPageError::NotZeroed);
        }
        // SAFETY: exclusive, aligned, large enough and previously uninitialized.
        unsafe { pointer.as_ptr().write(value) };
        Ok(Self {
            pointer,
            mapping: *mapping,
        })
    }

    pub fn value(&self) -> &T {
        // SAFETY: initialized at adoption; ownership lives as long as this borrow.
        unsafe { self.pointer.as_ref() }
    }

    pub fn value_mut(&mut self) -> &mut T {
        // SAFETY: the handle uniquely owns the initialized value.
        unsafe { self.pointer.as_mut() }
    }

    /// Kernel-issued metadata only; copying it does not duplicate ownership.
    pub fn mapping(&self) -> &ProcessSessionMemoryReceipt {
        &self.mapping
    }

    /// Consume the owner, destroy the value and scrub the page before returning
    /// copied mapping metadata for RELEASE. No pointer or borrow is returned.
    /// The caller must still check the release outcome; retirement itself does
    /// not assert that the kernel has unmapped the allocation.
    ///
    /// ```compile_fail
    /// use osaiku_runtime::session::SessionPage;
    /// fn cannot_keep_a_borrow(page: SessionPage<u64>) {
    ///     let value = page.value();
    ///     let mapping = page.retire();
    ///     assert_eq!(*value, 0);
    /// }
    /// ```
    pub fn retire(self) -> ProcessSessionMemoryReceipt {
        let mapping = self.mapping;
        drop(self);
        mapping
    }
}

impl<T> Drop for SessionPage<T> {
    fn drop(&mut self) {
        // SAFETY: exclusive live allocation; destroy T before touching its bytes.
        unsafe {
            core::ptr::drop_in_place(self.pointer.as_ptr());
            let bytes = self.pointer.as_ptr().cast::<u8>();
            for offset in 0..PROCESS_SESSION_MEMORY_PAGE_SIZE as usize {
                bytes.add(offset).write_volatile(0);
            }
        }
    }
}
