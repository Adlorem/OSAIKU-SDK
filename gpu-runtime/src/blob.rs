//! EGL_ANDROID_blob_cache callbacks. One process-lifetime cache, without locks
//! that can park a Mesa thread. Contention becomes a cache miss.
use crate::cache::{Cache, MAX_BLOB, MAX_KEY};
use core::{
    cell::UnsafeCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

pub type Set = unsafe extern "C" fn(*const c_void, isize, *const c_void, isize);
pub type Get = unsafe extern "C" fn(*const c_void, isize, *mut c_void, isize) -> isize;
static ACTIVE: AtomicPtr<SharedCache> = AtomicPtr::new(core::ptr::null_mut());

pub struct SharedCache {
    busy: AtomicBool,
    cache: UnsafeCell<Cache<'static>>,
    build_text: [u8; 65],
}
// Access to the cell requires exclusive acquisition of busy. Cache never lends
// pointers outside the critical section and its arena has a static lifetime.
unsafe impl Sync for SharedCache {}
impl SharedCache {
    pub fn new(cache: Cache<'static>) -> Self {
        let mut build_text = [0; 65];
        let build = cache.identity().build;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for (i, byte) in build.iter().enumerate() {
            build_text[2 * i] = HEX[(byte >> 4) as usize];
            build_text[2 * i + 1] = HEX[(byte & 15) as usize];
        }
        Self {
            busy: AtomicBool::new(false),
            cache: UnsafeCell::new(cache),
            build_text,
        }
    }
    /// Installation is irreversible for the lifetime of a process: EGL may
    /// retain callbacks on compiler workers even after a caller returns.
    pub fn install(&'static self) -> Option<(Set, Get)> {
        let pointer = core::ptr::from_ref(self).cast_mut();
        match ACTIVE.compare_exchange(
            core::ptr::null_mut(),
            pointer,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Some((set, get)),
            Err(existing) if existing == pointer => Some((set, get)),
            Err(_) => None,
        }
    }
}
/// The static native cache backend uses the same checked callbacks as EGL.
#[no_mangle]
pub unsafe extern "C" fn osaiku_mesa_cache_set(
    key: *const c_void,
    ks: isize,
    value: *const c_void,
    vs: isize,
) {
    unsafe {
        set(key, ks, value, vs);
    }
}
#[no_mangle]
pub unsafe extern "C" fn osaiku_mesa_cache_get(
    key: *const c_void,
    ks: isize,
    value: *mut c_void,
    vs: isize,
) -> isize {
    unsafe { get(key, ks, value, vs) }
}
#[no_mangle]
pub extern "C" fn osaiku_mesa_build_identity() -> *const u8 {
    let pointer = ACTIVE.load(Ordering::Acquire);
    if pointer.is_null() {
        core::ptr::null()
    } else {
        unsafe { (*pointer).build_text.as_ptr() }
    }
}
struct Guard(&'static SharedCache);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}
fn acquire() -> Option<Guard> {
    let pointer = ACTIVE.load(Ordering::Acquire);
    if pointer.is_null() {
        return None;
    }
    // install accepts only a static reference; ACTIVE is never cleared.
    let shared = unsafe { &*pointer };
    shared
        .busy
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .ok()?;
    Some(Guard(shared))
}
// Pointer validity is the trusted Mesa callback ABI's precondition. Signed
// lengths are validated before conversion. The arena is exclusively owned by
// Cache, so Mesa input/output buffers cannot alias it through safe Rust.
unsafe extern "C" fn set(key: *const c_void, ks: isize, value: *const c_void, vs: isize) {
    if key.is_null()
        || value.is_null()
        || ks <= 0
        || ks > MAX_KEY as isize
        || vs <= 0
        || vs > MAX_BLOB as isize
    {
        return;
    }
    let Some(guard) = acquire() else {
        return;
    };
    unsafe {
        (&mut *guard.0.cache.get()).put(
            core::slice::from_raw_parts(key.cast(), ks as usize),
            core::slice::from_raw_parts(value.cast(), vs as usize),
        );
    }
}
unsafe extern "C" fn get(
    key: *const c_void,
    ks: isize,
    output: *mut c_void,
    capacity: isize,
) -> isize {
    if key.is_null()
        || ks <= 0
        || ks > MAX_KEY as isize
        || capacity < 0
        || capacity > MAX_BLOB as isize
        || (capacity > 0 && output.is_null())
    {
        return 0;
    }
    let Some(guard) = acquire() else {
        return 0;
    };
    unsafe {
        let target = if capacity == 0 {
            &mut []
        } else {
            core::slice::from_raw_parts_mut(output.cast(), capacity as usize)
        };
        (&mut *guard.0.cache.get())
            .get(core::slice::from_raw_parts(key.cast(), ks as usize), target) as isize
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::cache::{Identity, HEADER, SLOTS};
    use std::{boxed::Box, vec};
    #[test]
    fn callback_sizes_contention_and_process_lifetime() {
        let arena = Box::leak(vec![0; SLOTS * (HEADER + 16)].into_boxed_slice());
        let cache = Cache::new(
            Identity {
                build: [1; 32],
                device: [2; 32],
                configuration: [3; 32],
            },
            arena,
        )
        .unwrap();
        let shared = Box::leak(Box::new(SharedCache::new(cache)));
        let (set, get) = shared.install().unwrap();
        let key = b"key";
        let value = b"shader";
        let mut out = [0xa5; 16];
        unsafe {
            set(key.as_ptr().cast(), -1, value.as_ptr().cast(), 6);
            assert_eq!(get(key.as_ptr().cast(), 3, out.as_mut_ptr().cast(), 16), 0);
            set(key.as_ptr().cast(), 3, value.as_ptr().cast(), 6);
            assert_eq!(get(key.as_ptr().cast(), 3, core::ptr::null_mut(), 0), 6);
            assert_eq!(get(key.as_ptr().cast(), 3, out.as_mut_ptr().cast(), 2), 6);
            assert_eq!(out, [0xa5; 16]);
            shared.busy.store(true, Ordering::Release);
            assert_eq!(get(key.as_ptr().cast(), 3, out.as_mut_ptr().cast(), 16), 0);
            shared.busy.store(false, Ordering::Release);
            assert_eq!(get(key.as_ptr().cast(), 3, out.as_mut_ptr().cast(), 16), 6);
            assert_eq!(&out[..6], value);
            assert_eq!(get(core::ptr::null(), 3, out.as_mut_ptr().cast(), 16), 0);
        }
        let workers: std::vec::Vec<_> = (0u8..8)
            .map(|id| {
                std::thread::spawn(move || {
                    let key = [id; 8];
                    let value = [id; 16];
                    for _ in 0..200 {
                        let mut output = [0xff; 16];
                        unsafe {
                            set(key.as_ptr().cast(), 8, value.as_ptr().cast(), 16);
                            let size = get(key.as_ptr().cast(), 8, output.as_mut_ptr().cast(), 16);
                            assert!(size == 0 || size == 16);
                            if size == 16 {
                                assert_eq!(output, value);
                            }
                        }
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
