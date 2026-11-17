//! Opaque package-private backend RPC. The platform authenticates endpoints;
//! only the package interprets the payload (e.g. bounded i915 ioctl records).
pub const SYSCALL: u64 = 413;
pub const HEADER: usize = 128;
pub const MAX_PAYLOAD: usize = 64 * 1024;
pub const MAX_BYTES: usize = HEADER + MAX_PAYLOAD;
pub const SLOTS: usize = 8;
pub mod op {
    pub const SEND: u64 = 1;
    pub const RECEIVE: u64 = 2;
    pub const REPLY: u64 = 3;
    pub const POLL: u64 = 4;
    pub const PROBE: u64 = 5;
    pub const CALL: u64 = 6;
    pub const WAIT: u64 = 7;
    pub const RECEIVE_WAIT: u64 = 8;
}
pub mod state {
    pub const QUEUED: u64 = 1;
    pub const RUNNING: u64 = 2;
    pub const DONE: u64 = 3;
    pub const CLOSED: u64 = 4;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    pub client: u64,
    pub renderer: u64,
    pub session: u64,
    pub generation: u64,
    pub device: u64,
    pub connection: u64,
}
impl Peer {
    pub fn valid(self) -> bool {
        self.client as u32 != 0
            && self.client >> 32 != 0
            && self.renderer as u32 != 0
            && self.renderer >> 32 != 0
            && self.client != self.renderer
            && self.session != 0
            && self.generation != 0
            && self.device != 0
            && self.connection != 0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub operation: u64,
    pub serial: u64,
    pub peer: Peer,
    pub state: u64,
    pub length: u64,
}
impl Message {
    pub fn new(operation: u64) -> Self {
        Self {
            operation,
            ..Self::default()
        }
    }
    pub fn encode(self) -> [u8; HEADER] {
        let mut b = [0; HEADER];
        b[..8].copy_from_slice(b"OSAGRP01");
        for (i, v) in [
            self.operation,
            self.serial,
            self.peer.client,
            self.peer.renderer,
            self.peer.session,
            self.peer.generation,
            self.peer.device,
            self.peer.connection,
            self.state,
            self.length,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, ()> {
        if b.len() != HEADER || &b[..8] != b"OSAGRP01" || b[88..].iter().any(|n| *n != 0) {
            return Err(());
        }
        let n = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let m = Self {
            operation: n(8),
            serial: n(16),
            peer: Peer {
                client: n(24),
                renderer: n(32),
                session: n(40),
                generation: n(48),
                device: n(56),
                connection: n(64),
            },
            state: n(72),
            length: n(80),
        };
        if !(op::SEND..=op::RECEIVE_WAIT).contains(&m.operation) || m.length > MAX_PAYLOAD as u64 {
            return Err(());
        }
        Ok(m)
    }
    pub fn request(self) -> Result<Self, ()> {
        if self.state != 0 || self.length > MAX_PAYLOAD as u64 {
            return Err(());
        }
        match self.operation {
            op::SEND | op::CALL if self.peer.valid() && self.serial == 0 && self.length != 0 => (),
            op::RECEIVE if self.peer == Peer::default() && self.serial == 0 && self.length == 0 => {
                ()
            }
            op::RECEIVE_WAIT if self.peer == Peer::default() && self.serial <= 10 && self.length == 0 => {
                ()
            }
            op::REPLY if self.peer.valid() && self.serial != 0 && self.length != 0 => (),
            op::POLL | op::WAIT if self.peer.valid() && self.serial != 0 && self.length == 0 => (),
            op::PROBE if self.peer.valid() && self.serial == 0 && self.length == 0 => (),
            _ => return Err(()),
        }
        Ok(self)
    }
}
