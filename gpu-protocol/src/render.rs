//! SDK-to-renderer drawing batches. Explicit little-endian records contain no
//! native pointers. Decoding checks shape, not authority: the receiver must
//! bind the session and resolve each asset against its sealed-buffer ledger.
use crate::buffer::{kind, Handle};

pub const HEADER_BYTES: usize = 64;
pub const DRAW_BYTES: usize = 152;
pub const MAX_DRAWS: usize = 32;
pub const MAX_BYTES: usize = HEADER_BYTES + MAX_DRAWS * DRAW_BYTES;
pub const MAX_ASSET_BYTES: u64 = 4 * 1024 * 1024;
pub const NO_ASSET: Handle = Handle {
    owner: 0,
    device: 0,
    generation: 0,
    serial: 0,
    kind: 0,
    reserved: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Length,
    Version,
    Reserved,
    Operation,
    Geometry,
    Asset,
    Scope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub session: u64,
    pub generation: u64,
    pub sequence: u64,
}
impl Scope {
    fn validate(self) -> Result<(), Error> {
        if self.session == 0 || self.generation == 0 || self.sequence == 0 {
            Err(Error::Scope)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Draw {
    pub operation: u32,
    pub color: u32,
    pub rect: [i32; 4],
    pub clip: [i32; 4],
    pub shape: [i32; 4],
    pub params: [u32; 8],
    pub asset: Handle,
    pub offset: u64,
    pub length: u64,
}
impl Draw {
    pub fn control(self) -> bool { matches!(self.operation, 1 | 2 | 7 | 8 | 10) }
    pub fn validate(self, width: u32, height: u32) -> Result<(), Error> {
        viewport(width, height)?;
        if self.control() {
            if self.color != 0 || self.clip != [0; 4] || self.shape != [0; 4]
                || self.params != [0; 8] || self.asset != NO_ASSET || self.offset != 0 || self.length != 0
                || (self.operation == 1 && (width > 4096 || height > 4096 || self.rect != [0, 0, width as i32, height as i32]))
                || (self.operation != 1 && self.rect != [0; 4]) { return Err(Error::Operation); }
            return Ok(());
        }
        if !(3..=6).contains(&self.operation) {
            return Err(Error::Operation);
        }
        if !rectangle(self.rect)
            || self.clip[0] < 0
            || self.clip[1] < 0
            || self.clip[2] <= 0
            || self.clip[3] <= 0
            || i64::from(self.clip[0]) + i64::from(self.clip[2]) > i64::from(width)
            || i64::from(self.clip[1]) + i64::from(self.clip[3]) > i64::from(height)
            || self.params[0] > 255
        {
            return Err(Error::Geometry);
        }
        let data = matches!(self.operation, 4 | 6);
        if data {
            if self.asset.owner as u32 == 0
                || self.asset.owner >> 32 == 0
                || self.asset.device == 0
                || self.asset.generation == 0
                || self.asset.serial == 0
                || self.asset.kind != kind::BUFFER
                || self.asset.reserved != 0
                || self.length == 0
                || self.length > MAX_ASSET_BYTES
                || self.offset.checked_add(self.length).is_none()
            {
                return Err(Error::Asset);
            }
        } else if self.asset != NO_ASSET || self.offset != 0 || self.length != 0 {
            return Err(Error::Asset);
        }
        match self.operation {
            3 if self.params[1..].iter().all(|n| *n == 0) && self.shape == [0; 4] => {}
            4 if self.rect[2] <= 64
                && self.rect[3] <= 64
                && self.length == (self.rect[2] as u64 * self.rect[3] as u64).div_ceil(2)
                && (1..=u16::MAX as u32).contains(&self.params[1])
                && self.params[2] == 1
                && self.params[3..].iter().all(|n| *n == 0)
                && self.shape == [0; 4] => {}
            5 if rectangle(self.shape)
                && self.params[1] <= u16::MAX as u32
                && self.params[2] <= 1
                && self.params[3..7].iter().all(|n| *n <= u16::MAX as u32)
                && self.params[7] == 0
                && (self.params[2] == 1 || self.params[3..7] == [0; 4]) => {}
            6 if self.rect[2] as u32 == self.params[1]
                && self.rect[3] as u32 == self.params[2]
                && self.params[0] == 255
                && self.params[3..].iter().all(|n| *n == 0)
                && self.length == u64::from(self.params[1]) * u64::from(self.params[2]) * 4
                && self.shape == [0; 4] => {}
            3..=6 => return Err(Error::Geometry),
            _ => return Err(Error::Operation),
        }
        Ok(())
    }
    fn decode(b: &[u8]) -> Result<Self, Error> {
        if b[88..96] != [0; 8] {
            return Err(Error::Reserved);
        }
        let rect = |at| core::array::from_fn(|i| u32_at(b, at + i * 4) as i32);
        Ok(Self {
            operation: u32_at(b, 0),
            color: u32_at(b, 4),
            rect: rect(8),
            clip: rect(24),
            shape: rect(40),
            params: core::array::from_fn(|i| u32_at(b, 56 + i * 4)),
            asset: Handle {
                owner: u64_at(b, 96),
                device: u64_at(b, 104),
                generation: u64_at(b, 112),
                serial: u64_at(b, 120),
                kind: u32_at(b, 128),
                reserved: u32_at(b, 132),
            },
            offset: u64_at(b, 136),
            length: u64_at(b, 144),
        })
    }
    fn encode(self, b: &mut [u8]) {
        b.fill(0);
        for (i, n) in [self.operation, self.color]
            .into_iter()
            .chain(self.rect.map(|n| n as u32))
            .chain(self.clip.map(|n| n as u32))
            .chain(self.shape.map(|n| n as u32))
            .chain(self.params)
            .enumerate()
        {
            b[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        for (at, n) in [
            (96, self.asset.owner),
            (104, self.asset.device),
            (112, self.asset.generation),
            (120, self.asset.serial),
            (136, self.offset),
            (144, self.length),
        ] {
            b[at..at + 8].copy_from_slice(&n.to_le_bytes());
        }
        b[128..132].copy_from_slice(&self.asset.kind.to_le_bytes());
        b[132..136].copy_from_slice(&self.asset.reserved.to_le_bytes());
    }
}

/// All records are checked before the first one is exposed to the renderer.
/// The caller must keep these bytes immutable until execution completes.
#[derive(Clone, Copy)]
pub struct Batch<'a> {
    bytes: &'a [u8],
    scope: Scope,
    width: u32,
    height: u32,
    count: usize,
}
impl<'a> Batch<'a> {
    pub fn decode(b: &'a [u8], expected: Scope) -> Result<Self, Error> {
        if b.len() < HEADER_BYTES || b.len() > MAX_BYTES {
            return Err(Error::Length);
        }
        if &b[..8] != b"OSAGRB01" {
            return Err(Error::Version);
        }
        if b[48..64] != [0; 16] {
            return Err(Error::Reserved);
        }
        let scope = Scope {
            session: u64_at(b, 8),
            generation: u64_at(b, 16),
            sequence: u64_at(b, 24),
        };
        scope.validate()?;
        if scope != expected {
            return Err(Error::Scope);
        }
        let count = u32_at(b, 32) as usize;
        if count == 0
            || count > MAX_DRAWS
            || u32_at(b, 36) as usize != b.len()
            || b.len() != HEADER_BYTES + count * DRAW_BYTES
        {
            return Err(Error::Length);
        }
        let width = u32_at(b, 40);
        let height = u32_at(b, 44);
        viewport(width, height)?;
        let result = Self {
            bytes: b,
            scope,
            width,
            height,
            count,
        };
        for i in 0..count {
            if count != 1 && result.draw(i).ok_or(Error::Length)??.control() { return Err(Error::Operation); }
            result
                .draw(i)
                .ok_or(Error::Length)??
                .validate(width, height)?;
        }
        Ok(result)
    }
    pub fn scope(self) -> Scope {
        self.scope
    }
    pub fn control(self) -> Option<u32> {
        self.draw(0)?.ok().filter(|d| self.count == 1 && d.control()).map(|d| d.operation)
    }
    pub fn viewport(self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn len(self) -> usize {
        self.count
    }
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
    pub fn draw(self, index: usize) -> Option<Result<Draw, Error>> {
        if index >= self.count {
            return None;
        }
        let at = HEADER_BYTES + index * DRAW_BYTES;
        Some(Draw::decode(&self.bytes[at..at + DRAW_BYTES]))
    }
}

/// Reject the complete batch before writing output. No partially encoded
/// request is published on a short buffer, malformed draw or zero scope.
pub fn encode(
    scope: Scope,
    width: u32,
    height: u32,
    draws: &[Draw],
    out: &mut [u8],
) -> Result<usize, Error> {
    scope.validate()?;
    viewport(width, height)?;
    if draws.is_empty() || draws.len() > MAX_DRAWS {
        return Err(Error::Length);
    }
    let length = HEADER_BYTES + draws.len() * DRAW_BYTES;
    if out.len() < length {
        return Err(Error::Length);
    }
    for draw in draws {
        if draws.len() != 1 && draw.control() { return Err(Error::Operation); }
        draw.validate(width, height)?;
    }
    out[..length].fill(0);
    out[..8].copy_from_slice(b"OSAGRB01");
    for (at, n) in [
        (8, scope.session),
        (16, scope.generation),
        (24, scope.sequence),
    ] {
        out[at..at + 8].copy_from_slice(&n.to_le_bytes());
    }
    for (at, n) in [
        (32, draws.len() as u32),
        (36, length as u32),
        (40, width),
        (44, height),
    ] {
        out[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    for (i, draw) in draws.iter().enumerate() {
        let at = HEADER_BYTES + i * DRAW_BYTES;
        draw.encode(&mut out[at..at + DRAW_BYTES]);
    }
    Ok(length)
}
fn viewport(width: u32, height: u32) -> Result<(), Error> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        Err(Error::Geometry)
    } else {
        Ok(())
    }
}
fn rectangle(r: [i32; 4]) -> bool {
    (-1_048_576..=1_048_576).contains(&r[0])
        && (-1_048_576..=1_048_576).contains(&r[1])
        && (1..=65535).contains(&r[2])
        && (1..=65535).contains(&r[3])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
