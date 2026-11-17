//! Copied GPU process control. Identity is supplied by the kernel transport;
//! a peer cannot select its device, role or activation by claiming an endpoint.
pub const SYSCALL: u64 = 410;
pub const BYTES: usize = 128;
pub mod op {
    pub const IDENTITY: u64 = 1;
    pub const HEARTBEAT: u64 = 2;
    pub const FENCE: u64 = 3;
    pub const PRESENT: u64 = 4;
    pub const FAILURE: u64 = 5;
    pub const STATUS: u64 = 6;
    pub const STOPPED: u64 = 7;
    /// Terminal pre-HANDOFF stop proof, after complete resource revocation.
    pub const FIRMWARE_ABORTED: u64 = 8;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message {
    pub operation: u64,
    pub attempt: u64,
    pub selection: u64,
    pub device: u64,
    pub connection: u64,
    pub role: u64,
    pub sequence: u64,
    pub value: u64,
    pub digest: [u8; 32],
}
impl Message {
    pub const fn new(operation: u64) -> Self {
        Self {
            operation,
            attempt: 0,
            selection: 0,
            device: 0,
            connection: 0,
            role: 0,
            sequence: 0,
            value: 0,
            digest: [0; 32],
        }
    }
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"OSAGPC01");
        for (i, n) in [
            self.operation,
            self.attempt,
            self.selection,
            self.device,
            self.connection,
            self.role,
            self.sequence,
            self.value,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[72..104].copy_from_slice(&self.digest);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, super::Error> {
        if b.len() != BYTES {
            return Err(super::Error::Length);
        }
        if &b[..8] != b"OSAGPC01" {
            return Err(super::Error::Version);
        }
        if b[104..].iter().any(|b| *b != 0) {
            return Err(super::Error::Reserved);
        }
        let n = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if !(1..=op::FIRMWARE_ABORTED).contains(&n(8)) {
            return Err(super::Error::Version);
        }
        Ok(Self {
            operation: n(8),
            attempt: n(16),
            selection: n(24),
            device: n(32),
            connection: n(40),
            role: n(48),
            sequence: n(56),
            value: n(64),
            digest: b[72..104].try_into().unwrap(),
        })
    }
}
