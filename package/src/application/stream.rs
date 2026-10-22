//! Same canonical manifest/component rules as Package, with bounded memory.
//! The constructor authenticates and consumes metadata; push accepts only the
//! remaining bytes, in order. Any failure poisons the verifier permanently.
use super::*;

pub struct Verifier {
    metadata: [u8; MAX_METADATA_BYTES],
    metadata_len: usize,
    total: u64,
    position: u64,
    component: usize,
    hash: Sha256,
    prefix: [u8; 64],
    prefix_len: usize,
    content: ContentVerifier,
    failed: bool,
    publisher: [u8; 32],
}
impl Verifier {
    pub fn new(
        header: &[u8],
        metadata: &[u8],
        total: u64,
        policy: &TrustPolicy<'_>,
    ) -> Result<Self, Error> {
        Manifest::parse(metadata, total)?;
        let mut content = Envelope::parse(header, total, Domain::Application)?.begin(policy)?;
        content.push(metadata)?;
        let mut owned = [0; MAX_METADATA_BYTES];
        owned[..metadata.len()].copy_from_slice(metadata);
        Ok(Self {
            metadata: owned,
            metadata_len: metadata.len(),
            total,
            position: (HEADER_BYTES + metadata.len()) as u64,
            component: 0,
            hash: Sha256::new(),
            prefix: [0; 64],
            prefix_len: 0,
            content,
            failed: false,
            publisher: Envelope::parse(header, total, Domain::Application)?.key_id(),
        })
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Failed);
        }
        let result = self.push_inner(bytes);
        self.failed = result.is_err();
        result
    }
    fn push_inner(&mut self, mut bytes: &[u8]) -> Result<(), Error> {
        self.content.push(bytes)?;
        let manifest = Manifest::parse(&self.metadata[..self.metadata_len], self.total)?;
        while !bytes.is_empty() {
            let c = manifest
                .components()
                .nth(self.component)
                .ok_or(Error::Length)?;
            if self.position < c.offset() {
                let n = bytes.len().min((c.offset() - self.position) as usize);
                zero(&bytes[..n])?;
                self.position += n as u64;
                bytes = &bytes[n..];
                continue;
            }
            let n = bytes
                .len()
                .min((c.offset() + c.length() - self.position) as usize);
            self.hash.update(&bytes[..n]);
            let prefix = n.min(self.prefix.len() - self.prefix_len);
            self.prefix[self.prefix_len..self.prefix_len + prefix]
                .copy_from_slice(&bytes[..prefix]);
            self.prefix_len += prefix;
            self.position += n as u64;
            bytes = &bytes[n..];
            if self.position == c.offset() + c.length() {
                if <[u8; 32]>::from(self.hash.clone().finalize()) != c.sha256() {
                    return Err(Error::Digest);
                }
                match c.kind() {
                    1 => elf_header(&self.prefix[..self.prefix_len])?,
                    3 => icon_prefix(&self.prefix[..self.prefix_len], c.length())?,
                    _ => {}
                }
                self.component += 1;
                self.hash = Sha256::new();
                self.prefix_len = 0;
            }
        }
        Ok(())
    }
    pub fn finish(self) -> Result<[u8; 32], Error> {
        Ok(self.finish_verified()?.sha256())
    }
    /// Proof of the complete streamed package, including every component.
    /// Metadata alone, a signed header or an incomplete stream cannot create it.
    pub fn finish_verified(self) -> Result<VerifiedManifest, Error> {
        if self.failed {
            return Err(Error::Failed);
        }
        if self.position != self.total {
            return Err(Error::Incomplete);
        }
        let digest = self.content.finish()?;
        Ok(VerifiedManifest {
            metadata: self.metadata,
            metadata_len: self.metadata_len,
            total: self.total,
            publisher: self.publisher,
            digest,
        })
    }
}

/// Authenticated metadata retained without keeping the complete package in RAM.
/// This proves package verification, not approval or continued storage ownership.
pub struct VerifiedManifest {
    metadata: [u8; MAX_METADATA_BYTES],
    metadata_len: usize,
    total: u64,
    publisher: [u8; 32],
    digest: [u8; 32],
}
impl VerifiedManifest {
    pub fn manifest(&self) -> Manifest<'_> {
        Manifest::parse(&self.metadata[..self.metadata_len], self.total).expect("verified manifest")
    }
    pub fn publisher(&self) -> [u8; 32] {
        self.publisher
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.digest
    }
    pub fn length(&self) -> u64 {
        self.total
    }
}
