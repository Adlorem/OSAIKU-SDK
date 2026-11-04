//! Driver package v1. The common envelope, streaming verifier and component
//! layout are shared with applications; this domain never grants an app role.
use crate::{application::Manifest, *};
pub const EXTENSION: &str = "osadrv";
pub const RESOURCES: u64 = 0x3f;
pub const CLASSES: u64 = 0x3f;
pub const MAX_FIRMWARE_BYTES: u64 = 1024 * 1024;
pub const MATCH_BYTES: usize = 32;
pub const DEPENDENCY_BYTES: usize = 208;
pub const MAX_MATCHES: usize = 16;
pub const MAX_DEPENDENCIES: usize = 16;

pub fn parse(bytes: &[u8]) -> Result<application::Package<'_>, Error> {
    application::Package::parse_for(bytes, Domain::Driver)
}
pub(crate) fn extension(b: &[u8]) -> Result<usize, Error> {
    let matches = usize::from(u16_at(b, 470));
    let dependencies = usize::from(u16_at(b, 472));
    if matches == 0
        || matches > MAX_MATCHES
        || dependencies > MAX_DEPENDENCIES
        || u64_at(b, 448) == 0
        || u64_at(b, 448) & !CLASSES != 0
        || !(1..=1000).contains(&u32_at(b, 456))
        || u32_at(b, 460) > 100_000
        || !(1..=64).contains(&u16_at(b, 464))
        || !matches!(u16_at(b, 466), 1 | 2)
        || !matches!(u16_at(b, 468), 1..=3)
        || u16_at(b, 54) != 1
    {
        return Err(Error::Manifest);
    }
    zero(&b[474..512])?;
    Ok(matches * MATCH_BYTES + dependencies * DEPENDENCY_BYTES)
}
pub(crate) fn validate_records(b: &[u8], start: usize, end: usize) -> Result<(), Error> {
    let records = b.get(start..end).ok_or(Error::Length)?;
    let split = usize::from(u16_at(b, 470)) * MATCH_BYTES;
    let mut previous: Option<&[u8]> = None;
    for r in records[..split].chunks_exact(MATCH_BYTES) {
        if !matches!(u16_at(r, 0), 1 | 2 | 3) || previous.is_some_and(|p| p >= r) {
            return Err(Error::Manifest);
        }
        zero(&r[14..])?;
        previous = Some(r);
    }
    previous = None;
    for r in records[split..].chunks_exact(DEPENDENCY_BYTES) {
        if !identifier(text(&r[32..160])?)
            || r[..32] == [0; 32]
            || u64_at(r, 160) == 0
            || r[168..200] == [0; 32]
            || previous.is_some_and(|p| p[..160] >= r[..160])
        {
            return Err(Error::Manifest);
        }
        zero(&r[200..])?;
        previous = Some(r);
    }
    Ok(())
}
pub fn classes(m: &Manifest<'_>) -> Result<u64, Error> {
    if m.domain() != Domain::Driver {
        return Err(Error::Format);
    }
    Ok(u64_at(m.raw_metadata(), 448))
}
pub fn authorize(
    policy: &TrustPolicy<'_>,
    publisher: [u8; 32],
    epoch: u64,
    m: &Manifest<'_>,
) -> Result<(), Error> {
    let classes = classes(m)?;
    policy.key(&publisher, Domain::Driver, epoch)?;
    for (bit, domain) in [
        (1, Domain::DriverInput),
        (2, Domain::DriverStorage),
        (4, Domain::DriverController),
    ] {
        if classes & bit != 0 {
            policy.key(&publisher, domain, epoch)?;
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dependency<'a> {
    pub publisher: [u8; 32],
    pub id: &'a str,
    pub release: u64,
    pub digest: [u8; 32],
}
pub fn dependencies<'a>(m: &'a Manifest<'a>) -> impl Iterator<Item = Dependency<'a>> {
    let b = m.raw_metadata();
    let start =
        ((512 + usize::from(u16_at(b, 60)) + 7) & !7) + usize::from(u16_at(b, 470)) * MATCH_BYTES;
    let end = start + usize::from(u16_at(b, 472)) * DEPENDENCY_BYTES;
    b[start..end]
        .chunks_exact(DEPENDENCY_BYTES)
        .map(|r| Dependency {
            publisher: r[..32].try_into().unwrap(),
            id: text(&r[32..160]).unwrap(),
            release: u64_at(r, 160),
            digest: r[168..200].try_into().unwrap(),
        })
}
/// Match inputs come from the bus owner, never from a package's claimed identity.
pub fn matches(m: &Manifest<'_>, bus: u16, identity: [u16; 6]) -> bool {
    if m.domain() != Domain::Driver {
        return false;
    }
    let b = m.raw_metadata();
    let start = (512 + usize::from(u16_at(b, 60)) + 7) & !7;
    b[start..start + usize::from(u16_at(b, 470)) * MATCH_BYTES]
        .chunks_exact(MATCH_BYTES)
        .any(|r| {
            u16_at(r, 0) == bus
                && identity.iter().enumerate().all(|(i, v)| {
                    let expected = u16_at(r, 2 + i * 2);
                    expected == 0xffff || expected == *v
                })
        })
}

/// Bound the selected dependency graph before any activation is published.
pub fn acyclic(graph: &[u64]) -> bool {
    if graph.len() > 32 {
        return false;
    }
    let allowed = (1u64 << graph.len()) - 1;
    if graph.iter().any(|e| e & !allowed != 0) {
        return false;
    }
    let mut closure = [0u64; 32];
    closure[..graph.len()].copy_from_slice(graph);
    for k in 0..graph.len() {
        for i in 0..graph.len() {
            if closure[i] & (1 << k) != 0 {
                closure[i] |= closure[k];
            }
        }
    }
    !(0..graph.len()).any(|i| closure[i] & (1 << i) != 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Requirements {
    pub cpu_permille: u32,
    pub irq_per_second: u32,
    pub queue: u16,
    pub isolation: u16,
    pub reset: u16,
}
pub fn requirements(m: &Manifest<'_>) -> Result<Requirements, Error> {
    classes(m)?;
    let b = m.raw_metadata();
    Ok(Requirements {
        cpu_permille: u32_at(b, 456),
        irq_per_second: u32_at(b, 460),
        queue: u16_at(b, 464),
        isolation: u16_at(b, 466),
        reset: u16_at(b, 468),
    })
}
/// Assigned by the OS loader from authenticated distribution provenance, never
/// read from a driver manifest or inferred from its name/publisher alone.
/// Neither class is a DMA mapping or a waiver of platform admission checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustClass {
    System,
    Installable,
}
impl TrustClass {
    pub const fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Installable => "installable",
        }
    }
    /// The current controller transport is reserved for release-owned drivers.
    /// Installable class drivers use mediated transports; direct assignment
    /// needs its own qualified platform contract before this can be extended.
    pub fn admit(self, manifest: &Manifest<'_>) -> Result<(), Error> {
        let classes = classes(manifest)?;
        if self == Self::Installable && classes & 4 != 0 {
            return Err(Error::Policy);
        }
        Ok(())
    }
}
