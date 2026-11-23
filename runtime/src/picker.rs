//! Human-selected file scopes. OPEN asks for a chooser; it grants no file.
//! Read/replace use only the opaque returned token, never paths or object IDs.
pub const SYSCALL: u64 = 398;
pub const OPEN: u64 = 1;
pub const STATUS: u64 = 2;
pub const CANCEL: u64 = 3;
pub const READ: u64 = 4;
pub const REPLACE: u64 = 5;
/// Ask for a new file in a human-selected directory. The payload is one file
/// name, never a path. Confirmation grants only create-if-absent for that name.
pub const CREATE: u64 = 6;
/// Retired model chooser operation; always refused.
pub const OPEN_MODEL: u64 = 7;
/// Start replacing the scope's file with more than one request holds:
/// `offset` is the replacement's whole length (over CAPACITY, at most
/// MAX_REPLACE), with no bytes. Any earlier staged replacement of the same
/// application is abandoned.
pub const BEGIN_REPLACE: u64 = 8;
/// The next part of a staged replacement at `offset`, the bytes written so
/// far. The part that completes the declared length commits the file at once,
/// as one REPLACE would; a part out of order abandons it. CANCEL of the token
/// abandons it too, and leaves the file as it was.
pub const WRITE: u64 = 9;
/// The chosen file's name (at most 64 bytes) for a live scope, so an editor
/// can title its tab. A pending choice answers 1; it names no path.
pub const NAME: u64 = 10;
pub const CAPACITY: usize = 4096;
pub const MAX_REPLACE: u64 = 16 * 1024 * 1024;
/// Authenticated host delivery of the selected file. A native editor with
/// selected-file WRITE permission can also save through this scope.
/// Receiving these bytes from another app never authorizes an open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenScope {
    pub sequence: u64,
    pub window: u64,
    pub incarnation: u64,
    pub token: u64,
}
impl OpenScope {
    const MAGIC: u64 = 0x31504f43534e504f;
    pub fn encode(self) -> [u8; 40] {
        let mut b = [0; 40];
        for (v, c) in [
            Self::MAGIC,
            self.sequence,
            self.window,
            self.incarnation,
            self.token,
        ]
        .iter()
        .zip(b.chunks_exact_mut(8))
        {
            c.copy_from_slice(&v.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != 40 {
            return None;
        }
        let mut w = [0; 5];
        for (v, c) in w.iter_mut().zip(b.chunks_exact(8)) {
            *v = u64::from_le_bytes(c.try_into().ok()?);
        }
        (w[0] == Self::MAGIC && w[1..].iter().all(|v| *v != 0)).then_some(Self {
            sequence: w[1],
            window: w[2],
            incarnation: w[3],
            token: w[4],
        })
    }
    pub fn reply(self, accepted: bool) -> [u8; 24] {
        let mut b = [0; 24];
        b[..8].copy_from_slice(&Self::MAGIC.to_le_bytes());
        b[8..16].copy_from_slice(&self.sequence.to_le_bytes());
        b[16..].copy_from_slice(&u64::from(accepted).to_le_bytes());
        b
    }
    pub fn accepted(self, b: &[u8]) -> bool {
        b == self.reply(true)
    }
}
pub fn valid_name(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes != b"."
        && bytes != b".."
        && core::str::from_utf8(bytes).is_ok_and(|s| {
            !s.chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
                && !s.ends_with([' ', '.'])
        })
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub version: u64,
    pub sequence: u64,
    pub operation: u64,
    pub window: u64,
    pub incarnation: u64,
    pub token: u64,
    pub rights: u64,
    pub offset: u64,
    pub length: u64,
    pub bytes: [u8; CAPACITY],
}
impl Request {
    pub const EMPTY: Self = Self {
        version: 1,
        sequence: 1,
        operation: OPEN,
        window: 0,
        incarnation: 0,
        token: 0,
        rights: 0,
        offset: 0,
        length: 0,
        bytes: [0; CAPACITY],
    };
    pub fn canonical(&self) -> bool {
        use crate::launch::permission as p;
        let selected = p::SELECTED_FILE_READ | p::SELECTED_FILE_WRITE;
        self.version == 1
            && self.sequence != 0
            && self.window != 0
            && self.incarnation != 0
            && self.length <= CAPACITY as u64
            && match self.operation {
                CREATE => {
                    self.token == 0
                        && self.rights == p::SELECTED_FILE_WRITE
                        && self.offset == 0
                        && valid_name(&self.bytes[..self.length as usize])
                        && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
                }
                OPEN => {
                    self.token == 0
                        && self.rights != 0
                        && self.rights & !selected == 0
                        && self.offset == 0
                        && self.length == 0
                        && self.bytes == [0; CAPACITY]
                }
                STATUS | CANCEL | NAME => {
                    self.token != 0
                        && self.rights == 0
                        && self.offset == 0
                        && self.length == 0
                        && self.bytes == [0; CAPACITY]
                }
                READ => {
                    self.token != 0
                        && self.rights == 0
                        && self.length != 0
                        && self.bytes == [0; CAPACITY]
                }
                REPLACE => {
                    self.token != 0
                        && self.rights == 0
                        && self.offset == 0
                        && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
                }
                BEGIN_REPLACE => {
                    self.token != 0
                        && self.rights == 0
                        && self.offset > CAPACITY as u64
                        && self.offset <= MAX_REPLACE
                        && self.length == 0
                        && self.bytes == [0; CAPACITY]
                }
                WRITE => {
                    self.token != 0
                        && self.rights == 0
                        && self.length != 0
                        && self.offset < MAX_REPLACE
                        && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
                }
                _ => false,
            }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub version: u64,
    pub sequence: u64,
    pub token: u64,
    pub status: u64,
    pub length: u64,
    pub bytes: [u8; CAPACITY],
}
impl Reply {
    pub const EMPTY: Self = Self {
        version: 1,
        sequence: 0,
        token: 0,
        status: 0,
        length: 0,
        bytes: [0; CAPACITY],
    };
    pub fn valid_for(&self, r: &Request) -> bool {
        r.canonical()
            && self.version == r.version
            && self.sequence == r.sequence
            && self.status <= 6
            && (matches!(r.operation, OPEN | CREATE) || self.token == r.token)
            && self.length <= CAPACITY as u64
            && self.bytes[self.length as usize..].iter().all(|b| *b == 0)
            && (self.status == 0 || self.length == 0)
            && ((r.operation == READ && self.length <= r.length)
                || (r.operation == NAME && self.length <= 64)
                || self.length == 0)
    }
}
/// Borrowed wire workspace for small-stack native applications. No payload
/// array is allocated by this function; the caller may retain it in its image.
pub fn exchange_into_with(
    r: &Request,
    out: &mut Reply,
    mut f: impl FnMut(&Request, &mut Reply) -> isize,
) -> Result<(), isize> {
    *out = Reply::EMPTY;
    if !r.canonical() {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    let code = f(r, out);
    if code != 0 || !out.valid_for(r) {
        *out = Reply::EMPTY;
        return Err(if code != 0 {
            code
        } else {
            crate::abi::SYSCALL_ERR_INVALID_STATE
        });
    }
    Ok(())
}
pub fn exchange_with(
    r: &Request,
    f: impl FnMut(&Request, &mut Reply) -> isize,
) -> Result<Reply, isize> {
    let mut out = Reply::EMPTY;
    exchange_into_with(r, &mut out, f)?;
    Ok(out)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange_into(r: &Request, out: &mut Reply) -> Result<(), isize> {
    exchange_into_with(r, out, |r, out| {
        let code: isize;
        unsafe {
            core::arch::asm!("int 0x80",inlateout("rax") SYSCALL as isize=>code,in("rdi")r as *const Request,in("rsi")out as *mut Reply,in("rdx")0u64,in("r8")0u64,in("r9")0u64,options(nostack));
        }
        code
    })
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(r: &Request) -> Result<Reply, isize> {
    let mut out = Reply::EMPTY;
    exchange_into(r, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;

    fn request(operation: u64) -> Box<Request> {
        let mut r = Box::new(Request::EMPTY);
        r.operation = operation;
        r.window = 4;
        r.incarnation = 2;
        r.token = 9;
        r.rights = 0;
        r
    }
    #[test]
    fn staged_replacement_parts_have_one_shape() {
        let mut begin = request(BEGIN_REPLACE);
        begin.offset = CAPACITY as u64 + 1;
        assert!(begin.canonical());
        // A replacement one request holds uses REPLACE; bytes and lengths
        // never ride along with BEGIN_REPLACE.
        begin.offset = CAPACITY as u64;
        assert!(!begin.canonical());
        begin.offset = MAX_REPLACE + 1;
        assert!(!begin.canonical());
        begin.offset = MAX_REPLACE;
        begin.length = 1;
        assert!(!begin.canonical());
        let mut part = request(WRITE);
        part.offset = 8192;
        part.length = 3;
        part.bytes[..3].copy_from_slice(b"abc");
        assert!(part.canonical());
        part.length = 0;
        assert!(!part.canonical());
        part.length = 2;
        assert!(!part.canonical(), "bytes past the length");
        part.length = 3;
        part.token = 0;
        assert!(!part.canonical());
    }
    #[test]
    fn a_name_reply_carries_at_most_a_name() {
        let r = request(NAME);
        assert!(r.canonical());
        let mut reply = Box::new(Reply::EMPTY);
        reply.sequence = r.sequence;
        reply.token = r.token;
        reply.length = 11;
        reply.bytes[..11].copy_from_slice(b"notatka.txt");
        assert!(reply.valid_for(&r));
        reply.length = 65;
        assert!(!reply.valid_for(&r));
        // Other operations still answer without bytes.
        let status = request(STATUS);
        reply.length = 11;
        assert!(!reply.valid_for(&status));
    }
}
