//! A candidate policy for interactive inspection, never runtime authority.
//! The host must verify the complete stream, obtain explicit consent, recheck
//! the exact source/policy snapshot and persist the change before installation.
use super::*;

pub struct Proposal {
    keys: [TrustedKey; MAX_KEYS],
    count: usize,
    epoch: u64,
    changed: bool,
}
impl Proposal {
    pub fn new(
        m: &Manifest<'_>,
        publisher: [u8; 32],
        signing_epoch: u64,
        old: &TrustPolicy<'_>,
    ) -> Result<Self, Error> {
        if old.keys.len() > MAX_KEYS || old.epoch == 0 || signing_epoch == 0 {
            return Err(Error::Policy);
        }
        let key = public_key(m)?;
        if key.is_some_and(|k| digest(&k) != publisher) {
            return Err(Error::Identity);
        }
        let mut result = Self {
            keys: [TrustedKey {
                public_key: [0; 32],
                domain: Domain::Driver,
                minimum_epoch: 1,
                revoked: true,
            }; MAX_KEYS],
            count: old.keys.len(),
            epoch: old.epoch,
            changed: false,
        };
        result.keys[..old.keys.len()].copy_from_slice(old.keys);
        let classes = classes(m)?;
        for (bit, domain) in [
            (0, Domain::Driver),
            (1, Domain::DriverInput),
            (2, Domain::DriverStorage),
            (4, Domain::DriverController),
            (GPU_CLASS, Domain::DriverGpu),
        ] {
            if bit != 0 && classes & bit == 0 {
                continue;
            }
            let mut existing = old
                .keys
                .iter()
                .filter(|k| k.domain == domain && digest(&k.public_key) == publisher);
            if let Some(k) = existing.next() {
                if existing.next().is_some() {
                    return Err(Error::Policy);
                }
                if k.revoked {
                    return Err(Error::RevokedKey);
                }
                if signing_epoch < k.minimum_epoch {
                    return Err(Error::PolicyEpoch);
                }
            } else {
                let public_key = key.ok_or(Error::UnknownKey)?;
                if result.count == MAX_KEYS {
                    return Err(Error::Limit);
                }
                result.keys[result.count] = TrustedKey {
                    public_key,
                    domain,
                    minimum_epoch: signing_epoch,
                    revoked: false,
                };
                result.count += 1;
                result.changed = true;
            }
        }
        if result.changed {
            result.epoch = old
                .epoch
                .checked_add(1)
                .ok_or(Error::PolicyEpoch)?
                .max(signing_epoch);
            if result.epoch == u64::MAX {
                return Err(Error::PolicyEpoch);
            }
        }
        authorize(&result.policy(), publisher, signing_epoch, m)?;
        Ok(result)
    }
    pub fn requires_consent(&self) -> bool {
        self.changed
    }
    pub fn policy(&self) -> TrustPolicy<'_> {
        TrustPolicy {
            epoch: self.epoch,
            keys: &self.keys[..self.count],
        }
    }
    /// Serialize only after the host has bound consent to a verified package.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        let len = 32 + self.count * 64;
        if out.len() < len {
            return Err(Error::Length);
        }
        out[..len].fill(0);
        out[..8].copy_from_slice(b"OSAPOL01");
        out[8..12].copy_from_slice(&1u32.to_le_bytes());
        out[12..16].copy_from_slice(&(self.count as u32).to_le_bytes());
        out[16..24].copy_from_slice(&self.epoch.to_le_bytes());
        for (i, k) in self.keys[..self.count].iter().enumerate() {
            let r = &mut out[32 + i * 64..32 + (i + 1) * 64];
            r[..32].copy_from_slice(&k.public_key);
            r[32] = match k.domain {
                Domain::Application => 1,
                Domain::Repository => 2,
                Domain::System => 3,
                Domain::Driver => 4,
                Domain::DriverInput => 5,
                Domain::DriverStorage => 6,
                Domain::DriverController => 7,
                Domain::DriverRepository => 8,
                Domain::DriverGpu => 9,
            };
            r[33] = u8::from(k.revoked);
            r[40..48].copy_from_slice(&k.minimum_epoch.to_le_bytes());
        }
        crate::policy::PolicyDocument::parse(&out[..len])?;
        Ok(len)
    }
}
