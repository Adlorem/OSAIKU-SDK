#![no_std]
//! Application distribution contracts. No allocation, filesystem, private-key
//! loading, execution, or capability grants. Policy is trusted host input.
//! Verification must operate on one immutable source and policy snapshot;
//! publication and launch recheck policy and pin that exact revision.

pub mod application;
pub mod compatibility;
pub mod driver;
pub mod policy;
pub mod repository;
pub mod system;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const HEADER_BYTES: usize = 192;
pub const CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
pub const MAX_KEYS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Domain {
    Application,
    Repository,
    System,
    Driver,
    DriverInput,
    DriverStorage,
    DriverController,
}
impl Domain {
    pub fn magic(self) -> &'static [u8; 8] {
        match self {
            Self::Application => b"OSAPPK01",
            Self::Repository => b"OSAIDX01",
            Self::System => b"OSASYS01",
            Self::Driver => b"OSADRV01",
            Self::DriverInput => b"OSADIN01",
            Self::DriverStorage => b"OSADST01",
            Self::DriverController => b"OSADCT01",
        }
    }
    pub fn signature_domain(self) -> &'static [u8] {
        match self {
            Self::Application => b"OSAIKU/application-package/v1\0",
            Self::Repository => b"OSAIKU/repository-index/v1\0",
            Self::System => b"OSAIKU/system-release/v1\0",
            Self::Driver => b"OSAIKU/driver-package/v1\0",
            Self::DriverInput => b"OSAIKU/driver-input/v1\0",
            Self::DriverStorage => b"OSAIKU/driver-storage/v1\0",
            Self::DriverController => b"OSAIKU/driver-controller/v1\0",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Format,
    Length,
    Limit,
    Reserved,
    Identity,
    Manifest,
    Compatibility,
    Layout,
    Digest,
    Signature,
    UnknownKey,
    RevokedKey,
    PolicyEpoch,
    Policy,
    Chunk,
    Incomplete,
    Failed,
    Elf,
    Icon,
    Time,
    Rollback,
    Equivocation,
    Repository,
    Url,
    Binding,
}

#[derive(Clone, Copy, Debug)]
pub struct TrustedKey {
    pub public_key: [u8; 32],
    pub domain: Domain,
    pub minimum_epoch: u64,
    pub revoked: bool,
}

/// Construct only from host-approved, rollback-protected policy, never from
/// package contents. Keys are scoped independently to each signature domain.
pub struct TrustPolicy<'a> {
    pub epoch: u64,
    pub keys: &'a [TrustedKey],
}
impl TrustPolicy<'_> {
    fn key(&self, id: &[u8; 32], domain: Domain, epoch: u64) -> Result<VerifyingKey, Error> {
        if self.epoch == 0 || self.keys.len() > MAX_KEYS {
            return Err(Error::Policy);
        }
        let mut found = None;
        for (i, key) in self.keys.iter().enumerate() {
            if key.minimum_epoch == 0 || key.minimum_epoch > self.epoch {
                return Err(Error::Policy);
            }
            if self.keys[..i]
                .iter()
                .any(|old| old.domain == key.domain && old.public_key == key.public_key)
            {
                return Err(Error::Policy);
            }
            if key.domain == domain && digest(&key.public_key) == *id {
                found = Some(key);
            }
        }
        let key = found.ok_or(Error::UnknownKey)?;
        if key.revoked {
            return Err(Error::RevokedKey);
        }
        if epoch == 0 || epoch < key.minimum_epoch || epoch > self.epoch {
            return Err(Error::PolicyEpoch);
        }
        VerifyingKey::from_bytes(&key.public_key).map_err(|_| Error::Policy)
    }
}

#[derive(Debug)]
pub struct Envelope<'a> {
    header: &'a [u8],
    domain: Domain,
}
impl<'a> Envelope<'a> {
    pub fn parse(header: &'a [u8], object_bytes: u64, domain: Domain) -> Result<Self, Error> {
        if header.len() != HEADER_BYTES {
            return Err(Error::Length);
        }
        if &header[..8] != domain.magic()
            || u32_at(header, 8) != 1
            || u32_at(header, 12) != HEADER_BYTES as u32
        {
            return Err(Error::Format);
        }
        let limit = if domain == Domain::Repository {
            MAX_INDEX_BYTES
        } else {
            MAX_PACKAGE_BYTES
        };
        if object_bytes > limit {
            return Err(Error::Limit);
        }
        if object_bytes <= HEADER_BYTES as u64
            || u64_at(header, 16) != object_bytes
            || u64_at(header, 96).checked_add(HEADER_BYTES as u64) != Some(object_bytes)
        {
            return Err(Error::Length);
        }
        zero(&header[104..128])?;
        Ok(Self { header, domain })
    }
    pub fn key_id(&self) -> [u8; 32] {
        self.header[32..64].try_into().unwrap()
    }
    pub fn epoch(&self) -> u64 {
        u64_at(self.header, 24)
    }
    pub fn payload_bytes(&self) -> u64 {
        u64_at(self.header, 96)
    }
    pub fn authenticate(&self, policy: &TrustPolicy<'_>) -> Result<(), Error> {
        let key = policy.key(&self.key_id(), self.domain, self.epoch())?;
        // Maximum domain length plus the signed 128-byte prefix; no payload-sized
        // copy or allocation, and no raw struct serialization.
        let mut message = [0u8; 192];
        let domain = self.domain.signature_domain();
        message[..domain.len()].copy_from_slice(domain);
        message[domain.len()..domain.len() + 128].copy_from_slice(&self.header[..128]);
        let signature = Signature::from_bytes(self.header[128..192].try_into().unwrap());
        key.verify_strict(&message[..domain.len() + 128], &signature)
            .map_err(|_| Error::Signature)
    }
    /// Caller pumps at most 64 KiB per step and may cancel by dropping the state.
    pub fn begin(&self, policy: &TrustPolicy<'_>) -> Result<ContentVerifier, Error> {
        self.authenticate(policy)?;
        let mut whole = Sha256::new();
        whole.update(self.header);
        Ok(ContentVerifier {
            remaining: self.payload_bytes(),
            expected: self.header[64..96].try_into().unwrap(),
            payload: Sha256::new(),
            whole,
            failed: false,
        })
    }
}

pub struct ContentVerifier {
    remaining: u64,
    expected: [u8; 32],
    payload: Sha256,
    whole: Sha256,
    failed: bool,
}
impl ContentVerifier {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Failed);
        }
        if bytes.is_empty() || bytes.len() > CHUNK_BYTES || bytes.len() as u64 > self.remaining {
            self.failed = true;
            return Err(Error::Chunk);
        }
        self.payload.update(bytes);
        self.whole.update(bytes);
        self.remaining -= bytes.len() as u64;
        Ok(())
    }
    /// Content digest only. Does not validate manifest/ELF or publish storage.
    pub fn finish(self) -> Result<[u8; 32], Error> {
        if self.failed {
            return Err(Error::Failed);
        }
        if self.remaining != 0 {
            return Err(Error::Incomplete);
        }
        if <[u8; 32]>::from(self.payload.finalize()) != self.expected {
            return Err(Error::Digest);
        }
        Ok(self.whole.finalize().into())
    }
}

pub(crate) fn verify_bytes(
    bytes: &[u8],
    domain: Domain,
    policy: &TrustPolicy<'_>,
) -> Result<[u8; 32], Error> {
    let envelope = Envelope::parse(
        bytes.get(..HEADER_BYTES).ok_or(Error::Length)?,
        bytes.len() as u64,
        domain,
    )?;
    let mut verifier = envelope.begin(policy)?;
    for chunk in bytes[HEADER_BYTES..].chunks(CHUNK_BYTES) {
        verifier.push(chunk)?;
    }
    verifier.finish()
}
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub(crate) fn zero(bytes: &[u8]) -> Result<(), Error> {
    if bytes.iter().any(|&b| b != 0) {
        Err(Error::Reserved)
    } else {
        Ok(())
    }
}
pub(crate) fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
pub(crate) fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub(crate) fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
pub(crate) fn text(bytes: &[u8]) -> Result<&str, Error> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    if end == 0 {
        return Err(Error::Identity);
    }
    zero(&bytes[end..])?;
    let value = core::str::from_utf8(&bytes[..end]).map_err(|_| Error::Identity)?;
    if value
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    {
        return Err(Error::Identity);
    }
    Ok(value)
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && part
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
