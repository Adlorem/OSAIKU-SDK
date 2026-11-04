//! Optional numeric diagnostic observations, never presentation receipts.
//! Native timing is invariant-TSC wall time, including preemption/migration.
pub const MAGIC: [u8; 8] = *b"OSAGPUT1";
pub const BYTES: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub phase: u64,
    pub cycles: Option<u64>,
    pub bytes: u64,
}
impl Record {
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(&MAGIC);
        b[8..16].copy_from_slice(&self.phase.to_le_bytes());
        b[16..24].copy_from_slice(&self.cycles.unwrap_or(u64::MAX).to_le_bytes());
        b[24..].copy_from_slice(&self.bytes.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != BYTES || b[..8] != MAGIC {
            return None;
        }
        let phase = u64::from_le_bytes(b[8..16].try_into().ok()?);
        let cycles = u64::from_le_bytes(b[16..24].try_into().ok()?);
        let bytes = u64::from_le_bytes(b[24..32].try_into().ok()?);
        if phase > 1 || bytes > 1024 * 1024 * 1024 {
            return None;
        }
        Some(Self {
            phase,
            cycles: (cycles != u64::MAX).then_some(cycles),
            bytes,
        })
    }
}
#[derive(Clone, Copy)]
pub struct Accumulator {
    cycles: Option<u64>,
    bytes: u64,
}
impl Accumulator {
    pub const EMPTY: Self = Self {
        cycles: Some(0),
        bytes: 0,
    };
    pub fn add(&mut self, start: Option<u64>, end: Option<u64>, bytes: usize) {
        self.cycles = self
            .cycles
            .zip(start.zip(end))
            .and_then(|(sum, (a, b))| sum.checked_add(b.checked_sub(a)?));
        self.bytes = self.bytes.saturating_add(bytes as u64);
    }
    pub fn record(self, phase: u64) -> Record {
        Record {
            phase,
            cycles: self.cycles,
            bytes: self.bytes,
        }
    }
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn stamp() -> Option<u64> {
    use core::sync::atomic::{AtomicU8, Ordering};
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
    // Diagnostic ordering only; no clock or deadline authority is granted.
    Some(unsafe {
        core::arch::x86_64::_mm_lfence();
        let t = core::arch::x86_64::_rdtsc();
        core::arch::x86_64::_mm_lfence();
        t
    })
}
#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn stamp() -> Option<u64> {
    None
}
pub fn emit(record: Record) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let bytes = record.encode();
        // Best effort. Older kernels may ignore this version; rendering cannot
        // fail because a diagnostic receiver is absent or full.
        unsafe {
            crate::raw::syscall(
                crate::abi::number::LOG_WRITE,
                [bytes.as_ptr() as u64, bytes.len() as u64, 0, 0, 0],
            );
        }
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    let _ = record;
}
