//! CPU-only page objects. All integers on the wire are little endian.
//! IDs are claims checked against the calling endpoint by the kernel.
use crate::Error;
pub const SYSCALL: u64 = 404;
pub const BYTES: usize = 128;
pub const PAGE: u64 = 4096;
pub const WINDOW_START: u64 = 0xa000_0000;
pub const WINDOW_END: u64 = 0xa100_0000;
pub const MAX_PAGES: u64 = 256;
pub const READ: u64 = 1;
pub const WRITE: u64 = 2;
pub mod op {
    pub const CREATE: u64 = 1;
    pub const SHARE: u64 = 2;
    pub const MAP: u64 = 3;
    pub const UNMAP: u64 = 4;
    pub const REVOKE: u64 = 5;
    pub const QUERY: u64 = 6;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handle {
    pub id: u64,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub operation: u64,
    pub handle: Handle,
    /// Exact process endpoint, never a PID alone. Only CREATE's owner may SHARE.
    pub peer: u64,
    pub first_page: u64,
    pub pages: u64,
    pub rights: u64,
    /// MAP returns a process-local address and alias token. UNMAP takes token.
    pub token: u64,
    pub address: u64,
    /// QUERY/REVOKE: charged pages still mapped, reclaimed, quarantined.
    pub live: u64,
    pub returned: u64,
    pub quarantined: u64,
}
impl Request {
    pub fn new(operation: u64) -> Self {
        Self {
            operation,
            ..Self::default()
        }
    }
    pub fn encode(self) -> [u8; BYTES] {
        let mut out = [0; BYTES];
        out[..8].copy_from_slice(b"OSHARE01");
        for (i, value) in [
            self.operation,
            self.handle.id,
            self.handle.generation,
            self.peer,
            self.first_page,
            self.pages,
            self.rights,
            self.token,
            self.address,
            self.live,
            self.returned,
            self.quarantined,
        ]
        .into_iter()
        .enumerate()
        {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
    pub fn decode(bytes: &[u8; BYTES]) -> Result<Self, Error> {
        if &bytes[..8] != b"OSHARE01" {
            return Err(Error::Version);
        }
        if bytes[104..].iter().any(|b| *b != 0) {
            return Err(Error::Reserved);
        }
        let n = |i: usize| u64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().unwrap());
        if !(op::CREATE..=op::QUERY).contains(&n(0)) {
            return Err(Error::Kind);
        }
        Ok(Self {
            operation: n(0),
            handle: Handle {
                id: n(1),
                generation: n(2),
            },
            peer: n(3),
            first_page: n(4),
            pages: n(5),
            rights: n(6),
            token: n(7),
            address: n(8),
            live: n(9),
            returned: n(10),
            quarantined: n(11),
        })
    }
}
pub fn span(first: u64, pages: u64, total: u64) -> Result<(), Error> {
    if pages == 0 || first.checked_add(pages).is_none_or(|end| end > total) {
        Err(Error::Range)
    } else {
        Ok(())
    }
}
pub fn valid_rights(rights: u64) -> bool {
    rights == READ || rights == READ | WRITE
}
