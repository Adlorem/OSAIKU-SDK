//! Mapped ring access. Never forms Rust references to mutable peer payloads.
use crate::{network::*, protocol::Error};
use core::sync::atomic::{fence, AtomicU32, Ordering};

/// Pointers remain pinned by the caller's channel mapping. Index and waiting
/// flags each occupy their own 64-byte line on their sole writer's page.
pub struct Ring {
    entries: *mut AtomicU32,
    own: *mut AtomicU32,
    peer: *const AtomicU32,
    waiting: *mut AtomicU32,
    peer_waiting: *const AtomicU32,
    cursor: Cursor,
    producer: bool,
}
impl Ring {
    pub fn matches(&self, capacity: u32, producer: bool) -> bool {
        self.cursor.matches(capacity, producer)
    }
    /// # Safety
    /// All mappings cover `capacity * 32` bytes plus four separate cache lines,
    /// remain live throughout use, and have rights per NET-00b.1 §3.2. Entries
    /// and own index are writable only for a producer; own consumer index and
    /// waiting are writable for a consumer. No two local Ring values may write
    /// the same index. Revoke must stop access before unmapping these pointers.
    pub unsafe fn mapped(
        entries: *mut u8,
        own: *mut u32,
        peer: *const u32,
        waiting: *mut u32,
        peer_waiting: *const u32,
        capacity: u32,
        producer: bool,
    ) -> Result<Self, Error> {
        if entries.is_null()
            || entries as usize & 4095 != 0
            || [
                own as usize,
                peer as usize,
                waiting as usize,
                peer_waiting as usize,
            ]
            .iter()
            .any(|p| *p == 0 || *p & 63 != 0)
        {
            return Err(Error::Range);
        }
        Ok(Self {
            entries: entries.cast(),
            own: own.cast(),
            peer: peer.cast(),
            waiting: waiting.cast(),
            peer_waiting: peer_waiting.cast(),
            cursor: Cursor::new(capacity, producer)?,
            producer,
        })
    }
    pub fn available(&mut self) -> Result<u32, Error> {
        let peer = u32::from_le(unsafe { (*self.peer).load(Ordering::Acquire) });
        self.cursor.available(peer)
    }
    /// Returns whether the peer requested a wake. Caller coalesces these hints
    /// into one notify per batch, including after an early batch error.
    pub fn push(&mut self, e: Entry) -> Result<bool, Error> {
        if !self.producer {
            return Err(Error::Permission);
        }
        if self.available()? == 0 {
            return Err(Error::Backpressure);
        }
        let bytes = e.encode();
        let slot = self.cursor.index() * 8;
        for i in 0..8 {
            unsafe {
                (*self.entries.add(slot + i)).store(
                    u32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()),
                    Ordering::Relaxed,
                );
            }
        }
        let next = self.cursor.advance();
        unsafe {
            (*self.own).store(next.to_le(), Ordering::Release);
        }
        fence(Ordering::SeqCst);
        Ok(unsafe { (*self.peer_waiting).load(Ordering::Relaxed) } != 0)
    }
    /// Copy the entire untrusted entry before validation. Atomic word reads
    /// remain defined if a malicious peer rewrites a published entry.
    pub fn pop(&mut self) -> Result<([u8; ENTRY_BYTES], bool), Error> {
        if self.producer {
            return Err(Error::Permission);
        }
        if self.available()? == 0 {
            return Err(Error::NotReady);
        }
        let mut bytes = [0; ENTRY_BYTES];
        let slot = self.cursor.index() * 8;
        for i in 0..8 {
            let word = unsafe { (*self.entries.add(slot + i)).load(Ordering::Relaxed) };
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_ne_bytes());
        }
        let next = self.cursor.advance();
        unsafe {
            (*self.own).store(next.to_le(), Ordering::Release);
        }
        fence(Ordering::SeqCst);
        Ok((
            bytes,
            unsafe { (*self.peer_waiting).load(Ordering::Relaxed) } != 0,
        ))
    }
    /// true means the caller can enter wait_event with its PREVIOUS last_seen.
    /// Call disarm after every wait return, including stale/deadline errors.
    pub fn arm(&mut self) -> Result<bool, Error> {
        unsafe {
            (*self.waiting).store(1u32.to_le(), Ordering::Relaxed);
        }
        fence(Ordering::SeqCst);
        match self.available() {
            Ok(0) => Ok(true),
            Ok(_) => {
                self.disarm();
                Ok(false)
            }
            Err(e) => {
                self.disarm();
                Err(e)
            }
        }
    }
    pub fn disarm(&mut self) {
        unsafe {
            (*self.waiting).store(0, Ordering::Relaxed);
        }
    }
}

/// Copy RX into private memory before any protocol parser sees it. A device
/// may continue writing its RX pages; lengths/offsets come only from snapshot.
/// # Safety
/// base maps the pinned RX arena, at least geometry.data_bytes() bytes, R+NX.
pub unsafe fn copy_rx(
    base: *const u8,
    g: Geometry,
    e: Entry,
    out: &mut [u8],
) -> Result<usize, Error> {
    g.validate()?;
    let e = Entry::decode(e.encode(), g, RingKind::RxDone)?;
    if out.len() < e.length as usize {
        return Err(Error::Size);
    }
    let offset = g.offset(e.slot)? as usize;
    for (i, b) in out[..e.length as usize].iter_mut().enumerate() {
        *b = unsafe { core::ptr::read_volatile(base.add(offset + i)) };
    }
    Ok(e.length as usize)
}
