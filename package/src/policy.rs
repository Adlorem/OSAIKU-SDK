//! Host-owned policy persistence codec. Unauthenticated storage must not supply
//! this record. A2 provides durable anti-rollback and A5 binds changes to consent.
use crate::*;
pub const PREFIX_BYTES: usize = 32;
pub const KEY_BYTES: usize = 64;
pub const MAX_BYTES: usize = PREFIX_BYTES + MAX_KEYS * KEY_BYTES;
pub struct PolicyDocument {
    epoch: u64,
    count: usize,
    keys: [TrustedKey; MAX_KEYS],
}
impl PolicyDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < PREFIX_BYTES || bytes.len() > MAX_BYTES {
            return Err(Error::Length);
        }
        if &bytes[..8] != b"OSAPOL01" || u32_at(bytes, 8) != 1 {
            return Err(Error::Format);
        }
        let count = u32_at(bytes, 12) as usize;
        let epoch = u64_at(bytes, 16);
        if count > MAX_KEYS || epoch == 0 || bytes.len() != PREFIX_BYTES + count * KEY_BYTES {
            return Err(Error::Policy);
        }
        zero(&bytes[24..32])?;
        let empty = TrustedKey {
            public_key: [0; 32],
            domain: Domain::Application,
            minimum_epoch: 1,
            revoked: true,
        };
        let mut result = Self {
            epoch,
            count,
            keys: [empty; MAX_KEYS],
        };
        for (i, r) in bytes[PREFIX_BYTES..].chunks_exact(KEY_BYTES).enumerate() {
            let domain = match r[32] {
                1 => Domain::Application,
                2 => Domain::Repository,
                3 => Domain::System,
                4 => Domain::Driver,
                5 => Domain::DriverInput,
                6 => Domain::DriverStorage,
                7 => Domain::DriverController,
                _ => return Err(Error::Policy),
            };
            if r[33] > 1 {
                return Err(Error::Policy);
            }
            zero(&r[34..40])?;
            zero(&r[48..64])?;
            let key = TrustedKey {
                public_key: r[..32].try_into().unwrap(),
                domain,
                revoked: r[33] == 1,
                minimum_epoch: u64_at(r, 40),
            };
            let public = VerifyingKey::from_bytes(&key.public_key).map_err(|_| Error::Policy)?;
            if public.is_weak()
                || key.minimum_epoch == 0
                || key.minimum_epoch > epoch
                || result.keys[..i]
                    .iter()
                    .any(|old| old.domain == key.domain && old.public_key == key.public_key)
            {
                return Err(Error::Policy);
            }
            result.keys[i] = key;
        }
        Ok(result)
    }
    pub fn policy(&self) -> TrustPolicy<'_> {
        TrustPolicy {
            epoch: self.epoch,
            keys: &self.keys[..self.count],
        }
    }
}
