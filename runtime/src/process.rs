#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessId(pub u32);

impl ProcessId {
    pub const KERNEL: Self = Self(0);
}

/// A process, and which instance of it.
///
/// An identity says where to deliver; it does not say to whom. Where a slot can
/// be reused -- and an application's can -- a message addressed by identity
/// alone follows the number into whatever instance holds it next, so a sender
/// holding an address it learned from a closed instance would be answered by
/// that instance's successor. The generation names the instance, and a mismatch
/// is refused rather than delivered.
///
/// Generation zero means no instance was named. It is accepted only where the
/// identity cannot be reused, which today is the fixed services.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessEndpoint {
    pub process_id: ProcessId,
    pub generation: u32,
}

impl ProcessEndpoint {
    /// An address that names an instance.
    pub const fn new(process_id: ProcessId, generation: u32) -> Self {
        Self {
            process_id,
            generation,
        }
    }

    /// An address with no instance named, for an identity that cannot be
    /// reused.
    pub const fn unbound(process_id: ProcessId) -> Self {
        Self {
            process_id,
            generation: 0,
        }
    }

    /// Whether this address names an instance at all.
    pub const fn names_instance(&self) -> bool {
        self.generation != 0
    }

    /// One machine word, generation above identity.
    ///
    /// Syscalls take this rather than a bare identity, so an endpoint cannot
    /// quietly decay into a plain number on the way to the kernel.
    pub const fn encode(&self) -> u64 {
        ((self.generation as u64) << 32) | self.process_id.0 as u64
    }

    pub const fn decode(raw: u64) -> Self {
        Self {
            process_id: ProcessId(raw as u32),
            generation: (raw >> 32) as u32,
        }
    }
}
pub mod observe;

pub const FIXED_SERVICE_COUNT: usize = 8;
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Empty = 0,
    Ready = 1,
    Running = 2,
    Waiting = 3,
    Faulted = 4,
    Stopped = 5,
    Exhausted = 6,
    Retired = 7,
    Runnable = 8,
    BlockedOnIpc = 9,
    BlockedOnReply = 10,
    BlockedOnStorage = 11,
}
