#![no_std]
//! Versioned, copied network service requests. Handles never contain addresses.
pub const SYSCALL: u64 = 408;
pub const BYTES: usize = 1152;
pub const PAYLOAD: usize = 1024;
pub const MAGIC: u64 = 0x3154454e4941534f;
pub mod control;
pub mod op {
    pub const CONFIGURE: u64 = 1;
    pub const GRANT: u64 = 2;
    pub const REVOKE: u64 = 3;
    pub const OPEN: u64 = 4;
    pub const SEND: u64 = 5;
    pub const RECEIVE: u64 = 6;
    pub const CLOSE: u64 = 7;
    pub const RESOLVE: u64 = 8;
    pub const STATUS: u64 = 9;
    pub const CANCEL: u64 = 10;
    pub const POLL: u64 = 11;
    pub const TAKE: u64 = 20;
    pub const COMPLETE: u64 = 21;
    pub const CONFIGURATION: u64 = 22;
    pub const VALIDATE: u64 = 23;
    pub const LINK_STATE: u64 = 24;
    pub const OVERVIEW: u64 = 30;
    pub const PUBLISH_STATE: u64 = 31;
    pub const CONTROL: u64 = 32;
    pub const POLICY_TAKE: u64 = 33;
    pub const POLICY_DECIDE: u64 = 34;
    pub const CONTROL_POLL: u64 = 35;
    pub const OUTBOUND_REQUEST: u64 = 36;
    pub const OUTBOUND_ROW: u64 = 37;
    pub const OUTBOUND_DECIDE: u64 = 38;
    pub const OUTBOUND_RESULT: u64 = 39;
    pub const OUTBOUND_REVOKE: u64 = 40;
}
pub mod kind {
    pub const UDP: u64 = 1;
    pub const TCP: u64 = 2;
    pub const DNS: u64 = 3;
}
pub mod mode {
    pub const OFFLINE: u64 = 0;
    pub const STATIC: u64 = 1;
    pub const DHCP: u64 = 2;
}
/// Stable STATUS values; independent of the IP stack's internal state enum.
pub mod state {
    pub const CLOSED: u64 = 0;
    pub const CONNECTING: u64 = 1;
    pub const CONNECTED: u64 = 2;
    pub const CLOSING: u64 = 3;
}
pub mod error {
    pub const INVALID: i64 = -5;
    pub const DENIED: i64 = -8;
    pub const LIMIT: i64 = -10;
    pub const UNSUPPORTED: i64 = -11;
    pub const STALE: i64 = -12;
    pub const BUSY: i64 = -13;
    pub const TIMEOUT: i64 = -14;
    pub const OFFLINE: i64 = -15;
    pub const CLOSED: i64 = -16;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message {
    pub operation: u64,
    pub id: u64,
    pub owner: u64,
    pub handle: u64,
    pub epoch: u64,
    pub deadline: u64,
    pub budget: u64,
    pub kind: u64,
    pub address: u64,
    pub port: u64,
    pub flags: u64,
    pub length: u64,
    pub result: i64,
    pub data: [u8; PAYLOAD],
}
impl Message {
    pub const fn new(operation: u64) -> Self {
        Self {
            operation,
            id: 0,
            owner: 0,
            handle: 0,
            epoch: 0,
            deadline: 0,
            budget: 0,
            kind: 0,
            address: 0,
            port: 0,
            flags: 0,
            length: 0,
            result: 0,
            data: [0; PAYLOAD],
        }
    }
    pub fn encode(&self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        for (i, n) in [
            MAGIC,
            1,
            self.operation,
            self.id,
            self.owner,
            self.handle,
            self.epoch,
            self.deadline,
            self.budget,
            self.kind,
            self.address,
            self.port,
            self.flags,
            self.length,
            self.result as u64,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            b[i * 8..i * 8 + 8].copy_from_slice(&n.to_le_bytes());
        }
        b[128..].copy_from_slice(&self.data);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, i64> {
        if b.len() != BYTES {
            return Err(error::INVALID);
        }
        let n = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        if n(0) != MAGIC
            || n(1) != 1
            || n(15) != 0
            || n(13) > PAYLOAD as u64
            || !matches!(n(2),1..=11|20..=24|30..=40)
        {
            return Err(error::INVALID);
        }
        let mut m = Self {
            operation: n(2),
            id: n(3),
            owner: n(4),
            handle: n(5),
            epoch: n(6),
            deadline: n(7),
            budget: n(8),
            kind: n(9),
            address: n(10),
            port: n(11),
            flags: n(12),
            length: n(13),
            result: n(14) as i64,
            data: [0; PAYLOAD],
        };
        m.data.copy_from_slice(&b[128..]);
        Ok(m)
    }
    pub fn payload(&self) -> Result<&[u8], i64> {
        self.data.get(..self.length as usize).ok_or(error::INVALID)
    }
}
/// Network-order IPv4 bytes in a portable integer, never a native sockaddr.
pub fn ipv4(bytes: [u8; 4]) -> u64 {
    u32::from_be_bytes(bytes) as u64
}
pub fn unicast(address: u64) -> bool {
    address <= u32::MAX as u64 && !matches!((address >> 24) as u8, 0 | 127 | 224..=255)
}
pub fn hostname(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split(|b| *b == b'.').all(|s| {
            !s.is_empty()
                && s.len() <= 63
                && s[0].is_ascii_alphanumeric()
                && s[s.len() - 1].is_ascii_alphanumeric()
                && s.iter()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        })
}
