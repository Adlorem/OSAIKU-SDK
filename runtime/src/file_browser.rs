//! File Manager's removable-media binding. No raw block or arbitrary copy API.
pub const SYSCALL: u64 = 392;
pub const SNAPSHOT: u64 = 1;
// Operation 2 (automatic USB binding) is retired.
// Event 14 (USB automatic window opening) is retired.
pub const COPY_DIALOG: u64 = 15;
/// Kernel-to-desktop notice: the bound copy dialog completed successfully.
pub const COPY_COMPLETE: u64 = 19;
pub const BIND_COPY: u64 = 3;
pub const CANCEL: u64 = 16;
pub const DEVICES: u64 = 4;
pub const SELECT_USB: u64 = 17;
pub const VIEWPORT: u64 = 18;
pub const REPLACE: u64 = 49;
pub const KEEP_BOTH: u64 = 50;
pub const SKIP: u64 = 51;
pub const CONFLICT: u64 = 5;
pub const CONFLICT_NO_REPLACE: u64 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Replace,
    KeepBoth,
    Skip,
}
impl Choice {
    pub fn from_action(action: u64) -> Option<Self> {
        match action {
            REPLACE => Some(Self::Replace),
            KEEP_BOTH => Some(Self::KeepBoth),
            SKIP => Some(Self::Skip),
            _ => None,
        }
    }
}
pub fn conflict(transfer: u64) -> bool {
    matches!(transfer, CONFLICT | CONFLICT_NO_REPLACE)
}

/// Compact recognized FAT32 inventory. Selection names a port in this exact
/// revision; the kernel retains the attachment identities and I/O authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Devices {
    pub revision: u64,
    pub ports: u32,
    /// Installed volume: 0 idle/ready, 1 physical claims, 2 namespace, 3 failed.
    pub preparation: u32,
}
impl Devices {
    pub const EMPTY: Self = Self {
        revision: 0,
        ports: 0,
        preparation: 0,
    };
    pub fn canonical(self) -> bool {
        (self.ports == 0 || self.revision != 0) && self.preparation <= 3
    }
    pub fn contains(self, port: u64) -> bool {
        (1..=32).contains(&port) && self.canonical() && self.ports & (1 << (port - 1)) != 0
    }
}

/// Public location and copy progress, shared with the window's published rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Location {
    /// Zero is the installed Documents volume; 1..=32 identifies a USB port.
    pub port: u64,
    pub attachment: u64,
    /// 0 idle, 1 copying, 2 complete, 3 cancelled, 4 failed,
    /// 5 awaiting a name-conflict decision, 6 awaiting with Replace unavailable.
    pub transfer: u64,
    pub copied: u64,
    pub total: u64,
    pub dialog: u64,
    pub elapsed_ms: u64,
    /// Copy dialogs: where the item comes from and goes to, as `port`
    /// (0 Documents, 1..=32 a USB port).
    pub source: u64,
    pub target: u64,
}
impl Location {
    pub const EMPTY: Self = Self {
        port: 0,
        attachment: 0,
        transfer: 0,
        copied: 0,
        total: 0,
        dialog: 0,
        elapsed_ms: 0,
        source: 0,
        target: 0,
    };
    pub fn canonical(self) -> bool {
        self.port <= 32
            && (self.port == 0) == (self.attachment == 0)
            && self.transfer <= CONFLICT_NO_REPLACE
            && self.dialog <= 1
            && self.copied <= self.total
            && self.source <= 32
            && self.target <= 32
            && (self.dialog == 1 || (self.source == 0 && self.target == 0))
    }
    pub fn words(self) -> [u64; 9] {
        [
            self.port,
            self.attachment,
            self.transfer,
            self.copied,
            self.total,
            self.dialog,
            self.elapsed_ms,
            self.source,
            self.target,
        ]
    }
}
