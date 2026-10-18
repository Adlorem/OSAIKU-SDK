//! Saved reports are data, never authority. Unknown versions are refused; v1
//! has no implicit migration. Storage authenticates bytes, this envelope also
//! detects truncation and accidental mixing before any provider is invoked.
use osaiku_ui::{report::Contract, Identity};
pub mod storage;
pub const HEADER: usize = 128;
pub const MAX_BYTES: usize = HEADER + osaiku_ui::MAX_WIRE;
pub const DEFINITION: u64 = 1;
pub const SNAPSHOT: u64 = 2;
const MAGIC: &[u8; 8] = b"OSAREP01";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Version,
    Corrupt,
    Contract,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u64,
    /// Application-owned, versioned provider/query configuration.
    pub recipe: u64,
    pub settings: u64,
    pub contract: u64,
    pub recorded: Identity,
    pub mount: u64,
    pub catalog: u64,
}
fn word(b: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(b[n..n + 8].try_into().unwrap())
}
fn hash(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x100000001b3);
    }
    h
}
pub fn contract_key(c: &Contract<'_>) -> u64 {
    let mut h = hash(&c.catalog_version.to_le_bytes(), 0xcbf29ce484222325);
    h = hash(&c.wire_version.to_le_bytes(), h);
    // Length separators prevent concatenated names from aliasing a schema.
    let mut part = |s: &[u8]| {
        h = hash(&(s.len() as u64).to_le_bytes(), h);
        h = hash(s, h);
    };
    for s in c.sources {
        part(s.name.as_bytes());
        part(&s.schema_version.to_le_bytes());
        part(&[s.shape as u8]);
    }
    for f in c.fields {
        part(f.name.as_bytes());
        part(f.source.as_bytes());
        part(f.data_type.name().as_bytes());
    }
    for a in c.actions {
        part(a.name.as_bytes());
        part(&[a.kind as u8]);
    }
    h
}
impl Header {
    /// Seal a payload already encoded directly after HEADER in caller storage.
    pub fn seal(self, bytes: &mut [u8], payload: usize) -> Result<usize, Error> {
        if !matches!(self.kind, DEFINITION | SNAPSHOT)
            || self.recipe == 0
            || self.contract == 0
            || payload == 0
            || payload > osaiku_ui::MAX_WIRE
            || bytes.len() < HEADER + payload
        {
            return Err(Error::Limit);
        }
        bytes[..HEADER].fill(0);
        bytes[..8].copy_from_slice(MAGIC);
        for (i, v) in [
            1,
            self.kind,
            self.recipe,
            self.settings,
            self.contract,
            payload as u64,
            self.recorded.owner,
            self.recorded.window,
            self.recorded.incarnation,
            self.recorded.epoch,
            self.recorded.revision,
            self.mount,
            self.catalog,
        ]
        .iter()
        .enumerate()
        {
            bytes[8 + i * 8..16 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        let sum = hash(
            &bytes[HEADER..HEADER + payload],
            hash(&bytes[..120], 0xcbf29ce484222325),
        );
        bytes[120..128].copy_from_slice(&sum.to_le_bytes());
        Ok(HEADER + payload)
    }
    pub fn parse(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        if bytes.len() < HEADER || bytes.len() > MAX_BYTES || &bytes[..8] != MAGIC {
            return Err(Error::Corrupt);
        }
        if word(bytes, 8) != 1 {
            return Err(Error::Version);
        }
        if !matches!(word(bytes, 16), DEFINITION | SNAPSHOT)
            || word(bytes, 24) == 0
            || word(bytes, 40) == 0
            || word(bytes, 48) == 0
            || word(bytes, 48) != (bytes.len() - HEADER) as u64
            || word(bytes, 112) != 0
            || word(bytes, 120) != hash(&bytes[HEADER..], hash(&bytes[..120], 0xcbf29ce484222325))
        {
            return Err(Error::Corrupt);
        }
        Ok((
            Self {
                kind: word(bytes, 16),
                recipe: word(bytes, 24),
                settings: word(bytes, 32),
                contract: word(bytes, 40),
                recorded: Identity {
                    owner: word(bytes, 56),
                    window: word(bytes, 64),
                    incarnation: word(bytes, 72),
                    epoch: word(bytes, 80),
                    revision: word(bytes, 88),
                },
                mount: word(bytes, 96),
                catalog: word(bytes, 104),
            },
            &bytes[HEADER..],
        ))
    }
    pub fn compatible(self, contract: &Contract<'_>, recipe: u64) -> Result<(), Error> {
        if self.recipe != recipe || self.contract != contract_key(contract) {
            Err(Error::Contract)
        } else {
            Ok(())
        }
    }
}
