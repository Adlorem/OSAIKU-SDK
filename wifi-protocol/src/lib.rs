#![no_std]
#![forbid(unsafe_code)]
//! Copied Wi-Fi control plane. No MMIO, DMA, raw frames or generic host commands.
//! Secrets have neither Debug nor Copy; all owned message payloads wipe on drop.
use zeroize::Zeroize;
pub mod scan;
pub const SYSCALL: u64 = 409;
pub const BYTES: usize = 640;
pub const PAYLOAD: usize = 512;
pub const MAX_BSS: usize = 64;
pub const MAX_PROFILES: usize = 16;
pub const SCAN_TTL_MS: u64 = 30_000;
/// Policy-only radio grant, valid until explicit revocation or owner/service
/// termination. This sentinel never extends an individual operation deadline.
pub const SESSION_DEADLINE: u64 = u64::MAX;
pub const MAX_OPERATION_MS: u64 = 30_000;
pub mod op {
    pub const AUTHORIZE: u64 = 1;
    pub const REVOKE: u64 = 2;
    pub const STATUS: u64 = 3;
    /// flags=1 starts admitted firmware and returns FIRMWARE_READY; flags=0
    /// disconnects and waits for kernel DMA cutoff before reporting OFFLINE.
    /// The service composes INITIALIZE/DISCONNECT. This is not an implicit
    /// hardware reset or authorization renewal; a new start may be refused.
    pub const RADIO: u64 = 4;
    pub const SCAN: u64 = 5;
    /// Retained observation from the specified scan serial, including seen_ms.
    /// An old row is display data, not permission to associate or transmit.
    pub const SCAN_ROW: u64 = 6;
    pub const CONNECT: u64 = 7;
    pub const DISCONNECT: u64 = 8;
    pub const CANCEL: u64 = 9;
    pub const PROFILE_PUT: u64 = 10;
    pub const PROFILE_GET: u64 = 11;
    pub const PROFILE_FORGET: u64 = 12;
    pub const POLL: u64 = 13;
    /// Initialize the admitted device/firmware only. Does not grant scan, data
    /// transmission or association; the result state is FIRMWARE_READY.
    pub const INITIALIZE: u64 = 14;
    pub const TAKE: u64 = 20;
    pub const COMPLETE: u64 = 21;
    pub const BACKEND_SUBMIT: u64 = 22;
    pub const BACKEND_TAKE: u64 = 23;
    pub const BACKEND_COMPLETE: u64 = 24;
    pub const BACKEND_POLL: u64 = 25;
    pub const BACKEND_EVENT: u64 = 26;
    pub const EVENT_TAKE: u64 = 27;
    pub const BACKEND_VALIDATE: u64 = 28;
    pub const AUTHORITY_STATE: u64 = 29;
    /// Atomically invalidate `index` and replace it with a bounded disconnect.
    /// Completion means backend acknowledgement, never merely queue removal.
    pub const BACKEND_CANCEL: u64 = 30;
    /// Bind a prepared DirectDevice handle (index=id, flags=generation) to
    /// this authorized INITIALIZE/RADIO/CONNECT operation before firmware DMA START.
    pub const BACKEND_BIND: u64 = 31;
    /// Check the retained radio session after the originating operation ends.
    pub const BACKEND_SESSION: u64 = 32;
    /// Read bounded platform constraints through the kernel's ACPI worker.
    /// Same exact handle/authority fields as BACKEND_SESSION; kind=table 0..7,
    /// id=page 0..3 (46 integers/page); BUSY while pending. One immutable snapshot.
    pub const BACKEND_LIMITS: u64 = 33;
    /// Trusted wifid only: kind=STATUS/PROFILE_PUT/PROFILE_GET/PROFILE_FORGET/
    /// CONNECT, correlated with the taken UI request. No key export. Persistent
    /// operations require AUTHORIZE flags=PERSISTENT and request flags=PERSISTENT.
    pub const CREDENTIAL_STORE: u64 = 34;
    /// Public metadata only; usable before radio authorization.
    pub const OBSERVE: u64 = 35;
    /// wifid publishes its current non-secret status for the desktop/panel.
    pub const PUBLISH: u64 = 36;
    /// Trusted wifid boot client only. kind=boot::BEGIN/END/DHCP; no payload.
    /// BEGIN is available once per boot, before any manual radio decision,
    /// and only with the installation credential vault available.
    pub const BOOT_SESSION: u64 = 37;
    /// Kernel-authenticated network panel attaches to the service-owned boot
    /// session without changing its epoch, owner or hardware binding.
    pub const BOOT_ATTACH: u64 = 38;
}
pub mod boot {
    pub const BEGIN: u64 = 0;
    pub const END: u64 = 1;
    pub const DHCP: u64 = 2;
}
pub mod profile {
    /// Installation-wide TPM vault, explicitly granted by policyd. Not a
    /// per-process or per-human account. Restoring never grants radio access.
    pub const PERSISTENT: u64 = 1;
}
pub mod error {
    pub const INVALID: i64 = -5;
    pub const DENIED: i64 = -8;
    pub const LIMIT: i64 = -10;
    pub const UNSUPPORTED: i64 = -11;
    pub const STALE: i64 = -12;
    pub const BUSY: i64 = -13;
    pub const TIMEOUT: i64 = -14;
    pub const OFFLINE: i64 = -15;
    pub const NO_DEVICE: i64 = -16;
    pub const NO_BACKEND: i64 = -17;
    pub const CREDENTIALS: i64 = -18;
    pub const CANCELLED: i64 = -19;
    pub const RFKILL: i64 = -20;
    pub const REGULATORY: i64 = -21;
}
pub mod state {
    pub const NO_DEVICE: u64 = 0;
    pub const NO_BACKEND: u64 = 1;
    pub const OFFLINE: u64 = 2;
    pub const IDLE: u64 = 3;
    pub const SCANNING: u64 = 4;
    pub const CONNECTING: u64 = 5;
    pub const CONNECTED: u64 = 6;
    pub const DISCONNECTING: u64 = 7;
    pub const FAILED: u64 = 8;
    pub const RFKILL: u64 = 9;
    pub const INITIALIZING: u64 = 10;
    pub const FIRMWARE_READY: u64 = 11;
}
pub mod security {
    pub const OPEN: u8 = 0;
    pub const WPA2_PSK_CCMP: u8 = 1;
    pub const WPA3_SAE: u8 = 2;
    pub const ENTERPRISE: u8 = 3;
    pub const UNSUPPORTED: u8 = 255;
}
pub mod capability {
    pub const SCAN: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const WPA2_PSK_CCMP: u64 = 4;
    pub const RADIO: u64 = 8;
    pub const PERSISTENT_PROFILES: u64 = 16;
    pub const INITIALIZE: u64 = 32;
}
/// Serial/epoch/owner fields are authority only after kernel broker binding.
#[derive(Clone)]
pub struct Message {
    pub operation: u64,
    pub id: u64,
    pub owner: u64,
    pub epoch: u64,
    pub device: u64,
    pub generation: u64,
    pub deadline: u64,
    pub kind: u64,
    pub index: u64,
    pub flags: u64,
    pub length: u64,
    pub result: i64,
    pub data: [u8; PAYLOAD],
}
impl Drop for Message {
    fn drop(&mut self) {
        self.data.zeroize();
    }
}
impl Message {
    pub const fn new(operation: u64) -> Self {
        Self {
            operation,
            id: 0,
            owner: 0,
            epoch: 0,
            device: 0,
            generation: 0,
            deadline: 0,
            kind: 0,
            index: 0,
            flags: 0,
            length: 0,
            result: 0,
            data: [0; PAYLOAD],
        }
    }
    pub fn clear_payload(&mut self) {
        self.data.zeroize();
        self.length = 0;
    }
    pub fn encode(&self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"OSWIFI01");
        for (i, n) in [
            1,
            self.operation,
            self.id,
            self.owner,
            self.epoch,
            self.device,
            self.generation,
            self.deadline,
            self.kind,
            self.index,
            self.flags,
            self.length,
            self.result as u64,
            0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[128..].copy_from_slice(&self.data);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, i64> {
        if b.len() != BYTES || &b[..8] != b"OSWIFI01" {
            return Err(error::INVALID);
        }
        let n = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        if n(1) != 1
            || n(14) != 0
            || n(15) != 0
            || n(12) > PAYLOAD as u64
            || !(1..=14).contains(&n(2)) && !(20..=38).contains(&n(2))
            || b[128 + n(12) as usize..].iter().any(|b| *b != 0)
        {
            return Err(error::INVALID);
        }
        let mut m = Self::new(n(2));
        m.id = n(3);
        m.owner = n(4);
        m.epoch = n(5);
        m.device = n(6);
        m.generation = n(7);
        m.deadline = n(8);
        m.kind = n(9);
        m.index = n(10);
        m.flags = n(11);
        m.length = n(12);
        m.result = n(13) as i64;
        m.data.copy_from_slice(&b[128..]);
        Ok(m)
    }
}
/// SSIDs are opaque octets, not UTF-8. UI must escape, never interpret markup.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Ssid {
    len: u8,
    bytes: [u8; 32],
}
impl Ssid {
    pub const EMPTY: Self = Self {
        len: 0,
        bytes: [0; 32],
    };
    pub fn new(bytes: &[u8]) -> Result<Self, i64> {
        if bytes.len() > 32 {
            return Err(error::INVALID);
        }
        let mut s = Self::EMPTY;
        s.len = bytes.len() as u8;
        s.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(s)
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}
/// A scan record contains no credential. Frame-derived fields remain untrusted.
#[derive(Clone, Copy)]
pub struct Bss {
    pub ssid: Ssid,
    pub bssid: [u8; 6],
    pub frequency_mhz: u16,
    pub signal_dbm: i16,
    pub security: u8,
    pub seen_ms: u64,
    pub generation: u64,
}
impl Bss {
    pub fn encode(&self) -> [u8; 64] {
        let mut b = [0; 64];
        b[0] = self.ssid.len;
        b[1] = self.security;
        b[2..8].copy_from_slice(&self.bssid);
        b[8..10].copy_from_slice(&self.frequency_mhz.to_le_bytes());
        b[10..12].copy_from_slice(&self.signal_dbm.to_le_bytes());
        b[16..24].copy_from_slice(&self.seen_ms.to_le_bytes());
        b[24..32].copy_from_slice(&self.generation.to_le_bytes());
        b[32..64].copy_from_slice(&self.ssid.bytes);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, i64> {
        if b.len() != 64
            || b[0] > 32
            || b[12..16].iter().any(|x| *x != 0)
            || b[32 + b[0] as usize..].iter().any(|x| *x != 0)
        {
            return Err(error::INVALID);
        }
        let mut bssid = [0; 6];
        bssid.copy_from_slice(&b[2..8]);
        if bssid == [0; 6] || bssid[0] & 1 != 0 {
            return Err(error::INVALID);
        }
        Ok(Self {
            ssid: Ssid::new(&b[32..32 + b[0] as usize])?,
            bssid,
            frequency_mhz: u16::from_le_bytes(b[8..10].try_into().unwrap()),
            signal_dbm: i16::from_le_bytes(b[10..12].try_into().unwrap()),
            security: b[1],
            seen_ms: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            generation: u64::from_le_bytes(b[24..32].try_into().unwrap()),
        })
    }
}
