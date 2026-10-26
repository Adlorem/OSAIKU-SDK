//! Copied asynchronous block requests. The owner, never a peer, supplies geometry
//! and already-authorized write ranges. No caller slice or address is retained.
use crate::block::{self, Error, Geometry, Identity, Operation, Region, Request};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Durability {
    Volatile,
    DeviceCache,
    Stable,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Complete(Durability),
    Failed(Error),
    UnknownCommit,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    pub identity: Identity,
    pub id: u64,
}
#[derive(Clone, Copy)]
pub struct Dispatch {
    pub token: Token,
    pub operation: Operation,
    pub physical: Region,
    pub bytes: usize,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    Queued,
    Submitted,
    Done,
    Uncertain,
}
struct Slot<const B: usize> {
    request: Request,
    phase: Phase,
    bytes: [u8; B],
    length: usize,
    received: usize,
    deadline: u64,
    outcome: Option<Outcome>,
}
pub struct Queue<const N: usize, const B: usize> {
    geometry: Geometry,
    slots: [Option<Slot<B>>; N],
    last_id: u64,
    accepting: bool,
    dirty: bool,
}
impl<const N: usize, const B: usize> Queue<N, B> {
    pub fn new(geometry: Geometry) -> Result<Self, Error> {
        geometry.validate()?;
        if N == 0
            || N > block::QUEUE_CEILING
            || N < usize::from(geometry.queue_depth)
            || B < geometry.max_bytes as usize
            || B > 256 * 1024
        {
            return Err(Error::Resources);
        }
        Ok(Self {
            geometry,
            slots: [const { None }; N],
            last_id: 0,
            accepting: true,
            dirty: false,
        })
    }
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
    /// `write_range` is supplied only after the storage owner's policy check.
    /// This contract object does not mint a capability or authorize hardware.
    pub fn submit(
        &mut self,
        request: Request,
        data: &[u8],
        write_range: Option<Region>,
        now: u64,
    ) -> Result<Token, Error> {
        if !self.accepting {
            return Err(Error::Quarantined);
        }
        let length = usize::try_from(
            request
                .range
                .blocks
                .checked_mul(u64::from(self.geometry.block_bytes))
                .ok_or(Error::Length)?,
        )
        .map_err(|_| Error::Length)?;
        let mutation = matches!(request.operation, Operation::Write | Operation::Flush);
        let ticket = if mutation {
            let allowed = write_range.ok_or(Error::Permission)?;
            if request.operation == Operation::Write && !allowed.contains(request.range) {
                return Err(Error::Permission);
            }
            if !self.geometry.region.contains(Region {
                start: block::Lba(
                    self.geometry
                        .region
                        .start
                        .0
                        .checked_add(allowed.start.0)
                        .ok_or(Error::InvalidRange)?,
                ),
                blocks: allowed.blocks,
            }) {
                return Err(Error::Permission);
            }
            Some(request.ticket)
        } else {
            None
        };
        block::validate_common(self.geometry, request, length, 0, ticket)?;
        if request.id != self.last_id.checked_add(1).ok_or(Error::Exhausted)? {
            return Err(Error::Stale);
        }
        if (request.operation == Operation::Write && data.len() != length)
            || (request.operation != Operation::Write && !data.is_empty())
        {
            return Err(Error::Length);
        }
        // One ordered stream. A flush cannot overtake any earlier request.
        if self.slots.iter().flatten().any(|s| {
            s.phase != Phase::Done
                && (request.operation == Operation::Flush
                    || s.request.operation == Operation::Flush
                    || s.request.id <= request.predecessor)
        }) {
            return Err(Error::NotReady);
        }
        let deadline = now
            .checked_add(request.timeout_ticks)
            .ok_or(Error::InvalidRange)?;
        let slot = self.slots[..usize::from(self.geometry.queue_depth)]
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::Resources)?;
        let mut bytes = [0; B];
        bytes[..data.len()].copy_from_slice(data);
        *slot = Some(Slot {
            request,
            phase: Phase::Queued,
            bytes,
            length,
            received: 0,
            deadline,
            outcome: None,
        });
        self.last_id = request.id;
        Ok(Token {
            identity: request.identity,
            id: request.id,
        })
    }
    fn slot(&mut self, token: Token) -> Result<&mut Slot<B>, Error> {
        self.slots
            .iter_mut()
            .flatten()
            .find(|s| s.request.identity == token.identity && s.request.id == token.id)
            .ok_or(Error::Stale)
    }
    pub fn dispatch(&mut self, now: u64) -> Result<Option<Dispatch>, Error> {
        self.expire(now);
        if !self.accepting {
            return Err(Error::Quarantined);
        }
        // Backends share one ordering domain; no concurrent access to a BOT LUN.
        if self
            .slots
            .iter()
            .flatten()
            .any(|s| s.phase == Phase::Submitted)
        {
            return Ok(None);
        }
        let Some(s) = self
            .slots
            .iter_mut()
            .flatten()
            .filter(|s| s.phase == Phase::Queued)
            .min_by_key(|s| s.request.id)
        else {
            return Ok(None);
        };
        let physical = if s.length == 0 {
            s.request.range
        } else {
            Region {
                start: self
                    .geometry
                    .region
                    .physical_lba(s.request.range.start, s.request.range.blocks)?,
                blocks: s.request.range.blocks,
            }
        };
        s.phase = Phase::Submitted;
        if s.request.operation == Operation::Write {
            self.dirty = true;
        }
        Ok(Some(Dispatch {
            token: Token {
                identity: s.request.identity,
                id: s.request.id,
            },
            operation: s.request.operation,
            physical,
            bytes: s.length,
        }))
    }
    pub fn copy_write(&mut self, token: Token, offset: usize, out: &mut [u8]) -> Result<(), Error> {
        let s = self.slot(token)?;
        if s.phase != Phase::Submitted || s.request.operation != Operation::Write {
            return Err(Error::Permission);
        }
        let end = offset.checked_add(out.len()).ok_or(Error::Length)?;
        if end > s.length {
            return Err(Error::Length);
        }
        out.copy_from_slice(&s.bytes[offset..end]);
        Ok(())
    }
    pub fn copy_read(&mut self, token: Token, offset: usize, bytes: &[u8]) -> Result<(), Error> {
        let s = self.slot(token)?;
        if s.phase != Phase::Submitted
            || s.request.operation != Operation::Read
            || offset != s.received
        {
            return Err(Error::Stale);
        }
        let end = offset.checked_add(bytes.len()).ok_or(Error::Length)?;
        if end > s.length {
            return Err(Error::Length);
        }
        s.bytes[offset..end].copy_from_slice(bytes);
        s.received = end;
        Ok(())
    }
    /// A successful flush means the device acknowledged its persistence barrier;
    /// it is not a claim about host caches, power protection, or physical media.
    pub fn complete(&mut self, token: Token, result: Result<(), Error>) -> Result<(), Error> {
        let cache = self.geometry.capabilities.volatile_write_cache;
        let ram = self.geometry.backend == block::BackendKind::RamEphemeral;
        let s = self.slot(token)?;
        if s.phase != Phase::Submitted {
            return Err(Error::Stale);
        }
        let result =
            if result.is_ok() && s.request.operation == Operation::Read && s.received != s.length {
                Err(Error::Length)
            } else {
                result
            };
        let mutation = matches!(s.request.operation, Operation::Write | Operation::Flush);
        let uncertain = mutation && result.is_err();
        let flush = s.request.operation == Operation::Flush && result.is_ok();
        s.outcome = Some(match result {
            Err(_) if mutation => Outcome::UnknownCommit,
            Err(e) => Outcome::Failed(e),
            Ok(()) => Outcome::Complete(if ram {
                Durability::Volatile
            } else if !cache || flush {
                Durability::Stable
            } else {
                Durability::DeviceCache
            }),
        });
        s.phase = if uncertain {
            Phase::Uncertain
        } else {
            Phase::Done
        };
        if uncertain {
            self.accepting = false;
        }
        if flush {
            self.dirty = false;
        }
        Ok(())
    }
    pub fn cancel(&mut self, token: Token) -> Result<(), Error> {
        let s = self.slot(token)?;
        match s.phase {
            Phase::Queued => {
                s.phase = Phase::Done;
                s.outcome = Some(Outcome::Failed(Error::Cancelled));
            }
            Phase::Submitted => {
                s.phase = Phase::Uncertain;
                s.outcome = Some(
                    if matches!(s.request.operation, Operation::Write | Operation::Flush) {
                        Outcome::UnknownCommit
                    } else {
                        Outcome::Failed(Error::Cancelled)
                    },
                );
                self.accepting = false;
            }
            _ => return Err(Error::Stale),
        }
        Ok(())
    }
    pub fn expire(&mut self, now: u64) {
        for s in self.slots.iter_mut().flatten() {
            if now < s.deadline || !matches!(s.phase, Phase::Queued | Phase::Submitted) {
                continue;
            }
            let submitted = s.phase == Phase::Submitted;
            s.outcome = Some(
                if submitted && matches!(s.request.operation, Operation::Write | Operation::Flush) {
                    Outcome::UnknownCommit
                } else {
                    Outcome::Failed(Error::Timeout)
                },
            );
            s.phase = if submitted {
                Phase::Uncertain
            } else {
                Phase::Done
            };
            if submitted {
                self.accepting = false;
            }
        }
    }
    pub fn lost(&mut self) {
        self.accepting = false;
        for s in self.slots.iter_mut().flatten() {
            if s.phase == Phase::Done {
                continue;
            }
            let submitted = matches!(s.phase, Phase::Submitted | Phase::Uncertain);
            s.outcome = Some(
                if submitted && matches!(s.request.operation, Operation::Write | Operation::Flush) {
                    Outcome::UnknownCommit
                } else {
                    Outcome::Failed(Error::Removed)
                },
            );
            s.phase = if submitted {
                Phase::Uncertain
            } else {
                Phase::Done
            };
        }
    }
    pub fn outcome(&mut self, token: Token) -> Result<Option<Outcome>, Error> {
        Ok(self.slot(token)?.outcome)
    }
    pub fn take(&mut self, token: Token, out: &mut [u8]) -> Result<Outcome, Error> {
        let s = self.slot(token)?;
        let outcome = s.outcome.ok_or(Error::NotReady)?;
        if s.phase == Phase::Uncertain {
            return Ok(outcome);
        } // retain until owner proves transport stopped
        let count =
            if s.request.operation == Operation::Read && matches!(outcome, Outcome::Complete(_)) {
                s.length
            } else {
                0
            };
        if out.len() != count {
            return Err(Error::Length);
        }
        out.copy_from_slice(&s.bytes[..count]);
        let slot = self
            .slots
            .iter_mut()
            .find(|s| s.as_ref().is_some_and(|s| s.request.id == token.id))
            .ok_or(Error::Stale)?;
        *slot = None;
        Ok(outcome)
    }
    pub fn quiesce(&mut self) {
        self.accepting = false;
    }
    pub fn pending(&self) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.phase != Phase::Done)
            .count()
    }
    pub fn dirty(&self) -> bool {
        self.dirty
    }
    /// Trusted transport owner only: completion/cancel or a process exit does
    /// not prove this. Retire this queue; a new attachment gets a new identity.
    pub fn transport_stopped(&mut self) {
        self.accepting = false;
        self.slots.fill_with(|| None);
    }
}
