//! Frozen A0 system-release descriptor. A8 connects this contract to UEFI,
//! BootInfo revision 7 and the installer; it does not change today's boot.
use crate::*;
pub const HANDOFF_REVISION: u32 = 7;
pub const BASE_VERSION: u32 = 1;
pub const DESCRIPTOR_BYTES: usize = 128;
pub const MAX_BASE_ENTRIES: usize = 32;
pub const BASE_HEADER_BYTES: usize = 32;
pub const BASE_ENTRY_BYTES: usize = 64;

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
}
