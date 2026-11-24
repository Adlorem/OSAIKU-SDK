//! Read-only snapshots returned to Disk Info through its live window.
use super::{status, FileReply, VERSION};

pub const CATEGORIES: usize = crate::report::metadata::CATEGORIES;
const SPACE_TAG: &[u8; 8] = b"OSASPC01";
const HEALTH_TAG: &[u8; 8] = b"OSANVM01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub total: u64,
    pub free: u64,
    pub categories: [u64; CATEGORIES],
}
impl Space {
    pub fn write(&self, reply: &mut FileReply) {
        reply.content[..8].copy_from_slice(SPACE_TAG);
        let values = [self.total, self.free].into_iter().chain(self.categories);
        for (field, value) in reply.content[8..].chunks_exact_mut(8).zip(values) {
            field.copy_from_slice(&value.to_le_bytes());
        }
        reply.content_len = (24 + CATEGORIES * 8) as u64;
    }
    pub fn read(reply: &FileReply) -> Option<Self> {
        if !body(reply, SPACE_TAG, 24 + 8 * CATEGORIES) {
            return None;
        }
        let word = |i: usize| u64::from_le_bytes(reply.content[i..i + 8].try_into().unwrap());
        let total = word(8);
        let free = word(16);
        (total != 0 && free <= total).then(|| Self {
            total,
            free,
            categories: core::array::from_fn(|i| word(24 + i * 8)),
        })
    }
}

fn body(r: &FileReply, tag: &[u8; 8], len: usize) -> bool {
    r.version == VERSION
        && r.status == status::OK
        && r.content_len == len as u64
        && &r.content[..8] == tag
}

/// Only the human-readable Identify fields and standardized SMART values cross
/// the file broker. Preserve the full unsigned 128-bit lifetime counters.
#[derive(Clone, Copy)]
pub struct Health {
    pub controller: [u8; 68],
    pub version: u32,
    pub capacity: u64,
    pub smart: Option<Smart>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Smart {
    pub warning: u8,
    pub kelvin: u16,
    pub spare: u8,
    pub threshold: u8,
    pub used: u8,
    pub read_units: u128,
    pub written_units: u128,
    pub cycles: u128,
    pub hours: u128,
    pub unsafe_shutdowns: u128,
    pub media_errors: u128,
    pub error_entries: u128,
}
impl Smart {
    pub fn from_log(log: &[u8; 512]) -> Self {
        let counter = |i: usize| u128::from_le_bytes(log[i..i + 16].try_into().unwrap());
        Self {
            warning: log[0],
            kelvin: u16::from_le_bytes([log[1], log[2]]),
            spare: log[3],
            threshold: log[4],
            used: log[5],
            read_units: counter(32),
            written_units: counter(48),
            cycles: counter(112),
            hours: counter(128),
            unsafe_shutdowns: counter(144),
            media_errors: counter(160),
            error_entries: counter(176),
        }
    }
}
impl Health {
    pub fn write(&self, reply: &mut FileReply) {
        let b = &mut reply.content;
        b[..8].copy_from_slice(HEALTH_TAG);
        b[8..16].copy_from_slice(&self.capacity.to_le_bytes());
        b[16..20].copy_from_slice(&self.version.to_le_bytes());
        b[20..88].copy_from_slice(&self.controller);
        reply.content_len = 88;
        if let Some(s) = self.smart {
            b[88] = s.warning;
            b[89..91].copy_from_slice(&s.kelvin.to_le_bytes());
            b[91..94].copy_from_slice(&[s.spare, s.threshold, s.used]);
            for (out, n) in b[94..206].chunks_exact_mut(16).zip([
                s.read_units,
                s.written_units,
                s.cycles,
                s.hours,
                s.unsafe_shutdowns,
                s.media_errors,
                s.error_entries,
            ]) {
                out.copy_from_slice(&n.to_le_bytes());
            }
            reply.content_len = 206;
        }
    }
    pub fn read(r: &FileReply) -> Option<Self> {
        let b = &r.content;
        let smart = if body(r, HEALTH_TAG, 206) {
            let n = |i: usize| u128::from_le_bytes(b[i..i + 16].try_into().unwrap());
            Some(Smart {
                warning: b[88],
                kelvin: u16::from_le_bytes([b[89], b[90]]),
                spare: b[91],
                threshold: b[92],
                used: b[93],
                read_units: n(94),
                written_units: n(110),
                cycles: n(126),
                hours: n(142),
                unsafe_shutdowns: n(158),
                media_errors: n(174),
                error_entries: n(190),
            })
        } else if body(r, HEALTH_TAG, 88) {
            None
        } else {
            return None;
        };
        Some(Self {
            controller: b[20..88].try_into().unwrap(),
            version: u32::from_le_bytes(b[16..20].try_into().unwrap()),
            capacity: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            smart,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smart_offsets_full_width_counters_and_absence_survive_the_wire() {
        let mut log = [0u8; 512];
        log[..6].copy_from_slice(&[4, 47, 1, 92, 10, 108]);
        for (i, off) in [32, 48, 112, 128, 144, 160, 176].iter().enumerate() {
            log[*off..*off + 16].copy_from_slice(&(u128::MAX - i as u128).to_le_bytes());
        }
        let expected = Smart {
            warning: 4,
            kelvin: 303,
            spare: 92,
            threshold: 10,
            used: 108,
            read_units: u128::MAX,
            written_units: u128::MAX - 1,
            cycles: u128::MAX - 2,
            hours: u128::MAX - 3,
            unsafe_shutdowns: u128::MAX - 4,
            media_errors: u128::MAX - 5,
            error_entries: u128::MAX - 6,
        };
        assert_eq!(Smart::from_log(&log), expected);
        let mut reply = FileReply {
            status: status::OK,
            ..FileReply::EMPTY
        };
        for smart in [Some(expected), None] {
            Health {
                controller: [b'A'; 68],
                capacity: 1 << 40,
                version: 0x10400,
                smart,
            }
            .write(&mut reply);
            let decoded = Health::read(&reply).unwrap();
            assert_eq!(decoded.smart, smart);
            assert_eq!(decoded.capacity, 1 << 40);
            assert_eq!(decoded.controller, [b'A'; 68]);
            assert!(Space::read(&reply).is_none());
            reply.content_len -= 1;
            assert!(Health::read(&reply).is_none());
        }
    }
    #[test]
    fn impossible_volume_counts_and_different_reply_kinds_are_rejected() {
        let mut r = FileReply {
            status: status::OK,
            ..FileReply::EMPTY
        };
        let mut v = Space {
            total: 4096,
            free: 512,
            categories: [9; CATEGORIES],
        };
        v.write(&mut r);
        assert_eq!(Space::read(&r), Some(v));
        assert!(Health::read(&r).is_none());
        v.free += 4096;
        v.write(&mut r);
        assert!(Space::read(&r).is_none());
    }
}
