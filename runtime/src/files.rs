//! Named-file requests carry identities and bounded bytes, never disk addresses.
//! The storage service derives authority from the caller and its live window.
pub const VERSION: u64 = 1;
pub const NAME_BYTES: usize = 64;
pub const CONTENT_BYTES: usize = 256;
pub const ROOT: u64 = 1;
pub mod operation {
    pub const LIST: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const SAVE: u64 = 3;
    pub const SAVE_AS: u64 = 4;
    pub const CREATE_DIRECTORY: u64 = 5;
    pub const RENAME: u64 = 6;
    pub const REMOVE: u64 = 7;
    // 8 and 9 are retired manual import/write-consent operations.
    /// The application reads only the file already bound to its own window.
    pub const READ_BOUND: u64 = 10;
    pub const STAT: u64 = 11;
    pub const MOUNT_INFO: u64 = 12;
    pub const OPEN_NAMED: u64 = 13;
    pub const CREATE_FILE: u64 = 14;
    /// Versioned logical length, independent of reply payload length.
    pub const STAT_V2: u64 = 15;
    /// Name-ordered enumeration with an opaque, revision-bound continuation.
    pub const LIST_V2: u64 = 16;
    pub const REPORT_INFO: u64 = 17;
    pub const REPORT_READ: u64 = 18;
    pub const REPORT_BEGIN: u64 = 19;
    pub const REPORT_CHUNK: u64 = 20;
    pub const REPORT_COMMIT: u64 = 21;
    pub const REPORT_ABORT: u64 = 22;
}
pub mod status {
    pub const OK: u64 = 0;
    pub const END: u64 = 1;
    pub const UNAVAILABLE: u64 = 2;
    pub const INVALID: u64 = 3;
    pub const PERMISSION: u64 = 4;
    pub const STALE: u64 = 5;
    pub const CONFLICT: u64 = 6;
    pub const NO_SPACE: u64 = 7;
    pub const BUSY: u64 = 8;
    pub const INCOMPATIBLE: u64 = 9;
    pub const CORRUPT: u64 = 10;
    pub const RECOVERY_REQUIRED: u64 = 11;
    pub const UNKNOWN_COMMIT: u64 = 12;
    pub const MIGRATION_REQUIRED: u64 = 13;
}

pub const fn private_operation(operation: u64) -> bool {
    matches!(
        operation,
        operation::READ_BOUND
            | operation::SAVE
            | operation::SAVE_AS
            | operation::MOUNT_INFO
            | operation::OPEN_NAMED
    )
}

pub const fn broker_operation(operation: u64) -> bool {
    matches!(
        operation,
        operation::LIST
            | operation::LIST_V2
            | operation::OPEN
            | operation::CREATE_DIRECTORY
            | operation::CREATE_FILE
            | operation::RENAME
            | operation::REMOVE
            | operation::STAT
            | operation::STAT_V2
    )
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileRequest {
    pub version: u64,
    pub operation: u64,
    pub sequence: u64,
    pub window_id: u64,
    pub window_generation: u64,
    pub object: u64,
    pub parent: u64,
    pub expected_version: u64,
    /// LIST binds continuation to this revision; zero starts a new enumeration.
    pub catalog_revision: u64,
    pub mount_generation: u64,
    pub name_len: u64,
    pub content_len: u64,
    pub name: [u8; NAME_BYTES],
    pub content: [u8; CONTENT_BYTES],
}
impl FileRequest {
    pub const EMPTY: Self = Self {
        version: VERSION,
        operation: 0,
        sequence: 0,
        window_id: 0,
        window_generation: 0,
        object: 0,
        parent: ROOT,
        expected_version: 0,
        catalog_revision: 0,
        mount_generation: 0,
        name_len: 0,
        content_len: 0,
        name: [0; NAME_BYTES],
        content: [0; CONTENT_BYTES],
    };
    pub fn canonical(&self) -> bool {
        if self.version != VERSION
            || self.sequence == 0
            || !(private_operation(self.operation)
                || broker_operation(self.operation)
                || report_operation(self.operation))
            || self.name_len > NAME_BYTES as u64
            || self.content_len > CONTENT_BYTES as u64
        {
            return false;
        }
        if report_operation(self.operation) {
            return self.name_len == 0
                && self.name.iter().all(|b| *b == 0)
                && self.parent < 16
                && (self.operation == operation::REPORT_CHUNK || self.content_len == 0)
                && self.content[self.content_len as usize..]
                    .iter()
                    .all(|b| *b == 0);
        }
        if self.name[self.name_len as usize..].iter().any(|b| *b != 0)
            || self.content[self.content_len as usize..]
                .iter()
                .any(|b| *b != 0)
        {
            return false;
        }
        let has_name = matches!(
            self.operation,
            operation::SAVE_AS
                | operation::CREATE_FILE
                | operation::CREATE_DIRECTORY
                | operation::RENAME
                | operation::OPEN_NAMED
        );
        if self.operation == operation::LIST_V2
            && self.content_len != 0
            && self.list_session().is_none()
        {
            return false;
        }
        let has_content = matches!(
            self.operation,
            operation::SAVE | operation::SAVE_AS | operation::LIST_V2
        );
        (has_name == (self.name_len != 0)) && (has_content || self.content_len == 0)
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileReply {
    pub version: u64,
    pub status: u64,
    pub sequence: u64,
    pub catalog_revision: u64,
    pub mount_generation: u64,
    pub object: u64,
    pub parent: u64,
    pub object_version: u64,
    /// 1=file, 2=directory. Zero denotes no entry.
    pub kind: u64,
    pub capacity: u64,
    pub name_len: u64,
    pub content_len: u64,
    pub name: [u8; NAME_BYTES],
    pub content: [u8; CONTENT_BYTES],
}
impl FileReply {
    pub const EMPTY: Self = Self {
        version: VERSION,
        status: status::UNAVAILABLE,
        sequence: 0,
        catalog_revision: 0,
        mount_generation: 0,
        object: 0,
        parent: 0,
        object_version: 0,
        kind: 0,
        capacity: 0,
        name_len: 0,
        content_len: 0,
        name: [0; NAME_BYTES],
        content: [0; CONTENT_BYTES],
    };
}
const _: () = assert!(core::mem::size_of::<FileRequest>() == 416);
const _: () = assert!(core::mem::size_of::<FileReply>() == 416);

/// STAT_V2 uses the existing envelope with a strictly versioned metadata body.
impl FileReply {
    pub fn set_logical_bytes(&mut self, bytes: u64) {
        self.content = [0; CONTENT_BYTES];
        self.content[..8].copy_from_slice(&2u64.to_le_bytes());
        self.content[8..16].copy_from_slice(&bytes.to_le_bytes());
        self.content_len = 16;
    }
    pub fn logical_bytes(&self) -> Option<u64> {
        (self.version == VERSION
            && self.status == status::OK
            && self.content_len == 16
            && matches!(self.kind, 1 | 2 | 3)
            && self.content[..8] == 2u64.to_le_bytes()
            && self.content[16..].iter().all(|b| *b == 0))
        .then(|| u64::from_le_bytes(self.content[8..16].try_into().unwrap()))
    }
}

/// LIST_V2 has a distinct body tag; it cannot be confused with STAT_V2 length.
impl FileReply {
    pub fn set_list_cursor(&mut self, cursor: u64) {
        self.content = [0; CONTENT_BYTES];
        self.content[..8].copy_from_slice(&3u64.to_le_bytes());
        self.content[8..16].copy_from_slice(&cursor.to_le_bytes());
        self.content_len = 16;
    }
    pub fn list_cursor(&self) -> Option<u64> {
        (self.version == VERSION
            && self.status == status::OK
            && self.content_len == 16
            && self.content[..8] == 3u64.to_le_bytes()
            && self.content[16..].iter().all(|b| *b == 0))
        .then(|| u64::from_le_bytes(self.content[8..16].try_into().unwrap()))
        .filter(|n| *n != 0)
    }
}

/// LIST_V2 may identify independent enumeration consumers in one window. The
/// tag and body are versioned; a zero/empty body is the foreground consumer.
impl FileRequest {
    pub fn set_list_session(&mut self, session: u64) {
        self.content = [0; CONTENT_BYTES];
        self.content[..8].copy_from_slice(&1u64.to_le_bytes());
        self.content[8..16].copy_from_slice(&session.to_le_bytes());
        self.content_len = 16;
    }
    pub fn list_session(&self) -> Option<u64> {
        if self.operation != operation::LIST_V2 {
            return None;
        }
        if self.content_len == 0 && self.content.iter().all(|b| *b == 0) {
            return Some(0);
        }
        (self.content_len == 16
            && self.content[..8] == 1u64.to_le_bytes()
            && self.content[16..].iter().all(|b| *b == 0))
        .then(|| u64::from_le_bytes(self.content[8..16].try_into().unwrap()))
    }
}

/// Metadata only: no content, mutations, mount changes or raw device access.
pub const fn metadata_operation(op: u64) -> bool {
    matches!(op, operation::LIST_V2 | operation::STAT_V2)
}

/// Restricted, versioned report slots; never an arbitrary path or file grant.
pub const fn report_operation(op: u64) -> bool {
    matches!(op, operation::REPORT_INFO..=operation::REPORT_ABORT)
}
