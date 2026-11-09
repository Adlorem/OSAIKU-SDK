//! Stage33 control records. Neither an overview nor association is a grant.
pub const CONFIGURE: u64 = 1;
pub const WIFI_ON: u64 = 2;
pub const WIFI_OFF: u64 = 3;
pub const ETHERNET: u64 = 0;
pub const WIFI: u64 = 1;
pub const OFFLINE: u64 = 0;
pub const NO_SERVICE: u64 = 1;
pub const NO_LINK: u64 = 2;
pub const ADDRESSING: u64 = 3;
pub const CONFIGURED: u64 = 4;
/// DHCP did not acquire an address in 30 seconds; Apply starts a new attempt.
pub const CONFIGURATION_FAILED: u64 = 5;
/// OUTBOUND_REQUEST data: purpose length, data class (1 metadata, 2 user data),
/// redaction (1 none needed, 2 applied), reserved, 124 UTF-8 purpose bytes,
/// then optional DNS name. All padding must be zero. No data itself is sent.
pub const PURPOSE_BYTES: usize = 124;
pub fn intent(m: &super::Message) -> bool {
    let n = m.data[0] as usize;
    if !(1..=PURPOSE_BYTES).contains(&n)
        || !(1..=2).contains(&m.data[1])
        || !(1..=2).contains(&m.data[2])
        || m.data[3] != 0
        || m.length < 128
        || m.length > 381
        || m.data[4 + n..128].iter().any(|v| *v != 0)
        || core::str::from_utf8(&m.data[4..4 + n]).is_err()
        || m.data[4..4 + n].iter().any(|b| *b < 32)
    {
        return false;
    }
    if m.kind == super::kind::DNS {
        m.address == 0
            && m.port == 0
            && m.budget <= 16
            && super::hostname(&m.data[128..m.length as usize])
    } else {
        matches!(m.kind, super::kind::UDP | super::kind::TCP)
            && m.length == 128
            && super::unicast(m.address)
            && (1..=65535).contains(&m.port)
    }
}

/// Shared preflight for UI and privileged configuration. Does not grant access.
pub fn configuration(m: &super::Message) -> bool {
    if m.address > 1 || m.flags > 2 || m.length != 16 || m.data[5..8] != [0; 3] {
        return false;
    }
    if m.flags != super::mode::STATIC {
        return m.data[..16] == [0; 16];
    }
    let ip = super::ipv4(m.data[..4].try_into().unwrap());
    let gateway = super::ipv4(m.data[8..12].try_into().unwrap());
    let dns = super::ipv4(m.data[12..16].try_into().unwrap());
    if !super::unicast(ip)
        || !(1..=30).contains(&m.data[4])
        || gateway != 0 && !super::unicast(gateway)
        || dns != 0 && !super::unicast(dns)
    {
        return false;
    }
    let mask = u32::MAX << (32 - m.data[4]);
    let host = ip as u32 & !mask;
    host != 0
        && host != !mask
        && (gateway == 0
            || gateway != ip
                && ((gateway as u32 ^ ip as u32) & mask) == 0
                && gateway as u32 & !mask != 0
                && gateway as u32 & !mask != !mask)
}
#[derive(Clone, Copy)]
pub enum DataClass {
    Metadata = 1,
    UserData = 2,
}
#[derive(Clone, Copy)]
pub enum Redaction {
    NotNeeded = 1,
    Applied = 2,
}
pub enum Destination<'a> {
    Tcp([u8; 4], u16),
    Udp([u8; 4], u16),
    Dns(&'a str),
}
/// Build a copy-only consent intent. Approval and application instance binding
/// happen in the kernel/policyd, never from caller-supplied owner or package IDs.
pub fn outbound(
    destination: Destination<'_>,
    purpose: &str,
    data: DataClass,
    redaction: Redaction,
    deadline: u64,
    budget: u64,
) -> Result<super::Message, i64> {
    use super::{error, kind, op, Message};
    if purpose.len() > PURPOSE_BYTES {
        return Err(error::INVALID);
    }
    let mut m = Message::new(op::OUTBOUND_REQUEST);
    m.deadline = deadline;
    m.budget = budget;
    m.length = 128;
    m.data[0] = purpose.len() as u8;
    m.data[1] = data as u8;
    m.data[2] = redaction as u8;
    m.data[4..4 + purpose.len()].copy_from_slice(purpose.as_bytes());
    match destination {
        Destination::Tcp(ip, port) | Destination::Udp(ip, port) => {
            m.kind = if matches!(destination, Destination::Tcp(..)) {
                kind::TCP
            } else {
                kind::UDP
            };
            m.address = super::ipv4(ip);
            m.port = u64::from(port);
        }
        Destination::Dns(name) => {
            if name.len() > 253 || budget > 16 {
                return Err(error::INVALID);
            }
            m.kind = kind::DNS;
            m.data[128..128 + name.len()].copy_from_slice(name.as_bytes());
            m.length += name.len() as u64;
        }
    }
    if !intent(&m) || budget == 0 || budget > 1_048_576 {
        return Err(error::INVALID);
    }
    Ok(m)
}
