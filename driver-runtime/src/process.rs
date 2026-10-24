//! Process-side protocol and owner-held supervision. No addresses or DMA grants.
use crate::protocol::{Error, Scope};
pub const SYSCALL: u64 = 400;
pub const BYTES: usize = 384;
pub const DATA: usize = 128;
pub const PAYLOAD: usize = BYTES - DATA;
pub const WINDOW: u64 = 1000;
pub const START_TIMEOUT: u64 = 5000;
pub const WATCHDOG: u64 = 3000;
/// Control-plane operations can wait behind a bounded storage continuation.
pub const MANAGER_WATCHDOG: u64 = 15000;
pub const MAX_ATTEMPTS: u8 = 3;
pub const MAX_CALLS: u16 = 128;
pub const SLOTS: usize = 8;
pub mod op {
    pub const BOOT: u64 = 1;
    pub const HELLO: u64 = 2;
    pub const READY: u64 = 3;
    pub const HEARTBEAT: u64 = 4;
    pub const SNAPSHOT: u64 = 5;
    pub const RECONCILE: u64 = 6;
    pub const START: u64 = 7;
    pub const SUBMIT: u64 = 8;
    pub const TAKE: u64 = 9;
    pub const CANCEL: u64 = 10;
    pub const STATUS: u64 = 11;
    pub const APPROVE: u64 = 12;
    pub const HID_READ: u64 = 13;
    pub const HID_INPUT: u64 = 14;
    pub const BLOCK_FETCH: u64 = 15;
    pub const BLOCK_STEP: u64 = 16;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Manager,
    Driver,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Initializing,
    Ready,
    Quarantined,
    Stopped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Failure {
    None,
    StartTimeout,
    Watchdog,
    ProcessLost,
    Flood,
    Protocol,
    Memory,
    Exhausted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub operation: u64,
    pub scope: Scope,
    pub release: u64,
    pub endpoint: u64,
    pub sequence: u64,
    pub offset: u64,
    pub length: u64,
    pub value: u64,
    pub payload: [u8; PAYLOAD],
}
impl Message {
    pub fn new(operation: u64, scope: Scope) -> Self {
        Self {
            operation,
            scope,
            release: 1,
            endpoint: 1,
            sequence: 0,
            offset: 0,
            length: 0,
            value: 0,
            payload: [0; PAYLOAD],
        }
    }
    pub fn encode(&self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"ODPROC01");
        for (i, n) in [
            self.operation,
            self.scope.device.get(),
            self.scope.connection.get(),
            self.scope.instance.get(),
            self.release,
            self.endpoint,
            self.sequence,
            self.offset,
            self.length,
            self.value,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[DATA..].copy_from_slice(&self.payload);
        b
    }
    pub fn decode(b: &[u8; BYTES]) -> Result<Self, Error> {
        if &b[..8] != b"ODPROC01" {
            return Err(Error::Version);
        }
        if b[88..DATA].iter().any(|x| *x != 0) {
            return Err(Error::Reserved);
        }
        let n = |i: usize| u64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap());
        let scope = Scope::new(n(1), n(2), n(3)).ok_or(Error::InvalidId)?;
        let mut m = Self::new(n(0), scope);
        m.release = n(4);
        m.endpoint = n(5);
        m.sequence = n(6);
        m.offset = n(7);
        m.length = n(8);
        m.value = n(9);
        if m.offset
            .checked_add(m.length)
            .is_none_or(|end| end > PAYLOAD as u64)
        {
            return Err(Error::Range);
        }
        m.payload.copy_from_slice(&b[DATA..]);
        Ok(m)
    }
}
/// Fixed owner-side snapshot queue. Metadata is never mapped into a peer.
/// Copied input remains private through completion/cancellation. A DMA adapter
/// must retain its independent lease until platform stop, never rely on this queue.
pub struct Copies {
    entries: [Option<Message>; SLOTS],
    last: u64,
    closed: bool,
}
impl Copies {
    pub const fn new() -> Self {
        Self {
            entries: [None; SLOTS],
            last: 0,
            closed: false,
        }
    }
    pub fn submit(&mut self, mut message: Message) -> Result<u64, Error> {
        if self.closed {
            return Err(Error::State);
        }
        if message
            .offset
            .checked_add(message.length)
            .is_none_or(|n| n > PAYLOAD as u64)
        {
            return Err(Error::Range);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::Backpressure)?;
        let sequence = self.last.checked_add(1).ok_or(Error::Exhausted)?;
        self.last = sequence;
        message.sequence = sequence;
        *slot = Some(message);
        Ok(sequence)
    }
    pub fn take(&mut self, sequence: u64) -> Result<Message, Error> {
        if self.closed {
            return Err(Error::State);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|s| s.is_some_and(|m| m.sequence == sequence))
            .ok_or(Error::Stale)?;
        Ok(slot.take().unwrap())
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.entries.fill(None);
    }
}
impl Default for Copies {
    fn default() -> Self {
        Self::new()
    }
}
/// Authority created only by the host's approved launch path. A decoded wire
/// message cannot replace this record or renew the start deadline.
pub struct Instance {
    pub role: Role,
    pub scope: Scope,
    pub release: u64,
    pub endpoint: u64,
    pub phase: Phase,
    pub failure: Failure,
    deadline: u64,
    window: u64,
    calls: u16,
    hid: bool,
    pub copies: Copies,
}
impl Instance {
    pub fn admitted(role: Role, scope: Scope, release: u64, endpoint: u64, now: u64) -> Self {
        Self {
            role,
            scope,
            release,
            endpoint,
            phase: Phase::Starting,
            failure: Failure::None,
            deadline: now.saturating_add(START_TIMEOUT),
            window: now,
            calls: 0,
            hid: false,
            copies: Copies::new(),
        }
    }
    /// Only the trusted owner may admit an input-role package.
    pub fn admit_hid(&mut self) {
        self.hid = true;
    }
    pub fn authorize(&mut self, role: Role, m: &Message, now: u64) -> Result<(), Error> {
        if self.poll(now, true) {
            return Err(Error::Deadline);
        }
        if matches!(self.phase, Phase::Quarantined | Phase::Stopped) {
            return Err(Error::State);
        }
        if role != self.role
            || m.scope != self.scope
            || m.release != self.release
            || m.endpoint != self.endpoint
        {
            return Err(Error::Permission);
        }
        if now.saturating_sub(self.window) >= WINDOW {
            self.window = now;
            self.calls = 0;
        }
        if self.calls >= if self.hid { 8192 } else { MAX_CALLS } {
            self.fail(Failure::Flood);
            return Err(Error::Backpressure);
        }
        self.calls += 1;
        match m.operation {
            op::HELLO if self.phase == Phase::Starting => {
                self.phase = Phase::Initializing;
            }
            op::READY if self.phase == Phase::Initializing => {
                self.phase = Phase::Ready;
                self.deadline = now.saturating_add(if role == Role::Manager {
                    MANAGER_WATCHDOG
                } else {
                    WATCHDOG
                });
            }
            op::HEARTBEAT if self.phase == Phase::Ready => {
                self.deadline = now.saturating_add(if role == Role::Manager {
                    MANAGER_WATCHDOG
                } else {
                    WATCHDOG
                });
            }
            op::SNAPSHOT | op::RECONCILE | op::START | op::STATUS | op::APPROVE
                if self.phase == Phase::Ready && role == Role::Manager =>
            {
                ()
            }
            op::HID_READ | op::HID_INPUT | op::BLOCK_FETCH | op::BLOCK_STEP
                if self.hid && self.phase == Phase::Ready && role == Role::Driver =>
            {
                ()
            }
            op::SUBMIT | op::TAKE | op::CANCEL
                if self.phase == Phase::Ready && role == Role::Driver =>
            {
                ()
            }
            _ => return Err(Error::Permission),
        }
        Ok(())
    }
    pub fn poll(&mut self, now: u64, live: bool) -> bool {
        if matches!(self.phase, Phase::Stopped | Phase::Quarantined) {
            return false;
        }
        if !live {
            self.fail(Failure::ProcessLost);
            return true;
        }
        if now >= self.deadline {
            self.fail(if self.phase == Phase::Ready {
                Failure::Watchdog
            } else {
                Failure::StartTimeout
            });
            return true;
        }
        false
    }
    pub fn fail(&mut self, cause: Failure) {
        if !matches!(self.phase, Phase::Stopped | Phase::Quarantined) {
            self.failure = cause;
            self.phase = Phase::Quarantined;
            self.copies.close();
        }
    }
    /// Called by the platform after revocation/IRQ join/DMA stop. Peer Stopped
    /// messages and process death are never proofs. Uncertain writes stay failed.
    pub fn platform_stopped(&mut self) {
        self.copies.close();
        self.phase = Phase::Stopped;
    }
}
