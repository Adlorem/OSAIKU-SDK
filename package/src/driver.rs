//! Driver packages v1/v2. The common envelope, streaming verifier and component
//! layout are shared with applications; this domain never grants an app role.
use crate::{application::Manifest, *};
pub const EXTENSION: &str = "osadrv";
pub const RESOURCES: u64 = 0x3f;
pub const GPU_CLASS: u64 = 1 << 6;
pub const CLASSES: u64 = 0x7f;
pub use osaiku_gpu_protocol as gpu_protocol;
pub const MAX_FIRMWARE_BYTES: u64 = 1024 * 1024;
pub const MATCH_BYTES: usize = 32;
pub const DEPENDENCY_BYTES: usize = 208;
pub const MAX_MATCHES: usize = 16;
pub const MAX_DEPENDENCIES: usize = 16;
pub mod installation;

pub(crate) fn gpu_format(b: &[u8]) -> bool {
    matches!(u32_at(b, 8), 2 | 3) && u64_at(b, 448) == GPU_CLASS
}
/// Optional self-contained publisher identity. It conveys no trust.
pub fn public_key(m: &Manifest<'_>) -> Result<Option<[u8; 32]>, Error> {
    classes(m)?;
    Ok((u32_at(m.raw_metadata(), 8) == 3)
        .then(|| m.raw_metadata()[480..512].try_into().unwrap()))
}

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
    let keyed = u32_at(b, 8) == 3;
    if keyed {
        zero(&b[476..480])?;
        let key = VerifyingKey::from_bytes(b[480..512].try_into().unwrap()).map_err(|_| Error::Policy)?;
        if key.is_weak() { return Err(Error::Policy); }
    }
    let gpu_bytes = if u32_at(b, 8) == 2 || gpu_format(b) {
        if u64_at(b, 448) != GPU_CLASS
            || u16_at(b, 466) != 2
            || usize::from(u16_at(b, 474)) != gpu_protocol::PACKAGE_BYTES
        {
            return Err(Error::Manifest);
        }
        if !keyed { zero(&b[476..512])?; }
        gpu_protocol::PACKAGE_BYTES
    } else {
        if u64_at(b, 448) & GPU_CLASS != 0 {
            return Err(Error::Manifest);
        }
        zero(&b[474..if keyed { 480 } else { 512 }])?;
        0
    };
    Ok(matches * MATCH_BYTES + dependencies * DEPENDENCY_BYTES + gpu_bytes)
}
pub(crate) fn validate_records(b: &[u8], start: usize, end: usize) -> Result<(), Error> {
    let gpu_bytes = if gpu_format(b) {
        gpu_protocol::PACKAGE_BYTES
    } else {
        0
    };
    let record_end = end.checked_sub(gpu_bytes).ok_or(Error::Length)?;
    let records = b.get(start..record_end).ok_or(Error::Length)?;
    let split = usize::from(u16_at(b, 470)) * MATCH_BYTES;
    let mut previous: Option<&[u8]> = None;
    for r in records[..split].chunks_exact(MATCH_BYTES) {
        if !matches!(u16_at(r, 0), 1 | 2 | 3) || previous.is_some_and(|p| p >= r) {
            return Err(Error::Manifest);
        }
        if gpu_bytes != 0 {
            // GPU matches require exact PCI vendor/device and display class.
            // Revision is independent of the interface/wildcard field.
            if u16_at(r, 0) != 1
                || u16_at(r, 2) == 0xffff
                || u16_at(r, 4) == 0xffff
                || u16_at(r, 6) != 3
                || r[14] > r[15]
            {
                return Err(Error::Manifest);
            }
            zero(&r[16..])?;
        } else {
            zero(&r[14..])?;
        }
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
    if gpu_bytes != 0 {
        let gpu =
            gpu_protocol::Package::decode(&b[record_end..end]).map_err(|_| Error::Manifest)?;
        if gpu.backend_budget.memory != u64_at(b, 32)
            || gpu.backend_budget.threads != u16_at(b, 48)
            || gpu.backend_budget.instances != u16_at(b, 50)
        {
            return Err(Error::Manifest);
        }
        let compatibility =
            compatibility::Requirements::decode(&b[512..512 + usize::from(u16_at(b, 60))])
                .map_err(|_| Error::Compatibility)?;
        for (id, required) in [
            (compatibility::protocol::GPU_PLATFORM, gpu.platform),
            (compatibility::protocol::GPU_RENDERER, gpu.renderer),
            (compatibility::protocol::GPU_CLIENT, gpu.client),
        ] {
            if !compatibility.protocols().any(|p| {
                p.id == id
                    && !p.optional
                    && p.version.major == required.major
                    && p.version.minor == required.minor
            }) {
                return Err(Error::Compatibility);
            }
        }
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
    if public_key(m)?.is_some_and(|key| digest(&key) != publisher) {
        return Err(Error::Identity);
    }
    policy.key(&publisher, Domain::Driver, epoch)?;
    for (bit, domain) in [
        (1, Domain::DriverInput),
        (2, Domain::DriverStorage),
        (4, Domain::DriverController),
        (GPU_CLASS, Domain::DriverGpu),
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
    matches_revision(m, bus, identity, None)
}
/// A v2 match cannot succeed without a bus-owner supplied revision.
pub fn matches_revision(
    m: &Manifest<'_>,
    bus: u16,
    identity: [u16; 6],
    revision: Option<u8>,
) -> bool {
    if m.domain() != Domain::Driver {
        return false;
    }
    let b = m.raw_metadata();
    let start = (512 + usize::from(u16_at(b, 60)) + 7) & !7;
    b[start..start + usize::from(u16_at(b, 470)) * MATCH_BYTES]
        .chunks_exact(MATCH_BYTES)
        .any(|r| {
            u16_at(r, 0) == bus
                && (!gpu_format(b) || revision.is_some_and(|v| (r[14]..=r[15]).contains(&v)))
                && identity.iter().enumerate().all(|(i, v)| {
                    let expected = u16_at(r, 2 + i * 2);
                    expected == 0xffff || expected == *v
                })
        })
}

pub fn gpu<'a>(m: &Manifest<'a>) -> Result<Option<gpu_protocol::Package<'a>>, Error> {
    classes(m)?;
    let b = m.raw_metadata();
    if !gpu_format(b) {
        return Ok(None);
    }
    let start = ((512 + usize::from(u16_at(b, 60)) + 7) & !7)
        + usize::from(u16_at(b, 470)) * MATCH_BYTES
        + usize::from(u16_at(b, 472)) * DEPENDENCY_BYTES;
    gpu_protocol::Package::decode(&b[start..start + gpu_protocol::PACKAGE_BYTES])
        .map(Some)
        .map_err(|_| Error::Manifest)
}

/// Legacy v1 records for existing catalog adapters. A v2 package returns no
/// records here: dropping its revision bounds would widen its device scope.
/// Selection remains with the OS registry; this grants no hardware authority.
pub fn match_records<'a>(m: &'a Manifest<'a>) -> impl Iterator<Item = (u16, [u16; 6])> + 'a {
    let b = m.raw_metadata();
    let start = (512 + usize::from(u16_at(b, 60)) + 7) & !7;
    b[start..start + usize::from(u16_at(b, 470)) * MATCH_BYTES]
        .chunks_exact(MATCH_BYTES)
        .take(if !gpu_format(b) { MAX_MATCHES } else { 0 })
        .map(|r| (u16_at(r, 0), core::array::from_fn(|i| u16_at(r, 2 + i * 2))))
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
