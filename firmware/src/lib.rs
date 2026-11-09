#![no_std]
#![forbid(unsafe_code)]
//! Firmware is data, never an executable process or authority to program a device.
//! The caller supplies an authority from the authenticated OS release, never from
//! the artifact. The artifact and its payload stay immutably borrowed throughout
//! upload. Device-specific parsing, DMA, reset, NVM and radio policy are separate.
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const HEADER: usize = 256;
pub const SIGNED: usize = 192;
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const NAME_BYTES: usize = 96;
pub const MAGIC: &[u8; 8] = b"OSAIFW01";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Format,
    Bounds,
    Signature,
    Digest,
    Target,
    Rollback,
    License,
    Offset,
}

/// Release policy binds exact content and a rollback floor. PCI subsystem/DMI
/// identifiers are deliberately absent: the device backend selects a family.
pub struct Policy<'a> {
    pub authority: &'a [u8; 32],
    pub name: &'a str,
    pub family: u32,
    pub api: u32,
    pub minimum_revision: u64,
    pub digest: &'a [u8; 32],
    /// Hash of the redistributable license text shipped alongside the artifact.
    pub license_digest: &'a [u8; 32],
}

/// Manifest compiled into a process authenticated by the signed OS release.
/// This is the alternative to a separately signed OSAIFW01 envelope. The API
/// does not authenticate an arbitrary manifest: its provenance is the caller's
/// obligation, just as Policy::authority must not come from the artifact.
pub struct ReleasePolicy<'a> {
    pub name: &'a str,
    pub family: u32,
    pub api: u32,
    pub revision: u64,
    pub minimum_revision: u64,
    pub bytes: usize,
    pub digest: &'a [u8; 32],
    pub license_digest: &'a [u8; 32],
}

pub struct Verified<'a> {
    payload: &'a [u8],
    name: &'a str,
    family: u32,
    api: u32,
    revision: u64,
    digest: [u8; 32],
}
impl<'a> Verified<'a> {
    /// Both immutable bytes and policy must belong to an authenticated release.
    /// No file chosen by an application, download, or device-provided manifest
    /// may enter through this path. The exact manifest is pinned at build time.
    pub fn from_release(
        payload: &'a [u8],
        license: &[u8],
        policy: &ReleasePolicy<'a>,
    ) -> Result<Self, Error> {
        if payload.is_empty() || payload.len() > MAX_BYTES || payload.len() != policy.bytes {
            return Err(Error::Bounds);
        }
        if policy.name.is_empty()
            || policy.name.len() >= NAME_BYTES
            || !policy
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || policy.name.as_bytes().windows(2).any(|b| b == b"..")
            || policy.family == 0
            || policy.api == 0
        {
            return Err(Error::Target);
        }
        if policy.revision < policy.minimum_revision {
            return Err(Error::Rollback);
        }
        let digest: [u8; 32] = Sha256::digest(payload).into();
        if &digest != policy.digest {
            return Err(Error::Digest);
        }
        let license_digest: [u8; 32] = Sha256::digest(license).into();
        if &license_digest != policy.license_digest {
            return Err(Error::License);
        }
        Ok(Self {
            payload,
            name: policy.name,
            family: policy.family,
            api: policy.api,
            revision: policy.revision,
            digest,
        })
    }

    pub fn open(artifact: &'a [u8], license: &[u8], policy: &Policy<'_>) -> Result<Self, Error> {
        if artifact.len() < HEADER || artifact.len() > HEADER + MAX_BYTES {
            return Err(Error::Bounds);
        }
        let u32_at = |n: usize| u32::from_le_bytes(artifact[n..n + 4].try_into().unwrap());
        let u64_at = |n: usize| u64::from_le_bytes(artifact[n..n + 8].try_into().unwrap());
        if &artifact[..8] != MAGIC
            || u32_at(8) != HEADER as u32
            || u32_at(12) != 1
            || artifact[176..SIGNED].iter().any(|b| *b != 0)
        {
            return Err(Error::Format);
        }
        let len = usize::try_from(u64_at(32)).map_err(|_| Error::Bounds)?;
        if len == 0 || len > MAX_BYTES || len.checked_add(HEADER) != Some(artifact.len()) {
            return Err(Error::Bounds);
        }
        let name_len = artifact[80..176]
            .iter()
            .position(|b| *b == 0)
            .ok_or(Error::Format)?;
        let name_bytes = &artifact[80..80 + name_len];
        if name_bytes.is_empty()
            || !name_bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"-_ .".contains(b))
            || name_bytes.contains(&b' ')
            || name_bytes.windows(2).any(|b| b == b"..")
            || artifact[80 + name_len..176].iter().any(|b| *b != 0)
        {
            return Err(Error::Format);
        }
        let name = core::str::from_utf8(name_bytes).map_err(|_| Error::Format)?;
        VerifyingKey::from_bytes(policy.authority)
            .map_err(|_| Error::Signature)?
            .verify_strict(
                &artifact[..SIGNED],
                &Signature::from_slice(&artifact[SIGNED..HEADER]).map_err(|_| Error::Signature)?,
            )
            .map_err(|_| Error::Signature)?;
        let family = u32_at(16);
        let api = u32_at(20);
        let revision = u64_at(24);
        if name != policy.name || family != policy.family || api != policy.api {
            return Err(Error::Target);
        }
        if revision < policy.minimum_revision {
            return Err(Error::Rollback);
        }
        let digest: [u8; 32] = Sha256::digest(&artifact[HEADER..]).into();
        if digest != artifact[40..72] || &digest != policy.digest {
            return Err(Error::Digest);
        }
        // The full license hash is pinned by the signed release policy. The
        // envelope contains its first 8 bytes, not a caller-selected license.
        let license_digest: [u8; 32] = Sha256::digest(license).into();
        if &license_digest != policy.license_digest || artifact[72..80] != license_digest[..8] {
            return Err(Error::License);
        }
        Ok(Self {
            payload: &artifact[HEADER..],
            name,
            family,
            api,
            revision,
            digest,
        })
    }
    pub fn bytes(&self) -> &'a [u8] {
        self.payload
    }
    pub fn name(&self) -> &'a str {
        self.name
    }
    pub fn family(&self) -> u32 {
        self.family
    }
    pub fn api(&self) -> u32 {
        self.api
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
    /// A copied fragment; no physical address or device-memory offset is exposed.
    pub fn read(&self, offset: usize, out: &mut [u8]) -> Result<usize, Error> {
        let remaining = self.payload.get(offset..).ok_or(Error::Offset)?;
        let n = out.len().min(remaining.len());
        out[..n].copy_from_slice(&remaining[..n]);
        out[n..].fill(0);
        Ok(n)
    }
}
