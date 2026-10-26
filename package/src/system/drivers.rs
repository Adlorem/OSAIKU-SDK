//! Immutable early driver set nested in the authenticated system base.
//! Parsing alone confers no authority. Verify with the release owner's policy.
use crate::{
    application::{
        self,
        stream::{VerifiedManifest, Verifier},
    },
    *,
};
pub const ROLE: u32 = 31;
pub const VERSION: u32 = 1;
pub const HEADER: usize = 64;
pub const ROW: usize = 64;
pub const MAX_ENTRIES: usize = 16;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const CRITICAL: u32 = 1;
pub const RECOVERY: u32 = 2;
/// Pre-mutation installer validation without retaining the entire set in RAM.
/// `read` must return exactly the requested bytes from the authenticated source.
pub fn verify_stream(
    length: u64,
    policy: &TrustPolicy<'_>,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    if length < HEADER as u64 || length > MAX_BYTES as u64 {
        return Err(Error::Length);
    }
    let mut table = [0u8; HEADER + MAX_ENTRIES * ROW];
    read(0, &mut table[..HEADER])?;
    let count = u32_at(&table, 12) as usize;
    if &table[..8] != b"OSADSET1"
        || u32_at(&table, 8) != VERSION
        || u64_at(&table, 24) != super::SERVICE_ABI
    {
        return Err(Error::Compatibility);
    }
    if count == 0
        || count > MAX_ENTRIES
        || u64_at(&table, 16) != length
        || length < (HEADER + count * ROW) as u64
    {
        return Err(Error::Length);
    }
    zero(&table[32..HEADER])?;
    read(HEADER as u64, &mut table[HEADER..HEADER + count * ROW])?;
    #[derive(Clone, Copy)]
    struct Identity {
        publisher: [u8; 32],
        id: [u8; 128],
        release: u64,
        digest: [u8; 32],
        flags: u32,
    }
    let mut previous: [Option<Identity>; MAX_ENTRIES] = [None; MAX_ENTRIES];
    let mut end = (HEADER + count * ROW) as u64;
    let mut chunk = [0u8; 1024];
    for i in 0..count {
        let r = &table[HEADER + i * ROW..HEADER + (i + 1) * ROW];
        let flags = u32_at(r, 0);
        let deps = u32_at(r, 4);
        let at = u64_at(r, 8);
        let n = u64_at(r, 16);
        if flags & !3 != 0
            || (flags & RECOVERY != 0 && flags & CRITICAL == 0)
            || deps & !((1 << i) - 1) != 0
        {
            return Err(Error::Manifest);
        }
        zero(&r[56..])?;
        if at != (end + 7) & !7
            || n < (HEADER_BYTES + application::MANIFEST_BYTES) as u64
            || at.checked_add(n).is_none_or(|v| v > length)
        {
            return Err(Error::Layout);
        }
        let padding = (at - end) as usize;
        read(end, &mut chunk[..padding])?;
        zero(&chunk[..padding])?;
        end = at + n;
        let mut header = [0u8; HEADER_BYTES];
        read(at, &mut header)?;
        let mut metadata = [0u8; application::MAX_METADATA_BYTES];
        read(at + HEADER_BYTES as u64, &mut metadata[..512])?;
        let meta = u32_at(&metadata, 12) as usize;
        if meta < 512 || meta > metadata.len() || HEADER_BYTES as u64 + meta as u64 > n {
            return Err(Error::Length);
        }
        read(at + HEADER_BYTES as u64 + 512, &mut metadata[512..meta])?;
        let mut verifier =
            Verifier::new_for(&header, &metadata[..meta], n, policy, Domain::Driver)?;
        let mut pos = HEADER_BYTES as u64 + meta as u64;
        while pos < n {
            let take = (n - pos).min(chunk.len() as u64) as usize;
            read(at + pos, &mut chunk[..take])?;
            verifier.push(&chunk[..take])?;
            pos += take as u64;
        }
        let verified = verifier.finish_verified()?;
        if verified.sha256() != r[24..56] {
            return Err(Error::Digest);
        }
        let m = verified.manifest();
        if m.compatibility().profile().process_abi.major != 1 {
            return Err(Error::Compatibility);
        }
        let mut id = [0; 128];
        id[..m.id().len()].copy_from_slice(m.id().as_bytes());
        let mut declared = 0;
        for d in driver::dependencies(&m) {
            let mut dep_id = [0; 128];
            dep_id[..d.id.len()].copy_from_slice(d.id.as_bytes());
            let found = previous[..i]
                .iter()
                .position(|p| {
                    p.is_some_and(|p| {
                        p.publisher == d.publisher
                            && p.id == dep_id
                            && p.release == d.release
                            && p.digest == d.digest
                    })
                })
                .ok_or(Error::Compatibility)?;
            declared |= 1u32 << found;
        }
        if declared != deps {
            return Err(Error::Compatibility);
        }
        for (j, p) in previous[..i].iter().flatten().enumerate() {
            if p.publisher == verified.publisher() && p.id == id {
                return Err(Error::Identity);
            }
            if deps & (1 << j) != 0 && p.flags & flags != flags {
                return Err(Error::Policy);
            }
        }
        previous[i] = Some(Identity {
            publisher: verified.publisher(),
            id,
            release: m.release(),
            digest: verified.sha256(),
            flags,
        });
    }
    if end != length {
        return Err(Error::Length);
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub struct Entry<'a> {
    record: &'a [u8],
    object: &'a [u8],
}
impl<'a> Entry<'a> {
    pub fn flags(&self) -> u32 {
        u32_at(self.record, 0)
    }
    pub fn dependencies(&self) -> u32 {
        u32_at(self.record, 4)
    }
    pub fn digest(&self) -> [u8; 32] {
        self.record[24..56].try_into().unwrap()
    }
    pub fn bytes(&self) -> &'a [u8] {
        let at = u64_at(self.record, 8) as usize;
        let n = u64_at(self.record, 16) as usize;
        &self.object[at..at + n]
    }
    pub fn package(&self) -> Result<application::Package<'a>, Error> {
        driver::parse(self.bytes())
    }
    pub fn verified(&self, policy: &TrustPolicy<'_>) -> Result<VerifiedManifest, Error> {
        let bytes = self.bytes();
        let p = self.package()?;
        let metadata = p.manifest().raw_metadata();
        let mut v = Verifier::new_for(
            &bytes[..HEADER_BYTES],
            metadata,
            bytes.len() as u64,
            policy,
            Domain::Driver,
        )?;
        v.push(&bytes[HEADER_BYTES + metadata.len()..])?;
        v.finish_verified()
    }
}
pub struct Set<'a> {
    bytes: &'a [u8],
    count: usize,
}
impl<'a> Set<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER || bytes.len() > MAX_BYTES {
            return Err(Error::Length);
        }
        if &bytes[..8] != b"OSADSET1"
            || u32_at(bytes, 8) != VERSION
            || u64_at(bytes, 24) != super::SERVICE_ABI
        {
            return Err(Error::Compatibility);
        }
        let count = u32_at(bytes, 12) as usize;
        if count == 0
            || count > MAX_ENTRIES
            || u64_at(bytes, 16) != bytes.len() as u64
            || bytes.len() < HEADER + count * ROW
        {
            return Err(Error::Length);
        }
        zero(&bytes[32..HEADER])?;
        let set = Self { bytes, count };
        let mut end = HEADER + count * ROW;
        for (i, e) in set.entries().enumerate() {
            let flags = e.flags();
            let deps = e.dependencies();
            if flags & !(CRITICAL | RECOVERY) != 0
                || (flags & RECOVERY != 0 && flags & CRITICAL == 0)
                || deps & !((1u32 << i) - 1) != 0
            {
                return Err(Error::Manifest);
            }
            zero(&e.record[56..64])?;
            let at = usize::try_from(u64_at(e.record, 8)).map_err(|_| Error::Length)?;
            let n = usize::try_from(u64_at(e.record, 16)).map_err(|_| Error::Length)?;
            if at != (end + 7) & !7 || n == 0 || at.checked_add(n).is_none_or(|v| v > bytes.len()) {
                return Err(Error::Layout);
            }
            zero(&bytes[end..at])?;
            end = at + n;
            if digest(e.bytes()) != e.digest() {
                return Err(Error::Digest);
            }
            let p = e.package()?;
            if p.manifest().compatibility().profile().process_abi.major != 1 {
                return Err(Error::Compatibility);
            }
            let mut declared = 0;
            for dep in driver::dependencies(p.manifest()) {
                let mut found = None;
                for (j, d) in set.entries().take(i).enumerate() {
                    let q = d.package()?;
                    if q.publisher() == dep.publisher
                        && q.manifest().id() == dep.id
                        && q.manifest().release() == dep.release
                        && d.digest() == dep.digest
                    {
                        found = Some(j);
                    }
                }
                declared |= 1u32 << found.ok_or(Error::Compatibility)?;
            }
            if declared != deps {
                return Err(Error::Compatibility);
            }
            for (j, d) in set.entries().take(i).enumerate() {
                let q = d.package()?;
                if q.publisher() == p.publisher() && q.manifest().id() == p.manifest().id() {
                    return Err(Error::Identity);
                }
                if deps & (1 << j) != 0 && d.flags() & flags != flags {
                    return Err(Error::Policy);
                }
            }
        }
        if end != bytes.len() {
            return Err(Error::Length);
        }
        Ok(set)
    }
    pub fn entries(&self) -> impl ExactSizeIterator<Item = Entry<'a>> + '_ {
        self.bytes[HEADER..HEADER + self.count * ROW]
            .chunks_exact(ROW)
            .map(|record| Entry {
                record,
                object: self.bytes,
            })
    }
    pub fn verify(&self, policy: &TrustPolicy<'_>) -> Result<(), Error> {
        for entry in self.entries() {
            entry.verified(policy)?;
        }
        Ok(())
    }
    pub fn digest(&self) -> [u8; 32] {
        digest(self.bytes)
    }
}
