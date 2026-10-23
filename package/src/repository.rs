use crate::application::VerifiedPackage;
use crate::*;
pub const PREFIX_BYTES: usize = 160;
pub const ENTRY_BYTES: usize = 512;
pub const MAX_ENTRIES: usize = 1024;
pub const MAX_VALIDITY_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    bytes: &'a [u8],
}
impl<'a> Entry<'a> {
    pub fn publisher(&self) -> [u8; 32] {
        self.bytes[..32].try_into().unwrap()
    }
    pub fn id(&self) -> &'a str {
        text(&self.bytes[32..160]).unwrap()
    }
    pub fn release(&self) -> u64 {
        u64_at(self.bytes, 160)
    }
    pub fn package_bytes(&self) -> u64 {
        u64_at(self.bytes, 168)
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.bytes[176..208].try_into().unwrap()
    }
    pub fn url(&self) -> &'a str {
        text(&self.bytes[216..512]).unwrap()
    }
    pub fn bind(&self, package: &VerifiedPackage<'_>) -> Result<(), Error> {
        let profile = package.manifest().compatibility().profile();
        if self.publisher() != package.publisher()
            || self.id() != package.manifest().id()
            || self.release() != package.manifest().release()
            || self.package_bytes() != package.length()
            || self.sha256() != package.sha256()
            || u16_at(self.bytes, 208) != profile.architecture
            || u16_at(self.bytes, 210) != profile.process_abi.major
            || u16_at(self.bytes, 212) != profile.process_abi.minor
        {
            Err(Error::Binding)
        } else {
            Ok(())
        }
    }
}
pub struct RepositoryState<'a> {
    pub id: &'a str,
    pub sequence: u64,
    pub digest: [u8; 32],
}
#[derive(Debug)]
pub struct Index<'a> {
    bytes: &'a [u8],
    payload: &'a [u8],
}
impl<'a> Index<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        Envelope::parse(
            bytes.get(..HEADER_BYTES).ok_or(Error::Length)?,
            bytes.len() as u64,
            Domain::Repository,
        )?;
        let payload = bytes.get(HEADER_BYTES..).ok_or(Error::Length)?;
        if payload.len() < PREFIX_BYTES {
            return Err(Error::Length);
        }
        if !identifier(text(&payload[..128])?) || u64_at(payload, 128) == 0 {
            return Err(Error::Repository);
        }
        let count = u32_at(payload, 152) as usize;
        if count > MAX_ENTRIES {
            return Err(Error::Limit);
        }
        if payload.len() != PREFIX_BYTES + count * ENTRY_BYTES {
            return Err(Error::Length);
        }
        zero(&payload[156..160])?;
        let issued = u64_at(payload, 136);
        let expires = u64_at(payload, 144);
        if issued == 0 || expires <= issued || expires - issued > MAX_VALIDITY_SECONDS {
            return Err(Error::Time);
        }
        let mut previous: Option<&[u8]> = None;
        for record in payload[PREFIX_BYTES..].chunks_exact(ENTRY_BYTES) {
            let id = text(&record[32..160])?;
            if !identifier(id)
                || application::reserved_identity(id)
                || record[..32].iter().all(|&b| b == 0)
            {
                return Err(Error::Identity);
            }
            if previous.is_some_and(|p| p >= &record[..160]) {
                return Err(Error::Identity);
            }
            previous = Some(&record[..160]);
            if u64_at(record, 160) == 0
                || !(HEADER_BYTES as u64 + 1..=MAX_PACKAGE_BYTES).contains(&u64_at(record, 168))
                || u16_at(record, 208) == 0
                || u16_at(record, 210) == 0
            {
                return Err(Error::Manifest);
            }
            zero(&record[214..216])?;
            if !https_url(text(&record[216..512])?) {
                return Err(Error::Url);
            }
        }
        Ok(Self { bytes, payload })
    }
    pub fn id(&self) -> &'a str {
        text(&self.payload[..128]).unwrap()
    }
    pub fn sequence(&self) -> u64 {
        u64_at(self.payload, 128)
    }
    pub fn entries(&self) -> impl ExactSizeIterator<Item = Entry<'a>> + '_ {
        self.payload[PREFIX_BYTES..]
            .chunks_exact(ENTRY_BYTES)
            .map(|bytes| Entry { bytes })
    }
    /// Returned checkpoint MUST be committed durably before any entry is used.
    /// `now` must come from the trusted clock, never the downloaded index.
    pub fn verify(
        &self,
        policy: &TrustPolicy<'_>,
        now: Option<u64>,
        state: &RepositoryState<'_>,
    ) -> Result<[u8; 32], Error> {
        let hash = verify_bytes(self.bytes, Domain::Repository, policy)?;
        let now = now.ok_or(Error::Time)?;
        if now < u64_at(self.payload, 136) || now >= u64_at(self.payload, 144) {
            return Err(Error::Time);
        }
        if state.id != self.id() {
            return Err(Error::Repository);
        }
        if self.sequence() < state.sequence {
            return Err(Error::Rollback);
        }
        if self.sequence() == state.sequence && hash != state.digest {
            return Err(Error::Equivocation);
        }
        Ok(hash)
    }
}
/// Deliberately narrow v1 URL grammar: HTTPS/443, DNS hostname and an ASCII
/// path. No credentials, redirects, IP literals, escapes, queries or fragments.
/// The network broker still applies destination/DNS policy at connection time.
pub fn https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    if host.len() > 253 || host.is_empty() || path.is_empty() {
        return false;
    }
    if !host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) {
        return false;
    }
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return false;
    }
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    })
}
