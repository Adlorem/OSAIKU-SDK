//! Bounded stop-and-wait storage client. A save becomes visible only at COMMIT.
//! Reopening pins both mount and object version; mixed chunks are never returned.
use super::*;
use crate::files::{operation as op, status, FileReply, FileRequest};
use crate::report::metadata::Scope;
use core::sync::atomic::{AtomicU64, Ordering};
static SEQUENCE: AtomicU64 = AtomicU64::new(1);
pub const CANCELED: u64 = 100;
pub const INVALID_ARTIFACT: u64 = 101;
pub fn name(slot: u64) -> Option<[u8; 17]> {
    if slot >= 16 {
        return None;
    }
    let mut n = *b"report-1.osa-def1";
    n[7] = b'1' + (slot / 2) as u8;
    if slot % 2 == 1 {
        n[13..].copy_from_slice(b"res1");
    }
    Some(n)
}
fn exchange(
    r: &mut FileRequest,
    call: &mut impl FnMut(&FileRequest) -> FileReply,
) -> Result<FileReply, u64> {
    r.sequence = SEQUENCE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .map_err(|_| status::NO_SPACE)?;
    let p = call(r);
    if p.version != crate::files::VERSION
        || p.sequence != r.sequence
        || p.content_len > 256
        || p.name_len > 64
        || p.content[p.content_len as usize..].iter().any(|b| *b != 0)
    {
        return Err(status::INVALID);
    }
    if !matches!(p.status, status::OK | status::END) {
        return Err(p.status);
    }
    if r.mount_generation != 0 && p.mount_generation != r.mount_generation {
        return Err(status::STALE);
    }
    Ok(p)
}
/// Use from a worker; no application/session pointers are retained by storage.
pub fn run(
    scope: Scope,
    slot: u64,
    save: Option<usize>,
    bytes: &mut [u8],
    mut canceled: impl FnMut() -> bool,
    mut call: impl FnMut(&FileRequest) -> FileReply,
) -> Result<usize, u64> {
    if save.is_none() {
        bytes.fill(0);
    }
    if name(slot).is_none() {
        return Err(status::INVALID);
    }
    if let Some(n) = save {
        let (h, _) =
            Header::parse(bytes.get(..n).ok_or(INVALID_ARTIFACT)?).map_err(|_| INVALID_ARTIFACT)?;
        if h.kind != slot % 2 + 1 {
            return Err(INVALID_ARTIFACT);
        }
    }
    let mut r = FileRequest {
        operation: op::REPORT_INFO,
        window_id: scope.window,
        window_generation: scope.incarnation,
        parent: slot,
        ..FileRequest::EMPTY
    };
    if canceled() {
        return Err(CANCELED);
    }
    let info = exchange(&mut r, &mut call)?;
    if info.mount_generation == 0
        || (info.status == status::END && (info.object != 0 || info.object_version != 0))
        || (info.status == status::OK
            && (info.object == 0 || info.object_version == 0 || info.kind != 1))
    {
        return Err(status::INVALID);
    }
    r.mount_generation = info.mount_generation;
    r.object = info.object;
    r.expected_version = info.object_version;
    if let Some(n) = save {
        r.operation = op::REPORT_BEGIN;
        r.catalog_revision = n as u64;
        let begin = exchange(&mut r, &mut call)?;
        if begin.status != status::OK || begin.object == 0 {
            return Err(status::INVALID);
        }
        r.object = begin.object;
        let result = (|| {
            for (i, part) in bytes[..n].chunks(256).enumerate() {
                if canceled() {
                    return Err(CANCELED);
                }
                r.operation = op::REPORT_CHUNK;
                r.catalog_revision = (i * 256) as u64;
                r.content.fill(0);
                r.content[..part.len()].copy_from_slice(part);
                r.content_len = part.len() as u64;
                if exchange(&mut r, &mut call)?.status != status::OK {
                    return Err(status::INVALID);
                }
            }
            if canceled() {
                return Err(CANCELED);
            }
            r.operation = op::REPORT_COMMIT;
            r.content.fill(0);
            r.content_len = 0;
            r.catalog_revision = 0;
            let committed = exchange(&mut r, &mut call).map_err(|e| {
                if matches!(e, status::UNAVAILABLE | status::INVALID) {
                    status::UNKNOWN_COMMIT
                } else {
                    e
                }
            })?;
            if committed.status != status::OK
                || committed.object == 0
                || committed.object_version == 0
                || committed.kind != 1
                || committed.capacity != n as u64
            {
                return Err(status::UNKNOWN_COMMIT);
            }
            // Cancellation after the durable point cannot turn success into a retry.
            Ok(n)
        })();
        if result.is_err() {
            r.operation = op::REPORT_ABORT;
            r.content.fill(0);
            r.content_len = 0;
            r.catalog_revision = 0;
            let _ = exchange(&mut r, &mut call);
        }
        result
    } else {
        bytes.fill(0);
        let result = (|| {
            if info.status == status::END {
                return Err(status::END);
            }
            let n = usize::try_from(info.capacity).map_err(|_| status::INVALID)?;
            if info.kind != 1
                || info.object == 0
                || info.object_version == 0
                || !(HEADER + 1..=MAX_BYTES).contains(&n)
                || n > bytes.len()
            {
                return Err(INVALID_ARTIFACT);
            }
            r.operation = op::REPORT_READ;
            let mut offset = 0;
            while offset < n {
                if canceled() {
                    return Err(CANCELED);
                }
                r.catalog_revision = offset as u64;
                let p = exchange(&mut r, &mut call)?;
                let count = (n - offset).min(256);
                if p.status != status::OK
                    || p.mount_generation != info.mount_generation
                    || p.object != info.object
                    || p.object_version != info.object_version
                    || p.capacity != info.capacity
                    || p.content_len != count as u64
                {
                    return Err(status::STALE);
                }
                bytes[offset..offset + count].copy_from_slice(&p.content[..count]);
                offset += count;
            }
            let (h, _) = Header::parse(&bytes[..n]).map_err(|_| INVALID_ARTIFACT)?;
            if h.kind != slot % 2 + 1 {
                return Err(INVALID_ARTIFACT);
            }
            if canceled() {
                return Err(CANCELED);
            }
            Ok(n)
        })();
        if result.is_err() {
            bytes.fill(0);
        }
        result
    }
}
