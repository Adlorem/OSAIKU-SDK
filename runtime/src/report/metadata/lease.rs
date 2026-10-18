//! Ownership of the process-wide worker workspace, including abandoned workers.
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(super) struct Lease {
    held: AtomicBool,
    orphan: AtomicU64,
}
impl Lease {
    pub const fn new() -> Self {
        Self {
            held: AtomicBool::new(false),
            orphan: AtomicU64::new(0),
        }
    }
    pub fn acquire(&self, join: impl FnOnce(u64) -> bool, clear: impl FnOnce()) -> bool {
        // One contender takes responsibility for joining. Others still observe
        // held=true until the worker has stopped and its workspace is cleared.
        let orphan = self.orphan.swap(0, Ordering::AcqRel);
        if orphan != 0 {
            if !join(orphan) {
                self.orphan.store(orphan, Ordering::Release);
                return false;
            }
            clear();
            self.release();
        }
        self.held
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
    pub fn release(&self) {
        self.held.store(false, Ordering::Release);
    }
    pub fn abandon(&self, thread: u64) {
        // The owner keeps the lease charged; admission may release it only
        // after a successful join. Thread IDs supplied here are nonzero.
        self.orphan.store(thread, Ordering::Release);
    }
}
