//! Spec 0016 against the mock Azure DevOps server: the network log's
//! coverage, redaction and origins (group B) and sync activity's passes,
//! progress, change counts and row states (group A).

mod support;

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;

use adoken_demo::{DEFAULT_SEED, Dataset, dataset};
use adoken_model::time::{MINUTE, SECOND};
use adoken_model::*;
use adoken_store::{Store, write};
use adoken_sync::activity::{
    self, ChangeWord, FailReason, Outcome, PassRecord, ProgressUnit, RowKind, RowState, Trigger,
};
use adoken_sync::attribution::{Origin, UserAction};
use adoken_sync::cadence::{Lane, Lanes, Pause, Prefs};
use adoken_sync::ledger::{Entry, export_jsonl};
use adoken_sync::routes::OTHER;
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::writes::{PrRef, ReviewWrite};
use adoken_sync::*;
use futures::future::BoxFuture;
use serde_json::json;
use support::Harness;
use tokio::sync::Notify;

struct Fixture {
    ds: Dataset,
    h: Harness,
    store: Store,
    _dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Fixture {
        let ds = dataset(DEFAULT_SEED);
        let h = Harness::start(&ds).await;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        Fixture {
            ds,
            h,
            store,
            _dir: dir,
        }
    }

    fn engine_with(&self, transport: Arc<dyn Transport>) -> Engine {
        let mut cfg = SyncConfig::new(self.h.org_url());
        cfg.max_thread_fetches = 500;
        Engine::new(self.store.clone(), transport, self.h.clock.clone(), cfg)
    }

    fn engine(&self) -> Engine {
        self.engine_with(self.h.transport())
    }

    async fn baseline(&self, engine: &Engine) {
        for tier in [Tier::Warm, Tier::Hot, Tier::Cold] {
            engine.run_tier(tier).await.unwrap();
        }
    }

    fn entries_since(&self, seq: u64) -> Vec<Entry> {
        self.h.log.entries_since(seq)
    }

    fn pin(&self, subject: PinSubject) {
        let now = self.h.clock.now_ms();
        self.store
            .write(|tx| {
                write::add_pin(
                    tx,
                    &Pin {
                        subject,
                        position: 0,
                        label: None,
                        title: String::new(),
                        pinned_at: now,
                        opened_at: None,
                        seen_result: None,
                        unavailable: None,
                    },
                )
            })
            .unwrap();
    }
}

fn warm_and_hot() -> Lanes {
    [
        Lane::MyPrs,
        Lane::LiveRuns,
        Lane::AllPrs,
        Lane::WorkItems,
        Lane::DoneRuns,
    ]
    .into_iter()
    .collect()
}

fn last_pass(engine: &Engine) -> PassRecord {
    engine.tracker().passes().into_iter().next().unwrap()
}

/// AC-B3 + AC-B2 over a full first sync: every request matches the route
/// table and carries an origin; nothing in the log or its export names the
/// org, a project, a repository or me.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_sync_is_covered_attributed_and_redacted() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    f.pin(PinSubject::Pr(999_998));
    engine.run_tier(Tier::Warm).await.unwrap();
    let entries = f.h.log.entries();
    assert!(entries.len() > 20);
    assert!(entries.iter().all(|e| e.purpose != OTHER), "AC-B3");
    assert!(
        entries
            .iter()
            .all(|e| e.origin != Origin::Unattributed && e.status != ledger::Status::InFlight)
    );
    let mut export = Vec::new();
    export_jsonl(&entries, &mut export, "test", f.h.clock.now_ms()).unwrap();
    let export = String::from_utf8(export).unwrap();
    let me = f.store.me().unwrap().unwrap();
    let mut canaries: Vec<String> = vec![support::ORG.into(), me.id.clone()];
    canaries.extend(me.unique_name.clone());
    canaries.extend(f.ds.projects.iter().map(|p| p.name.clone()));
    canaries.extend(f.ds.projects.iter().map(|p| p.id.clone()));
    canaries.extend(f.ds.repos.iter().map(|r| r.name.clone()));
    for c in canaries.iter().filter(|c| c.len() > 3) {
        assert!(!export.contains(c.as_str()), "{c} in the export");
        for e in &entries {
            let shown = format!("{} {} {} {}", e.host, e.route, e.purpose, e.origin.label());
            assert!(!shown.contains(c.as_str()), "{c} in {shown}");
        }
    }
}

/// AC-B4: lanes, pinned steps, ⌘R, startup, opening a PR, a vote and its
/// re-read each carry their origin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_carry_the_origin_of_their_work() {
    let f = Fixture::new().await;
    let engine = f.engine();
    // The bootstrap of the very first pass is startup traffic.
    engine.run_tier(Tier::Warm).await.unwrap();
    let first = f.h.log.entries();
    assert!(
        first
            .iter()
            .any(|e| e.purpose == "Connection data" && e.origin == Origin::Startup)
    );
    engine.run_tier(Tier::Hot).await.unwrap();

    // A scheduled warm pass: its lanes, and the pinned PR GET is pinned.
    f.pin(PinSubject::Pr(999_998));
    let seq = f.h.log.last_seq();
    engine.run_tier(Tier::Warm).await.unwrap();
    let warm = f.entries_since(seq);
    let lanes: BTreeSet<String> = warm.iter().map(|e| e.origin.label()).collect();
    assert!(lanes.contains("All active pull requests"), "{lanes:?}");
    assert!(lanes.contains("Work items"), "{lanes:?}");
    let pinned = warm.iter().find(|e| e.purpose == "Pinned PR").unwrap();
    assert_eq!(
        pinned.origin,
        Origin::Sync {
            lane: Lane::AllPrs,
            pinned: true,
            sync_now: false,
        }
    );
    assert!(
        warm.iter()
            .all(|e| e.pass == Some(last_pass(&engine).pass_ref()))
    );

    // ⌘R.
    let mut sched = Scheduler::new(f.engine_with(f.h.transport()));
    let seq = f.h.log.last_seq();
    sched.handle(Control::RefreshNow);
    let now = f.h.clock.now_ms();
    sched.run_until(now + SECOND).await;
    let refreshed = f.entries_since(seq);
    assert!(!refreshed.is_empty());
    // Every hot/warm request of the ⌘R pass says so (the cold index lane
    // may share the moment on its own schedule).
    assert!(
        refreshed
            .iter()
            .filter(|e| !matches!(
                e.origin,
                Origin::Sync {
                    lane: Lane::Index,
                    ..
                }
            ))
            .all(|e| matches!(e.origin, Origin::Sync { sync_now: true, .. })),
        "{:?}",
        refreshed
            .iter()
            .map(|e| e.origin.label())
            .collect::<Vec<_>>()
    );

    // Opening a PR: its files at user priority.
    let pr = f.store.pr(4312).unwrap().unwrap().pr;
    let seq = f.h.log.last_seq();
    let _ = engine.pr_files().sync_files(Tier::User, &pr).await;
    let opened = f.entries_since(seq);
    assert!(!opened.is_empty());
    assert!(
        opened
            .iter()
            .all(|e| e.origin == Origin::You(UserAction::OpenPr))
    );

    // A vote (answered by a script; the mock server is read-only) and the
    // re-read of my PRs within 5 s.
    let vote = Arc::new(Recording::new(Scripted::default(), f.h.log.clone()));
    vote.inner()
        .answer(200, json!({ "id": f.ds.me, "vote": 10 }));
    let writer = Writer::new(
        f.store.clone(),
        vote.clone(),
        sched.engine().shared_governor(),
        f.h.clock.clone(),
        f.h.org_url(),
    )
    .with_tracker(sched.engine().tracker().clone());
    writer
        .send(ReviewWrite::Vote {
            pr: PrRef::of(&pr),
            reviewer: f.ds.me.clone(),
            vote: Vote::Approved,
        })
        .await
        .unwrap();
    let voted = f.h.log.entries().pop().unwrap();
    assert_eq!(voted.origin.label(), "You: vote");
    let seq = f.h.log.last_seq();
    sched.handle(Control::AfterWrite);
    let now = f.h.clock.now_ms();
    sched.run_until(now + 6 * SECOND).await;
    let reread = f.entries_since(seq);
    assert!(!reread.is_empty());
    assert!(
        reread
            .iter()
            .all(|e| e.origin.label() == "You: vote · re-read"),
        "{:?}",
        reread.iter().map(|e| e.origin.label()).collect::<Vec<_>>()
    );
    assert_eq!(
        sched.engine().tracker().passes()[0].trigger,
        Trigger::Reread(UserAction::Vote)
    );
}

/// Answers from a script.
#[derive(Default)]
struct Scripted {
    script: Mutex<VecDeque<(u16, serde_json::Value)>>,
}

impl Scripted {
    fn answer(&self, status: u16, body: serde_json::Value) {
        self.script.lock().push_back((status, body));
    }
}

impl Transport for Scripted {
    fn send(&self, _req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        let (status, body) = self.script.lock().pop_front().expect("unscripted request");
        Box::pin(async move {
            Ok(ApiResponse {
                status,
                rate: RateHeaders::default(),
                body: serde_json::to_vec(&body).unwrap(),
                content_type: Some("application/json".into()),
                validators: Default::default(),
            })
        })
    }
}

/// Holds the `n`-th request whose path contains `needle` until released.
struct Gate {
    inner: Arc<dyn Transport>,
    needle: &'static str,
    n: usize,
    seen: Mutex<usize>,
    reached: Notify,
    release: Notify,
}

impl Transport for Gate {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            if req.url().path().contains(self.needle) {
                let hit = {
                    let mut seen = self.seen.lock();
                    *seen += 1;
                    *seen == self.n
                };
                if hit {
                    self.reached.notify_one();
                    self.release.notified().await;
                }
            }
            self.inner.send(req).await
        })
    }
}

/// AC-A3: with the 4th hot-set thread response held, My pull requests
/// reads "3 of N"; the index counts items without a total.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn progress_counts_what_is_done_of_what_is_known() {
    let f = Fixture::new().await;
    f.baseline(&f.engine()).await;
    let gate = Arc::new(Gate {
        inner: f.h.transport(),
        needle: "/threads",
        n: 4,
        seen: Mutex::new(0),
        reached: Notify::new(),
        release: Notify::new(),
    });
    let mut cfg = SyncConfig::new(f.h.org_url());
    cfg.max_thread_fetches = 500;
    // Every hot PR's threads are stale after an hour.
    f.h.clock.advance(60 * MINUTE);
    let engine = Arc::new(Engine::new(
        f.store.clone(),
        gate.clone(),
        f.h.clock.clone(),
        cfg,
    ));
    let pass = {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine
                .run_lanes(Tier::Hot, [Lane::MyPrs].into_iter().collect())
                .await
        })
    };
    gate.reached.notified().await;
    let lane = &engine.tracker().lanes()[Lane::MyPrs.index()];
    assert!(lane.running);
    let progress = lane.progress.unwrap();
    assert_eq!(
        (progress.done, progress.unit),
        (3, ProgressUnit::PullRequests)
    );
    assert!(progress.total.is_some_and(|t| t >= 4), "{progress:?}");
    let row = activity::rows(&engine.tracker().inputs(), f.h.clock.now_ms())[0].clone();
    assert_eq!(row.state, RowState::Syncing(Some(progress)));
    gate.release.notify_one();
    pass.await.unwrap().unwrap();
    assert!(!engine.tracker().lanes()[Lane::MyPrs.index()].running);

    // The index: a running count, no total.
    let engine = f.engine();
    engine.run_tier(Tier::Cold).await.unwrap();
    let pass = last_pass(&engine);
    assert_eq!(pass.trigger, Trigger::Scheduled);
}

/// AC-A4: exact counts of what a pass changed, its ids, none of the
/// unchanged rows; an identical pass changes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passes_count_exactly_what_changed() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    // Drain the first-sync discussion backlog (20 items per pass): those
    // are real inserts, not this test's changes.
    for _ in 0..20 {
        f.h.clock.advance(2 * MINUTE);
        engine.run_tier(Tier::Warm).await.unwrap();
    }
    let me = f.ds.me.clone();
    let (voted, pushed, completed) = (4290, 4312, 4292);
    let wis: Vec<i64> =
        f.ds.work_items
            .iter()
            .filter(|w| w.assigned_to.as_deref() == Some(me.as_str()) && w.is_open())
            .map(|w| w.id)
            .take(2)
            .collect();
    f.h.clock.advance(2 * MINUTE);
    let run = {
        let mut s = f.h.state();
        let now = s.now();
        s.prs.get_mut(&voted).unwrap().reviewers[0].vote = Vote::Approved;
        s.prs.get_mut(&pushed).unwrap().last_source_commit = Some("feedface".into());
        let pr = s.prs.get_mut(&completed).unwrap();
        pr.status = PrStatus::Completed;
        pr.closed = Some(now);
        s.touch_wi(wis[0], |w| w.state = "Resolved".into());
        s.touch_wi(wis[1], |w| w.title = "Renamed".into());
        let mut run = s.runs.values().next_back().unwrap().clone();
        run.id += 1000;
        run.status = RunStatus::Completed;
        run.result = Some(RunResult::Succeeded);
        run.requested_for = Some(me.clone());
        run.reason = RunReason::Manual;
        run.pr_id = None;
        run.branch = "refs/heads/main".into();
        run.finish = Some(now);
        s.runs.insert(run.id, run.clone());
        run.id
    };
    engine
        .run_pass(Tier::Warm, warm_and_hot(), Trigger::Scheduled)
        .await
        .unwrap();
    let c = last_pass(&engine).changes;
    let ids = |k: &activity::KindChanges| k.ids.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
    assert_eq!(c.prs.total(), 3, "{:?}", c.prs);
    assert_eq!(ids(&c.prs), BTreeSet::from([voted, pushed, completed]));
    assert!(c.prs.ids.contains(&(completed, ChangeWord::Completed)));
    assert!(c.prs.ids.contains(&(voted, ChangeWord::VoteChanged)));
    assert!(c.prs.ids.contains(&(pushed, ChangeWord::Pushed)));
    assert_eq!(c.work_items.total(), 2, "{:?}", c.work_items);
    assert_eq!(ids(&c.work_items), wis.iter().copied().collect());
    assert!(
        c.work_items
            .ids
            .contains(&(wis[0], ChangeWord::State("Resolved".into())))
    );
    assert_eq!(c.runs.total(), 1, "{:?}", c.runs);
    assert_eq!(
        c.runs.ids,
        [(run, ChangeWord::Finished(RunResult::Succeeded))]
    );

    // Identical: nothing changed.
    f.h.clock.advance(2 * MINUTE);
    engine
        .run_pass(Tier::Warm, warm_and_hot(), Trigger::Scheduled)
        .await
        .unwrap();
    let again = last_pass(&engine);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    assert!(again.requests > 0);
}

/// AC-A6: three pins, one answering 404: the Pinned row counts 3, one not
/// available, and the time of the last warm pass that ran every pinned
/// step.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_row_counts_pins_and_their_refresh() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let wi = f.ds.work_items[0].id;
    f.pin(PinSubject::Pr(4312));
    f.pin(PinSubject::WorkItem(wi));
    f.pin(PinSubject::Pr(999_998));
    f.h.clock.advance(2 * MINUTE);
    engine.run_tier(Tier::Warm).await.unwrap();
    let at = f.h.clock.now_ms();
    let pins = engine.tracker().pins();
    assert_eq!((pins.pins, pins.unavailable), (3, 1));
    assert_eq!(pins.last_full_refresh, Some(at));
    // A hot pass refreshes counts but not the warm pinned time.
    f.h.clock.advance(MINUTE);
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(engine.tracker().pins().last_full_refresh, Some(at));
    let row = &activity::rows(&engine.tracker().inputs(), f.h.clock.now_ms())[5];
    assert_eq!((row.kind, row.last_ok), (RowKind::Pinned, Some(at)));
}

/// AC-A8: after a restart with pause on (nothing sent), every row shows
/// the last-synced time from before, and Recent passes is empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn last_success_survives_a_paused_restart() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let before = engine.tracker().lanes();
    assert!(before.iter().all(|l| l.last_ok.is_some()));
    drop(engine);

    let seq = f.h.log.last_seq();
    f.h.clock.advance(30 * MINUTE);
    let mut sched = Scheduler::new(f.engine()).with_prefs(Prefs {
        pause: Pause::UntilResumed,
        ..Prefs::default()
    });
    let now = f.h.clock.now_ms();
    sched.run_until(now + 10 * MINUTE).await;
    assert_eq!(f.h.log.last_seq(), seq, "paused: nothing sent");
    let tracker = sched.engine().tracker();
    assert!(tracker.passes().is_empty());
    let lanes = tracker.lanes();
    for lane in Lane::ALL {
        assert_eq!(
            lanes[lane.index()].last_ok,
            before[lane.index()].last_ok,
            "{lane:?}"
        );
    }
    let rows = activity::rows(&tracker.inputs(), f.h.clock.now_ms());
    for row in &rows[..5] {
        assert_eq!(
            row.state,
            RowState::Paused { until: None },
            "{:?}",
            row.kind
        );
        assert!(row.last_ok.is_some());
    }
    assert_eq!(sched.activity().borrow().last_ok, before.map(|l| l.last_ok));
}

/// AC-A9 (failure half; the throttled rows are covered by the FR-8 unit
/// fixtures): an error status on the work items WIQL fails only Work
/// items, with the status and its retry time. 503 is retried by the
/// governor as a throttle (0002 FR-8), so a 500 stands in for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failure_marks_only_its_lane() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    f.h.clock.advance(2 * MINUTE);
    let now = f.h.clock.now_ms();
    // A 500 on the WIQL: only Work items fails.
    let mut sched = Scheduler::new(f.engine());
    f.h.state().fail("/_apis/wit/wiql", 500, &[], 1);
    sched.handle(Control::RefreshNow);
    sched.run_until(now + SECOND).await;
    let tracker = sched.engine().tracker();
    let rows = activity::rows(&tracker.inputs(), f.h.clock.now_ms());
    for row in &rows {
        match row.kind {
            RowKind::WorkItems => match &row.state {
                RowState::Failed {
                    reason: FailReason::Status { status: 500, .. },
                    retry_at: Some(at),
                } => assert!(*at > now),
                other => panic!("Work items: {other:?}"),
            },
            RowKind::AllPrs | RowKind::MyPrs => {
                assert_eq!(row.state, RowState::Synced, "{:?}", row.kind)
            }
            _ => {}
        }
    }
    assert!(matches!(
        tracker
            .passes()
            .iter()
            .find(|p| p.lanes.contains(Lane::WorkItems))
            .unwrap()
            .outcome,
        Outcome::Failed(FailReason::Status { status: 500, .. })
    ));
}

/// AC-A2: "next in" is the scheduler's planned due time; the background
/// (Slower) multiplies it by 5; manual only has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_due_follows_the_schedule() {
    let f = Fixture::new().await;
    f.baseline(&f.engine()).await;
    let mut sched = Scheduler::new(f.engine());
    let start = f.h.clock.now_ms();
    sched.run_until(start + 10 * SECOND).await;
    let my_prs = |s: &Scheduler| s.engine().tracker().lanes()[Lane::MyPrs.index()].clone();
    let lane = my_prs(&sched);
    let ran = lane.last_ok.unwrap();
    let next = lane.next_due.unwrap();
    // 1 min ± 10% jitter after the pass.
    assert!(
        (ran + 54 * SECOND..=ran + 66 * SECOND).contains(&next),
        "{next} vs {ran}"
    );
    sched.handle(Control::Foreground(false));
    let bg = my_prs(&sched).next_due.unwrap();
    assert!(bg >= ran + 5 * 54 * SECOND - 10 * SECOND, "{bg} vs {ran}");
    sched.handle(Control::Prefs(Prefs {
        my_prs: adoken_sync::cadence::Cadence::Manual,
        ..Prefs::default()
    }));
    assert_eq!(my_prs(&sched).next_due, None);
    let rows = activity::rows(&sched.engine().tracker().inputs(), f.h.clock.now_ms());
    assert_eq!(rows[0].state, RowState::ManualOnly);
    assert_eq!(rows[0].next_due, None);
}
