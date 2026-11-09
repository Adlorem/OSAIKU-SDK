//! Consumer-side L2 batches. No packet parser receives a shared RX reference.
//! One consumer instance owns all four cursors and both private slot ledgers.
use crate::{
    network::*,
    network_ring::{self, Ring},
    protocol::Error,
};

pub struct Consumer<const N: usize> {
    geometry: Geometry,
    rx: Ring,
    rx_return: Ring,
    tx: Ring,
    tx_done: Ring,
    rx_owner: Ownership<N>,
    tx_owner: Ownership<N>,
    returns: Returns<N>,
    rx_data: *const u8,
    tx_data: *mut u8,
    tx_available: [bool; N],
    tx_lengths: [u32; N],
    pub counters: Counters,
    closed: bool,
    errors: u8,
}
impl<const N: usize> Consumer<N> {
    /// # Safety
    /// All rings and data mappings belong to the same admitted generation.
    /// RX covers geometry.data_bytes() R+NX, TX covers the same size RW+NX.
    /// They are disjoint from metadata and descriptors. The caller must stop
    /// using this object before kernel unmap; it must not expose TX to another
    /// local writer while a slot is pending. RX can still be written by DMA.
    pub unsafe fn mapped(
        g: Geometry,
        rx: Ring,
        rx_return: Ring,
        tx: Ring,
        tx_done: Ring,
        rx_data: *const u8,
        tx_data: *mut u8,
    ) -> Result<Self, Error> {
        g.validate()?;
        if !rx.matches(g.capacity, false)
            || !rx_return.matches(g.capacity, true)
            || !tx.matches(g.capacity, true)
            || !tx_done.matches(g.capacity, false)
        {
            return Err(Error::State);
        }
        if N < g.slots as usize
            || rx_data.is_null()
            || tx_data.is_null()
            || (rx_data as usize | tx_data as usize) & 4095 != 0
        {
            return Err(Error::Range);
        }
        Ok(Self {
            geometry: g,
            rx,
            rx_return,
            tx,
            tx_done,
            rx_owner: Ownership::new(g, false)?,
            tx_owner: Ownership::new(g, true)?,
            returns: Returns::new(g.slots as usize)?,
            rx_data,
            tx_data,
            tx_available: [true; N],
            tx_lengths: [0; N],
            counters: Counters::default(),
            closed: false,
            errors: 0,
        })
    }
    fn live(&self) -> Result<(), Error> {
        if self.closed {
            Err(Error::Stale)
        } else {
            Ok(())
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.rx_owner.close();
        self.tx_owner.close();
        self.tx_available.fill(false);
        // Do not touch mappings here: close may follow a revocation notification.
    }
    fn invalid(&mut self, e: Error) -> Error {
        self.counters.invalid = self.counters.invalid.saturating_add(1);
        self.errors = self.errors.saturating_add(1);
        if self.errors >= 2 {
            self.close();
        }
        e
    }
    /// New TX work uses backpressure. No ownership is given up unless a ring
    /// entry can be published; the device cannot read any private app page.
    pub fn transmit(&mut self, bytes: &[u8]) -> Result<bool, Error> {
        self.live()?;
        if bytes.len() < 14 || bytes.len() > self.geometry.mtu as usize + 18 {
            return Err(Error::Size);
        }
        let slot = self.tx_available[..self.geometry.slots as usize]
            .iter()
            .position(|v| *v)
            .ok_or(Error::Backpressure)?;
        if self.tx.available().map_err(|e| self.invalid(e))? == 0 {
            self.counters.full = self.counters.full.saturating_add(1);
            return Err(Error::Backpressure);
        }
        let e = Entry {
            slot: slot as u32,
            generation: self.geometry.generation,
            length: bytes.len() as u32,
            status: COMPLETED,
        };
        let offset = self.geometry.offset(e.slot)? as usize;
        for (i, b) in bytes.iter().enumerate() {
            unsafe {
                core::ptr::write_volatile(self.tx_data.add(offset + i), *b);
            }
        }
        // Publish can still reject a malicious peer index; keep our credit in
        // that case and never claim a TX completion or allocator release.
        let wake = self.tx.push(e).map_err(|e| self.invalid(e))?;
        self.tx_owner.publish(e)?;
        self.tx_available[slot] = false;
        self.tx_lengths[slot] = e.length;
        Ok(wake)
    }
    /// Drain at most the negotiated ring capacity. notify is called even if a
    /// later entry in the batch fails, so previously published work is visible.
    pub fn poll(
        &mut self,
        now: u64,
        return_timeout: u64,
        scratch: &mut [u8],
        mut receive: impl FnMut(&[u8]),
        mut notify: impl FnMut(),
    ) -> Result<usize, Error> {
        self.live()?;
        if scratch.len() < self.geometry.slot_bytes as usize {
            return Err(Error::Size);
        }
        if self.returns.expired(now) {
            self.close();
            return Err(Error::Deadline);
        }
        let mut wake = false;
        let mut packets = 0;
        let result = (|| {
            for _ in 0..self.geometry.capacity {
                if self.tx_done.available()? == 0 {
                    break;
                }
                let (bytes, hint) = self.tx_done.pop()?;
                wake |= hint;
                let e = Entry::decode(bytes, self.geometry, RingKind::TxDone)?;
                let expected = self.tx_lengths[e.slot as usize];
                if e.length > expected || (e.status == COMPLETED && e.length != expected) {
                    return Err(Error::Range);
                }
                self.tx_owner.receive(e)?;
                self.tx_available[e.slot as usize] = true;
                self.tx_lengths[e.slot as usize] = 0;
            }
            for _ in 0..self.geometry.capacity {
                if self.rx.available()? == 0 {
                    break;
                }
                let (bytes, hint) = self.rx.pop()?;
                wake |= hint;
                let e = Entry::decode(bytes, self.geometry, RingKind::RxDone)?;
                self.rx_owner.receive(e)?;
                let n = unsafe { network_ring::copy_rx(self.rx_data, self.geometry, e, scratch) }?;
                // Parser sees only private bytes. Even malformed traffic must
                // return its slot after the callback; parsing cannot pin DMA.
                if e.status == COMPLETED {
                    receive(&scratch[..n]);
                    packets += 1;
                    self.counters.packets = self.counters.packets.saturating_add(1);
                    self.counters.bytes = self.counters.bytes.saturating_add(n as u64);
                } else {
                    self.counters.dropped = self.counters.dropped.saturating_add(1);
                }
                let returned = Entry {
                    length: 0,
                    status: COMPLETED,
                    ..e
                };
                self.returns.push(returned, now, return_timeout)?;
                self.rx_owner.queue_return(returned)?;
            }
            for _ in 0..self.geometry.capacity {
                let Some(e) = self.returns.front() else {
                    break;
                };
                if self.rx_return.available()? == 0 {
                    break;
                }
                wake |= self.rx_return.push(e)?;
                self.rx_owner.returned(e)?;
                self.returns.published()?;
            }
            Ok(packets)
        })();
        if wake {
            notify();
        }
        result.map_err(|e| self.invalid(e))
    }
    /// Arm all directions that can unblock this consumer. The caller waits
    /// using the last sequence returned by wait_event, then always disarms.
    pub fn arm(&mut self) -> Result<bool, Error> {
        self.live()?;
        let result = (|| {
            let rx = self.rx.arm()?;
            let tx = self.tx_done.arm()?;
            let space = if self.returns.front().is_some() {
                self.rx_return.arm()?
            } else {
                true
            };
            Ok(rx && tx && space)
        })();
        if result != Ok(true) {
            self.disarm();
        }
        result
    }
    pub fn disarm(&mut self) {
        if self.closed {
            return;
        }
        self.rx.disarm();
        self.tx_done.disarm();
        self.rx_return.disarm();
    }
}
