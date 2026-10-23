//! Bounded metadata-only file manager recipe, separate from Document content.
use crate::files::{FileReply, NAME_BYTES};
pub const APP_ID: u64 = 6;
pub const START_KIND: u16 = 3;
pub const ELEMENT_ID: u64 = 0x3107;
pub const FILE_OPEN_ELEMENT_ID: u64 = 0x3108;
/// Bounded viewport cache, independent of the directory's total size.
pub const ROWS: usize = 28;
pub const ITEM_HEIGHT: u32 = 36;
pub const MAX_ITEMS: u64 = (osaiku_ui::virtual_list::MAX_EXTENT / ITEM_HEIGHT) as u64;
pub fn cache_first(scroll: u32, total: u64) -> u64 {
    u64::from(scroll / ITEM_HEIGHT)
        .saturating_sub(2)
        .min(total.saturating_sub(ROWS as u64))
}
pub const MAGIC: u64 = 0x4f53414946494c31;
pub const EVENT_MAGIC: u64 = 0x4f53414946455631;
/// Eleven words, then the context click's point (`Event::point`).
pub const EVENT_BYTES: usize = 96;
pub mod feedback {
    pub const NONE: u64 = 0;
    pub const FOLDER_CREATED: u64 = 1;
    // Feedback values 2 and 3 are retired.
    pub const FILE_CREATED: u64 = 4;
    pub const ARTIFACT_SELECTED: u64 = 5;
}
pub mod action {
    pub const OPEN: u64 = 1;
    pub const ENTER: u64 = 2;
    pub const UP: u64 = 3;
    pub const REFRESH: u64 = 4;
    pub const NEXT: u64 = 5;
    // Actions 6 and 7 are retired.
    pub const RESULT: u64 = 8;
    pub const NEW_FOLDER: u64 = 9;
    pub const NEW_FILE: u64 = 10;
    pub const HOME: u64 = 11;
    pub const INSPECT: u64 = 12;
    pub const SUMMARY: u64 = 13;
    /// Kernel inventory notice; does not navigate or restart directory listing.
    pub const DEVICES: u64 = 18;
    // 14..=19 belong to browser/transfer notices, 20..=25 are retired USB export IDs.
    // These IDs share the kernel dispatcher: they must never overlap.
    pub const SELECT: u64 = 40;
    pub const CONTEXT: u64 = 41;
    pub const CUT: u64 = 42;
    pub const COPY: u64 = 43;
    pub const PASTE: u64 = 44;
    pub const RENAME: u64 = 45;
    pub const REMOVE: u64 = 46;
    pub const COMMIT_RENAME: u64 = 47;
    pub const DISMISS: u64 = 48;
}
// Compile-time protocol guard, including each command rather than assuming
// future additions will preserve the numeric ordering of either family.
const _: () = {
    let commands = [
        action::SELECT,
        action::CONTEXT,
        action::CUT,
        action::COPY,
        action::PASTE,
        action::RENAME,
        action::REMOVE,
        action::COMMIT_RENAME,
        action::DISMISS,
    ];
    let mut i = 0;
    while i < commands.len() {
        assert!(commands[i] > action::DEVICES && commands[i] > crate::file_browser::COPY_COMPLETE);
        assert!(
            commands[i] != crate::file_browser::REPLACE
                && commands[i] != crate::file_browser::KEEP_BOTH
                && commands[i] != crate::file_browser::SKIP
        );
        assert!(commands[i] != crate::file_export::SAVE);
        let mut j = 0;
        while j < i {
            assert!(commands[i] != commands[j]);
            j += 1;
        }
        i += 1;
    }
};
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub magic: u64,
    pub sequence: u64,
    pub window_id: u64,
    pub window_generation: u64,
    pub owner: u64,
    pub action: u64,
    pub object: u64,
    pub version: u64,
    pub parent: u64,
    pub view_hash: u64,
    pub mount_generation: u64,
    /// `action::CONTEXT` only: where the secondary click landed, in the
    /// window's content pixels, as `x | y << 32` (each at most 4096). The
    /// menu opens there; zero elsewhere.
    pub point: u64,
}
/// Pack a content point for `Event::point` / `View::anchor`.
pub fn point(x: u32, y: u32) -> u64 {
    u64::from(x.min(4096)) | (u64::from(y.min(4096)) << 32)
}
/// Unpack `Event::point` / `View::anchor`.
pub fn point_xy(point: u64) -> (u32, u32) {
    ((point & 0xffff_ffff) as u32, (point >> 32) as u32)
}
fn point_canonical(point: u64) -> bool {
    let (x, y) = point_xy(point);
    x <= 4096 && y <= 4096
}
impl Event {
    pub const EMPTY: Self = Self {
        magic: 0,
        sequence: 0,
        window_id: 0,
        window_generation: 0,
        owner: 0,
        action: 0,
        object: 0,
        version: 0,
        parent: 0,
        view_hash: 0,
        mount_generation: 0,
        point: 0,
    };
    pub fn encode(self) -> [u8; EVENT_BYTES] {
        let mut bytes = [0; EVENT_BYTES];
        for (word, out) in [
            self.magic,
            self.sequence,
            self.window_id,
            self.window_generation,
            self.owner,
            self.action,
            self.object,
            self.version,
            self.parent,
            self.view_hash,
            self.mount_generation,
            self.point,
        ]
        .into_iter()
        .zip(bytes.chunks_exact_mut(8))
        {
            out.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != EVENT_BYTES {
            return None;
        }
        let word = |index: usize| {
            u64::from_le_bytes(bytes[index * 8..(index + 1) * 8].try_into().unwrap())
        };
        let event = Self {
            magic: word(0),
            sequence: word(1),
            window_id: word(2),
            window_generation: word(3),
            owner: word(4),
            action: word(5),
            object: word(6),
            version: word(7),
            parent: word(8),
            view_hash: word(9),
            mount_generation: word(10),
            point: word(11),
        };
        (event.magic == EVENT_MAGIC
            && event.sequence != 0
            && event.window_id != 0
            && event.window_generation != 0
            && crate::process::ProcessEndpoint::decode(event.owner).names_instance()
            && ((action::OPEN..=action::SUMMARY).contains(&event.action)
                || event.action == action::DEVICES
                || event.action == crate::file_broker::INPUT
                || (action::SELECT..=action::DISMISS).contains(&event.action))
            && !matches!(event.action, 6 | 7)
            && (event.point == 0
                || (event.action == action::CONTEXT && point_canonical(event.point)))
            && event.view_hash != 0)
            .then_some(event)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub object: u64,
    pub version: u64,
    pub kind: u64,
    pub name_len: u64,
    pub name: [u8; NAME_BYTES],
}
impl Row {
    pub const EMPTY: Self = Self {
        object: 0,
        version: 0,
        kind: 0,
        name_len: 0,
        name: [0; NAME_BYTES],
    };
    pub fn from_reply(reply: &FileReply) -> Self {
        Self {
            object: reply.object,
            version: reply.object_version,
            kind: reply.kind,
            name_len: reply.name_len,
            name: reply.name,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    /// 0 closed, 1 context actions, 2 editing a name.
    pub context: u64,
    /// Where the open menu is anchored (`point(x, y)`, content pixels).
    pub anchor: u64,
    pub edit_len: u64,
    pub edit_name: [u8; NAME_BYTES],
    pub devices: crate::file_browser::Devices,
    pub location: crate::file_browser::Location,
    pub magic: u64,
    /// Application-owned presentation generation, independent of storage revisions.
    pub generation: u64,
    pub parent: u64,
    /// Directory revision; for a copy dialog, the kernel-owned transfer ID.
    pub revision: u64,
    /// Directory mount; for a copy dialog, the kernel-owned transfer revision.
    pub mount_generation: u64,
    pub status: u64,
    pub count: u64,
    pub first: u64,
    pub total: u64,
    pub selected: u64,
    pub more: u64,
    pub feedback: u64,
    pub summary: crate::directory_summary::View,
    pub export: crate::file_export::View,
    pub rows: [Row; ROWS],
}
impl View {
    pub const EMPTY: Self = Self {
        context: 0,
        anchor: 0,
        edit_len: 0,
        edit_name: [0; NAME_BYTES],
        location: crate::file_browser::Location::EMPTY,
        devices: crate::file_browser::Devices::EMPTY,
        magic: 0,
        generation: 0,
        parent: 0,
        revision: 0,
        mount_generation: 0,
        status: 0,
        count: 0,
        first: 0,
        total: 0,
        selected: 0,
        more: 0,
        feedback: feedback::NONE,
        summary: crate::directory_summary::View::EMPTY,
        export: crate::file_export::View::EMPTY,
        rows: [Row::EMPTY; ROWS],
    };
    pub fn canonical(&self) -> bool {
        if self.magic != MAGIC
            || self.context > 2
            || !point_canonical(self.anchor)
            || self.edit_len > NAME_BYTES as u64
            || self.edit_name[self.edit_len as usize..]
                .iter()
                .any(|b| *b != 0)
            || core::str::from_utf8(&self.edit_name[..self.edit_len as usize]).is_err()
            || (self.context != 0 && self.selected == 0)
            || !self.location.canonical()
            || !self.devices.canonical()
            || self.generation == 0
            || self.parent == 0
            || self.count > ROWS as u64
            || self.total > MAX_ITEMS
            || self.first > self.total
            || self.count > self.total - self.first
            || self.status > crate::files::status::MIGRATION_REQUIRED
            || self.more > 1
            || !matches!(
                self.feedback,
                feedback::NONE
                    | feedback::FOLDER_CREATED
                    | feedback::FILE_CREATED
                    | feedback::ARTIFACT_SELECTED
            )
            || (self.feedback != feedback::NONE
                && !matches!(
                    self.status,
                    crate::files::status::OK | crate::files::status::END
                ))
            || !self
                .summary
                .canonical(self.parent, self.revision, self.mount_generation)
            || !self.export.canonical()
        {
            return false;
        }
        for (index, row) in self.rows[..self.count as usize].iter().enumerate() {
            if row.object == 0
                || self.rows[..index]
                    .iter()
                    .any(|previous| previous.object == row.object)
                || row.version == 0
                || !matches!(row.kind, 1 | 2 | 3)
                || row.name_len == 0
                || row.name_len > NAME_BYTES as u64
                || row.name[row.name_len as usize..].iter().any(|b| *b != 0)
                || core::str::from_utf8(&row.name[..row.name_len as usize]).is_err()
            {
                return false;
            }
        }
        self.rows[self.count as usize..]
            .iter()
            .all(|row| *row == Row::EMPTY)
            && (self.selected == 0
                || self.rows[..self.count as usize]
                    .iter()
                    .any(|row| row.object == self.selected))
    }
    pub fn content_hash(&self) -> u64 {
        const AUDIT_JOURNAL_FNV1A_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const AUDIT_JOURNAL_FNV1A_PRIME: u64 = 0x100_0000_01b3;
        let mut hash = AUDIT_JOURNAL_FNV1A_OFFSET;
        let mut fold = |bytes: &[u8]| {
            for byte in bytes {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(AUDIT_JOURNAL_FNV1A_PRIME);
            }
        };
        fold(b"osaiku.file-manager.content.v6");
        fold(&self.context.to_le_bytes());
        fold(&self.anchor.to_le_bytes());
        fold(&self.edit_len.to_le_bytes());
        fold(&self.edit_name);
        fold(&self.devices.revision.to_le_bytes());
        fold(&self.devices.ports.to_le_bytes());
        fold(&self.devices.preparation.to_le_bytes());
        for word in self.location.words() {
            fold(&word.to_le_bytes());
        }
        for word in self.summary.words() {
            fold(&word.to_le_bytes());
        }
        for word in self.export.words() {
            fold(&word.to_le_bytes());
        }
        fold(&self.export.reserved_label);
        fold(&self.export.filename);
        fold(&self.export.diagnostic);
        for word in [
            self.magic,
            self.generation,
            self.parent,
            self.revision,
            self.mount_generation,
            self.status,
            self.feedback,
            self.count,
            self.first,
            self.total,
            self.selected,
            self.more,
        ] {
            fold(&word.to_le_bytes());
        }
        for row in &self.rows {
            for word in [row.object, row.version, row.kind, row.name_len] {
                fold(&word.to_le_bytes());
            }
            fold(&row.name);
        }
        hash.max(1)
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ContentFrame {
    pub map: crate::file_broker::Map,
    pub frame: crate::window::UiDocumentWindowContentFrame,
    pub view: View,
}
