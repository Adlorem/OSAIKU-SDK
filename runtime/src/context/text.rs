//! Text-only model input from one owner-authorized retained publication. Every
//! consumption rechecks the token in the kernel; callers must not cache authority.
use super::*;
use osaiku_ui::surface::{DISABLED, FOCUSED};

/// A local correlation record; it does not claim that refused source data was
/// read, nor that an action was executed. No stale source digest is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub reason: system::Reason,
    pub evidence: system::Evidence,
}
pub fn consume_recorded_with<R>(
    expected: &Info,
    actor: u64,
    request_id: u64,
    out: &mut Snapshot,
    exchange: impl FnMut(&Request, &mut [u8]) -> isize,
    consume: impl FnOnce(Text<'_>) -> R,
) -> Result<R, Failure> {
    consume_with(expected, actor, request_id, out, exchange, consume).map_err(|error| {
        let reason = system::Reason::context(error);
        Failure {
            reason,
            evidence: system::Evidence {
                request_id,
                actor,
                operation: 2,
                object_kind: system::WINDOW,
                object: expected.handle.target.window,
                reason: reason as u32,
                decision: 2,
                ..system::Evidence::default()
            },
        }
    })
}

pub struct Text<'a> {
    snapshot: &'a Snapshot,
    actor: u64,
    request_id: u64,
}
pub struct Node<'a> {
    pub id: u32,
    pub parent: u32,
    pub role: u32,
    pub text: &'a str,
    pub state: u32,
    pub bounds: Rect,
    pub focused: bool,
    pub redacted: bool,
    /// Descriptive action ID only. Execution still needs separate live authority.
    pub offered_action: Option<u32>,
}
impl Text<'_> {
    pub fn info(&self) -> Info {
        self.snapshot.info
    }
    pub fn evidence(&self) -> system::Evidence {
        let info = self.info();
        system::Evidence {
            request_id: self.request_id,
            actor: self.actor,
            operation: 2,
            object_kind: system::WINDOW,
            object: info.handle.target.window,
            decision: 1,
            revision: info.handle.revision,
            digest: info.source_hash,
            ..system::Evidence::default()
        }
    }
    pub fn nodes(&self) -> impl Iterator<Item = Node<'_>> {
        self.snapshot.elements[..self.snapshot.count as usize]
            .iter()
            .map(|e| {
                let redacted = e.flags & REDACTED != 0;
                Node {
                    id: e.id,
                    parent: e.parent,
                    role: e.role,
                    text: core::str::from_utf8(
                        &self.snapshot.labels
                            [e.label_offset as usize..(e.label_offset + e.label_len) as usize],
                    )
                    .expect("validated snapshot"),
                    state: e.state,
                    bounds: e.bounds,
                    focused: e.state & FOCUSED != 0,
                    redacted,
                    offered_action: (e.action != 0 && e.state & DISABLED == 0 && !redacted)
                        .then_some(e.action),
                }
            })
    }
}
/// Runs `consume` only after a fresh kernel read and exact identity comparison.
/// No pixel request is made. The timestamp is unavailable; frame/revision/hash
/// identify a historical immutable publication, not necessarily the latest UI.
/// Closure, owner death, recipient death and explicit release revoke future reads.
pub fn consume_with<R>(
    expected: &Info,
    actor: u64,
    request_id: u64,
    out: &mut Snapshot,
    exchange: impl FnMut(&Request, &mut [u8]) -> isize,
    consume: impl FnOnce(Text<'_>) -> R,
) -> Result<R, Error> {
    bytes(out).fill(0);
    expected.validate().map_err(Error::Invalid)?;
    if request_id == 0 || (actor != expected.recipient && actor != expected.handle.target.owner) {
        return Err(Error::Invalid(Refusal::Permission));
    }
    semantics_with(expected.token, out, exchange)?;
    if out.info != *expected {
        bytes(out).fill(0);
        return Err(Error::Invalid(Refusal::Stale));
    }
    let result = consume(Text {
        snapshot: out,
        actor,
        request_id,
    });
    // Don't retain a reusable context inside the SDK staging buffer.
    bytes(out).fill(0);
    Ok(result)
}
