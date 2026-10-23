//! The statusbar network indicator's model (spec 0016 FR-21; activity.md
//! §2.3, §2.4): it samples the log's [`NetSummary`], never mirrors it. Busy
//! shows only after a request has been in flight 150 ms and bridges idle
//! gaps under 500 ms; the problems chip counts unseen failures of the last
//! 5 minutes. [`Indicator::sample`] returns what to paint and when the
//! painted state could next change by itself, so no timer runs while
//! nothing changes. Also the polite announcers of FR-4. Pure: no gpui.

use adoken_model::Millis;
use adoken_sync::ledger::{Entry, NetSummary, Status};

/// Busy appears after a request has been in flight this long.
pub const BUSY_AFTER: Millis = 150;
/// Busy stays through idle gaps shorter than this.
pub const BUSY_BRIDGE: Millis = 500;
/// Unseen failures count for this long (FR-21).
pub const PROBLEM_WINDOW: Millis = 5 * 60_000;
/// In-flight counts above this read "99+".
pub const MAX_COUNT: usize = 99;
/// Network failure announcements: at most one per 30 s (FR-4).
pub const NET_ANNOUNCE_EVERY: Millis = 30_000;
/// Sync state announcements: at most one per 10 s (FR-4).
pub const SYNC_ANNOUNCE_EVERY: Millis = 10_000;

/// Whether the indicator shows at all (FR-21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Demo mode, no token, or `[activity] network_indicator = false`.
    Hidden,
    /// The slot becomes the Offline pill.
    Offline,
    Shown,
}

pub fn visibility(demo: bool, no_token: bool, setting_on: bool, offline: bool) -> Visibility {
    if demo || no_token || !setting_on {
        Visibility::Hidden
    } else if offline {
        Visibility::Offline
    } else {
        Visibility::Shown
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemTone {
    Danger,
    Warning,
}

/// The problems chip: tone, its count, and both counts for the tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Problems {
    pub tone: ProblemTone,
    pub danger: usize,
    pub warning: usize,
}

impl Problems {
    pub fn count(self) -> usize {
        match self.tone {
            ProblemTone::Danger => self.danger,
            ProblemTone::Warning => self.warning,
        }
    }
}

/// What the indicator paints; the statusbar repaints only when it changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndicatorView {
    Hidden,
    Offline,
    Shown {
        busy: bool,
        /// Busy: the in-flight count; idle: the last status ("200",
        /// "Failed", "Canceled"); `None`: glyph only (no request yet).
        value: Option<String>,
        problems: Option<Problems>,
    },
}

impl IndicatorView {
    /// The button's spoken label (activity.md §9.1).
    pub fn label(&self) -> String {
        match self {
            IndicatorView::Hidden => String::new(),
            IndicatorView::Offline => "Offline. Nothing is sent until you reconnect".into(),
            IndicatorView::Shown {
                busy,
                value,
                problems,
            } => {
                let head = match (busy, value) {
                    (true, Some(count)) => format!("Network: {count} requests in flight"),
                    (false, Some(status)) => format!("Network: last request returned {status}"),
                    _ => "Network: no requests yet".into(),
                };
                let problems = match problems {
                    Some(p) if p.tone == ProblemTone::Danger => {
                        format!(", {} failed requests in the last 5 minutes", p.danger)
                    }
                    Some(p) => format!(", {} warnings in the last 5 minutes", p.warning),
                    None => String::new(),
                };
                format!("{head}{problems}. Show network log, Command Shift A")
            }
        }
    }
}

/// One sample: what to paint, and when to sample again without new input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub view: IndicatorView,
    pub wake_at: Option<Millis>,
}

/// The sampled busy state (design §2.4).
#[derive(Clone, Debug, Default)]
pub struct Indicator {
    busy: bool,
    busy_count: usize,
    idle_since: Option<Millis>,
}

impl Indicator {
    pub fn sample(
        &mut self,
        summary: &NetSummary,
        seen_through: u64,
        visibility: Visibility,
        now: Millis,
    ) -> Sample {
        match visibility {
            Visibility::Hidden | Visibility::Offline => {
                *self = Indicator::default();
                let view = if visibility == Visibility::Hidden {
                    IndicatorView::Hidden
                } else {
                    IndicatorView::Offline
                };
                return Sample {
                    view,
                    wake_at: None,
                };
            }
            Visibility::Shown => {}
        }
        let mut wake: Option<Millis> = None;
        let mut wake_at = |at: Millis| wake = Some(wake.map_or(at, |w| w.min(at)));
        if summary.in_flight > 0 {
            self.idle_since = None;
            let oldest = summary.oldest_in_flight_at.unwrap_or(now);
            if self.busy || now - oldest >= BUSY_AFTER {
                self.busy = true;
                self.busy_count = summary.in_flight;
            } else {
                wake_at(oldest + BUSY_AFTER);
            }
        } else if self.busy {
            let idle = *self.idle_since.get_or_insert(now);
            if now - idle >= BUSY_BRIDGE {
                self.busy = false;
                self.idle_since = None;
            } else {
                wake_at(idle + BUSY_BRIDGE);
            }
        }
        let (danger, warning) = summary.unseen(seen_through);
        let problems = if danger > 0 {
            Some(ProblemTone::Danger)
        } else if warning > 0 {
            Some(ProblemTone::Warning)
        } else {
            None
        }
        .map(|tone| Problems {
            tone,
            danger,
            warning,
        });
        // The chip ages out 5 minutes after the oldest unseen failure.
        if let Some(oldest) = summary
            .recent_problems
            .iter()
            .filter(|(seq, _, _)| *seq > seen_through)
            .map(|(_, at, _)| *at)
            .min()
        {
            wake_at(oldest + PROBLEM_WINDOW);
        }
        let value = if self.busy {
            Some(count_value(self.busy_count))
        } else {
            summary
                .last_completed
                .as_ref()
                .map(|entry| last_status(&entry.status))
        };
        Sample {
            view: IndicatorView::Shown {
                busy: self.busy,
                value,
                problems,
            },
            wake_at: wake,
        }
    }
}

fn count_value(count: usize) -> String {
    if count > MAX_COUNT {
        format!("{MAX_COUNT}+")
    } else {
        count.to_string()
    }
}

/// The idle value: the code, or "Failed" / "Canceled" without one.
fn last_status(status: &Status) -> String {
    match status {
        Status::Http(code) => code.to_string(),
        Status::Canceled => "Canceled".into(),
        Status::Failed { .. } => "Failed".into(),
        Status::InFlight => "…".into(),
    }
}

/// The idle/busy/problems tooltip lines (activity.md §2.3).
pub fn tooltip(
    view: &IndicatorView,
    summary: &NetSummary,
    latest_problem: Option<&Entry>,
    offset: time::UtcOffset,
    now: Millis,
) -> String {
    use super::net::{latency_text, status_text, time_text};
    let tail = "Network log ⌘⇧A";
    match view {
        IndicatorView::Hidden => String::new(),
        IndicatorView::Offline => format!("Offline — nothing is sent until you reconnect · {tail}"),
        IndicatorView::Shown { problems, busy, .. } => {
            let mut lines = Vec::new();
            if let Some(p) = problems {
                let mut line = match p.tone {
                    ProblemTone::Danger => format!("{} failed in the last 5 min", p.danger),
                    ProblemTone::Warning => format!("{} warnings in the last 5 min", p.warning),
                };
                if let Some(e) = latest_problem {
                    line.push_str(&format!(
                        " · latest {} {} {} · {}",
                        time_text(e.at, offset, false),
                        e.method,
                        e.purpose,
                        status_text(&e.status)
                    ));
                }
                if p.tone == ProblemTone::Danger && p.warning > 0 {
                    line.push_str(&format!(" · {} warning", p.warning));
                }
                lines.push(line);
            } else if *busy {
                let oldest = summary
                    .oldest_in_flight_at
                    .map(|at| format!(" · oldest {}", latency_text((now - at).max(0) as u64)))
                    .unwrap_or_default();
                lines.push(format!("{} requests in flight{oldest}", summary.in_flight));
            } else {
                lines.push(match &summary.last_completed {
                    Some(e) => format!(
                        "Last request {} · {} {} · {}{}",
                        time_text(e.at, offset, false),
                        e.method,
                        e.purpose,
                        status_text(&e.status),
                        e.elapsed_ms
                            .map(|ms| format!(" · {}", latency_text(ms)))
                            .unwrap_or_default()
                    ),
                    None => "No requests yet".into(),
                });
            }
            lines.push(tail.into());
            lines.join(" · ")
        }
    }
}

/// Network failure announcements (FR-4): one when unseen danger failures
/// go from 0 to ≥ 1, then at most one per 30 s for later ones. Never one
/// per request.
#[derive(Clone, Debug, Default)]
pub struct NetAnnouncer {
    announced: usize,
    last_at: Option<Millis>,
}

impl NetAnnouncer {
    /// `describe` names the newest failure ("503, work items batch").
    /// Returns the announcement, if one is due now, and when to look again
    /// for one held back by the 30 s limit.
    pub fn observe(
        &mut self,
        danger_unseen: usize,
        describe: impl FnOnce() -> String,
        now: Millis,
    ) -> (Option<String>, Option<Millis>) {
        if danger_unseen == 0 {
            self.announced = 0;
            return (None, None);
        }
        if danger_unseen <= self.announced {
            self.announced = danger_unseen;
            return (None, None);
        }
        if let Some(last) = self.last_at
            && now - last < NET_ANNOUNCE_EVERY
        {
            return (None, Some(last + NET_ANNOUNCE_EVERY));
        }
        let text = if self.announced == 0 {
            format!("Azure DevOps request failed: {}", describe())
        } else {
            let more = danger_unseen - self.announced;
            if more == 1 {
                "1 more request failed".to_owned()
            } else {
                format!("{more} more requests failed")
            }
        };
        self.announced = danger_unseen;
        self.last_at = Some(now);
        (Some(text), None)
    }
}

/// Coarse sync state for announcements (FR-4): changes to failed,
/// throttled, offline, and back to normal.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum SyncMood {
    #[default]
    Normal,
    /// Which kinds fail ("Work items").
    Failed(String),
    Throttled,
    Offline,
}

/// Sync state announcements, at most one per 10 s; a change inside the
/// window is announced when it ends if it still holds.
#[derive(Clone, Debug, Default)]
pub struct SyncAnnouncer {
    announced: SyncMood,
    last_at: Option<Millis>,
}

impl SyncAnnouncer {
    pub fn observe(&mut self, mood: &SyncMood, now: Millis) -> (Option<String>, Option<Millis>) {
        if *mood == self.announced {
            return (None, None);
        }
        if let Some(last) = self.last_at
            && now - last < SYNC_ANNOUNCE_EVERY
        {
            return (None, Some(last + SYNC_ANNOUNCE_EVERY));
        }
        let text = match mood {
            SyncMood::Normal => "Sync recovered".to_owned(),
            SyncMood::Failed(kinds) => format!("Sync failed: {kinds}"),
            SyncMood::Throttled => "Azure DevOps is throttling requests; sync waits".to_owned(),
            SyncMood::Offline => "Offline; sync waits until you reconnect".to_owned(),
        };
        self.announced = mood.clone();
        self.last_at = Some(now);
        (Some(text), None)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::activity::fixtures;
    use adoken_sync::ledger::NetLog;

    fn log(entries: Vec<Entry>) -> NetLog {
        NetLog::from_entries(
            Arc::new(adoken_sync::ManualClock::new(fixtures::NOW)),
            entries,
        )
    }

    fn shown(sample: &Sample) -> (bool, Option<&str>, Option<Problems>) {
        match &sample.view {
            IndicatorView::Shown {
                busy,
                value,
                problems,
            } => (*busy, value.as_deref(), *problems),
            other => panic!("expected shown, got {other:?}"),
        }
    }

    #[test]
    fn one_503_is_danger_until_seen_or_five_minutes_old() {
        // AC-B7.
        let now = fixtures::NOW;
        let failed = fixtures::entry(1, now - 1_000, Status::Http(503), fixtures::lane_origin());
        let log = log(vec![failed]);
        let mut indicator = Indicator::default();
        let sample = indicator.sample(&log.summary(now), 0, Visibility::Shown, now);
        let (busy, value, problems) = shown(&sample);
        assert!(!busy);
        assert_eq!(value, Some("503"));
        let problems = problems.expect("danger chip");
        assert_eq!((problems.tone, problems.count()), (ProblemTone::Danger, 1));
        assert!(sample.view.label().contains("1 failed requests"));
        assert_eq!(sample.wake_at, Some(now - 1_000 + PROBLEM_WINDOW));
        // Opening the Network tab marks it seen.
        let seen = indicator.sample(&log.summary(now), 1, Visibility::Shown, now);
        assert_eq!(shown(&seen).2, None, "quiet once seen");
        // Unseen but 5 minutes old: quiet.
        let later = now - 1_000 + PROBLEM_WINDOW + 1;
        let aged = indicator.sample(&log.summary(later), 0, Visibility::Shown, later);
        assert_eq!(shown(&aged).2, None, "aged out");
    }

    #[test]
    fn a_404_alone_warns_and_a_background_avatar_404_does_not() {
        let now = fixtures::NOW;
        let missing = fixtures::entry(1, now, Status::Http(404), fixtures::lane_origin());
        let sample = Indicator::default().sample(
            &log(vec![missing]).summary(now),
            0,
            Visibility::Shown,
            now,
        );
        assert_eq!(shown(&sample).2.map(|p| p.tone), Some(ProblemTone::Warning));
        // Q-10: an avatar 404 fetched in the background is logged only.
        let avatar = fixtures::avatar_entry(1, now, Status::Http(404));
        let sample =
            Indicator::default().sample(&log(vec![avatar]).summary(now), 0, Visibility::Shown, now);
        let (_, value, problems) = shown(&sample);
        assert_eq!(problems, None, "tone unchanged");
        assert_eq!(value, Some("404"), "still the last status");
    }

    #[test]
    fn busy_shows_after_150_ms_and_bridges_short_gaps() {
        let now = fixtures::NOW;
        let flying = fixtures::entry(1, now, Status::InFlight, fixtures::lane_origin());
        let log = log(vec![flying.clone()]);
        let mut indicator = Indicator::default();
        let early = indicator.sample(&log.summary(now + 100), 0, Visibility::Shown, now + 100);
        assert!(!shown(&early).0, "sub-150 ms: no spinner");
        assert_eq!(early.wake_at, Some(now + BUSY_AFTER));
        let busy = indicator.sample(&log.summary(now + 150), 0, Visibility::Shown, now + 150);
        assert!(shown(&busy).0, "busy after 150 ms");
        assert_eq!(shown(&busy).1, Some("1"));
        // Done: busy holds through a 500 ms bridge, then shows the status.
        log.record_fixture(Entry {
            status: Status::Http(200),
            elapsed_ms: Some(200),
            ..flying
        });
        let t = now + 400;
        let bridged = indicator.sample(&log.summary(t), 0, Visibility::Shown, t);
        assert!(shown(&bridged).0);
        assert_eq!(bridged.wake_at, Some(t + BUSY_BRIDGE));
        let idle = indicator.sample(
            &log.summary(t + BUSY_BRIDGE),
            0,
            Visibility::Shown,
            t + BUSY_BRIDGE,
        );
        assert_eq!(shown(&idle), (false, Some("200"), None));
        assert_eq!(idle.wake_at, None, "nothing pending: no timer");
    }

    #[test]
    fn offline_demo_and_the_setting_replace_or_hide_the_indicator() {
        assert_eq!(visibility(false, false, true, true), Visibility::Offline);
        assert_eq!(visibility(true, false, true, false), Visibility::Hidden);
        assert_eq!(visibility(false, true, true, false), Visibility::Hidden);
        assert_eq!(visibility(false, false, false, false), Visibility::Hidden);
        let summary = log(Vec::new()).summary(fixtures::NOW);
        let sample = Indicator::default().sample(&summary, 0, Visibility::Offline, fixtures::NOW);
        assert_eq!(sample.view, IndicatorView::Offline);
    }

    #[test]
    fn three_failures_within_30_s_announce_once() {
        // AC-S4: 200 requests with 3 failures within 30 s → one.
        let mut announcer = NetAnnouncer::default();
        let mut said = Vec::new();
        let mut danger = 0;
        for i in 0..200 {
            let now = fixtures::NOW + i * 100;
            if [20, 90, 150].contains(&i) {
                danger += 1;
            }
            if let (Some(text), _) =
                announcer.observe(danger, || "503, work items batch".into(), now)
            {
                said.push(text);
            }
        }
        assert_eq!(said, ["Azure DevOps request failed: 503, work items batch"]);
        // A failure past the window announces everything since the first
        // ("3 more": the two held back plus the new one).
        let (text, _) = announcer.observe(4, || unreachable!(), fixtures::NOW + 60_000);
        assert_eq!(text.as_deref(), Some("3 more requests failed"));
    }

    #[test]
    fn failed_then_synced_announces_exactly_twice() {
        // AC-S4: Failed → Synced → two polite announcements, even when the
        // recovery lands inside the 10 s window (held back, then said).
        let mut announcer = SyncAnnouncer::default();
        let t = fixtures::NOW;
        let mut said = Vec::new();
        let mut step = |mood: SyncMood, now: Millis| {
            let (text, wake) = announcer.observe(&mood, now);
            said.extend(text);
            wake
        };
        assert_eq!(step(SyncMood::Normal, t), None);
        step(SyncMood::Failed("Work items".into()), t + 1_000);
        let wake = step(SyncMood::Normal, t + 4_000);
        assert_eq!(wake, Some(t + 11_000));
        step(SyncMood::Normal, t + 11_000);
        step(SyncMood::Normal, t + 30_000);
        assert_eq!(said, ["Sync failed: Work items", "Sync recovered"]);
    }
}
