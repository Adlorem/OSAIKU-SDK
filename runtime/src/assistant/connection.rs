//! Readiness is separate from model installation and conversation text. A retry
//! requests capabilities only; it never resends a question or changes a model.
use super::{op, Packet, Status, BYTES};
use crate::process::ProcessEndpoint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum State {
    Unknown,
    Preparing,
    Ready,
    Busy,
    Retry,
    Reload,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub model: u64,
    pub state: State,
    pub reason: Status,
    pub elapsed_ms: u64,
    pub attempts: u64,
}
impl Record {
    pub const EMPTY: Self = Self {
        model: 0,
        state: State::Unknown,
        reason: Status::Unavailable,
        elapsed_ms: 0,
        attempts: 0,
    };
    pub fn encode(self) -> [u8; 40] {
        let mut out = [0; 40];
        for (bytes, value) in out.chunks_exact_mut(8).zip([
            self.model,
            self.state as u64,
            self.reason as u64,
            self.elapsed_ms,
            self.attempts,
        ]) {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        out
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 40 {
            return None;
        }
        let w = |i: usize| u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        let state = match w(1) {
            1 => State::Preparing,
            2 => State::Ready,
            3 => State::Busy,
            4 => State::Retry,
            5 => State::Reload,
            _ => return None,
        };
        if w(2) > Status::StartupTimeout as u64 || w(0) == 0 {
            return None;
        }
        let reason = Packet {
            value: w(2),
            ..Packet::default()
        }
        .status()?;
        if !matches!(
            (state, reason),
            (State::Preparing, Status::Preparing)
                | (State::Ready, Status::Ok)
                | (State::Busy, Status::Busy)
                | (State::Retry, Status::StartupTimeout)
                | (State::Reload, _)
        ) {
            return None;
        }
        Some(Self {
            model: w(0),
            state,
            reason,
            elapsed_ms: w(3),
            attempts: w(4),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub server: ProcessEndpoint,
    pub session: u64,
    pub window: u64,
    pub incarnation: u64,
    pub model: u64,
}
#[derive(Clone, Copy)]
struct Pending {
    packet: Packet,
    until: u64,
    sent: bool,
}
pub struct Client {
    scope: Option<Scope>,
    pending: Option<Pending>,
    sequence: u64,
    next: u64,
    last: u64,
    retry: bool,
    renewed: Option<Scope>,
    failure: Option<super::failure::Record>,
    pub record: Record,
    pub status: Status,
    revision: u64,
}
impl Client {
    fn already_renewed(&self, scope: Scope) -> bool {
        self.renewed.is_some_and(|old| {
            old.server == scope.server
                && old.model == scope.model
                && old.window == scope.window
                && old.incarnation == scope.incarnation
        })
    }
    pub fn wake_after_ms(&self, now: u64) -> Option<u64> {
        match self.pending {
            Some(p) if !p.sent => Some(1),
            Some(p) if self.revision == 0 => Some(p.until.saturating_sub(now).max(1)),
            Some(_) => None,
            None if self.scope.is_none() => Some(1),
            None if self.status == Status::Stale
                && self.scope.is_some_and(|s| !self.already_renewed(s)) =>
            {
                Some(self.next.saturating_sub(now).max(1))
            }
            _ => None,
        }
    }
    pub const EMPTY: Self = Self {
        scope: None,
        pending: None,
        sequence: 0,
        next: 0,
        last: 0,
        retry: false,
        renewed: None,
        failure: None,
        record: Record::EMPTY,
        status: Status::Unavailable,
        revision: 0,
    };
    pub fn retry_available(&self) -> bool {
        self.status == Status::Ok && self.record.state == State::Retry
    }
    pub fn service_lost(&mut self) {
        self.pending = None;
        self.record.state = State::Reload;
        self.record.reason = Status::Unavailable;
        self.status = Status::Unavailable;
    }
    /// Explicit session renewal carries no previous question. The caller must
    /// leave active requests alone and perform only a NEW handshake.
    pub fn session_retry_available(&self) -> bool {
        self.pending.is_none()
            && self.scope.is_some_and(|s| s.model != 0)
            && (matches!(self.status, Status::Stale | Status::ServiceTimeout)
                || self.status == Status::Unavailable && self.record.state == State::Reload)
    }
    /// Controller activation replaces the fallback broker. Renew only after an
    /// authenticated Stale reply, once per model/server/window incarnation and
    /// only while idle. A lost reply or a question failure is not permission to
    /// replay anything. NEW carries no prompt, history or source authority.
    pub fn renew_session(&mut self, conversation: &mut super::Client, now: u64) -> bool {
        let Some(scope) = self.scope else {
            return false;
        };
        if self.already_renewed(scope)
            || self.status != Status::Stale
            || !self.session_retry_available()
            || conversation.active()
            || conversation.endpoint() != scope.server
            || conversation.session != scope.session
            || now < self.last
            || now < self.next
        {
            return false;
        }
        self.renewed = Some(scope);
        if let Err(status) = conversation.reset(scope.window, scope.incarnation) {
            conversation.status = status;
        }
        self.record = Record::EMPTY;
        self.status = Status::Unavailable;
        true
    }
    pub fn failure_report(&self) -> Option<super::failure::Record> {
        self.failure
    }
    pub fn clock_lost(&mut self) -> bool {
        let changed = self.status != Status::ClockFailure;
        self.pending = None;
        self.retry = false;
        self.failure = None;
        self.status = Status::ClockFailure;
        self.record = Record::EMPTY;
        changed
    }
    pub fn reconnect(&mut self) {
        if self.retry_available() {
            self.pending = None;
            self.revision = 0;
            self.retry = true;
            self.next = 0;
        }
    }
    /// Subscribe once per scope, with a complete initial snapshot. A successful
    /// command is never replayed and healthy subscriptions have no timer.
    pub fn tick(
        &mut self,
        scope: Scope,
        now: u64,
        idle: bool,
        mut send: impl FnMut(ProcessEndpoint, &[u8]) -> bool,
    ) -> bool {
        if now < self.last || self.status == Status::ClockFailure {
            return self.clock_lost();
        }
        self.last = now;
        let mut changed = false;
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.pending = None;
            self.revision = 0;
            self.retry = false;
            self.failure = None;
            self.record = Record::EMPTY;
            self.status = Status::Unavailable;
            self.next = 0;
            changed = true;
        }
        if scope.model == 0
            || !scope.server.names_instance()
            || scope.session == 0
            || scope.window == 0
            || scope.incarnation == 0
        {
            return changed;
        }
        if self.revision == 0 && self.pending.is_some_and(|p| now >= p.until) {
            self.pending = None;
            self.status = Status::ServiceTimeout;
            self.record = Record::EMPTY;
            self.next = now.saturating_add(1000);
            return true;
        }
        if self.pending.is_none()
            && idle
            && now >= self.next
            && (self.retry
                || self.status == Status::Unavailable && self.record.state == State::Unknown)
        {
            let Some(sequence) = self.sequence.checked_add(1) else {
                self.status = Status::Limit;
                return true;
            };
            self.sequence = sequence;
            self.pending = Some(Pending {
                packet: Packet {
                    operation: op::CONNECTION,
                    session: scope.session,
                    request: sequence,
                    window: scope.window,
                    incarnation: scope.incarnation,
                    offset: u64::from(self.retry),
                    value: scope.model,
                },
                until: now.saturating_add(2000),
                sent: false,
            });
            self.retry = false;
        }
        if let Some(p) = &mut self.pending {
            if !p.sent && idle {
                let mut bytes = [0; BYTES];
                if let Some(n) = p.packet.encode(&[], &mut bytes) {
                    p.sent = send(scope.server, &bytes[..n]);
                }
            }
        }
        changed
    }
    pub fn receive(&mut self, sender: ProcessEndpoint, bytes: &[u8], now: u64) -> bool {
        let (Some(scope), Some(pending)) = (self.scope, self.pending) else {
            return false;
        };
        let Some((p, body)) = Packet::decode(bytes) else {
            return false;
        };
        if sender != scope.server || !pending.sent || !p.response_to(pending.packet) {
            return false;
        }
        if now < self.last || self.revision == 0 && now >= pending.until || p.expires().is_some() {
            return false;
        }
        let Some(status) = p.status() else {
            return false;
        };
        let record = if status == Status::Ok {
            if body.len() != 48 {
                return false;
            }
            let revision = u64::from_le_bytes(body[40..48].try_into().unwrap());
            if revision <= self.revision {
                return false;
            }
            let Some(r) = Record::decode(&body[..40]) else {
                return false;
            };
            if r.model != scope.model {
                return false;
            }
            // Each event is a complete snapshot; a revision gap is repaired
            // by this snapshot, without replaying the conversation prompt.
            self.revision = revision;
            r
        } else {
            if !body.is_empty() {
                return false;
            }
            Record::EMPTY
        };
        if status != Status::Ok {
            self.pending = None;
        }
        if record.state == State::Retry {
            if self.failure.is_none() {
                self.failure = Some(super::failure::Record {
                    session: scope.session,
                    request: pending.packet.request,
                    window: scope.window,
                    incarnation: scope.incarnation,
                    status: Status::StartupTimeout,
                    elapsed_ms: record.elapsed_ms,
                    operation: op::CONNECTION,
                });
            }
        } else if status == Status::Ok {
            self.failure = None;
        }
        self.record = record;
        self.status = status;
        self.last = now;
        self.next = now.saturating_add(1000);
        true
    }
}
