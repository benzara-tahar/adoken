//! Injected time: the engine never reads the wall clock itself, so tests
//! drive quiet hours, delivery windows, and snooze wake-up deterministically
//! (spec 0005 §6 harness).

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use adoken_model::Millis;

/// Now plus the local UTC offset, which schedules (quiet hours, windows,
/// "Tomorrow 9:00") are expressed in.
pub trait Clock: Send + Sync {
    fn now(&self) -> Millis;
    /// Local UTC offset in seconds at `at` (DST-aware on the system clock).
    fn offset_at(&self, at: Millis) -> i32;
}

/// Wall clock + the OS time zone.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Millis {
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Millis::try_from(since_epoch.as_millis()).unwrap_or(Millis::MAX)
    }

    fn offset_at(&self, at: Millis) -> i32 {
        system_offset_at(at)
    }
}

/// `localtime_r` rather than `time::UtcOffset::local_offset_at`, which refuses
/// to answer once a process has more than one thread (every gpui app).
#[cfg(unix)]
fn system_offset_at(at: Millis) -> i32 {
    let secs = at.div_euclid(1000) as libc::time_t;
    // SAFETY: `localtime_r` writes only into the `tm` we pass and reads `secs`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::localtime_r(&secs, &mut tm) };
    if result.is_null() {
        0
    } else {
        tm.tm_gmtoff as i32
    }
}

#[cfg(not(unix))]
fn system_offset_at(_at: Millis) -> i32 {
    0
}

/// Test/demo clock: fixed offset, time moves only when told to. Cloning shares
/// the same instant.
#[derive(Clone, Debug)]
pub struct FakeClock {
    now: Arc<AtomicI64>,
    offset_secs: i32,
}

impl FakeClock {
    pub fn new(now: Millis, offset_secs: i32) -> Self {
        Self {
            now: Arc::new(AtomicI64::new(now)),
            offset_secs,
        }
    }

    pub fn set(&self, now: Millis) {
        self.now.store(now, Ordering::SeqCst);
    }

    pub fn advance(&self, by: Millis) {
        self.now.fetch_add(by, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Millis {
        self.now.load(Ordering::SeqCst)
    }

    fn offset_at(&self, _at: Millis) -> i32 {
        self.offset_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_clones_share_time() {
        let clock = FakeClock::new(1_000, 3_600);
        let other = clock.clone();
        clock.advance(500);
        assert_eq!(other.now(), 1_500);
        other.set(10);
        assert_eq!(clock.now(), 10);
        assert_eq!(clock.offset_at(0), 3_600);
    }

    #[test]
    fn system_clock_reports_a_plausible_offset() {
        let clock = SystemClock;
        let offset = clock.offset_at(clock.now());
        assert!(offset.abs() <= 14 * 3_600, "{offset}");
    }
}
