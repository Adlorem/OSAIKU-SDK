//! Bounded conversation transport. Routing authority comes from the live window
//! and kernel endpoint, never from these bytes or a model-supplied function name.
pub mod connection;
pub mod evidence;
pub mod execution;
pub mod failure;
/// V2: CANCEL acceptance is separate from the terminal result and preserves text.
pub const MAGIC: u64 = 0x324e4f434941534f;
pub const BYTES: usize = 160;
pub const HEADER: usize = 64;
pub const PAYLOAD: usize = BYTES - HEADER;
pub const TEXT: usize = 1024;
/// Finite command/setup budget. Interactive OBSERVE has no overall time limit;
/// independent transport, computation and cleanup watchdogs remain in force.
pub const REQUEST_MS: u64 = 300_000;
/// Owned copy of a live registry entry; constructing this value grants nothing.
pub struct WindowScope {
    pub owner: crate::process::ProcessEndpoint,
    pub app: u64,
    pub window: u64,
    pub incarnation: u64,
    pub live: bool,
}
impl WindowScope {
    pub fn allows(
        &self,
        sender: crate::process::ProcessEndpoint,
        target: crate::process::ProcessEndpoint,
        p: Packet,
    ) -> bool {
        use crate::process::ProcessId;
        if !self.live
            || self.app != 9
            || !sender.names_instance()
            || !target.names_instance()
            || self.window != p.window
            || self.incarnation != p.incarnation
        {
            return false;
        }
        if p.operation == op::REPLY {
            sender.process_id == ProcessId(4) && target == self.owner
        } else {
            target.process_id == ProcessId(4) && sender == self.owner
        }
    }
}
pub mod op {
    pub const NEW: u64 = 1;
    pub const INPUT: u64 = 2;
    pub const NEXT: u64 = 3;
    pub const CANCEL: u64 = 4;
    pub const RESOURCES: u64 = 5;
    pub const REPLY: u64 = 6;
    pub const EVIDENCE: u64 = 7;
    pub const DIAGNOSTICS: u64 = 8;
    pub const CONNECTION: u64 = 9;
    /// One output credit, deferred until data or a terminal state is available.
    pub const OBSERVE: u64 = 10;
}
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    Pending = 1,
    Complete = 2,
    Cancelled = 3,
    Unavailable = 4,
    Invalid = 5,
    Stale = 6,
    Limit = 7,
    Busy = 8,
    Refused = 9,
    Expired = 10,
    OutOfMemory = 11,
    RequestTimeout = 12,
    ServiceTimeout = 13,
    ClockFailure = 14,
    NoProgress = 15,
    Preparing = 16,
    StartupTimeout = 17,
    CancelAccepted = 18,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Packet {
    pub operation: u64,
    pub session: u64,
    pub request: u64,
    pub window: u64,
    pub incarnation: u64,
    pub offset: u64,
    pub value: u64,
}
pub fn candidate(bytes: &[u8]) -> bool {
    bytes.get(..8).is_some_and(|b| b == MAGIC.to_le_bytes())
}
impl Packet {
    pub fn decode(bytes: &[u8]) -> Option<(Self, &[u8])> {
        if !(HEADER..=BYTES).contains(&bytes.len()) || !candidate(bytes) {
            return None;
        }
        let word = |i: usize| u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        let p = Self {
            operation: word(1),
            session: word(2),
            request: word(3),
            window: word(4),
            incarnation: word(5),
            offset: word(6),
            value: word(7),
        };
        if !(op::NEW..=op::OBSERVE).contains(&p.operation)
            || p.session == 0
            || p.request == 0
            || p.window == 0
            || p.incarnation == 0
        {
            return None;
        }
        Some((p, &bytes[HEADER..]))
    }
    pub fn encode(self, payload: &[u8], out: &mut [u8; BYTES]) -> Option<usize> {
        if payload.len() > PAYLOAD {
            return None;
        }
        out.fill(0);
        for (b, w) in out.chunks_exact_mut(8).zip([
            MAGIC,
            self.operation,
            self.session,
            self.request,
            self.window,
            self.incarnation,
            self.offset,
            self.value,
        ]) {
            b.copy_from_slice(&w.to_le_bytes());
        }
        out[HEADER..HEADER + payload.len()].copy_from_slice(payload);
        Self::decode(&out[..HEADER + payload.len()])?;
        Some(HEADER + payload.len())
    }
    pub fn reply(self, status: Status) -> Self {
        Self {
            operation: op::REPLY,
            value: status as u64 | (self.operation << 8),
            ..self
        }
    }
    pub fn status(self) -> Option<Status> {
        Some(match self.value & 255 {
            0 => Status::Ok,
            1 => Status::Pending,
            2 => Status::Complete,
            3 => Status::Cancelled,
            4 => Status::Unavailable,
            5 => Status::Invalid,
            6 => Status::Stale,
            7 => Status::Limit,
            8 => Status::Busy,
            9 => Status::Refused,
            10 => Status::Expired,
            11 => Status::OutOfMemory,
            12 => Status::RequestTimeout,
            13 => Status::ServiceTimeout,
            14 => Status::ClockFailure,
            15 => Status::NoProgress,
            16 => Status::Preparing,
            17 => Status::StartupTimeout,
            18 => Status::CancelAccepted,
            _ => return None,
        })
    }
    pub fn response_to(self, request: Self) -> bool {
        self.operation == op::REPLY
            && ((self.value >> 8) & 255) == request.operation
            && self.session == request.session
            && self.request == request.request
            && self.window == request.window
            && self.incarnation == request.incarnation
            && self.offset == request.offset
    }
    pub fn expires(self) -> Option<u64> {
        let t = self.value >> 16;
        (t != 0).then_some(t)
    }
}

/// UTF-8 storage shared by the app and broker; dropping/clearing scrubs bytes.
pub struct Text {
    bytes: [u8; TEXT],
    len: usize,
}
impl Text {
    pub const EMPTY: Self = Self {
        bytes: [0; TEXT],
        len: 0,
    };
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
    pub fn clear(&mut self) {
        for b in &mut self.bytes {
            unsafe { core::ptr::write_volatile(b, 0) }
        }
        self.len = 0;
    }
    pub fn append(&mut self, s: &str) -> Result<(), Status> {
        if self.len + s.len() > TEXT {
            return Err(Status::Limit);
        }
        self.bytes[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
    pub fn pop(&mut self) {
        if let Some((i, _)) = self.as_str().char_indices().last() {
            self.bytes[i..self.len].fill(0);
            self.len = i;
        }
    }
    pub fn chunk(&self, offset: usize, max: usize) -> Option<&str> {
        let s = self.as_str();
        if !s.is_char_boundary(offset) {
            return None;
        }
        let mut end = offset.saturating_add(max).min(s.len());
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        Some(&s[offset..end])
    }
}
impl core::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.append(s).map_err(|_| core::fmt::Error)
    }
}
impl Drop for Text {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Transport stage, not an invented percentage or a compute-progress claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Idle,
    Connecting,
    Sending,
    Reading,
    Waiting,
    Receiving,
    Verifying,
    Cancelling,
}

/// A full mailbox has not accepted the message. Other failures never authorize
/// replay of a message whose delivery may be unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    Full,
    Unavailable,
}
pub fn send_result(status: isize) -> Result<(), SendError> {
    match status {
        crate::abi::SYSCALL_OK => Ok(()),
        crate::abi::SYSCALL_ERR_MAILBOX_FULL => Err(SendError::Full),
        _ => Err(SendError::Unavailable),
    }
}
struct Cleanup {
    packet: Packet,
    until: u64,
    sent: bool,
}

/// One outstanding exchange, bounded UTF-8 chunks, no retries after an unknown
/// delivery outcome. Call tick at most once per display interval (50 ms).
pub struct Client {
    pub evidence: evidence::Evidence,
    pub input: Text,
    pub output: Text,
    pub status: Status,
    pub session: u64,
    request: u64,
    window: u64,
    incarnation: u64,
    phase: u64,
    offset: usize,
    waiting: Option<Packet>,
    sent: usize,
    deadline: u64,
    send_until: Option<u64>,
    until: u64,
    last_now: u64,
    backend: u64,
    clock_failed: bool,
    expiry: Option<u64>,
    after_connect: u64,
    started: Option<u64>,
    elapsed: u64,
    failure_operation: u64,
    cleanup: Option<Cleanup>,
    stopping: bool,
}
impl Client {
    /// Next concrete transport/source deadline. A parked output credit has no
    /// timer: the dispatcher wakes the window when its response is available.
    pub fn wake_after_ms(&self, now: u64) -> Option<u64> {
        let deadline = if let Some(cleanup) = &self.cleanup {
            Some(if cleanup.sent {
                cleanup.until
            } else {
                now.saturating_add(1)
            })
        } else if let Some(packet) = self.waiting {
            (packet.operation != op::OBSERVE).then_some(self.deadline)
        } else if self.phase != 0 {
            Some(now.saturating_add(1))
        } else {
            self.expiry
        };
        deadline.map(|until| until.saturating_sub(now).max(1))
    }
    pub const EMPTY: Self = Self {
        evidence: evidence::Evidence::EMPTY,
        input: Text::EMPTY,
        output: Text::EMPTY,
        status: Status::Ok,
        session: 0,
        request: 0,
        window: 0,
        incarnation: 0,
        phase: 0,
        offset: 0,
        waiting: None,
        sent: 0,
        deadline: 0,
        send_until: None,
        until: 0,
        last_now: 0,
        backend: 0,
        clock_failed: false,
        expiry: None,
        after_connect: 0,
        started: None,
        elapsed: 0,
        failure_operation: 0,
        cleanup: None,
        stopping: false,
    };
    pub fn active(&self) -> bool {
        self.phase != 0 || self.cleanup.is_some()
    }
    /// Resolve only the initial handshake by service ID. Private turns and
    /// cancellation stay bound to the instance that acknowledged that handshake.
    pub fn endpoint(&self) -> crate::process::ProcessEndpoint {
        if self.backend == 0 {
            crate::process::ProcessEndpoint::unbound(crate::process::ProcessId(4))
        } else {
            crate::process::ProcessEndpoint::decode(self.backend)
        }
    }
    pub fn reset(&mut self, window: u64, incarnation: u64) -> Result<(), Status> {
        if self.cleanup.is_some() {
            return Err(Status::Busy);
        }
        if self.clock_failed {
            return Err(Status::ClockFailure);
        }
        self.evidence = evidence::Evidence::EMPTY;
        self.stopping = false;
        self.session = self.session.checked_add(1).ok_or(Status::Limit)?;
        self.request = self.request.checked_add(1).ok_or(Status::Limit)?;
        self.backend = 0;
        self.window = window;
        self.incarnation = incarnation;
        self.input.clear();
        self.output.clear();
        self.waiting = None;
        self.send_until = None;
        self.expiry = None;
        self.phase = op::NEW;
        self.offset = 0;
        self.status = Status::Pending;
        self.until = 0;
        self.after_connect = 0;
        self.started = None;
        self.elapsed = 0;
        Ok(())
    }
    pub fn begin(&mut self, prompt: &str, resources: bool) -> Result<(), Status> {
        if self.clock_failed {
            return Err(Status::ClockFailure);
        }
        if self.active() || self.session == 0 {
            return Err(Status::Busy);
        }
        // A session number is allocated before NEW is acknowledged. A timeout,
        // refusal or failed send must not allow private traffic to the unbound
        // service ID; the caller must explicitly establish a new handshake.
        if self.backend == 0 {
            return Err(Status::Unavailable);
        }
        if !resources
            && (prompt.is_empty()
                || prompt.len() > TEXT
                || prompt
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\t')))
        {
            return Err(Status::Invalid);
        }
        // A new explicit request may renew an expired conversation, but never
        // replay the old prompt or accept private traffic before NEW is ACKed.
        let reconnect = matches!(
            self.status,
            Status::Stale | Status::RequestTimeout | Status::ServiceTimeout
        );
        if reconnect {
            self.reset(self.window, self.incarnation)?;
        }
        self.request = self.request.checked_add(1).ok_or(Status::Limit)?;
        self.input.clear();
        self.stopping = false;
        self.output.clear();
        self.evidence = evidence::Evidence::EMPTY;
        self.expiry = None;
        if !resources {
            self.input.append(prompt)?;
        }
        self.phase = if resources { op::RESOURCES } else { op::INPUT };
        if reconnect {
            self.after_connect = self.phase;
            self.phase = op::NEW;
        }
        self.offset = 0;
        self.status = Status::Pending;
        self.until = 0;
        self.started = None;
        self.elapsed = 0;
        Ok(())
    }
    /// Explicit read-only diagnostic intent, with the normal window/session gate.
    pub fn begin_diagnostics(&mut self) -> Result<(), Status> {
        self.begin("", true)?;
        if self.after_connect != 0 {
            self.after_connect = op::DIAGNOSTICS;
        } else {
            self.phase = op::DIAGNOSTICS;
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        if self.cleanup.is_some() || self.stopping {
            return;
        }
        if self.active() && self.phase != op::CANCEL {
            self.stopping = true;
            self.phase = op::CANCEL;
            self.waiting = None;
            self.send_until = None;
            self.offset = 0;
            self.input.clear();
            self.expiry = None;
            self.until = 0;
        }
    }
    /// Local terminal state on loss of the clock; repeated polls stay quiet.
    pub fn clock_lost(&mut self) -> bool {
        self.clock_failed = true;
        if self.status == Status::ClockFailure && !self.active() {
            return false;
        }
        self.fail(Status::ClockFailure);
        true
    }
    /// Called only after a kernel lifecycle notice or failed registration of
    /// this exact endpoint. Accepted text remains historical data.
    pub fn service_lost(&mut self) {
        self.fail(Status::Unavailable);
    }
    /// One bounded send attempt during retirement, followed by private scrubbing.
    /// This does not claim that cancellation was acknowledged by the backend.
    pub fn close(&mut self, now: Option<u64>, send: impl FnMut(&[u8]) -> Result<(), SendError>) {
        self.cancel();
        if let Some(now) = now {
            self.tick(now, send);
        }
        self.fail(Status::Cancelled);
    }
    /// Authenticated endpoint must remain the same throughout this conversation.
    /// A replacement requires an explicit new conversation, never prompt replay.
    pub fn receive_from(
        &mut self,
        sender: crate::process::ProcessEndpoint,
        bytes: &[u8],
        now: u64,
    ) -> bool {
        if sender.process_id != crate::process::ProcessId(4) || !sender.names_instance() {
            return false;
        }
        let Some((p, payload)) = Packet::decode(bytes) else {
            return false;
        };
        if let Some(cleanup) = self.cleanup.as_ref() {
            if sender.encode() != self.backend || !cleanup.sent || !p.response_to(cleanup.packet) {
                return false;
            }
            if now < self.last_now {
                return self.clock_lost();
            }
            // A cleanup receipt ends this bounded exchange, not the original
            // failure or a promise that the model is ready for a new request.
            if now < cleanup.until && payload.is_empty() && p.status().is_some() {
                self.cleanup = None;
                self.last_now = now;
                return true;
            }
            return false;
        }
        let Some(q) = self.waiting.filter(|q| p.response_to(*q)) else {
            return false;
        };
        if self.backend != 0 && self.backend != sender.encode() {
            self.fail(Status::Stale);
            return true;
        }
        let consumed = self.receive(bytes, now);
        if consumed
            && self.backend == 0
            && q.operation == op::NEW
            && p.status() == Some(Status::Ok)
            && matches!(self.status, Status::Ok | Status::Pending)
        {
            self.backend = sender.encode();
        }
        consumed
    }
    pub fn progress(&self) -> Progress {
        if self.cleanup.is_some() || self.stopping && self.active() {
            return Progress::Cancelling;
        }
        match self.phase {
            op::NEW => Progress::Connecting,
            op::INPUT => Progress::Sending,
            op::RESOURCES | op::DIAGNOSTICS => Progress::Reading,
            op::OBSERVE if self.output.as_str().is_empty() => Progress::Waiting,
            op::OBSERVE => Progress::Receiving,
            op::EVIDENCE => Progress::Verifying,
            op::CANCEL => Progress::Cancelling,
            _ => Progress::Idle,
        }
    }
    /// Last observed elapsed time, retained at failure; no invented work estimate.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed
    }
    /// Numeric local observation; not proof that the model or kernel failed.
    pub fn failure_report(&self) -> Option<failure::Record> {
        let r = failure::Record {
            session: self.session,
            request: self.request,
            window: self.window,
            incarnation: self.incarnation,
            status: self.status,
            elapsed_ms: self.elapsed,
            operation: self.failure_operation,
        };
        r.valid().then_some(r)
    }
    fn fail(&mut self, status: Status) {
        self.cleanup = None;
        self.evidence.flags |= match status {
            Status::Expired | Status::Stale | Status::ClockFailure => evidence::STALE,
            Status::Limit => evidence::TRUNCATED,
            Status::Invalid => evidence::CONFLICT,
            _ => evidence::MISSING,
        };
        self.status = status;
        self.failure_operation = self.phase;
        self.phase = 0;
        self.after_connect = 0;
        self.waiting = None;
        self.send_until = None;
        self.input.clear();
        self.expiry = None;
    }
    fn transport_failed(&mut self, status: Status, now: u64) {
        let accepted_input = self.backend != 0
            && (matches!(self.phase, op::OBSERVE | op::EVIDENCE)
                || self.phase == op::INPUT && (self.offset != 0 || self.waiting.is_some()));
        let cleanup = accepted_input.then_some(Cleanup {
            packet: Packet {
                operation: op::CANCEL,
                session: self.session,
                request: self.request,
                window: self.window,
                incarnation: self.incarnation,
                offset: 0,
                value: 0,
            },
            until: now.saturating_add(2_000),
            sent: false,
        });
        self.fail(status);
        self.cleanup = cleanup;
    }
    /// Only receive_from may enter after checking the authenticated endpoint.
    fn receive(&mut self, bytes: &[u8], now: u64) -> bool {
        let Some((p, payload)) = Packet::decode(bytes) else {
            return false;
        };
        let Some(q) = self.waiting else {
            return false;
        };
        if !p.response_to(q) {
            return false;
        }
        if now < self.last_now {
            return self.clock_lost();
        }
        self.observe_elapsed(now);
        if !matches!(self.phase, op::OBSERVE | op::EVIDENCE) && now >= self.until {
            self.transport_failed(Status::RequestTimeout, now);
            return true;
        }
        if q.operation != op::OBSERVE && now >= self.deadline {
            self.transport_failed(Status::ServiceTimeout, now);
            return true;
        }
        if p.expires().is_some_and(|t| now >= t) {
            self.fail(Status::Expired);
            return true;
        }
        self.last_now = now;
        let Some(status) = p.status() else {
            self.fail(Status::Invalid);
            return true;
        };
        if !matches!(q.operation, op::OBSERVE | op::EVIDENCE)
            && (!payload.is_empty() || p.expires().is_some())
        {
            self.fail(Status::Invalid);
            return true;
        }
        self.waiting = None;
        match q.operation {
            op::EVIDENCE if status == Status::Complete => {
                if p.expires().is_some() {
                    self.fail(Status::Invalid);
                    return true;
                }
                let Some(e) =
                    evidence::Evidence::decode(payload).filter(|e| e.context == q.request)
                else {
                    self.fail(Status::Invalid);
                    return true;
                };
                self.evidence = e;
                if e.flags & evidence::SOURCE != 0 && now >= e.expires {
                    self.fail(Status::Expired);
                    return true;
                }
                if self.expiry != (e.expires != 0).then_some(e.expires)
                    || !e.references(self.output.as_str(), now)
                {
                    self.fail(Status::Invalid);
                    return true;
                }
                self.phase = 0;
                self.status = Status::Complete;
                self.input.clear();
            }
            op::NEW if status == Status::Ok => {
                self.phase = core::mem::take(&mut self.after_connect);
                self.status = if self.phase == 0 {
                    Status::Ok
                } else {
                    Status::Pending
                };
                if self.phase != 0 {
                    let Some(next) = self.request.checked_add(1) else {
                        self.fail(Status::Limit);
                        return true;
                    };
                    self.request = next;
                }
            }
            op::INPUT if matches!(status, Status::Ok | Status::Pending) => {
                self.offset += self.sent;
                if self.offset == self.input.as_str().len() && status == Status::Pending {
                    self.phase = op::OBSERVE;
                    self.offset = 0;
                } else if self.offset >= self.input.as_str().len() || status != Status::Ok {
                    self.fail(Status::Invalid);
                }
            }
            op::RESOURCES | op::DIAGNOSTICS if status == Status::Complete => {
                self.phase = op::OBSERVE;
                self.offset = 0;
            }
            op::OBSERVE if matches!(status, Status::Pending | Status::Complete) => {
                let valid = core::str::from_utf8(payload).ok().filter(|s| {
                    !s.chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
                });
                if valid.is_none_or(|s| self.output.append(s).is_err()) {
                    self.fail(Status::Invalid);
                    return true;
                }
                if self.expiry.is_some() && p.expires() != self.expiry {
                    self.fail(Status::Invalid);
                    return true;
                }
                self.expiry = p.expires();
                self.offset = self.output.as_str().len();
                if status == Status::Complete && payload.is_empty() {
                    self.phase = op::EVIDENCE;
                    self.offset = 0;
                }
            }
            op::CANCEL if status == Status::CancelAccepted => {
                self.phase = op::OBSERVE;
                self.offset = self.output.as_str().len();
            }
            op::CANCEL if status == Status::Cancelled => self.fail(Status::Cancelled),
            _ => self.fail(
                if matches!(status, Status::Ok | Status::Pending | Status::Complete) {
                    Status::Invalid
                } else {
                    status
                },
            ),
        }
        true
    }
    pub fn tick(&mut self, now: u64, mut send: impl FnMut(&[u8]) -> Result<(), SendError>) -> bool {
        if self.clock_failed {
            return false;
        }
        if now < self.last_now {
            return self.clock_lost();
        }
        self.last_now = now;
        if let Some(cleanup) = self.cleanup.as_mut() {
            if now >= cleanup.until {
                self.cleanup = None;
                return true;
            }
            if !cleanup.sent {
                let mut bytes = [0; BYTES];
                let n = cleanup.packet.encode(&[], &mut bytes).unwrap();
                match send(&bytes[..n]) {
                    Ok(()) => cleanup.sent = true,
                    Err(SendError::Full) => {}
                    Err(SendError::Unavailable) => self.cleanup = None,
                }
            }
            return false;
        }
        if self.expiry.is_some_and(|t| now >= t) {
            self.fail(Status::Expired);
            return true;
        }
        if !self.active() {
            return false;
        }
        if self.until == 0 {
            self.until = now.saturating_add(REQUEST_MS);
        }
        self.observe_elapsed(now);
        if !matches!(self.phase, op::OBSERVE | op::EVIDENCE) && now >= self.until {
            self.transport_failed(Status::RequestTimeout, now);
            return true;
        }
        if self.waiting.is_some_and(|p| p.operation != op::OBSERVE) && now >= self.deadline {
            self.transport_failed(Status::ServiceTimeout, now);
            return true;
        }
        if self.waiting.is_some() {
            return false;
        }
        // A rejected send is safe to retry on a later turn, but cannot extend
        // either the exchange deadline or the absolute request deadline.
        let until = *self.send_until.get_or_insert(now.saturating_add(2_000));
        if now >= until {
            self.transport_failed(Status::ServiceTimeout, now);
            return true;
        }
        let payload = if self.phase == op::INPUT {
            self.input
                .chunk(self.offset, PAYLOAD)
                .unwrap_or("")
                .as_bytes()
        } else {
            &[]
        };
        let p = Packet {
            operation: self.phase,
            session: self.session,
            request: self.request,
            window: self.window,
            incarnation: self.incarnation,
            offset: self.offset as u64,
            value: if self.phase == op::INPUT {
                self.input.as_str().len() as u64
            } else {
                0
            },
        };
        let mut bytes = [0; BYTES];
        self.sent = payload.len();
        let result = p
            .encode(payload, &mut bytes)
            .ok_or(SendError::Unavailable)
            .and_then(|n| send(&bytes[..n]));
        for b in &mut bytes {
            unsafe { core::ptr::write_volatile(b, 0) }
        }
        match result {
            Err(SendError::Full) => return false,
            Err(SendError::Unavailable) => {
                self.transport_failed(Status::Unavailable, now);
                return true;
            }
            Ok(()) => {}
        }
        self.send_until = None;
        self.waiting = Some(p);
        self.deadline = now.saturating_add(2_000);
        false
    }
    fn observe_elapsed(&mut self, now: u64) {
        let started = *self.started.get_or_insert(now);
        self.elapsed = now.saturating_sub(started);
    }
}

/// Explicit user intents only. Generated text never passes through this gate.
pub fn resource_question(prompt: &str) -> bool {
    [
        "ile mam ram?",
        "ile pamięci jest dostępne?",
        "jakie jest obciążenie systemu?",
        "pokaż zasoby systemu",
    ]
    .iter()
    .any(|q| prompt.trim().eq_ignore_ascii_case(q))
}

/// Exact user intent only; never inspect model output for this command.
pub fn diagnostic_question(prompt: &str) -> bool {
    [
        "/diagnostics",
        "explain recent errors",
        "wyjaśnij ostatnie błędy",
    ]
    .iter()
    .any(|q| prompt.trim().eq_ignore_ascii_case(q))
}
