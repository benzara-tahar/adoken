//! Injected time source (spec 0006 §6 harness: "injected clock").
//!
//! Rules never read the system clock; the caller passes a [`Clock`], so tests
//! and gallery stories pin "now" and get identical output every run.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use adoken_model::Millis;

pub trait Clock: Send + Sync {
    /// Current instant, Unix milliseconds UTC.
    fn now(&self) -> Millis;
}

/// Wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Millis {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as Millis)
    }
}

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct FixedClock(AtomicI64);

impl FixedClock {
    pub fn new(now: Millis) -> Self {
        Self(AtomicI64::new(now))
    }

    pub fn set(&self, now: Millis) {
        self.0.store(now, Ordering::Relaxed);
    }

    pub fn advance(&self, by: Millis) {
        self.0.fetch_add(by, Ordering::Relaxed);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Millis {
        self.0.load(Ordering::Relaxed)
    }
}

/// The OS's current UTC offset in minutes (east positive), for
/// [`crate::WorkingHours::utc_offset_min`]. `0` where it can't be read.
///
/// Read once at startup; a DST change mid-session shifts working-hour
/// boundaries by the DST delta until the next launch.
pub fn local_utc_offset_min() -> i32 {
    #[cfg(unix)]
    {
        // SAFETY: `time(NULL)` has no preconditions; `localtime_r` writes only
        // into the zeroed `tm` we own and is the re-entrant variant.
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&now, &mut tm).is_null() {
                return 0;
            }
            (tm.tm_gmtoff / 60) as i32
        }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_moves_only_when_told() {
        let clock = FixedClock::new(1_000);
        assert_eq!(clock.now(), 1_000);
        clock.advance(500);
        assert_eq!(clock.now(), 1_500);
        clock.set(42);
        assert_eq!(clock.now(), 42);
    }

    #[test]
    fn local_offset_is_a_real_zone_offset() {
        let offset = local_utc_offset_min();
        assert!((-12 * 60..=14 * 60).contains(&offset), "{offset}");
    }
}
