//! Bounded block-class validation and completion ownership, independent of media.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lba(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The operation's owner cancelled it; this says nothing about media health.
    Cancelled,
    Stale,
    InvalidRange,
    Alignment,
    Length,
    Permission,
    ReadOnly,
    Resources,
    Timeout,
    Transport,
    Integrity,
    NotReady,
    Removed,
    Unsupported,
    Quarantined,
    RecoveryRequired,
    Conflict,
    NoSpace,
    UnknownCommit,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    RamEphemeral,
    Nvme,
    UsbBot,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Ready,
    Degraded,
    ReadOnly,
    Removed,
    Quarantined,
    RecoveryRequired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub endpoint: u64,
    pub backend_generation: u64,
    pub device_generation: u64,
    pub namespace_generation: u64,
    pub media_generation: u64,
    pub target_hash: u64,
}
impl Identity {
    pub fn valid(self) -> bool {
        self.endpoint != 0
            && self.backend_generation != 0
            && self.device_generation != 0
            && self.namespace_generation != 0
            && self.media_generation != 0
            && self.target_hash != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub start: Lba,
    pub blocks: u64,
}
impl Region {
    pub fn end(self) -> Result<u64, Error> {
        if self.blocks == 0 {
            return Err(Error::InvalidRange);
        }
        self.start
            .0
            .checked_add(self.blocks)
            .ok_or(Error::InvalidRange)
    }
    /// Class requests are region-relative. Convert only at the backend edge.
    pub fn physical_lba(self, relative: Lba, blocks: u64) -> Result<Lba, Error> {
        if blocks == 0
            || relative
                .0
                .checked_add(blocks)
                .is_none_or(|end| end > self.blocks)
        {
            return Err(Error::InvalidRange);
        }
        self.end()?;
        Ok(Lba(self
            .start
            .0
            .checked_add(relative.0)
            .ok_or(Error::InvalidRange)?))
    }
    pub fn contains(self, other: Self) -> bool {
        matches!((self.end(), other.end()), (Ok(a), Ok(b)) if other.start.0 >= self.start.0 && b <= a)
    }
    pub fn overlaps(self, other: Self) -> Result<bool, Error> {
        Ok(self.start.0 < other.end()? && other.start.0 < self.end()?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    pub read_only: bool,
    pub removable: bool,
    pub volatile_write_cache: bool,
    pub flush: bool,
    pub discard: bool,
    pub fua: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub identity: Identity,
    pub backend: BackendKind,
    pub backend_version: u32,
    pub block_bytes: u32,
    pub physical_write_bytes: Option<u32>,
    pub namespace_blocks: u64,
    pub region: Region,
    pub max_blocks: u32,
    pub max_bytes: u32,
    pub buffer_alignment: u32,
    pub lba_alignment: u32,
    pub queue_depth: u16,
    pub capabilities: Capabilities,
    pub state: State,
}
pub const QUEUE_CEILING: usize = 8;
impl Geometry {
    pub fn validate(self) -> Result<(), Error> {
        if !self.identity.valid()
            || self.backend_version == 0
            || !self.block_bytes.is_power_of_two()
            || self.block_bytes < 512
            || !self.buffer_alignment.is_power_of_two()
            || !self.lba_alignment.is_power_of_two()
            || self.max_blocks == 0
            || self.max_bytes < self.block_bytes
            || self.queue_depth == 0
            || usize::from(self.queue_depth) > QUEUE_CEILING
            || self.region.start.0 % u64::from(self.lba_alignment) != 0
            || self.region.end()? > self.namespace_blocks
        {
            return Err(Error::InvalidRange);
        }
        if let Some(size) = self.physical_write_bytes {
            if size < self.block_bytes || !size.is_power_of_two() {
                return Err(Error::Alignment);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Read,
    Write,
    Flush,
    Query,
    Discard,
    Fua,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    None,
    IntoBuffer,
    FromBuffer,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub identity: Identity,
    pub id: u64,
    pub operation: Operation,
    pub range: Region,
    pub buffer_token: u64,
    pub direction: Direction,
    pub ordering_domain: u64,
    pub predecessor: u64,
    pub timeout_ticks: u64,
    pub ticket: u64,
    pub hash: u64,
}
impl Request {
    /// FNV is a canonical corruption check, never a source of authority.
    pub fn compute_hash(self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for word in [
            self.identity.endpoint,
            self.identity.backend_generation,
            self.identity.device_generation,
            self.identity.namespace_generation,
            self.identity.media_generation,
            self.identity.target_hash,
            self.id,
            self.operation as u64,
            self.range.start.0,
            self.range.blocks,
            self.buffer_token,
            self.direction as u64,
            self.ordering_domain,
            self.predecessor,
            self.timeout_ticks,
            self.ticket,
        ] {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        hash
    }
    pub fn seal(mut self) -> Self {
        self.hash = self.compute_hash();
        self
    }
}

pub fn validate_request(
    g: Geometry,
    r: Request,
    buffer_len: usize,
    buffer_address: usize,
    admitted_ticket: Option<u64>,
) -> Result<(), Error> {
    if g.backend != BackendKind::RamEphemeral
        && matches!(r.operation, Operation::Write | Operation::Flush)
    {
        return Err(Error::Permission);
    }
    validate_common(g, r, buffer_len, buffer_address, admitted_ticket)
}

pub fn validate_common(
    g: Geometry,
    r: Request,
    buffer_len: usize,
    buffer_address: usize,
    admitted_ticket: Option<u64>,
) -> Result<(), Error> {
    g.validate()?;
    if r.identity != g.identity {
        return Err(Error::Stale);
    }
    match g.state {
        State::Removed => return Err(Error::Removed),
        State::Quarantined => return Err(Error::Quarantined),
        State::RecoveryRequired => return Err(Error::RecoveryRequired),
        _ => (),
    }
    if r.hash != r.compute_hash() {
        return Err(Error::Integrity);
    }
    if r.ordering_domain != 1 {
        return Err(Error::Unsupported);
    }
    if r.id == 0 || r.timeout_ticks == 0 || r.predecessor >= r.id {
        return Err(Error::InvalidRange);
    }
    if matches!(r.operation, Operation::Discard | Operation::Fua) {
        return Err(Error::Unsupported);
    }
    if matches!(r.operation, Operation::Write | Operation::Flush) {
        if admitted_ticket != Some(r.ticket) || r.ticket == 0 {
            return Err(Error::Permission);
        }
        if g.capabilities.read_only || g.state == State::ReadOnly {
            return Err(Error::ReadOnly);
        }
    } else if r.ticket != 0 {
        return Err(Error::Permission);
    }
    if matches!(r.operation, Operation::Query | Operation::Flush) {
        if r.range.blocks != 0
            || r.range.start.0 != 0
            || buffer_len != 0
            || r.buffer_token != 0
            || r.direction != Direction::None
        {
            return Err(Error::Length);
        }
        if r.operation == Operation::Flush && !g.capabilities.flush {
            return Err(Error::Unsupported);
        }
        return Ok(());
    }
    if r.range.end()? > g.region.blocks || r.range.blocks > u64::from(g.max_blocks) {
        return Err(Error::InvalidRange);
    }
    let bytes = r
        .range
        .blocks
        .checked_mul(u64::from(g.block_bytes))
        .ok_or(Error::Length)?;
    if bytes != buffer_len as u64 || bytes > u64::from(g.max_bytes) || r.buffer_token == 0 {
        return Err(Error::Length);
    }
    if buffer_address % g.buffer_alignment as usize != 0
        || r.range.start.0 % u64::from(g.lba_alignment) != 0
    {
        return Err(Error::Alignment);
    }
    let direction = if r.operation == Operation::Read {
        Direction::IntoBuffer
    } else {
        Direction::FromBuffer
    };
    if r.direction != direction {
        return Err(Error::Permission);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command {
    pub request: Request,
    pub queue: u16,
    pub command: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    pub identity: Identity,
    pub request_id: u64,
    pub queue: u16,
    pub command: u16,
    pub blocks: u64,
    pub status: Result<(), Error>,
}
#[derive(Clone, Copy)]
struct Pending {
    command: Command,
    quarantined: bool,
    deadline: u64,
}

/// Storage is supplied by the resource owner; no allocator or media-sized scan.
pub struct Queue {
    slots: [Option<Pending>; QUEUE_CEILING],
    depth: usize,
    last_id: u64,
    completed_id: u64,
    successful_high: u64,
    high_water: usize,
}
impl Queue {
    pub const fn new(depth: usize) -> Self {
        Self {
            slots: [None; QUEUE_CEILING],
            depth,
            last_id: 0,
            completed_id: 0,
            successful_high: 0,
            high_water: 0,
        }
    }
    pub fn next_id(&self) -> Result<u64, Error> {
        self.last_id.checked_add(1).ok_or(Error::Exhausted)
    }
    pub fn in_use(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
    pub fn high_water(&self) -> usize {
        self.high_water
    }
    pub fn admit(&mut self, command: Command) -> Result<(), Error> {
        self.admit_at(command, 0)
    }
    pub fn admit_at(&mut self, command: Command, now: u64) -> Result<(), Error> {
        // One ledger owns exactly one ordering domain in 29A.
        if command.request.ordering_domain != 1 {
            return Err(Error::Unsupported);
        }
        let deadline = now
            .checked_add(command.request.timeout_ticks)
            .ok_or(Error::InvalidRange)?;
        if command.request.timeout_ticks == 0 {
            return Err(Error::InvalidRange);
        }
        if self.depth == 0 || self.depth > QUEUE_CEILING {
            return Err(Error::Resources);
        }
        if command.request.id != self.next_id()? {
            return Err(Error::Stale);
        }
        // A predecessor is an ordering boundary, not just an earlier submission.
        if command.request.predecessor > self.completed_id {
            return Err(Error::NotReady);
        }
        if self
            .slots
            .iter()
            .flatten()
            .any(|p| p.command.queue == command.queue && p.command.command == command.command)
        {
            return Err(Error::Resources);
        }
        let slot = self.slots[..self.depth]
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::Resources)?;
        *slot = Some(Pending {
            command,
            quarantined: false,
            deadline,
        });
        self.last_id = command.request.id;
        self.high_water = self.high_water.max(self.in_use());
        Ok(())
    }
    pub fn complete(&mut self, c: Completion) -> Result<(), Error> {
        let slot = self
            .slots
            .iter_mut()
            .find(|s| {
                s.is_some_and(|p| {
                    p.command.request.identity == c.identity
                        && p.command.request.id == c.request_id
                        && p.command.queue == c.queue
                        && p.command.command == c.command
                })
            })
            .ok_or(Error::Stale)?;
        let pending = slot.as_mut().ok_or(Error::Stale)?;
        if pending.quarantined {
            return Err(Error::Quarantined);
        }
        if c.status.is_err() || c.blocks != pending.command.request.range.blocks {
            pending.quarantined = true;
            return Err(c.status.err().unwrap_or(Error::Integrity));
        }
        *slot = None;
        self.successful_high = self.successful_high.max(c.request_id);
        self.completed_id = self
            .slots
            .iter()
            .flatten()
            .map(|p| p.command.request.id - 1)
            .min()
            .unwrap_or(self.successful_high)
            .min(self.successful_high);
        Ok(())
    }
    /// Bounded pump timeout handling retains uncertain ownership, never frees it.
    pub fn expire(&mut self, now: u64) -> usize {
        let mut expired = 0;
        for p in self.slots.iter_mut().flatten() {
            if !p.quarantined && now >= p.deadline {
                p.quarantined = true;
                expired += 1;
            }
        }
        expired
    }
    pub fn invalidate(&mut self, identity: Identity) {
        for pending in self.slots.iter_mut().flatten() {
            if pending.command.request.identity == identity {
                pending.quarantined = true;
            }
        }
    }
    pub fn retire_quarantined(&mut self, proof: &QuiescenceProof) {
        for slot in &mut self.slots {
            if slot.is_some_and(|p| p.quarantined && p.command.request.identity == proof.old) {
                *slot = None;
            }
        }
        // The monotonic request sequence is deliberately not reset.
    }
}

/// Move-only kernel proof, never derived from a timeout, user data, or Drop.
pub struct QuiescenceProof {
    old: Identity,
    replacement: Identity,
}
impl QuiescenceProof {
    /// # Safety
    /// The driver resource owner must have established that *all* queues and
    /// DMA mappings of `old` can no longer be accessed by the controller. Merely
    /// issuing a reset is insufficient. No production caller exists in 29A.
    pub unsafe fn after_hardware_shutdown(
        old: Identity,
        replacement: Identity,
    ) -> Result<Self, Error> {
        if !old.valid()
            || !replacement.valid()
            || old.target_hash != replacement.target_hash
            || replacement.backend_generation <= old.backend_generation
        {
            return Err(Error::Stale);
        }
        Ok(Self { old, replacement })
    }
    pub fn old(&self) -> Identity {
        self.old
    }
    pub fn replacement(&self) -> Identity {
        self.replacement
    }
}
