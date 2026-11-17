//! Pointer-free privileged composition stream. Kernel admission restricts this
//! stream to the current compositord endpoint; the backend separately checks
//! display ownership. Pixel uploads are immutable CPU input, never readback.
use crate::{
    buffer::{kind, Handle},
    render::{Error, Scope, DRAW_BYTES, HEADER_BYTES, MAX_BYTES, MAX_DRAWS, NO_ASSET},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub op: u32,
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub rect: [u32; 4],
    pub clip: [u32; 4],
    pub window: [u32; 4],
    pub args: [u64; 4],
    pub asset: Handle,
    pub offset: u64,
    pub length: u64,
}
impl Command {
    pub const EMPTY: Self = Self {
        op: 0,
        slot: 0,
        width: 0,
        height: 0,
        rect: [0; 4],
        clip: [0; 4],
        window: [0; 4],
        args: [0; 4],
        asset: NO_ASSET,
        offset: 0,
        length: 0,
    };
    pub fn deferred(self) -> bool {
        matches!(self.op, 6 | 7 | 8 | 9 | 12 | 13)
    }
    pub fn validate(self, viewport: (u32, u32)) -> Result<(), Error> {
        if self.width > 4160 || self.height > 4160 || self.slot >= 12 {
            return Err(Error::Geometry);
        }
        for r in [self.rect, self.clip, self.window] {
            if r.iter().any(|n| *n > 16384)
                || r[0].checked_add(r[2]).is_none()
                || r[1].checked_add(r[3]).is_none()
            {
                return Err(Error::Geometry);
            }
        }
        if self.op == 21 || self.op == 23 {
            if self.asset.owner as u32 == 0
                || self.asset.owner >> 32 == 0
                || self.asset.device == 0
                || self.asset.generation == 0
                || self.asset.serial == 0
                || self.asset.kind != kind::BUFFER
                || self.asset.reserved != 0
                || self.offset.checked_add(self.length).is_none()
                || self.rect[2] == 0
                || self.rect[3] == 0
                || (self.op == 21
                    && (self.length != u64::from(self.rect[2]) * u64::from(self.rect[3]) * 4
                        || self.length > 1024 * 1024))
                || (self.op == 23
                    && (self.offset != 0
                        || self.length != u64::from(self.width) * u64::from(self.height) * 4
                        || self.length > 64 * 1024 * 1024))
            {
                return Err(Error::Asset);
            }
        } else if self.asset != NO_ASSET || self.offset != 0 || self.length != 0 {
            return Err(Error::Asset);
        }
        let rectangle = |r: [u32; 4]| {
            r[2] > 0
                && r[3] > 0
                && u64::from(r[0]) + u64::from(r[2]) <= u64::from(viewport.0)
                && u64::from(r[1]) + u64::from(r[3]) <= u64::from(viewport.1)
        };
        let zero = self.rect == [0; 4] && self.clip == [0; 4] && self.window == [0; 4];
        let valid = match self.op {
            1 => {
                self.slot == 0
                    && (self.width, self.height) == viewport
                    && zero
                    && self.args == [0; 4]
            }
            6 => rectangle(self.rect) && self.args[0] <= 1 && self.args[1..] == [0; 3],
            7 => {
                self.slot < 8
                    && rectangle(self.rect)
                    && rectangle(self.clip)
                    && self.args[0] <= 1
                    && self.args[2] <= 64
                    && self.args[3] == 0
            }
            8 | 15 | 17 | 18 => zero && self.args == [0; 4] && self.slot == 0,
            10 => zero && self.slot == 0 && self.args[0] <= 1 && self.args[1..] == [0; 3],
            9 => rectangle(self.rect) && self.args == [0; 4],
            11 => self.slot < 8 && zero && self.args == [0; 4],
            12 | 13 => rectangle(self.rect) && self.args[0] <= 64 && self.args[1..] == [0; 3],
            14 => {
                self.slot < 8
                    && self.width > 0
                    && self.height > 0
                    && self.args[0] >> 63 == 1
                    && self.args[1] >> 63 == 1
                    && self.args[0] != self.args[1]
                    && self.args[2] > 0
                    && self.args[2] <= 65536
                    && self.args[3] == 0x34325241
            }
            16 => zero && self.slot == 0 && self.args[0] < 6 && self.args[1..] == [0; 3],
            20 | 22 => {
                self.width > 0
                    && self.height > 0
                    && zero
                    && self.args[1] <= 1
                    && self.args[2..] == [0; 2]
                    && self.texture_kind()
            }
            23 => {
                self.width > 0
                    && self.height > 0
                    && self.slot < 8
                    && self.clip == [0; 4]
                    && self.window == [0; 4]
                    && self.args[0] == 2
                    && self.args[1] <= 1
                    && self.args[2..] == [0; 2]
                    && u64::from(self.rect[0]) + u64::from(self.rect[2]) <= u64::from(self.width)
                    && u64::from(self.rect[1]) + u64::from(self.rect[3]) <= u64::from(self.height)
                    && (self.args[1] == 0 || self.rect == [0, 0, self.width, self.height])
            }
            21 => {
                self.width > 0
                    && self.height > 0
                    && self.clip == [0; 4]
                    && self.window == [0; 4]
                    && self.args[1..] == [0; 3]
                    && self.texture_kind()
                    && u64::from(self.rect[0]) + u64::from(self.rect[2]) <= u64::from(self.width)
                    && u64::from(self.rect[1]) + u64::from(self.rect[3]) <= u64::from(self.height)
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Operation)
        }
    }
    fn texture_kind(self) -> bool {
        match self.args[0] {
            2 => self.slot < 12,
            3 => self.slot < 9,
            4 => self.slot == 0 && self.width == 256 && self.height == 65,
            _ => false,
        }
    }
    fn encode(self, b: &mut [u8]) {
        for (i, n) in [self.op, self.slot, self.width, self.height]
            .into_iter()
            .chain(self.rect)
            .chain(self.clip)
            .chain(self.window)
            .enumerate()
        {
            b[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        for (i, n) in self
            .args
            .into_iter()
            .chain([
                self.asset.owner,
                self.asset.device,
                self.asset.generation,
                self.asset.serial,
            ])
            .enumerate()
        {
            b[64 + i * 8..72 + i * 8].copy_from_slice(&n.to_le_bytes());
        }
        b[128..132].copy_from_slice(&self.asset.kind.to_le_bytes());
        b[132..136].copy_from_slice(&self.asset.reserved.to_le_bytes());
        b[136..144].copy_from_slice(&self.offset.to_le_bytes());
        b[144..152].copy_from_slice(&self.length.to_le_bytes());
    }
    fn decode(b: &[u8]) -> Self {
        let a = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let n = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        Self {
            op: a(0),
            slot: a(4),
            width: a(8),
            height: a(12),
            rect: core::array::from_fn(|i| a(16 + i * 4)),
            clip: core::array::from_fn(|i| a(32 + i * 4)),
            window: core::array::from_fn(|i| a(48 + i * 4)),
            args: core::array::from_fn(|i| n(64 + i * 8)),
            asset: Handle {
                owner: n(96),
                device: n(104),
                generation: n(112),
                serial: n(120),
                kind: a(128),
                reserved: a(132),
            },
            offset: n(136),
            length: n(144),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Batch<'a> {
    bytes: &'a [u8],
    scope: Scope,
    viewport: (u32, u32),
    count: usize,
}
impl<'a> Batch<'a> {
    pub fn decode(bytes: &'a [u8], scope: Scope) -> Result<Self, Error> {
        if bytes.len() < HEADER_BYTES || bytes.len() > MAX_BYTES {
            return Err(Error::Length);
        }
        if &bytes[..8] != b"OSAGCB01" {
            return Err(Error::Version);
        }
        let n = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        let a = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        if scope.session == 0
            || scope.generation == 0
            || scope.sequence == 0
            || [n(8), n(16), n(24)] != [scope.session, scope.generation, scope.sequence]
        {
            return Err(Error::Scope);
        }
        if bytes[48..64] != [0; 16] {
            return Err(Error::Reserved);
        }
        let count = a(32) as usize;
        let viewport = (a(40), a(44));
        if count == 0
            || count > MAX_DRAWS
            || bytes.len() != a(36) as usize
            || bytes.len() != HEADER_BYTES + count * DRAW_BYTES
        {
            return Err(Error::Length);
        }
        if viewport.0 == 0 || viewport.1 == 0 || viewport.0 > 4096 || viewport.1 > 4096 {
            return Err(Error::Geometry);
        }
        let b = Self {
            bytes,
            scope,
            viewport,
            count,
        };
        for i in 0..count {
            let c = b.command(i).unwrap();
            c.validate(viewport)?;
            if count != 1 && !c.deferred() {
                return Err(Error::Operation);
            }
        }
        Ok(b)
    }
    pub fn scope(self) -> Scope {
        self.scope
    }
    pub fn viewport(self) -> (u32, u32) {
        self.viewport
    }
    pub fn len(self) -> usize {
        self.count
    }
    pub fn command(self, n: usize) -> Option<Command> {
        if n >= self.count {
            return None;
        }
        let at = HEADER_BYTES + n * DRAW_BYTES;
        Some(Command::decode(&self.bytes[at..at + DRAW_BYTES]))
    }
}
pub fn encode(
    scope: Scope,
    viewport: (u32, u32),
    commands: &[Command],
    out: &mut [u8],
) -> Result<usize, Error> {
    if commands.is_empty() || commands.len() > MAX_DRAWS {
        return Err(Error::Length);
    }
    let len = HEADER_BYTES + commands.len() * DRAW_BYTES;
    if out.len() < len {
        return Err(Error::Length);
    }
    if scope.session == 0 || scope.generation == 0 || scope.sequence == 0 {
        return Err(Error::Scope);
    }
    if viewport.0 == 0 || viewport.1 == 0 || viewport.0 > 4096 || viewport.1 > 4096 {
        return Err(Error::Geometry);
    }
    for c in commands {
        c.validate(viewport)?;
        if commands.len() != 1 && !c.deferred() {
            return Err(Error::Operation);
        }
    }
    out[..len].fill(0);
    out[..8].copy_from_slice(b"OSAGCB01");
    for (i, n) in [scope.session, scope.generation, scope.sequence]
        .into_iter()
        .enumerate()
    {
        out[8 + i * 8..16 + i * 8].copy_from_slice(&n.to_le_bytes());
    }
    for (i, n) in [commands.len() as u32, len as u32, viewport.0, viewport.1]
        .into_iter()
        .enumerate()
    {
        out[32 + i * 4..36 + i * 4].copy_from_slice(&n.to_le_bytes());
    }
    for (i, c) in commands.iter().enumerate() {
        c.encode(&mut out[HEADER_BYTES + i * DRAW_BYTES..HEADER_BYTES + (i + 1) * DRAW_BYTES]);
    }
    Ok(len)
}
