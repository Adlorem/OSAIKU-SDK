//! Kernel-brokered CPU-only L2 channel. Each participant owns one write object;
//! its peer maps that object read-only. DMA never reaches either object's PTEs.
use crate::{
    network::{Geometry, RingKind},
    network_ring::Ring,
    protocol::Error,
};
pub const SYSCALL: u64 = 407;
pub const BYTES: usize = 128;
pub const PAGES: u64 = 70;
pub const DATA_OFFSET: u64 = 6 * 4096;
pub mod op {
    pub const OPEN: u64 = 1;
    pub const NEXT: u64 = 2;
    pub const ATTACH: u64 = 3;
    pub const LINK: u64 = 4;
    pub const STATUS: u64 = 5;
    pub const CLOSE: u64 = 6;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Wire {
    pub operation: u64,
    pub id: u64,
    pub generation: u64,
    pub cursor: u64,
    pub own: u64,
    pub peer: u64,
    pub event: u64,
    pub mtu: u32,
    pub mac: [u8; 6],
    pub up: bool,
    /// 0 Ethernet, 1 Wi-Fi; assigned from the native DirectDevice profile.
    pub medium: u8,
}
impl Wire {
    pub const fn new(operation: u64) -> Self {
        Self {
            operation,
            id: 0,
            generation: 0,
            cursor: 0,
            own: 0,
            peer: 0,
            event: 0,
            mtu: 1500,
            mac: [0; 6],
            up: false,
            medium: 0,
        }
    }
    pub fn encode(self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        b[..8].copy_from_slice(b"OL2CHAN1");
        for (i, n) in [
            self.operation,
            self.id,
            self.generation,
            self.cursor,
            self.own,
            self.peer,
            self.event,
        ]
        .into_iter()
        .enumerate()
        {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[64..68].copy_from_slice(&self.mtu.to_le_bytes());
        b[68..74].copy_from_slice(&self.mac);
        b[74] = self.up as u8;
        b[75] = self.medium;
        b
    }
    pub fn decode(b: &[u8; BYTES]) -> Result<Self, Error> {
        if &b[..8] != b"OL2CHAN1" {
            return Err(Error::Version);
        }
        if b[74] > 1 || b[75]>1 || b[76..].iter().any(|n| *n != 0) {
            return Err(Error::Reserved);
        }
        let n = |i: usize| u64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap());
        if !(op::OPEN..=op::CLOSE).contains(&n(0)) {
            return Err(Error::Kind);
        }
        Ok(Self {
            operation: n(0),
            id: n(1),
            generation: n(2),
            cursor: n(3),
            own: n(4),
            peer: n(5),
            event: n(6),
            mtu: u32::from_le_bytes(b[64..68].try_into().unwrap()),
            mac: b[68..74].try_into().unwrap(),
            up: b[74] != 0,
            medium: b[75],
        })
    }
    pub fn geometry(self) -> Result<Geometry, Error> {
        let geometry = Geometry {
            generation: self.generation.try_into().map_err(|_| Error::Range)?,
            capacity: 64,
            slots: 64,
            slot_bytes: 4096,
            mtu: self.mtu,
        };
        geometry.validate()?;
        Ok(geometry)
    }
}
/// # Safety
/// Receipt comes directly from this process's successful ATTACH syscall and is
/// private. The mappings must remain live and each ring may be constructed once.
pub unsafe fn rings(receipt: Wire, driver: bool) -> Result<[Ring; 4], Error> {
    let g = receipt.geometry()?;
    if receipt.own == 0
        || receipt.peer == 0
        || (receipt.own | receipt.peer) & 4095 != 0
        || receipt.own == receipt.peer
    {
        return Err(Error::Range);
    }
    let (d, c) = if driver {
        (receipt.own, receipt.peer)
    } else {
        (receipt.peer, receipt.own)
    };
    let make = |kind| {
        let (entries, producer, consumer, producing) = match kind {
            RingKind::RxDone => (d, d + 2 * 4096, c + 2 * 4096, driver),
            RingKind::RxReturn => (c, c + 4 * 4096, d + 4 * 4096, !driver),
            RingKind::TxSubmit => (c + 4096, c + 5 * 4096, d + 5 * 4096, !driver),
            RingKind::TxDone => (d + 4096, d + 3 * 4096, c + 3 * 4096, driver),
        };
        let (own, peer) = if producing {
            (producer, consumer)
        } else {
            (consumer, producer)
        };
        unsafe {
            Ring::mapped(
                entries as *mut u8,
                own as *mut u32,
                peer as *const u32,
                (own + 64) as *mut u32,
                (peer + 64) as *const u32,
                g.capacity,
                producing,
            )
        }
    };
    Ok([
        make(RingKind::RxDone)?,
        make(RingKind::RxReturn)?,
        make(RingKind::TxSubmit)?,
        make(RingKind::TxDone)?,
    ])
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(w: Wire) -> Result<Wire, isize> {
    crate::native::network_channel(w)
}
