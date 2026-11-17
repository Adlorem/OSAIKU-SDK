//! Bounded diagnostic aggregates, never scheduling or completion authority.
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
pub const MAGIC: [u8; 8] = *b"OSAMEAS1";
pub const BYTES: usize = 72;
pub const PHASES: usize = 22;
pub const NAMES: [&str; PHASES] = [
    "prepare",
    "rpc",
    "rcs-wait",
    "driverd-work",
    "catalog",
    "initd-tasks",
    "transaction",
    "send",
    "receive",
    "catalog-page",
    "scanout-capacity",
    "present-barrier",
    "tx-begin",
    "tx-full",
    "tx-cancel",
    "tx-drain",
    "tx-dead",
    "tx-failed",
    "tx-no-slot",
    "initd-active-gap",
    "diagnostic-report",
    "rcs-observed",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub phase: u64,
    pub count: u64,
    pub ticks: u64,
    pub max: u64,
    pub invalid: u64,
    pub bytes: u64,
    pub lost: u64,
    pub auxiliary: u64,
}
impl Snapshot {
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(&MAGIC);
        for (i, n) in [
            self.phase,
            self.count,
            self.ticks,
            self.max,
            self.invalid,
            self.bytes,
            self.lost,
            self.auxiliary,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != BYTES || b[..8] != MAGIC {
            return None;
        }
        let n = |i: usize| u64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap());
        let s = Self {
            phase: n(0),
            count: n(1),
            ticks: n(2),
            max: n(3),
            invalid: n(4),
            bytes: n(5),
            lost: n(6),
            auxiliary: n(7),
        };
        (s.phase < PHASES as u64 && s.max <= s.ticks && (s.count != 0 || s.ticks == 0)).then_some(s)
    }
}
pub struct Counter {
    lock: AtomicBool,
    fields: [AtomicU64; 6],
    lost: AtomicU64,
}
impl Counter {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
            fields: [const { AtomicU64::new(0) }; 6],
            lost: AtomicU64::new(0),
        }
    }
    fn acquire(&self) -> bool {
        if self
            .lock
            .compare_exchange(false, true, core::sync::atomic::Ordering::Acquire, Relaxed)
            .is_err()
        {
            self.lost.fetch_add(1, Relaxed);
            false
        } else {
            true
        }
    }
    pub fn record(&self, ticks: Option<u64>, bytes: u64, auxiliary: u64) {
        if !self.acquire() {
            return;
        }
        let v = core::array::from_fn::<_, 6, _>(|i| self.fields[i].load(Relaxed));
        let sums = ticks.and_then(|t| {
            Some((
                v[0].checked_add(1)?,
                v[1].checked_add(t)?,
                v[2].max(t),
                v[4].checked_add(bytes)?,
                v[5].checked_add(auxiliary)?,
            ))
        });
        if let Some((n, t, m, b, a)) = sums {
            for (i, x) in [(0, n), (1, t), (2, m), (4, b), (5, a)] {
                self.fields[i].store(x, Relaxed);
            }
        } else {
            self.fields[3].store(v[3].saturating_add(1), Relaxed);
        }
        self.lock
            .store(false, core::sync::atomic::Ordering::Release);
    }
    pub fn snapshot(&self, phase: u64) -> Option<Snapshot> {
        if !self.acquire() {
            return None;
        }
        let v = core::array::from_fn::<_, 6, _>(|i| self.fields[i].load(Relaxed));
        self.lock
            .store(false, core::sync::atomic::Ordering::Release);
        Some(Snapshot {
            phase,
            count: v[0],
            ticks: v[1],
            max: v[2],
            invalid: v[3],
            bytes: v[4],
            auxiliary: v[5],
            lost: self.lost.load(Relaxed),
        })
    }
}
