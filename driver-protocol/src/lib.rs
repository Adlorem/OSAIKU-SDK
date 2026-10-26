#![no_std]
#![forbid(unsafe_code)]

//! Driver Kit v1. Rust types are local values, never the process wire ABI.
//! Identifiers and decoded handles are claims: the trusted resource owner must
//! compare them with its live registry before granting access.
use core::num::NonZeroU64;

pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 0;
pub const HEADER_BYTES: usize = 64;
pub const MAX_PAYLOAD: usize = 256;
pub const MAX_MESSAGE: usize = HEADER_BYTES + MAX_PAYLOAD;
pub const MAX_BUFFER: u64 = 256 * 1024;
pub const MAX_IN_FLIGHT: usize = 32;
pub const CANCEL: u64 = 1;
pub const DEADLINE: u64 = 2;
pub const KNOWN_FEATURES: u64 = CANCEL | DEADLINE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidId,
    Exhausted,
    Version,
    Features,
    Size,
    Reserved,
    Kind,
    Stale,
    Permission,
    Range,
    State,
    Backpressure,
    Deadline,
    Context,
    NotReady,
}

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name(NonZeroU64);
        impl $name {
            pub const fn new(raw: u64) -> Option<Self> {
                match NonZeroU64::new(raw) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}
id!(DeviceId);
id!(ConnectionGeneration);
id!(DriverInstance);
id!(RequestId);
id!(ResourceId);
id!(EndpointId);

/// One issuer per namespace, retained for its entire lifetime. Never wraps.
/// Restoring a persisted namespace must restore the high-water mark too.
pub struct Sequence {
    last: u64,
}
impl Sequence {
    pub const fn after(last: u64) -> Self {
        Self { last }
    }
    pub fn issue(&mut self) -> Result<u64, Error> {
        self.last = self.last.checked_add(1).ok_or(Error::Exhausted)?;
        Ok(self.last)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Scope {
    pub device: DeviceId,
    pub connection: ConnectionGeneration,
    pub instance: DriverInstance,
}
impl Scope {
    pub const fn new(device: u64, connection: u64, instance: u64) -> Option<Self> {
        match (
            DeviceId::new(device),
            ConnectionGeneration::new(connection),
            DriverInstance::new(instance),
        ) {
            (Some(device), Some(connection), Some(instance)) => Some(Self {
                device,
                connection,
                instance,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Offer {
    pub version: Version,
    pub supported: u64,
    pub required: u64,
}
impl Offer {
    pub fn encode(self) -> [u8; 32] {
        let mut out = [0; 32];
        out[..4].copy_from_slice(b"ODH1");
        out[4..6].copy_from_slice(&self.version.major.to_le_bytes());
        out[6..8].copy_from_slice(&self.version.minor.to_le_bytes());
        out[8..16].copy_from_slice(&self.supported.to_le_bytes());
        out[16..24].copy_from_slice(&self.required.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 32 {
            return Err(Error::Size);
        }
        if &bytes[..4] != b"ODH1" {
            return Err(Error::Version);
        }
        if bytes[24..].iter().any(|b| *b != 0) {
            return Err(Error::Reserved);
        }
        Ok(Self {
            version: Version {
                major: u16_at(bytes, 4),
                minor: u16_at(bytes, 6),
            },
            supported: u64_at(bytes, 8),
            required: u64_at(bytes, 16),
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Negotiated {
    pub version: Version,
    pub features: u64,
}
pub fn negotiate(local: Offer, peer: Offer) -> Result<Negotiated, Error> {
    if local.version.major != MAJOR || peer.version.major != MAJOR {
        return Err(Error::Version);
    }
    let features = local.supported & peer.supported & KNOWN_FEATURES;
    if (local.required | peer.required) & !features != 0 {
        return Err(Error::Features);
    }
    Ok(Negotiated {
        version: Version {
            major: MAJOR,
            minor: MINOR.min(local.version.minor).min(peer.version.minor),
        },
        features,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum Kind {
    Hello = 1,
    Submit = 2,
    Complete = 3,
    Cancel = 4,
    Quiesce = 5,
    Stopped = 6,
    Ready = 7,
    Fault = 8,
}
impl Kind {
    fn decode(value: u16) -> Result<Self, Error> {
        Ok(match value {
            1 => Self::Hello,
            2 => Self::Submit,
            3 => Self::Complete,
            4 => Self::Cancel,
            5 => Self::Quiesce,
            6 => Self::Stopped,
            7 => Self::Ready,
            8 => Self::Fault,
            _ => return Err(Error::Kind),
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub kind: Kind,
    pub scope: Scope,
    pub request: RequestId,
    pub features: u64,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn validate_session(session: Negotiated) -> Result<(), Error> {
    if session.version
        != (Version {
            major: MAJOR,
            minor: MINOR,
        })
    {
        return Err(Error::Version);
    }
    if session.features & !KNOWN_FEATURES != 0 {
        return Err(Error::Features);
    }
    Ok(())
}
impl Header {
    fn validate_features(self, session: Negotiated) -> Result<(), Error> {
        if self.features & !session.features != 0
            || (self.kind == Kind::Cancel && self.features & CANCEL == 0)
            || (self.kind == Kind::Submit && self.features & DEADLINE == 0)
        {
            return Err(Error::Features);
        }
        Ok(())
    }
    pub fn encode(
        self,
        session: Negotiated,
        payload: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        validate_session(session)?;
        if payload.len() > MAX_PAYLOAD || output.len() < HEADER_BYTES + payload.len() {
            return Err(Error::Size);
        }
        self.validate_features(session)?;
        let n = HEADER_BYTES + payload.len();
        output[..n].fill(0);
        output[..4].copy_from_slice(b"ODRV");
        output[4..6].copy_from_slice(&MAJOR.to_le_bytes());
        output[6..8].copy_from_slice(&MINOR.to_le_bytes());
        output[8..10].copy_from_slice(&(self.kind as u16).to_le_bytes());
        output[12..16].copy_from_slice(&(n as u32).to_le_bytes());
        for (at, value) in [
            (16, self.scope.device.get()),
            (24, self.scope.connection.get()),
            (32, self.scope.instance.get()),
            (40, self.request.get()),
            (56, self.features),
        ] {
            output[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        output[48..52].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        output[HEADER_BYTES..n].copy_from_slice(payload);
        Ok(n)
    }
    pub fn decode(
        session: Negotiated,
        expected: Scope,
        bytes: &[u8],
    ) -> Result<(Self, &[u8]), Error> {
        validate_session(session)?;
        if !(HEADER_BYTES..=MAX_MESSAGE).contains(&bytes.len()) {
            return Err(Error::Size);
        }
        if &bytes[..4] != b"ODRV" || u16_at(bytes, 4) != MAJOR || u16_at(bytes, 6) != MINOR {
            return Err(Error::Version);
        }
        if u16_at(bytes, 10) != 0 || u32_at(bytes, 52) != 0 {
            return Err(Error::Reserved);
        }
        if u32_at(bytes, 12) as usize != bytes.len()
            || u32_at(bytes, 48) as usize != bytes.len() - HEADER_BYTES
        {
            return Err(Error::Size);
        }
        let scope = Scope::new(u64_at(bytes, 16), u64_at(bytes, 24), u64_at(bytes, 32))
            .ok_or(Error::InvalidId)?;
        if scope != expected {
            return Err(Error::Stale);
        }
        let request = RequestId::new(u64_at(bytes, 40)).ok_or(Error::InvalidId)?;
        let features = u64_at(bytes, 56);
        if features & !session.features != 0 {
            return Err(Error::Features);
        }
        let header = Self {
            kind: Kind::decode(u16_at(bytes, 8))?,
            scope,
            request,
            features,
        };
        header.validate_features(session)?;
        Ok((header, &bytes[HEADER_BYTES..]))
    }
}

pub const DEVICE_READ: u8 = 1;
pub const DEVICE_WRITE: u8 = 2;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceHandle {
    pub scope: Scope,
    pub id: ResourceId,
}
impl ResourceHandle {
    pub fn encode(self) -> [u8; 32] {
        let mut out = [0; 32];
        for (i, value) in [
            self.scope.device.get(),
            self.scope.connection.get(),
            self.scope.instance.get(),
            self.id.get(),
        ]
        .iter()
        .enumerate()
        {
            out[i * 8..i * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 32 {
            return Err(Error::Size);
        }
        Ok(Self {
            scope: Scope::new(u64_at(bytes, 0), u64_at(bytes, 8), u64_at(bytes, 16))
                .ok_or(Error::InvalidId)?,
            id: ResourceId::new(u64_at(bytes, 24)).ok_or(Error::InvalidId)?,
        })
    }
}

/// Bounds in a registered buffer, never a CPU, bus or physical address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferRange {
    pub offset: u64,
    pub bytes: u64,
}
impl BufferRange {
    pub fn validate(self, capacity: u64) -> Result<(), Error> {
        if self.bytes == 0
            || self
                .offset
                .checked_add(self.bytes)
                .is_none_or(|end| end > capacity)
        {
            return Err(Error::Range);
        }
        Ok(())
    }
}

/// Submit payload: a registered buffer claim, offset, length and absolute
/// host-issued monotonic deadline. The resource owner validates capacity and
/// direction separately. No payload contains a device or CPU address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transfer {
    pub buffer: ResourceHandle,
    pub range: BufferRange,
    pub deadline: u64,
}
impl Transfer {
    pub fn encode(self) -> Result<[u8; 56], Error> {
        if self.range.bytes > MAX_BUFFER || self.deadline == 0 {
            return Err(Error::Range);
        }
        self.range.validate(u64::MAX)?;
        let mut out = [0; 56];
        out[..32].copy_from_slice(&self.buffer.encode());
        out[32..40].copy_from_slice(&self.range.offset.to_le_bytes());
        out[40..48].copy_from_slice(&self.range.bytes.to_le_bytes());
        out[48..56].copy_from_slice(&self.deadline.to_le_bytes());
        Ok(out)
    }
    pub fn decode(expected: Scope, bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 56 {
            return Err(Error::Size);
        }
        let value = Self {
            buffer: ResourceHandle::decode(&bytes[..32])?,
            range: BufferRange {
                offset: u64_at(bytes, 32),
                bytes: u64_at(bytes, 40),
            },
            deadline: u64_at(bytes, 48),
        };
        if value.buffer.scope != expected {
            return Err(Error::Stale);
        }
        value.encode()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum Outcome {
    Completed = 0,
    Cancelled = 1,
    TimedOut = 2,
    Failed = 3,
    UncertainWrite = 4,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    pub actual: u64,
    pub outcome: Outcome,
}
impl Completion {
    pub fn encode(self) -> [u8; 16] {
        let mut out = [0; 16];
        out[..8].copy_from_slice(&self.actual.to_le_bytes());
        out[8..10].copy_from_slice(&(self.outcome as u16).to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 16 {
            return Err(Error::Size);
        }
        if bytes[10..].iter().any(|b| *b != 0) {
            return Err(Error::Reserved);
        }
        let outcome = match u16_at(bytes, 8) {
            0 => Outcome::Completed,
            1 => Outcome::Cancelled,
            2 => Outcome::TimedOut,
            3 => Outcome::Failed,
            4 => Outcome::UncertainWrite,
            _ => return Err(Error::Kind),
        };
        Ok(Self {
            actual: u64_at(bytes, 0),
            outcome,
        })
    }
}
