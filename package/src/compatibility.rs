//! Canonical compatibility requirements for installable applications.
//!
//! This is a bounded field-wise little-endian codec, not a Rust memory layout.
//! The package signature must cover these bytes. Parsing and matching do not
//! authenticate a publisher, grant capabilities, validate ELF, or authorize a
//! launch. SDK release numbers deliberately do not participate in matching.

pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 32;
pub const PROTOCOL_BYTES: usize = 16;
pub const MAX_PROTOCOLS: usize = 32;
pub const MAX_BYTES: usize = HEADER_BYTES + MAX_PROTOCOLS * PROTOCOL_BYTES;
pub const X86_64: u16 = 62;

/// Registry for the installable-application profile. Defining an ID does not
/// advertise it: hosts offer it only after implementing the versioned service.
pub mod protocol {
    pub const PROCESS_CONTEXT: u32 = 1;
    pub const WINDOW: u32 = 2;
    pub const SELECTED_FILES: u32 = 3;
    pub const DESKTOP_APPEARANCE: u32 = 4;
    pub const SYSTEM_METRICS: u32 = 5;
    pub const PACKAGE_CATALOG: u32 = 6;
    pub const PRIVATE_DATA: u32 = 7;
}

/// Additional features beyond the native x86-64 baseline. The host advertises
/// usable features, including OS state-save support, not raw CPUID bits.
pub mod cpu {
    pub const SSE3: u64 = 1 << 0;
    pub const SSSE3: u64 = 1 << 1;
    pub const SSE41: u64 = 1 << 2;
    pub const SSE42: u64 = 1 << 3;
    pub const AVX: u64 = 1 << 4;
    pub const AVX2: u64 = 1 << 5;
    pub const KNOWN: u64 = SSE3 | SSSE3 | SSE41 | SSE42 | AVX | AVX2;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl Version {
    fn supports(self, required: Self) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Profile {
    pub architecture: u16,
    pub process_abi: Version,
    pub cpu_features: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolRequirement {
    pub id: u32,
    pub version: Version,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolSupport {
    pub id: u32,
    pub version: Version,
}

/// One supported major per protocol in v1, sorted by strictly increasing ID.
/// A newer minor must preserve the behavior and wire format of older minors.
pub struct Platform<'a> {
    pub profile: Profile,
    pub protocols: &'a [ProtocolSupport],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Length,
    Version,
    Reserved,
    Profile,
    ProtocolLimit,
    ProtocolOrder,
    ProtocolVersion,
    ProtocolFlags,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityError {
    InvalidPlatform,
    Architecture,
    ProcessAbi,
    CpuFeatures,
    RequiredProtocol(u32),
}

/// Bit positions correspond to the canonical requirement order. An absent or
/// incompatible optional protocol is explicitly unavailable, not silently used.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Availability(u32);

impl Availability {
    pub fn contains(self, requirement_index: usize) -> bool {
        requirement_index < MAX_PROTOCOLS && self.0 & (1 << requirement_index) != 0
    }
}

/// Borrows exactly the validated compatibility record, without allocation.
/// It conveys structural validity only; it is not a trusted package handle.
#[derive(Debug)]
pub struct Requirements<'a> {
    profile: Profile,
    protocols: &'a [u8],
}

impl<'a> Requirements<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        if bytes.len() < HEADER_BYTES || bytes.len() > MAX_BYTES {
            return Err(DecodeError::Length);
        }
        if read_u16(bytes, 0) != VERSION {
            return Err(DecodeError::Version);
        }
        let count = usize::from(read_u16(bytes, 16));
        if count > MAX_PROTOCOLS {
            return Err(DecodeError::ProtocolLimit);
        }
        if bytes.len() != HEADER_BYTES + count * PROTOCOL_BYTES {
            return Err(DecodeError::Length);
        }
        if bytes[18..HEADER_BYTES].iter().any(|&byte| byte != 0) {
            return Err(DecodeError::Reserved);
        }
        let profile = Profile {
            architecture: read_u16(bytes, 2),
            process_abi: Version {
                major: read_u16(bytes, 4),
                minor: read_u16(bytes, 6),
            },
            cpu_features: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        };
        validate_profile(profile)?;
        let result = Self {
            profile,
            protocols: &bytes[HEADER_BYTES..],
        };
        let mut previous = 0;
        for record in result.protocols.chunks_exact(PROTOCOL_BYTES) {
            if read_u32(record, 8) > 1 {
                return Err(DecodeError::ProtocolFlags);
            }
            if read_u32(record, 12) != 0 {
                return Err(DecodeError::Reserved);
            }
            let requirement = decode_protocol(record);
            validate_protocol(requirement.id, requirement.version, previous)?;
            previous = requirement.id;
        }
        Ok(result)
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn protocols(&self) -> impl ExactSizeIterator<Item = ProtocolRequirement> + '_ {
        self.protocols
            .chunks_exact(PROTOCOL_BYTES)
            .map(decode_protocol)
    }

    pub fn check(&self, platform: &Platform<'_>) -> Result<Availability, CompatibilityError> {
        if validate_profile(platform.profile).is_err()
            || platform.profile.architecture != X86_64
            || platform.profile.cpu_features & !cpu::KNOWN != 0
            || (platform.profile.cpu_features & cpu::AVX2 != 0
                && platform.profile.cpu_features & cpu::AVX == 0)
            || platform.protocols.len() > MAX_PROTOCOLS
        {
            return Err(CompatibilityError::InvalidPlatform);
        }
        let mut previous = 0;
        for protocol in platform.protocols {
            if validate_protocol(protocol.id, protocol.version, previous).is_err() {
                return Err(CompatibilityError::InvalidPlatform);
            }
            previous = protocol.id;
        }
        if self.profile.architecture != platform.profile.architecture {
            return Err(CompatibilityError::Architecture);
        }
        if !platform
            .profile
            .process_abi
            .supports(self.profile.process_abi)
        {
            return Err(CompatibilityError::ProcessAbi);
        }
        if self.profile.cpu_features & !platform.profile.cpu_features != 0 {
            return Err(CompatibilityError::CpuFeatures);
        }
        let mut available = 0;
        for (index, required) in self.protocols().enumerate() {
            let supported = platform
                .protocols
                .iter()
                .any(|p| p.id == required.id && p.version.supports(required.version));
            if supported {
                available |= 1 << index;
            } else if !required.optional {
                return Err(CompatibilityError::RequiredProtocol(required.id));
            }
        }
        Ok(Availability(available))
    }
}

/// Encode only after validating all fields and output capacity. A refusal
/// leaves the caller's output untouched. Bytes after the record are untouched.
pub fn encode(
    profile: Profile,
    protocols: &[ProtocolRequirement],
    output: &mut [u8],
) -> Result<usize, DecodeError> {
    validate_profile(profile)?;
    if protocols.len() > MAX_PROTOCOLS {
        return Err(DecodeError::ProtocolLimit);
    }
    let length = HEADER_BYTES + protocols.len() * PROTOCOL_BYTES;
    if output.len() < length {
        return Err(DecodeError::Length);
    }
    let mut previous = 0;
    for protocol in protocols {
        validate_protocol(protocol.id, protocol.version, previous)?;
        previous = protocol.id;
    }
    let output = &mut output[..length];
    output.fill(0);
    output[0..2].copy_from_slice(&VERSION.to_le_bytes());
    output[2..4].copy_from_slice(&profile.architecture.to_le_bytes());
    output[4..6].copy_from_slice(&profile.process_abi.major.to_le_bytes());
    output[6..8].copy_from_slice(&profile.process_abi.minor.to_le_bytes());
    output[8..16].copy_from_slice(&profile.cpu_features.to_le_bytes());
    output[16..18].copy_from_slice(&(protocols.len() as u16).to_le_bytes());
    for (protocol, record) in protocols
        .iter()
        .zip(output[HEADER_BYTES..].chunks_exact_mut(PROTOCOL_BYTES))
    {
        record[0..4].copy_from_slice(&protocol.id.to_le_bytes());
        record[4..6].copy_from_slice(&protocol.version.major.to_le_bytes());
        record[6..8].copy_from_slice(&protocol.version.minor.to_le_bytes());
        record[8..12].copy_from_slice(&u32::from(!protocol.optional).to_le_bytes());
    }
    Ok(length)
}

fn validate_profile(profile: Profile) -> Result<(), DecodeError> {
    // Preserve unknown nonzero architectures/features for inspection. Matching
    // must refuse them unless the host implements their defined semantics.
    if profile.architecture == 0 || profile.process_abi.major == 0 {
        Err(DecodeError::Profile)
    } else {
        Ok(())
    }
}

fn validate_protocol(id: u32, version: Version, previous: u32) -> Result<(), DecodeError> {
    if id <= previous {
        Err(DecodeError::ProtocolOrder)
    } else if version.major == 0 {
        Err(DecodeError::ProtocolVersion)
    } else {
        Ok(())
    }
}

fn decode_protocol(record: &[u8]) -> ProtocolRequirement {
    ProtocolRequirement {
        id: read_u32(record, 0),
        version: Version {
            major: read_u16(record, 4),
            minor: read_u16(record, 6),
        },
        optional: read_u32(record, 8) == 0,
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
