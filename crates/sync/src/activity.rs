//! Sync activity (spec 0016 group A): what is syncing now, what the last
//! passes changed, and each kind's row state.
//!
//! - [`Tracker`]: per-lane activity (running, progress, last success,
//!   failure, next due), the running passes and a ring of finished
//!   [`PassRecord`]s (200 passes or 2 hours, FR-13). The engine and the
//!   scheduler feed it; the app reads it.
//! - [`ChangeSet`]: exact change counts per kind (FR-12), recorded by the
//!   engine's appliers inside each page transaction through a thread-local
//!   collector ([`collect_changes`]) and merged into the pass only after
//!   the transaction committed.
//! - [`rows`]: FR-8's row state as a pure function of [`SyncInputs`].
//! - [`Signal`] and [`Coalescer`]: change notification and the ≤ 4 Hz
//!   trailing-edge publish rule (FR-6).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use adoken_model::time::HOUR;
use adoken_model::{Millis, RunResult};

use crate::attribution::{self, Attribution, Origin, PassCounters, PassLink, PassRef, UserAction};
use crate::cadence::{Background, Cadence, Kind, Lane, Lanes, Prefs};
use crate::clock::{Clock, SystemClock};
use crate::engine::SyncError;
use crate::governor::{GovError, GovSnapshot, Governor, Hold, Pressure};

/// Pass records kept (FR-13): this many, or [`MAX_PASS_AGE_MS`], whichever
/// is fewer.
pub const MAX_PASSES: usize = 200;
pub const MAX_PASS_AGE_MS: Millis = 2 * HOUR;
/// Ids kept per kind per pass (FR-12); the rest is "+N more".
pub const MAX_IDS: usize = 20;
/// FR-6: at most 4 publishes a second.
pub const PUBLISH_INTERVAL_MS: Millis = 250;

// ── change notification ─────────────────────────────────────────────────

/// A generation counter plus a wakeup for the activity publisher. Bumping
/// is two atomics; nothing runs while nothing changes.
#[derive(Debug, Default)]
pub struct Signal {
    generation: AtomicU64,
    notify: tokio::sync::Notify,
}

impl Signal {
    pub fn bump(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.notify.notify_one();
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Resolves after a [`Self::bump`] (one stored wakeup if none waited).
    pub async fn changed(&self) {
        self.notify.notified().await;
    }
}

/// FR-6 trailing-edge throttle: the first change publishes at once, later
/// ones at most every `interval`, and the last change always publishes.
#[derive(Clone, Debug)]
pub struct Coalescer {
    interval: Millis,
    last_publish: Option<Millis>,
    wake_at: Option<Millis>,
}

impl Coalescer {
    pub fn new(interval: Millis) -> Self {
        Coalescer {
            interval,
            last_publish: None,
            wake_at: None,
        }
    }

    /// Something changed at `now`: when to publish.
    pub fn change(&mut self, now: Millis) -> Millis {
        *self.wake_at.get_or_insert_with(|| {
            self.last_publish
                .map_or(now, |last| (last + self.interval).max(now))
        })
    }

    /// The timer fired at `now`: publish?
    pub fn tick(&mut self, now: Millis) -> bool {
        match self.wake_at {
            Some(at) if at <= now => {
                self.wake_at = None;
                self.last_publish = Some(now);
                true
            }
            _ => false,
        }
    }

    pub fn pending(&self) -> Option<Millis> {
        self.wake_at
    }
}

// ── what changed (FR-12) ────────────────────────────────────────────────

/// Why an item is listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeWord {
    New,
    Updated,
    NewComment,
    VoteChanged,
    Pushed,
    Completed,
    Abandoned,
    /// Work item state ("Active").
    State(String),
    AssignedToYou,
    /// Deleted or left the synced set.
    Removed,
    Started,
    Finished(RunResult),
}

/// Changes of one kind in one pass.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KindChanges {
    pub new: u32,
    pub updated: u32,
    /// PRs completed/abandoned, work items removed, runs finished.
    pub removed: u32,
    /// The first [`MAX_IDS`] items, in the order they changed.
    pub ids: Vec<(i64, ChangeWord)>,
    /// Changed items beyond `ids`.
    pub more: u32,
}

impl KindChanges {
    pub fn total(&self) -> u32 {
        self.new + self.updated + self.removed
    }
}

/// What a pass changed, per kind (FR-12). Counts are exact.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    pub prs: KindChanges,
    pub pr_comments: u32,
    pub work_items: KindChanges,
    pub wi_comments: u32,
    pub runs: KindChanges,
    pub index_entries: u32,
    pub projects: u32,
    pub repos: u32,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        *self == Changes::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Updated,
    Removed,
    New,
}

fn class_of(word: &ChangeWord) -> Class {
    match word {
        ChangeWord::New | ChangeWord::Started => Class::New,
        ChangeWord::Completed
        | ChangeWord::Abandoned
        | ChangeWord::Removed
        | ChangeWord::Finished(_) => Class::Removed,
        _ => Class::Updated,
    }
}

/// Distinct changed ids of one kind, with the strongest change per id (a
/// new item stays "New" however often it changed in the pass).
#[derive(Clone, Debug, Default)]
struct KindSet {
    order: Vec<i64>,
    words: HashMap<i64, ChangeWord>,
}

impl KindSet {
    fn note(&mut self, id: i64, word: ChangeWord) {
        match self.words.get_mut(&id) {
            None => {
                self.order.push(id);
                self.words.insert(id, word);
            }
            Some(old) if class_of(&word) > class_of(old) => *old = word,
            Some(_) => {}
        }
    }

    fn merge(&mut self, other: KindSet) {
        let KindSet { order, mut words } = other;
        for id in order {
            if let Some(word) = words.remove(&id) {
                self.note(id, word);
            }
        }
    }

    fn finish(&self) -> KindChanges {
        let mut out = KindChanges::default();
        for id in &self.order {
            let word = &self.words[id];
            match class_of(word) {
                Class::New => out.new += 1,
                Class::Updated => out.updated += 1,
                Class::Removed => out.removed += 1,
            }
            if out.ids.len() < MAX_IDS {
                out.ids.push((*id, word.clone()));
            } else {
                out.more += 1;
            }
        }
        out
    }
}

/// Changes being collected (one page, or a whole pass).
#[derive(Clone, Debug, Default)]
pub struct ChangeSet {
    prs: KindSet,
    pr_comments: u32,
    work_items: KindSet,
    wi_comments: u32,
    runs: KindSet,
    index_entries: u32,
    projects: u32,
    repos: u32,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.prs.order.is_empty()
            && self.work_items.order.is_empty()
            && self.runs.order.is_empty()
            && self.pr_comments == 0
            && self.wi_comments == 0
            && self.index_entries == 0
            && self.projects == 0
            && self.repos == 0
    }

    pub fn note_pr(&mut self, id: i64, word: ChangeWord) {
        self.prs.note(id, word);
    }

    pub fn note_pr_comments(&mut self, pr: i64, n: u32) {
        if n > 0 {
            self.pr_comments += n;
            self.prs.note(pr, ChangeWord::NewComment);
        }
    }

    pub fn note_work_item(&mut self, id: i64, word: ChangeWord) {
        self.work_items.note(id, word);
    }

    pub fn note_wi_comment(&mut self, work_item: i64, word: ChangeWord) {
        self.wi_comments += 1;
        self.work_items.note(work_item, word);
    }

    pub fn note_run(&mut self, id: i64, word: ChangeWord) {
        self.runs.note(id, word);
    }

    pub fn note_index_entry(&mut self) {
        self.index_entries += 1;
    }

    pub fn note_project(&mut self) {
        self.projects += 1;
    }

    pub fn note_repo(&mut self) {
        self.repos += 1;
    }

    pub fn merge(&mut self, other: ChangeSet) {
        self.prs.merge(other.prs);
        self.work_items.merge(other.work_items);
        self.runs.merge(other.runs);
        self.pr_comments += other.pr_comments;
        self.wi_comments += other.wi_comments;
        self.index_entries += other.index_entries;
        self.projects += other.projects;
        self.repos += other.repos;
    }

    pub fn finish(&self) -> Changes {
        Changes {
            prs: self.prs.finish(),
            pr_comments: self.pr_comments,
            work_items: self.work_items.finish(),
            wi_comments: self.wi_comments,
            runs: self.runs.finish(),
            index_entries: self.index_entries,
            projects: self.projects,
            repos: self.repos,
        }
    }
}

struct Collector {
    set: ChangeSet,
    /// Work item upserts count as index entries (the revisions feed).
    as_index: bool,
}

thread_local! {
    static COLLECTOR: RefCell<Option<Collector>> = const { RefCell::new(None) };
}

/// Run `f` (a store write) collecting the changes the appliers note inside
/// it. Use the set only if `f` committed.
pub fn collect_changes<R>(f: impl FnOnce() -> R) -> (R, ChangeSet) {
    let outer = COLLECTOR.with(|c| {
        c.borrow_mut().replace(Collector {
            set: ChangeSet::default(),
            as_index: false,
        })
    });
    let r = f();
    let set = COLLECTOR.with(|c| {
        let mut c = c.borrow_mut();
        let mine = c.take().map(|c| c.set).unwrap_or_default();
        *c = outer;
        mine
    });
    (r, set)
}

/// Note a change into the running collector (no-op outside one).
pub(crate) fn note(f: impl FnOnce(&mut ChangeSet, bool)) {
    COLLECTOR.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            f(&mut c.set, c.as_index);
        }
    });
}

/// Run `f` with work item changes counted as index entries.
pub(crate) fn as_index<R>(f: impl FnOnce() -> R) -> R {
    let set = |v: bool| {
        COLLECTOR.with(|c| {
            if let Some(c) = c.borrow_mut().as_mut() {
                c.as_index = v;
            }
        })
    };
    set(true);
    let r = f();
    set(false);
    r
}

// ── passes (FR-11, FR-13) ───────────────────────────────────────────────

/// What started a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Scheduled,
    /// ⌘R.
    SyncNow,
    /// Wake or reconnect.
    CatchUp,
    /// A write's re-read (0002 FR-15).
    Reread(UserAction),
    /// A completing PR's re-read (0004 FR-10).
    Completing(i64),
    LoadOlderRuns,
    FirstSync,
    /// "Sync this item" (0015 FR-30/31): "You: Sync #20101".
    ItemSync(i64),
    /// User-tier fetches of an open PR.
    OpenPr(i64),
    /// Demo mode's local feed (FR-15).
    DemoFeed,
}

impl Trigger {
    /// The origin of requests in a pass so triggered, for `lane`.
    pub fn origin(self, lane: Lane, pinned: bool) -> Origin {
        match self {
            Trigger::Reread(action) => Origin::YouReread(action),
            Trigger::Completing(_) => Origin::YouReread(UserAction::Complete),
            Trigger::LoadOlderRuns => Origin::You(UserAction::LoadOlderRuns),
            Trigger::ItemSync(id) => Origin::You(UserAction::SyncItem(id)),
            Trigger::OpenPr(_) => Origin::You(UserAction::OpenPr),
            Trigger::Scheduled
            | Trigger::SyncNow
            | Trigger::CatchUp
            | Trigger::FirstSync
            | Trigger::DemoFeed => Origin::Sync {
                lane,
                pinned,
                sync_now: self == Trigger::SyncNow,
            },
        }
    }
}

/// Why a pass or lane failed (FR-8 item 7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailReason {
    /// Azure DevOps answered an error status.
    Status {
        status: u16,
        code: Option<String>,
        message: Option<String>,
    },
    /// "Couldn't reach dev.azure.com".
    Unreachable {
        host: String,
    },
    /// "Unexpected response, adoken may need an update".
    Decode,
    Unauthorized,
    Other(String),
}

impl FailReason {
    pub fn of(error: &SyncError, host: &str) -> FailReason {
        match error {
            SyncError::Gov(GovError::Status {
                status, message, ..
            }) => FailReason::Status {
                status: *status,
                code: message.as_deref().and_then(crate::ledger::error_code),
                message: message.clone(),
            },
            SyncError::Gov(GovError::Offline(_)) => FailReason::Unreachable {
                host: host.to_owned(),
            },
            SyncError::Gov(GovError::Unauthorized) => FailReason::Unauthorized,
            SyncError::Decode { .. } => FailReason::Decode,
            other => FailReason::Other(other.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Running,
    Ok,
    Failed(FailReason),
    /// Held by Azure DevOps (`Retry-After` / 429 backoff) until then.
    Throttled {
        until: Millis,
    },
    /// Deferred by rate pressure.
    Deferred,
    /// Stopped by offline or sleep.
    Canceled,
}

impl Outcome {
    pub fn of(result: &Result<(), &SyncError>, host: &str) -> Outcome {
        match result {
            Ok(()) => Outcome::Ok,
            Err(SyncError::Gov(GovError::Throttled { until })) => {
                Outcome::Throttled { until: *until }
            }
            Err(SyncError::Gov(GovError::Deferred)) => Outcome::Deferred,
            Err(SyncError::Gov(GovError::Offline(_))) => Outcome::Canceled,
            Err(e) => Outcome::Failed(FailReason::of(e, host)),
        }
    }
}

/// One pass (FR-11).
#[derive(Clone, Debug, PartialEq)]
pub struct PassRecord {
    pub id: u64,
    pub started: Millis,
    /// `None` while running.
    pub ended: Option<Millis>,
    pub lanes: Lanes,
    pub trigger: Trigger,
    pub outcome: Outcome,
    pub requests: u32,
    pub tstu: f64,
    /// Sync events persisted (0002 FR-11): "2 notifications".
    pub events: usize,
    pub changes: Changes,
}

impl PassRecord {
    pub fn pass_ref(&self) -> PassRef {
        PassRef {
            id: self.id,
            started: self.started,
        }
    }
}

// ── lanes and rows (FR-7, FR-8) ─────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressUnit {
    PullRequests,
    Projects,
    WorkItems,
    Timelines,
    IndexItems,
}

/// "3 of 12 pull requests"; `total: None` = a count so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u32,
    pub total: Option<u32>,
    pub unit: ProgressUnit,
}

/// What the scheduler and engine know about one lane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneActivity {
    pub last_ok: Option<Millis>,
    /// The scheduler's planned due time (jitter, background, pause and
    /// pressure applied); `None`: no scheduled pass.
    pub next_due: Option<Millis>,
    pub running: bool,
    pub progress: Option<Progress>,
    /// The last attempt failed: why, and when the backoff retries.
    pub failure: Option<(FailReason, Option<Millis>)>,
}

/// The Pinned items row (FR-7, AC-A6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PinsRow {
    pub pins: u32,
    pub unavailable: u32,
    /// The last warm pass whose pinned steps all ran.
    pub last_full_refresh: Option<Millis>,
}

/// FR-7 rows, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowKind {
    MyPrs,
    AllPrs,
    WorkItems,
    Pipelines,
    Index,
    Pinned,
}

impl RowKind {
    pub const ALL: [RowKind; 6] = [
        RowKind::MyPrs,
        RowKind::AllPrs,
        RowKind::WorkItems,
        RowKind::Pipelines,
        RowKind::Index,
        RowKind::Pinned,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RowKind::MyPrs => "My pull requests",
            RowKind::AllPrs => "All active pull requests",
            RowKind::WorkItems => "Work items",
            RowKind::Pipelines => "Pipeline runs",
            RowKind::Index => "Search index and projects",
            RowKind::Pinned => "Pinned items",
        }
    }

    /// The scheduler lanes behind the row (FR-7).
    pub fn lanes(self) -> &'static [Lane] {
        match self {
            RowKind::MyPrs => &[Lane::MyPrs],
            RowKind::AllPrs => &[Lane::AllPrs],
            RowKind::WorkItems => &[Lane::WorkItems],
            RowKind::Pipelines => &[Lane::LiveRuns, Lane::DoneRuns],
            RowKind::Index => &[Lane::Index],
            RowKind::Pinned => &[
                Lane::AllPrs,
                Lane::WorkItems,
                Lane::LiveRuns,
                Lane::DoneRuns,
            ],
        }
    }

    /// The Settings › Sync row that sets the cadence.
    pub fn kind(self) -> Option<Kind> {
        match self {
            RowKind::MyPrs => Some(Kind::MyPrs),
            RowKind::AllPrs => Some(Kind::AllPrs),
            RowKind::WorkItems => Some(Kind::WorkItems),
            RowKind::Pipelines => Some(Kind::Pipelines),
            RowKind::Index | RowKind::Pinned => None,
        }
    }

    /// The token scope the kind needs (0001 §5.2, 0014 FR-1).
    pub fn scope(self) -> Option<&'static str> {
        match self {
            RowKind::MyPrs | RowKind::AllPrs => Some("vso.code"),
            RowKind::WorkItems => Some("vso.work"),
            RowKind::Pipelines => Some("vso.build"),
            RowKind::Index | RowKind::Pinned => None,
        }
    }
}

/// Scopes of `granted` that would satisfy `scope` (write/full/execute
/// variants include the read scope).
fn scope_granted(scope: &str, granted: &[String]) -> bool {
    granted.iter().any(|s| {
        s == scope
            || s.strip_prefix(scope).is_some_and(|rest| {
                matches!(
                    rest,
                    "_write" | "_full" | "_execute" | "_manage" | "_status"
                )
            })
    })
}

/// The sync scopes `granted` lacks; nothing when the scopes are unknown
/// (empty list).
pub fn missing_scopes(granted: &[String]) -> Vec<String> {
    if granted.is_empty() {
        return Vec::new();
    }
    ["vso.code", "vso.work", "vso.build"]
        .into_iter()
        .filter(|s| !scope_granted(s, granted))
        .map(str::to_owned)
        .collect()
}

/// FR-8 state, highest precedence first.
#[derive(Clone, Debug, PartialEq)]
pub enum RowState {
    TokenRejected,
    NeedsScope(&'static str),
    Offline,
    WaitingForAdo {
        until: Millis,
        resource: Option<String>,
    },
    WaitingForBudget {
        until: Millis,
    },
    Syncing(Option<Progress>),
    Failed {
        reason: FailReason,
        retry_at: Option<Millis>,
    },
    Paused {
        until: Option<Millis>,
    },
    PausedInBackground,
    Slowed {
        remaining_pct: Option<f64>,
        resets_at: Option<Millis>,
    },
    ManualOnly,
    Synced,
    NotSyncedYet,
}

/// Everything [`rows`] reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncInputs {
    pub lanes: [LaneActivity; 6],
    pub gov: GovSnapshot,
    pub hold: Option<Hold>,
    pub pressure: Option<Pressure>,
    /// `X-RateLimit-Remaining / Limit` in percent, when last sent.
    pub remaining_pct: Option<f64>,
    pub prefs: Prefs,
    /// Sync scopes the token lacks ([`missing_scopes`]).
    pub missing_scopes: Vec<String>,
    pub online: bool,
    pub foreground: bool,
    pub pins: PinsRow,
}

/// One FR-7 row.
#[derive(Clone, Debug, PartialEq)]
pub struct KindRow {
    pub kind: RowKind,
    pub state: RowState,
    pub last_ok: Option<Millis>,
    /// The sooner next pass of the row's lanes.
    pub next_due: Option<Millis>,
    /// Settings' cadence (`None`: fixed or derived).
    pub cadence: Option<Cadence>,
}

fn rank(state: &RowState) -> u8 {
    match state {
        RowState::TokenRejected => 0,
        RowState::NeedsScope(_) => 1,
        RowState::Offline => 2,
        RowState::WaitingForAdo { .. } => 3,
        RowState::WaitingForBudget { .. } => 4,
        RowState::Syncing(_) => 5,
        RowState::Failed { .. } => 6,
        RowState::Paused { .. } => 7,
        RowState::PausedInBackground => 8,
        RowState::Slowed { .. } => 9,
        RowState::ManualOnly => 10,
        RowState::Synced => 11,
        RowState::NotSyncedYet => 12,
    }
}

/// FR-8 for one lane, before the row combines its lanes.
fn lane_state(i: &SyncInputs, row: RowKind, lane: Lane, now: Millis) -> RowState {
    let a = &i.lanes[lane.index()];
    if i.gov.unauthorized {
        return RowState::TokenRejected;
    }
    if let Some(scope) = row.scope()
        && i.missing_scopes.iter().any(|s| s == scope)
    {
        return RowState::NeedsScope(scope);
    }
    if !i.online {
        return RowState::Offline;
    }
    match i.hold {
        Some(Hold::Paused { until }) if until > now => {
            return RowState::WaitingForAdo {
                until,
                resource: i.gov.resource.clone(),
            };
        }
        Some(Hold::Budget { until }) if until > now => return RowState::WaitingForBudget { until },
        _ => {}
    }
    if a.running {
        return RowState::Syncing(a.progress);
    }
    if let Some((reason, retry_at)) = &a.failure {
        return RowState::Failed {
            reason: reason.clone(),
            retry_at: *retry_at,
        };
    }
    if i.prefs.pause.active(now) {
        return RowState::Paused {
            until: match i.prefs.pause {
                crate::cadence::Pause::Until(at) => Some(at),
                _ => None,
            },
        };
    }
    if !i.foreground && i.prefs.background == Background::Paused {
        return RowState::PausedInBackground;
    }
    let slowed = match i.pressure {
        Some(Pressure::Slow) => lane.tier() != crate::Tier::Hot,
        Some(Pressure::HotOnly) => true,
        _ => false,
    };
    if slowed {
        return RowState::Slowed {
            remaining_pct: i.remaining_pct,
            resets_at: i.gov.pressure_until,
        };
    }
    if lane
        .kind()
        .is_some_and(|k| i.prefs.cadence(k) == Cadence::Manual)
    {
        return RowState::ManualOnly;
    }
    if a.last_ok.is_some() {
        RowState::Synced
    } else {
        RowState::NotSyncedYet
    }
}

/// FR-7/FR-8 rows from the current inputs. Pure: call at render with
/// `now`.
pub fn rows(inputs: &SyncInputs, now: Millis) -> [KindRow; 6] {
    RowKind::ALL.map(|kind| {
        let lanes = kind.lanes();
        let state = lanes
            .iter()
            .map(|lane| lane_state(inputs, kind, *lane, now))
            .min_by_key(rank)
            .unwrap_or(RowState::NotSyncedYet);
        let (last_ok, state) = match kind {
            RowKind::Pinned => {
                let last = inputs.pins.last_full_refresh;
                let state = match state {
                    RowState::Synced | RowState::NotSyncedYet if last.is_some() => RowState::Synced,
                    RowState::Synced => RowState::NotSyncedYet,
                    s => s,
                };
                (last, state)
            }
            _ => (
                lanes
                    .iter()
                    .filter_map(|l| inputs.lanes[l.index()].last_ok)
                    .max(),
                state,
            ),
        };
        let next_due = match state {
            RowState::Offline | RowState::ManualOnly | RowState::TokenRejected => None,
            _ => lanes
                .iter()
                .filter_map(|l| inputs.lanes[l.index()].next_due)
                .min(),
        };
        KindRow {
            kind,
            state,
            last_ok,
            next_due,
            cadence: kind.kind().map(|k| inputs.prefs.cadence(k)),
        }
    })
}

/// The `lane_sync` key of a lane (store migration 0010, FR-10).
pub fn lane_key(lane: Lane) -> &'static str {
    match lane {
        Lane::MyPrs => "my_prs",
        Lane::LiveRuns => "live_runs",
        Lane::AllPrs => "all_prs",
        Lane::WorkItems => "work_items",
        Lane::DoneRuns => "done_runs",
        Lane::Index => "index",
    }
}

pub fn lane_from_key(key: &str) -> Option<Lane> {
    Lane::ALL.into_iter().find(|l| lane_key(*l) == key)
}

// ── the tracker ─────────────────────────────────────────────────────────

struct Running {
    record: PassRecord,
    counters: Arc<PassCounters>,
    changes: ChangeSet,
}

#[derive(Default)]
struct State {
    finished: VecDeque<PassRecord>,
    running: Vec<Running>,
    lanes: [LaneActivity; 6],
    pins: PinsRow,
    next_id: u64,
    prefs: Prefs,
    foreground: bool,
    online: bool,
    scopes: Vec<String>,
    /// The last write's action, for its re-read's origin (FR-18).
    last_write: Option<UserAction>,
    governor: Option<Arc<Governor>>,
}

/// Sync activity of one session (group A): fed by the engine and the
/// scheduler, read by the app.
pub struct Tracker {
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
    signal: Arc<Signal>,
}

impl std::fmt::Debug for Tracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tracker").finish_non_exhaustive()
    }
}

impl Tracker {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::with_signal(clock, Arc::new(Signal::default()))
    }

    pub fn with_signal(clock: Arc<dyn Clock>, signal: Arc<Signal>) -> Self {
        Tracker {
            clock,
            state: Mutex::new(State {
                foreground: true,
                online: true,
                next_id: 1,
                ..State::default()
            }),
            signal,
        }
    }

    /// Fixtures (gallery): finished `passes` (any order), lanes and pins.
    pub fn from_fixture(passes: Vec<PassRecord>, lanes: [LaneActivity; 6], pins: PinsRow) -> Self {
        let t = Tracker::new(Arc::new(SystemClock));
        {
            let mut s = t.lock();
            s.lanes = lanes;
            s.pins = pins;
            s.next_id = passes.iter().map(|p| p.id + 1).max().unwrap_or(1);
            let mut passes = passes;
            passes.sort_by_key(|p| (p.started, p.id));
            s.finished = passes.into();
        }
        t
    }

    /// Append a finished pass (fixtures, live stories).
    pub fn push_fixture(&self, pass: PassRecord) {
        {
            let mut s = self.lock();
            s.next_id = s.next_id.max(pass.id + 1);
            s.finished.push_back(pass);
        }
        self.signal.bump();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn update<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let r = f(&mut self.lock());
        self.signal.bump();
        r
    }

    pub fn signal(&self) -> &Arc<Signal> {
        &self.signal
    }

    pub fn generation(&self) -> u64 {
        self.signal.generation()
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Running passes first, then finished ones, newest first.
    pub fn passes(&self) -> Vec<PassRecord> {
        let s = self.lock();
        let mut out: Vec<PassRecord> = s
            .running
            .iter()
            .rev()
            .map(|r| {
                let mut p = r.record.clone();
                p.requests = r.counters.requests();
                p.tstu = r.counters.tstu();
                p.changes = r.changes.finish();
                p
            })
            .collect();
        out.extend(s.finished.iter().rev().cloned());
        out
    }

    pub fn lanes(&self) -> [LaneActivity; 6] {
        self.lock().lanes.clone()
    }

    pub fn pins(&self) -> PinsRow {
        self.lock().pins.clone()
    }

    /// The inputs of [`rows`] now (no IO).
    pub fn inputs(&self) -> SyncInputs {
        let s = self.lock();
        let (gov, hold, pressure) = match &s.governor {
            Some(g) => (g.snapshot(), g.hold(), Some(g.pressure())),
            None => (GovSnapshot::default(), None, None),
        };
        SyncInputs {
            lanes: s.lanes.clone(),
            remaining_pct: gov.remaining_pct,
            gov,
            hold,
            pressure,
            prefs: s.prefs,
            missing_scopes: missing_scopes(&s.scopes),
            online: s.online,
            foreground: s.foreground,
            pins: s.pins.clone(),
        }
    }

    // Feeding (engine, scheduler, writer).

    pub fn attach_governor(&self, gov: Arc<Governor>) {
        self.update(|s| s.governor = Some(gov));
    }

    /// The token's granted scopes (empty: unknown).
    pub fn set_scopes(&self, scopes: Vec<String>) {
        self.update(|s| s.scopes = scopes);
    }

    pub fn set_schedule(
        &self,
        next_due: [Option<Millis>; 6],
        prefs: Prefs,
        foreground: bool,
        online: bool,
    ) {
        let mut s = self.lock();
        let changed = s.prefs != prefs
            || s.foreground != foreground
            || s.online != online
            || s.lanes.iter().zip(&next_due).any(|(l, n)| l.next_due != *n);
        if !changed {
            return;
        }
        s.prefs = prefs;
        s.foreground = foreground;
        s.online = online;
        for (l, n) in s.lanes.iter_mut().zip(next_due) {
            l.next_due = n;
        }
        drop(s);
        self.signal.bump();
    }

    /// Last successes read from the store at launch (FR-10).
    pub fn restore(&self, last_ok: [Option<Millis>; 6], pins: PinsRow) {
        self.update(|s| {
            for (l, at) in s.lanes.iter_mut().zip(last_ok) {
                l.last_ok = l.last_ok.max(at);
            }
            s.pins = pins;
        });
    }

    pub fn set_pins(&self, pins: PinsRow) {
        self.update(|s| s.pins = pins);
    }

    pub fn lane_running(&self, lane: Lane, running: bool) {
        self.update(|s| {
            let l = &mut s.lanes[lane.index()];
            l.running = running;
            l.progress = None;
        });
    }

    pub fn progress(&self, lane: Lane, progress: Progress) {
        self.update(|s| s.lanes[lane.index()].progress = Some(progress));
    }

    pub fn lane_ok(&self, lane: Lane, at: Millis) {
        self.update(|s| {
            let l = &mut s.lanes[lane.index()];
            l.last_ok = l.last_ok.max(Some(at));
            l.running = false;
            l.progress = None;
            l.failure = None;
        });
    }

    pub fn lane_failed(&self, lane: Lane, reason: FailReason) {
        self.update(|s| {
            let l = &mut s.lanes[lane.index()];
            l.running = false;
            l.progress = None;
            l.failure = Some((reason, None));
        });
    }

    /// When the failed `lane` retries (the scheduler's backoff).
    pub fn set_retry(&self, lane: Lane, at: Millis) {
        self.update(|s| {
            if let Some((_, retry)) = &mut s.lanes[lane.index()].failure {
                *retry = Some(at);
            }
        });
    }

    pub fn note_write(&self, action: UserAction) {
        self.lock().last_write = Some(action);
    }

    pub fn take_last_write(&self) -> Option<UserAction> {
        self.lock().last_write.take()
    }

    /// Merge changes a committed page made into pass `id`.
    pub fn add_changes(&self, id: u64, changes: ChangeSet) {
        if changes.is_empty() {
            return;
        }
        self.update(|s| {
            if let Some(r) = s.running.iter_mut().find(|r| r.record.id == id) {
                r.changes.merge(changes);
            }
        });
    }

    /// Start a pass; finish it with [`PassGuard::finish`] (dropping it
    /// records it as canceled).
    pub fn begin_pass(self: &Arc<Self>, trigger: Trigger, lanes: Lanes) -> PassGuard {
        let started = self.clock.now_ms();
        let counters = Arc::new(PassCounters::default());
        let pass = self.update(|s| {
            let id = s.next_id;
            s.next_id += 1;
            let record = PassRecord {
                id,
                started,
                ended: None,
                lanes,
                trigger,
                outcome: Outcome::Running,
                requests: 0,
                tstu: 0.0,
                events: 0,
                changes: Changes::default(),
            };
            let pass = record.pass_ref();
            s.running.push(Running {
                record,
                counters: counters.clone(),
                changes: ChangeSet::default(),
            });
            pass
        });
        PassGuard {
            tracker: self.clone(),
            link: PassLink { pass, counters },
            trigger,
            lead: lanes.iter().next().unwrap_or(Lane::MyPrs),
            done: false,
        }
    }

    /// "You: Sync #20101" and other user-started passes (spec 0015).
    pub fn begin_user_pass(self: &Arc<Self>, trigger: Trigger) -> PassGuard {
        let lanes = match trigger {
            Trigger::ItemSync(_) => [Lane::WorkItems].into_iter().collect(),
            Trigger::OpenPr(_) | Trigger::Completing(_) | Trigger::Reread(_) => {
                [Lane::MyPrs].into_iter().collect()
            }
            Trigger::LoadOlderRuns => [Lane::DoneRuns].into_iter().collect(),
            _ => Lanes::NONE,
        };
        self.begin_pass(trigger, lanes)
    }

    /// Demo mode (FR-15): one "Demo feed" pass per feed tick.
    pub fn record_demo_tick(&self, changes: Changes, events: usize, now: Millis) {
        let id = {
            let mut s = self.lock();
            let id = s.next_id;
            s.next_id += 1;
            id
        };
        self.push_finished(PassRecord {
            id,
            started: now,
            ended: Some(now),
            lanes: Lanes::NONE,
            trigger: Trigger::DemoFeed,
            outcome: Outcome::Ok,
            requests: 0,
            tstu: 0.0,
            events,
            changes,
        });
    }

    fn end(&self, id: u64, outcome: Outcome, events: usize) {
        let now = self.clock.now_ms();
        let record = {
            let mut s = self.lock();
            let Some(ix) = s.running.iter().position(|r| r.record.id == id) else {
                return;
            };
            let r = s.running.remove(ix);
            let mut record = r.record;
            record.ended = Some(now.max(record.started));
            record.outcome = outcome;
            record.requests = r.counters.requests();
            record.tstu = r.counters.tstu();
            record.events = events;
            record.changes = r.changes.finish();
            record
        };
        // FR-11: a pass that sent nothing and changed nothing is folded
        // into the row state.
        if record.requests == 0 && record.events == 0 && record.changes.is_empty() {
            self.signal.bump();
            return;
        }
        self.push_finished(record);
    }

    fn push_finished(&self, record: PassRecord) {
        self.update(|s| {
            let now = record.ended.unwrap_or(record.started);
            s.finished.push_back(record);
            while s.finished.len() > MAX_PASSES
                || s.finished
                    .front()
                    .is_some_and(|p| p.ended.unwrap_or(p.started) < now - MAX_PASS_AGE_MS)
            {
                s.finished.pop_front();
            }
        });
    }
}

/// A running pass. Requests inside [`Self::scope`] count toward it.
pub struct PassGuard {
    tracker: Arc<Tracker>,
    link: PassLink,
    trigger: Trigger,
    /// The first lane, for the trigger's default origin.
    lead: Lane,
    done: bool,
}

impl PassGuard {
    pub fn pass(&self) -> PassRef {
        self.link.pass
    }

    pub fn trigger(&self) -> Trigger {
        self.trigger
    }

    /// The attribution of requests in this pass with `origin`.
    pub fn attribution(&self, origin: Origin) -> Attribution {
        Attribution {
            origin,
            pass: Some(self.link.clone()),
        }
    }

    /// Run `fut` in this pass with the trigger's own origin
    /// ([`Trigger::origin`]).
    pub fn scope<'a, F: Future + 'a>(
        &self,
        fut: F,
    ) -> impl Future<Output = F::Output> + use<'a, F> {
        self.scope_as(self.trigger.origin(self.lead, false), fut)
    }

    /// Run `fut` in this pass with `origin`.
    pub fn scope_as<'a, F: Future + 'a>(
        &self,
        origin: Origin,
        fut: F,
    ) -> impl Future<Output = F::Output> + use<'a, F> {
        attribution::scope(self.attribution(origin), fut)
    }

    /// Changes committed outside the engine (per-item sync).
    pub fn add_changes(&self, changes: ChangeSet) {
        self.tracker.add_changes(self.link.pass.id, changes);
    }

    pub fn finish(mut self, outcome: Outcome, events: usize) {
        self.done = true;
        self.tracker.end(self.link.pass.id, outcome, events);
    }
}

impl Drop for PassGuard {
    fn drop(&mut self) {
        if !self.done {
            self.tracker.end(self.link.pass.id, Outcome::Canceled, 0);
        }
    }
}

#[cfg(test)]
mod tests;
