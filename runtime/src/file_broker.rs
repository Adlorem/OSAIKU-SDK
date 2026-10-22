//! Typed browser operations. A map is a declaration, not authority. The OS
//! binds a one-shot receipt to an actual presented physical input and profile.
use osaiku_ui::{input::Event, Document, Identity};
pub const TAKE: u64 = 26;
pub const EXECUTE: u64 = 27;
pub const INPUT: u64 = 52;
/// EXECUTE accepted a local state transition, or completed/queued server work.
pub const LOCAL: isize = 0;
pub const HANDLED: isize = 1;
pub const CAPACITY: usize = 96;
pub const SOURCE: u32 = 1;
pub const DESTINATION: u32 = 2;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub element: u32,
    pub flags: u32,
    pub operations: [u64; 3],
    pub object: u64,
    pub version: u64,
}
impl Binding {
    pub const EMPTY: Self = Self {
        element: 0,
        flags: 0,
        operations: [0; 3],
        object: 0,
        version: 0,
    };
    pub fn allows(&self, operation: u64, object: u64, version: u64) -> bool {
        operation != 0
            && self.operations.contains(&operation)
            && self.object == object
            && self.version == version
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Map {
    pub len: u64,
    pub entries: [Binding; CAPACITY],
}
impl Map {
    pub const EMPTY: Self = Self {
        len: 0,
        entries: [Binding::EMPTY; CAPACITY],
    };
    pub fn get(&self, element: u32) -> Option<Binding> {
        self.entries
            .get(..self.len as usize)?
            .iter()
            .find(|b| b.element == element)
            .copied()
    }
    pub fn push(&mut self, b: Binding) -> bool {
        if b.element == 0 || self.len as usize >= CAPACITY || self.get(b.element).is_some() {
            return false;
        }
        self.entries[self.len as usize] = b;
        self.len += 1;
        true
    }
    pub fn valid(&self, document: &Document) -> bool {
        if self.len as usize > CAPACITY {
            return false;
        }
        self.entries[self.len as usize..]
            .iter()
            .all(|b| *b == Binding::EMPTY)
            && self.entries[..self.len as usize]
                .iter()
                .enumerate()
                .all(|(i, b)| {
                    b.element != 0
                        && b.flags & !(SOURCE | DESTINATION) == 0
                        && !self.entries[..i].iter().any(|a| a.element == b.element)
                        && document.nodes[..document.len as usize]
                            .iter()
                            .any(|n| n.id == b.element)
                })
    }
    pub fn valid_for(&self, view: &crate::file_manager::View, document: &Document) -> bool {
        use crate::{file_browser as b, file_export as e, file_manager::action as a};
        self.valid(document)
            && view.canonical()
            && self.entries[..self.len as usize].iter().all(|binding| {
                let row = view.rows[..view.count as usize]
                    .iter()
                    .find(|r| r.object == binding.object && r.version == binding.version);
                if binding.flags & SOURCE != 0 && row.is_none() {
                    return false;
                }
                if binding.flags & DESTINATION != 0
                    && binding.object != 0
                    && row.is_none_or(|r| r.kind != 2)
                {
                    return false;
                }
                binding.operations.iter().all(|op| match *op {
                    0 => true,
                    a::OPEN => row.is_some_and(|r| r.kind == 1),
                    a::ENTER => row.is_some_and(|r| r.kind == 2),
                    a::INSPECT => row.is_some_and(|r| r.kind == 3),
                    a::SELECT
                    | a::CONTEXT
                    | a::CUT
                    | a::COPY
                    | a::RENAME
                    | a::REMOVE
                    | a::COMMIT_RENAME
                    | a::DISMISS => row.is_some(),
                    a::UP
                    | a::REFRESH
                    | a::NEXT
                    | a::NEW_FOLDER
                    | a::NEW_FILE
                    | a::HOME
                    | a::SUMMARY
                    | a::PASTE => binding.object == 0 && binding.version == 0,
                    b::CANCEL => {
                        view.location.dialog != 0 && binding.object == 0 && binding.version == 0
                    }
                    b::SELECT_USB => {
                        view.devices.contains(binding.object)
                            && binding.version == view.devices.revision
                    }
                    b::REPLACE | b::KEEP_BOTH | b::SKIP => {
                        view.location.dialog != 0
                            && b::conflict(view.location.transfer)
                            && (*op != b::REPLACE || view.location.transfer == b::CONFLICT)
                            && binding.object == view.revision
                            && binding.version == view.mount_generation
                    }
                    e::SCAN..=e::ENTER => binding.object == 0 && binding.version == 0,
                    _ => false,
                })
            })
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub token: u64,
    pub event: Event,
    pub binding: Binding,
    /// 0 keyboard, 1 primary pointer, 2 secondary pointer.
    pub kind: u64,
    pub point: u64,
    pub time_ms: u64,
    /// Changes when a captured pointer gesture is cancelled or becomes a drag.
    pub gesture_epoch: u64,
}
impl Receipt {
    pub const EMPTY: Self = Self {
        token: 0,
        event: Event::EMPTY,
        binding: Binding::EMPTY,
        kind: 0,
        point: 0,
        time_ms: 0,
        gesture_epoch: 0,
    };
    pub fn permits(
        &self,
        caller: u64,
        target: Identity,
        token: u64,
        op: u64,
        object: u64,
        version: u64,
    ) -> bool {
        self.token != 0
            && self.token == token
            && self.event.target == target
            && target.owner == caller
            && self.binding.allows(op, object, version)
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Command {
    pub token: u64,
    pub target: Identity,
    pub operation: u64,
    pub object: u64,
    pub version: u64,
}
/// Server-side one-shot storage. This helper cannot issue physical authority;
/// only the OS's authenticated input path inserts receipts into its own slot.
pub struct ReceiptSlot {
    value: Option<Receipt>,
    last: u64,
}
impl ReceiptSlot {
    pub const EMPTY: Self = Self {
        value: None,
        last: 0,
    };
    pub fn pending(&self, now: u64) -> bool {
        self.value
            .is_some_and(|r| now.checked_sub(r.time_ms).is_some_and(|d| d < 2000))
    }
    pub fn insert(&mut self, r: Receipt, now: u64) -> bool {
        if self.pending(now) || r.token <= self.last || r.time_ms != now {
            return false;
        }
        self.last = r.token;
        self.value = Some(r);
        true
    }
    pub fn get(&self, owner: u64, token: u64, now: u64) -> Option<Receipt> {
        self.value
            .filter(|r| self.pending(now) && r.event.target.owner == owner && r.token == token)
    }
    pub fn consume(&mut self, caller: u64, c: &Command, now: u64) -> bool {
        let Some(r) = self.get(caller, c.token, now) else {
            return false;
        };
        if !r.permits(caller, c.target, c.token, c.operation, c.object, c.version) {
            return false;
        }
        self.value = None;
        true
    }
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn take(token: u64, out: &mut Receipt) -> isize {
    *out = Receipt::EMPTY;
    unsafe {
        crate::raw::syscall(
            crate::file_browser::SYSCALL,
            [
                TAKE,
                token,
                out as *mut _ as u64,
                core::mem::size_of::<Receipt>() as u64,
                0,
            ],
        ) as isize
    }
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn execute(command: &Command) -> isize {
    unsafe {
        crate::raw::syscall(
            crate::file_browser::SYSCALL,
            [
                EXECUTE,
                command as *const _ as u64,
                core::mem::size_of::<Command>() as u64,
                0,
                0,
            ],
        ) as isize
    }
}
