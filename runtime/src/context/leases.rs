//! Bounded capture authority and immutable backing ownership. The platform
//! authenticates caller and liveness; neither IDs nor source bytes grant access.
use super::*;
pub const LIMIT: usize = 8;
pub const OWNER_LIMIT: usize = 2;
pub const TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub struct Lease<T> {
    pub info: Info,
    pub backing: T,
    pub charged: u64,
}
pub struct Pool<T> {
    slots: [Option<Lease<T>>; LIMIT],
    serial: u64,
}
impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Pool<T> {
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; LIMIT],
            serial: 0,
        }
    }
    pub fn info_at(&self, index: usize) -> Option<Info> {
        self.slots.get(index)?.as_ref().map(|l| l.info)
    }
    pub fn insert(
        &mut self,
        caller: u64,
        mut info: Info,
        charged: u64,
        backing: T,
    ) -> Result<Info, (Refusal, T)> {
        if caller != info.handle.target.owner {
            return Err((Refusal::Permission, backing));
        }
        // Set the token before validating the complete descriptor; never wrap.
        let Some(serial) = self.serial.checked_add(1) else {
            return Err((Refusal::Exhausted, backing));
        };
        info.token = serial;
        if let Err(e) = info.validate() {
            return Err((e, backing));
        }
        let count = self
            .slots
            .iter()
            .flatten()
            .filter(|l| l.info.handle.target.owner == caller)
            .count();
        let recipient = self
            .slots
            .iter()
            .flatten()
            .filter(|l| l.info.recipient == info.recipient)
            .count();
        let total = self
            .slots
            .iter()
            .flatten()
            .fold(0u64, |sum, l| sum.saturating_add(l.charged));
        if count >= OWNER_LIMIT
            || recipient >= OWNER_LIMIT
            || charged == 0
            || charged > osaiku_ui::surface::MAX_BYTES
            || total.checked_add(charged).is_none_or(|n| n > TOTAL_BYTES)
        {
            return Err((Refusal::Quota, backing));
        }
        let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) else {
            return Err((Refusal::Quota, backing));
        };
        self.serial = serial;
        *slot = Some(Lease {
            info,
            backing,
            charged,
        });
        Ok(info)
    }
    pub fn get(&self, caller: u64, token: u64) -> Result<&Lease<T>, Refusal> {
        let lease = self
            .slots
            .iter()
            .flatten()
            .find(|l| l.info.token == token)
            .ok_or(Refusal::Stale)?;
        if caller != lease.info.recipient && caller != lease.info.handle.target.owner {
            return Err(Refusal::Permission);
        }
        Ok(lease)
    }
    pub fn take(&mut self, caller: u64, token: u64) -> Result<Lease<T>, Refusal> {
        self.get(caller, token)?;
        Ok(self
            .slots
            .iter_mut()
            .find(|s| s.as_ref().is_some_and(|l| l.info.token == token))
            .unwrap()
            .take()
            .unwrap())
    }
    /// Retire one pin at a time; drop it only outside platform locks/borrows.
    pub fn revoke_one(&mut self, mut predicate: impl FnMut(&Info) -> bool) -> Option<Lease<T>> {
        self.slots
            .iter_mut()
            .find(|s| s.as_ref().is_some_and(|l| predicate(&l.info)))?
            .take()
    }
}
