//! The resource meter's pure model (spec 0023 FR-3, FR-5, FR-6, FR-9,
//! FR-10; activity.md §2.7): formatting, the 10 s CPU average, hysteresis,
//! the 5 s text rate, the sampling cadence, and the tooltip and copied
//! line. No gpui, no IO: times are monotonic milliseconds from the caller.

use std::collections::VecDeque;

/// One process reading (spec FR-4), CPU already in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    /// Physical footprint, bytes (Activity Monitor's Memory column).
    pub footprint: u64,
    /// Resident size, bytes.
    pub resident: u64,
    /// Peak footprint since launch, bytes; `None` on the fallback path.
    pub peak: Option<u64>,
    /// User + system CPU time since launch, ns.
    pub cpu_ns: u64,
}

/// A reading stamped with the monotonic time it was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub at: u64,
    pub reading: Reading,
}

/// Samples kept: 10 s of CPU deltas at the active 2 s cadence (FR-5).
pub const KEEP: usize = 6;
/// The CPU average's window (FR-3).
pub const CPU_WINDOW_MS: u64 = 10_000;
/// The text changes at most once per this long (FR-6).
pub const MIN_CHANGE_GAP_MS: u64 = 5_000;
/// Active and visible: one sample per 2 s (FR-5).
pub const ACTIVE_INTERVAL_MS: u64 = 2_000;
/// Visible, adoken in the background: one per 10 s (FR-5).
pub const BACKGROUND_INTERVAL_MS: u64 = 10_000;
/// Consecutive failed reads before the meter hides (spec §3 States).
pub const FAILURES_TO_HIDE: u8 = 3;

const MB: f64 = 1024.0 * 1024.0;
/// From here memory shows in GB (995 MB would round to "1,000 MB").
const GB_FROM_MB: f64 = 995.0;
/// Hysteresis: the raw value must move this far from the shown one.
const MB_STEP: f64 = 7.5;
const GB_STEP_MB: f64 = 0.075 * 1024.0;

/// The memory part as shown: "260 MB" or "1.2 GB".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Memory {
    /// Whole MB, a multiple of 10 (10…990).
    Mb(u32),
    /// Tenths of a GB (10 = "1.0 GB").
    Gb(u32),
}

impl Memory {
    pub fn from_bytes(bytes: u64) -> Self {
        let mb = bytes as f64 / MB;
        if mb < GB_FROM_MB {
            let tens = (mb / 10.0).round().max(1.0);
            Self::Mb(tens as u32 * 10)
        } else {
            Self::Gb((mb / 1024.0 * 10.0).round() as u32)
        }
    }

    fn mb(self) -> f64 {
        match self {
            Self::Mb(mb) => f64::from(mb),
            Self::Gb(tenths) => f64::from(tenths) / 10.0 * 1024.0,
        }
    }

    pub fn text(self) -> String {
        match self {
            Self::Mb(mb) => format!("{mb} MB"),
            Self::Gb(tenths) => format!("{}.{} GB", tenths / 10, tenths % 10),
        }
    }

    fn spoken(self) -> String {
        match self {
            Self::Mb(mb) => format!("{mb} megabytes"),
            Self::Gb(tenths) => format!("{}.{} gigabytes", tenths / 10, tenths % 10),
        }
    }
}

/// Whole percent of one core: "0%" below 0.5 (FR-6).
pub fn cpu_percent(percent: f64) -> u32 {
    percent.max(0.0).round() as u32
}

/// Mach absolute-time ticks to nanoseconds (`mach_timebase_info`: 125/3
/// on Apple Silicon, 1/1 on Intel; FR-4).
pub fn ticks_to_ns(ticks: u64, numer: u32, denom: u32) -> u64 {
    if denom == 0 {
        return ticks;
    }
    (u128::from(ticks) * u128::from(numer) / u128::from(denom)) as u64
}

/// CPU over a span: `cpu_ns` of CPU time in `wall_ms`, as % of one core.
pub fn cpu_over(cpu_ns: u64, wall_ms: u64) -> Option<f64> {
    (wall_ms > 0).then(|| cpu_ns as f64 / (wall_ms as f64 * 1_000_000.0) * 100.0)
}

/// What the statusbar paints, rebuilt only when it changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeterText {
    pub memory: Memory,
    /// `None` until the first CPU interval completes.
    pub cpu: Option<u32>,
}

impl MeterText {
    /// "260 MB · 1% CPU", or "260 MB" before the first CPU interval.
    pub fn full(&self) -> String {
        match self.cpu {
            Some(cpu) => format!("{} · {cpu}% CPU", self.memory.text()),
            None => self.memory.text(),
        }
    }

    /// The collapsed form (activity.md §2.5 step 2): memory only.
    pub fn short(&self) -> String {
        self.memory.text()
    }

    /// The accessible label (FR-12): collapsed states keep it.
    pub fn spoken(&self) -> String {
        match self.cpu {
            Some(cpu) => format!("adoken memory {}, CPU {cpu} percent", self.memory.spoken()),
            None => format!("adoken memory {}", self.memory.spoken()),
        }
    }
}

/// Everything the tooltip and the copied line say (FR-9, FR-10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Details {
    pub reading: Reading,
    /// CPU % of one core over the last 10 s, once known.
    pub cpu_recent: Option<f64>,
    /// Since launch.
    pub uptime_ms: u64,
}

fn whole_mb(bytes: u64) -> String {
    let mb = (bytes as f64 / MB).round() as usize;
    crate::activity::net::count_text(mb)
}

/// "14s", "2m 14s", "3h 12m".
pub fn duration_text(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// The four tooltip lines (activity.md §2.7); numbers in whole MB.
pub fn tooltip(details: &Details) -> String {
    let r = &details.reading;
    let memory = format!(
        "adoken is using {} MB of memory, as Activity Monitor counts it.",
        whole_mb(r.footprint)
    );
    let resident = match r.peak {
        Some(peak) => format!(
            "Resident {} MB · peak {} MB since launch",
            whole_mb(r.resident),
            whole_mb(peak)
        ),
        None => format!("Resident {} MB", whole_mb(r.resident)),
    };
    let total = duration_text(r.cpu_ns / 1_000_000);
    let cpu = match details.cpu_recent {
        Some(cpu) => format!(
            "CPU {}% of one core over the last 10 s · {total} since launch",
            cpu_percent(cpu)
        ),
        None => format!("CPU {total} since launch"),
    };
    format!(
        "{memory}\n{resident}\n{cpu}\nadoken keeps caches so views open instantly. Memory rises \
         as you open things and settles; a high number alone isn't a leak."
    )
}

/// The "Copy resource usage" line (activity.md §2.7): no org, user or item
/// data.
pub fn copy_line(version: &str, details: &Details) -> String {
    let r = &details.reading;
    let memory = match r.peak {
        Some(peak) => format!(
            "memory {} MB (resident {} MB, peak {} MB)",
            whole_mb(r.footprint),
            whole_mb(r.resident),
            whole_mb(peak)
        ),
        None => format!(
            "memory {} MB (resident {} MB)",
            whole_mb(r.footprint),
            whole_mb(r.resident)
        ),
    };
    let total = duration_text(r.cpu_ns / 1_000_000);
    let cpu = match details.cpu_recent {
        Some(cpu) => format!("CPU {}% over 10 s, {total} total", cpu_percent(cpu)),
        None => format!("CPU {total} total"),
    };
    format!(
        "adoken {version} · {memory} · {cpu} · up {}",
        duration_text(details.uptime_ms)
    )
}

/// The shown text's state machine: last [`KEEP`] samples, hysteresis
/// (FR-6) and the trailing-edge 5 s rate.
#[derive(Clone, Debug, Default)]
pub struct Meter {
    samples: VecDeque<Sample>,
    shown: Option<MeterText>,
    changed_at: Option<u64>,
    /// A different text is waiting for the 5 s gap to pass.
    pending: bool,
    failures: u8,
}

impl Meter {
    /// Record a reading taken at `at`; returns the new text if it changes
    /// now.
    pub fn push(&mut self, sample: Sample) -> Option<MeterText> {
        self.failures = 0;
        if self.samples.len() == KEEP {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        self.settle(sample.at)
    }

    /// A failed read: the last good value stays; after
    /// [`FAILURES_TO_HIDE`] in a row the meter hides. Returns true when it
    /// just hid.
    pub fn fail(&mut self) -> bool {
        self.failures = self.failures.saturating_add(1);
        if self.failures == FAILURES_TO_HIDE && self.shown.is_some() {
            self.shown = None;
            self.pending = false;
            return true;
        }
        false
    }

    pub fn failed(&self) -> bool {
        self.failures >= FAILURES_TO_HIDE
    }

    /// Sampling stopped (hidden, off): drop the CPU history so a resume
    /// doesn't average over the gap. The shown text stays.
    pub fn pause(&mut self) {
        self.samples.clear();
        self.pending = false;
    }

    pub fn shown(&self) -> Option<&MeterText> {
        self.shown.as_ref()
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    /// CPU % of one core over the last 10 s: from the newest sample at
    /// least 10 s older than the latest, else the oldest kept.
    pub fn cpu_recent(&self) -> Option<f64> {
        let newest = self.samples.back()?;
        let base = self
            .samples
            .iter()
            .rev()
            .skip(1)
            .find(|s| newest.at - s.at >= CPU_WINDOW_MS)
            .or_else(|| self.samples.front().filter(|s| s.at < newest.at))?;
        cpu_over(
            newest.reading.cpu_ns.saturating_sub(base.reading.cpu_ns),
            newest.at - base.at,
        )
    }

    /// When a pending change may show (trailing edge).
    pub fn due(&self) -> Option<u64> {
        self.pending
            .then(|| self.changed_at.map(|t| t + MIN_CHANGE_GAP_MS))
            .flatten()
    }

    /// Apply a pending change once its gap has passed.
    pub fn settle(&mut self, now: u64) -> Option<MeterText> {
        let desired = self.desired()?;
        if self.shown.as_ref() == Some(&desired) {
            self.pending = false;
            return None;
        }
        if self.changed_at.is_some_and(|t| now < t + MIN_CHANGE_GAP_MS) {
            self.pending = true;
            return None;
        }
        self.pending = false;
        self.changed_at = Some(now);
        self.shown = Some(desired.clone());
        Some(desired)
    }

    /// What the meter would show now, hysteresis applied (FR-6).
    fn desired(&self) -> Option<MeterText> {
        let latest = self.samples.back()?;
        let raw_mb = latest.reading.footprint as f64 / MB;
        let rounded = Memory::from_bytes(latest.reading.footprint);
        let cpu = self.cpu_recent();
        let Some(shown) = &self.shown else {
            return Some(MeterText {
                memory: rounded,
                cpu: cpu.map(cpu_percent),
            });
        };
        let step = if matches!(rounded, Memory::Gb(_)) || matches!(shown.memory, Memory::Gb(_)) {
            GB_STEP_MB
        } else {
            MB_STEP
        };
        let memory = if rounded != shown.memory && (raw_mb - shown.memory.mb()).abs() >= step {
            rounded
        } else {
            shown.memory
        };
        let cpu = match (cpu, shown.cpu) {
            (Some(avg), Some(was)) if (avg - f64::from(was)).abs() >= 1.0 => Some(cpu_percent(avg)),
            (Some(avg), None) => Some(cpu_percent(avg)),
            (_, was) => was,
        };
        Some(MeterText { memory, cpu })
    }
}

/// Why sampling runs, or not (FR-5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lifecycle {
    /// `[activity] resource_meter`.
    pub enabled: bool,
    /// The main window isn't minimized, hidden or fully covered.
    pub visible: bool,
    /// adoken is the active app.
    pub active: bool,
    /// The meter is collapsed away (activity.md §2.5 step 4).
    pub collapsed: bool,
}

impl Lifecycle {
    /// Sampling interval, `None` while nothing should sample.
    pub fn interval(&self) -> Option<u64> {
        if !self.enabled || !self.visible || self.collapsed {
            None
        } else if self.active {
            Some(ACTIVE_INTERVAL_MS)
        } else {
            Some(BACKGROUND_INTERVAL_MS)
        }
    }
}

/// What a wake did.
#[derive(Clone, Debug, PartialEq)]
pub enum Wake {
    /// Nothing visible changed.
    Quiet,
    /// The text changed.
    Text(MeterText),
    /// Three failures in a row: hide.
    Hidden,
}

/// The sampler's schedule over [`Meter`]: when to read, when to flush a
/// pending change. Runs on the background executor (FR-5).
#[derive(Clone, Debug, Default)]
pub struct Sampler {
    pub meter: Meter,
    lifecycle: Lifecycle,
    next_at: Option<u64>,
    last_at: Option<u64>,
}

impl Sampler {
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// New lifecycle at `now`: starting (or resuming) samples at once;
    /// a cadence change keeps the last sample's phase; stopping pauses.
    pub fn set_lifecycle(&mut self, lifecycle: Lifecycle, now: u64) {
        let was = self.lifecycle.interval();
        self.lifecycle = lifecycle;
        match (was, lifecycle.interval()) {
            (_, None) => {
                if was.is_some() {
                    self.meter.pause();
                }
                self.next_at = None;
                self.last_at = None;
            }
            (None, Some(_)) => self.next_at = Some(now),
            (Some(_), Some(interval)) => {
                self.next_at = Some(self.last_at.map_or(now, |last| (last + interval).max(now)));
            }
        }
    }

    /// The next time [`Self::wake`] has work.
    pub fn next_wake(&self) -> Option<u64> {
        match (self.next_at, self.meter.due()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Whether a read is due at `now`.
    pub fn sample_due(&self, now: u64) -> bool {
        self.next_at.is_some_and(|at| now >= at)
    }

    /// Do what's due at `now`; `read` is called only when a sample is due.
    pub fn wake(&mut self, now: u64, read: impl FnOnce() -> Option<Reading>) -> Wake {
        if self.sample_due(now) {
            self.last_at = Some(now);
            self.next_at = self.lifecycle.interval().map(|interval| now + interval);
            return match read() {
                Some(reading) => match self.meter.push(Sample { at: now, reading }) {
                    Some(text) => Wake::Text(text),
                    None => Wake::Quiet,
                },
                None if self.meter.fail() => Wake::Hidden,
                None => Wake::Quiet,
            };
        }
        match self.meter.settle(now) {
            Some(text) => Wake::Text(text),
            None => Wake::Quiet,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn mb(value: f64) -> u64 {
        (value * MIB as f64) as u64
    }

    fn reading(footprint_mb: f64, cpu_ns: u64) -> Reading {
        Reading {
            footprint: mb(footprint_mb),
            resident: mb(325.0),
            peak: Some(mb(373.0)),
            cpu_ns,
        }
    }

    #[test]
    fn memory_and_cpu_round_as_activity_monitor_reads() {
        // AC-1.
        let text = |v: f64| Memory::from_bytes(mb(v)).text();
        assert_eq!(text(260.4), "260 MB");
        assert_eq!(text(4.9), "10 MB");
        assert_eq!(text(994.9), "990 MB");
        assert_eq!(text(995.0), "1.0 GB", "\"1,000 MB\" is never shown");
        assert_eq!(text(1288.0), "1.3 GB");
        assert_eq!(cpu_percent(0.4), 0);
        assert_eq!(cpu_percent(1.6), 2);
        assert_eq!(cpu_percent(250.2), 250);
    }

    #[test]
    fn cpu_converts_mach_ticks_on_both_timebases() {
        // AC-2: 1 s of CPU over 10 s of wall time is 10% of one core.
        let apple = ticks_to_ns(24_000_000, 125, 3);
        assert_eq!(apple, 1_000_000_000);
        let intel = ticks_to_ns(1_000_000_000, 1, 1);
        for ns in [apple, intel] {
            assert_eq!(cpu_percent(cpu_over(ns, 10_000).unwrap()), 10);
        }
    }

    /// Feed `f(i)` every 2 s for `secs`; returns (time, text) per change.
    fn run(secs: u64, f: impl Fn(u64) -> Reading) -> Vec<(u64, String)> {
        let mut sampler = Sampler::default();
        let on = Lifecycle {
            enabled: true,
            visible: true,
            active: true,
            collapsed: false,
        };
        sampler.set_lifecycle(on, 0);
        let mut changes = Vec::new();
        let mut now = 0;
        let mut i = 0;
        while now <= secs * 1000 {
            let wake = sampler.wake(now, || {
                let r = f(i);
                i += 1;
                Some(r)
            });
            if let Wake::Text(text) = wake {
                changes.push((now, text.full()));
            }
            now = sampler.next_wake().unwrap();
        }
        changes
    }

    #[test]
    fn hysteresis_and_the_five_second_rate_hold_under_any_input() {
        // AC-3: 255 / 265 MB alternating changes memory at most once.
        let changes = run(60, |i| reading(if i % 2 == 0 { 255.0 } else { 265.0 }, 0));
        let memories: Vec<_> = changes
            .iter()
            .map(|(_, t)| t.split(" · ").next().unwrap().to_owned())
            .collect();
        let mut distinct = memories.clone();
        distinct.dedup();
        assert!(distinct.len() <= 2, "{changes:?}");
        // 0.2% / 0.4% shows "0%" and never changes.
        let ns_per_2s = |pct: f64| (pct / 100.0 * 2e9) as u64;
        let changes = run(60, |i| {
            let cpu: u64 = (0..i)
                .map(|k| ns_per_2s(if k % 2 == 0 { 0.2 } else { 0.4 }))
                .sum();
            reading(260.0, cpu)
        });
        assert!(
            changes
                .iter()
                .all(|(_, t)| !t.contains("CPU") || t.ends_with(" 0% CPU"))
        );
        assert!(changes.len() <= 2, "{changes:?}");
        // 1.2% / 1.8% changes CPU at most once after it first shows.
        let changes = run(60, |i| {
            let cpu: u64 = (0..i)
                .map(|k| ns_per_2s(if k % 2 == 0 { 1.2 } else { 1.8 }))
                .sum();
            reading(260.0, cpu)
        });
        assert!(changes.len() <= 3, "{changes:?}");
        // A jump 260 → 400 MB shows within 5 s.
        let changes = run(30, |i| reading(if i < 7 { 260.0 } else { 400.0 }, 0));
        let jump_at = 7 * 2000;
        let shown = changes
            .iter()
            .find(|(_, t)| t.starts_with("400 MB"))
            .unwrap();
        assert!(shown.0 - jump_at <= MIN_CHANGE_GAP_MS, "{changes:?}");
        // No two changes < 5 s apart, whatever the input.
        let noisy = run(120, |i| {
            reading(
                100.0 + (i * 37 % 500) as f64,
                i * 3_000_000_000 % 7_000_000_000 * i,
            )
        });
        for pair in noisy.windows(2) {
            assert!(pair[1].0 - pair[0].0 >= MIN_CHANGE_GAP_MS, "{noisy:?}");
        }
    }

    #[test]
    fn memory_shows_first_then_cpu_once_an_interval_completes() {
        let changes = run(10, |i| reading(260.0, i * 20_000_000));
        assert_eq!(changes[0], (0, "260 MB".to_owned()));
        assert_eq!(
            changes[1],
            (MIN_CHANGE_GAP_MS, "260 MB · 1% CPU".to_owned())
        );
    }

    fn count_reads(lifecycle: Lifecycle, secs: u64) -> usize {
        let mut sampler = Sampler::default();
        sampler.set_lifecycle(lifecycle, 0);
        let mut reads = 0;
        let mut now = 0;
        while let Some(at) = sampler.next_wake() {
            if at >= secs * 1000 {
                break;
            }
            now = at.max(now);
            sampler.wake(now, || {
                reads += 1;
                Some(reading(260.0, 0))
            });
        }
        reads
    }

    #[test]
    fn cadence_follows_window_and_app_state() {
        // AC-5 over 60 s.
        let on = Lifecycle {
            enabled: true,
            visible: true,
            active: true,
            collapsed: false,
        };
        assert_eq!(count_reads(on, 60), 30);
        assert_eq!(
            count_reads(
                Lifecycle {
                    active: false,
                    ..on
                },
                60
            ),
            6
        );
        assert_eq!(
            count_reads(
                Lifecycle {
                    visible: false,
                    ..on
                },
                60
            ),
            0
        );
        assert_eq!(
            count_reads(
                Lifecycle {
                    enabled: false,
                    ..on
                },
                60
            ),
            0
        );
        assert_eq!(
            count_reads(
                Lifecycle {
                    collapsed: true,
                    ..on
                },
                60
            ),
            0
        );
        // Turning it on reads at once, before the 2 s timer.
        let mut sampler = Sampler::default();
        sampler.set_lifecycle(on, 5_000);
        assert_eq!(sampler.next_wake(), Some(5_000));
        // Going to the background keeps the phase; hiding stops it.
        sampler.wake(5_000, || Some(reading(260.0, 0)));
        sampler.set_lifecycle(
            Lifecycle {
                active: false,
                ..on
            },
            6_000,
        );
        assert_eq!(sampler.next_wake(), Some(15_000));
        sampler.set_lifecycle(
            Lifecycle {
                visible: false,
                ..on
            },
            7_000,
        );
        assert_eq!(sampler.next_wake(), None);
    }

    #[test]
    fn three_failures_hide_and_one_success_recovers() {
        let mut sampler = Sampler::default();
        let on = Lifecycle {
            enabled: true,
            visible: true,
            active: true,
            collapsed: false,
        };
        sampler.set_lifecycle(on, 0);
        assert!(matches!(
            sampler.wake(0, || Some(reading(260.0, 0))),
            Wake::Text(_)
        ));
        assert_eq!(
            sampler.wake(2_000, || None),
            Wake::Quiet,
            "keeps the last value"
        );
        assert_eq!(sampler.wake(4_000, || None), Wake::Quiet);
        assert_eq!(sampler.wake(6_000, || None), Wake::Hidden);
        assert!(sampler.meter.failed());
        assert!(matches!(
            sampler.wake(8_000, || Some(reading(260.0, 0))),
            Wake::Text(_)
        ));
    }

    #[test]
    fn tooltip_and_copy_line_match_the_design() {
        // AC-9 (activity.md §2.7 fixture).
        let details = Details {
            reading: Reading {
                footprint: 260 * MIB,
                resident: 325 * MIB,
                peak: Some(373 * MIB),
                cpu_ns: 134 * 1_000_000_000,
            },
            cpu_recent: Some(1.2),
            uptime_ms: (3 * 3600 + 12 * 60) * 1000,
        };
        assert_eq!(
            copy_line("0.1.0", &details),
            "adoken 0.1.0 · memory 260 MB (resident 325 MB, peak 373 MB) · CPU 1% over 10 s, \
             2m 14s total · up 3h 12m"
        );
        assert_eq!(
            tooltip(&details),
            "adoken is using 260 MB of memory, as Activity Monitor counts it.\n\
             Resident 325 MB · peak 373 MB since launch\n\
             CPU 1% of one core over the last 10 s · 2m 14s since launch\n\
             adoken keeps caches so views open instantly. Memory rises as you open things and \
             settles; a high number alone isn't a leak."
        );
        // Fallback source: no peak.
        let fallback = Details {
            reading: Reading {
                peak: None,
                ..details.reading
            },
            ..details
        };
        assert!(tooltip(&fallback).contains("\nResident 325 MB\n"));
        let text = MeterText {
            memory: Memory::Mb(260),
            cpu: Some(1),
        };
        assert_eq!(text.full(), "260 MB · 1% CPU");
        assert_eq!(text.spoken(), "adoken memory 260 megabytes, CPU 1 percent");
    }

    #[test]
    fn the_model_stays_small() {
        // §7: < 1 KB held.
        assert!(std::mem::size_of::<Sampler>() + KEEP * std::mem::size_of::<Sample>() < 1024);
    }
}
