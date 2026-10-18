//! Service-owned answer provenance. Model prose cannot mint a source receipt.
pub const BYTES: usize = 96;
pub const SOURCE: u64 = 1;
pub const MODEL: u64 = 2;
pub const UNSOURCED: u64 = 4;
pub const MISSING: u64 = 8;
pub const TRUNCATED: u64 = 16;
pub const STALE: u64 = 32;
pub const CONFLICT: u64 = 64;
const FLAGS: u64 = 127;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub model_sha: [u8; 32],
    pub model: u64,
    pub model_revision: u64,
    pub context: u64,
    pub source: u64,
    pub source_revision: u64,
    pub expires: u64,
    pub flags: u64,
}
impl Evidence {
    pub const EMPTY: Self = Self {
        model_sha: [0; 32],
        model: 0,
        model_revision: 0,
        context: 0,
        source: 0,
        source_revision: 0,
        expires: 0,
        flags: UNSOURCED | MISSING,
    };
    pub fn canonical(self) -> bool {
        self.context != 0
            && self.flags & !FLAGS == 0
            && if self.flags & MODEL != 0 {
                self.model_sha != [0; 32]
                    && crate::process::ProcessEndpoint::decode(self.model).names_instance()
                    && self.model_revision != 0
            } else {
                self.model_sha == [0; 32] && self.model == 0 && self.model_revision == 0
            }
            && if self.flags & SOURCE != 0 {
                self.source != 0
                    && self.source_revision != 0
                    && self.expires != 0
                    && self.flags & UNSOURCED == 0
            } else {
                self.source == 0
                    && self.source_revision == 0
                    && self.expires == 0
                    && self.flags & UNSOURCED != 0
            }
    }
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..32].copy_from_slice(&self.model_sha);
        for (out, n) in b[32..].chunks_exact_mut(8).zip([
            self.model,
            self.model_revision,
            self.context,
            self.source,
            self.source_revision,
            self.expires,
            self.flags,
            1,
        ]) {
            out.copy_from_slice(&n.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != BYTES {
            return None;
        }
        let word = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        if word(11) != 1 {
            return None;
        }
        let e = Self {
            model_sha: b[..32].try_into().unwrap(),
            model: word(4),
            model_revision: word(5),
            context: word(6),
            source: word(7),
            source_revision: word(8),
            expires: word(9),
            flags: word(10),
        };
        e.canonical().then_some(e)
    }
    /// Reserved citation syntax; only the recorded read can authorize its ID.
    /// Ordinary URLs/prose remain unverified text, never clickable source receipts.
    pub fn references(self, text: &str, now: u64) -> bool {
        let mut rest = text;
        while let Some(pos) = rest.find("[[source:") {
            rest = &rest[pos + 9..];
            let Some(end) = rest.find("]]") else {
                return false;
            };
            let id = &rest[..end];
            if id.is_empty()
                || id.starts_with('0')
                || !id.bytes().all(|b| b.is_ascii_digit())
                || id.parse::<u64>().ok() != Some(self.source)
                || self.flags & SOURCE == 0
                || self.flags & (STALE | CONFLICT | TRUNCATED | MISSING) != 0
                || now >= self.expires
            {
                return false;
            }
            rest = &rest[end + 2..];
        }
        true
    }
}

pub const AUDIT_MAGIC: u64 = 0x314455414941534f;
/// Numeric metadata only; no prompt hashes, paths, tokens or content buffers.
#[derive(Clone, Copy)]
pub struct Audit {
    pub packet: super::Packet,
    pub flags: u64,
    pub model: u64,
    pub revision: u64,
    pub context: u64,
}
impl Audit {
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        for (out, n) in b.chunks_exact_mut(8).zip([
            AUDIT_MAGIC,
            self.packet.session,
            self.packet.request,
            self.packet.window,
            self.packet.incarnation,
            (self.packet.value >> 8) & 255,
            self.packet.value & 255,
            self.flags,
            self.model,
            self.revision,
            self.context,
            1,
        ]) {
            out.copy_from_slice(&n.to_le_bytes());
        }
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != BYTES {
            return None;
        }
        let w = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        if w(0) != AUDIT_MAGIC
            || w(11) != 1
            || !(1..=super::op::CONNECTION).contains(&w(5))
            || w(6) > super::Status::StartupTimeout as u64
            || w(7) & !FLAGS != 0
            || (1..=4).any(|i| w(i) == 0)
        {
            return None;
        }
        Some(Self {
            packet: super::Packet {
                operation: super::op::REPLY,
                session: w(1),
                request: w(2),
                window: w(3),
                incarnation: w(4),
                offset: 0,
                value: w(6) | (w(5) << 8),
            },
            flags: w(7),
            model: w(8),
            revision: w(9),
            context: w(10),
        })
    }
}
