//! Read-only semantic projections. IDs are boot-local identities, never pointers.
//! Catalog entries describe compiled operations and confer no authority. Text
//! supplied by applications is untrusted data, never a policy or instruction.
pub const VERSION: u32 = 1;
pub const PAGE_SIZE: usize = 4;
pub const MAX_SCAN: usize = 256;
pub const MAX_AGE_MS: u64 = 60_000;
pub const CPU: u32 = 1;
pub const MEMORY: u32 = 2;
pub const PROCESS: u32 = 3;
pub const APPLICATION: u32 = 4;
pub const WINDOW: u32 = 5;
pub const VOLUME: u32 = 6;
pub const SYSTEM_READ: u32 = 1;
pub const WINDOW_READ: u32 = 2;
pub const WINDOW_IMAGE: u32 = 4;
pub const OWNER_CONTROL: u32 = 8;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    None = 0,
    Invalid = 1,
    Permission = 2,
    Unavailable = 3,
    Stale = 4,
    Busy = 5,
    Budget = 6,
    Revoked = 7,
    Timeout = 8,
    Unsupported = 9,
    Integrity = 10,
}
impl Reason {
    pub fn context(error: super::Error) -> Self {
        use osaiku_ui::surface::Refusal;
        match error {
            super::Error::Transport(code) => Self::transport(code),
            super::Error::Buffer => Self::Invalid,
            super::Error::Incomplete => Self::Unavailable,
            super::Error::Invalid(reason) => match reason {
                Refusal::Permission => Self::Permission,
                Refusal::Stale => Self::Stale,
                Refusal::Busy => Self::Busy,
                Refusal::Quota | Refusal::Exhausted => Self::Budget,
                Refusal::Incomplete => Self::Unavailable,
                Refusal::Identity => Self::Integrity,
                Refusal::Shape | Refusal::Bounds => Self::Invalid,
            },
        }
    }
    pub fn transport(code: isize) -> Self {
        use crate::abi::*;
        match code {
            SYSCALL_ERR_PERMISSION_DENIED => Self::Permission,
            SYSCALL_ERR_STALE_ENDPOINT => Self::Stale,
            SYSCALL_ERR_BUSY => Self::Busy,
            SYSCALL_ERR_TIMEOUT => Self::Timeout,
            SYSCALL_ERR_LIMIT_EXCEEDED => Self::Budget,
            SYSCALL_ERR_NOT_FOUND => Self::Unavailable,
            SYSCALL_ERR_NOT_SUPPORTED => Self::Unsupported,
            _ => Self::Invalid,
        }
    }
}

/// The values have a fixed meaning for each kind (see `fields`); unused values
/// must be zero. Producers never place strings, filenames, document bytes or raw
/// addresses in these fields; only the documented metadata projections are valid.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entity {
    pub version: u32,
    pub kind: u32,
    pub id: u64,
    pub generation: u64,
    pub owner: u64,
    pub related_kind: u32,
    pub reason: u32,
    pub related_id: u64,
    pub related_generation: u64,
    pub state: u64,
    pub source_revision: u64,
    pub sampled_ms: u64,
    pub valid_for_ms: u64,
    pub values: [u64; 8],
}
pub mod fields {
    /// CPU: logical index, online. Utilization is explicitly unavailable until
    /// an authorized sampled monitoring source exists (reason=Unavailable).
    pub const CPU_VALUES: usize = 2;
    /// Memory bytes: total, available, quarantined, weights, KV, workspace,
    /// transfer, unbacked admission. Admission is not physical usage.
    pub const MEMORY_VALUES: usize = 8;
    /// Process: fault count, restart count. No exit payload or process name.
    pub const PROCESS_VALUES: usize = 2;
    /// Application: installed app ID. Relation names its process instance.
    pub const APPLICATION_VALUES: usize = 1;
    /// Window: app ID, visible, focused, published semantic revision.
    pub const WINDOW_VALUES: usize = 4;
    /// Volume: writable, filesystem format. State is the native trust enum.
    pub const VOLUME_VALUES: usize = 2;
}
impl Entity {
    pub fn new(kind: u32, id: u64, generation: u64) -> Self {
        Self {
            version: VERSION,
            kind,
            id,
            generation,
            ..Self::default()
        }
    }
    pub fn validate(&self, source: u32) -> Result<(), Reason> {
        let n = match self.kind {
            CPU => fields::CPU_VALUES,
            MEMORY => fields::MEMORY_VALUES,
            PROCESS => fields::PROCESS_VALUES,
            APPLICATION => fields::APPLICATION_VALUES,
            WINDOW => fields::WINDOW_VALUES,
            VOLUME => fields::VOLUME_VALUES,
            _ => return Err(Reason::Invalid),
        };
        if self.version != VERSION
            || self.kind != source
            || self.id == 0
            || self.generation == 0
            || self.reason > Reason::Integrity as u32
            || self.valid_for_ms > MAX_AGE_MS
            || self.values[n..].iter().any(|v| *v != 0)
            || (self.related_kind == 0 && (self.related_id != 0 || self.related_generation != 0))
            || (self.related_kind != 0
                && (!(CPU..=VOLUME).contains(&self.related_kind)
                    || self.related_id == 0
                    || self.related_generation == 0))
        {
            return Err(Reason::Invalid);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub version: u32,
    pub source: u32,
    pub request_id: u64,
    /// Cursor is a source slot, not an offset into private object content.
    pub cursor: u64,
    pub limit: u64,
    /// Required on continuation pages. Restart at zero after Stale.
    pub revision: u64,
    pub max_age_ms: u64,
}
impl Query {
    pub fn first(source: u32, request_id: u64) -> Self {
        Self {
            version: VERSION,
            source,
            request_id,
            limit: PAGE_SIZE as u64,
            max_age_ms: 1000,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<(), Reason> {
        if self.version != VERSION
            || !(CPU..=VOLUME).contains(&self.source)
            || self.request_id == 0
            || self.limit == 0
            || self.limit > PAGE_SIZE as u64
            || self.cursor > MAX_SCAN as u64
            || (self.cursor != 0 && self.revision == 0)
            || self.max_age_ms > MAX_AGE_MS
        {
            return Err(Reason::Invalid);
        }
        Ok(())
    }
}

/// Correlation is evidence of this read decision, not execution authority or a
/// cryptographic audit receipt. The producer supplies actor from the live caller.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    pub request_id: u64,
    pub actor: u64,
    pub operation: u32,
    pub reason: u32,
    pub object_kind: u32,
    pub decision: u32, // 1=read completed, 2=refused
    pub object: u64,   // source cursor for a page
    pub revision: u64,
    pub sampled_ms: u64,
    pub digest: u64, // non-authenticating FNV checksum of the projected page
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub version: u32,
    pub count: u32,
    pub next: u64, // zero means complete
    pub evidence: Evidence,
    pub entities: [Entity; PAGE_SIZE],
}
pub trait Provider {
    fn now_ms(&self) -> Result<u64, Reason>;
    fn slots(&self, source: u32) -> Result<usize, Reason>;
    fn read(&self, source: u32, index: usize) -> Result<Option<Entity>, Reason>;
}
fn fold(h: u64, value: u64) -> u64 {
    value
        .to_le_bytes()
        .iter()
        .fold(h, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}
fn entity_hash(mut h: u64, e: &Entity) -> u64 {
    // Timestamp is deliberately excluded from the source revision.
    for value in [
        e.version as u64,
        e.kind as u64,
        e.id,
        e.generation,
        e.owner,
        e.related_kind as u64,
        e.reason as u64,
        e.related_id,
        e.related_generation,
        e.state,
        e.source_revision,
    ]
    .into_iter()
    .chain(e.values)
    {
        h = fold(h, value);
    }
    h
}
fn checksum(page: &Page) -> u64 {
    let e = page.evidence;
    let mut h = 0xcbf29ce484222325;
    for v in [
        page.version as u64,
        page.count as u64,
        page.next,
        e.request_id,
        e.actor,
        e.operation as u64,
        e.reason as u64,
        e.object_kind as u64,
        e.decision as u64,
        e.object,
        e.revision,
        e.sampled_ms,
    ] {
        h = fold(h, v);
    }
    for entity in &page.entities {
        h = entity_hash(h, entity);
        h = fold(fold(h, entity.sampled_ms), entity.valid_for_ms);
    }
    h
}

/// Bounded cost: at most MAX_SCAN metadata reads, PAGE_SIZE returned records,
/// no allocation and no disk I/O. A changed projection invalidates continuation.
pub fn query(provider: &impl Provider, actor: u64, scopes: u32, q: &Query) -> Page {
    let mut page = Page {
        version: VERSION,
        evidence: Evidence {
            request_id: q.request_id,
            actor,
            operation: 1,
            object_kind: q.source,
            object: q.cursor,
            ..Evidence::default()
        },
        ..Page::default()
    };
    let result = (|| {
        if actor == 0 || scopes & SYSTEM_READ == 0 {
            return Err(Reason::Permission);
        }
        q.validate()?;
        let now = provider.now_ms()?;
        let slots = provider.slots(q.source)?;
        if slots > MAX_SCAN {
            return Err(Reason::Budget);
        }
        if q.cursor as usize > slots {
            return Err(Reason::Invalid);
        }
        let mut revision = fold(0xcbf29ce484222325, slots as u64);
        for index in 0..slots {
            revision = fold(revision, index as u64);
            let Some(mut entity) = provider.read(q.source, index)? else {
                revision = fold(revision, 0);
                continue;
            };
            entity.validate(q.source)?;
            revision = entity_hash(revision, &entity);
            if entity.sampled_ms > now
                || now - entity.sampled_ms > q.max_age_ms.min(entity.valid_for_ms)
            {
                return Err(Reason::Stale);
            }
            entity.valid_for_ms = entity.valid_for_ms.min(q.max_age_ms);
            if index >= q.cursor as usize {
                if page.count < q.limit as u32 {
                    page.entities[page.count as usize] = entity;
                    page.count += 1;
                } else if page.next == 0 {
                    page.next = index as u64;
                }
            }
        }
        revision = revision.max(1);
        if q.revision != 0 && q.revision != revision {
            return Err(Reason::Stale);
        }
        page.evidence.revision = revision;
        page.evidence.sampled_ms = now;
        Ok(())
    })();
    if let Err(reason) = result {
        page.count = 0;
        page.next = 0;
        page.entities.fill(Entity::default());
        page.evidence.reason = reason as u32;
        page.evidence.decision = 2;
        page.evidence.revision = 0;
        page.evidence.sampled_ms = 0;
    } else {
        page.evidence.decision = 1;
    }
    page.evidence.digest = checksum(&page);
    page
}
impl Page {
    pub fn validate(&self, query: &Query, actor: u64) -> Result<(), Reason> {
        let e = self.evidence;
        if self.version != VERSION
            || self.count as usize > PAGE_SIZE
            || e.actor != actor
            || e.request_id != query.request_id
            || e.operation != 1
            || e.object_kind != query.source
            || e.object != query.cursor
            || e.digest != checksum(self)
        {
            return Err(Reason::Integrity);
        }
        if e.decision == 2 {
            if e.reason == 0
                || e.reason > Reason::Integrity as u32
                || self.count != 0
                || self.next != 0
                || e.revision != 0
                || e.sampled_ms != 0
                || self.entities != [Entity::default(); PAGE_SIZE]
            {
                return Err(Reason::Integrity);
            }
            return Ok(());
        }
        if e.decision != 1
            || e.reason != 0
            || e.revision == 0
            || self.count as u64 > query.limit
            || (query.revision != 0 && query.revision != e.revision)
            || (self.next != 0 && (self.next <= query.cursor || self.next >= MAX_SCAN as u64))
            || self.entities[self.count as usize..]
                .iter()
                .any(|v| *v != Entity::default())
        {
            return Err(Reason::Integrity);
        }
        for entity in &self.entities[..self.count as usize] {
            entity.validate(query.source)?;
            if entity.sampled_ms > e.sampled_ms
                || e.sampled_ms - entity.sampled_ms > query.max_age_ms.min(entity.valid_for_ms)
                || entity.valid_for_ms > query.max_age_ms
            {
                return Err(Reason::Stale);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    Source,
    Cursor,
    Limit,
    Revision,
    MaxAgeMs,
    WindowToken,
    Target,
    Crop,
    Recipient,
    Disclosure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    SourceEnum,
    Unsigned64,
    WindowTarget,
    Rectangle,
    ProcessEndpoint,
    DisclosureEnum,
}
impl Parameter {
    pub const fn value_type(self) -> ValueType {
        match self {
            Self::Source => ValueType::SourceEnum,
            Self::Target => ValueType::WindowTarget,
            Self::Crop => ValueType::Rectangle,
            Self::Recipient => ValueType::ProcessEndpoint,
            Self::Disclosure => ValueType::DisclosureEnum,
            _ => ValueType::Unsigned64,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    ResourceSummary,
    EntityPage,
    WindowSemantics,
    ImageChunks,
    CaptureInfo,
    Empty,
}
#[derive(Clone, Copy, Debug)]
pub struct Operation {
    pub id: u32,
    pub name: &'static str,
    pub version: u32,
    pub parameters: &'static [Parameter],
    pub shape: Shape,
    pub required_scope: u32,
    pub max_items: usize,
    pub max_bytes: usize,
    pub mutates_capture: bool,
}
pub const RESOURCE_SUMMARY: u32 = 6;
pub const CATALOG: &[Operation] = &[
    Operation {
        id: 1,
        name: "system.entities",
        version: VERSION,
        parameters: &[
            Parameter::Source,
            Parameter::Cursor,
            Parameter::Limit,
            Parameter::Revision,
            Parameter::MaxAgeMs,
        ],
        shape: Shape::EntityPage,
        required_scope: SYSTEM_READ,
        max_items: PAGE_SIZE,
        max_bytes: core::mem::size_of::<Page>(),
        mutates_capture: false,
    },
    Operation {
        id: 2,
        name: "window.semantics",
        version: super::VERSION,
        parameters: &[Parameter::WindowToken, Parameter::Revision],
        shape: Shape::WindowSemantics,
        required_scope: WINDOW_READ,
        max_items: osaiku_ui::MAX_NODES,
        max_bytes: core::mem::size_of::<super::Snapshot>(),
        mutates_capture: false,
    },
    Operation {
        id: 3,
        name: "window.image",
        version: super::VERSION,
        parameters: &[
            Parameter::WindowToken,
            Parameter::Cursor,
            Parameter::Revision,
        ],
        shape: Shape::ImageChunks,
        required_scope: WINDOW_READ | WINDOW_IMAGE,
        max_items: 1,
        max_bytes: super::MAX_TRANSFER,
        mutates_capture: false,
    },
    Operation {
        id: 4,
        name: "window.capture",
        version: super::VERSION,
        parameters: &[
            Parameter::Target,
            Parameter::Revision,
            Parameter::Crop,
            Parameter::Recipient,
            Parameter::Disclosure,
        ],
        shape: Shape::CaptureInfo,
        required_scope: OWNER_CONTROL,
        max_items: 1,
        max_bytes: core::mem::size_of::<super::Info>(),
        mutates_capture: true,
    },
    Operation {
        id: 5,
        name: "window.release",
        version: super::VERSION,
        parameters: &[Parameter::WindowToken],
        shape: Shape::Empty,
        required_scope: WINDOW_READ,
        max_items: 0,
        max_bytes: 0,
        mutates_capture: true,
    },
    Operation {
        id: RESOURCE_SUMMARY,
        name: "system.resource_summary",
        version: VERSION,
        parameters: &[],
        shape: Shape::ResourceSummary,
        required_scope: SYSTEM_READ,
        max_items: 1,
        max_bytes: 1024,
        mutates_capture: false,
    },
];

/// Correlated service receipt and the documented memory projection. CPU load
/// is unavailable; admission budgets are not physical memory consumption.
pub struct ResourceSummary {
    pub evidence: Evidence,
    pub memory: Entity,
    pub expires_ms: u64,
}
pub fn resource_summary_with(
    actor: u64,
    request: u64,
    exchange: impl FnMut(&Query, &mut Page) -> isize,
    clock: impl FnOnce() -> Result<u64, Reason>,
) -> Result<ResourceSummary, Reason> {
    let q = Query::first(MEMORY, request);
    let mut page = Page::default();
    read_with(&q, actor, &mut page, exchange)?;
    if page.evidence.decision != 1 {
        return Err(if page.evidence.reason == Reason::Permission as u32 {
            Reason::Permission
        } else {
            Reason::Unavailable
        });
    }
    if page.count != 1 || page.next != 0 || page.entities[0].reason != 0 {
        return Err(Reason::Unavailable);
    }
    let memory = page.entities[0];
    let expires_ms = memory
        .sampled_ms
        .checked_add(memory.valid_for_ms)
        .ok_or(Reason::Invalid)?;
    let now = clock()?;
    if now < memory.sampled_ms || now >= expires_ms {
        return Err(Reason::Stale);
    }
    Ok(ResourceSummary {
        evidence: page.evidence,
        memory,
        expires_ms,
    })
}

pub fn read_with(
    q: &Query,
    actor: u64,
    out: &mut Page,
    mut exchange: impl FnMut(&Query, &mut Page) -> isize,
) -> Result<(), Reason> {
    *out = Page::default();
    q.validate()?;
    let code = exchange(q, out);
    let result = if code == 0 {
        out.validate(q, actor)
    } else {
        Err(Reason::transport(code))
    };
    if result.is_err() {
        *out = Page::default();
    }
    result
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn exchange(q: &Query, out: &mut Page) -> isize {
    unsafe {
        crate::raw::syscall(
            crate::abi::number::SYSTEM_CONTEXT,
            [q as *const _ as u64, out as *mut _ as u64, 0, 0, 0],
        ) as isize
    }
}
const _: () = assert!(core::mem::size_of::<Query>() == 48);
const _: () = assert!(core::mem::size_of::<Entity>() == 152);
const _: () = assert!(core::mem::size_of::<Evidence>() == 64);
const _: () = assert!(core::mem::size_of::<Page>() == 688);
