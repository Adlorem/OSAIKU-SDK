//! Revision-bound, bounded depth traversal shared by the application and tests.
use crate::files::*;
use core::sync::atomic::{AtomicU64, Ordering::*};
pub const IDLE: u64 = 0;
pub const RUNNING: u64 = 1;
pub const COMPLETE: u64 = 2;
pub const CANCELED: u64 = 3;
pub const UNAVAILABLE: u64 = 4;
pub const INVALIDATED: u64 = 5;
pub const MAX_DEPTH: usize = 32;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub state: u64,
    pub parent: u64,
    pub revision: u64,
    pub mount: u64,
    pub files: u64,
    pub bytes: u64,
}
impl View {
    pub const EMPTY: Self = Self {
        state: IDLE,
        parent: 0,
        revision: 0,
        mount: 0,
        files: 0,
        bytes: 0,
    };
    pub fn words(self) -> [u64; 6] {
        [
            self.state,
            self.parent,
            self.revision,
            self.mount,
            self.files,
            self.bytes,
        ]
    }
    pub fn canonical(self, parent: u64, revision: u64, mount: u64) -> bool {
        if self.state == IDLE {
            return self == Self::EMPTY;
        }
        self.state <= INVALIDATED
            && self.parent != 0
            && self.revision != 0
            && self.mount != 0
            && (self.state == COMPLETE || self.files == 0 && self.bytes == 0)
            && (!matches!(self.state, RUNNING | COMPLETE)
                || (self.parent, self.revision, self.mount) == (parent, revision, mount))
    }
    pub fn invalidate(&mut self, parent: u64, revision: u64, mount: u64) -> bool {
        if matches!(self.state, RUNNING | COMPLETE)
            && (self.parent, self.revision, self.mount) != (parent, revision, mount)
        {
            self.state = INVALIDATED;
            self.files = 0;
            self.bytes = 0;
            true
        } else {
            false
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Totals {
    pub files: u64,
    pub bytes: u64,
}
impl Totals {
    pub const ZERO: Self = Self { files: 0, bytes: 0 };
    pub fn add(self, other: Self) -> Result<Self, u64> {
        Ok(Self {
            files: self.files.checked_add(other.files).ok_or(UNAVAILABLE)?,
            bytes: self.bytes.checked_add(other.bytes).ok_or(UNAVAILABLE)?,
        })
    }
}

/// One producer (worker 0), one consumer (worker 1), one pending root child.
/// The producer also traverses alternate children itself. Full/empty waits
/// yield via the caller, including on a single CPU. No lock spans metadata I/O.
/// Atomic descriptor words avoid unsafe shared memory; ready Release/Acquire
/// publishes the descriptor and acknowledges its consumption before reuse.
pub struct WorkQueue {
    ready: AtomicU64,
    terminal: AtomicU64,
    entry: [AtomicU64; 4],
}
impl WorkQueue {
    pub const fn new() -> Self {
        Self {
            ready: AtomicU64::new(0),
            terminal: AtomicU64::new(RUNNING),
            entry: [const { AtomicU64::new(0) }; 4],
        }
    }
    /// Only the job owner may reset, after BOTH workers have joined and before
    /// publishing the next job. A canceled job never donates queued work.
    pub fn reset(&self) {
        self.ready.store(0, Relaxed);
        self.terminal.store(RUNNING, Relaxed);
    }
    fn fail(&self, error: u64) {
        let _ = self.terminal.fetch_update(AcqRel, Acquire, |state| {
            matches!(state, RUNNING | COMPLETE).then_some(error)
        });
    }
    fn check(&self, canceled: &mut impl FnMut() -> bool) -> Result<(), u64> {
        let state = self.terminal.load(Acquire);
        if state > COMPLETE {
            return Err(state);
        }
        if canceled() {
            return Err(CANCELED);
        }
        Ok(())
    }
    fn push(
        &self,
        entry: &FileReply,
        canceled: &mut impl FnMut() -> bool,
        wait: &mut impl FnMut() -> Result<(), u64>,
    ) -> Result<(), u64> {
        loop {
            self.check(canceled)?;
            if self.ready.load(Acquire) == 0 {
                break;
            }
            wait()?;
        }
        for (slot, value) in
            self.entry
                .iter()
                .zip([entry.object, entry.parent, entry.kind, entry.object_version])
        {
            slot.store(value, Relaxed);
        }
        self.ready.store(1, Release);
        Ok(())
    }
    fn pop(
        &self,
        canceled: &mut impl FnMut() -> bool,
        wait: &mut impl FnMut() -> Result<(), u64>,
    ) -> Result<Option<FileReply>, u64> {
        loop {
            self.check(canceled)?;
            // Acquire terminal BEFORE checking the slot: seeing COMPLETE must
            // also see the last published entry, even if producer just ended.
            let terminal = self.terminal.load(Acquire);
            if self.ready.load(Acquire) != 0 {
                let entry = FileReply {
                    object: self.entry[0].load(Relaxed),
                    parent: self.entry[1].load(Relaxed),
                    kind: self.entry[2].load(Relaxed),
                    object_version: self.entry[3].load(Relaxed),
                    status: status::OK,
                    ..FileReply::EMPTY
                };
                self.ready.store(0, Release);
                return Ok(Some(entry));
            }
            if terminal == COMPLETE {
                return Ok(None);
            }
            wait()?;
        }
    }
}
impl Default for WorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Worker 0 alone lists the root and sends alternate children to worker 1.
/// Each worker owns every descendant of the children assigned to it.
/// LIST cursors must advance, ancestry must be acyclic, all replies name one revision.
/// Errors never return partial totals as successful results.
pub fn enumerate(
    worker: usize,
    job: View,
    queue: &WorkQueue,
    mut canceled: impl FnMut() -> bool,
    mut wait: impl FnMut() -> Result<(), u64>,
    mut call: impl FnMut(FileRequest) -> FileReply,
) -> Result<Totals, u64> {
    let result = enumerate_inner(worker, job, queue, &mut canceled, &mut wait, &mut call);
    match result {
        Ok(_) if worker == 0 => {
            let _ = queue
                .terminal
                .compare_exchange(RUNNING, COMPLETE, Release, Relaxed);
        }
        Err(error) => queue.fail(error),
        _ => {}
    }
    result
}
fn enumerate_inner(
    worker: usize,
    job: View,
    queue: &WorkQueue,
    canceled: &mut impl FnMut() -> bool,
    wait: &mut impl FnMut() -> Result<(), u64>,
    call: &mut impl FnMut(FileRequest) -> FileReply,
) -> Result<Totals, u64> {
    if worker >= 2 || job.parent == 0 || job.revision == 0 || job.mount == 0 {
        return Err(UNAVAILABLE);
    }
    let mut stack = [(0u64, 0u64); MAX_DEPTH];
    stack[0].0 = job.parent;
    let mut depth = 0;
    let mut ordinal = 0usize;
    let mut sequence = 0u64;
    let mut total = Totals::ZERO;
    loop {
        queue.check(canceled)?;
        let reply = if worker == 1 && depth == 0 {
            match queue.pop(canceled, wait)? {
                Some(entry) => entry,
                None => return Ok(total),
            }
        } else {
            sequence = sequence.checked_add(1).ok_or(UNAVAILABLE)?;
            let mut request = FileRequest {
                operation: operation::LIST_V2,
                sequence,
                parent: stack[depth].0,
                object: stack[depth].1,
                catalog_revision: job.revision,
                mount_generation: job.mount,
                ..FileRequest::EMPTY
            };
            request.set_list_session((worker + 1) as u64);
            let reply = call(request);
            queue.check(canceled)?;
            check(&reply, sequence, job)?;
            if reply.status == status::END {
                if depth == 0 {
                    return Ok(total);
                }
                depth -= 1;
                continue;
            }
            reply
        };
        let next = if worker == 1 && depth == 0 {
            reply.object
        } else {
            reply.list_cursor().ok_or(UNAVAILABLE)?
        };
        if next == stack[depth].1
            || reply.object == 0
            || reply.parent != stack[depth].0
            || reply.object_version == 0
        {
            return Err(UNAVAILABLE);
        }
        stack[depth].1 = next;
        if depth == 0 && worker == 0 {
            let assigned = ordinal % 2 == 0;
            ordinal = ordinal.checked_add(1).ok_or(UNAVAILABLE)?;
            if !assigned {
                queue.push(&reply, canceled, wait)?;
                continue;
            }
        }
        match reply.kind {
            2 => {
                if depth + 1 == MAX_DEPTH || stack[..=depth].iter().any(|s| s.0 == reply.object) {
                    return Err(UNAVAILABLE);
                }
                depth += 1;
                stack[depth] = (reply.object, 0);
            }
            1 | 3 => {
                queue.check(canceled)?;
                sequence = sequence.checked_add(1).ok_or(UNAVAILABLE)?;
                let metadata = call(FileRequest {
                    operation: operation::STAT_V2,
                    sequence,
                    object: reply.object,
                    parent: reply.parent,
                    expected_version: reply.object_version,
                    catalog_revision: job.revision,
                    mount_generation: job.mount,
                    ..FileRequest::EMPTY
                });
                queue.check(canceled)?;
                check(&metadata, sequence, job)?;
                if metadata.object != reply.object
                    || metadata.object_version != reply.object_version
                    || metadata.kind != reply.kind
                {
                    return Err(INVALIDATED);
                }
                total = total.add(Totals {
                    files: 1,
                    bytes: metadata.logical_bytes().ok_or(UNAVAILABLE)?,
                })?;
            }
            _ => return Err(UNAVAILABLE),
        }
    }
}
fn check(reply: &FileReply, sequence: u64, job: View) -> Result<(), u64> {
    if matches!(reply.status, status::CONFLICT | status::STALE) {
        return Err(INVALIDATED);
    }
    if reply.version != VERSION
        || reply.sequence != sequence
        || !matches!(reply.status, status::OK | status::END)
    {
        return Err(UNAVAILABLE);
    }
    if reply.catalog_revision != job.revision || reply.mount_generation != job.mount {
        return Err(INVALIDATED);
    }
    Ok(())
}
