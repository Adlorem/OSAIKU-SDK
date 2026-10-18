//! Streaming noncryptographic pixel fingerprints. These are change hints, never
//! content authentication or authority. A failed publication invalidates the base.
use super::*;
pub(crate) const ROWS: u32 = 32;
pub(crate) type Fingerprint = [u64; 2];
pub(crate) struct DirtyTracker {
    pub base: Option<FrameSnapshot>,
    hashes: [Fingerprint; MAX_EXTENT as usize / 4],
}
impl DirtyTracker {
    pub const fn new() -> Self {
        Self {
            base: None,
            hashes: [[0; 2]; MAX_EXTENT as usize / 4],
        }
    }
    pub fn changed(&self, index: usize, hash: Fingerprint) -> bool {
        self.hashes[index] != hash
    }
    pub fn record(&mut self, index: usize, hash: Fingerprint) {
        self.hashes[index] = hash;
    }
}
/// Two independent word-wise accumulators with avalanche finalization. A 128-bit
/// fingerprint avoids the byte-at-a-time SHA pass; it cannot authenticate pixels.
pub(crate) fn fingerprint(bytes: &[u8]) -> Fingerprint {
    fn mix(mut x: u64) -> u64 {
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        x ^ (x >> 31)
    }
    let mut a = 0x243f6a8885a308d3u64;
    let mut b = 0x13198a2e03707344u64;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_le_bytes(chunk.try_into().unwrap());
        a = (a ^ word).rotate_left(27).wrapping_mul(0x9e3779b185ebca87);
        b = b
            .wrapping_add(word)
            .rotate_left(31)
            .wrapping_mul(0xc2b2ae3d27d4eb4f);
    }
    let mut tail = [0; 8];
    tail[..chunks.remainder().len()].copy_from_slice(chunks.remainder());
    let tail = u64::from_le_bytes(tail);
    [
        mix(a ^ tail ^ bytes.len() as u64),
        mix(b.wrapping_add(tail) ^ bytes.len() as u64),
    ]
}
