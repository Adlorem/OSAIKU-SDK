//! TSC wall scopes (including preemption), bounded nonblocking aggregation.
use crate::protocol::measurement::{Counter, PHASES};
use core::sync::atomic::{AtomicU64, Ordering};
static COUNTERS: [Counter; PHASES] = [const { Counter::new() }; PHASES];
static NEXT: AtomicU64 = AtomicU64::new(5000);
pub fn stamp() -> Option<u64> {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        use core::sync::atomic::AtomicU8;
        static SUPPORT: AtomicU8 = AtomicU8::new(0);
        let mut supported = SUPPORT.load(Ordering::Relaxed);
        if supported == 0 {
            supported = if core::arch::x86_64::__cpuid(0x80000000).eax >= 0x80000007
                && core::arch::x86_64::__cpuid(0x80000007).edx & 256 != 0
            {
                1
            } else {
                2
            };
            SUPPORT.store(supported, Ordering::Relaxed);
        }
        if supported != 1 {
            return None;
        }
        Some(unsafe {
            core::arch::x86_64::_mm_lfence();
            let t = core::arch::x86_64::_rdtsc();
            core::arch::x86_64::_mm_lfence();
            t
        })
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        None
    }
}
pub struct Scope {
    phase: usize,
    start: Option<u64>,
    bytes: u64,
    auxiliary: u64,
}
impl Scope {
    pub fn new(phase: usize, bytes: u64) -> Self {
        Self {
            phase,
            start: stamp(),
            bytes,
            auxiliary: 0,
        }
    }
    pub fn auxiliary(&mut self, n: u64) {
        self.auxiliary = n;
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        record(self.phase, self.start, self.bytes, self.auxiliary);
    }
}
pub fn event(phase: usize) {
    if let Some(c) = COUNTERS.get(phase) {
        c.record(Some(0), 0, 0);
    }
}
pub fn record(phase: usize, start: Option<u64>, bytes: u64, auxiliary: u64) {
    if let Some(c) = COUNTERS.get(phase) {
        c.record(
            start.zip(stamp()).and_then(|(a, b)| b.checked_sub(a)),
            bytes,
            auxiliary,
        );
    }
}
/// At most 21 binary records per five seconds. No allocations, retry, UART,
/// peer mailbox or data-dependent payload; failure never affects the caller.
pub fn report(now: Option<u64>) {
    let Some(now) = now.filter(|n| *n != u64::MAX) else {
        return;
    };
    let next = NEXT.load(Ordering::Relaxed);
    if now < next {
        return;
    }
    if NEXT
        .compare_exchange(
            next,
            now.saturating_add(5000),
            Ordering::Relaxed,
            Ordering::Relaxed,
        )
        .is_err()
    {
        return;
    }
    let _overhead = Scope::new(20, 0);
    for (i, c) in COUNTERS.iter().enumerate() {
        if let Some(s) = c.snapshot(i as u64) {
            if s.count == 0 && s.invalid == 0 && s.lost == 0 {
                continue;
            }
            #[cfg(all(target_arch = "x86_64", target_os = "none"))]
            {
                let bytes = s.encode();
                let _ = crate::native::log_bytes(&bytes);
            }
            #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
            let _ = s;
        }
    }
}
