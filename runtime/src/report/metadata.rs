//! Bounded, read-only OSAIFS metadata queries. Logical sizes are not allocated
//! disk usage. Categories are filename rules, not content inspection or MIME.
use crate::files::{operation as op, status, FileReply, FileRequest, ROOT};
pub const CAPACITY: usize = 256;
pub const DIRECTORIES: usize = 64;
pub const CATEGORIES: usize = 6;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod native;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub use native::Query;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Running,
    Complete,
    Partial,
    Canceled,
    Stale,
    Unavailable,
    Invalid,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub window: u64,
    pub incarnation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub object: u64,
    pub version: u64,
    pub parent: u64,
    pub bytes: Option<u64>,
    pub name: [u8; 64],
    pub name_len: usize,
}
impl Entry {
    pub const EMPTY: Self = Self {
        object: 0,
        version: 0,
        parent: 0,
        bytes: None,
        name: [0; 64],
        name_len: 0,
    };
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
    pub fn category(&self) -> usize {
        category(self.name())
    }
}
pub const CATEGORY_NAMES: [&str; CATEGORIES] = [
    "Dokumenty",
    "Obrazy",
    "Multimedia",
    "Archiwa",
    "Kod",
    "Inne",
];
pub fn category(name: &str) -> usize {
    let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    for (index, extensions) in [
        &["txt", "md", "pdf", "doc", "docx", "csv", "ods", "xlsx"][..],
        &["png", "jpg", "jpeg", "gif", "webp", "svg"][..],
        &["mp3", "wav", "ogg", "mp4", "webm", "mkv"][..],
        &["zip", "tar", "gz", "7z", "xz"][..],
        &[
            "rs", "c", "h", "cpp", "py", "js", "ts", "html", "css", "json", "toml",
        ][..],
    ]
    .iter()
    .enumerate()
    {
        if extensions
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext))
        {
            return index;
        }
    }
    5
}

/// One outstanding LIST/STAT at a time. Each step makes at most one service call;
/// callers may cancel between steps. All accepted values share mount/revision.
pub struct Scan {
    pub state: State,
    pub scope: Scope,
    pub revision: u64,
    pub mount: u64,
    pub entries: [Entry; CAPACITY],
    pub len: usize,
    pub omitted: u64,
    directories: [u64; DIRECTORIES],
    dir_count: usize,
    dir_index: usize,
    cursor: u64,
    sequence: u64,
    pending: Option<Entry>,
    retries: u16,
    steps: usize,
    partial: bool,
}
impl Scan {
    pub const EMPTY: Self = Self {
        state: State::Idle,
        scope: Scope {
            window: 0,
            incarnation: 0,
        },
        revision: 0,
        mount: 0,
        entries: [Entry::EMPTY; CAPACITY],
        len: 0,
        omitted: 0,
        directories: [0; DIRECTORIES],
        dir_count: 0,
        dir_index: 0,
        cursor: 0,
        sequence: 0,
        pending: None,
        retries: 0,
        steps: 0,
        partial: false,
    };
    pub fn start(&mut self, scope: Scope) {
        self.clear(State::Running);
        self.scope = scope;
        self.revision = 0;
        self.mount = 0;
        self.directories.fill(0);
        self.directories[0] = ROOT;
        self.dir_count = 1;
        self.dir_index = 0;
        self.cursor = 0;
        self.sequence = 0;
        self.retries = 0;
        self.steps = 0;
        self.partial = false;
        if scope.window == 0 || scope.incarnation == 0 {
            self.state = State::Invalid;
        }
    }
    fn clear(&mut self, state: State) {
        self.entries.fill(Entry::EMPTY);
        self.len = 0;
        self.omitted = 0;
        self.pending = None;
        self.state = state;
    }
    pub fn cancel(&mut self) {
        self.clear(State::Canceled);
    }
    pub fn copy_result(&self, result: &mut Self) {
        result.clear(self.state);
        result.scope = self.scope;
        result.revision = self.revision;
        result.mount = self.mount;
        result.entries.copy_from_slice(&self.entries);
        result.len = self.len;
        result.omitted = self.omitted;
    }
    pub fn step(&mut self, mut fetch: impl FnMut(&FileRequest) -> FileReply) {
        if self.state != State::Running {
            return;
        }
        if self.steps >= 2048 {
            self.state = State::Partial;
            return;
        }
        if self.dir_index == self.dir_count {
            self.state = if self.partial {
                State::Partial
            } else {
                State::Complete
            };
            return;
        }
        self.steps += 1;
        self.sequence += 1;
        let mut request = FileRequest {
            operation: if self.pending.is_some() {
                op::STAT_V2
            } else {
                op::LIST_V2
            },
            sequence: self.sequence,
            window_id: self.scope.window,
            window_generation: self.scope.incarnation,
            parent: self.directories[self.dir_index],
            object: self.cursor,
            catalog_revision: self.revision,
            mount_generation: self.mount,
            ..FileRequest::EMPTY
        };
        if let Some(entry) = self.pending {
            request.object = entry.object;
            request.expected_version = entry.version;
        } else {
            request.set_list_session(3);
        }
        let reply = fetch(&request);
        if reply.sequence != request.sequence || reply.version != crate::files::VERSION {
            self.clear(State::Invalid);
            return;
        }
        if reply.status == status::BUSY {
            self.retries += 1;
            if self.retries >= 100 {
                self.state = if self.len == 0 {
                    State::Unavailable
                } else {
                    State::Partial
                };
            }
            return;
        }
        self.retries = 0;
        if matches!(reply.status, status::STALE | status::CONFLICT) {
            self.clear(State::Stale);
            return;
        }
        if !matches!(reply.status, status::OK | status::END) {
            self.state = if self.len == 0 {
                State::Unavailable
            } else {
                State::Partial
            };
            return;
        }
        if reply.mount_generation == 0 || reply.catalog_revision == 0 {
            self.clear(State::Invalid);
            return;
        }
        if self.mount != 0
            && (self.mount != reply.mount_generation || self.revision != reply.catalog_revision)
        {
            self.clear(State::Stale);
            return;
        }
        self.mount = reply.mount_generation;
        self.revision = reply.catalog_revision;
        if let Some(mut entry) = self.pending.take() {
            if reply.status != status::OK
                || reply.object != entry.object
                || reply.object_version != entry.version
                || reply.parent != entry.parent
                || reply.kind != 1
            {
                self.clear(State::Invalid);
                return;
            }
            entry.bytes = reply.logical_bytes();
            self.partial |= entry.bytes.is_none();
            self.entries[self.len] = entry;
            self.len += 1;
            return;
        }
        if reply.status == status::END {
            self.dir_index += 1;
            self.cursor = 0;
            return;
        }
        if reply.parent != request.parent
            || reply.object == 0
            || reply.object_version == 0
            || reply.name_len == 0
            || reply.name_len > 64
            || reply.name[reply.name_len as usize..]
                .iter()
                .any(|b| *b != 0)
            || core::str::from_utf8(&reply.name[..reply.name_len as usize]).is_err()
        {
            self.clear(State::Invalid);
            return;
        }
        let Some(next) = reply.list_cursor().filter(|c| *c != self.cursor) else {
            self.clear(State::Invalid);
            return;
        };
        self.cursor = next;
        if reply.kind == 2 {
            if self.directories[..self.dir_count].contains(&reply.object) {
                self.clear(State::Invalid);
                return;
            }
            if self.dir_count == DIRECTORIES {
                self.partial = true;
                self.omitted += 1;
                return;
            }
            self.directories[self.dir_count] = reply.object;
            self.dir_count += 1;
        } else if reply.kind == 1 {
            if self.entries[..self.len]
                .iter()
                .any(|e| e.object == reply.object)
            {
                self.clear(State::Invalid);
                return;
            }
            if self.len == CAPACITY {
                self.omitted += 1;
                self.state = State::Partial;
                return;
            }
            self.pending = Some(Entry {
                object: reply.object,
                version: reply.object_version,
                parent: reply.parent,
                name: reply.name,
                name_len: reply.name_len as usize,
                bytes: None,
            });
        } else {
            self.partial = true;
            self.omitted += 1;
        }
    }
    pub fn totals(&self) -> [Option<u64>; CATEGORIES] {
        let mut totals = [Some(0u64); CATEGORIES];
        for entry in &self.entries[..self.len] {
            let slot = &mut totals[entry.category()];
            *slot = slot.and_then(|sum| entry.bytes.and_then(|bytes| sum.checked_add(bytes)));
        }
        totals
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Name,
    Size,
}
/// Stable keys are object IDs within one mount. Presentation ordinals are never
/// retained as selection keys; callers bind an Order to its accepted frame.
pub struct Order {
    pub indices: [usize; CAPACITY],
    pub len: usize,
}
impl Order {
    pub const EMPTY: Self = Self {
        indices: [0; CAPACITY],
        len: 0,
    };
    pub fn rebuild(
        &mut self,
        scan: &Scan,
        category: Option<usize>,
        text: &str,
        sort: Sort,
        descending: bool,
    ) {
        self.len = 0;
        for (i, entry) in scan.entries[..scan.len].iter().enumerate() {
            if category.is_some_and(|c| entry.category() != c) || !contains(entry.name(), text) {
                continue;
            }
            self.indices[self.len] = i;
            self.len += 1;
        }
        // In-place O(n log n), without allocation. Object IDs break ties, so
        // equal names/sizes retain deterministic order without a stable sort.
        self.indices[..self.len].sort_unstable_by(|a, b| {
            let (a, b) = (&scan.entries[*a], &scan.entries[*b]);
            let order = match sort {
                Sort::Name => a.name().cmp(b.name()),
                Sort::Size => a.bytes.cmp(&b.bytes),
            }
            .then(a.object.cmp(&b.object));
            if descending {
                order.reverse()
            } else {
                order
            }
        });
    }
    pub fn index_of(&self, scan: &Scan, object: u64) -> Option<usize> {
        self.indices[..self.len]
            .iter()
            .position(|i| scan.entries[*i].object == object)
    }
}
fn contains(haystack: &str, needle: &str) -> bool {
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}
