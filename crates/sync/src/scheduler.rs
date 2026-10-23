//! Tier scheduling (spec 0002 FR-4, FR-4a, FR-5, FR-8, §3 states).
//!
//! Each [`Lane`] (a slice of a tier, see [`crate::cadence`]) has its own due
//! time from Settings › Sync. A pass runs the lane that is due plus the
//! other lanes of its tier due within a tenth of their interval, so lanes
//! with the same cadence stay one pass (the default is exactly FR-4's hot /
//! warm / cold passes). The governor still decides what may go out:
//! pressure slows or defers lanes, `Retry-After` and backoff hold them, and
//! the budget caps every pass.

use adoken_model::Millis;
use adoken_model::time::{MINUTE, SECOND};
use tokio::sync::{mpsc, watch};

use crate::Tier;
use crate::activity::Trigger;
use crate::cadence::{self, Activity, Lane, Lanes, Prefs, SLOWER_FACTOR};
use crate::engine::{Engine, PassReport, SyncError};
use crate::governor::{GovError, Pressure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Cmd+R: hot + warm now (coalesced within 10 s), also for lanes set to
    /// manual only and while sync is paused.
    RefreshNow,
    /// App window key / visible in the last 5 min.
    Foreground(bool),
    /// Reachability; no requests while offline (FR-5). Going offline shows
    /// `Phase::Offline` at once.
    Online(bool),
    /// The Mac is going to sleep: no requests until [`Control::Wake`].
    Sleep,
    /// The Mac woke up: hot pass now, warm after 10 s (FR-5).
    Wake,
    /// Palette "Load older pipeline runs": one [`Engine::load_older_runs`].
    LoadOlderRuns,
    /// A review write landed (spec 0002 FR-15, server wins): a pass of my
    /// PRs within 5 s re-reads the PR (paused or not).
    AfterWrite,
    /// Azure DevOps accepted my Complete of this PR and merges it
    /// asynchronously (spec 0004 FR-10): re-read the PR after 2 s, then 4 s
    /// and 8 s while it's still merging. Bounded; the warm tier's
    /// left-the-active-set fetch covers anything slower.
    Completing(i64),
    /// Settings › Sync changed (FR-4a): applies at once.
    Prefs(Prefs),
    /// A replacement token verified (spec 0001 FR-15, AC-13): requests go
    /// out again and a hot pass runs now, without a restart.
    TokenReplaced,
    Stop,
}

const REFRESH_COALESCE_MS: Millis = 10 * SECOND;
const ERROR_BACKOFF_START: Millis = 5 * SECOND;
const ERROR_BACKOFF_CAP: Millis = 5 * MINUTE;
/// FR-8 "< 20% remaining: hot tier only, at background cadence": never
/// faster than FR-4's background hot interval, whatever Settings say.
const HOT_ONLY_MIN: Millis = 5 * MINUTE;
/// FR-15: the affected PR is re-fetched within this after a write.
const AFTER_WRITE_MS: Millis = 5 * SECOND;
/// First re-read of a completing PR; each later one waits twice as long.
const RECHECK_FIRST_MS: Millis = 2 * SECOND;
/// Re-reads per accepted Complete.
const RECHECK_ATTEMPTS: u32 = 3;
/// ±10% jitter on every interval (FR-4).
const JITTER_PCT: i64 = 10;
const NEVER: Millis = Millis::MAX;

/// A pending re-read of a completing PR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Recheck {
    pr_id: i64,
    at: Millis,
    /// 0-based attempt this re-read is.
    attempt: u32,
}

/// Delay before re-read `attempt` (0-based): 2 s, 4 s, 8 s; `None` once the
/// attempts are spent.
fn recheck_delay(attempt: u32) -> Option<Millis> {
    (attempt < RECHECK_ATTEMPTS).then(|| RECHECK_FIRST_MS << attempt)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    /// A pass led by this lane.
    Pass(Lane),
    Recheck,
}

pub struct Scheduler {
    engine: Engine,
    prefs: Prefs,
    foreground: bool,
    online: bool,
    asleep: bool,
    stopped: bool,
    /// Next scheduled pass per lane ([`NEVER`]: manual only).
    next: [Millis; 6],
    /// Passes that go out whatever the cadence or pause says (Sync now, a
    /// write's re-read), per lane.
    forced: [Option<Millis>; 6],
    last_start: [Option<Millis>; 6],
    /// Per tier (hot, warm, cold).
    error_backoff: [Millis; 3],
    last_refresh: Millis,
    /// A "load older runs" request waiting for the network.
    older_runs: bool,
    /// Completing PRs to re-read (spec 0004 FR-10).
    rechecks: Vec<Recheck>,
    /// Why a forced pass of a lane was asked for (spec 0016 FR-11).
    forced_by: [Option<Trigger>; 6],
    /// Lanes whose next pass is a wake / reconnect catch-up.
    catching_up: [bool; 6],
    rng: u64,
    activity: watch::Sender<Activity>,
}

fn tier_ix(t: Tier) -> usize {
    match t {
        Tier::Hot | Tier::User => 0,
        Tier::Warm => 1,
        Tier::Cold => 2,
    }
}

impl Scheduler {
    /// First sync order: warm (PR lists, my items) → hot (threads) → cold.
    pub fn new(engine: Engine) -> Self {
        let now = engine.clock().now_ms();
        let next = Lane::ALL.map(|lane| match lane.tier() {
            Tier::Warm => now,
            Tier::Hot | Tier::User => now + 1,
            Tier::Cold => now + 2,
        });
        // FR-10: last successes survive restarts (paused or offline at
        // launch, every row still shows its real time).
        engine.restore_activity();
        let restored = engine.tracker().lanes();
        let (activity, _) = watch::channel(Activity {
            foreground: true,
            last_ok: std::array::from_fn(|i| restored[i].last_ok),
            ..Activity::default()
        });
        let mut s = Scheduler {
            engine,
            prefs: Prefs::default(),
            foreground: true,
            online: true,
            asleep: false,
            stopped: false,
            next,
            forced: [None; 6],
            last_start: [None; 6],
            error_backoff: [0; 3],
            last_refresh: Millis::MIN / 2,
            older_runs: false,
            rechecks: Vec::new(),
            forced_by: [None; 6],
            catching_up: [false; 6],
            rng: now as u64 | 1,
            activity,
        };
        s.apply_prefs(Prefs::default(), now);
        s
    }

    /// Start with Settings › Sync as read at launch.
    pub fn with_prefs(mut self, prefs: Prefs) -> Self {
        let now = self.engine.clock().now_ms();
        self.apply_prefs(prefs, now);
        self
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    /// Last passes and measured costs, for Settings › Sync.
    pub fn activity(&self) -> watch::Receiver<Activity> {
        self.activity.subscribe()
    }

    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn jitter(&mut self, ms: Millis, pct: i64) -> Millis {
        let r = self.random();
        jitter_with(ms, pct, r)
    }

    /// Current interval of `lane` given the settings, foreground state and
    /// rate pressure; `None` = no scheduled passes (manual only, or paused
    /// in the background).
    pub fn interval(&self, lane: Lane) -> Option<Millis> {
        let base = self.prefs.interval(lane, self.foreground)?;
        Some(match (self.engine.governor().pressure(), lane.tier()) {
            (Pressure::HotOnly, Tier::Hot | Tier::User) => {
                let fg = self.prefs.interval(lane, true).unwrap_or(base);
                (fg * SLOWER_FACTOR).max(HOT_ONLY_MIN).max(base)
            }
            (Pressure::Slow, Tier::Warm | Tier::Cold) => base * 4,
            _ => base,
        })
    }

    /// The interval the next pass is planned with: the current one, else
    /// the foreground one (background paused); `None` only for manual.
    fn plan_interval(&self, lane: Lane) -> Option<Millis> {
        self.interval(lane)
            .or_else(|| self.prefs.interval(lane, true))
    }

    /// Requests may go out: running, online, awake, token accepted.
    fn can_send(&self) -> bool {
        !self.stopped && self.online && !self.asleep && !self.engine.governor().is_unauthorized()
    }

    /// When `lane` runs next on its own schedule: after its interval and
    /// after any pause; never while manual only or paused in the background.
    fn scheduled(&self, lane: Lane) -> Option<Millis> {
        let next = self.next[lane.index()];
        if next == NEVER {
            return None;
        }
        self.interval(lane)?;
        match self.prefs.pause {
            cadence::Pause::Off => Some(next),
            cadence::Pause::Until(at) => Some(next.max(at)),
            cadence::Pause::UntilResumed => None,
        }
    }

    fn lane_due(&self, lane: Lane) -> Option<Millis> {
        self.forced[lane.index()]
            .into_iter()
            .chain(self.scheduled(lane))
            .min()
    }

    /// The next pass or completing re-read, whichever is first.
    fn next_job(&self) -> Option<(Job, Millis)> {
        if !self.can_send() {
            return None;
        }
        let pass = Lane::ALL
            .into_iter()
            .filter_map(|lane| self.lane_due(lane).map(|at| (Job::Pass(lane), at)))
            .min_by_key(|(_, at)| *at);
        let recheck = self
            .rechecks
            .iter()
            .map(|r| r.at)
            .min()
            .map(|at| (Job::Recheck, at));
        pass.into_iter().chain(recheck).min_by_key(|(_, at)| *at)
    }

    pub fn handle(&mut self, c: Control) {
        self.handle_inner(c);
        self.publish_schedule();
    }

    fn handle_inner(&mut self, c: Control) {
        let now = self.engine.clock().now_ms();
        match c {
            Control::RefreshNow => {
                if now - self.last_refresh >= REFRESH_COALESCE_MS {
                    self.last_refresh = now;
                    for lane in Lane::ALL {
                        if matches!(lane.tier(), Tier::Hot | Tier::Warm) {
                            self.forced[lane.index()] = Some(now);
                            self.forced_by[lane.index()] = Some(Trigger::SyncNow);
                        }
                    }
                }
            }
            Control::Foreground(fg) => {
                let before = self.plans();
                self.foreground = fg;
                self.replan(before, now);
                self.publish();
            }
            Control::Online(on) => {
                if on && !self.online {
                    // FR-5: hot within 0-5 s, warm after 10 s, cold on its
                    // next slot (never before the catch-up passes).
                    let hot = self.jitter(2500, 100);
                    self.catch_up(now, hot);
                }
                self.online = on;
                self.engine.set_online(on);
            }
            Control::Sleep => self.asleep = true,
            Control::Wake => {
                self.asleep = false;
                self.catch_up(now, 0);
            }
            Control::LoadOlderRuns => self.older_runs = true,
            Control::AfterWrite => {
                let slot = &mut self.forced[Lane::MyPrs.index()];
                *slot = Some(slot.unwrap_or(NEVER).min(now + AFTER_WRITE_MS));
                // FR-18: the re-read is labeled with the write it follows.
                if let Some(action) = self.engine.tracker().take_last_write() {
                    self.forced_by[Lane::MyPrs.index()] = Some(Trigger::Reread(action));
                }
            }
            Control::Completing(pr_id) => {
                self.rechecks.retain(|r| r.pr_id != pr_id);
                self.rechecks.push(Recheck {
                    pr_id,
                    at: now + RECHECK_FIRST_MS,
                    attempt: 0,
                });
            }
            Control::Prefs(prefs) => self.apply_prefs(prefs, now),
            Control::TokenReplaced => {
                self.engine.token_replaced();
                self.catch_up(now, 0);
            }
            Control::Stop => self.stopped = true,
        }
    }

    /// New settings apply at once (see [`Self::replan`]).
    fn apply_prefs(&mut self, prefs: Prefs, now: Millis) {
        let before = self.plans();
        self.prefs = prefs.clamped();
        self.replan(before, now);
        self.engine
            .follow_hot_interval(self.prefs.interval(Lane::MyPrs, true));
        self.publish();
        self.publish_schedule();
    }

    fn plans(&self) -> [Option<Millis>; 6] {
        Lane::ALL.map(|lane| self.plan_interval(lane))
    }

    /// Lanes whose planned interval changed (settings, foreground) are
    /// replanned from their last pass, so a shorter interval can make one
    /// due at once; manual lanes stop; lanes back from manual run now when
    /// they never ran. Pause applies on the next check.
    fn replan(&mut self, before: [Option<Millis>; 6], now: Millis) {
        for lane in Lane::ALL {
            let i = lane.index();
            let after = self.plan_interval(lane);
            if after == before[i] && (after.is_none() || self.next[i] != NEVER) {
                continue;
            }
            self.next[i] = match (after, self.last_start[i]) {
                (None, _) => NEVER,
                (Some(iv), Some(start)) => start + iv,
                (Some(_), None) if self.next[i] == NEVER => now,
                (Some(_), None) => self.next[i],
            };
        }
    }

    /// Reconnect / wake: hot after `hot_in`, warm after 10 s, cold not
    /// before 20 s; failures from the outage don't keep backing off. Manual
    /// lanes stay manual.
    fn catch_up(&mut self, now: Millis, hot_in: Millis) {
        for lane in Lane::ALL {
            let next = &mut self.next[lane.index()];
            if *next == NEVER {
                continue;
            }
            *next = match lane.tier() {
                Tier::Hot | Tier::User => now + hot_in,
                Tier::Warm => (*next).max(now + 10 * SECOND),
                Tier::Cold => (*next).max(now + 20 * SECOND),
            };
            self.catching_up[lane.index()] = true;
        }
        self.error_backoff = [0; 3];
    }

    fn publish(&self) {
        let window_cost = self.engine.governor().window_cost();
        let foreground = self.foreground;
        self.activity.send_if_modified(|a| {
            let changed = a.window_cost != window_cost || a.foreground != foreground;
            a.window_cost = window_cost;
            a.foreground = foreground;
            changed
        });
    }

    /// The planned due time per lane (jitter, background, pause, pressure
    /// and forced passes applied) for the Activity rows (spec 0016 FR-8).
    fn publish_schedule(&self) {
        self.engine.tracker().set_schedule(
            Lane::ALL.map(|lane| self.lane_due(lane)),
            self.prefs,
            self.foreground,
            self.online && !self.asleep,
        );
    }

    /// Why the pass led by `lead` runs (spec 0016 FR-11).
    fn trigger(&mut self, lanes: Lanes) -> Trigger {
        let forced = lanes.iter().find_map(|l| self.forced_by[l.index()]);
        let catching_up = lanes.iter().any(|l| self.catching_up[l.index()]);
        let tracked = self.engine.tracker().lanes();
        let first = lanes.iter().all(|l| tracked[l.index()].last_ok.is_none());
        for lane in lanes.iter() {
            self.forced_by[lane.index()] = None;
            self.catching_up[lane.index()] = false;
        }
        match forced {
            Some(trigger) => trigger,
            None if catching_up => Trigger::CatchUp,
            None if first => Trigger::FirstSync,
            None => Trigger::Scheduled,
        }
    }

    /// Run a pending user request (load older runs) when requests may go
    /// out.
    async fn run_requests(&mut self) {
        if self.older_runs && self.can_send() {
            self.older_runs = false;
            if let Err(e) = self.engine.load_older_runs().await {
                log::debug!(target: "adoken_sync", "older runs failed: {e}");
            }
        }
    }

    /// `lead` and the other lanes of its tier due within their jitter span
    /// (a tenth of their interval), so equal cadences stay one pass.
    fn pass_lanes(&self, lead: Lane, now: Millis) -> Lanes {
        Lane::ALL
            .into_iter()
            .filter(|lane| lane.tier() == lead.tier())
            .filter(|lane| {
                let slack = self.interval(*lane).map_or(0, |iv| iv * JITTER_PCT / 100);
                *lane == lead || self.lane_due(*lane).is_some_and(|at| at <= now + slack)
            })
            .collect()
    }

    async fn run_pass(&mut self, lead: Lane) -> (Tier, Result<PassReport, SyncError>) {
        let tier = lead.tier();
        let start = self.engine.clock().now_ms();
        let lanes = self.pass_lanes(lead, start);
        let trigger = self.trigger(lanes);
        let result = if self.engine.governor().allows(tier) {
            self.engine.run_pass(tier, lanes, trigger).await
        } else {
            Err(SyncError::Gov(GovError::Deferred))
        };
        let now = self.engine.clock().now_ms();
        // One draw per pass: lanes with the same interval stay together.
        let r = self.random();
        let t = tier_ix(tier);
        let backoff = match &result {
            Ok(_) | Err(SyncError::Gov(GovError::Deferred)) => {
                self.error_backoff[t] = 0;
                None
            }
            Err(SyncError::Gov(GovError::Throttled { until })) => Some((*until).max(now + SECOND)),
            Err(_) => {
                let b = &mut self.error_backoff[t];
                *b = if *b == 0 {
                    ERROR_BACKOFF_START
                } else {
                    (*b * 2).min(ERROR_BACKOFF_CAP)
                };
                Some(now + jitter_with(*b, JITTER_PCT, r))
            }
        };
        for lane in lanes.iter() {
            let i = lane.index();
            self.forced[i] = None;
            self.last_start[i] = Some(start);
            let planned = self.plan_interval(lane);
            self.next[i] = match (planned, backoff) {
                (None, _) => NEVER,
                (Some(_), Some(at)) => at,
                (Some(iv), None) => now + jitter_with(iv, JITTER_PCT, r),
            };
        }
        // FR-8: a failed lane says when the backoff retries.
        let tracked = self.engine.tracker().lanes();
        for lane in lanes.iter() {
            if tracked[lane.index()].failure.is_some() && result.is_err() {
                self.engine
                    .tracker()
                    .set_retry(lane, self.next[lane.index()]);
            }
        }
        // Lanes that completed (all of them when the pass did) count as
        // synced, whatever happened after them (FR-10).
        let done: Vec<(Lane, Millis)> = lanes
            .iter()
            .filter_map(|lane| {
                tracked[lane.index()]
                    .last_ok
                    .filter(|at| *at >= start)
                    .map(|at| (lane, at))
            })
            .collect();
        let costs = result
            .as_ref()
            .map(|r| r.lane_costs.clone())
            .unwrap_or_default();
        if !done.is_empty() || !costs.is_empty() {
            self.activity.send_modify(|a| {
                for (lane, at) in &done {
                    a.last_ok[lane.index()] = Some(*at);
                }
                for (lane, cost) in &costs {
                    let slot = &mut a.pass_cost[lane.index()];
                    *slot = Some(cadence::smooth(*slot, *cost));
                }
            });
        }
        self.publish();
        self.publish_schedule();
        (tier, result)
    }

    /// Re-read the earliest pending completing PR; schedule the next
    /// attempt while its merge is still queued.
    async fn run_recheck(&mut self) {
        let Some(ix) = (0..self.rechecks.len()).min_by_key(|i| self.rechecks[*i].at) else {
            return;
        };
        let r = self.rechecks.swap_remove(ix);
        let still = if self.engine.governor().allows(Tier::Hot) {
            match self.engine.recheck_completing(r.pr_id).await {
                Ok(still) => still,
                Err(e) => {
                    log::debug!(target: "adoken_sync", "recheck !{} failed: {e}", r.pr_id);
                    true
                }
            }
        } else {
            true
        };
        let attempt = r.attempt + 1;
        if still && let Some(delay) = recheck_delay(attempt) {
            let now = self.engine.clock().now_ms();
            self.rechecks.push(Recheck {
                pr_id: r.pr_id,
                at: now + delay,
                attempt,
            });
        }
    }

    async fn run_job(&mut self, job: Job) -> Option<(Tier, bool)> {
        match job {
            Job::Pass(lane) => {
                let (tier, result) = self.run_pass(lane).await;
                if let Err(e) = &result {
                    log::debug!(target: "adoken_sync", "{tier:?} pass failed: {e}");
                }
                Some((tier, result.is_ok()))
            }
            Job::Recheck => {
                self.run_recheck().await;
                None
            }
        }
    }

    /// Drive the schedule until `until` (clock time). Returns `(start, tier)`
    /// of every pass attempted (completing re-reads run too, unlogged).
    /// With a `ManualClock` this simulates hours instantly.
    pub async fn run_until(&mut self, until: Millis) -> Vec<(Millis, Tier, bool)> {
        let mut log = Vec::new();
        self.run_requests().await;
        while let Some((job, at)) = self.next_job() {
            if at > until {
                break;
            }
            let now = self.engine.clock().now_ms();
            if at > now {
                self.engine.clock().sleep(at - now).await;
            }
            let start = self.engine.clock().now_ms();
            if let Some((tier, ok)) = self.run_job(job).await {
                log.push((start, tier, ok));
            }
        }
        let now = self.engine.clock().now_ms();
        if until > now {
            self.engine.clock().sleep(until - now).await;
        }
        log
    }

    /// Run forever on the current Tokio runtime until [`Control::Stop`] or
    /// the control channel closes.
    pub async fn run(mut self, mut control: mpsc::UnboundedReceiver<Control>) {
        loop {
            self.run_requests().await;
            let wait = match self.next_job() {
                Some((_, at)) => Some((at - self.engine.clock().now_ms()).max(0)),
                None if self.stopped => return,
                None => None,
            };
            let sleep = async {
                match wait {
                    Some(ms) => self.engine.clock().sleep(ms).await,
                    None => futures::future::pending::<()>().await,
                }
            };
            tokio::select! {
                msg = control.recv() => match msg {
                    Some(c) => self.handle(c),
                    None => return,
                },
                () = sleep => {
                    if let Some((job, _)) = self.next_job() {
                        self.run_job(job).await;
                    }
                }
            }
        }
    }
}

/// `ms` ± `pct`% from the random draw `r`.
fn jitter_with(ms: Millis, pct: i64, r: u64) -> Millis {
    let span = ms * pct / 100;
    if span == 0 {
        return ms;
    }
    ms - span + (r % (2 * span as u64 + 1)) as Millis
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use adoken_model::{Person, PrStatus, Project, PullRequest, Repo};
    use adoken_store::{Store, write};
    use futures::future::BoxFuture;
    use serde_json::json;

    use super::*;
    use crate::clock::{Clock, ManualClock};
    use crate::engine::SyncConfig;
    use crate::transport::{ApiRequest, ApiResponse, RateHeaders, Transport, TransportError};

    const T0: Millis = 1_790_769_600_000;

    /// Azure DevOps for one completing PR: every read of !912 answers
    /// `active` + `queued` until read number `merged_at` answers `completed`.
    struct Merging {
        merged_at: usize,
        reads: AtomicUsize,
        clock: Arc<ManualClock>,
        read_at: parking_lot::Mutex<Vec<Millis>>,
    }

    impl Transport for Merging {
        fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
            assert_eq!(req.url().path(), "/contoso/p1/_apis/git/pullrequests/912");
            self.read_at.lock().push(self.clock.now_ms());
            let n = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
            let done = n >= self.merged_at;
            let body = json!({
                "pullRequestId": 912,
                "repository": { "id": "r1", "name": "web", "project": { "id": "p1", "name": "Payments" } },
                "title": "Fix checkout",
                "status": if done { "completed" } else { "active" },
                "createdBy": { "id": "bo", "displayName": "Bo" },
                "creationDate": "2026-09-30T10:00:00Z",
                "closedDate": if done { json!("2026-09-30T12:00:03Z") } else { json!(null) },
                "closedBy": { "id": "me", "displayName": "Me" },
                "sourceRefName": "refs/heads/fix",
                "targetRefName": "refs/heads/main",
                "mergeStatus": if done { "succeeded" } else { "queued" },
                "lastMergeSourceCommit": { "commitId": "aaaa" },
            });
            Box::pin(async move {
                Ok(ApiResponse {
                    status: 200,
                    rate: RateHeaders::default(),
                    body: serde_json::to_vec(&body).unwrap(),
                    content_type: Some("application/json".into()),
                    validators: Default::default(),
                })
            })
        }
    }

    fn scheduler(merged_at: usize) -> (Scheduler, Arc<Merging>, Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        let person = |id: &str| Person {
            id: id.into(),
            display_name: id.into(),
            unique_name: None,
            is_group: false,
            image_url: None,
        };
        store
            .write(|tx| {
                write::upsert_person(tx, &person("me"), true)?;
                write::upsert_person(tx, &person("bo"), false)?;
                write::upsert_project(
                    tx,
                    &Project {
                        id: "p1".into(),
                        name: "Payments".into(),
                    },
                )?;
                write::upsert_repo(
                    tx,
                    &Repo {
                        id: "r1".into(),
                        project_id: "p1".into(),
                        name: "web".into(),
                        default_branch: None,
                    },
                )?;
                write::upsert_pr(
                    tx,
                    &PullRequest {
                        id: 912,
                        repo_id: "r1".into(),
                        project_id: "p1".into(),
                        title: "Fix checkout".into(),
                        description: String::new(),
                        author: "bo".into(),
                        reviewers: Vec::new(),
                        status: PrStatus::Active,
                        is_draft: false,
                        merge_status: adoken_model::MergeStatus::Queued,
                        source_branch: "refs/heads/fix".into(),
                        target_branch: "refs/heads/main".into(),
                        last_source_commit: Some("aaaa".into()),
                        created: T0 - 60_000,
                        updated: T0,
                        closed: None,
                        iterations: 1,
                        last_push: None,
                    },
                )?;
                write::set_completing(tx, 912, "aaaa", T0)
            })
            .unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let ado = Arc::new(Merging {
            merged_at,
            reads: AtomicUsize::new(0),
            clock: clock.clone(),
            read_at: Default::default(),
        });
        let engine = Engine::new(
            store.clone(),
            ado.clone(),
            clock,
            SyncConfig::new(url::Url::parse("https://dev.azure.com/contoso/").unwrap()),
        );
        let mut s = Scheduler::new(engine);
        // Only the re-reads under test run.
        s.next = [NEVER; 6];
        (s, ado, store, dir)
    }

    #[tokio::test]
    async fn completing_pr_is_reread_until_azure_devops_finishes_the_merge() {
        let (mut s, ado, store, _dir) = scheduler(2);
        s.handle(Control::Completing(912));
        s.run_until(T0 + MINUTE).await;
        // 2 s after the Complete, then 4 s later it had merged: no more.
        assert_eq!(*ado.read_at.lock(), [T0 + 2 * SECOND, T0 + 6 * SECOND]);
        let pr = store.pr(912).unwrap().unwrap().pr;
        assert_eq!(pr.status, PrStatus::Completed);
        assert_eq!(store.completing(912).unwrap(), None);
    }

    #[tokio::test]
    async fn completing_rereads_are_bounded() {
        let (mut s, ado, store, _dir) = scheduler(usize::MAX);
        s.handle(Control::Completing(912));
        s.run_until(T0 + 10 * MINUTE).await;
        assert_eq!(
            *ado.read_at.lock(),
            [T0 + 2 * SECOND, T0 + 6 * SECOND, T0 + 14 * SECOND]
        );
        assert!(store.completing(912).unwrap().is_some());
    }

    #[tokio::test]
    async fn offline_holds_rereads_until_back_online() {
        let (mut s, ado, _store, _dir) = scheduler(1);
        s.handle(Control::Completing(912));
        s.handle(Control::Online(false));
        s.run_until(T0 + MINUTE).await;
        assert!(ado.read_at.lock().is_empty());
        s.handle(Control::Online(true));
        // Coming back online schedules catch-up tier passes; keep them out.
        s.next = [NEVER; 6];
        s.run_until(T0 + 2 * MINUTE).await;
        assert_eq!(ado.read_at.lock().len(), 1);
    }
}
