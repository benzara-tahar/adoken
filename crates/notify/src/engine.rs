//! The notification engine: ingestion (FR-1), noise removal (FR-2),
//! priorities and rules (FR-3/4), snooze (FR-5), held interruptions (FR-6),
//! local triage with undo (FR-10, AC-11), retention (FR-9).
//!
//! Pure and synchronous: callers pass events plus [`DeliveryEnv`], read the
//! returned [`Effects`], and persist [`Engine::take_changes`]. Time comes
//! only from the injected [`Clock`].

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use adoken_model::time::{DAY, MINUTE};
use adoken_model::{Millis, PersonId, RunResult, Subject, SyncEvent, SyncEventKind};
use serde::{Deserialize, Serialize};

use crate::classify::{EventContext, Kind, Priority, RuleKey, Source};
use crate::clock::Clock;
use crate::delivery::{
    Banner, BannerAction, DeliveryEnv, Effects, Interrupt, SoundCue, decide, immediate,
    release_time,
};
use crate::filter::Filter;
use crate::rules::{EventRef, Mute, RuleAction, evaluate};
use crate::schedule::TimeOfDay;
use crate::settings::NotifySettings;
use crate::snooze::{Snooze, SnoozeChoice};

pub type NotificationId = i64;

/// Events on one subject this close together form one burst: one row
/// update, at most one interruption (FR-2, AC-1).
pub const COALESCE_WINDOW: Millis = 2 * MINUTE;
/// Done items are pruned after this (FR-9).
pub const RETENTION: Millis = 30 * DAY;
/// Events older than this when they arrive (app was closed, backfill)
/// land in the inbox without interrupting.
pub const STALE_AFTER: Millis = 15 * MINUTE;
/// Depth of `z`.
const UNDO_DEPTH: usize = 64;

/// One event on its way in: a sync event (0002 FR-11) or a nudge (0006).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    /// Idempotency key (FR-1, AC-14).
    pub dedupe_key: String,
    pub kind: Kind,
    pub subject: Subject,
    pub actor: Option<PersonId>,
    pub self_caused: bool,
    pub at: Millis,
    pub summary: String,
    pub ctx: EventContext,
}

impl Incoming {
    pub fn from_sync(event: &SyncEvent, ctx: EventContext) -> Self {
        Self {
            dedupe_key: event.unique_key(),
            kind: Kind::Sync(event.kind),
            subject: event.subject.clone(),
            actor: event.actor.clone(),
            self_caused: event.self_caused,
            at: event.at,
            summary: event.summary.clone(),
            ctx,
        }
    }

    pub fn nudge(
        key: &str,
        subject: Subject,
        at: Millis,
        summary: String,
        ctx: EventContext,
    ) -> Self {
        Self {
            dedupe_key: format!("Nudge|{key}"),
            kind: Kind::Nudge,
            subject,
            actor: None,
            self_caused: false,
            at,
            summary,
            ctx,
        }
    }
}

/// One inbox row: everything that happened on one subject (FR-1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub id: NotificationId,
    pub subject: Subject,
    pub source: Source,
    /// Latest event's kind.
    pub kind: Kind,
    /// Most urgent priority among events since last read.
    pub priority: Priority,
    pub actor: Option<PersonId>,
    /// Latest event's summary.
    pub summary: String,
    /// Latest context (title, repo, pipeline, …).
    pub ctx: EventContext,
    /// Events since last read, per kind.
    pub kind_counts: Vec<(Kind, u32)>,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub read: bool,
    pub done_at: Option<Millis>,
    pub snooze: Option<Snooze>,
    /// Returned from a snooze (flow step 4 badge).
    pub snoozed_badge: bool,
    pub muted: bool,
    /// Interruption waiting for quiet hours / a window to end.
    pub held_until: Option<Millis>,
    /// Latest rule said "inbox only".
    pub skip_banner: bool,
    /// Last event time of the current burst.
    pub burst_at: Millis,
    /// Most urgent priority that already interrupted (or was held) in the
    /// current burst.
    pub burst_priority: Option<Priority>,
}

impl Notification {
    /// Visible in the inbox (the All tab).
    pub fn in_inbox(&self) -> bool {
        !self.muted && self.done_at.is_none() && self.snooze.is_none()
    }

    pub fn count(&self) -> u32 {
        self.kind_counts.iter().map(|(_, n)| n).sum()
    }

    /// "3 new comments"; mixed kinds → "4 updates"; one event → "".
    pub fn count_label(&self) -> String {
        match self.kind_counts.as_slice() {
            [] => String::new(),
            [(_, 1)] => String::new(),
            [(kind, n)] => kind.count_label(*n),
            _ => format!("{} updates", self.count()),
        }
    }

    fn event_ref(&self) -> EventRef<'_> {
        EventRef {
            kind: self.kind,
            subject: &self.subject,
            actor: self.actor.as_deref(),
            ctx: &self.ctx,
        }
    }

    fn banner(&self) -> Banner {
        let label = self.count_label();
        Banner {
            title: if self.ctx.title.is_empty() {
                self.source.label().to_owned()
            } else {
                self.ctx.title.clone()
            },
            body: if label.is_empty() {
                self.summary.clone()
            } else {
                format!("{label} · {}", self.summary)
            },
            thread_id: self.subject.key(),
            notification: Some(self.id),
            actions: vec![BannerAction::Open, BannerAction::SnoozeOneHour],
        }
    }
}

/// Why an event didn't become (or update) a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Duplicate,
    SelfCaused,
    Muted,
    /// The pin is my only relation to it (spec 0014 FR-16): the pin's
    /// change dot reads the sync event, the inbox doesn't.
    PinOnly,
    /// Below the source's minimum priority.
    Filtered,
    Added(NotificationId),
    Updated(NotificationId),
}

/// An interruption waiting for its gate to open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub id: NotificationId,
    pub source: Source,
    pub priority: Priority,
    pub until: Millis,
}

/// Everything the engine persists (loaded at startup).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineState {
    pub settings: NotifySettings,
    pub filter: Filter,
    pub items: Vec<Notification>,
    /// Dedupe key → event time.
    pub seen: Vec<(String, Millis)>,
    pub mutes: Vec<Mute>,
    pub held: Vec<Held>,
    /// Last result per (pipeline definition, branch), for "still green".
    pub builds: Vec<((i64, String), RunResult)>,
    pub permission_notice_dismissed: bool,
}

/// Dirty state since the last [`Engine::take_changes`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub upserted: Vec<Notification>,
    pub deleted: Vec<NotificationId>,
    pub seen: Vec<(String, Millis)>,
    pub seen_pruned_before: Option<Millis>,
    pub builds: Vec<((i64, String), RunResult)>,
    /// Whole-value replacements (small).
    pub mutes: Option<Vec<Mute>>,
    pub held: Option<Vec<Held>>,
    pub settings: Option<NotifySettings>,
    pub filter: Option<Filter>,
    pub permission_notice_dismissed: Option<bool>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Default)]
struct Dirty {
    items: BTreeSet<NotificationId>,
    deleted: Vec<NotificationId>,
    seen: Vec<(String, Millis)>,
    seen_pruned_before: Option<Millis>,
    builds: Vec<((i64, String), RunResult)>,
    mutes: bool,
    held: bool,
    settings: bool,
    filter: bool,
    permission: bool,
}

struct UndoEntry {
    items: Vec<Notification>,
    mutes: Option<Vec<Mute>>,
}

pub struct Engine {
    clock: Arc<dyn Clock>,
    settings: NotifySettings,
    filter: Filter,
    items: Vec<Notification>,
    by_subject: HashMap<Subject, usize>,
    by_id: HashMap<NotificationId, usize>,
    seen: HashMap<String, Millis>,
    mutes: Vec<Mute>,
    held: Vec<Held>,
    builds: HashMap<(i64, String), RunResult>,
    next_id: NotificationId,
    permission_notice_dismissed: bool,
    undo: Vec<UndoEntry>,
    dirty: Dirty,
}

impl Engine {
    pub fn new(clock: Arc<dyn Clock>, settings: NotifySettings) -> Self {
        Self::from_state(
            clock,
            EngineState {
                settings,
                ..EngineState::default()
            },
        )
    }

    pub fn from_state(clock: Arc<dyn Clock>, state: EngineState) -> Self {
        let mut engine = Self {
            clock,
            settings: state.settings,
            filter: state.filter,
            next_id: state.items.iter().map(|n| n.id).max().unwrap_or(0) + 1,
            items: state.items,
            by_subject: HashMap::new(),
            by_id: HashMap::new(),
            seen: state.seen.into_iter().collect(),
            mutes: state.mutes,
            held: state.held,
            builds: state.builds.into_iter().collect(),
            permission_notice_dismissed: state.permission_notice_dismissed,
            undo: Vec::new(),
            dirty: Dirty::default(),
        };
        engine.reindex();
        engine
    }

    fn reindex(&mut self) {
        self.by_subject = self
            .items
            .iter()
            .enumerate()
            .map(|(i, n)| (n.subject.clone(), i))
            .collect();
        self.by_id = self
            .items
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
    }

    pub fn now(&self) -> Millis {
        self.clock.now()
    }

    pub fn clock(&self) -> &dyn Clock {
        &*self.clock
    }

    // ── settings / filter ──────────────────────────────────────────────

    pub fn settings(&self) -> &NotifySettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: NotifySettings) {
        self.settings = settings;
        self.dirty.settings = true;
    }

    pub fn filter(&self) -> Filter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: Filter) {
        if filter != self.filter {
            self.filter = filter;
            self.dirty.filter = true;
        }
    }

    // ── ingestion ──────────────────────────────────────────────────────

    /// FR-1/FR-2/FR-3/FR-4/FR-6 for one event. Interruptions are appended to
    /// `effects`.
    pub fn ingest(&mut self, event: Incoming, env: &DeliveryEnv, effects: &mut Effects) -> Outcome {
        self.ingest_inner(event, Some((env, effects)))
    }

    /// Ingest without interrupting: the first-run baseline (spec 0005
    /// States: first sync) lands in the inbox, never as banners.
    pub fn ingest_silently(&mut self, event: Incoming) -> Outcome {
        self.ingest_inner(event, None)
    }

    fn ingest_inner(
        &mut self,
        event: Incoming,
        interrupt: Option<(&DeliveryEnv, &mut Effects)>,
    ) -> Outcome {
        if event.ctx.pin_only {
            return Outcome::PinOnly;
        }
        if self.seen.contains_key(&event.dedupe_key) {
            return Outcome::Duplicate;
        }
        self.seen.insert(event.dedupe_key.clone(), event.at);
        self.dirty.seen.push((event.dedupe_key.clone(), event.at));

        let still_green = self.record_build(&event);
        if event.self_caused && !self.settings.show_self_caused {
            return Outcome::SelfCaused;
        }
        let event_ref = EventRef {
            kind: event.kind,
            subject: &event.subject,
            actor: event.actor.as_deref(),
            ctx: &event.ctx,
        };
        if self.mutes.iter().any(|mute| mute.matches(&event_ref)) {
            return Outcome::Muted;
        }
        let source = event.kind.source();
        let mut priority = self
            .settings
            .priorities
            .get(RuleKey::of(event.kind, &event.ctx));
        // "Still green" is not news: P3, inbox only (FR-2, AC-10).
        let mut skip_banner = still_green;
        if still_green {
            priority = Priority::P3;
        }
        match evaluate(&self.settings.rules, &event_ref) {
            Some(RuleAction::SetPriority(p)) => priority = p,
            Some(RuleAction::Mute) => return Outcome::Muted,
            Some(RuleAction::SkipBanner) => skip_banner = true,
            None => {}
        }
        if !self.settings.admits(source, priority) {
            return Outcome::Filtered;
        }
        // Your own activity is shown only on request, and never interrupts.
        skip_banner |= event.self_caused;

        let event_at = event.at;
        let (index, outcome, may_interrupt) = self.upsert(event, source, priority, skip_banner);
        let fresh = self.clock.now() - event_at <= STALE_AFTER;
        if let (true, true, Some((env, effects))) = (may_interrupt, fresh, interrupt) {
            self.interrupt(index, env, effects);
        }
        outcome
    }

    /// Remembers the build result; true when a success follows a success on
    /// the same definition + branch.
    fn record_build(&mut self, event: &Incoming) -> bool {
        if event.kind != Kind::Sync(SyncEventKind::BuildCompleted) {
            return false;
        }
        let ctx = &event.ctx;
        let (Some(pipeline), Some(branch), Some(result)) =
            (ctx.pipeline_id, ctx.branch.clone(), ctx.build_result)
        else {
            return false;
        };
        let key = (pipeline, branch);
        let previous = self.builds.insert(key.clone(), result);
        self.dirty.builds.push((key, result));
        result == RunResult::Succeeded && previous == Some(RunResult::Succeeded)
    }

    /// Returns (row index, outcome, whether this event may interrupt).
    fn upsert(
        &mut self,
        event: Incoming,
        source: Source,
        priority: Priority,
        skip_banner: bool,
    ) -> (usize, Outcome, bool) {
        let Some(&index) = self.by_subject.get(&event.subject) else {
            let id = self.next_id;
            self.next_id += 1;
            let index = self.items.len();
            self.by_subject.insert(event.subject.clone(), index);
            self.by_id.insert(id, index);
            self.items.push(Notification {
                id,
                subject: event.subject,
                source,
                kind: event.kind,
                priority,
                actor: event.actor,
                summary: event.summary,
                ctx: event.ctx,
                kind_counts: vec![(event.kind, 1)],
                created_at: event.at,
                updated_at: event.at,
                read: false,
                done_at: None,
                snooze: None,
                snoozed_badge: false,
                muted: false,
                held_until: None,
                skip_banner,
                burst_at: event.at,
                burst_priority: None,
            });
            self.dirty.items.insert(id);
            return (index, Outcome::Added(id), true);
        };

        let n = &mut self.items[index];
        self.dirty.items.insert(n.id);
        // Done or muted-then-unmuted rows come back as fresh activity.
        let reopened = n.done_at.is_some() || n.muted;
        if reopened || n.read {
            n.kind_counts.clear();
            n.priority = priority;
            n.snoozed_badge = false;
        } else {
            n.priority = n.priority.min(priority);
        }
        if reopened {
            n.done_at = None;
            n.muted = false;
            n.burst_priority = None;
        }
        match n.kind_counts.iter_mut().find(|(k, _)| *k == event.kind) {
            Some((_, count)) => *count += 1,
            None => n.kind_counts.push((event.kind, 1)),
        }
        n.read = false;
        n.kind = event.kind;
        n.source = source;
        n.skip_banner = skip_banner;
        if event.at >= n.updated_at {
            n.updated_at = event.at;
            n.summary = event.summary;
            n.actor = event.actor;
            n.ctx = event.ctx;
        }

        let mut snoozed = false;
        match n.snooze {
            Some(Snooze::UntilChanged) if !event.self_caused => {
                n.snooze = None;
                n.snoozed_badge = true;
                n.burst_priority = None;
            }
            Some(_) => snoozed = true,
            None => {}
        }
        // A burst continues while events keep arriving within the window;
        // only a more urgent event may interrupt again.
        let in_burst = event.at - n.burst_at <= COALESCE_WINDOW;
        if !in_burst {
            n.burst_priority = None;
        }
        n.burst_at = n.burst_at.max(event.at);
        let louder = n.burst_priority.is_none_or(|already| priority < already);
        (index, Outcome::Updated(n.id), !snoozed && louder)
    }

    fn interrupt(&mut self, index: usize, env: &DeliveryEnv, effects: &mut Effects) {
        let now = self.clock.now();
        let n = &self.items[index];
        let decision = decide(
            n.priority,
            n.source,
            n.skip_banner,
            &self.settings,
            env,
            now,
            &*self.clock,
        );
        let (id, source, priority, kind) = (n.id, n.source, n.priority, n.kind);
        match decision {
            Interrupt::Silent => return,
            Interrupt::Now { banner, sound } => {
                if banner {
                    effects.banners.push(n.banner());
                }
                if sound {
                    effects.sounds.push(SoundCue::Event(kind, priority));
                }
            }
            Interrupt::Held { until } => self.hold(id, source, priority, until),
        }
        let n = &mut self.items[index];
        n.burst_priority = Some(n.burst_priority.map_or(priority, |p| p.min(priority)));
    }

    /// One held entry per notification: the most urgent priority wins.
    fn hold(&mut self, id: NotificationId, source: Source, priority: Priority, until: Millis) {
        match self.held.iter_mut().find(|h| h.id == id) {
            Some(held) => {
                held.priority = held.priority.min(priority);
                held.until = held.until.min(until);
            }
            None => self.held.push(Held {
                id,
                source,
                priority,
                until,
            }),
        }
        if let Some(&index) = self.by_id.get(&id) {
            self.items[index].held_until = Some(until);
            self.dirty.items.insert(id);
        }
        self.dirty.held = true;
    }

    // ── time passing ───────────────────────────────────────────────────

    /// Wakes due snoozes (once, even after a long sleep: AC-7) and releases
    /// held interruptions as one summary per source (FR-6, AC-4, AC-6). Call
    /// on a timer and on system wake.
    pub fn tick(&mut self, env: &DeliveryEnv) -> Effects {
        let now = self.clock.now();
        let mut effects = Effects::default();

        let due: Vec<usize> = (0..self.items.len())
            .filter(|&i| matches!(self.items[i].snooze, Some(Snooze::Until(t)) if t <= now))
            .collect();
        for index in due {
            let n = &mut self.items[index];
            n.snooze = None;
            n.read = false;
            n.snoozed_badge = true;
            n.burst_priority = None;
            self.dirty.items.insert(n.id);
            if !n.muted && n.done_at.is_none() {
                self.interrupt(index, env, &mut effects);
            }
        }

        self.release_held(now, env, &mut effects);
        effects
    }

    fn release_held(&mut self, now: Millis, env: &DeliveryEnv, effects: &mut Effects) {
        if !self.held.iter().any(|h| h.until <= now) {
            return;
        }
        let (due, later): (Vec<Held>, Vec<Held>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|h| h.until <= now);
        self.held = later;
        self.dirty.held = true;

        let mut by_source: Vec<(Source, Vec<Held>)> = Vec::new();
        for held in due {
            let Some(&index) = self.by_id.get(&held.id) else {
                continue;
            };
            let n = &mut self.items[index];
            n.held_until = None;
            self.dirty.items.insert(n.id);
            // Triaged while held: nothing to interrupt about.
            if !n.in_inbox() || n.read {
                continue;
            }
            match by_source.iter_mut().find(|(s, _)| *s == held.source) {
                Some((_, group)) => group.push(held),
                None => by_source.push((held.source, vec![held])),
            }
        }

        for (source, group) in by_source {
            let most_urgent = group
                .iter()
                .map(|h| h.priority)
                .min()
                .unwrap_or(Priority::P3);
            // Settings may have changed since holding: re-check the gates.
            match release_time(now, source, most_urgent, &self.settings, &*self.clock) {
                None => continue,
                Some(at) if at > now => {
                    for held in group {
                        self.hold(held.id, held.source, held.priority, at);
                    }
                    continue;
                }
                Some(_) => {}
            }
            let wants_sound = group
                .iter()
                .any(|h| self.settings.delivery.get(h.priority).sound());
            let Interrupt::Now { banner, sound } = immediate(wants_sound, env) else {
                continue;
            };
            if let [single] = group.as_slice() {
                let n = &self.items[self.by_id[&single.id]];
                if banner {
                    effects.banners.push(n.banner());
                }
                if sound {
                    effects
                        .sounds
                        .push(SoundCue::Event(n.kind, single.priority));
                }
            } else {
                if banner {
                    effects.banners.push(self.summary_banner(source, &group));
                }
                if sound {
                    effects.sounds.push(SoundCue::Digest);
                }
            }
        }
    }

    /// "While you were away" + "4 pipeline updates", or counts per kind.
    fn summary_banner(&self, source: Source, group: &[Held]) -> Banner {
        let mut per_kind: Vec<(Kind, u32)> = Vec::new();
        for held in group {
            let kind = self.items[self.by_id[&held.id]].kind;
            match per_kind.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, n)) => *n += 1,
                None => per_kind.push((kind, 1)),
            }
        }
        let body = per_kind
            .iter()
            .map(|(kind, n)| kind.count_label(*n))
            .collect::<Vec<_>>()
            .join(", ");
        Banner {
            title: "While you were away".into(),
            body,
            thread_id: format!("summary:{}", source.noun()),
            notification: None,
            actions: vec![BannerAction::Open],
        }
    }

    /// Earliest pending wake-up (snooze or held release), for scheduling the
    /// next [`Engine::tick`].
    pub fn next_wake(&self) -> Option<Millis> {
        let snoozes = self.items.iter().filter_map(|n| match n.snooze {
            Some(Snooze::Until(t)) => Some(t),
            _ => None,
        });
        snoozes.chain(self.held.iter().map(|h| h.until)).min()
    }

    /// Soonest snoozed item's return time, for the inbox-zero state.
    pub fn next_snooze_return(&self) -> Option<Millis> {
        self.items
            .iter()
            .filter(|n| !n.muted && n.done_at.is_none())
            .filter_map(|n| match n.snooze {
                Some(Snooze::Until(t)) => Some(t),
                _ => None,
            })
            .min()
    }

    /// End of the current quiet hours, for the statusbar.
    pub fn quiet_until(&self) -> Option<Millis> {
        let quiet = &self.settings.quiet_hours;
        let now = self.clock.now();
        if !quiet.enabled || !quiet.schedule.contains(now, &*self.clock) {
            return None;
        }
        quiet.schedule.next_end(now, &*self.clock)
    }

    /// FR-9: drop done items older than 30 days and their dedupe keys.
    pub fn prune(&mut self) {
        let cutoff = self.clock.now() - RETENTION;
        let before = self.items.len();
        let mut deleted = Vec::new();
        self.items.retain(|n| {
            let keep = n.done_at.is_none_or(|done| done >= cutoff);
            if !keep {
                deleted.push(n.id);
            }
            keep
        });
        self.seen.retain(|_, at| *at >= cutoff);
        self.dirty.seen_pruned_before = Some(cutoff);
        if self.items.len() != before {
            self.held.retain(|h| !deleted.contains(&h.id));
            self.dirty.held = true;
            self.dirty.deleted.extend(deleted);
            self.undo.clear();
            self.reindex();
        }
    }

    // ── triage (local only, FR-10) ─────────────────────────────────────

    pub fn get(&self, id: NotificationId) -> Option<&Notification> {
        self.by_id.get(&id).map(|&i| &self.items[i])
    }

    fn remember(&mut self, ids: &[NotificationId], mutes: bool) {
        let items = ids.iter().filter_map(|id| self.get(*id).cloned()).collect();
        self.undo.push(UndoEntry {
            items,
            mutes: mutes.then(|| self.mutes.clone()),
        });
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    fn edit(&mut self, id: NotificationId, f: impl FnOnce(&mut Notification)) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        self.remember(&[id], false);
        f(&mut self.items[index]);
        self.dirty.items.insert(id);
        true
    }

    /// Opening an item reads it (not an undoable triage action).
    pub fn open(&mut self, id: NotificationId) {
        if let Some(&index) = self.by_id.get(&id) {
            let n = &mut self.items[index];
            if !n.read || n.snoozed_badge {
                n.read = true;
                n.snoozed_badge = false;
                self.dirty.items.insert(id);
            }
        }
    }

    /// A nudge's condition cleared (0006): its row is done, unless newer
    /// non-nudge activity arrived on the subject since.
    pub fn resolve_nudge(&mut self, subject: &Subject) {
        let now = self.clock.now();
        let Some(&index) = self.by_subject.get(subject) else {
            return;
        };
        let n = &mut self.items[index];
        if n.kind == Kind::Nudge && n.done_at.is_none() {
            n.done_at = Some(now);
            n.read = true;
            self.dirty.items.insert(n.id);
        }
    }

    /// `e`.
    pub fn mark_done(&mut self, id: NotificationId) -> bool {
        let now = self.clock.now();
        self.edit(id, |n| {
            n.done_at = Some(now);
            n.read = true;
            n.snooze = None;
            n.snoozed_badge = false;
        })
    }

    /// `u`.
    pub fn toggle_read(&mut self, id: NotificationId) -> bool {
        self.edit(id, |n| {
            n.read = !n.read;
            if n.read {
                n.snoozed_badge = false;
            }
        })
    }

    /// `s` → menu choice. `morning` is the working-hours start (0006) or
    /// 09:00.
    pub fn snooze(&mut self, id: NotificationId, choice: SnoozeChoice, morning: TimeOfDay) -> bool {
        let snooze = choice.resolve(self.clock.now(), morning, &*self.clock);
        self.edit(id, |n| {
            n.snooze = Some(snooze);
            n.snoozed_badge = false;
            n.done_at = None;
        })
    }

    /// Back into the inbox now (from the Snoozed / Done tabs).
    pub fn restore(&mut self, id: NotificationId) -> bool {
        self.edit(id, |n| {
            n.snooze = None;
            n.done_at = None;
        })
    }

    /// `m`: mute this subject.
    pub fn mute_subject(&mut self, id: NotificationId) -> bool {
        let Some(n) = self.get(id) else {
            return false;
        };
        let mute = Mute::Subject {
            subject: n.subject.clone(),
            title: n.ctx.title.clone(),
        };
        self.mute(mute)
    }

    /// `Shift+M` scopes (and `m`): hides existing matching rows and drops
    /// future events (AC-9).
    pub fn mute(&mut self, mute: Mute) -> bool {
        let hit: Vec<NotificationId> = self
            .items
            .iter()
            .filter(|n| !n.muted && mute.matches(&n.event_ref()))
            .map(|n| n.id)
            .collect();
        self.remember(&hit, true);
        for id in &hit {
            let index = self.by_id[id];
            self.items[index].muted = true;
            self.dirty.items.insert(*id);
        }
        if !self.mutes.contains(&mute) {
            self.mutes.push(mute);
        }
        self.dirty.mutes = true;
        true
    }

    /// Future events come back; already-hidden rows stay hidden (AC-9).
    pub fn unmute(&mut self, mute: &Mute) -> bool {
        if !self.mutes.contains(mute) {
            return false;
        }
        self.remember(&[], true);
        self.mutes.retain(|m| m != mute);
        self.dirty.mutes = true;
        true
    }

    pub fn mutes(&self) -> &[Mute] {
        &self.mutes
    }

    /// `z`: restores the state before the last triage action exactly.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        for item in entry.items {
            if let Some(&index) = self.by_id.get(&item.id) {
                self.dirty.items.insert(item.id);
                self.items[index] = item;
            }
        }
        if let Some(mutes) = entry.mutes {
            self.mutes = mutes;
            self.dirty.mutes = true;
        }
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    // ── reads ──────────────────────────────────────────────────────────

    pub fn items(&self) -> &[Notification] {
        &self.items
    }

    /// Rows for `filter`, newest activity first.
    pub fn list(&self, filter: &Filter) -> Vec<&Notification> {
        let mut rows: Vec<&Notification> =
            self.items.iter().filter(|n| filter.matches(n)).collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        rows
    }

    /// Unread rows in the inbox (sidebar count).
    pub fn unread_count(&self) -> usize {
        self.items
            .iter()
            .filter(|n| n.in_inbox() && !n.read)
            .count()
    }

    /// Dock badge (flow step 6, AC-12).
    pub fn badge(&self) -> usize {
        let mode = self.settings.badge;
        self.items
            .iter()
            .filter(|n| n.in_inbox() && !n.read && mode.counts(n.priority))
            .count()
    }

    /// AC-13: the one-time "banners are off" prompt.
    pub fn permission_notice(&self, env: &DeliveryEnv) -> bool {
        env.permission == crate::delivery::OsPermission::Denied && !self.permission_notice_dismissed
    }

    pub fn dismiss_permission_notice(&mut self) {
        if !self.permission_notice_dismissed {
            self.permission_notice_dismissed = true;
            self.dirty.permission = true;
        }
    }

    /// Full state, for a first save or an export.
    pub fn state(&self) -> EngineState {
        EngineState {
            settings: self.settings.clone(),
            filter: self.filter,
            items: self.items.clone(),
            seen: self.seen.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            mutes: self.mutes.clone(),
            held: self.held.clone(),
            builds: self.builds.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            permission_notice_dismissed: self.permission_notice_dismissed,
        }
    }

    /// What changed since the last call, for the store writer.
    pub fn take_changes(&mut self) -> Changes {
        let dirty = std::mem::take(&mut self.dirty);
        Changes {
            upserted: dirty
                .items
                .iter()
                .filter_map(|id| self.get(*id).cloned())
                .collect(),
            deleted: dirty.deleted,
            seen: dirty.seen,
            seen_pruned_before: dirty.seen_pruned_before,
            builds: dirty.builds,
            mutes: dirty.mutes.then(|| self.mutes.clone()),
            held: dirty.held.then(|| self.held.clone()),
            settings: dirty.settings.then(|| self.settings.clone()),
            filter: dirty.filter.then_some(self.filter),
            permission_notice_dismissed: dirty
                .permission
                .then_some(self.permission_notice_dismissed),
        }
    }
}
