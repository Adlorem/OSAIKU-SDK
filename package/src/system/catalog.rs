//! Read-only default catalog authenticated as data inside the system base.
//! Bodies remain on disk. Entries carry ordinary application packages only.
use crate::*;
pub const ROLE: u32 = 32;
pub const MAX_ENTRIES: usize = 32;
pub const HEADER: usize = 64;
pub const RECORD: usize = 64;
pub const MAX_METADATA: usize =
    HEADER + MAX_ENTRIES * (RECORD + HEADER_BYTES + application::MAX_METADATA_BYTES + 7);
pub struct Catalog<'a> {
    bytes: &'a [u8],
}
#[derive(Clone, Copy)]
pub struct Entry<'a> {
    record: &'a [u8],
    metadata: &'a [u8],
}
impl<'a> Catalog<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER || bytes.len() > MAX_METADATA {
            return Err(Error::Length);
        }
        if &bytes[..8] != b"OSACAT01" || u32_at(bytes, 8) != 1 {
            return Err(Error::Format);
        }
        let count = u32_at(bytes, 12) as usize;
        if count > MAX_ENTRIES {
            return Err(Error::Limit);
        }
        if !(1..=MAX_PACKAGE_BYTES).contains(&u64_at(bytes, 16))
            || u64_at(bytes, 24) != bytes.len() as u64
            || bytes[32..64] == [0; 32]
        {
            return Err(Error::Length);
        }
        let mut at = HEADER + count * RECORD;
        if at > bytes.len() {
            return Err(Error::Length);
        }
        let mut body_end = 0;
        for index in 0..count {
            let r = &bytes[HEADER + index * RECORD..HEADER + (index + 1) * RECORD];
            zero(&r[56..64])?;
            let offset = u64_at(r, 0);
            let length = u64_at(r, 8);
            let metadata_at = u32_at(r, 48) as usize;
            let metadata_len = u32_at(r, 52) as usize;
            if offset != (body_end + 7) & !7
                || length <= HEADER_BYTES as u64
                || length > MAX_PACKAGE_BYTES
                || r[16..48] == [0; 32]
                || metadata_at != at
                || !(HEADER_BYTES + application::MANIFEST_BYTES
                    ..=HEADER_BYTES + application::MAX_METADATA_BYTES)
                    .contains(&metadata_len)
            {
                return Err(Error::Layout);
            }
            body_end = offset
                .checked_add(length)
                .filter(|v| *v <= u64_at(bytes, 16))
                .ok_or(Error::Length)?;
            let end = at
                .checked_add(metadata_len)
                .filter(|v| *v <= bytes.len())
                .ok_or(Error::Length)?;
            let metadata = &bytes[at..end];
            Envelope::parse(&metadata[..HEADER_BYTES], length, Domain::Application)?;
            let manifest = application::Manifest::parse(&metadata[HEADER_BYTES..], length)?;
            // Duplicate publisher/id pairs are ambiguous even when releases differ.
            for old in (Self { bytes }).entries().take(index) {
                let old_manifest = old.manifest()?;
                if old_manifest.id() == manifest.id() && old.metadata[32..64] == metadata[32..64] {
                    return Err(Error::Identity);
                }
            }
            at = end;
        }
        if at != bytes.len() || body_end != if count == 0 { 0 } else { u64_at(bytes, 16) } {
            return Err(Error::Length);
        }
        if count == 0 && u64_at(bytes, 16) != 1 {
            return Err(Error::Length);
        }
        Ok(Self { bytes })
    }
    pub fn bundle_bytes(&self) -> u64 {
        u64_at(self.bytes, 16)
    }
    pub fn bundle_digest(&self) -> [u8; 32] {
        self.bytes[32..64].try_into().unwrap()
    }
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + '_ {
        self.bytes[HEADER..HEADER + u32_at(self.bytes, 12) as usize * RECORD]
            .chunks_exact(RECORD)
            .map(|record| {
                let at = u32_at(record, 48) as usize;
                let len = u32_at(record, 52) as usize;
                Entry {
                    record,
                    metadata: &self.bytes[at..at + len],
                }
            })
    }
}
impl<'a> Entry<'a> {
    pub fn offset(&self) -> u64 {
        u64_at(self.record, 0)
    }
    pub fn length(&self) -> u64 {
        u64_at(self.record, 8)
    }
    pub fn digest(&self) -> [u8; 32] {
        self.record[16..48].try_into().unwrap()
    }
    pub fn metadata(&self) -> &'a [u8] {
        self.metadata
    }
    pub fn manifest(&self) -> Result<application::Manifest<'a>, Error> {
        application::Manifest::parse(&self.metadata[HEADER_BYTES..], self.length())
    }
}
