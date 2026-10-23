//! Fetchers, diffing and event emission (spec 0002 FR-2..FR-13, ADR 0001 §5).
//!
//! Every ADO page is applied in one store transaction together with its
//! derived events and the cursor advance, so a crash resumes from the last
//! committed page and replays are no-ops.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use adoken_model::time::{DAY, HOUR, MINUTE, SECOND};
use adoken_model::{
    Millis, PinSubject, PinUnavailable, Pipeline, PipelineRun, PrStatus, PullRequest, Repo,
    Reviewer, RunStatus, Subject, SyncEvent, SyncEventKind, Vote,
};
use adoken_store::read::{self, Cursor};
use adoken_store::rusqlite::Connection;
use adoken_store::{PrFilter, Store, write};
use serde::de::DeserializeOwned;
use tokio::sync::watch;
use url::Url;

use crate::Tier;
use crate::activity::{
    self, ChangeWord, FailReason, Outcome, PinsRow, Progress, ProgressUnit, Tracker, Trigger,
};
use crate::attribution::{self, Origin};
use crate::cadence::{Lane, Lanes};
use crate::clock::Clock;
use crate::governor::{GovError, Governor};
use crate::transport::{ApiRequest, Transport};
use crate::wire::{
    self, GitPullRequest, WireBuild, WireThread, WireWiComment, WireWorkItem, format_time,
    parse_time,
};

pub const API_VERSION: &str = "7.1";
/// `connectionData` only exists as a preview API: `7.1` answers 400
/// `VssInvalidPreviewVersionException` (verified against dev.azure.com).
const CONNECTION_DATA_VERSION: &str = "7.1-preview";
/// Work item comments exist only as a preview API (spec 0002 §5.1).
const COMMENTS_VERSION: &str = "7.1-preview.4";
/// Time-based cursors re-query from `high_water - OVERLAP` (ADR 0001 §5).
pub const OVERLAP_MS: Millis = 5 * MINUTE;
/// FR-13: never notify about changes older than this.
pub const MAX_EVENT_AGE_MS: Millis = 24 * HOUR;
const WORK_ITEM_BATCH: usize = 200;
const MAX_PAGES_PER_PASS: usize = 500;
/// Build timelines fetched per hot pass (running runs first) and per warm
/// pass (the backlog of finished runs without current stages).
const HOT_TIMELINES: usize = 4;
const WARM_TIMELINES: usize = 8;
/// `X-RateLimit-Cost` of one timeline GET, measured on dev.azure.com
/// (2026-09-26: 0.037-0.038 TSTU, ~10× a list call). Booked when the
/// response doesn't say.
pub const TIMELINE_COST: f64 = 0.04;
/// Timeline fetches pause while the rolling 5-minute cost is at or above
/// this: a fifth of the FR-9 steady target (10 TSTU / 5 min), the rest is
/// for PRs and work items.
pub const TIMELINE_BUDGET: f64 = 2.0;
/// Hot-set PRs whose file list (iterations + changes, spec 0004 FR-1) is
/// refetched per hot pass after a push, and the rolling 5-minute cost at
/// or above which those refetches wait (another fifth of the FR-9 steady
/// target). A push costs one list refetch per PR, ~0.05 TSTU.
const HOT_FILE_LISTS: usize = 5;
pub const FILE_LIST_BUDGET: f64 = 2.0;
/// Work item discussions fetched per warm pass, and comment pages per item.
const COMMENT_FETCHES: usize = 20;
const COMMENT_PAGES: usize = 10;
/// Runs per "load older runs" request, per project.
pub const OLDER_RUNS_PAGE: usize = 50;
/// A Complete Azure DevOps accepted but hasn't finished after this is
/// treated as not completing any more (spec 0004 FR-10).
pub const COMPLETING_STALE_MS: Millis = 10 * MINUTE;

/// Fields requested from the reporting revisions feed (title index, FR-2).
/// The feed returns only the fields asked for (verified against
/// dev.azure.com), so `System.IsDeleted` must be listed for
/// `includeDeleted` rows to be recognised as deletions.
const REVISION_FIELDS: &str = "System.Id,System.Title,System.WorkItemType,System.State,\
System.AssignedTo,System.CreatedBy,System.ChangedBy,System.ChangedDate,System.CreatedDate,\
System.TeamProject,System.AreaPath,System.IterationPath,Microsoft.VSTS.Common.Priority,\
System.Tags,System.Parent,System.CommentCount,System.IsDeleted,\
Microsoft.VSTS.Common.ClosedDate,Microsoft.VSTS.Common.StateChangeDate";

#[derive(Clone, Debug)]
pub struct SyncConfig {
    /// Org base URL with trailing slash, e.g. `https://dev.azure.com/contoso/`.
    pub org_url: Url,
    /// FR-2 hot set cap.
    pub hot_cap: usize,
    /// Refetch a hot PR's threads at least this often (FR-6).
    pub hot_refresh_ms: Millis,
    /// Rotation: thread fetches per hot pass.
    pub max_thread_fetches: usize,
    /// First `wi.revisions` call: `startDateTime = now - horizon`. `None`
    /// (default) indexes every work item (spec 0002 Q-2); `Some(365 days)`
    /// is the fallback for projects whose backfill is too expensive.
    pub revisions_horizon_ms: Option<Millis>,
    /// First `build.done` window.
    pub builds_window_ms: Millis,
    /// Selected project ids (onboarding / Settings → Projects). `None`
    /// syncs every project the token can see; `Some(empty)` syncs none.
    pub projects: Option<Vec<String>>,
}

impl SyncConfig {
    pub fn new(org_url: Url) -> Self {
        let mut org_url = org_url;
        if !org_url.path().ends_with('/') {
            org_url.set_path(&format!("{}/", org_url.path()));
        }
        SyncConfig {
            org_url,
            hot_cap: 150,
            hot_refresh_ms: 60 * SECOND,
            max_thread_fetches: 30,
            revisions_horizon_ms: None,
            builds_window_ms: 7 * DAY,
            projects: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Store(#[from] adoken_store::Error),
    #[error(transparent)]
    Gov(#[from] GovError),
    #[error("decode {what}: {message}")]
    Decode { what: String, message: String },
    #[error("sync prerequisite missing: {0}")]
    Missing(&'static str),
}

/// What the status pill shows (spec 0002 §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Syncing(Tier),
    Offline,
    Throttled {
        until: Millis,
        resource: Option<String>,
    },
    Unauthorized,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatus {
    pub phase: Phase,
    pub last_ok_hot: Option<Millis>,
    pub last_ok_warm: Option<Millis>,
    pub last_ok_cold: Option<Millis>,
}

/// Outcome of one tier pass.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassReport {
    /// Ids of newly persisted events.
    pub events: Vec<i64>,
    /// TSTU each lane of the pass booked at the governor.
    pub lane_costs: Vec<(Lane, f64)>,
}

pub struct Engine {
    store: Store,
    transport: Arc<dyn Transport>,
    gov: Arc<Governor>,
    clock: Arc<dyn Clock>,
    cfg: SyncConfig,
    status: watch::Sender<SyncStatus>,
    /// Reachability says offline (FR-5): passes end in `Phase::Offline`.
    offline: AtomicBool,
    /// Review files (spec 0004 FR-1..FR-3) through the same transport and
    /// governor: hot-set file lists here, user-priority opens in the app.
    files: Arc<crate::pr_files::PrFiles>,
    /// Hot-set threads are refetched when older than this (FR-6); the
    /// scheduler lowers it when "my PRs" syncs faster than `hot_refresh_ms`.
    thread_refresh_ms: AtomicI64,
    /// Sync activity (spec 0016 group A): passes, lane state, changes.
    tracker: Arc<Tracker>,
    /// Last attempt to fetch each `(project, type)`'s states, so a type
    /// Azure DevOps won't describe (renamed, no permission) isn't asked for
    /// again every pass.
    type_attempts: std::sync::Mutex<HashMap<(String, String), Millis>>,
}

/// Emits events inside a page transaction; applies FR-12/FR-13 and dedupe.
pub(crate) struct Emitter<'a> {
    pub(crate) tx: &'a Connection,
    pub(crate) me: &'a str,
    pub(crate) enabled: bool,
    pub(crate) now: Millis,
    pub(crate) new_ids: Vec<i64>,
}

impl Emitter<'_> {
    pub(crate) fn emit(
        &mut self,
        kind: SyncEventKind,
        subject: Subject,
        dedupe_key: String,
        actor: Option<&str>,
        at: Millis,
        summary: String,
    ) -> adoken_store::Result<()> {
        if !self.enabled || at < self.now - MAX_EVENT_AGE_MS {
            return Ok(());
        }
        let event = SyncEvent {
            id: 0,
            kind,
            subject,
            dedupe_key,
            actor: actor.map(str::to_owned),
            self_caused: actor == Some(self.me),
            at,
            summary,
        };
        if let Some(id) = write::insert_event(self.tx, &event)? {
            self.new_ids.push(id);
        }
        Ok(())
    }
}

fn baseline_done(tx: &Connection, scope: &str, project: &str) -> adoken_store::Result<bool> {
    Ok(read::cursor(tx, scope, project)?.is_some_and(|c| c.baseline_done))
}

fn cursor_or_new(tx: &Connection, scope: &str, project: &str) -> adoken_store::Result<Cursor> {
    Ok(read::cursor(tx, scope, project)?.unwrap_or(Cursor {
        scope: scope.to_owned(),
        project_id: project.to_owned(),
        ..Default::default()
    }))
}

pub(crate) fn name(tx: &Connection, id: &str) -> String {
    read::person(tx, id)
        .ok()
        .flatten()
        .map(|p| p.display_name)
        .unwrap_or_else(|| "Someone".to_owned())
}

impl Engine {
    pub fn new(
        store: Store,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
        cfg: SyncConfig,
    ) -> Self {
        let (status, _) = watch::channel(SyncStatus {
            phase: Phase::Idle,
            last_ok_hot: None,
            last_ok_warm: None,
            last_ok_cold: None,
        });
        let gov = Arc::new(Governor::new(clock.clone()));
        let files = Arc::new(crate::pr_files::PrFiles::new(
            store.clone(),
            transport.clone(),
            gov.clone(),
            clock.clone(),
            cfg.org_url.clone(),
        ));
        let thread_refresh_ms = AtomicI64::new(cfg.hot_refresh_ms);
        let tracker = Arc::new(Tracker::new(clock.clone()));
        tracker.attach_governor(gov.clone());
        Engine {
            store,
            transport,
            gov,
            clock,
            cfg,
            status,
            offline: AtomicBool::new(false),
            files,
            thread_refresh_ms,
            tracker,
            type_attempts: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Report activity into `tracker` (the app's session-wide one).
    pub fn with_tracker(mut self, tracker: Arc<Tracker>) -> Self {
        tracker.attach_governor(self.gov.clone());
        self.tracker = tracker;
        self
    }

    pub fn tracker(&self) -> &Arc<Tracker> {
        &self.tracker
    }

    /// Follow the "my PRs" interval (spec 0002 FR-4a): when it's shorter
    /// than `SyncConfig::hot_refresh_ms`, hot-set threads older than 80% of
    /// it are refetched, so each pass really refreshes them (passes move by
    /// ±10% jitter and may start a tenth early to share their tier's pass).
    /// `None` (manual only) or longer intervals keep `hot_refresh_ms`.
    pub fn follow_hot_interval(&self, interval: Option<Millis>) {
        let refresh = match interval {
            Some(ms) if ms < self.cfg.hot_refresh_ms => (ms * 8 / 10).max(SECOND),
            _ => self.cfg.hot_refresh_ms,
        };
        self.thread_refresh_ms.store(refresh, Ordering::Release);
    }

    pub fn status(&self) -> watch::Receiver<SyncStatus> {
        self.status.subscribe()
    }

    pub fn governor(&self) -> &Governor {
        &self.gov
    }

    /// The same governor, for other request sources that must share its
    /// windows and budget (avatars, user writes).
    pub fn shared_governor(&self) -> Arc<Governor> {
        self.gov.clone()
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// User-initiated review writes (spec 0004) through this engine's
    /// transport and governor, at user priority, into the same store.
    pub fn writer(&self) -> crate::writes::Writer {
        crate::writes::Writer::new(
            self.store.clone(),
            self.transport.clone(),
            self.gov.clone(),
            self.clock.clone(),
            self.cfg.org_url.clone(),
        )
        .with_tracker(self.tracker.clone())
    }

    /// Review files at user priority (the review view's source of file
    /// lists and blobs), sharing this engine's transport, governor and
    /// single-flight blob requests.
    pub fn pr_files(&self) -> Arc<crate::pr_files::PrFiles> {
        self.files.clone()
    }

    fn set_phase(&self, phase: Phase) {
        self.status.send_modify(|s| s.phase = phase);
    }

    /// The resting phase: idle, or offline while reachability says so.
    fn rest_phase(&self) -> Phase {
        if self.offline.load(Ordering::Acquire) {
            Phase::Offline
        } else {
            Phase::Idle
        }
    }

    /// Reachability (FR-5): the pill says Offline at once, without waiting
    /// for a request to fail; back online it returns to idle. Never masks
    /// `Unauthorized` or a throttle countdown.
    pub fn set_online(&self, online: bool) {
        self.offline.store(!online, Ordering::Release);
        self.status.send_modify(|s| match (&s.phase, online) {
            (Phase::Idle, false) => s.phase = Phase::Offline,
            (Phase::Offline, true) => s.phase = Phase::Idle,
            _ => {}
        });
    }

    /// A replacement token verified (spec 0001 FR-15, AC-13): clear the
    /// governor's 401 latch and leave `Unauthorized`, so the next pass
    /// sends requests without a restart.
    pub fn token_replaced(&self) {
        self.gov.token_replaced();
        let rest = self.rest_phase();
        self.status.send_modify(|s| {
            if s.phase == Phase::Unauthorized {
                s.phase = rest;
            }
        });
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        self.url_v(path, query, API_VERSION)
    }

    fn url_v(&self, path: &str, query: &[(&str, &str)], version: &str) -> Url {
        let mut url = self.cfg.org_url.join(path).expect("relative ADO path");
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in query {
                q.append_pair(k, v);
            }
            q.append_pair("api-version", version);
        }
        url
    }

    async fn fetch<T: DeserializeOwned>(
        &self,
        tier: Tier,
        req: ApiRequest,
    ) -> Result<T, SyncError> {
        let what = req.url().path().to_owned();
        let resp = self.gov.send(self.transport.as_ref(), tier, req).await?;
        serde_json::from_slice(&resp.body).map_err(|e| SyncError::Decode {
            what,
            message: e.to_string(),
        })
    }

    /// Run `f` on the blocking pool. Changes the appliers note inside it
    /// (FR-12) count toward the running pass once `f` committed.
    async fn db<R: Send + 'static>(
        &self,
        f: impl FnOnce(&Store) -> adoken_store::Result<R> + Send + 'static,
    ) -> Result<R, SyncError> {
        let store = self.store.clone();
        let (result, changes) =
            tokio::task::spawn_blocking(move || activity::collect_changes(|| f(&store)))
                .await
                .map_err(|e| SyncError::Decode {
                    what: "store task".into(),
                    message: e.to_string(),
                })?;
        let result = result.map_err(SyncError::from)?;
        if !changes.is_empty()
            && let Some(link) = attribution::current().and_then(|a| a.pass)
        {
            self.tracker.add_changes(link.pass.id, changes);
        }
        Ok(result)
    }

    /// Run one tier pass, updating the status pill. The first pass on an
    /// empty store bootstraps identity, projects and repos.
    pub async fn run_tier(&self, tier: Tier) -> Result<PassReport, SyncError> {
        self.run_lanes(tier, Lanes::of(tier)).await
    }

    /// Run the `lanes` of `tier` (spec 0002 FR-4a: each lane keeps its own
    /// cadence), updating the status pill.
    pub async fn run_lanes(&self, tier: Tier, lanes: Lanes) -> Result<PassReport, SyncError> {
        self.run_pass(tier, lanes, Trigger::Scheduled).await
    }

    /// [`Self::run_lanes`] as one pass record (spec 0016 FR-11): lanes show
    /// as syncing with progress, each lane's last success is stored when
    /// its last page committed (FR-10), a failure marks the lane it
    /// happened in (FR-8), and every request carries the lane's origin
    /// (FR-18).
    pub async fn run_pass(
        &self,
        tier: Tier,
        lanes: Lanes,
        trigger: Trigger,
    ) -> Result<PassReport, SyncError> {
        self.set_phase(Phase::Syncing(tier));
        let pass = self.tracker.begin_pass(trigger, lanes);
        for lane in lanes.iter() {
            self.tracker.lane_running(lane, true);
        }
        let mut current = None;
        let lead = lanes.iter().next().unwrap_or(Lane::MyPrs);
        let result = pass
            .scope_as(
                trigger.origin(lead, false),
                self.run_lanes_inner(tier, lanes, trigger, &mut current),
            )
            .await;
        let lanes_now = self.tracker.lanes();
        for lane in lanes.iter() {
            if lanes_now[lane.index()].running {
                self.tracker.lane_running(lane, false);
            }
        }
        let outcome = Outcome::of(&result.as_ref().map(|_| ()), self.host());
        if let (Outcome::Failed(reason), Some(lane)) = (&outcome, current)
            && *reason != FailReason::Unauthorized
        {
            self.tracker.lane_failed(lane, reason.clone());
        }
        pass.finish(outcome, result.as_ref().map_or(0, |r| r.events.len()));
        let now = self.clock.now_ms();
        match &result {
            Ok(_) => self.status.send_modify(|s| {
                s.phase = self.rest_phase();
                match tier {
                    Tier::Hot | Tier::User => s.last_ok_hot = Some(now),
                    Tier::Warm => s.last_ok_warm = Some(now),
                    Tier::Cold => s.last_ok_cold = Some(now),
                }
            }),
            Err(SyncError::Gov(GovError::Unauthorized)) => self.set_phase(Phase::Unauthorized),
            Err(SyncError::Gov(GovError::Offline(_))) => self.set_phase(Phase::Offline),
            Err(SyncError::Gov(GovError::Throttled { until })) => {
                let resource = self.gov.snapshot().resource;
                self.set_phase(Phase::Throttled {
                    until: *until,
                    resource,
                })
            }
            Err(_) => self.set_phase(self.rest_phase()),
        }
        result
    }

    /// The org's host, for "Couldn't reach dev.azure.com".
    fn host(&self) -> &str {
        self.cfg.org_url.host_str().unwrap_or("dev.azure.com")
    }

    /// `lane` finished its last step: store its last success (FR-10, in
    /// its own transaction right after the lane's last page committed).
    async fn lane_done(&self, lane: Lane) -> Result<(), SyncError> {
        let now = self.clock.now_ms();
        self.db(move |s| s.write(|tx| write::put_lane_sync(tx, activity::lane_key(lane), now)))
            .await?;
        self.tracker.lane_ok(lane, now);
        Ok(())
    }

    /// Per-lane last successes and the pins row, from the store (launch,
    /// FR-10). Blocking: one small read.
    pub fn restore_activity(&self) {
        let read = self.store.with_conn(|c| {
            let lanes = read::lane_sync(c)?;
            let pins = read::pins(c)?;
            Ok((lanes, pins))
        });
        let Ok((lanes, pins)) = read else {
            return;
        };
        let mut last_ok = [None; 6];
        for (key, at) in lanes {
            if let Some(lane) = activity::lane_from_key(&key) {
                last_ok[lane.index()] = Some(at);
            }
        }
        let last_full_refresh = [Lane::AllPrs, Lane::WorkItems, Lane::DoneRuns]
            .iter()
            .map(|l| last_ok[l.index()])
            .min()
            .flatten();
        self.tracker.restore(
            last_ok,
            PinsRow {
                pins: pins.len() as u32,
                unavailable: pins.iter().filter(|p| p.unavailable.is_some()).count() as u32,
                last_full_refresh,
            },
        );
    }

    /// Pin counts after a pass; a warm pass that ran every pinned step
    /// refreshes the Pinned row's time (AC-A6).
    async fn refresh_pins_row(&self, all_pinned_steps: bool) -> Result<(), SyncError> {
        let pins = self.db(|s| s.with_conn(read::pins)).await?;
        let mut row = self.tracker.pins();
        row.pins = pins.len() as u32;
        row.unavailable = pins.iter().filter(|p| p.unavailable.is_some()).count() as u32;
        if all_pinned_steps {
            row.last_full_refresh = Some(self.clock.now_ms());
        }
        self.tracker.set_pins(row);
        Ok(())
    }

    async fn run_lanes_inner(
        &self,
        tier: Tier,
        lanes: Lanes,
        trigger: Trigger,
        current: &mut Option<Lane>,
    ) -> Result<PassReport, SyncError> {
        let origin = |lane, pinned| trigger.origin(lane, pinned);
        let mut report = PassReport::default();
        let mut spent = LaneMeter::new(&self.gov, tier);
        let has_structure = self
            .db(|s| Ok(s.me()?.is_some() && !s.synced_projects()?.is_empty()))
            .await?;
        if !has_structure || lanes.contains(Lane::Index) {
            // Bootstrap before the first pass is "Startup" (FR-18).
            let structure = if lanes.contains(Lane::Index) {
                *current = Some(Lane::Index);
                origin(Lane::Index, false)
            } else {
                *current = lanes.iter().next();
                Origin::Startup
            };
            attribution::with_origin(structure, self.sync_structure(tier)).await?;
            if lanes.contains(Lane::Index) {
                spent.book(Lane::Index, &mut report);
            } else {
                // First-launch bootstrap: not any lane's steady cost.
                spent.skip();
            }
        }
        let me = self.db(|s| s.me()).await?.ok_or(SyncError::Missing("me"))?;
        let projects = self.db(|s| s.synced_projects()).await?;
        let of = |done: usize, unit| Progress {
            done: done as u32,
            total: Some(projects.len() as u32),
            unit,
        };
        if lanes.contains(Lane::MyPrs) {
            *current = Some(Lane::MyPrs);
            let events = attribution::with_origin(
                origin(Lane::MyPrs, false),
                self.sync_threads(tier, &me.id),
            )
            .await?;
            report.events.extend(events);
            spent.book(Lane::MyPrs, &mut report);
        }
        if lanes.contains(Lane::LiveRuns) {
            *current = Some(Lane::LiveRuns);
            let o = origin(Lane::LiveRuns, false);
            for (i, p) in projects.iter().enumerate() {
                self.tracker
                    .progress(Lane::LiveRuns, of(i, ProgressUnit::Projects));
                match attribution::with_origin(o, self.sync_live_builds(tier, &p.id, &me)).await {
                    Ok(events) => report.events.extend(events),
                    Err(SyncError::Gov(GovError::Status { status, .. }))
                        if (status == 403 || status == 404)
                            && self
                                .mark_definition_project_unavailable(&p.id, status)
                                .await? => {}
                    Err(e) => return Err(e),
                }
            }
            report.events.extend(
                attribution::with_origin(
                    origin(Lane::LiveRuns, true),
                    self.sync_unselected_pinned_live(tier, &me.id),
                )
                .await?,
            );
            attribution::with_origin(o, self.sync_timelines(tier, Lane::LiveRuns, HOT_TIMELINES))
                .await?;
            spent.book(Lane::LiveRuns, &mut report);
            self.lane_done(Lane::LiveRuns).await?;
        }
        if lanes.contains(Lane::MyPrs) {
            *current = Some(Lane::MyPrs);
            attribution::with_origin(
                origin(Lane::MyPrs, false),
                self.sync_file_lists(tier, &me.id),
            )
            .await?;
            spent.book(Lane::MyPrs, &mut report);
            self.lane_done(Lane::MyPrs).await?;
        }
        if lanes.contains(Lane::AllPrs) {
            *current = Some(Lane::AllPrs);
            let o = origin(Lane::AllPrs, false);
            for (i, p) in projects.iter().enumerate() {
                self.tracker
                    .progress(Lane::AllPrs, of(i, ProgressUnit::Projects));
                report.events.extend(
                    attribution::with_origin(o, self.sync_active_prs(tier, &p.id, &me.id)).await?,
                );
            }
            report.events.extend(
                attribution::with_origin(
                    origin(Lane::AllPrs, true),
                    self.sync_pinned_prs(tier, &me.id),
                )
                .await?,
            );
            spent.book(Lane::AllPrs, &mut report);
            self.lane_done(Lane::AllPrs).await?;
        }
        if lanes.contains(Lane::WorkItems) {
            *current = Some(Lane::WorkItems);
            let o = origin(Lane::WorkItems, false);
            report
                .events
                .extend(attribution::with_origin(o, self.sync_hot_work_items(tier, &me)).await?);
            report.events.extend(
                attribution::with_origin(
                    origin(Lane::WorkItems, true),
                    self.sync_pinned_work_items(tier, &me.id),
                )
                .await?,
            );
            report
                .events
                .extend(attribution::with_origin(o, self.sync_wi_comments(tier, &me.id)).await?);
            attribution::with_origin(o, self.sync_type_states(tier)).await?;
            spent.book(Lane::WorkItems, &mut report);
            self.lane_done(Lane::WorkItems).await?;
        }
        if lanes.contains(Lane::DoneRuns) {
            *current = Some(Lane::DoneRuns);
            let o = origin(Lane::DoneRuns, false);
            for (i, p) in projects.iter().enumerate() {
                self.tracker
                    .progress(Lane::DoneRuns, of(i, ProgressUnit::Projects));
                match attribution::with_origin(o, self.sync_done_builds(tier, &p.id, &me)).await {
                    Ok(events) => report.events.extend(events),
                    Err(SyncError::Gov(GovError::Status { status, .. }))
                        if (status == 403 || status == 404)
                            && self
                                .mark_definition_project_unavailable(&p.id, status)
                                .await? => {}
                    Err(e) => return Err(e),
                }
            }
            report.events.extend(
                attribution::with_origin(
                    origin(Lane::DoneRuns, true),
                    self.sync_pinned_definitions(tier, &me.id),
                )
                .await?,
            );
            attribution::with_origin(o, self.sync_timelines(tier, Lane::DoneRuns, WARM_TIMELINES))
                .await?;
            spent.book(Lane::DoneRuns, &mut report);
            self.lane_done(Lane::DoneRuns).await?;
        }
        if lanes.contains(Lane::Index) {
            *current = Some(Lane::Index);
            let o = origin(Lane::Index, false);
            let mut so_far = 0;
            for p in &projects {
                report.events.extend(
                    attribution::with_origin(
                        o,
                        self.sync_revisions(tier, &p.id, &me.id, &mut so_far),
                    )
                    .await?,
                );
            }
            attribution::with_origin(o, self.sync_type_states(tier)).await?;
            spent.book(Lane::Index, &mut report);
            self.lane_done(Lane::Index).await?;
        }
        let pinned_lanes = [
            Lane::AllPrs,
            Lane::WorkItems,
            Lane::LiveRuns,
            Lane::DoneRuns,
        ];
        if pinned_lanes.iter().any(|l| lanes.contains(*l)) {
            let all = [Lane::AllPrs, Lane::WorkItems, Lane::DoneRuns]
                .iter()
                .all(|l| lanes.contains(*l));
            self.refresh_pins_row(all).await?;
        }
        Ok(report)
    }

    /// Me (`connectionData`), the org, selected projects and their repos.
    async fn sync_structure(&self, tier: Tier) -> Result<(), SyncError> {
        let conn: wire::ConnectionData = self
            .fetch(
                tier,
                ApiRequest::get(self.url_v("_apis/connectionData", &[], CONNECTION_DATA_VERSION)),
            )
            .await?;
        let mut projects: wire::List<wire::ProjectRef> = self
            .fetch(tier, ApiRequest::get(self.url("_apis/projects", &[])))
            .await?;
        if let Some(selected) = &self.cfg.projects {
            projects.value.retain(|p| selected.contains(&p.id));
        }
        let mut repos = Vec::new();
        for p in &projects.value {
            let list: wire::List<wire::GitRepo> = self
                .fetch(
                    tier,
                    ApiRequest::get(self.url(&format!("{}/_apis/git/repositories", p.id), &[])),
                )
                .await?;
            repos.extend(list.value);
        }
        let org_name = self
            .cfg
            .org_url
            .path_segments()
            .and_then(|mut s| s.next())
            .unwrap_or_default()
            .to_owned();
        let org = adoken_model::Org {
            id: conn.instance_id.clone().unwrap_or_else(|| org_name.clone()),
            name: org_name,
            url: self.cfg.org_url.to_string(),
        };
        let now = self.clock.now_ms();
        self.db(move |s| {
            s.write(|tx| {
                write::put_org(tx, &org)?;
                let existing = read::person(tx, &conn.authenticated_user.id)?;
                let me = adoken_model::Person {
                    id: conn.authenticated_user.id.clone(),
                    display_name: conn.authenticated_user.provider_display_name.clone(),
                    unique_name: conn
                        .authenticated_user
                        .account()
                        .map(str::to_owned)
                        .or_else(|| existing.and_then(|p| p.unique_name)),
                    is_group: false,
                    image_url: None,
                };
                write::upsert_person(tx, &me, true)?;
                // FR-12: new or renamed projects and repositories.
                let known: HashMap<String, String> = read::projects(tx)?
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect();
                for p in &projects.value {
                    if known.get(&p.id) != Some(&p.name) {
                        activity::note(|c, _| c.note_project());
                    }
                    write::upsert_project(
                        tx,
                        &adoken_model::Project {
                            id: p.id.clone(),
                            name: p.name.clone(),
                        },
                    )?;
                }
                let known: HashMap<String, Repo> = read::repos(tx)?
                    .into_iter()
                    .map(|r| (r.id.clone(), r))
                    .collect();
                for r in &repos {
                    let repo = Repo {
                        id: r.id.clone(),
                        project_id: r.project.id.clone(),
                        name: r.name.clone(),
                        default_branch: r.default_branch.clone(),
                    };
                    if known.get(&r.id).is_none_or(|old| {
                        (&old.project_id, &old.name, &old.default_branch)
                            != (&repo.project_id, &repo.name, &repo.default_branch)
                    }) {
                        activity::note(|c, _| c.note_repo());
                    }
                    write::upsert_repo(tx, &repo)?;
                }
                let mut c = cursor_or_new(tx, "structure", "")?;
                c.baseline_done = true;
                c.last_ok_ms = Some(now);
                write::put_cursor(tx, &c)
            })
        })
        .await
    }

    /// States of every work item type the store holds (`workitemtypes/{type}`,
    /// with each state's category), refreshed every [`META_TTL_MS`]. Process
    /// templates name states and types freely, so "open", "done" and the
    /// Today/nudge rules read these categories instead of state names.
    /// Best effort: a type that can't be described keeps the stock-name
    /// fallback and is retried after the TTL; only offline/auth/throttle
    /// errors end the step early (the next pass retries).
    ///
    /// [`META_TTL_MS`]: crate::wi::META_TTL_MS
    async fn sync_type_states(&self, tier: Tier) -> Result<(), SyncError> {
        let now = self.clock.now_ms();
        let ttl = crate::wi::META_TTL_MS;
        let due = self
            .db(move |s| s.with_conn(|c| adoken_store::wi::types_needing_states(c, now - ttl)))
            .await?;
        for (project, type_name) in due {
            let key = (project.clone(), type_name.clone());
            {
                let mut tried = self.type_attempts.lock().unwrap_or_else(|e| e.into_inner());
                if tried.get(&key).is_some_and(|at| now - at < ttl) {
                    continue;
                }
                tried.insert(key, now);
            }
            let req = crate::wi::patch::type_request(&self.cfg.org_url, &project, &type_name);
            let body = match self.gov.send(self.transport.as_ref(), tier, req).await {
                Ok(resp) => resp.body,
                Err(GovError::Status { .. }) => continue,
                Err(e) => {
                    log::debug!("work item type states: {e}");
                    return Ok(());
                }
            };
            let Ok(meta) = crate::wi::meta::parse_type(&body, now) else {
                continue;
            };
            if meta.states.is_empty() {
                continue;
            }
            self.db(move |s| {
                s.write(|tx| adoken_store::wi::put_type_meta(tx, &project, &type_name, &meta))
            })
            .await?;
        }
        Ok(())
    }

    // ── pull requests ─────────────────────────────────────────────────────

    /// `pr.active`: full list per project (small set), fingerprint diff.
    /// PRs that left the active set are fetched by id.
    async fn sync_active_prs(
        &self,
        tier: Tier,
        project: &str,
        me: &str,
    ) -> Result<Vec<i64>, SyncError> {
        let mut all: Vec<GitPullRequest> = Vec::new();
        let top = 1000;
        loop {
            let skip = all.len().to_string();
            let page: wire::List<GitPullRequest> = match self
                .fetch(
                    tier,
                    ApiRequest::get(self.url(
                        &format!("{project}/_apis/git/pullrequests"),
                        &[
                            ("searchCriteria.status", "active"),
                            ("$top", &top.to_string()),
                            ("$skip", &skip),
                        ],
                    )),
                )
                .await
            {
                Ok(page) => page,
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if (status == 403 || status == 404)
                        && self.mark_pr_project_unavailable(project, status).await? =>
                {
                    return Ok(Vec::new());
                }
                Err(e) => return Err(e),
            };
            let n = page.value.len();
            all.extend(page.value);
            if n < top {
                break;
            }
        }
        let mut index = 0;
        while index < all.len() {
            if self.resolve_push_actor(tier, &mut all[index], me).await? {
                index += 1;
            } else {
                all.swap_remove(index);
            }
        }
        let now = self.clock.now_ms();
        let (project_s, me_s) = (project.to_owned(), me.to_owned());
        let (mut events, gone) = self
            .db(move |s| {
                s.write(|tx| {
                    let enabled = baseline_done(tx, "pr.active", &project_s)?;
                    let mut em = Emitter {
                        tx,
                        me: &me_s,
                        enabled,
                        now,
                        new_ids: Vec::new(),
                    };
                    let seen: HashSet<i64> = all.iter().map(|p| p.pull_request_id).collect();
                    for pr in &all {
                        apply_pr(&mut em, pr)?;
                    }
                    let gone: Vec<(i64, String)> = read::prs(
                        tx,
                        &PrFilter {
                            status: Some(PrStatus::Active),
                            project_id: Some(project_s.clone()),
                            ..Default::default()
                        },
                    )?
                    .into_iter()
                    .filter(|p| !seen.contains(&p.id))
                    .map(|p| (p.id, p.repo_id))
                    .collect();
                    let mut c = cursor_or_new(tx, "pr.active", &project_s)?;
                    c.baseline_done = true;
                    c.last_ok_ms = Some(now);
                    c.last_error = None;
                    write::put_cursor(tx, &c)?;
                    Ok((em.new_ids, gone))
                })
            })
            .await?;
        for (id, _repo) in gone {
            events.extend(self.fetch_pr(tier, project, id, me).await?);
        }
        Ok(events)
    }

    /// One PR by id, applied like a list row (events and guards included).
    async fn fetch_pr(
        &self,
        tier: Tier,
        project: &str,
        id: i64,
        me: &str,
    ) -> Result<Vec<i64>, SyncError> {
        let mut pr: GitPullRequest = match self
            .fetch(
                tier,
                ApiRequest::get(self.url(&format!("{project}/_apis/git/pullrequests/{id}"), &[])),
            )
            .await
        {
            Ok(pr) => pr,
            Err(
                e @ SyncError::Gov(GovError::Status {
                    status: 403 | 404, ..
                }),
            ) => {
                if self.is_pinned_pr(id).await? {
                    let SyncError::Gov(GovError::Status { status, .. }) = &e else {
                        unreachable!()
                    };
                    self.mark_unavailable(PinSubject::Pr(id), *status).await?;
                    return Ok(Vec::new());
                }
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        if !self.resolve_push_actor(tier, &mut pr, me).await? {
            return Ok(Vec::new());
        }
        let now = self.clock.now_ms();
        let (me_s, project_s) = (me.to_owned(), project.to_owned());
        self.db(move |s| {
            s.write(|tx| {
                let enabled = baseline_done(tx, "pr.active", &project_s)?;
                let mut em = Emitter {
                    tx,
                    me: &me_s,
                    enabled,
                    now,
                    new_ids: Vec::new(),
                };
                apply_pr(&mut em, &pr)?;
                write::set_pin_title(tx, &PinSubject::Pr(id), &pr.title)?;
                write::set_pin_unavailable(tx, &PinSubject::Pr(id), None)?;
                Ok(em.new_ids)
            })
        })
        .await
    }

    async fn is_pinned_pr(&self, id: i64) -> Result<bool, SyncError> {
        self.db(move |s| s.with_conn(|c| Ok(read::pin(c, &PinSubject::Pr(id))?.is_some())))
            .await
    }

    async fn mark_pr_project_unavailable(
        &self,
        project: &str,
        status: u16,
    ) -> Result<bool, SyncError> {
        let project = project.to_owned();
        self.db(move |s| {
            s.write(|tx| {
                let mut marked = false;
                for pin in read::pins(tx)? {
                    if let PinSubject::Pr(id) = pin.subject
                        && read::pr(tx, id)?.is_some_and(|pr| pr.project_id == project)
                    {
                        marked = true;
                        write::set_pin_unavailable(
                            tx,
                            &PinSubject::Pr(id),
                            Some(if status == 403 {
                                PinUnavailable::Forbidden
                            } else {
                                PinUnavailable::Gone
                            }),
                        )?;
                    }
                }
                Ok(marked)
            })
        })
        .await
    }

    async fn resolve_push_actor(
        &self,
        tier: Tier,
        pr: &mut GitPullRequest,
        me: &str,
    ) -> Result<bool, SyncError> {
        let (id, commit, me_s) = (
            pr.pull_request_id,
            pr.last_merge_source_commit
                .as_ref()
                .map(|c| c.commit_id.clone()),
            me.to_owned(),
        );
        let due = self
            .db(move |s| {
                s.with_conn(|c| {
                    let Some(old) = read::pr(c, id)? else {
                        return Ok(false);
                    };
                    Ok(old.last_source_commit != commit
                        && (old.is_reviewer(&me_s)
                            || read::pin(c, &PinSubject::Pr(id))?.is_some()
                            || read::auto_complete(c, id)?.is_some()))
                })
            })
            .await?;
        if !due {
            return Ok(true);
        }
        let url = self.url(
            &format!(
                "{}/_apis/git/repositories/{}/pullRequests/{id}/iterations",
                pr.repository.project.id, pr.repository.id,
            ),
            &[],
        );
        let iterations: wire::List<wire::WireIteration> = match self
            .fetch(
                tier,
                ApiRequest::get(url).with_cost_hint(crate::pr_files::ITERATIONS_COST),
            )
            .await
        {
            Ok(list) => list,
            Err(SyncError::Gov(GovError::Status { status, .. }))
                if (status == 403 || status == 404) && self.is_pinned_pr(id).await? =>
            {
                self.mark_unavailable(PinSubject::Pr(id), status).await?;
                return Ok(false);
            }
            Err(e) => return Err(e),
        };
        let head = pr
            .last_merge_source_commit
            .as_ref()
            .map(|c| c.commit_id.as_str());
        pr.push_actor = iterations
            .value
            .into_iter()
            .filter(|it| match (it.source_ref_commit.as_ref(), head) {
                (Some(commit), Some(head)) => commit.commit_id.eq_ignore_ascii_case(head),
                _ => false,
            })
            .max_by_key(|it| it.id)
            .and_then(|it| it.push.and_then(|p| p.pushed_by).or(it.author));
        Ok(true)
    }

    /// Spec 0004 FR-10: Azure DevOps merges an accepted Complete
    /// asynchronously. Re-read that PR (hot tier, through the governor) so
    /// the review flips to Completed, or shows why the queued merge failed.
    /// Returns whether my Complete is still merging.
    pub async fn recheck_completing(&self, pr_id: i64) -> Result<bool, SyncError> {
        let pass = self.tracker.begin_user_pass(Trigger::Completing(pr_id));
        let result = pass.scope(self.recheck_completing_inner(pr_id)).await;
        pass.finish(Outcome::of(&result.as_ref().map(|_| ()), self.host()), 0);
        result
    }

    async fn recheck_completing_inner(&self, pr_id: i64) -> Result<bool, SyncError> {
        let me = self.db(|s| s.me()).await?.ok_or(SyncError::Missing("me"))?;
        let project = self
            .db(move |s| s.with_conn(|c| read::pr(c, pr_id)))
            .await?
            .map(|pr| pr.project_id);
        let Some(project) = project else {
            return Ok(false);
        };
        self.fetch_pr(Tier::Hot, &project, pr_id, &me.id).await?;
        Ok(self.db(move |s| s.completing(pr_id)).await?.is_some())
    }

    /// Hot set threads (FR-2, FR-6): active PRs I author or review, rotated
    /// oldest-hydrated first, refetched on list change or after
    /// `hot_refresh_ms`.
    async fn sync_threads(&self, tier: Tier, me: &str) -> Result<Vec<i64>, SyncError> {
        let now = self.clock.now_ms();
        let (me_s, cap, refresh, max) = (
            me.to_owned(),
            self.cfg.hot_cap,
            self.thread_refresh_ms.load(Ordering::Acquire),
            self.cfg.max_thread_fetches,
        );
        let due: Vec<(PullRequest, Option<Millis>)> = self
            .db(move |s| {
                s.with_conn(|c| {
                    let (hot, pinned) = hot_set(c, &me_s, cap)?;
                    let mut due = Vec::new();
                    for pr in hot {
                        let hydrated = read::pr_hydrated(c, pr.id)?;
                        let stale = match hydrated {
                            None => true,
                            Some(h) => pr.updated > h || now - h >= refresh,
                        };
                        if stale {
                            due.push((pr, hydrated));
                        }
                    }
                    // Pinned first (0002 FR-2 overflow order), then the
                    // longest unrefreshed.
                    due.sort_by_key(|(pr, h)| (!pinned.contains(&pr.id), h.unwrap_or(i64::MIN)));
                    due.truncate(max);
                    Ok(due)
                })
            })
            .await?;
        let mut events = Vec::new();
        let total = due.len() as u32;
        for (done, (pr, _)) in due.into_iter().enumerate() {
            self.tracker.progress(
                Lane::MyPrs,
                Progress {
                    done: done as u32,
                    total: Some(total),
                    unit: ProgressUnit::PullRequests,
                },
            );
            let list: wire::List<WireThread> = match self
                .fetch(
                    tier,
                    ApiRequest::get(self.url(
                        &format!(
                            "{}/_apis/git/repositories/{}/pullRequests/{}/threads",
                            pr.project_id, pr.repo_id, pr.id
                        ),
                        &[],
                    )),
                )
                .await
            {
                Ok(list) => list,
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if (status == 403 || status == 404) && self.is_pinned_pr(pr.id).await? =>
                {
                    self.mark_unavailable(PinSubject::Pr(pr.id), status).await?;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let me_s = me.to_owned();
            let now = self.clock.now_ms();
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        let enabled = baseline_done(tx, "pr.threads", "")?;
                        let mut em = Emitter {
                            tx,
                            me: &me_s,
                            enabled,
                            now,
                            new_ids: Vec::new(),
                        };
                        apply_threads(&mut em, &pr, list.value)?;
                        write::mark_pr_hydrated(tx, pr.id, now)?;
                        write::set_pin_title(tx, &PinSubject::Pr(pr.id), &pr.title)?;
                        write::set_pin_unavailable(tx, &PinSubject::Pr(pr.id), None)?;
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
        }
        self.db(move |s| {
            s.write(|tx| {
                let mut c = cursor_or_new(tx, "pr.threads", "")?;
                c.baseline_done = true;
                c.last_ok_ms = Some(now);
                write::put_cursor(tx, &c)
            })
        })
        .await?;
        Ok(events)
    }

    // ── work items ────────────────────────────────────────────────────────

    /// `wi.hot`: WIQL delta of my items, then batched GET of changed ids.
    /// The baseline has no date bound: my open items can be months old (FR-2
    /// "non-closed states or changed in the last 30 days" is a superset of
    /// "changed in the last 30 days"; views filter by state).
    async fn sync_hot_work_items(
        &self,
        tier: Tier,
        me: &adoken_model::Person,
    ) -> Result<Vec<i64>, SyncError> {
        let now = self.clock.now_ms();
        let cursor = self.db(|s| s.cursor("wi.hot", "")).await?;
        let since = cursor
            .as_ref()
            .and_then(|c| c.high_water.as_deref())
            .and_then(parse_time)
            .map(|t| t - OVERLAP_MS);
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let who = quote(me.unique_name.as_deref().unwrap_or(&me.id));
        let mut query = format!(
            "SELECT [System.Id] FROM WorkItems WHERE ([System.AssignedTo] = {who} OR [System.CreatedBy] = {who})"
        );
        if self.cfg.projects.is_some() {
            let names: Vec<String> = self
                .db(|s| s.synced_projects())
                .await?
                .iter()
                .map(|p| quote(&p.name))
                .collect();
            if names.is_empty() {
                return Ok(Vec::new());
            }
            query.push_str(&format!(
                " AND [System.TeamProject] IN ({})",
                names.join(", ")
            ));
        }
        if let Some(since) = since {
            query.push_str(&format!(
                " AND [System.ChangedDate] > '{}'",
                format_time(since)
            ));
        }
        query.push_str(" ORDER BY [System.ChangedDate] ASC");
        let ids: wire::WiqlResult = self
            .fetch(
                tier,
                ApiRequest::wiql(
                    self.url("_apis/wit/wiql", &[("timePrecision", "true")]),
                    &query,
                ),
            )
            .await?;
        let ids: Vec<i64> = ids.work_items.iter().map(|w| w.id).collect();
        let mut events = Vec::new();
        let chunks: Vec<Vec<i64>> = ids.chunks(WORK_ITEM_BATCH).map(<[i64]>::to_vec).collect();
        let last = chunks.len();
        let total = ids.len() as u32;
        let mut done = 0;
        for (i, chunk) in chunks.into_iter().enumerate() {
            self.tracker.progress(
                Lane::WorkItems,
                Progress {
                    done,
                    total: Some(total),
                    unit: ProgressUnit::WorkItems,
                },
            );
            done += chunk.len() as u32;
            let ids = chunk
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let list: wire::List<WireWorkItem> = self
                .fetch(
                    tier,
                    ApiRequest::get(self.url(
                        "_apis/wit/workitems",
                        &[
                            ("ids", &ids),
                            ("$expand", "relations"),
                            ("errorPolicy", "omit"),
                        ],
                    )),
                )
                .await?;
            let me_id = me.id.clone();
            let final_page = i + 1 == last;
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        let enabled = baseline_done(tx, "wi.hot", "")?;
                        let mut em = Emitter {
                            tx,
                            me: &me_id,
                            enabled,
                            now,
                            new_ids: Vec::new(),
                        };
                        let max_changed = apply_work_items(&mut em, list.value)?;
                        let mut c = cursor_or_new(tx, "wi.hot", "")?;
                        let prev = c.high_water.as_deref().and_then(parse_time).unwrap_or(0);
                        if let Some(m) = max_changed.filter(|m| *m > prev) {
                            c.high_water = Some(format_time(m));
                        }
                        if final_page {
                            c.baseline_done = true;
                            c.last_ok_ms = Some(now);
                        }
                        write::put_cursor(tx, &c)?;
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
        }
        if last == 0 {
            self.db(move |s| {
                s.write(|tx| {
                    let mut c = cursor_or_new(tx, "wi.hot", "")?;
                    c.baseline_done = true;
                    c.last_ok_ms = Some(now);
                    write::put_cursor(tx, &c)
                })
            })
            .await?;
        }
        Ok(events)
    }

    /// `wi.revisions`: title index via the reporting feed, resumable from the
    /// persisted continuation token (AC-12).
    async fn sync_revisions(
        &self,
        tier: Tier,
        project: &str,
        me: &str,
        so_far: &mut u32,
    ) -> Result<Vec<i64>, SyncError> {
        let mut events = Vec::new();
        for _ in 0..MAX_PAGES_PER_PASS {
            let project_s = project.to_owned();
            let cursor = self
                .db(move |s| s.cursor("wi.revisions", &project_s))
                .await?;
            let now = self.clock.now_ms();
            let start = self.cfg.revisions_horizon_ms.map(|h| format_time(now - h));
            let mut query: Vec<(&str, &str)> = vec![
                ("includeLatestOnly", "true"),
                ("includeDeleted", "true"),
                ("fields", REVISION_FIELDS),
            ];
            match (cursor.as_ref().and_then(|c| c.token.as_deref()), &start) {
                (Some(token), _) => query.push(("continuationToken", token)),
                (None, Some(start)) => query.push(("startDateTime", start)),
                (None, None) => {}
            }
            let url = self.url(
                &format!("{project}/_apis/wit/reporting/workitemrevisions"),
                &query,
            );
            let page: wire::RevisionsPage = self.fetch(tier, ApiRequest::get(url)).await?;
            let last = page.is_last_batch;
            *so_far += page.values.len() as u32;
            self.tracker.progress(
                Lane::Index,
                Progress {
                    done: *so_far,
                    total: None,
                    unit: ProgressUnit::IndexItems,
                },
            );
            let (me_s, project_s) = (me.to_owned(), project.to_owned());
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        let enabled = baseline_done(tx, "wi.revisions", &project_s)?;
                        let mut em = Emitter {
                            tx,
                            me: &me_s,
                            enabled,
                            now,
                            new_ids: Vec::new(),
                        };
                        // FR-12: the title index counts entries, not items.
                        activity::as_index(|| apply_work_items(&mut em, page.values))?;
                        let mut c = cursor_or_new(tx, "wi.revisions", &project_s)?;
                        if page.continuation_token.is_some() {
                            c.token = page.continuation_token;
                        }
                        if last {
                            c.baseline_done = true;
                            c.last_ok_ms = Some(now);
                        }
                        write::put_cursor(tx, &c)?;
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
            if last {
                break;
            }
        }
        Ok(events)
    }

    // ── builds ────────────────────────────────────────────────────────────

    /// `build.done`: runs I requested + PR runs for PRs I'm involved in,
    /// finished since `high_water - overlap`.
    async fn sync_done_builds(
        &self,
        tier: Tier,
        project: &str,
        me: &adoken_model::Person,
    ) -> Result<Vec<i64>, SyncError> {
        let now = self.clock.now_ms();
        let project_s = project.to_owned();
        let cursor = self.db(move |s| s.cursor("build.done", &project_s)).await?;
        let since = cursor
            .as_ref()
            .and_then(|c| c.high_water.as_deref())
            .and_then(parse_time)
            .map(|t| t - OVERLAP_MS)
            .unwrap_or(now - self.cfg.builds_window_ms);
        let min_time = format_time(since);
        let who = me.unique_name.clone().unwrap_or_else(|| me.id.clone());
        let path = format!("{project}/_apis/build/builds");
        let common = [
            ("minTime", min_time.as_str()),
            ("queryOrder", "finishTimeAscending"),
            ("statusFilter", "completed"),
        ];
        let mut q1 = common.to_vec();
        q1.push(("requestedFor", &who));
        let mine: wire::List<WireBuild> = self
            .fetch(tier, ApiRequest::get(self.url(&path, &q1)))
            .await?;
        let mut q2 = common.to_vec();
        q2.push(("reasonFilter", "pullRequest"));
        let pr_runs: wire::List<WireBuild> = self
            .fetch(tier, ApiRequest::get(self.url(&path, &q2)))
            .await?;
        let (me_id, project_s) = (me.id.clone(), project.to_owned());
        self.db(move |s| {
            s.write(|tx| {
                let enabled = baseline_done(tx, "build.done", &project_s)?;
                let mut em = Emitter {
                    tx,
                    me: &me_id,
                    enabled,
                    now,
                    new_ids: Vec::new(),
                };
                let max_finish =
                    apply_builds(&mut em, mine.value.into_iter().chain(pr_runs.value))?;
                let mut c = cursor_or_new(tx, "build.done", &project_s)?;
                let prev = c.high_water.as_deref().and_then(parse_time).unwrap_or(0);
                if let Some(m) = max_finish.filter(|m| *m > prev) {
                    c.high_water = Some(format_time(m));
                }
                c.baseline_done = true;
                c.last_ok_ms = Some(now);
                write::put_cursor(tx, &c)?;
                Ok(em.new_ids)
            })
        })
        .await
    }

    /// `build.live`: my in-progress runs, plus a by-id check of runs that
    /// were in progress and dropped out of the live list.
    async fn sync_live_builds(
        &self,
        tier: Tier,
        project: &str,
        me: &adoken_model::Person,
    ) -> Result<Vec<i64>, SyncError> {
        let who = me.unique_name.clone().unwrap_or_else(|| me.id.clone());
        let path = format!("{project}/_apis/build/builds");
        let live: wire::List<WireBuild> = self
            .fetch(
                tier,
                ApiRequest::get(self.url(
                    &path,
                    &[
                        ("statusFilter", "inProgress,notStarted"),
                        ("requestedFor", &who),
                    ],
                )),
            )
            .await?;
        let live_ids: HashSet<i64> = live.value.iter().map(|b| b.id).collect();
        let (me_id, project_s) = (me.id.clone(), project.to_owned());
        let watched: Vec<i64> = self
            .db(move |s| {
                s.with_conn(|c| {
                    let mut out = Vec::new();
                    for status in [RunStatus::InProgress, RunStatus::NotStarted] {
                        out.extend(
                            read::runs(
                                c,
                                &adoken_store::RunFilter {
                                    requested_for: Some(me_id.clone()),
                                    project_id: Some(project_s.clone()),
                                    status: Some(status),
                                    ..Default::default()
                                },
                            )?
                            .into_iter()
                            .map(|r| r.id),
                        );
                    }
                    // Spec 0014: the in-progress latest run of a pinned
                    // definition finishes on screen like my own runs.
                    for (project, definition, _) in pinned_definitions(c)? {
                        if project != project_s {
                            continue;
                        }
                        if let Some(run) = read::definition_runs(c, &project, definition, 1)?
                            .into_iter()
                            .find(|r| r.status != RunStatus::Completed)
                            && !out.contains(&run.id)
                        {
                            out.push(run.id);
                        }
                    }
                    Ok(out)
                })
            })
            .await?;
        let mut builds = live.value;
        for id in watched.into_iter().filter(|id| !live_ids.contains(id)) {
            let b: WireBuild = match self
                .fetch(
                    tier,
                    ApiRequest::get(self.url(&format!("{path}/{id}"), &[])),
                )
                .await
            {
                Ok(build) => build,
                Err(
                    e @ SyncError::Gov(GovError::Status {
                        status: 403 | 404, ..
                    }),
                ) => {
                    let subject = self
                        .db(move |s| {
                            s.with_conn(|c| {
                                let Some(run) = read::run(c, id)? else {
                                    return Ok(None);
                                };
                                let subject = PinSubject::Definition {
                                    project_id: run.project_id,
                                    id: run.pipeline_id,
                                };
                                Ok(read::pin(c, &subject)?.map(|_| subject))
                            })
                        })
                        .await?;
                    if let Some(subject) = subject {
                        let SyncError::Gov(GovError::Status { status, .. }) = &e else {
                            unreachable!()
                        };
                        self.mark_unavailable(subject, *status).await?;
                        continue;
                    }
                    return Err(e);
                }
                Err(e) => return Err(e),
            };
            builds.push(b);
        }
        let now = self.clock.now_ms();
        let (me_id, project_s) = (me.id.clone(), project.to_owned());
        self.db(move |s| {
            s.write(|tx| {
                let enabled = baseline_done(tx, "build.done", &project_s)?;
                let mut em = Emitter {
                    tx,
                    me: &me_id,
                    enabled,
                    now,
                    new_ids: Vec::new(),
                };
                apply_builds(&mut em, builds)?;
                Ok(em.new_ids)
            })
        })
        .await
    }

    async fn mark_definition_project_unavailable(
        &self,
        project: &str,
        status: u16,
    ) -> Result<bool, SyncError> {
        let project = project.to_owned();
        self.db(move |s| {
            s.write(|tx| {
                let mut marked = false;
                for (p, id, _) in pinned_definitions(tx)? {
                    if p == project {
                        marked = true;
                        write::set_pin_unavailable(
                            tx,
                            &PinSubject::Definition { project_id: p, id },
                            Some(if status == 403 {
                                PinUnavailable::Forbidden
                            } else {
                                PinUnavailable::Gone
                            }),
                        )?;
                    }
                }
                Ok(marked)
            })
        })
        .await
    }

    async fn sync_unselected_pinned_live(
        &self,
        tier: Tier,
        me: &str,
    ) -> Result<Vec<i64>, SyncError> {
        let watched = self
            .db(|s| {
                s.with_conn(|c| {
                    let selected: HashSet<String> = read::synced_projects(c)?
                        .into_iter()
                        .map(|p| p.id)
                        .collect();
                    let mut watched = Vec::new();
                    for (project, definition, _) in pinned_definitions(c)? {
                        if !selected.contains(&project)
                            && let Some(run) = read::definition_runs(c, &project, definition, 1)?
                                .into_iter()
                                .next()
                            && run.status != RunStatus::Completed
                        {
                            watched.push((project, definition, run.id));
                        }
                    }
                    Ok(watched)
                })
            })
            .await?;
        let mut events = Vec::new();
        for (project, definition, id) in watched {
            let url = self.url(&format!("{project}/_apis/build/builds/{id}"), &[]);
            let build: WireBuild = match self.fetch(tier, ApiRequest::get(url)).await {
                Ok(build) => build,
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if status == 403 || status == 404 =>
                {
                    self.mark_unavailable(
                        PinSubject::Definition {
                            project_id: project,
                            id: definition,
                        },
                        status,
                    )
                    .await?;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let (me, now) = (me.to_owned(), self.clock.now_ms());
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        let mut em = Emitter {
                            tx,
                            me: &me,
                            enabled: baseline_done(tx, "build.pins", &project)?,
                            now,
                            new_ids: Vec::new(),
                        };
                        let title = build.definition.name.clone();
                        apply_builds(&mut em, [build])?;
                        let subject = PinSubject::Definition {
                            project_id: project,
                            id: definition,
                        };
                        write::set_pin_title(tx, &subject, &title)?;
                        write::set_pin_unavailable(tx, &subject, None)?;
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
        }
        Ok(events)
    }

    /// `build.timeline`: stages of runs whose stored stages are stale (never
    /// fetched, fetched while running, or the run changed since), running
    /// runs first, at most `limit` per pass and only while the rolling
    /// 5-minute cost leaves room ([`TIMELINE_BUDGET`]). A finished run's
    /// timeline is final, so each finished run costs one request, once.
    async fn sync_timelines(&self, tier: Tier, lane: Lane, limit: usize) -> Result<(), SyncError> {
        let mut due = self
            .db(|s| s.with_conn(|c| read::runs_timeline_due(c, 200)))
            .await?;
        due.sort_by_key(|r| (r.status == RunStatus::Completed, Reverse(r.queued)));
        due.truncate(limit);
        self.fetch_timelines(tier, lane, due).await
    }

    /// Fetch and store the timelines of `runs`, in order, while the
    /// 5-minute budget allows ("timelines 2 of 4" on `lane`'s row).
    async fn fetch_timelines(
        &self,
        tier: Tier,
        lane: Lane,
        runs: Vec<PipelineRun>,
    ) -> Result<(), SyncError> {
        let total = runs.len() as u32;
        for (done, run) in runs.into_iter().enumerate() {
            self.tracker.progress(
                lane,
                Progress {
                    done: done as u32,
                    total: Some(total),
                    unit: ProgressUnit::Timelines,
                },
            );
            if self.gov.window_cost() + TIMELINE_COST > TIMELINE_BUDGET {
                log::debug!(target: "adoken_sync", "timelines deferred: 5-min budget used");
                break;
            }
            let url = self.url(
                &format!("{}/_apis/build/builds/{}/timeline", run.project_id, run.id),
                &[],
            );
            let records = match self
                .fetch::<wire::WireTimeline>(
                    tier,
                    ApiRequest::get(url).with_cost_hint(TIMELINE_COST),
                )
                .await
            {
                Ok(t) => t.records.unwrap_or_default(),
                // Deleted run (retention): nothing to show, stop asking.
                Err(SyncError::Gov(GovError::Status { status: 404, .. })) => Vec::new(),
                Err(e) => return Err(e),
            };
            let stages = wire::timeline_stages(&records);
            self.db(move |s| s.write(|tx| write::put_run_stages(tx, &run, &stages)))
                .await?;
        }
        Ok(())
    }

    /// Hot-set file lists (spec 0004 FR-1): active PRs I author or review
    /// whose stored list is missing or older than their last push, newest
    /// activity first, at most [`HOT_FILE_LISTS`] per pass and only while
    /// the rolling 5-minute cost stays under [`FILE_LIST_BUDGET`]. A PR
    /// without a new push costs nothing; opening one then paints its tree
    /// from the store. Blobs are left to the review view (FR-3).
    async fn sync_file_lists(&self, tier: Tier, me: &str) -> Result<(), SyncError> {
        let (me_s, cap) = (me.to_owned(), self.cfg.hot_cap);
        let due: Vec<PullRequest> = self
            .db(move |s| {
                s.with_conn(|c| {
                    let hot = read::prs(
                        c,
                        &PrFilter {
                            status: Some(PrStatus::Active),
                            involving: Some(me_s.clone()),
                            limit: Some(cap),
                            ..Default::default()
                        },
                    )?;
                    let mut due = Vec::new();
                    for pr in hot {
                        let synced = read::pr_file_sync(c, pr.id)?;
                        if crate::pr_files::files_due(&pr, synced.as_ref()) {
                            due.push(pr);
                        }
                        if due.len() == HOT_FILE_LISTS {
                            break;
                        }
                    }
                    Ok(due)
                })
            })
            .await?;
        for pr in due {
            if self.gov.window_cost() + crate::pr_files::CHANGES_COST > FILE_LIST_BUDGET {
                log::debug!(target: "adoken_sync", "file lists deferred: 5-min budget used");
                break;
            }
            match self.files.sync_files(tier, &pr).await {
                Ok(_) => {}
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if (status == 403 || status == 404) && self.is_pinned_pr(pr.id).await? =>
                {
                    self.mark_unavailable(PinSubject::Pr(pr.id), status).await?;
                }
                Err(SyncError::Gov(GovError::Status { status: 404, .. }))
                | Err(SyncError::Missing(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// `wi.comments`: discussions of my work items whose comment count
    /// changed (FR-2 mentions fallback), `MentionedInWorkItem` for new
    /// mentions of me. Silent until the first full drain of the queue
    /// (FR-13 baseline).
    async fn sync_wi_comments(&self, tier: Tier, me: &str) -> Result<Vec<i64>, SyncError> {
        let mut due = self
            .db(|s| s.with_conn(|c| read::work_items_comments_due(c, COMMENT_FETCHES + 1)))
            .await?;
        let drained = due.len() <= COMMENT_FETCHES;
        due.truncate(COMMENT_FETCHES);
        let mut events = Vec::new();
        for (id, project) in due {
            let path = format!("{project}/_apis/wit/workItems/{id}/comments");
            let mut comments: Vec<WireWiComment> = Vec::new();
            let mut token: Option<String> = None;
            let mut gone = false;
            for _ in 0..COMMENT_PAGES {
                let mut query = vec![("$top", "200")];
                if let Some(t) = token.as_deref() {
                    query.push(("continuationToken", t));
                }
                let url = self.url_v(&path, &query, COMMENTS_VERSION);
                match self
                    .fetch::<wire::WireCommentList>(tier, ApiRequest::get(url))
                    .await
                {
                    Ok(page) => {
                        comments.extend(page.comments);
                        token = page.continuation_token.filter(|t| !t.is_empty());
                    }
                    // Deleted or moved out of reach: forget its discussion.
                    Err(SyncError::Gov(GovError::Status { status: 404, .. })) => {
                        gone = true;
                        token = None;
                    }
                    Err(e) => return Err(e),
                }
                if token.is_none() {
                    break;
                }
            }
            let me_s = me.to_owned();
            let now = self.clock.now_ms();
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        let enabled = baseline_done(tx, "wi.comments", "")?;
                        let mut em = Emitter {
                            tx,
                            me: &me_s,
                            enabled,
                            now,
                            new_ids: Vec::new(),
                        };
                        if !gone {
                            apply_wi_comments(&mut em, id, comments)?;
                        }
                        write::set_wi_comments_due(tx, id, false)?;
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
        }
        if drained {
            let now = self.clock.now_ms();
            self.db(move |s| {
                s.write(|tx| {
                    let mut c = cursor_or_new(tx, "wi.comments", "")?;
                    c.baseline_done = true;
                    c.last_ok_ms = Some(now);
                    write::put_cursor(tx, &c)
                })
            })
            .await?;
        }
        Ok(events)
    }

    /// User request ("Load older pipeline runs"): the next
    /// [`OLDER_RUNS_PAGE`] of my finished runs older than anything stored,
    /// per project, then their timelines. The 7-day `build.done` window
    /// (FR-2) never reaches them. Old runs are history, never news: no
    /// events. Returns how many runs arrived.
    pub async fn load_older_runs(&self) -> Result<usize, SyncError> {
        self.set_phase(Phase::Syncing(Tier::User));
        let pass = self.tracker.begin_user_pass(Trigger::LoadOlderRuns);
        let result = pass.scope(self.load_older_runs_inner()).await;
        pass.finish(Outcome::of(&result.as_ref().map(|_| ()), self.host()), 0);
        match &result {
            Err(SyncError::Gov(GovError::Unauthorized)) => self.set_phase(Phase::Unauthorized),
            Err(SyncError::Gov(GovError::Offline(_))) => self.set_phase(Phase::Offline),
            _ => self.set_phase(self.rest_phase()),
        }
        result
    }

    async fn load_older_runs_inner(&self) -> Result<usize, SyncError> {
        let me = self.db(|s| s.me()).await?.ok_or(SyncError::Missing("me"))?;
        let who = me.unique_name.clone().unwrap_or_else(|| me.id.clone());
        let mut loaded: Vec<i64> = Vec::new();
        for project in self.db(|s| s.synced_projects()).await? {
            let (p, me_id) = (project.id.clone(), me.id.clone());
            let (cursor, oldest) = self
                .db(move |s| {
                    s.with_conn(|c| {
                        Ok((
                            read::cursor(c, "build.older", &p)?,
                            read::oldest_run_finish(c, &p, &me_id)?,
                        ))
                    })
                })
                .await?;
            if cursor.as_ref().is_some_and(|c| c.baseline_done) {
                continue; // Reached the project's first run.
            }
            let before = cursor
                .as_ref()
                .and_then(|c| c.high_water.as_deref())
                .and_then(parse_time)
                .or(oldest)
                .unwrap_or_else(|| self.clock.now_ms());
            // Strictly older than anything loaded (`maxTime` is inclusive).
            let max_time = format_time(before - 1);
            let top = OLDER_RUNS_PAGE.to_string();
            let page: wire::List<WireBuild> = self
                .fetch(
                    Tier::User,
                    ApiRequest::get(self.url(
                        &format!("{}/_apis/build/builds", project.id),
                        &[
                            ("requestedFor", &who),
                            ("statusFilter", "completed"),
                            ("queryOrder", "finishTimeDescending"),
                            ("maxTime", &max_time),
                            ("$top", &top),
                        ],
                    )),
                )
                .await?;
            let exhausted = page.value.len() < OLDER_RUNS_PAGE;
            let next = page
                .value
                .iter()
                .filter_map(|b| b.finish_time.as_deref().and_then(parse_time))
                .min()
                .map_or(before, |m| m.min(before));
            loaded.extend(page.value.iter().map(|b| b.id));
            let (me_id, p) = (me.id.clone(), project.id.clone());
            let now = self.clock.now_ms();
            self.db(move |s| {
                s.write(|tx| {
                    let mut em = Emitter {
                        tx,
                        me: &me_id,
                        enabled: false,
                        now,
                        new_ids: Vec::new(),
                    };
                    apply_builds(&mut em, page.value)?;
                    let mut c = cursor_or_new(tx, "build.older", &p)?;
                    c.high_water = Some(format_time(next));
                    c.baseline_done = exhausted;
                    c.last_ok_ms = Some(now);
                    write::put_cursor(tx, &c)
                })
            })
            .await?;
        }
        // Their stages now (newest first, budget permitting); the warm
        // backlog picks up the rest.
        let count = loaded.len();
        loaded.truncate(WARM_TIMELINES);
        let runs = self
            .db(move |s| {
                s.with_conn(|c| {
                    let mut runs = Vec::new();
                    for id in loaded {
                        runs.extend(read::run(c, id)?);
                    }
                    Ok(runs)
                })
            })
            .await?;
        self.fetch_timelines(Tier::User, Lane::DoneRuns, runs)
            .await?;
        Ok(count)
    }

    // ── pins (spec 0014 FR-11, 0002 FR-2) ──────────────────────────────────

    /// Pinned PRs outside the synced projects (or not stored yet), one
    /// `GET {org}/_apis/git/pullrequests/{id}` each per warm pass. 404 / 403
    /// mark the pin "Not available" (FR-13); the pin is never removed.
    async fn sync_pinned_prs(&self, tier: Tier, me: &str) -> Result<Vec<i64>, SyncError> {
        let due: Vec<i64> = self
            .db(|s| {
                s.with_conn(|c| {
                    let synced: HashSet<String> = read::synced_projects(c)?
                        .into_iter()
                        .map(|p| p.id)
                        .collect();
                    let mut due = Vec::new();
                    for pin in read::pins(c)? {
                        let PinSubject::Pr(id) = pin.subject else {
                            continue;
                        };
                        match read::pr(c, id)? {
                            Some(pr) if synced.contains(&pr.project_id) => {}
                            _ => due.push(id),
                        }
                    }
                    Ok(due)
                })
            })
            .await?;
        let mut events = Vec::new();
        for id in due {
            let subject = PinSubject::Pr(id);
            let url = self.url(&format!("_apis/git/pullrequests/{id}"), &[]);
            match self
                .fetch::<GitPullRequest>(tier, ApiRequest::get(url))
                .await
            {
                Ok(mut pr) => {
                    if !self.resolve_push_actor(tier, &mut pr, me).await? {
                        continue;
                    }
                    let (me_s, now) = (me.to_owned(), self.clock.now_ms());
                    events.extend(
                        self.db(move |s| {
                            s.write(|tx| {
                                write::upsert_project_ref(
                                    tx,
                                    &adoken_model::Project {
                                        id: pr.repository.project.id.clone(),
                                        name: pr.repository.project.name.clone(),
                                    },
                                )?;
                                let mut em = Emitter {
                                    tx,
                                    me: &me_s,
                                    enabled: true,
                                    now,
                                    new_ids: Vec::new(),
                                };
                                apply_pr(&mut em, &pr)?;
                                write::set_pin_title(tx, &subject, &pr.title)?;
                                write::set_pin_unavailable(tx, &subject, None)?;
                                Ok(em.new_ids)
                            })
                        })
                        .await?,
                    );
                }
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if status == 404 || status == 403 =>
                {
                    self.mark_unavailable(subject, status).await?;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(events)
    }

    /// Pins outside the WIQL delta's coverage, in one batch per warm pass.
    /// `errorPolicy: omit` marks unreadable items "Not available".
    async fn sync_pinned_work_items(&self, tier: Tier, me: &str) -> Result<Vec<i64>, SyncError> {
        let me_s = me.to_owned();
        let project_filtered = self.cfg.projects.is_some();
        let ids: Vec<i64> = self
            .db(move |s| {
                s.with_conn(|c| {
                    let synced: HashSet<String> = read::synced_projects(c)?
                        .into_iter()
                        .map(|p| p.id)
                        .collect();
                    let mut ids = Vec::new();
                    for pin in read::pins(c)? {
                        let PinSubject::WorkItem(id) = pin.subject else {
                            continue;
                        };
                        let covered = read::work_item(c, id)?.is_some_and(|wi| {
                            assigned_now_or_created(&wi, &me_s)
                                && (!project_filtered || synced.contains(&wi.project_id))
                        });
                        if !covered {
                            ids.push(id);
                        }
                    }
                    Ok(ids)
                })
            })
            .await?;
        let mut events = Vec::new();
        for chunk in ids.chunks(WORK_ITEM_BATCH) {
            let url = self.url("_apis/wit/workitemsbatch", &[]);
            let list: wire::List<Option<WireWorkItem>> = match self
                .fetch(tier, ApiRequest::workitems_batch(url, chunk))
                .await
            {
                Ok(list) => list,
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if status == 404 || status == 403 =>
                {
                    for &id in chunk {
                        self.mark_unavailable(PinSubject::WorkItem(id), status)
                            .await?;
                    }
                    continue;
                }
                Err(e) => return Err(e),
            };
            let items: Vec<WireWorkItem> = list.value.into_iter().flatten().collect();
            let (projects, failures) = self.unknown_projects(tier, &items).await?;
            let returned: HashMap<i64, (i64, Option<String>)> = items
                .iter()
                .filter(|w| !w.is_deleted())
                .map(|w| (w.id, (w.rev, w.project_name().map(str::to_owned))))
                .collect();
            let requested = chunk.to_vec();
            let (me_s, now) = (me.to_owned(), self.clock.now_ms());
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        for p in &projects {
                            write::upsert_project_ref(tx, p)?;
                        }
                        let enabled = baseline_done(tx, "wi.hot", "")?;
                        let mut em = Emitter {
                            tx,
                            me: &me_s,
                            enabled,
                            now,
                            new_ids: Vec::new(),
                        };
                        apply_work_items(&mut em, items)?;
                        for id in requested {
                            let subject = PinSubject::WorkItem(id);
                            let failure = returned
                                .get(&id)
                                .and_then(|(_, project)| project.as_ref())
                                .and_then(|project| failures.get(project))
                                .copied();
                            if let Some(reason) = failure {
                                write::set_pin_unavailable(tx, &subject, Some(reason))?;
                            } else if let Some(wi) = read::work_item(tx, id)?
                                && returned.get(&id).is_some_and(|(rev, _)| wi.rev >= *rev)
                            {
                                write::set_pin_title(tx, &subject, &wi.title)?;
                                write::set_pin_unavailable(tx, &subject, None)?;
                            } else {
                                write::set_pin_unavailable(
                                    tx,
                                    &subject,
                                    Some(PinUnavailable::Gone),
                                )?;
                            }
                        }
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
        }
        Ok(events)
    }

    /// The latest run of every pinned definition: one
    /// `builds?definitions=…&maxBuildsPerDefinition=1` per project with
    /// pinned definitions per warm pass. Unpinning drops the id from the
    /// next pass. A failed run of a pinned definition that finished after it
    /// was pinned emits `BuildCompleted` (notify: P2, 0005 FR-3).
    async fn sync_pinned_definitions(&self, tier: Tier, me: &str) -> Result<Vec<i64>, SyncError> {
        let pinned = self.db(|s| s.with_conn(pinned_definitions)).await?;
        let mut by_project: Vec<(String, Vec<i64>)> = Vec::new();
        for (project, definition, _) in pinned {
            match by_project.iter_mut().find(|(p, _)| *p == project) {
                Some((_, ids)) => ids.push(definition),
                None => by_project.push((project, vec![definition])),
            }
        }
        let mut events = Vec::new();
        for (project, definitions) in by_project {
            let ids = definitions
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let url = self.url(
                &format!("{project}/_apis/build/builds"),
                &[("definitions", &ids), ("maxBuildsPerDefinition", "1")],
            );
            let latest: wire::List<WireBuild> = match self.fetch(tier, ApiRequest::get(url)).await {
                Ok(list) => list,
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if status == 403 || status == 404 =>
                {
                    self.mark_definition_project_unavailable(&project, status)
                        .await?;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let returned: HashSet<i64> = latest.value.iter().map(|b| b.definition.id).collect();
            let mut empty = Vec::new();
            let mut unavailable = Vec::new();
            for &id in definitions.iter().filter(|id| !returned.contains(*id)) {
                let url = self.url(&format!("{project}/_apis/build/definitions/{id}"), &[]);
                match self
                    .fetch::<wire::WireDefinition>(tier, ApiRequest::get(url))
                    .await
                {
                    Ok(definition) => empty.push(definition),
                    Err(SyncError::Gov(GovError::Status { status, .. }))
                        if status == 403 || status == 404 =>
                    {
                        unavailable.push((
                            id,
                            if status == 403 {
                                PinUnavailable::Forbidden
                            } else {
                                PinUnavailable::Gone
                            },
                        ));
                    }
                    Err(e) => return Err(e),
                }
            }
            let (me_s, now) = (me.to_owned(), self.clock.now_ms());
            events.extend(
                self.db(move |s| {
                    s.write(|tx| {
                        let enabled = baseline_done(tx, "build.done", &project)?
                            || baseline_done(tx, "build.pins", &project)?;
                        let mut em = Emitter {
                            tx,
                            me: &me_s,
                            enabled,
                            now,
                            new_ids: Vec::new(),
                        };
                        apply_builds(&mut em, latest.value)?;
                        for id in &returned {
                            let subject = PinSubject::Definition { project_id: project.clone(), id: *id };
                            if let Some(run) = read::definition_runs(tx, &project, *id, 1)?.first() {
                                write::set_pin_title(tx, &subject, &run.pipeline_name)?;
                            }
                            write::set_pin_unavailable(tx, &subject, None)?;
                        }
                        for definition in empty {
                            write::upsert_project_ref(tx, &adoken_model::Project {
                                id: definition.project.id.clone(), name: definition.project.name,
                            })?;
                            let def = definition.definition;
                            write::upsert_pipeline(tx, &Pipeline {
                                id: def.id, project_id: definition.project.id,
                                name: def.name.clone(), folder: def.path,
                            })?;
                            tx.prepare_cached(
                                "DELETE FROM pipeline_run WHERE project_id = ?1 AND definition_id = ?2",
                            )?.execute(adoken_store::rusqlite::params![project, def.id])?;
                            let subject = PinSubject::Definition { project_id: project.clone(), id: def.id };
                            write::set_pin_title(tx, &subject, &def.name)?;
                            write::set_pin_unavailable(tx, &subject, None)?;
                        }
                        for (id, reason) in unavailable {
                            write::set_pin_unavailable(tx, &PinSubject::Definition {
                                project_id: project.clone(), id,
                            }, Some(reason))?;
                        }
                        let mut cursor = cursor_or_new(tx, "build.pins", &project)?;
                        cursor.baseline_done = true;
                        cursor.last_ok_ms = Some(now);
                        write::put_cursor(tx, &cursor)?;
                        Ok(em.new_ids)
                    })
                })
                .await?,
            );
        }
        Ok(events)
    }

    /// Projects of `items` the store doesn't know: a pinned work item in a
    /// project that isn't selected carries only the project name. One
    /// `GET {org}/_apis/projects/{name}` per unknown name; the caller stores
    /// them as project refs (not synced), so later passes resolve the name
    /// from the store. A name the token can't read is skipped.
    async fn unknown_projects(
        &self,
        tier: Tier,
        items: &[WireWorkItem],
    ) -> Result<(Vec<adoken_model::Project>, HashMap<String, PinUnavailable>), SyncError> {
        let mut names: Vec<String> = items
            .iter()
            .filter(|w| !w.is_deleted())
            .filter_map(|w| w.project_name().map(str::to_owned))
            .collect();
        if names.is_empty() {
            return Ok((Vec::new(), HashMap::new()));
        }
        let known: HashSet<String> = self
            .db(|s| s.projects())
            .await?
            .into_iter()
            .map(|p| p.name)
            .collect();
        names.retain(|n| !known.contains(n));
        names.sort_unstable();
        names.dedup();
        let mut found = Vec::with_capacity(names.len());
        let mut failures = HashMap::new();
        for name in names {
            let mut url = self.url("_apis/projects", &[]);
            url.path_segments_mut()
                .expect("ADO URLs have a path")
                .push(&name);
            match self
                .fetch::<wire::ProjectRef>(tier, ApiRequest::get(url))
                .await
            {
                Ok(p) => found.push(adoken_model::Project {
                    id: p.id,
                    name: if p.name.is_empty() { name } else { p.name },
                }),
                Err(SyncError::Gov(GovError::Status { status, .. }))
                    if status == 404 || status == 403 =>
                {
                    failures.insert(
                        name,
                        if status == 403 {
                            PinUnavailable::Forbidden
                        } else {
                            PinUnavailable::Gone
                        },
                    );
                }
                Err(e) => return Err(e),
            }
        }
        Ok((found, failures))
    }

    async fn mark_unavailable(&self, subject: PinSubject, status: u16) -> Result<(), SyncError> {
        let reason = if status == 403 {
            PinUnavailable::Forbidden
        } else {
            PinUnavailable::Gone
        };
        self.db(move |s| s.write(|tx| write::set_pin_unavailable(tx, &subject, Some(reason))))
            .await
    }
}

/// The PR hot set (0002 FR-2): pinned active PRs first, then active PRs I
/// author or review, capped at `cap` (pins rank first in the overflow).
/// Also returns the pinned ids.
fn hot_set(
    c: &Connection,
    me: &str,
    cap: usize,
) -> adoken_store::Result<(Vec<PullRequest>, HashSet<i64>)> {
    let mut pinned = HashSet::new();
    let mut hot = Vec::new();
    for pin in read::pins(c)? {
        if let PinSubject::Pr(id) = pin.subject {
            pinned.insert(id);
            if let Some(pr) = read::pr(c, id)?.filter(|pr| pr.status == PrStatus::Active) {
                hot.push(pr);
            }
        }
    }
    let mine = read::prs(
        c,
        &PrFilter {
            status: Some(PrStatus::Active),
            involving: Some(me.to_owned()),
            limit: Some(cap),
            ..Default::default()
        },
    )?;
    hot.extend(mine.into_iter().filter(|pr| !pinned.contains(&pr.id)));
    hot.truncate(cap);
    Ok((hot, pinned))
}

/// Pinned definitions: `(project id, definition id, pinned at)`.
fn pinned_definitions(c: &Connection) -> adoken_store::Result<Vec<(String, i64, Millis)>> {
    Ok(read::pins(c)?
        .into_iter()
        .filter_map(|pin| match pin.subject {
            PinSubject::Definition { project_id, id } => Some((project_id, id, pin.pinned_at)),
            _ => None,
        })
        .collect())
}

/// What each lane of a pass books at the governor (the pass's tier is the
/// scheduler's alone while it runs, so the difference is the lane's cost).
struct LaneMeter<'a> {
    gov: &'a Governor,
    tier: Tier,
    mark: f64,
}

impl<'a> LaneMeter<'a> {
    fn new(gov: &'a Governor, tier: Tier) -> Self {
        LaneMeter {
            gov,
            tier,
            mark: gov.spent(tier),
        }
    }

    /// Book everything since the last mark to `lane`.
    fn book(&mut self, lane: Lane, report: &mut PassReport) {
        let now = self.gov.spent(self.tier);
        let cost = now - self.mark;
        self.mark = now;
        match report.lane_costs.iter_mut().find(|(l, _)| *l == lane) {
            Some((_, c)) => *c += cost,
            None => report.lane_costs.push((lane, cost)),
        }
    }

    fn skip(&mut self) {
        self.mark = self.gov.spent(self.tier);
    }
}

// ── appliers (run inside the page transaction) ───────────────────────────

/// FR-11 "reviewer-set version": stable hash of the sorted reviewer ids.
fn reviewer_set_version(reviewers: &[Reviewer]) -> String {
    let mut ids: Vec<&str> = reviewers.iter().map(|r| r.person.as_str()).collect();
    ids.sort_unstable();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ids.join(",").bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("reviewers:{h:016x}")
}

/// Merge a list payload with the stored row (derived timestamps survive),
/// upsert, and emit PR events.
pub(crate) fn apply_pr(em: &mut Emitter<'_>, w: &GitPullRequest) -> adoken_store::Result<()> {
    let tx = em.tx;
    let now = em.now;
    write::upsert_person(tx, &w.created_by.person(), false)?;
    if let Some(actor) = &w.push_actor {
        write::upsert_person(tx, &actor.person(), false)?;
    }
    for r in &w.reviewers {
        write::upsert_person(tx, &r.identity.person(), false)?;
    }
    if read::person(tx, &w.created_by.id)?.is_none() {
        return Ok(());
    }
    // Repos can appear between structure passes.
    let repo_known: bool = tx
        .prepare_cached("SELECT 1 FROM repo WHERE id = ?1")?
        .exists([&w.repository.id])?;
    if !repo_known {
        write::upsert_repo(
            tx,
            &Repo {
                id: w.repository.id.clone(),
                project_id: w.repository.project.id.clone(),
                name: w.repository.name.clone(),
                default_branch: w.repository.default_branch.clone(),
            },
        )?;
    }
    let old = read::pr(tx, w.pull_request_id)?;
    let commit = w
        .last_merge_source_commit
        .as_ref()
        .map(|c| c.commit_id.clone());
    let reviewers: Vec<Reviewer> = w
        .reviewers
        .iter()
        .map(|r| {
            let vote = GitPullRequest::vote_of(r);
            let prev = old
                .as_ref()
                .and_then(|o| o.reviewers.iter().find(|x| x.person == r.identity.id));
            Reviewer {
                person: r.identity.id.clone(),
                vote,
                is_required: r.is_required,
                has_declined: r.has_declined,
                added_at: prev.and_then(|p| p.added_at).or(Some(now)),
                voted_at: match prev {
                    Some(p) if p.vote == vote => p.voted_at,
                    _ if vote == Vote::NoVote => None,
                    _ => Some(now),
                },
            }
        })
        .collect();
    let created = w.created();
    let pushed = old.as_ref().is_some_and(|o| o.last_source_commit != commit);
    let mut pr = PullRequest {
        id: w.pull_request_id,
        repo_id: w.repository.id.clone(),
        project_id: w.repository.project.id.clone(),
        title: w.title.clone(),
        description: w.description.clone(),
        author: w.created_by.id.clone(),
        reviewers,
        status: w.status(),
        is_draft: w.is_draft,
        merge_status: w.merge_status(),
        source_branch: w.source_ref_name.clone(),
        target_branch: w.target_ref_name.clone(),
        last_source_commit: commit.clone(),
        created,
        updated: created.max(w.closed().unwrap_or(0)),
        closed: w.closed(),
        iterations: old
            .as_ref()
            .map(|o| o.iterations + pushed as u32)
            .unwrap_or(1),
        last_push: if pushed {
            Some(now)
        } else {
            old.as_ref().and_then(|o| o.last_push).or(Some(created))
        },
    };
    if let Some(o) = &old {
        let changed = write::pr_fingerprint(o) != write::pr_fingerprint(&pr);
        pr.updated = if changed {
            now
        } else {
            o.updated.max(pr.updated)
        };
    }
    // Spec 0004 FR-10: my accepted Complete ends when the PR closes, the
    // queued merge fails (the review then shows why and offers it again),
    // or it never landed (then Complete is offered again too).
    let completing = read::completing(tx, pr.id)?;
    if let Some((_, since)) = completing
        && (pr.status != PrStatus::Active
            || pr.merge_status.is_failure()
            || now - since >= COMPLETING_STALE_MS)
    {
        write::clear_completing(tx, pr.id)?;
    }
    let completing = completing.is_some();
    let changed = write::upsert_pr(tx, &pr)?.changed();
    write::set_pin_title(tx, &PinSubject::Pr(pr.id), &pr.title)?;
    write::set_pin_unavailable(tx, &PinSubject::Pr(pr.id), None)?;
    if !changed {
        return Ok(());
    }
    activity::note(|c, _| c.note_pr(pr.id, pr_change_word(old.as_ref(), &pr, pushed)));
    let me = em.me.to_owned();
    let mine = pr.author == me;
    let reviewing = pr.is_reviewer(&me);
    // Spec 0014 FR-10: a pinned PR gets the events of my own PRs so its
    // change dot can read them; notify drops the ones whose only relation
    // to me is the pin (FR-16).
    let pinned = read::pin(tx, &PinSubject::Pr(pr.id))?.is_some();
    let subject = Subject::Pr(pr.id);
    let label = format!("!{} {}", pr.id, pr.title);
    let author_name = name(tx, &pr.author);
    // Spec 0004 FR-12: auto-complete I set in adoken, with the commit I saw.
    let guard = read::auto_complete(tx, pr.id)?;
    if guard.is_some() && pr.status != PrStatus::Active {
        write::clear_auto_complete(tx, pr.id)?;
    }
    let Some(old) = old else {
        if reviewing {
            em.emit(
                SyncEventKind::ReviewRequested,
                subject,
                reviewer_set_version(&pr.reviewers),
                Some(&pr.author),
                pr.created.max(now - MAX_EVENT_AGE_MS + MINUTE),
                format!("{author_name} requested your review on {label}"),
            )?;
        }
        return Ok(());
    };
    if reviewing && !old.is_reviewer(&me) {
        em.emit(
            SyncEventKind::ReviewRequested,
            subject.clone(),
            reviewer_set_version(&pr.reviewers),
            Some(&pr.author),
            now,
            format!("{author_name} requested your review on {label}"),
        )?;
    }
    if mine || pinned {
        for r in &pr.reviewers {
            if old.vote_of(&r.person).unwrap_or(Vote::NoVote) != r.vote && r.vote != Vote::NoVote {
                let who = name(tx, &r.person);
                em.emit(
                    SyncEventKind::PrVoteChanged,
                    subject.clone(),
                    format!(
                        "{}:{}:{}",
                        r.person,
                        r.vote.as_i8(),
                        pr.last_source_commit.as_deref().unwrap_or("")
                    ),
                    Some(&r.person),
                    now,
                    format!("{who} voted “{}” on {label}", r.vote.label()),
                )?;
            }
        }
        if pr.merge_status == adoken_model::MergeStatus::Conflicts
            && old.merge_status != adoken_model::MergeStatus::Conflicts
        {
            em.emit(
                SyncEventKind::PrMergeConflict,
                subject.clone(),
                pr.last_source_commit.clone().unwrap_or_default(),
                None,
                now,
                format!("{label} has merge conflicts"),
            )?;
        }
    }
    if pushed && (reviewing || pinned || guard.is_some()) {
        let unseen = guard
            .as_ref()
            .is_some_and(|(reviewed, _)| Some(reviewed) != commit.as_ref());
        em.emit(
            SyncEventKind::PrPushed,
            subject.clone(),
            commit.clone().unwrap_or_default(),
            w.push_actor.as_ref().map(|actor| actor.id.as_str()),
            now,
            if unseen {
                format!("New commits on !{} after you set auto-complete", pr.id)
            } else {
                format!(
                    "{} pushed to {label}",
                    w.push_actor
                        .as_ref()
                        .map_or_else(|| "Someone".to_owned(), |actor| name(tx, &actor.id),)
                )
            },
        )?;
    }
    if (mine || reviewing || pinned || completing)
        && (old.status != pr.status || old.is_draft != pr.is_draft)
    {
        let what = status_key(pr.status, pr.is_draft);
        // Completed / abandoned by whoever closed it (me when I merged it in
        // adoken, also when Azure DevOps finished my queued Complete later:
        // Ken's merge moment), else attributed to the author.
        let actor = w
            .closed_by
            .as_ref()
            .filter(|_| pr.status != PrStatus::Active)
            .map_or(pr.author.as_str(), |who| who.id.as_str());
        em.emit(
            SyncEventKind::PrStatusChanged,
            subject,
            what.to_owned(),
            Some(actor),
            pr.closed.unwrap_or(now),
            format!("{label} is now {what}"),
        )?;
    }
    Ok(())
}

/// The Sync tab's word for a changed PR row (spec 0016 FR-12, design §4.4).
fn pr_change_word(old: Option<&PullRequest>, pr: &PullRequest, pushed: bool) -> ChangeWord {
    let Some(old) = old else {
        return ChangeWord::New;
    };
    if old.status != pr.status {
        return match pr.status {
            PrStatus::Completed => ChangeWord::Completed,
            PrStatus::Abandoned => ChangeWord::Abandoned,
            _ => ChangeWord::Updated,
        };
    }
    if pushed {
        return ChangeWord::Pushed;
    }
    let votes_changed = pr
        .reviewers
        .iter()
        .any(|r| old.vote_of(&r.person).unwrap_or(Vote::NoVote) != r.vote);
    if votes_changed {
        ChangeWord::VoteChanged
    } else {
        ChangeWord::Updated
    }
}

fn wi_change_word(
    old: Option<&adoken_model::WorkItem>,
    wi: &adoken_model::WorkItem,
    me: &str,
) -> ChangeWord {
    let Some(old) = old else {
        return ChangeWord::New;
    };
    if old.state != wi.state {
        ChangeWord::State(wi.state.clone())
    } else if wi.assigned_to.as_deref() == Some(me) && old.assigned_to.as_deref() != Some(me) {
        ChangeWord::AssignedToYou
    } else {
        ChangeWord::Updated
    }
}

fn run_change_word(old: Option<&PipelineRun>, run: &PipelineRun) -> ChangeWord {
    let finished =
        run.status == RunStatus::Completed && old.is_none_or(|o| o.status != RunStatus::Completed);
    match (finished, run.result, old) {
        (true, Some(result), _) => ChangeWord::Finished(result),
        (_, _, None) => ChangeWord::Started,
        _ => ChangeWord::Updated,
    }
}

/// `PrStatusChanged` dedupe key: "draft" / "published" while active, else
/// the status ("completed", "abandoned"). Writes emit the same key.
pub(crate) fn status_key(status: PrStatus, draft: bool) -> &'static str {
    match (status, draft) {
        (PrStatus::Active, true) => "draft",
        (PrStatus::Active, false) => "published",
        (s, _) => s.as_str(),
    }
}

fn apply_threads(
    em: &mut Emitter<'_>,
    pr: &PullRequest,
    threads: Vec<WireThread>,
) -> adoken_store::Result<()> {
    let tx = em.tx;
    let me = em.me.to_owned();
    let old: HashMap<i64, adoken_model::PrThread> = read::pr_threads(tx, pr.id)?
        .into_iter()
        .map(|t| (t.id, t))
        .collect();
    // A pinned PR's comments feed its change dot (spec 0014 FR-10). The
    // first fetch after pinning is a baseline: pinning raises nothing.
    let involved = pr.author == me
        || pr.is_reviewer(&me)
        || (read::pin(tx, &PinSubject::Pr(pr.id))?.is_some()
            && read::pr_hydrated(tx, pr.id)?.is_some());
    // Bodies carry lowercased mention ids (`wire::normalize_mentions`).
    let mention = format!("@<{}>", me.to_ascii_lowercase());
    let mut latest = 0;
    for w in threads {
        for p in w.people() {
            write::upsert_person(tx, &p, false)?;
        }
        let t = w.into_model(pr.id);
        let new_ids = write::upsert_thread(tx, &t)?;
        let new_comments = t
            .comments
            .iter()
            .filter(|c| !c.is_system && !c.is_deleted && new_ids.contains(&c.id))
            .count() as u32;
        activity::note(|c, _| c.note_pr_comments(pr.id, new_comments));
        let prev = old.get(&t.id);
        let i_commented = t.comments.iter().any(|c| c.author == me);
        for c in &t.comments {
            latest = latest.max(c.updated);
            if c.is_system || c.is_deleted {
                continue;
            }
            let prev_body = prev
                .and_then(|p| p.comments.iter().find(|x| x.id == c.id))
                .map(|x| x.body.as_str());
            let is_new = new_ids.contains(&c.id);
            let who = name(tx, &c.author);
            let key = format!("{}:{}", t.id, c.id);
            let newly_mentions =
                c.body.contains(&mention) && !prev_body.is_some_and(|b| b.contains(&mention));
            if newly_mentions && c.author != me {
                em.emit(
                    SyncEventKind::MentionedInPr,
                    Subject::Pr(pr.id),
                    key,
                    Some(&c.author),
                    c.updated,
                    format!("{who} mentioned you in !{} {}", pr.id, pr.title),
                )?;
            } else if is_new && (involved || i_commented) {
                em.emit(
                    SyncEventKind::PrCommentAdded,
                    Subject::Pr(pr.id),
                    key,
                    Some(&c.author),
                    c.created,
                    format!("{who} commented on !{} {}", pr.id, pr.title),
                )?;
            }
        }
        if let Some(p) = prev
            && !p.status.is_resolved()
            && t.status.is_resolved()
            && t.comments.first().is_some_and(|c| c.author == me)
        {
            em.emit(
                SyncEventKind::PrThreadResolved,
                Subject::Pr(pr.id),
                format!("{}:{}", t.id, t.status.as_str()),
                None,
                t.updated,
                format!("Your thread on !{} was marked {}", pr.id, t.status.as_str()),
            )?;
        }
    }
    if latest > 0 {
        write::touch_pr(tx, pr.id, latest)?;
    }
    Ok(())
}

/// Maps the reporting feed's `"Name <unique name>"` identities to people the
/// store already knows: by unique name (case-insensitive), else by a display
/// name only one person has.
struct IdentityResolver {
    by_unique: HashMap<String, String>,
    /// `None` when several people share the display name.
    by_display: HashMap<String, Option<String>>,
}

impl IdentityResolver {
    fn load(tx: &Connection) -> adoken_store::Result<Self> {
        let mut by_unique = HashMap::new();
        let mut by_display: HashMap<String, Option<String>> = HashMap::new();
        for p in read::people(tx)? {
            if let Some(u) = &p.unique_name {
                by_unique.insert(u.to_lowercase(), p.id.clone());
            }
            by_display
                .entry(p.display_name)
                .and_modify(|id| *id = None)
                .or_insert(Some(p.id));
        }
        Ok(IdentityResolver {
            by_unique,
            by_display,
        })
    }

    fn resolve(&self, unique: Option<&str>, display: &str) -> Option<String> {
        unique
            .and_then(|u| self.by_unique.get(&u.to_lowercase()).cloned())
            .or_else(|| self.by_display.get(display).cloned().flatten())
    }
}

/// One work item page applied the way a sync pass applies it (one
/// transaction, events and FR-12 change counting included), returning what
/// changed. For the store-write budget bench (spec 0016 AC-A4).
#[doc(hidden)]
pub fn apply_work_item_page(
    store: &Store,
    me: &str,
    items: Vec<WireWorkItem>,
    now: Millis,
) -> adoken_store::Result<activity::Changes> {
    let (result, changes) = activity::collect_changes(|| {
        store.write(|tx| {
            let mut em = Emitter {
                tx,
                me,
                enabled: true,
                now,
                new_ids: Vec::new(),
            };
            apply_work_items(&mut em, items).map(|_| ())
        })
    });
    result.map(|()| changes.finish())
}

/// Returns the max `ChangedDate` applied.
pub(crate) fn apply_work_items(
    em: &mut Emitter<'_>,
    items: Vec<WireWorkItem>,
) -> adoken_store::Result<Option<Millis>> {
    let tx = em.tx;
    let me = em.me.to_owned();
    let projects: HashMap<String, String> = read::projects(tx)?
        .into_iter()
        .map(|p| (p.name, p.id))
        .collect();
    let mut resolver: Option<IdentityResolver> = None;
    let mut max_changed = None;
    for mut w in items {
        if w.is_deleted() {
            let existed = tx
                .prepare_cached("SELECT 1 FROM work_item WHERE id = ?1 AND is_deleted = 0")?
                .exists([w.id])?;
            write::delete_work_item(tx, w.id, w.rev)?;
            if existed {
                activity::note(|c, index| {
                    if index {
                        c.note_index_entry()
                    } else {
                        c.note_work_item(w.id, ChangeWord::Removed)
                    }
                });
            }
            continue;
        }
        let Some(project_id) = w.project_name().and_then(|n| projects.get(n)).cloned() else {
            continue;
        };
        if w.has_identity_strings() {
            if resolver.is_none() {
                resolver = Some(IdentityResolver::load(tx)?);
            }
            if let Some(r) = &resolver {
                w.normalize_identities(|unique, display| r.resolve(unique, display));
            }
        }
        for p in w.people() {
            write::upsert_person(tx, &p, false)?;
        }
        let transitions = w.transitions();
        // Spec 0015 FR-32/34: raw rich text, relations and freshness.
        let extras = crate::wi::Extras::of(&w);
        let Some(mut wi) = w.into_model(project_id) else {
            continue;
        };
        // The type's fetched states know custom states' categories.
        if let Some(category) =
            adoken_store::wi::state_category(tx, &wi.project_id, wi.kind.as_str(), &wi.state)?
        {
            wi.state_category = category;
        }
        max_changed = max_changed.max(Some(wi.changed));
        let old = read::work_item(tx, wi.id)?;
        let upsert = write::upsert_work_item_with_transitions(tx, &wi, transitions)?;
        crate::wi::store_extras(tx, &wi, &extras, em.now)?;
        if !upsert.changed() {
            continue;
        }
        activity::note(|c, index| {
            if index {
                c.note_index_entry();
            } else {
                c.note_work_item(wi.id, wi_change_word(old.as_ref(), &wi, &me));
            }
        });
        // My items' discussions are fetched when their comment count moves
        // or they become mine (`wi.comments`).
        let mine = assigned_now_or_created(&wi, &me);
        let refetch = match &old {
            None => wi.comment_count > 0,
            Some(o) => {
                o.comment_count != wi.comment_count
                    || (!assigned_now_or_created(o, &me) && wi.comment_count > 0)
            }
        };
        if mine && refetch {
            write::set_wi_comments_due(tx, wi.id, true)?;
        }
        let actor = wi.changed_by.clone();
        let who = name(tx, &actor);
        let subject = Subject::WorkItem(wi.id);
        let label = format!("{} {}: {}", wi.kind.as_str(), wi.id, wi.title);
        let assigned_now = wi.assigned_to.as_deref() == Some(me.as_str());
        let assigned_before = old
            .as_ref()
            .is_some_and(|o| o.assigned_to.as_deref() == Some(me.as_str()));
        if assigned_now && !assigned_before {
            em.emit(
                SyncEventKind::WorkItemAssigned,
                subject,
                format!("rev:{}", wi.rev),
                Some(&actor),
                wi.changed,
                format!("{who} assigned {label} to you"),
            )?;
        } else if let Some(o) = old
            && (o.state != wi.state || o.title != wi.title || o.priority != wi.priority)
            // A pinned item's changes feed its change dot (spec 0014 FR-10).
            && (assigned_now
                || wi.created_by == me
                || read::pin(tx, &PinSubject::WorkItem(wi.id))?.is_some())
        {
            em.emit(
                SyncEventKind::WorkItemChanged,
                subject,
                format!("rev:{}", wi.rev),
                Some(&actor),
                wi.changed,
                format!("{who} changed {label} ({} → {})", o.state, wi.state),
            )?;
        }
    }
    Ok(max_changed)
}

fn assigned_now_or_created(wi: &adoken_model::WorkItem, me: &str) -> bool {
    wi.assigned_to.as_deref() == Some(me) || wi.created_by == me
}

/// Upsert one work item's discussion (the full list the API returned) and
/// emit `MentionedInWorkItem` when a comment newly mentions me, by `@<guid>`
/// markup or the API's `mentions[]`.
pub(crate) fn apply_wi_comments(
    em: &mut Emitter<'_>,
    work_item_id: i64,
    comments: Vec<WireWiComment>,
) -> adoken_store::Result<()> {
    let tx = em.tx;
    let me = em.me.to_ascii_lowercase();
    let label = read::work_item(tx, work_item_id)?
        .map(|w| format!("{} {}: {}", w.kind.as_str(), w.id, w.title))
        .unwrap_or_else(|| format!("work item {work_item_id}"));
    let mut keep = Vec::with_capacity(comments.len());
    for w in comments {
        if w.is_deleted {
            continue;
        }
        write::upsert_person(tx, &w.created_by.person(), false)?;
        let cm = w.into_model();
        keep.push(cm.id);
        let before = read::comment_mentions(tx, "wi", work_item_id, 0, cm.id)?;
        let upsert = write::upsert_wi_comment(tx, &cm)?;
        if !upsert.changed() {
            continue;
        }
        activity::note(|c, _| {
            c.note_wi_comment(
                work_item_id,
                if upsert == write::Upsert::Inserted {
                    ChangeWord::NewComment
                } else {
                    ChangeWord::Updated
                },
            )
        });
        let mentions_me = adoken_store::read::mention_ids(&cm.body)
            .into_iter()
            .chain(cm.mentions.iter().map(String::as_str))
            .any(|id| id.eq_ignore_ascii_case(&me));
        let already = before.iter().any(|id| id.eq_ignore_ascii_case(&me));
        if mentions_me && !already && !cm.author.eq_ignore_ascii_case(&me) {
            let who = name(tx, &cm.author);
            em.emit(
                SyncEventKind::MentionedInWorkItem,
                Subject::WorkItem(work_item_id),
                cm.id.to_string(),
                Some(&cm.author),
                cm.updated,
                format!("{who} mentioned you in {label}"),
            )?;
        }
    }
    write::retain_wi_comments(tx, work_item_id, &keep)
}

/// Returns the max `finishTime` applied.
fn apply_builds(
    em: &mut Emitter<'_>,
    builds: impl IntoIterator<Item = WireBuild>,
) -> adoken_store::Result<Option<Millis>> {
    let tx = em.tx;
    let me = em.me.to_owned();
    let pinned: HashMap<(String, i64), Millis> = pinned_definitions(tx)?
        .into_iter()
        .map(|(project, id, at)| ((project, id), at))
        .collect();
    let mut max_finish = None;
    for b in builds {
        if let Some(r) = &b.requested_for {
            write::upsert_person(tx, &r.person(), false)?;
        }
        write::upsert_project_ref(
            tx,
            &adoken_model::Project {
                id: b.project.id.clone(),
                name: b.project.name.clone(),
            },
        )?;
        write::upsert_pipeline(
            tx,
            &Pipeline {
                id: b.definition.id,
                project_id: b.project.id.clone(),
                name: b.definition.name.clone(),
                folder: if b.definition.path.is_empty() {
                    "\\".into()
                } else {
                    b.definition.path.clone()
                },
            },
        )?;
        let run = b.into_model();
        let pr_involves_me = match run.pr_id {
            Some(id) => read::pr(tx, id)?.is_some_and(|p| p.author == me || p.is_reviewer(&me)),
            None => false,
        };
        let mine = run.requested_for.as_deref() == Some(me.as_str()) || pr_involves_me;
        let pinned_at = pinned
            .get(&(run.project_id.clone(), run.pipeline_id))
            .copied();
        if run.pr_id.is_some()
            && !mine
            && pinned_at.is_none()
            && run.requested_for.as_deref() != Some(me.as_str())
        {
            // PR-run query returns everyone's runs; keep only relevant ones.
            continue;
        }
        max_finish = max_finish.max(run.finish);
        let subject = PinSubject::Definition {
            project_id: run.project_id.clone(),
            id: run.pipeline_id,
        };
        write::set_pin_title(tx, &subject, &run.pipeline_name)?;
        write::set_pin_unavailable(tx, &subject, None)?;
        let old = read::run(tx, run.id)?;
        if !write::upsert_run(tx, &run)?.changed() {
            continue;
        }
        activity::note(|c, _| c.note_run(run.id, run_change_word(old.as_ref(), &run)));
        let finished_now = run.status == RunStatus::Completed
            && old
                .as_ref()
                .is_none_or(|o| o.status != RunStatus::Completed);
        // Spec 0014 FR-16: a pinned definition's runs that finish after it
        // was pinned (pinning itself raises nothing).
        let pinned_finish = pinned_at.is_some_and(|at| run.finish.unwrap_or(em.now) >= at);
        if finished_now && (mine || pinned_finish) {
            let result = run.result.map(|r| r.as_str()).unwrap_or("completed");
            let enabled = em.enabled;
            em.enabled |= pinned_finish;
            em.emit(
                SyncEventKind::BuildCompleted,
                Subject::Build(run.id),
                run.id.to_string(),
                run.requested_for.as_deref(),
                run.finish.unwrap_or(em.now),
                format!("{} #{} {result}", run.pipeline_name, run.run_number),
            )?;
            em.enabled = enabled;
        }
    }
    Ok(max_finish)
}
