use super::Error;
/// Readiness uses elapsed time, not CPU-dependent iteration counts. A running
/// clock may wrap between samples (ACPI PM timer); a stalled clock is bounded.
pub struct ReadyDeadline {
    previous: u64,
    elapsed: u64,
    budget: u64,
    mask: u64,
    stalled: u32,
}
impl ReadyDeadline {
    pub fn new(start: u64, mask: u64, frequency: u64, milliseconds: u64) -> Result<Self, Error> {
        if mask == 0
            || mask & mask.wrapping_add(1) != 0
            || frequency == 0
            || milliseconds == 0
            || milliseconds > 30_000
        {
            return Err(Error::Unsupported);
        }
        let budget = frequency
            .checked_mul(milliseconds)
            .and_then(|n| n.checked_add(999))
            .ok_or(Error::Unsupported)?
            / 1000;
        Ok(Self {
            previous: start & mask,
            elapsed: 0,
            budget,
            mask,
            stalled: 0,
        })
    }
    /// Sample at least once per counter wrap. The storage pump returns between
    /// short portions, so even the 24-bit PM timer's ~4.7s period is sufficient.
    pub fn expired(&mut self, now: u64) -> Result<bool, Error> {
        let now = now & self.mask;
        let delta = now.wrapping_sub(self.previous) & self.mask;
        self.previous = now;
        if delta == 0 {
            self.stalled = self.stalled.saturating_add(1);
            if self.stalled >= 31_250 {
                return Err(Error::Timeout);
            }
        } else {
            self.stalled = 0;
        }
        self.elapsed = self.elapsed.saturating_add(delta);
        Ok(self.elapsed >= self.budget)
    }
}
