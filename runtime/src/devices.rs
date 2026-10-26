//! Redacted device facts and proposals. Proposals never grant execution or trust.
pub const SYSCALL: u64 = 401;
pub const VERSION: u64 = 1;
pub const READ: u64 = 0;
pub const PROPOSE_DISABLE: u64 = 1;
pub const PROPOSE_ENABLE: u64 = 2;
pub const PROPOSE_BINDING: u64 = 3;
pub const PROPOSE_ROLLBACK: u64 = 4;
pub const EVENTS: usize = 64;
pub mod state {
    pub const UNBOUND: u64 = 1;
    pub const STARTING: u64 = 2;
    pub const READY: u64 = 3;
    pub const FAULT: u64 = 4;
    pub const DISCONNECTED: u64 = 5;
    pub const DISABLED: u64 = 6;
}
pub mod flag {
    pub const PROTECTED: u64 = 1;
    pub const RESTART: u64 = 2;
    pub const CONFIRMATION: u64 = 4;
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub publisher: [u8; 32],
    pub package_id: [u8; 128],
    pub version: u64,
    pub index: u64,
    pub device: u64,
    pub connection: u64,
    pub parent: u64,
    pub bus: u64,
    pub state: u64,
    pub selected: u64,
    pub active: u64,
    pub endpoint: u64,
    pub restarts: u64,
    pub resources: u64,
    pub failure: u64,
    pub flags: u64,
    pub stamp: u64,
}
impl Record {
    pub const EMPTY: Self = Self {
        publisher: [0; 32],
        package_id: [0; 128],
        version: VERSION,
        index: 0,
        device: 0,
        connection: 0,
        parent: 0,
        bus: 0,
        state: 0,
        selected: 0,
        active: 0,
        endpoint: 0,
        restarts: 0,
        resources: 0,
        failure: 0,
        flags: 0,
        stamp: 0,
    };
    pub fn seal(mut self) -> Self {
        let mut hash = 0xcbf29ce484222325u64;
        for value in [
            self.version,
            self.index,
            self.device,
            self.connection,
            self.parent,
            self.bus,
            self.state,
            self.selected,
            self.active,
            self.endpoint,
            self.restarts,
            self.resources,
            self.failure,
            self.flags,
        ] {
            for byte in value.to_le_bytes() {
                hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
            }
        }
        for byte in self.publisher.iter().chain(self.package_id.iter()) {
            hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3);
        }
        self.stamp = hash;
        self
    }
    pub fn proposal(self, operation: u64, stamp: u64) -> Option<Self> {
        if !(PROPOSE_DISABLE..=PROPOSE_ROLLBACK).contains(&operation)
            || self.device == 0
            || self.stamp != stamp
        {
            return None;
        }
        Some(Self {
            flags: self.flags | flag::CONFIRMATION,
            ..self
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Bind,
    Start,
    Ready,
    Fault,
    Quarantine,
    Update,
    Rollback,
    Disable,
    Enable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub sequence: u64,
    pub device: u64,
    pub connection: u64,
    pub release: u64,
    pub kind: EventKind,
}
pub struct Log {
    entries: [Option<Event>; EVENTS],
    sequence: u64,
}
impl Log {
    pub const NEW: Self = Self {
        entries: [None; EVENTS],
        sequence: 0,
    };
    pub fn push(
        &mut self,
        device: u64,
        connection: u64,
        release: u64,
        kind: EventKind,
    ) -> Option<Event> {
        let sequence = self.sequence.checked_add(1)?;
        let e = Event {
            sequence,
            device,
            connection,
            release,
            kind,
        };
        self.entries[((sequence - 1) % EVENTS as u64) as usize] = Some(e);
        self.sequence = sequence;
        Some(e)
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn page(&self, after: u64, out: &mut [Option<Event>]) -> Option<usize> {
        if out.is_empty() || after > self.sequence || self.sequence - after > EVENTS as u64 {
            return None;
        }
        out.fill(None);
        let count = out.len().min((self.sequence - after) as usize);
        for (i, item) in out.iter_mut().take(count).enumerate() {
            *item = self.entries[((after + i as u64) % EVENTS as u64) as usize];
        }
        Some(count)
    }
}
/// READ and proposal responses are copied records, not capabilities.
#[cfg(target_os = "none")]
pub fn query(index: u64, operation: u64, stamp: u64) -> Result<Record, isize> {
    let mut record = Record::EMPTY;
    let result: isize;
    unsafe {
        core::arch::asm!("int 0x80", inlateout("rax") SYSCALL as isize => result,
        in("rdi") index, in("rsi") &mut record as *mut Record, in("rdx") operation,
        in("r8") stamp, in("r9") 0u64);
    }
    if result == 0 && record.version == VERSION {
        Ok(record)
    } else {
        Err(result)
    }
}
