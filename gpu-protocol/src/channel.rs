//! Copied SDK/renderer mailbox. Payload is inline, never a nested user pointer.
//! A session is allocated by the kernel and bound to two exact incarnations.
use crate::buffer::Handle;
pub const SYSCALL: u64 = 412;
pub const HEADER: usize = 256;
pub const BYTES: usize = HEADER + crate::render::MAX_BYTES;
pub const MAX_SESSIONS: usize = 64;
/// Transient refusal: the operation consumed no input or ownership.
pub const BUSY: isize = -13;
pub const ASSET_START: u64 = 0xa000_0000;
pub const ASSET_END: u64 = 0xa100_0000;
pub mod op {
    pub const OPEN: u32 = 1;
    pub const IDENTITY: u32 = 2;
    pub const REGISTER: u32 = 3;
    pub const RELEASE: u32 = 4;
    pub const SUBMIT: u32 = 5;
    pub const TAKE: u32 = 6;
    pub const COMPLETE: u32 = 7;
    pub const POLL: u32 = 8;
    pub const CLOSE: u32 = 9;
    pub const MAP: u32 = 10;
    pub const RESIZE: u32 = 11;
    pub const OFFER: u32 = 12;
}
pub mod status {
    pub const PENDING: u32 = 1;
    pub const IDLE: u32 = 2;
    pub const QUEUED: u32 = 3;
    pub const RUNNING: u32 = 4;
    pub const DONE: u32 = 5;
    pub const FAILED: u32 = 6;
    pub const CLOSED: u32 = 7;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Length,
    Version,
    Reserved,
    Operation,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub operation: u32,
    pub status: u32,
    pub session: u64,
    pub generation: u64,
    pub sequence: u64,
    pub args: [u64; 16],
    pub handle: Handle,
    pub length: u64,
}
impl Message {
    pub fn new(operation: u32) -> Self {
        Self {
            operation,
            ..Self::default()
        }
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != HEADER {
            return Err(Error::Length);
        }
        if &b[..8] != b"OSAGCH01" {
            return Err(Error::Version);
        }
        if b[216..].iter().any(|v| *v != 0) {
            return Err(Error::Reserved);
        }
        let u32at = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u64at = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let m = Self {
            operation: u32at(8),
            status: u32at(12),
            session: u64at(16),
            generation: u64at(24),
            sequence: u64at(32),
            args: core::array::from_fn(|i| u64at(40 + i * 8)),
            handle: Handle {
                owner: u64at(168),
                device: u64at(176),
                generation: u64at(184),
                serial: u64at(192),
                kind: u32at(200),
                reserved: u32at(204),
            },
            length: u64at(208),
        };
        if !(op::OPEN..=op::OFFER).contains(&m.operation) {
            return Err(Error::Operation);
        }
        if m.length > crate::render::MAX_BYTES as u64 {
            return Err(Error::Length);
        }
        Ok(m)
    }
    pub fn encode(self, b: &mut [u8]) -> Result<(), Error> {
        if b.len() != HEADER {
            return Err(Error::Length);
        }
        b.fill(0);
        b[..8].copy_from_slice(b"OSAGCH01");
        b[8..12].copy_from_slice(&self.operation.to_le_bytes());
        b[12..16].copy_from_slice(&self.status.to_le_bytes());
        for (i, v) in [self.session, self.generation, self.sequence]
            .into_iter()
            .chain(self.args)
            .enumerate()
        {
            b[16 + i * 8..24 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, v) in [
            self.handle.owner,
            self.handle.device,
            self.handle.generation,
            self.handle.serial,
        ]
        .into_iter()
        .enumerate()
        {
            b[168 + i * 8..176 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        b[200..204].copy_from_slice(&self.handle.kind.to_le_bytes());
        b[204..208].copy_from_slice(&self.handle.reserved.to_le_bytes());
        b[208..216].copy_from_slice(&self.length.to_le_bytes());
        Ok(())
    }
    /// Reject ignored fields before changing any owner. Replies have their own
    /// shape and are never fed through this request validator.
    pub fn request(self) -> Result<Self, Error> {
        let (args, handle, seq, payload) = match self.operation {
            op::OPEN | op::RESIZE => (2, false, false, false),
            op::IDENTITY | op::TAKE | op::CLOSE | op::OFFER => (0, false, false, false),
            op::REGISTER => (3, false, false, false),
            op::RELEASE | op::MAP => (0, true, false, false),
            op::SUBMIT => (0, false, true, true),
            op::COMPLETE => (5, false, true, false),
            op::POLL => (0, false, true, false),
            _ => return Err(Error::Operation),
        };
        if self.status != 0
            || self.args[args..].iter().any(|v| *v != 0)
            || (!handle && self.handle != Handle::default())
            || self.handle.reserved != 0
            || (!seq && self.sequence != 0)
            || (seq && self.sequence == 0)
            || (!payload && self.length != 0)
            || (payload && self.length == 0)
            || (matches!(self.operation, op::OPEN | op::IDENTITY | op::OFFER)
                && (self.session != 0 || self.generation != 0))
        {
            return Err(Error::Reserved);
        }
        Ok(self)
    }
}
