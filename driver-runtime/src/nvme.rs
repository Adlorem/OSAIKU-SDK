//! NVM Express 1.4c command/completion encoding. No MMIO or filesystem policy.
use super::block::{Error, Operation};

pub const QUEUE_ENTRIES: u16 = 2; // admin ring depth two, at most one outstanding command
/// I/O ring entries (one 4 KiB submission page holds 64). A ring keeps one
/// entry free, so at most `IO_QUEUE_ENTRIES - 1` commands are outstanding; the
/// controller's CAP.MQES can lower it.
pub const IO_QUEUE_ENTRIES: u16 = 32;
pub const MAX_OUTSTANDING: usize = IO_QUEUE_ENTRIES as usize - 1;
pub const MAX_TRANSFER_BYTES: usize = 4096;
/// Private sequential I/O ceiling. General block requests retain their one-page
/// envelope; bulk writes require a separately validated partition capability.
/// 256 KiB (64 pages) still fits one unchained PRP list page; the controller's
/// MDTS can lower it further.
pub const MAX_IO_BYTES: usize = 256 * 1024;
pub const MAX_READ_BYTES: usize = MAX_IO_BYTES;
pub const READ_PAGES: usize = MAX_READ_BYTES / 4096;
pub const DMA_PAGES: u64 = 4 + 2 * READ_PAGES as u64 + 1;
/// Four queues, separate read/write buffers, and a shared PRP-list page.
/// Logical mapping indices remain stable and fit the DMA owner's seven slots.
pub fn dma_region(index: usize) -> Result<(u64, u64), Error> {
    let (page, pages) = match index {
        0..=3 => (index, 1),
        4 => (4, READ_PAGES),
        5 => (4 + READ_PAGES, READ_PAGES),
        6 => (4 + 2 * READ_PAGES, 1),
        _ => return Err(Error::InvalidRange),
    };
    Ok(((page * 4096) as u64, (pages * 4096) as u64))
}
/// MDTS is a power of two in minimum-page units (4 KiB); zero means no limit.
pub fn read_limit(mdts: u8) -> usize {
    if mdts == 0 || u32::from(mdts) >= READ_PAGES.trailing_zeros() {
        MAX_READ_BYTES
    } else {
        4096usize << mdts
    }
}

/// Page-aligned owned transfer buffer and one unchained PRP list. The driver must
/// obtain both bus addresses from its DMA owner before constructing this plan.
/// The list may start at any qword offset (NVMe 1.4 4.3) but never crosses its
/// page, so several commands in flight share one list page without chaining.
pub struct TransferPlan {
    bytes: usize,
    first: u64,
    second: u64,
    pub list: [u64; READ_PAGES - 1],
}
pub type ReadPlan = TransferPlan;
impl TransferPlan {
    pub fn new(bytes: usize, first: u64, list_address: u64) -> Result<Self, Error> {
        if bytes == 0 || bytes > MAX_READ_BYTES || bytes % 512 != 0 {
            return Err(Error::Length);
        }
        let pages = bytes.div_ceil(4096);
        let end = first
            .checked_add((pages * 4096) as u64)
            .ok_or(Error::InvalidRange)?;
        if first == 0 || first % 4096 != 0 {
            return Err(Error::Alignment);
        }
        let mut result = Self {
            bytes,
            first,
            second: 0,
            list: [0; READ_PAGES - 1],
        };
        if pages == 2 {
            result.second = first + 4096;
        } else if pages > 2 {
            let list_bytes = ((pages - 1) * 8) as u64;
            (list_address & !4095)
                .checked_add(4096)
                .ok_or(Error::InvalidRange)?;
            let list_end = list_address + list_bytes;
            if list_address == 0 || list_address % 8 != 0 || list_address % 4096 + list_bytes > 4096
            {
                return Err(Error::Alignment);
            }
            if first < list_end && list_address < end {
                return Err(Error::Conflict);
            }
            result.second = list_address;
            for i in 1..pages {
                result.list[i - 1] = first + (i * 4096) as u64;
            }
        }
        Ok(result)
    }
    pub fn command(&self, id: u16, namespace: u32, lba: u64) -> Result<Submission, Error> {
        self.command_for(Operation::Read, id, namespace, lba)
    }
    /// List entries the caller must store at the plan's list address.
    pub fn list_entries(&self) -> &[u64] {
        let pages = self.bytes.div_ceil(4096);
        if pages > 2 {
            &self.list[..pages - 1]
        } else {
            &[]
        }
    }
    pub fn command_for(
        &self,
        operation: Operation,
        id: u16,
        namespace: u32,
        lba: u64,
    ) -> Result<Submission, Error> {
        let blocks = (self.bytes / 512) as u64;
        if id == 0 || namespace == 0 || lba.checked_add(blocks).is_none() {
            return Err(Error::InvalidRange);
        }
        let opcode = if operation == Operation::Write { 1 } else { 2 };
        let mut command = Submission::admin(
            opcode,
            id,
            namespace,
            self.first,
            lba as u32,
            (lba >> 32) as u32,
        );
        command.0[8] = self.second as u32;
        command.0[9] = (self.second >> 32) as u32;
        command.0[12] = blocks as u32 - 1;
        Ok(command)
    }
}

/// Reviewed protocol envelopes, not a vendor/class allow-all or a list of
/// qualified physical devices. Matching confers no DMA or write authority.
struct MatchRule {
    id: u32,
    minimum_version: u32,
    maximum_version: u32,
    maximum_doorbell_stride: u8,
    maximum_ready_timeout_ms: u64,
}
const MATCH_RULES: &[MatchRule] = &[
    MatchRule {
        id: 1,
        minimum_version: 0x0001_0000,
        maximum_version: 0x0001_0400,
        maximum_doorbell_stride: 4,
        maximum_ready_timeout_ms: 10_000,
    },
    MatchRule {
        // Native NVMe 1.2+ controllers may advertise a 30-second CAP.TO.
        // This bounded extension changes no command, DMA or write authorization.
        id: 2,
        minimum_version: 0x0001_0200,
        maximum_version: 0x0001_0400,
        maximum_doorbell_stride: 4,
        maximum_ready_timeout_ms: 30_000,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    pub match_rule: u32,
    pub doorbell_stride: usize,
    pub ready_timeout_ms: u64,
    /// CAP.MQES + 1: the largest ring the controller accepts (at least 2).
    pub max_queue_entries: u32,
}
/// Register facts contain protocol values only, never a device address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityRejection {
    QueueDepth,
    CommandSet,
    PageSize,
    Version,
    DoorbellStride,
    ReadyTimeout,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityProbe {
    pub cap: u64,
    pub version: u32,
    pub rejection: Option<CapabilityRejection>,
}
impl CapabilityProbe {
    pub fn read(cap: u64, version: u32) -> Self {
        Self {
            cap,
            version,
            rejection: Capabilities::check(cap, version).err(),
        }
    }
}
impl Capabilities {
    pub fn parse(cap: u64, version: u32) -> Result<Self, Error> {
        Self::check(cap, version).map_err(|_| Error::Unsupported)
    }
    fn check(cap: u64, version: u32) -> Result<Self, CapabilityRejection> {
        use CapabilityRejection::*;
        if cap & 0xffff < 1 {
            return Err(QueueDepth);
        }
        if cap & (1 << 37) == 0 {
            return Err(CommandSet);
        }
        if (cap >> 48) & 15 != 0 {
            return Err(PageSize);
        }
        let timeout = ((cap >> 24) & 255).max(1) * 500;
        // Diagnose the first unmatched constraint across the reviewed rules.
        let version_rules = || {
            MATCH_RULES
                .iter()
                .filter(|rule| (rule.minimum_version..=rule.maximum_version).contains(&version))
        };
        if version_rules().next().is_none() {
            return Err(Version);
        }
        let stride_rules = || {
            version_rules()
                .filter(|rule| (cap >> 32) & 15 <= u64::from(rule.maximum_doorbell_stride))
        };
        if stride_rules().next().is_none() {
            return Err(DoorbellStride);
        }
        let rule = stride_rules()
            .find(|rule| timeout <= rule.maximum_ready_timeout_ms)
            .ok_or(ReadyTimeout)?;
        Ok(Self {
            match_rule: rule.id,
            doorbell_stride: 4usize << ((cap >> 32) & 15),
            ready_timeout_ms: timeout,
            max_queue_entries: (cap & 0xffff) as u32 + 1,
        })
    }
    /// Ring entries for the single I/O queue pair.
    pub fn io_queue_entries(self) -> u16 {
        self.max_queue_entries.clamp(2, u32::from(IO_QUEUE_ENTRIES)) as u16
    }
    pub fn doorbell(self, queue: u16, completion: bool) -> Result<usize, Error> {
        if queue > 1 {
            return Err(Error::Unsupported);
        }
        Ok(0x1000 + (usize::from(queue) * 2 + usize::from(completion)) * self.doorbell_stride)
    }
}
#[path = "deadline.rs"]
mod deadline;
pub use deadline::ReadyDeadline;

/// CNS=03 was introduced in NVMe 1.3. Never probe it on a 1.2 controller.
pub fn namespace_identification_cns(version: u32) -> Result<Option<u8>, Error> {
    match version {
        0x0001_0200 | 0x0001_0201 => Ok(None),
        0x0001_0300 | 0x0001_0400 => Ok(Some(3)),
        _ => Err(Error::Unsupported),
    }
}
/// Older controllers carry NGUID/EUI64 in Identify Namespace itself. Refuse a
/// namespace without either persistent identifier rather than inventing one.
pub fn legacy_namespace_identifier(bytes: &[u8; 4096]) -> Result<[u8; 24], Error> {
    let id: [u8; 24] = bytes[104..128].try_into().unwrap();
    if id == [0; 24] {
        return Err(Error::Unsupported);
    }
    Ok(id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Namespace {
    pub blocks: u64,
    pub block_bytes: u32,
}
impl Namespace {
    pub fn parse(bytes: &[u8; 4096]) -> Result<Self, Error> {
        let size = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let capacity = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let format = usize::from(bytes[26] & 15);
        if size == 0
            || capacity == 0
            || capacity > size
            || bytes[26] & 0xf0 != 0
            || bytes[25] >= 16
            || format > usize::from(bytes[25])
            || bytes[29] != 0
        {
            return Err(Error::Unsupported);
        }
        let entry = 128 + 4 * format;
        if bytes[entry] != 0 || bytes[entry + 1] != 0 || bytes[entry + 2] != 9 {
            return Err(Error::Unsupported);
        }
        Ok(Self {
            blocks: size,
            block_bytes: 512,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Submission(pub [u32; 16]);
impl Submission {
    pub fn admin(opcode: u8, id: u16, namespace: u32, dma: u64, cdw10: u32, cdw11: u32) -> Self {
        let mut words = [0; 16];
        words[0] = u32::from(opcode) | (u32::from(id) << 16);
        words[1] = namespace;
        words[6] = dma as u32;
        words[7] = (dma >> 32) as u32;
        words[10] = cdw10;
        words[11] = cdw11;
        Self(words)
    }
    pub fn io(
        operation: Operation,
        id: u16,
        namespace: u32,
        lba: u64,
        blocks: u32,
        block_bytes: u32,
        dma: u64,
    ) -> Result<Self, Error> {
        if id == 0 || namespace == 0 {
            return Err(Error::InvalidRange);
        }
        if operation == Operation::Flush {
            if blocks != 0 || dma != 0 || lba != 0 {
                return Err(Error::Length);
            }
            return Ok(Self::admin(0, id, namespace, 0, 0, 0));
        }
        let opcode = match operation {
            Operation::Read => 2,
            Operation::Write => 1,
            _ => return Err(Error::Unsupported),
        };
        let bytes = blocks.checked_mul(block_bytes).ok_or(Error::Length)?;
        if blocks == 0
            || block_bytes != 512
            || bytes > MAX_TRANSFER_BYTES as u32
            || dma == 0
            || dma % 4096 != 0
            || lba.checked_add(u64::from(blocks)).is_none()
        {
            return Err(Error::InvalidRange);
        }
        let mut command = Self::admin(opcode, id, namespace, dma, lba as u32, (lba >> 32) as u32);
        command.0[12] = blocks - 1;
        Ok(command)
    }
}

pub struct Queue {
    generation: u64,
    id: u16,
    // Ring size; at most entries - 1 commands are outstanding.
    entries: u16,
    // Software lifetime sequence; the wire CID only identifies outstanding
    // commands in this queue, not every command since controller startup.
    next_command: u64,
    // Outstanding wire CIDs (0 = free slot).
    waiting: [u16; MAX_OUTSTANDING],
    outstanding: usize,
    high_water: usize,
    submission_tail: u16,
    // Last submission-queue head reported by the controller.
    submission_head: u16,
    completion_head: u16,
    phase: bool,
    quarantined: bool,
}
impl Queue {
    pub const fn new(generation: u64, id: u16) -> Self {
        Self::with_entries(generation, id, QUEUE_ENTRIES)
    }
    /// `entries` outside 2..=IO_QUEUE_ENTRIES makes every issue fail closed.
    pub const fn with_entries(generation: u64, id: u16, entries: u16) -> Self {
        Self {
            generation,
            id,
            entries,
            next_command: 1,
            waiting: [0; MAX_OUTSTANDING],
            outstanding: 0,
            high_water: 0,
            submission_tail: 0,
            submission_head: 0,
            completion_head: 0,
            phase: true,
            quarantined: false,
        }
    }
    /// Commands that may be outstanding at once.
    pub fn capacity(&self) -> usize {
        usize::from(self.entries)
            .saturating_sub(1)
            .min(MAX_OUTSTANDING)
    }
    pub fn issue(&mut self) -> Result<(u16, u16), Error> {
        if self.quarantined {
            return Err(Error::Quarantined);
        }
        if self.generation == 0 || self.id > 1 || !(2..=IO_QUEUE_ENTRIES).contains(&self.entries) {
            return Err(Error::Stale);
        }
        if self.outstanding >= self.capacity() {
            return Err(Error::Resources);
        }
        // Reuse a wire CID only after its validated completion has retired;
        // quarantine/timeout never clear ownership for reuse. CQ head/phase and
        // controller generation persist across this rollover, and the driver
        // acknowledges CQ retirement before returning to its caller.
        let command = loop {
            let command = ((self.next_command - 1) % u64::from(u16::MAX) + 1) as u16;
            self.next_command = self.next_command.checked_add(1).ok_or(Error::Exhausted)?;
            if !self.waiting.contains(&command) {
                break command;
            }
        };
        let free = self
            .waiting
            .iter()
            .position(|w| *w == 0)
            .ok_or(Error::Resources)?;
        self.waiting[free] = command;
        self.outstanding += 1;
        self.high_water = self.high_water.max(self.outstanding);
        let slot = self.submission_tail;
        self.submission_tail = (slot + 1) % self.entries;
        Ok((command, slot))
    }
    pub fn submission_tail(&self) -> u16 {
        self.submission_tail
    }
    pub fn completion_head(&self) -> u16 {
        self.completion_head
    }
    pub fn in_use(&self) -> usize {
        self.outstanding
    }
    pub fn quiescent(&self) -> bool {
        !self.quarantined && self.outstanding == 0 && self.generation != 0
    }
    pub fn high_water(&self) -> usize {
        self.high_water
    }
    pub fn ready(&self, status: u32) -> bool {
        ((status >> 16) & 1 != 0) == self.phase
    }
    pub fn quarantine(&mut self) {
        self.quarantined = true;
    }
    fn distance(&self, from: u16, to: u16) -> u16 {
        (to + self.entries - from) % self.entries
    }
    pub fn complete(&mut self, generation: u64, entry: [u32; 4]) -> Result<u32, Error> {
        self.retire(generation, entry)
            .and_then(|(_, status)| status)
    }
    /// Retire one completion entry. The outer error is a protocol violation
    /// (the queue is quarantined or stale); the inner result is the command's
    /// own status. Completions may arrive in any order; each must name an
    /// outstanding CID and a submission head between the last reported head
    /// and the current tail.
    pub fn retire(
        &mut self,
        generation: u64,
        entry: [u32; 4],
    ) -> Result<(u16, Result<u32, Error>), Error> {
        if generation != self.generation {
            return Err(Error::Stale);
        }
        if self.quarantined {
            return Err(Error::Quarantined);
        }
        if !self.ready(entry[3]) {
            return Err(Error::NotReady);
        }
        let command = entry[3] as u16;
        let head = entry[2] as u16;
        let slot = self
            .waiting
            .iter()
            .position(|w| command != 0 && *w == command);
        let Some(slot) = slot.filter(|_| {
            (entry[2] >> 16) as u16 == self.id
                && head < self.entries
                && self.distance(self.submission_head, head)
                    <= self.distance(self.submission_head, self.submission_tail)
        }) else {
            self.quarantined = true;
            return Err(Error::Stale);
        };
        self.submission_head = head;
        self.waiting[slot] = 0;
        self.outstanding -= 1;
        self.completion_head = (self.completion_head + 1) % self.entries;
        if self.completion_head == 0 {
            self.phase = !self.phase;
        }
        let status = (entry[3] >> 17) & 0x7fff;
        if status != 0 {
            let code = status & 255;
            let kind = (status >> 8) & 7;
            return Ok((
                command,
                Err(match (kind, code) {
                    (0, 1) => Error::Unsupported,
                    (0, 2) | (0, 0x80) => Error::InvalidRange,
                    (0, 0x81) => Error::NoSpace,
                    (0, 0x82) => Error::NotReady,
                    (0, 0x20) | (1, 0x82) => Error::ReadOnly,
                    (2, _) => Error::Integrity,
                    _ => Error::Transport,
                }),
            ));
        }
        Ok((command, Ok(entry[0])))
    }
}
