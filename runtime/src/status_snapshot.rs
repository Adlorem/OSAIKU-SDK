//! A bounded read of the most recently updated/presented document metadata.
//! A preempted writer never makes the handler spin, follow a session pointer,
//! or borrow a main-task object. A mismatched/stale window simply cannot reply.
use core::sync::atomic::{AtomicU64, Ordering::SeqCst};

pub struct Snapshot {
    sequence: AtomicU64,
    words: [AtomicU64; 5],
}

impl Snapshot {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            words: [const { AtomicU64::new(0) }; 5],
        }
    }

    /// The main task is the sole writer. All fields are atomic because the
    /// separately scheduled reader can interrupt publication at any point.
    pub fn publish(&self, window: u64, generation: u64, revision: u64, length: u64, save: u64) {
        let before = self.sequence.load(SeqCst);
        if before >= u64::MAX - 2 {
            self.sequence.store(u64::MAX, SeqCst);
            return;
        }
        self.sequence.store(before + 1, SeqCst);
        for (word, value) in self
            .words
            .iter()
            .zip([window, generation, revision, length, save])
        {
            word.store(value, SeqCst);
        }
        self.sequence.store(before + 2, SeqCst);
    }

    pub fn read(&self, window: u64, generation: u64, revision: u64) -> Option<(u64, u64)> {
        let before = self.sequence.load(SeqCst);
        if before == 0 || before & 1 != 0 {
            return None;
        }
        let words = self.words.each_ref().map(|word| word.load(SeqCst));
        if self.sequence.load(SeqCst) != before || words[..3] != [window, generation, revision] {
            return None;
        }
        Some((words[3], words[4]))
    }
}
