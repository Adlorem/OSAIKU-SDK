//! Driver-side L2 batches, independent of NIC register layout. The device
//! backend owns hardware descriptors; class metadata is never a DMA target.
use crate::{network::*, network_ring::Ring, protocol::Error};
pub trait Device {
    /// A completed RX slot still owned by the driver, with untrusted length
    /// already snapped from the device descriptor. Does not recycle the slot.
    fn receive(&mut self) -> Result<Option<(u32, u32)>, Error>;
    fn recycle_rx(&mut self, slot: u32) -> Result<(), Error>;
    /// Publishes a TX descriptor only after the required memory/MMIO barriers.
    /// On Err no descriptor was published; uncertain writes are completions.
    fn transmit(&mut self, slot: u32, bytes: u32) -> Result<(), Error>;
    /// Returned only after the NIC has finished reading this slot. This is a
    /// slot credit, never permission to return pinned memory to an allocator.
    fn complete_tx(&mut self) -> Result<Option<(u32, u32, u16)>, Error>;
}
pub struct Driver<D: Device, const N: usize> {
    device: D,
    geometry: Geometry,
    rx: Ring,
    rx_return: Ring,
    tx: Ring,
    tx_done: Ring,
    rx_owner: Ownership<N>,
    tx_owner: Ownership<N>,
    returns: Returns<N>,
    tx_lengths: [u32; N],
    pub counters: Counters,
    closed: bool,
    errors: u8,
}
impl<D: Device, const N: usize> Driver<D, N> {
    /// Access backend health/link state without changing class-slot ownership.
    pub fn device_mut(&mut self) -> &mut D {
        &mut self.device
    }
    /// Rings are prepared under the same trusted geometry as the device arena.
    pub fn new(
        device: D,
        g: Geometry,
        rx: Ring,
        rx_return: Ring,
        tx: Ring,
        tx_done: Ring,
    ) -> Result<Self, Error> {
        g.validate()?;
        if !rx.matches(g.capacity, true)
            || !rx_return.matches(g.capacity, false)
            || !tx.matches(g.capacity, false)
            || !tx_done.matches(g.capacity, true)
        {
            return Err(Error::State);
        }
        if N < g.slots as usize {
            return Err(Error::Size);
        }
        Ok(Self {
            device,
            geometry: g,
            rx,
            rx_return,
            tx,
            tx_done,
            rx_owner: Ownership::new(g, true)?,
            tx_owner: Ownership::new(g, false)?,
            returns: Returns::new(g.slots as usize)?,
            tx_lengths: [0; N],
            counters: Counters::default(),
            closed: false,
            errors: 0,
        })
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.rx_owner.close();
        self.tx_owner.close();
    }
    fn invalid(&mut self, e: Error) -> Error {
        self.counters.invalid = self.counters.invalid.saturating_add(1);
        self.errors = self.errors.saturating_add(1);
        if self.errors >= 2 {
            self.close();
        }
        e
    }
    fn entry(&self, slot: u32, length: u32, status: u16, kind: RingKind) -> Result<Entry, Error> {
        Entry::decode(
            Entry {
                slot,
                length,
                status,
                generation: self.geometry.generation,
            }
            .encode(),
            self.geometry,
            kind,
        )
    }
    pub fn poll(
        &mut self,
        now: u64,
        timeout: u64,
        mut notify: impl FnMut(),
    ) -> Result<usize, Error> {
        if self.closed {
            return Err(Error::Stale);
        }
        if self.returns.expired(now) {
            self.close();
            return Err(Error::Deadline);
        }
        let mut wake = false;
        let mut count = 0;
        let result = (|| {
            for _ in 0..self.geometry.capacity {
                if self.rx_return.available()? == 0 {
                    break;
                }
                let (b, hint) = self.rx_return.pop()?;
                wake |= hint;
                let e = Entry::decode(b, self.geometry, RingKind::RxReturn)?;
                self.rx_owner.receive(e)?;
                self.device.recycle_rx(e.slot)?;
            }
            for _ in 0..self.geometry.capacity {
                let Some((slot, length, status)) = self.device.complete_tx()? else {
                    break;
                };
                let e = self.entry(slot, length, status, RingKind::TxDone)?;
                let expected = self.tx_lengths[slot as usize];
                if expected == 0 || length > expected || (status == COMPLETED && length != expected)
                {
                    return Err(Error::Range);
                }
                self.tx_owner.queue_return(e)?;
                self.returns.push(e, now, timeout)?;
                self.tx_lengths[slot as usize] = 0;
            }
            for _ in 0..self.geometry.capacity {
                let Some(e) = self.returns.front() else {
                    break;
                };
                if self.tx_done.available()? == 0 {
                    break;
                }
                wake |= self.tx_done.push(e)?;
                self.tx_owner.returned(e)?;
                self.returns.published()?;
            }
            for _ in 0..self.geometry.capacity {
                if self.tx.available()? == 0 {
                    break;
                }
                let (b, hint) = self.tx.pop()?;
                wake |= hint;
                let e = Entry::decode(b, self.geometry, RingKind::TxSubmit)?;
                self.tx_owner.receive(e)?;
                match self.device.transmit(e.slot, e.length) {
                    Ok(()) => {
                        self.tx_lengths[e.slot as usize] = e.length;
                        count += 1;
                    }
                    Err(_) => {
                        let done = Entry {
                            length: 0,
                            status: FAILED,
                            ..e
                        };
                        self.tx_owner.queue_return(done)?;
                        self.returns.push(done, now, timeout)?;
                    }
                }
            }
            for _ in 0..self.geometry.capacity {
                let Some((slot, length)) = self.device.receive()? else {
                    break;
                };
                let e = self.entry(slot, length, COMPLETED, RingKind::RxDone)?;
                if self.rx.available()? == 0 {
                    // New RX may drop, but its credit must immediately return
                    // to the NIC. No peer-owned slot may be recycled here.
                    self.rx_owner.publish(e)?;
                    self.rx_owner.receive(e)?;
                    self.device.recycle_rx(slot)?;
                    self.counters.dropped = self.counters.dropped.saturating_add(1);
                    self.counters.full = self.counters.full.saturating_add(1);
                    continue;
                }
                self.rx_owner.publish(e)?;
                match self.rx.push(e) {
                    Ok(hint) => wake |= hint,
                    Err(error) => {
                        self.rx_owner.receive(e)?;
                        self.device.recycle_rx(e.slot)?;
                        return Err(error);
                    }
                }
                count += 1;
                self.counters.packets = self.counters.packets.saturating_add(1);
                self.counters.bytes = self.counters.bytes.saturating_add(u64::from(length));
            }
            Ok(count)
        })();
        if wake {
            notify();
        }
        result.map_err(|e| self.invalid(e))
    }
    /// Arm the CPU queues before checking NIC work and invoking wait_event.
    /// IRQ notification shares that event but has an independent kernel budget.
    pub fn arm(&mut self) -> Result<bool, Error> {
        if self.closed {
            return Err(Error::Stale);
        }
        let result = (|| {
            let tx = self.tx.arm()?;
            let rx = self.rx_return.arm()?;
            let space = if self.returns.front().is_some() {
                self.tx_done.arm()?
            } else {
                true
            };
            Ok(tx && rx && space)
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
        self.tx.disarm();
        self.rx_return.disarm();
        self.tx_done.disarm();
    }
}
