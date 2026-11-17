//! Platform-owned resources. CPU windows are disjoint from application images,
//! legacy DRM, Driver Kit and GOP. No physical address crosses this interface.
pub const SYSCALL: u64 = 411;
pub const BYTES: usize = 256;
pub const PAGE: u64 = 4096;
pub const MAX_PAGES: u64 = 32;
pub const HANDOFF_VERSION: u64 = 1;
/// Backend attests retirement of firmware GT work and coherent WB RAM access.
/// This is consumed only from the exclusively bound, signed backend instance.
pub const HANDOFF_RETIRED_COHERENT: u64 = 3;
pub const MMIO_START: u64 = 0x20_0000_0000;
pub const MMIO_BYTES: u64 = 32 * 1024 * 1024;
pub const APERTURE_START: u64 = 0x20_1000_0000;
pub const APERTURE_BYTES: u64 = 512 * 1024 * 1024;
pub const DMA_START: u64 = 0x21_0000_0000;
pub const DMA_BYTES: u64 = 512 * 1024 * 1024;
pub const WINDOWS: [(u64, u64); 3] = [
    (MMIO_START, MMIO_BYTES),
    (APERTURE_START, APERTURE_BYTES),
    (DMA_START, DMA_BYTES),
];
pub mod op {
    pub const INFO: u64 = 1;
    pub const MAP_BAR: u64 = 2;
    pub const HANDOFF: u64 = 3;
    pub const ALLOCATE: u64 = 4;
    pub const RELEASE: u64 = 5;
    pub const SYNC: u64 = 6;
    pub const IRQ: u64 = 7;
    pub const REVOKE: u64 = 8;
    pub const CONFIG: u64 = 9;
    /// The preserved firmware interval in this device's DMA address space.
    /// This does not grant CPU access to its backing.
    pub const RESERVATION: u64 = 10;
    /// Grant one immutable session identity CPU access to an owned DMA buffer.
    pub const GRANT: u64 = 11;
    /// The renderer installs/removes its own pre-authorized alias. No IOVA or
    /// MMIO permission is conveyed to it.
    pub const MAP_BUFFER: u64 = 12;
    pub const UNMAP_BUFFER: u64 = 13;
    pub const REVOKE_GRANT: u64 = 14;
    /// Backend-only immutable publication metadata; these operations grant no
    /// CPU mapping, IOVA, display or family-register authority.
    pub const PUBLISH: u64 = 15;
    pub const PUBLICATION_LOOKUP: u64 = 16;
    pub const PUBLICATION_DROP: u64 = 17;
    pub const PUBLICATION_ALIVE: u64 = 18;
    pub const DISPLAY: u64 = 19;
    /// Kernel-owned scanout, independently recoverable after backend death.
    pub const DISPLAY_CONTROL: u64 = 20;
}
pub mod display {
    pub const VERSION: u64 = 3;
    pub const PROBE: u64 = 0;
    pub const INITIALIZE: u64 = 1;
    pub const BEGIN: u64 = 2;
    pub const PAGES: u64 = 3;
    pub const BIND: u64 = 4;
    pub const PRESENT: u64 = 5;
    pub const POLL: u64 = 6;
    pub const RESTORE: u64 = 7;
    pub const RELEASE: u64 = 8;
    /// Wait for this exact accepted presentation. Kernel parks the caller;
    /// success still requires the hardware receipt, never elapsed time alone.
    pub const WAIT: u64 = 9;
    pub const ENQUEUE: u64 = 10;
    pub const WAIT_QUEUED: u64 = 11;
    /// Three token states observed under one display-owner lock; not a lease.
    pub const SNAPSHOT: u64 = 12;
    /// Private immutable cursor backing; pixels/position come only from kernel.
    pub const CURSOR_BACKING: u64 = 13;
}
pub mod cache {
    pub const WRITE_BACK: u64 = 0;
    pub const UNCACHED: u64 = 1;
    pub const WRITE_COMBINING: u64 = 2;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub attempt: u64,
    pub selection: u64,
    pub device: u64,
    pub connection: u64,
    pub digest: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub operation: u64,
    pub scope: Scope,
    pub args: [u64; 16],
}
impl Message {
    pub fn new(operation: u64, scope: Scope) -> Self {
        Self {
            operation,
            scope,
            args: [0; 16],
        }
    }
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"OSAGPL01");
        for (i, n) in [
            self.operation,
            self.scope.attempt,
            self.scope.selection,
            self.scope.device,
            self.scope.connection,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[48..80].copy_from_slice(&self.scope.digest);
        for (i, n) in self.args.into_iter().enumerate() {
            b[80 + i * 8..88 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, super::Error> {
        if b.len() != BYTES {
            return Err(super::Error::Length);
        }
        if &b[..8] != b"OSAGPL01" {
            return Err(super::Error::Version);
        }
        if b[208..].iter().any(|b| *b != 0) {
            return Err(super::Error::Reserved);
        }
        let n = |at| u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        if !(op::INFO..=op::DISPLAY_CONTROL).contains(&n(8)) {
            return Err(super::Error::Version);
        }
        let scope = Scope {
            attempt: n(16),
            selection: n(24),
            device: n(32),
            connection: n(40),
            digest: b[48..80].try_into().unwrap(),
        };
        if [
            scope.attempt,
            scope.selection,
            scope.device,
            scope.connection,
        ]
        .contains(&0)
            || scope.digest == [0; 32]
        {
            return Err(super::Error::Identity);
        }
        Ok(Self {
            operation: n(8),
            scope,
            args: core::array::from_fn(|i| n(80 + i * 8)),
        })
    }
    pub fn request(&self) -> Result<(), super::Error> {
        let used = match self.operation {
            op::DISPLAY_CONTROL => {
                let a = self.args;
                if a[0] > display::CURSOR_BACKING {
                    return Err(super::Error::Version);
                }
                if a[0] == display::CURSOR_BACKING {
                    if a[1] == 0 {
                        return Err(super::Error::Identity);
                    }
                    2
                } else if a[0] <= display::INITIALIZE {
                    1
                } else {
                    if a[1..5].contains(&0) || (a[0] != display::RESTORE && a[5] == 0) {
                        return Err(super::Error::Identity);
                    }
                    match a[0] {
                        display::SNAPSHOT => {
                            if a[5..8].contains(&0) || a[5] == a[6] || a[5] == a[7] || a[6] == a[7]
                            {
                                return Err(super::Error::Identity);
                            }
                            8
                        }
                        display::PAGES => {
                            if a[7] == 0 {
                                return Err(super::Error::Identity);
                            }
                            8
                        }
                        display::PRESENT
                        | display::WAIT
                        | display::ENQUEUE
                        | display::WAIT_QUEUED => {
                            if a[6] == 0 {
                                return Err(super::Error::Identity);
                            }
                            7
                        }
                        display::RESTORE => {
                            if a[5] != 0 {
                                return Err(super::Error::Reserved);
                            }
                            6
                        }
                        _ => 6,
                    }
                }
            }
            op::DISPLAY => {
                let a = self.args;
                if !(1..=5).contains(&a[0])
                    || a[1..5].contains(&0)
                    || (a[0] != 4 && a[5] == 0)
                    || (a[0] == 4 && a[5..8] != [0; 3])
                    || (a[0] == 3 && (a[6] == 0 || a[7] == 0))
                    || (a[0] != 3 && a[6..8] != [0; 2])
                {
                    return Err(super::Error::Identity);
                }
                8
            }
            op::PUBLISH => {
                if self.args[..7].contains(&0) || self.args[5] % PAGE != 0 {
                    return Err(super::Error::Identity);
                }
                7
            }
            op::PUBLICATION_LOOKUP | op::PUBLICATION_DROP => {
                if self.args[..5].contains(&0) {
                    return Err(super::Error::Identity);
                }
                5
            }
            op::PUBLICATION_ALIVE => {
                if self.args[0] == 0 {
                    return Err(super::Error::Identity);
                }
                1
            }
            op::INFO | op::REVOKE | op::RESERVATION => 0,
            op::MAP_BAR => {
                if !matches!(self.args[0], 0 | 2)
                    || self.args[1] & (PAGE - 1) != 0
                    || !(1..=MAX_PAGES).contains(&self.args[2])
                    || !matches!(self.args[3], cache::UNCACHED | cache::WRITE_COMBINING)
                    || (self.args[0] == 2 && self.args[3] != cache::WRITE_COMBINING)
                {
                    return Err(super::Error::AddressSpace);
                }
                4
            }
            op::HANDOFF => {
                if self.args[..2] != [HANDOFF_VERSION, HANDOFF_RETIRED_COHERENT] {
                    return Err(super::Error::Version);
                }
                2
            }
            op::ALLOCATE => {
                if !(1..=MAX_PAGES).contains(&self.args[0]) || self.args[1] > 1 {
                    return Err(super::Error::Budget);
                }
                2
            }
            op::RELEASE | op::SYNC | op::REVOKE_GRANT => {
                if self.args[0] == 0 {
                    return Err(super::Error::Identity);
                }
                1
            }
            op::GRANT | op::MAP_BUFFER | op::UNMAP_BUFFER => {
                if self.args[..5].contains(&0)
                    || (self.operation == op::GRANT && self.args[5] > 1)
                    || (self.operation == op::UNMAP_BUFFER && self.args[5] != 0)
                    || (self.operation == op::MAP_BUFFER
                        && self.args[5] != 0
                        && (self.args[5] % PAGE != 0
                            || self.args[5] < DMA_START
                            || self.args[5] >= DMA_START + DMA_BYTES))
                {
                    return Err(super::Error::Identity);
                }
                6
            }
            op::IRQ => {
                if self.args[0] > 1 {
                    return Err(super::Error::Identity);
                }
                1
            }
            // Read-only identity, command and graphics firmware observations.
            // No arbitrary config mutation; encoding belongs to the backend.
            op::CONFIG => {
                if !matches!(self.args[0], 0 | 4 | 8 | 0x50 | 0x5c) {
                    return Err(super::Error::AddressSpace);
                }
                1
            }
            _ => return Err(super::Error::Version),
        };
        if self.args[used..].iter().any(|v| *v != 0) {
            return Err(super::Error::Reserved);
        }
        Ok(())
    }
}

impl Scope {
    pub fn from_control(m: super::control::Message) -> Result<Self, super::Error> {
        if m.role != 4
            || m.attempt == 0
            || m.selection == 0
            || m.device == 0
            || m.connection == 0
            || m.digest == [0; 32]
        {
            return Err(super::Error::Identity);
        }
        Ok(Self {
            attempt: m.attempt,
            selection: m.selection,
            device: m.device,
            connection: m.connection,
            digest: m.digest,
        })
    }
}

/// Bounds check only. The broker must separately prove backing, ownership,
/// memory type, permissions and process generation before changing page tables.
pub fn contains(start: u64, bytes: u64) -> bool {
    bytes != 0
        && (start | bytes) & (PAGE - 1) == 0
        && start.checked_add(bytes).is_some_and(|end| {
            WINDOWS
                .iter()
                .any(|&(base, len)| start >= base && end <= base + len)
        })
}
