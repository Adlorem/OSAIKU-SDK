//! Inert model proposals and owner-side action receipts. No model bytes grant consent.
use osaiku_ui::Identity;
pub const SYSCALL: u64 = 385;
pub const REPORT_APP: u64 = 8;
pub const ELEMENT: u32 = osaiku_ui::symbol("agent-open-report");
pub const ARM: u64 = 1;
pub const STATUS: u64 = 2;
pub const TAKE: u64 = 3;
pub const FINISH: u64 = 4;
pub const CLEAR: u64 = 5;
pub const START: u64 = 6;
pub const PROPOSED: u64 = 1;
pub const CONFIRMED: u64 = 2;
pub const EXECUTING: u64 = 3;
pub const STARTED: u64 = 4;
pub const OPENED: u64 = 5;
pub const REFUSED: u64 = 6;
pub const STALE: u64 = 7;
pub const UNKNOWN: u64 = 8;
pub const ISSUED: u64 = 9;
pub const CLOSED: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Composition {
    pub size_order: bool,
    pub descending: bool,
    pub category: Option<usize>,
    pub chart: bool,
    pub table: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proposal {
    OpenReport,
    Report(Composition),
}
impl Proposal {
    /// Admission for a complete service-validated answer. Parsing alone never
    /// establishes provenance or consent; executors still require a receipt.
    pub fn from_answer(
        status: crate::assistant::Status,
        e: crate::assistant::evidence::Evidence,
        text: &str,
    ) -> Option<Self> {
        use crate::assistant::{evidence::*, Status};
        if status != Status::Complete || !e.canonical() || e.flags != MODEL | UNSOURCED {
            return None;
        }
        Self::parse(text)
    }
    /// Parse one COMPLETE response only. Exact ASCII grammar, no suffix, code,
    /// nested content, paths, arbitrary app IDs, commands or chained operations.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > 64 {
            return None;
        }
        if text == "OSAIPROP1 OPEN 8" {
            return Some(Self::OpenReport);
        }
        let mut words = text.split(' ');
        if words.next()? != "OSAIPROP1" || words.next()? != "REPORT" {
            return None;
        }
        let size_order = match words.next()? {
            "size" => true,
            "name" => false,
            _ => return None,
        };
        let descending = match words.next()? {
            "desc" => true,
            "asc" => false,
            _ => return None,
        };
        let category = match words.next()? {
            "all" => None,
            "0" => Some(0),
            "1" => Some(1),
            "2" => Some(2),
            "3" => Some(3),
            "4" => Some(4),
            "5" => Some(5),
            _ => return None,
        };
        let (chart, table) = match words.next()? {
            "both" => (true, true),
            "chart" => (true, false),
            "table" => (false, true),
            _ => return None,
        };
        if words.next().is_some() {
            return None;
        }
        Some(Self::Report(Composition {
            size_order,
            descending,
            category,
            chart,
            table,
        }))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub target: Identity,
    pub request: u64,
    pub state: u64,
    pub endpoint: u64,
    pub until_ms: u64,
    pub focus: u64,
}
impl Record {
    pub const EMPTY: Self = Self {
        target: osaiku_ui::Action::EMPTY.target,
        request: 0,
        state: 0,
        endpoint: 0,
        until_ms: 0,
        focus: 0,
    };
}
pub const RECORD_BYTES: usize = 80;
const _: () = assert!(core::mem::size_of::<Record>() == RECORD_BYTES);

/// START returns the lifecycle owner's consumed sequence even when launch
/// fails. A refusal before calling that owner leaves the supplied value intact.
#[repr(C)]
pub struct Start {
    pub receipt: Record,
    pub sequence: u64,
}
pub const START_BYTES: usize = 88;
const _: () = assert!(core::mem::size_of::<Start>() == START_BYTES);

/// Bounded receipt retention. Unknown execution is never made retryable by a
/// timeout. Only the lifecycle owner can finish a taken request.
pub struct Ledger {
    records: [Record; 8],
    last_now: u64,
}
impl Ledger {
    pub fn refuse_unissued(&mut self, request: Record) -> bool {
        let Some(r) = self.records.iter_mut().find(|r| {
            r.target == request.target
                && r.request == request.request
                && r.focus == request.focus
                && r.state == EXECUTING
        }) else {
            return false;
        };
        r.state = REFUSED;
        true
    }
    pub fn uncertain(&mut self, request: Record) {
        if let Some(r) = self.records.iter_mut().find(|r| {
            r.target == request.target
                && r.request == request.request
                && r.focus == request.focus
                && r.state == ISSUED
        }) {
            r.state = UNKNOWN;
        }
    }
    pub const EMPTY: Self = Self {
        records: [Record::EMPTY; 8],
        last_now: 0,
    };
    pub fn expire(&mut self, now: u64) {
        for r in &mut self.records {
            if r.state != 0 && (now < self.last_now || now >= r.until_ms) {
                r.state = match r.state {
                    PROPOSED | CONFIRMED => STALE,
                    EXECUTING | ISSUED | STARTED => UNKNOWN,
                    other => other,
                };
            }
        }
        self.last_now = now;
    }
    pub fn arm(&mut self, target: Identity, request: u64, now: u64, focus: u64) -> Option<Record> {
        self.expire(now);
        if target.owner == 0
            || target.window == 0
            || target.incarnation == 0
            || target.epoch == 0
            || target.revision == 0
            || request == 0
        {
            return None;
        }
        if let Some(r) = self
            .records
            .iter()
            .find(|r| r.state != 0 && r.target.owner == target.owner)
        {
            return (r.request == request && r.target == target && r.focus == focus).then_some(*r);
        }
        let until_ms = now.checked_add(30_000)?;
        let r = self.records.iter_mut().find(|r| r.state == 0)?;
        *r = Record {
            target,
            request,
            state: PROPOSED,
            endpoint: 0,
            until_ms,
            focus,
        };
        Some(*r)
    }
    pub fn confirm(&mut self, target: Identity, now: u64, focus: u64) -> bool {
        self.expire(now);
        let Some(r) = self
            .records
            .iter_mut()
            .find(|r| r.target == target && r.state == PROPOSED)
        else {
            return false;
        };
        if r.focus != focus {
            r.state = STALE;
            return false;
        }
        r.state = CONFIRMED;
        true
    }
    pub fn take(&mut self, now: u64, mut valid: impl FnMut(Record) -> bool) -> Option<Record> {
        self.expire(now);
        for r in &mut self.records {
            if r.state != CONFIRMED {
                continue;
            }
            if !valid(*r) {
                r.state = STALE;
                continue;
            }
            r.state = EXECUTING;
            return Some(*r);
        }
        None
    }
    pub fn revalidate(&mut self, owner: u64, mut valid: impl FnMut(Record) -> bool) {
        for r in &mut self.records {
            if r.target.owner == owner && matches!(r.state, PROPOSED | CONFIRMED) && !valid(*r) {
                r.state = STALE;
            }
        }
    }
    pub fn finish(&mut self, receipt: Record) -> bool {
        let Some(r) = self.records.iter_mut().find(|r| {
            r.request == receipt.request
                && r.target == receipt.target
                && r.focus == receipt.focus
                && matches!(r.state, EXECUTING | ISSUED | STARTED | UNKNOWN)
        }) else {
            return false;
        };
        if receipt.state != REFUSED
            && (receipt.state != STARTED
                || !crate::process::ProcessEndpoint::decode(receipt.endpoint).names_instance())
        {
            return false;
        }
        if receipt.state == REFUSED && receipt.endpoint != 0 {
            return false;
        }
        r.state = receipt.state;
        r.endpoint = receipt.endpoint;
        true
    }
    pub fn issue(&mut self, request: Record, now: u64, valid: bool) -> bool {
        self.expire(now);
        let Some(r) = self.records.iter_mut().find(|r| {
            r.request == request.request
                && r.target == request.target
                && r.focus == request.focus
                && r.state == EXECUTING
        }) else {
            return false;
        };
        r.state = if valid { ISSUED } else { STALE };
        valid
    }
    pub fn status(
        &mut self,
        owner: u64,
        now: u64,
        mut live: impl FnMut(u64) -> Option<bool>,
    ) -> Option<Record> {
        self.expire(now);
        let r = self
            .records
            .iter_mut()
            .find(|r| r.state != 0 && r.target.owner == owner)?;
        if r.endpoint != 0 && matches!(r.state, STARTED | OPENED | UNKNOWN) {
            r.state = match live(r.endpoint) {
                Some(true) => OPENED,
                None => CLOSED,
                Some(false) => r.state,
            };
        }
        Some(*r)
    }
    pub fn clear(&mut self, owner: u64) -> bool {
        let Some(r) = self
            .records
            .iter_mut()
            .find(|r| r.state != 0 && r.target.owner == owner)
        else {
            return true;
        };
        if matches!(r.state, CONFIRMED | EXECUTING | ISSUED | STARTED | UNKNOWN) {
            return false;
        }
        *r = Record::EMPTY;
        true
    }
    pub fn close(&mut self, owner: u64, window: u64, incarnation: u64) {
        for r in &mut self.records {
            if r.target.owner == owner
                && r.target.window == window
                && r.target.incarnation == incarnation
            {
                *r = Record::EMPTY;
            }
        }
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn transition(operation: u64, record: &mut Record) -> isize {
    unsafe {
        crate::raw::syscall(
            SYSCALL,
            [
                operation,
                record as *mut Record as u64,
                core::mem::size_of::<Record>() as u64,
                0,
                0,
            ],
        ) as isize
    }
}
