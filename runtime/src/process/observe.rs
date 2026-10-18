//! Bounded kernel-owned lifecycle observations. Registration authority is
//! checked against the live conversation window or model controller binding.
pub const VERSION: u64 = 1;
pub const SUBSCRIBE: u64 = 1;
pub const TAKE: u64 = 2;
pub const REMOVE: u64 = 3;
pub const CAPACITY: usize = 8;
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Zero subscribes a live conversation window to model selection changes;
    /// nonzero names a concrete process incarnation whose departure is watched.
    pub endpoint: u64,
    pub window: u64,
    pub incarnation: u64,
}
#[derive(Clone, Copy)]
struct Entry {
    key: Notice,
    gone: bool,
}
/// Fixed kernel storage, tied to the observer process incarnation. A departure
/// is retained until TAKE, even if no mailbox slot was available at the time.
pub struct Observations {
    slots: [Option<Entry>; CAPACITY],
}
impl Observations {
    pub const EMPTY: Self = Self {
        slots: [None; CAPACITY],
    };
    pub fn subscribe(&mut self, key: Notice) -> bool {
        if self.slots.iter().flatten().any(|e| e.key == key) {
            return true;
        }
        let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) else {
            return false;
        };
        *slot = Some(Entry { key, gone: false });
        true
    }
    pub fn remove(&mut self, key: Notice) {
        for s in &mut self.slots {
            if s.is_some_and(|e| e.key == key) {
                *s = None;
            }
        }
    }
    pub fn departed(&mut self, endpoint: u64, window: Option<(u64, u64)>) -> bool {
        let mut changed = false;
        for e in self.slots.iter_mut().flatten() {
            if (window.is_none() && e.key.endpoint == endpoint)
                || window.is_some_and(|w| w == (e.key.window, e.key.incarnation))
            {
                changed |= !e.gone;
                e.gone = true;
            }
        }
        changed
    }
    pub fn pending(&self) -> bool {
        self.slots.iter().flatten().any(|e| e.gone)
    }
    pub fn take(&mut self) -> Option<Notice> {
        let s = self.slots.iter_mut().find(|s| s.is_some_and(|e| e.gone))?;
        s.take().map(|e| e.key)
    }
}
