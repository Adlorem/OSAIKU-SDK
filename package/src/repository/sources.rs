//! Local source configuration and candidate identities. Network retrieval is a
//! separate HTTPS broker contract; parsing metadata never grants publisher trust.
use super::{https_url, signature_domain, Entry, Index, RepositoryState};
use crate::{digest, identifier, Domain, Error, TrustPolicy, TrustedKey};
pub const BYTES: usize = 512;
pub const CAPACITY: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub domain: Domain,
    pub id: [u8; 128],
    pub address: [u8; 296],
    pub key: [u8; 32],
    pub channel: [u8; 24],
    pub priority: u16,
    pub enabled: bool,
    pub minimum_epoch: u64,
    pub architecture: u16,
    pub process_major: u16,
    pub process_minor: u16,
}
fn text(b: &[u8]) -> Result<&str, Error> {
    crate::text(b)
}
impl Source {
    pub fn validate(&self) -> Result<(), Error> {
        signature_domain(self.domain)?;
        if !identifier(text(&self.id)?)
            || !https_url(text(&self.address)?)
            || !identifier(text(&self.channel)?)
            || self.minimum_epoch == 0
            || self.architecture == 0
            || self.process_major == 0
        {
            return Err(Error::Repository);
        }
        let key = ed25519_dalek::VerifyingKey::from_bytes(&self.key).map_err(|_| Error::Policy)?;
        if key.is_weak() {
            return Err(Error::Policy);
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<[u8; BYTES], Error> {
        self.validate()?;
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"OSASRC01");
        b[8] = if self.domain == Domain::Application {
            1
        } else {
            4
        };
        b[9] = self.enabled as u8;
        b[10..12].copy_from_slice(&self.priority.to_le_bytes());
        b[12..14].copy_from_slice(&self.architecture.to_le_bytes());
        b[14..16].copy_from_slice(&self.process_major.to_le_bytes());
        b[16..18].copy_from_slice(&self.process_minor.to_le_bytes());
        b[24..32].copy_from_slice(&self.minimum_epoch.to_le_bytes());
        b[32..160].copy_from_slice(&self.id);
        b[160..456].copy_from_slice(&self.address);
        b[456..488].copy_from_slice(&self.key);
        b[488..].copy_from_slice(&self.channel);
        Ok(b)
    }
    pub fn parse(b: &[u8], expected: Domain) -> Result<Self, Error> {
        if b.len() != BYTES || &b[..8] != b"OSASRC01" || b[9] > 1 || b[18..24] != [0; 6] {
            return Err(Error::Format);
        }
        let domain = match b[8] {
            1 => Domain::Application,
            4 => Domain::Driver,
            _ => return Err(Error::Policy),
        };
        if domain != expected {
            return Err(Error::Policy);
        }
        let source = Self {
            domain,
            id: b[32..160].try_into().unwrap(),
            address: b[160..456].try_into().unwrap(),
            key: b[456..488].try_into().unwrap(),
            channel: b[488..512].try_into().unwrap(),
            priority: crate::u16_at(b, 10),
            enabled: b[9] == 1,
            minimum_epoch: crate::u64_at(b, 24),
            architecture: crate::u16_at(b, 12),
            process_major: crate::u16_at(b, 14),
            process_minor: crate::u16_at(b, 16),
        };
        source.validate()?;
        Ok(source)
    }
    /// Includes origin, domain, channel and trust configuration. Rotation or
    /// source changes cannot reuse another configuration's download/cache entry.
    pub fn identity(&self) -> Result<[u8; 32], Error> {
        Ok(digest(&self.encode()?))
    }
    pub fn verify<'a>(
        &self,
        bytes: &'a [u8],
        epoch: u64,
        now: Option<u64>,
        state: &RepositoryState<'_>,
    ) -> Result<VerifiedIndex<'a>, Error> {
        self.validate()?;
        if !self.enabled {
            return Err(Error::Policy);
        }
        let index = Index::parse_for(bytes, self.domain)?;
        if index.id() != text(&self.id)? {
            return Err(Error::Repository);
        }
        let keys = [TrustedKey {
            public_key: self.key,
            domain: signature_domain(self.domain)?,
            minimum_epoch: self.minimum_epoch,
            revoked: false,
        }];
        let hash = index.verify(&TrustPolicy { epoch, keys: &keys }, now, state)?;
        Ok(VerifiedIndex {
            index,
            source: *self,
            digest: hash,
        })
    }
}
pub struct VerifiedIndex<'a> {
    index: Index<'a>,
    source: Source,
    digest: [u8; 32],
}
impl<'a> VerifiedIndex<'a> {
    pub fn checkpoint(&self) -> (u64, [u8; 32]) {
        (self.index.sequence(), self.digest)
    }
    /// The caller durably checkpoints before using candidates. Each candidate
    /// remains bound to this exact source, package domain and authenticated index.
    pub fn candidate(
        &self,
        publisher: [u8; 32],
        id: &str,
        installed: u64,
    ) -> Option<Candidate<'a>> {
        self.index
            .entries()
            .find(|e| {
                e.publisher() == publisher
                    && e.id() == id
                    && e.release() > installed
                    && crate::u16_at(e.bytes, 208) == self.source.architecture
                    && crate::u16_at(e.bytes, 210) == self.source.process_major
                    && crate::u16_at(e.bytes, 212) <= self.source.process_minor
            })
            .map(|entry| Candidate {
                entry,
                domain: self.source.domain,
                source: self.source.identity().unwrap(),
                index: self.digest,
            })
    }
}
pub struct Candidate<'a> {
    entry: Entry<'a>,
    domain: Domain,
    source: [u8; 32],
    index: [u8; 32],
}
impl Candidate<'_> {
    pub fn cache_key(&self) -> [u8; 32] {
        let mut b = [0; 96];
        b[..32].copy_from_slice(&self.source);
        b[32..64].copy_from_slice(&self.index);
        b[64..].copy_from_slice(&self.entry.sha256());
        digest(&b)
    }
    pub fn bind(
        &self,
        domain: Domain,
        source: &Source,
        package: &crate::application::VerifiedPackage<'_>,
    ) -> Result<(), Error> {
        if domain != self.domain || source.identity()? != self.source || !source.enabled {
            return Err(Error::Policy);
        }
        self.entry.bind(package)
    }
    pub fn url(&self) -> &str {
        self.entry.url()
    }
}
/// Each owner holds its own set. A foreign API cannot insert or enumerate it.
pub struct Sources {
    domain: Domain,
    entries: [Option<Source>; CAPACITY],
}
impl Sources {
    pub fn new(domain: Domain) -> Result<Self, Error> {
        signature_domain(domain)?;
        Ok(Self {
            domain,
            entries: [None; CAPACITY],
        })
    }
    pub fn set(&mut self, caller_domain: Domain, source: Source) -> Result<(), Error> {
        if caller_domain != self.domain || source.domain != self.domain {
            return Err(Error::Policy);
        }
        source.validate()?;
        let slot = self
            .entries
            .iter()
            .position(|s| s.is_some_and(|s| s.id == source.id))
            .or_else(|| self.entries.iter().position(Option::is_none))
            .ok_or(Error::Limit)?;
        self.entries[slot] = Some(source);
        Ok(())
    }
    pub fn list(&self, caller_domain: Domain) -> Result<impl Iterator<Item = &Source>, Error> {
        if caller_domain != self.domain {
            return Err(Error::Policy);
        }
        Ok(self.entries.iter().flatten())
    }
}
