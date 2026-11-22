//! The desktop clock: the minute the taskbar shows, for applications that
//! write a time into a document (Notepad's F5). It is the firmware's reading
//! advanced by the calibrated counter, in the firmware's own zone. Reading it
//! grants nothing and names no other process.
pub const SYSCALL: u64 = 416;

/// A wall-clock minute as the kernel reports it: minutes since 1970-01-01
/// 00:00 plus one, zero when there is no clock (no firmware time, or the
/// counter is not calibrated yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Minute(pub u64);
impl Minute {
    /// Year, month (1..=12), day (1..=31), hour and minute; None without a clock.
    pub const fn civil(self) -> Option<(u64, u8, u8, u8, u8)> {
        let Some(minute) = self.0.checked_sub(1) else {
            return None;
        };
        let days = minute / (24 * 60) + 719_468;
        let (era, day) = (days / 146_097, days % 146_097);
        let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
        let day = day - (365 * year + year / 4 - year / 100);
        let month = (5 * day + 2) / 153;
        let date = day - (153 * month + 2) / 5 + 1;
        let (month, next) = if month < 10 {
            (month + 3, 0)
        } else {
            (month - 9, 1)
        };
        Some((
            era * 400 + year + next,
            month as u8,
            date as u8,
            (minute / 60 % 24) as u8,
            (minute % 60) as u8,
        ))
    }
}

/// The current minute. `Ok(Minute(0))` means the system has no clock.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn now() -> Result<Minute, isize> {
    // SAFETY: no pointers; the kernel only returns a value.
    let value = unsafe { crate::raw::syscall(SYSCALL, [0; 5]) } as i64;
    if value < 0 {
        Err(value as isize)
    } else {
        Ok(Minute(value as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil_dates_follow_the_calendar() {
        assert_eq!(Minute(0).civil(), None);
        assert_eq!(Minute(1).civil(), Some((1970, 1, 1, 0, 0)));
        // 2024-02-29 13:45 is 28 488 285 minutes after the epoch.
        let days = 19_782u64; // 2024-02-29
        let m = Minute(days * 1440 + 13 * 60 + 45 + 1);
        assert_eq!(m.civil(), Some((2024, 2, 29, 13, 45)));
        let m = Minute(20_818 * 1440 + 23 * 60 + 59 + 1); // 2026-12-31
        assert_eq!(m.civil(), Some((2026, 12, 31, 23, 59)));
    }
}
