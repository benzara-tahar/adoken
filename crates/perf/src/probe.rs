//! The line protocol between an app launched with [`ENV`] set and the
//! `perf-cold-start` runner that reads its stdout.
//!
//! The app half (`cold_start::install`, feature `gpui`) prints:
//!
//! ```text
//! adoken-perf first-frame unix_ns=1790000000123456789
//! adoken-perf frame draw_us=812
//! adoken-perf done
//! ```
//!
//! Timestamps are wall-clock (`SystemTime`) nanoseconds because they are
//! compared across processes: the runner reads the clock right before
//! `spawn`, the app right after its first frame is on screen.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Environment variable that arms the probe. `1` measures the first frame and
/// quits; `frames=N` additionally forces `N` full redraws and reports each
/// frame's draw time before quitting. Unset or anything else: probe is inert.
pub const ENV: &str = "ADOKEN_PERF";

const PREFIX: &str = "adoken-perf ";

/// What the probe was asked to do, parsed from [`ENV`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    /// Forced full redraws to time after the first frame (0 = quit at once).
    pub frames: u32,
}

impl Mode {
    pub fn from_env() -> Option<Self> {
        Self::parse(&std::env::var(ENV).ok()?)
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "1" => Some(Self { frames: 0 }),
            other => {
                let frames = other.strip_prefix("frames=")?.parse().ok()?;
                Some(Self { frames })
            }
        }
    }
}

/// One stdout line of the probe protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The first frame reached the screen at this wall-clock time.
    FirstFrame { unix_ns: u64 },
    /// One forced redraw took this long inside `Window::draw`.
    Frame { draw: Duration },
    /// The probe finished; the app is quitting.
    Done,
}

impl Event {
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.trim_end().strip_prefix(PREFIX)?;
        let (kind, field) = rest.split_once(' ').unwrap_or((rest, ""));
        match kind {
            "first-frame" => Some(Self::FirstFrame {
                unix_ns: field.strip_prefix("unix_ns=")?.parse().ok()?,
            }),
            "frame" => Some(Self::Frame {
                draw: Duration::from_micros(field.strip_prefix("draw_us=")?.parse().ok()?),
            }),
            "done" => Some(Self::Done),
            _ => None,
        }
    }

    /// Prints the event as one protocol line on stdout. Rust's stdout is
    /// line-buffered, so the runner sees it immediately even through a pipe.
    pub fn emit(self) {
        println!("{self}");
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FirstFrame { unix_ns } => write!(f, "{PREFIX}first-frame unix_ns={unix_ns}"),
            Self::Frame { draw } => write!(f, "{PREFIX}frame draw_us={}", draw.as_micros()),
            Self::Done => write!(f, "{PREFIX}done"),
        }
    }
}

/// Wall-clock nanoseconds since the Unix epoch.
pub fn unix_now_ns() -> u64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970");
    u64::try_from(since_epoch.as_nanos()).expect("unix ns overflows u64 (year 2554)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parsing() {
        assert_eq!(Mode::parse("1"), Some(Mode { frames: 0 }));
        assert_eq!(Mode::parse("frames=120"), Some(Mode { frames: 120 }));
        assert_eq!(Mode::parse("0"), None);
        assert_eq!(Mode::parse(""), None);
        assert_eq!(Mode::parse("frames=lots"), None);
    }

    #[test]
    fn events_round_trip_through_their_line_form() {
        for event in [
            Event::FirstFrame {
                unix_ns: 1_790_000_000_123_456_789,
            },
            Event::Frame {
                draw: Duration::from_micros(812),
            },
            Event::Done,
        ] {
            assert_eq!(Event::parse(&event.to_string()), Some(event));
        }
    }

    #[test]
    fn unrelated_app_output_is_ignored() {
        assert_eq!(Event::parse("[gpui] metal device: Apple M4 Pro"), None);
        assert_eq!(Event::parse("adoken-perf frame draw_us=abc"), None);
        assert_eq!(Event::parse("adoken-perf first-frame"), None);
        assert_eq!(Event::parse("adoken-perf warp speed"), None);
    }

    #[test]
    fn crlf_terminated_lines_parse() {
        assert_eq!(Event::parse("adoken-perf done\r\n"), Some(Event::Done));
    }
}
