//! Authenticated system release and bounded base container. Role interpretation
//! belongs to the kernel; UEFI authenticates the opaque object as a whole.
use crate::*;
pub mod catalog;
pub const HANDOFF_REVISION: u32 = 7;
pub const BASE_VERSION: u32 = 1;
pub const DESCRIPTOR_BYTES: usize = 128;
pub const MAX_BASE_ENTRIES: usize = 32;
pub const BASE_HEADER_BYTES: usize = 32;
pub const BASE_ENTRY_BYTES: usize = 64;
pub const SERVICE_ABI: u64 = 1;
/// Fixed recovery profile. Role 32 contains catalog metadata, never code.
pub const BASE_PROFILE_ROLES: [u32; 15] = [1, 2, 3, 4, 5, 6, 7, 11, 12, 13, 14, 15, 16, 17, 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseEntry {
    pub role: u32,
    pub offset: u64,
    pub length: u64,
    pub digest: [u8; 32],
    pub abi: u32,
}

/// Bootloader-visible structural metadata. Service interpretation belongs to
/// the kernel's service ABI, not UEFI. The signed release authenticates the
/// complete base object; this parser alone conveys no trust.
pub struct BaseMetadata<'a> {
    bytes: &'a [u8],
}
impl<'a> BaseMetadata<'a> {
    pub fn parse(bytes: &'a [u8], object_bytes: u64) -> Result<Self, Error> {
        if bytes.len() < BASE_HEADER_BYTES
            || bytes.len() > BASE_HEADER_BYTES + MAX_BASE_ENTRIES * BASE_ENTRY_BYTES
        {
            return Err(Error::Length);
        }
        if &bytes[..8] != b"OSABASE1" || u32_at(bytes, 8) != BASE_VERSION {
            return Err(Error::Format);
        }
        let count = u32_at(bytes, 12) as usize;
        if count == 0
            || count > MAX_BASE_ENTRIES
            || bytes.len() != BASE_HEADER_BYTES + count * BASE_ENTRY_BYTES
        {
            return Err(Error::Limit);
        }
        if u64_at(bytes, 16) != object_bytes || object_bytes > MAX_PACKAGE_BYTES {
            return Err(Error::Length);
        }
        zero(&bytes[24..32])?;
        let mut previous = 0;
        let mut end = bytes.len() as u64;
        for r in bytes[BASE_HEADER_BYTES..].chunks_exact(BASE_ENTRY_BYTES) {
            let role = u32_at(r, 0);
            if role <= previous || role > MAX_BASE_ENTRIES as u32 || u32_at(r, 56) == 0 {
                return Err(Error::Identity);
            }
            previous = role;
            zero(&r[4..8])?;
            zero(&r[60..64])?;
            let offset = u64_at(r, 8);
            let length = u64_at(r, 16);
            if offset != end.checked_add(7).ok_or(Error::Layout)? & !7
                || length == 0
                || length > application::MAX_ELF_BYTES
            {
                return Err(Error::Layout);
            }
            if r[24..56].iter().all(|&b| b == 0) {
                return Err(Error::Digest);
            }
            end = offset.checked_add(length).ok_or(Error::Layout)?;
        }
        if end != object_bytes {
            return Err(Error::Length);
        }
        Ok(Self { bytes })
    }
    pub fn service_count(&self) -> usize {
        u32_at(self.bytes, 12) as usize
    }
    pub fn validate_profile(&self) -> Result<(), Error> {
        if self.service_count() != BASE_PROFILE_ROLES.len()
            || self
                .entries()
                .zip(BASE_PROFILE_ROLES)
                .any(|(entry, role)| entry.role != role || u64::from(entry.abi) != SERVICE_ABI)
        {
            return Err(Error::Compatibility);
        }
        Ok(())
    }
    pub fn entries(&self) -> impl Iterator<Item = BaseEntry> + '_ {
        self.bytes[BASE_HEADER_BYTES..]
            .chunks_exact(BASE_ENTRY_BYTES)
            .map(|r| BaseEntry {
                role: u32_at(r, 0),
                offset: u64_at(r, 8),
                length: u64_at(r, 16),
                digest: r[24..56].try_into().unwrap(),
                abi: u32_at(r, 56),
            })
    }
    /// Validate an entire immutable object, including alignment padding. No
    /// reference to an entry is exposed before all component hashes match.
    pub fn validate_object(object: &'a [u8]) -> Result<Self, Error> {
        let count = object.get(12..16).ok_or(Error::Length)?;
        let count = u32::from_le_bytes(count.try_into().unwrap()) as usize;
        if count == 0 || count > MAX_BASE_ENTRIES {
            return Err(Error::Limit);
        }
        let end = BASE_HEADER_BYTES + count * BASE_ENTRY_BYTES;
        let metadata = Self::parse(object.get(..end).ok_or(Error::Length)?, object.len() as u64)?;
        let mut previous = end;
        for entry in metadata.entries() {
            let first = usize::try_from(entry.offset).map_err(|_| Error::Length)?;
            let last = usize::try_from(entry.offset + entry.length).map_err(|_| Error::Length)?;
            zero(&object[previous..first])?;
            if digest(&object[first..last]) != entry.digest {
                return Err(Error::Digest);
            }
            previous = last;
        }
        Ok(metadata)
    }
}
pub struct Release<'a> {
    bytes: &'a [u8],
}
impl<'a> Release<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() != HEADER_BYTES + DESCRIPTOR_BYTES {
            return Err(Error::Length);
        }
        Envelope::parse(&bytes[..HEADER_BYTES], bytes.len() as u64, Domain::System)?;
        let p = &bytes[HEADER_BYTES..];
        if u64_at(p, 0) == 0
            || u32_at(p, 8) != HANDOFF_REVISION
            || u32_at(p, 12) != BASE_VERSION
            || u64_at(p, 16) == 0
        {
            return Err(Error::Format);
        }
        for at in [24, 64] {
            if !(1..=MAX_PACKAGE_BYTES).contains(&u64_at(p, at))
                || p[at + 8..at + 40].iter().all(|&b| b == 0)
            {
                return Err(Error::Length);
            }
        }
        zero(&p[104..128])?;
        Ok(Self { bytes })
    }
    /// Signature binds release, handoff/base versions, service ABI, sizes and
    /// SHA-256 of both files. A8 verifies those files against these fields.
    pub fn verify(&self, policy: &TrustPolicy<'_>) -> Result<[u8; 32], Error> {
        verify_bytes(self.bytes, Domain::System, policy)
    }
    pub fn generation(&self) -> u64 {
        u64_at(self.bytes, HEADER_BYTES)
    }
    pub fn service_abi(&self) -> u64 {
        u64_at(self.bytes, HEADER_BYTES + 16)
    }
    pub fn kernel_bytes(&self) -> u64 {
        u64_at(self.bytes, HEADER_BYTES + 24)
    }
    pub fn kernel_digest(&self) -> [u8; 32] {
        self.bytes[HEADER_BYTES + 32..HEADER_BYTES + 64]
            .try_into()
            .unwrap()
    }
    pub fn base_bytes(&self) -> u64 {
        u64_at(self.bytes, HEADER_BYTES + 64)
    }
    pub fn base_digest(&self) -> [u8; 32] {
        self.bytes[HEADER_BYTES + 72..HEADER_BYTES + 104]
            .try_into()
            .unwrap()
    }
    pub fn verify_objects(&self, kernel: &[u8], base: &[u8]) -> Result<(), Error> {
        if self.service_abi() != SERVICE_ABI {
            return Err(Error::Compatibility);
        }
        if kernel.len() as u64 != self.kernel_bytes() || base.len() as u64 != self.base_bytes() {
            return Err(Error::Length);
        }
        if digest(kernel) != self.kernel_digest() || digest(base) != self.base_digest() {
            return Err(Error::Digest);
        }
        Ok(())
    }
}
