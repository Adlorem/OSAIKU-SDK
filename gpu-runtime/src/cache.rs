//! Bounded per-process shader blob cache. Keys include the complete runtime
//! build, configuration and stable device identity. Every hit verifies length,
//! identity and digest; malformed/missing data is always a miss, never executable
//! input to a driver. The caller retains the charged arena for the cache lifetime.
use sha2::{Digest, Sha256};

pub const SLOTS: usize = 32;
pub const HEADER: usize = 88;
pub const MAX_KEY: usize = 64;
pub const MAX_BLOB: usize = 256 * 1024;
const MAGIC: &[u8; 8] = b"OSAMSC01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Digest of Mesa sources, local patches and statically linked runtime.
    pub build: [u8; 32],
    /// Stable device family/revision/ABI identity; not a process-local handle.
    pub device: [u8; 32],
    /// Compiler flags, renderer ABI and supported extension policy.
    pub configuration: [u8; 32],
}
impl Identity {
    pub fn valid(self) -> bool {
        self.build != [0; 32] && self.device != [0; 32] && self.configuration != [0; 32]
    }
    pub fn key(self, mesa_key: &[u8]) -> Option<[u8; 32]> {
        if !self.valid() || mesa_key.is_empty() || mesa_key.len() > MAX_KEY {
            return None;
        }
        let mut h = Sha256::new();
        h.update(b"osaiku.mesa.blob-key.v1");
        h.update(self.build);
        h.update(self.device);
        h.update(self.configuration);
        h.update((mesa_key.len() as u64).to_le_bytes());
        h.update(mesa_key);
        Some(h.finalize().into())
    }
}

/// Exact bytes are suitable for an atomic private-file write by a future store.
/// No global paths or environment-based cache directory are opened by Mesa.
pub fn encode(key: [u8; 32], blob: &[u8], output: &mut [u8]) -> Option<usize> {
    let n = HEADER.checked_add(blob.len())?;
    if blob.is_empty() || blob.len() > MAX_BLOB || n > output.len() {
        return None;
    }
    output[..8].copy_from_slice(MAGIC);
    output[8..16].copy_from_slice(&(blob.len() as u64).to_le_bytes());
    output[16..48].copy_from_slice(&key);
    output[48..80].copy_from_slice(&Sha256::digest(blob));
    output[80..88].fill(0);
    output[HEADER..n].copy_from_slice(blob);
    Some(n)
}
pub fn decode(key: [u8; 32], record: &[u8]) -> Option<&[u8]> {
    if record.len() <= HEADER
        || record.get(..8)? != MAGIC
        || record.get(16..48)? != key
        || record.get(80..88)? != [0; 8]
    {
        return None;
    }
    let n = u64::from_le_bytes(record.get(8..16)?.try_into().ok()?);
    if n == 0 || n > MAX_BLOB as u64 || n.checked_add(HEADER as u64)? != record.len() as u64 {
        return None;
    }
    let blob = &record[HEADER..];
    if Sha256::digest(blob).as_slice() != &record[48..80] {
        return None;
    }
    Some(blob)
}

#[derive(Clone, Copy)]
struct Entry {
    key: [u8; 32],
    bytes: usize,
    used: u64,
}
impl Entry {
    const EMPTY: Self = Self {
        key: [0; 32],
        bytes: 0,
        used: 0,
    };
}

pub struct Cache<'a> {
    identity: Identity,
    arena: &'a mut [u8],
    entries: [Entry; SLOTS],
    sequence: u64,
}
impl<'a> Cache<'a> {
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn new(identity: Identity, arena: &'a mut [u8]) -> Option<Self> {
        if !identity.valid()
            || arena.len() / SLOTS <= HEADER
            || arena.len() > SLOTS * (MAX_BLOB + HEADER)
        {
            return None;
        }
        arena.fill(0);
        Some(Self {
            identity,
            arena,
            entries: [Entry::EMPTY; SLOTS],
            sequence: 0,
        })
    }
    fn tick(&mut self) -> u64 {
        if self.sequence == u64::MAX {
            self.entries.fill(Entry::EMPTY);
            self.sequence = 0;
        }
        self.sequence += 1;
        self.sequence
    }
    /// Nonblocking, no allocation. Oversized entries are not cached; callers
    /// still compile normally. Eviction never invalidates a borrowed hit.
    pub fn put(&mut self, mesa_key: &[u8], blob: &[u8]) -> bool {
        let Some(key) = self.identity.key(mesa_key) else {
            return false;
        };
        let stride = self.arena.len() / SLOTS;
        if blob.is_empty() || blob.len() > MAX_BLOB || blob.len() + HEADER > stride {
            return false;
        }
        let used = self.tick();
        let index = self
            .entries
            .iter()
            .position(|e| e.bytes != 0 && e.key == key)
            .or_else(|| self.entries.iter().position(|e| e.bytes == 0))
            .unwrap_or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.used)
                    .unwrap()
                    .0
            });
        let slot = &mut self.arena[index * stride..(index + 1) * stride];
        slot.fill(0);
        let Some(bytes) = encode(key, blob, slot) else {
            return false;
        };
        self.entries[index] = Entry { key, bytes, used };
        true
    }
    /// Copies a verified result as required by EGL_ANDROID_blob_cache. If the
    /// caller's buffer is too small, returns required size without modifying it.
    pub fn get(&mut self, mesa_key: &[u8], output: &mut [u8]) -> usize {
        let Some(key) = self.identity.key(mesa_key) else {
            return 0;
        };
        let used = self.tick();
        let Some(index) = self
            .entries
            .iter()
            .position(|e| e.bytes != 0 && e.key == key)
        else {
            return 0;
        };
        let stride = self.arena.len() / SLOTS;
        let slot = &self.arena[index * stride..index * stride + self.entries[index].bytes];
        let Some(blob) = decode(key, slot) else {
            self.entries[index] = Entry::EMPTY;
            return 0;
        };
        let size = blob.len();
        if output.len() >= size {
            output[..size].copy_from_slice(blob);
        }
        self.entries[index].used = used;
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: Identity = Identity {
        build: [1; 32],
        device: [2; 32],
        configuration: [3; 32],
    };
    #[test]
    fn keys_cover_build_device_configuration_and_key_length() {
        let key = ID.key(b"shader").unwrap();
        for other in [
            Identity {
                build: [4; 32],
                ..ID
            },
            Identity {
                device: [4; 32],
                ..ID
            },
            Identity {
                configuration: [4; 32],
                ..ID
            },
        ] {
            assert_ne!(other.key(b"shader"), Some(key));
        }
        assert_ne!(ID.key(b"shader\0"), Some(key));
        assert!(ID.key(&[]).is_none());
        assert!(ID.key(&[0; 65]).is_none());
    }
    #[test]
    fn corrupt_and_truncated_records_are_misses() {
        let key = ID.key(b"x").unwrap();
        let mut record = [0; HEADER + 8];
        encode(key, b"compiled", &mut record).unwrap();
        assert_eq!(decode(key, &record), Some(b"compiled".as_slice()));
        for i in 0..record.len() {
            record[i] ^= 1;
            assert_eq!(decode(key, &record), None, "byte {i}");
            record[i] ^= 1;
        }
        for n in 0..record.len() {
            assert_eq!(decode(key, &record[..n]), None);
        }
        assert_eq!(decode([9; 32], &record), None);
    }
    #[test]
    fn bounded_cache_eviction_small_output_and_updates() {
        let mut arena = [0; SLOTS * (HEADER + 8)];
        let mut cache = Cache::new(ID, &mut arena).unwrap();
        for i in 0..SLOTS {
            assert!(cache.put(&[i as u8], b"compiled"));
        }
        let mut output = [0xa5; 8];
        assert_eq!(cache.get(&[0], &mut output[..2]), 8);
        assert_eq!(output, [0xa5; 8]);
        assert!(cache.put(&[32], b"next"));
        assert_eq!(cache.get(&[1], &mut output), 0); // 0 was recently touched
        assert_eq!(cache.get(&[0], &mut output), 8);
        assert_eq!(&output, b"compiled");
        assert!(!cache.put(b"big", &[0; 9]));
        assert!(cache.put(&[0], b"new"));
        assert_eq!(cache.get(&[0], &mut output), 3);
        assert_eq!(&output[..3], b"new");
        cache.sequence = u64::MAX;
        assert_eq!(cache.get(&[0], &mut output), 0);
    }
}
