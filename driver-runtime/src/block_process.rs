//! One copied command step per receipt. The controller owner retains buffers;
//! an old process cannot complete a step belonging to its replacement.
use crate::protocol::Error;
pub const BYTES: usize = 64;
pub struct Channel {
    next: u64,
    pending: u64,
    event: Option<[u8; BYTES]>,
    action: Option<[u8; BYTES]>,
    failed: bool,
}
impl Channel {
    pub const fn new() -> Self {
        Self {
            next: 0,
            pending: 0,
            event: None,
            action: None,
            failed: false,
        }
    }
    pub fn post(&mut self, event: [u8; BYTES]) -> Result<(), Error> {
        if self.failed {
            return Err(Error::State);
        }
        if self.pending != 0 || self.event.is_some() || self.action.is_some() {
            return Err(Error::Backpressure);
        }
        self.next = self.next.checked_add(1).ok_or(Error::Exhausted)?;
        self.event = Some(event);
        Ok(())
    }
    pub fn fetch(&mut self) -> Result<Option<(u64, [u8; BYTES])>, Error> {
        if self.failed {
            return Err(Error::State);
        }
        let Some(event) = self.event.take() else {
            return Ok(None);
        };
        self.pending = self.next;
        Ok(Some((self.pending, event)))
    }
    pub fn reply(&mut self, receipt: u64, action: [u8; BYTES]) -> Result<(), Error> {
        if self.failed || receipt == 0 || receipt != self.pending || self.action.is_some() {
            return Err(Error::Stale);
        }
        self.pending = 0;
        self.action = Some(action);
        Ok(())
    }
    pub fn take(&mut self) -> Result<Option<[u8; BYTES]>, Error> {
        if self.failed {
            Err(Error::State)
        } else {
            Ok(self.action.take())
        }
    }
    pub fn readable(&self) -> bool {
        !self.failed && self.event.is_some()
    }
    pub fn lost(&mut self) {
        self.failed = true;
        self.pending = 0;
        self.event = None;
        self.action = None;
    }
    /// A new owner is admitted only with no active transport command. Monotonic
    /// receipts are preserved, including after process loss.
    pub fn rebind(&mut self) {
        self.failed = false;
        self.pending = 0;
        self.event = None;
        self.action = None;
    }
}
