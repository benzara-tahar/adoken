//! One evaluation pass (spec 0006 FR-4, FR-6, FR-8): thresholds, staleness,
//! opt-outs, snooze, per-subject dedupe, the daily cap, and auto-resolution.
//!
//! Firing policy:
//! - A nudge fires once per *episode*: while its condition keeps holding it
//!   stays open (visible on the Today card and in the inbox) and is not
//!   re-sent. It can fire again only after it resolved and came back.
//! - A subject gets at most one nudge per 24 h across all types (FR-4,
//!   AC-4), and only its highest-priority nudge is considered at a time.
//! - At most `daily_cap` nudges fire per local calendar day, in FR-4
//!   priority order; the rest are returned as `held` and listed by the
//!   Today card, never queued.

use std::collections::{BTreeMap, HashSet};

use adoken_model::time::{DAY, HOUR};
use adoken_model::{Millis, Subject};

use crate::clock::Clock;
use crate::hours::WorkingHours;
use crate::kind::{NudgeKind, Settings};
use crate::rules::{self, Nudge};
use crate::snapshot::Snapshot;

/// Minimum gap between two nudges about the same subject (FR-4).
pub const SUBJECT_COOLDOWN: Millis = 24 * HOUR;

/// Local, user-authored nudge state for one subject (`pr:912`, `wi:48213`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubjectState {
    pub last_fired: Option<Millis>,
    /// Fired and not yet resolved.
    pub open: Vec<NudgeKind>,
    /// Snoozed via the 0005 menu.
    pub snoozed_until: Option<Millis>,
    /// Marked done while the condition still held; cleared once it resolves.
    pub dismissed: Vec<NudgeKind>,
    /// "Not for this item" (FR-8, AC-6).
    pub opted_out: bool,
}

impl SubjectState {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Everything nudges remember between passes and restarts (AC-4). Persisted
/// by [`crate::store`]; plain data so tests can round-trip it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    subjects: BTreeMap<String, SubjectState>,
    /// Fire times of the last two days (daily cap).
    fired: Vec<Millis>,
    /// Local day the Today card was dismissed on.
    today_dismissed_day: Option<i64>,
}

impl Ledger {
    pub fn from_parts(
        subjects: impl IntoIterator<Item = (String, SubjectState)>,
        fired: Vec<Millis>,
        today_dismissed_day: Option<i64>,
    ) -> Self {
        Self {
            subjects: subjects.into_iter().collect(),
            fired,
            today_dismissed_day,
        }
    }

    pub fn subjects(&self) -> impl Iterator<Item = (&str, &SubjectState)> {
        self.subjects.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn fired(&self) -> &[Millis] {
        &self.fired
    }

    pub fn today_dismissed_day(&self) -> Option<i64> {
        self.today_dismissed_day
    }

    pub fn subject(&self, subject: &Subject) -> Option<&SubjectState> {
        self.subjects.get(&subject.key())
    }

    fn entry(&mut self, subject: &Subject) -> &mut SubjectState {
        self.subjects.entry(subject.key()).or_default()
    }

    /// Nudges fired on `now`'s local day.
    pub fn fired_today(&self, now: Millis, hours: &WorkingHours) -> usize {
        let today = hours.local_day(now);
        self.fired
            .iter()
            .filter(|t| hours.local_day(**t) == today)
            .count()
    }

    /// Snooze every nudge about `subject` until `until`.
    pub fn snooze(&mut self, subject: &Subject, until: Millis) {
        self.entry(subject).snoozed_until = Some(until);
    }

    /// Mark a nudge done: it stays quiet until its condition resolves.
    pub fn dismiss(&mut self, subject: &Subject, kind: NudgeKind) {
        let state = self.entry(subject);
        state.open.retain(|k| *k != kind);
        if !state.dismissed.contains(&kind) {
            state.dismissed.push(kind);
        }
    }

    /// "Not for this item": no nudge about `subject` again.
    pub fn not_for_this_item(&mut self, subject: &Subject) {
        self.entry(subject).opted_out = true;
    }

    /// Undo "Not for this item".
    pub fn allow_item(&mut self, subject: &Subject) {
        self.entry(subject).opted_out = false;
    }

    /// AC-9: the Today card shows on workdays once working hours begin,
    /// until dismissed for the day.
    pub fn today_visible(&self, now: Millis, hours: &WorkingHours) -> bool {
        hours.workday_started(now) && self.today_dismissed_day != Some(hours.local_day(now))
    }

    /// `e` on the Today card: hidden until the next workday.
    pub fn dismiss_today(&mut self, now: Millis, hours: &WorkingHours) {
        self.today_dismissed_day = Some(hours.local_day(now));
    }
}

/// What one pass decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pass {
    /// Send these now (as P2 notifications, spec 0005).
    pub fired: Vec<Nudge>,
    /// Eligible but over today's cap: shown on the Today card only.
    pub held: Vec<Nudge>,
    /// Previously fired nudges whose condition stopped holding (FR-6): mark
    /// them done in the inbox.
    pub resolved: Vec<(Subject, NudgeKind)>,
    /// Everything currently past its threshold and not snoozed, dismissed or
    /// opted out; one per subject, FR-4 order. The Today card's list.
    pub active: Vec<Nudge>,
    /// Every rule condition holding now, regardless of threshold or
    /// suppression (FR-4 order). Input for [`crate::today`].
    pub conditions: Vec<Nudge>,
}

/// Delivery seam to the notification center (spec 0005). Implementations
/// only touch local state; nothing here may reach ADO or another person.
pub trait NudgeSink {
    fn deliver(&mut self, nudge: &Nudge, at: Millis);
    fn resolve(&mut self, subject: &Subject, kind: NudgeKind, at: Millis);
}

fn fresh(snap: &Snapshot, kind: NudgeKind) -> bool {
    kind.tiers().iter().all(|tier| !snap.health.is_stale(*tier))
}

/// Runs one pass at `now`, updating `ledger`. Pure apart from the ledger.
pub fn evaluate(snap: &Snapshot, settings: &Settings, ledger: &mut Ledger, now: Millis) -> Pass {
    let hours = &settings.hours;
    let all = rules::conditions(snap, hours, now);
    let mut pass = Pass {
        conditions: all.clone(),
        ..Pass::default()
    };
    if !snap.health.initial_sync_done {
        return pass;
    }
    let holding: HashSet<(String, NudgeKind)> =
        all.iter().map(|n| (n.subject.key(), n.kind)).collect();

    // FR-6: resolve open nudges whose trigger stopped (only when the data
    // behind it is fresh; stale data proves nothing).
    for (key, state) in &mut ledger.subjects {
        let subject = Subject::parse_key(key);
        let still = |kind: &NudgeKind| {
            !fresh(snap, *kind)
                || (settings.enabled(*kind)
                    && !state.opted_out
                    && holding.contains(&(key.clone(), *kind)))
        };
        let (keep, gone): (Vec<_>, Vec<_>) = state.open.iter().partition(|k| still(k));
        if let Some(subject) = subject.filter(|_| !gone.is_empty()) {
            pass.resolved
                .extend(gone.into_iter().map(|k| (subject.clone(), k)));
        }
        state.open = keep;
        state
            .dismissed
            .retain(|k| !fresh(snap, *k) || holding.contains(&(key.clone(), *k)));
    }

    let candidates: Vec<Nudge> = all
        .into_iter()
        .filter(|n| {
            settings.enabled(n.kind)
                && fresh(snap, n.kind)
                && n.waited >= settings.threshold_ms(n.kind)
        })
        .filter(|n| {
            ledger.subject(&n.subject).is_none_or(|s| {
                !s.opted_out
                    && s.snoozed_until.is_none_or(|until| until <= now)
                    && !s.dismissed.contains(&n.kind)
            })
        })
        .collect();
    pass.active = rules::best_per_subject(candidates);

    let mut budget =
        usize::from(settings.daily_cap()).saturating_sub(ledger.fired_today(now, hours));
    for nudge in &pass.active {
        let state = ledger.subject(&nudge.subject);
        if state.is_some_and(|s| s.open.contains(&nudge.kind)) {
            continue;
        }
        if state
            .and_then(|s| s.last_fired)
            .is_some_and(|t| now - t < SUBJECT_COOLDOWN)
        {
            continue;
        }
        if budget == 0 {
            pass.held.push(nudge.clone());
            continue;
        }
        budget -= 1;
        let state = ledger.entry(&nudge.subject);
        state.open.push(nudge.kind);
        state.last_fired = Some(now);
        ledger.fired.push(now);
        pass.fired.push(nudge.clone());
    }

    ledger.fired.retain(|t| now - t < 2 * DAY);
    ledger.subjects.retain(|_, s| {
        s.snoozed_until.is_some_and(|until| until > now) || {
            let expired_fire = s.last_fired.is_some_and(|t| now - t >= SUBJECT_COOLDOWN);
            if expired_fire && s.open.is_empty() {
                s.last_fired = None;
            }
            if s.snoozed_until.is_some_and(|until| until <= now) {
                s.snoozed_until = None;
            }
            !s.is_empty()
        }
    });
    pass
}

/// [`evaluate`] at the clock's now, delivering to `sink`.
pub fn run_pass(
    snap: &Snapshot,
    settings: &Settings,
    ledger: &mut Ledger,
    clock: &dyn Clock,
    sink: &mut dyn NudgeSink,
) -> Pass {
    let now = clock.now();
    let pass = evaluate(snap, settings, ledger, now);
    for (subject, kind) in &pass.resolved {
        sink.resolve(subject, *kind, now);
    }
    for nudge in &pass.fired {
        sink.deliver(nudge, now);
    }
    pass
}
