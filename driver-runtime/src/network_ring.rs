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
        let next = self.stage(e);
        Ok(self.publish(next))
    }
    // The peer cannot consume these entries until publish releases the cursor.
    fn stage(&mut self, e: Entry) -> u32 {
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
        self.cursor.advance()
    }
    fn publish(&self, next: u32) -> bool {
        unsafe {
            (*self.own).store(next.to_le(), Ordering::Release);
        }
        fence(Ordering::SeqCst);
        unsafe { (*self.peer_waiting).load(Ordering::Relaxed) != 0 }
    }
    /// Publish a bounded FIFO prefix of private credits. One release and
    /// wait-flag check cover the entire prefix, also on a later ledger error.
    pub(crate) fn publish_returns<const N: usize>(
        &mut self,
        pending: &mut Returns<N>,
        owner: &mut Ownership<N>,
        wake: &mut bool,
    ) -> Result<(), Error> {
        if !self.producer {
            return Err(Error::Permission);
        }
        if pending.front().is_none() {
            return Ok(());
        }
        let capacity = self.available()?;
        let mut last = None;
        let result = (|| {
            for _ in 0..capacity {
                let Some(entry) = pending.front() else {
                    break;
                };
                owner.returned(entry)?;
                pending.published()?;
                last = Some(self.stage(entry));
            }
            Ok(())
        })();
        if let Some(next) = last {
            *wake |= self.publish(next);
        }
        result
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
        let (bytes, next) = self.take_entry();
        Ok((bytes, self.publish(next)))
    }

    /// Consume the bounded prefix visible at entry. Publish once, including
    /// the descriptor that caused a callback error, as for individual pops.
    pub(crate) fn drain(
        &mut self,
        wake: &mut bool,
        mut consume: impl FnMut([u8; ENTRY_BYTES]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.producer {
            return Err(Error::Permission);
        }
        let mut remaining = self.available()?;
        while remaining != 0 {
            let (bytes, next) = self.take_entry();
            remaining -= 1;
            if remaining == 0 {
                *wake |= self.publish(next);
                return consume(bytes);
            }
            if let Err(error) = consume(bytes) {
                *wake |= self.publish(next);
                return Err(error);
            }
        }
        Ok(())
    }

    fn take_entry(&mut self) -> ([u8; ENTRY_BYTES], u32) {
        let mut bytes = [0; ENTRY_BYTES];
        let slot = self.cursor.index() * 8;
        for i in 0..8 {
            let word = unsafe { (*self.entries.add(slot + i)).load(Ordering::Relaxed) };
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_ne_bytes());
        }
        (bytes, self.cursor.advance())
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
/// base maps the pinned RX data RAM, at least geometry.data_bytes() bytes,
/// R+NX. Device registers must never be passed as an RX arena.
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
    let mut source = unsafe { base.add(g.offset(e.slot)? as usize) };
    let mut rest = &mut out[..e.length as usize];
    // Each read stays within the validated frame. Aligned word reads reduce
    // CPU load instructions while preserving a fully private packet snapshot.
    #[cfg(target_arch = "x86_64")]
    if source.cast::<u64>().is_aligned() {
        let (words, tail) = rest.as_chunks_mut::<8>();
        for word in words {
            *word = unsafe { source.cast::<u64>().read_volatile() }.to_ne_bytes();
            source = unsafe { source.add(8) };
        }
        rest = tail;
    }
    for b in rest {
        *b = unsafe { source.read_volatile() };
        source = unsafe { source.add(1) };
    }
    Ok(e.length as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rx_snapshots_keep_exact_lengths_and_private_tails() {
        #[repr(align(4096))]
        struct Data([u8; 4097]);
        let mut source = Data(core::array::from_fn(|i| i.wrapping_mul(31) as u8));
        let original = source.0;
        let geometry = Geometry {
            generation: 1,
            capacity: 64,
            slots: 1,
            slot_bytes: 4096,
            mtu: 1500,
        };
        for skip in [0, 1] {
            for length in (14..=24).chain(1510..=1518) {
                source.0 = original;
                let entry = Entry {
                    length,
                    generation: 1,
                    ..Entry::default()
                };
                let mut copy = [0xcc; 1520];
                let n = unsafe {
                    copy_rx(source.0.as_ptr().add(skip), geometry, entry, &mut copy[1..])
                }
                .unwrap();
                source.0.fill(0);
                assert_eq!(n, length as usize);
                assert_eq!(&copy[1..1 + n], &original[skip..skip + n]);
                assert_eq!(copy[0], 0xcc);
                assert!(copy[1 + n..].iter().all(|b| *b == 0xcc));
            }
        }
    }

    #[test]
    fn publish_only_commits_and_wakes_for_the_valid_prefix_that_fits() {
        #[repr(align(4096))]
        struct Page([AtomicU32; 1024]);
        let entries = Page([const { AtomicU32::new(0) }; 1024]);
        let indices = Page([const { AtomicU32::new(0) }; 1024]);
        let line = |n: usize| indices.0.as_ptr().wrapping_add(16 * n).cast::<u32>();
        let mut ring = unsafe {
            Ring::mapped(
                entries.0.as_ptr().cast_mut().cast(),
                line(0).cast_mut(),
                line(1),
                line(2).cast_mut(),
                line(3),
                64,
                true,
            )
            .unwrap()
        };
        let geometry = Geometry {
            generation: 1,
            capacity: 64,
            slots: 64,
            slot_bytes: 4096,
            mtu: 1500,
        };
        let mut owner = Ownership::<64>::new(geometry, true).unwrap();
        let mut pending = Returns::<64>::new(64).unwrap();
        for _ in 0..62 {
            ring.push(Entry::default()).unwrap();
        }
        for slot in 0..4 {
            let e = Entry {
                slot,
                generation: 1,
                ..Entry::default()
            };
            if slot != 3 {
                owner.queue_return(e).unwrap();
            }
            pending.push(e, 1, 20).unwrap();
        }
        indices.0[48].store(1, Ordering::Relaxed);
        let mut wake = false;
        ring.publish_returns(&mut pending, &mut owner, &mut wake)
            .unwrap();
        assert!(wake);
        assert_eq!(indices.0[0].load(Ordering::Acquire), 64);
        assert_eq!(pending.front().unwrap().slot, 2);
        wake = false;
        ring.publish_returns(&mut pending, &mut owner, &mut wake)
            .unwrap();
        assert!(!wake);
        indices.0[16].store(2, Ordering::Release);
        assert_eq!(
            ring.publish_returns(&mut pending, &mut owner, &mut wake),
            Err(Error::State)
        );
        assert!(wake);
        assert_eq!(indices.0[0].load(Ordering::Acquire), 65);
        assert_eq!(entries.0[0].load(Ordering::Relaxed), 2);
        assert_eq!(pending.front().unwrap().slot, 3);
        assert!(pending.expired(21));
    }
}
