//! Sync engine against a local mock ADO server serving the demo dataset
//! (spec 0002 §6 harness: recorded-shape mock, injected clock, temp store).

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;

use adoken_demo::{DEFAULT_SEED, Dataset, dataset};
use adoken_model::time::{HOUR, MINUTE, SECOND};
use adoken_model::*;
use adoken_store::read::Cursor;
use adoken_store::{PrFilter, RunFilter, Store, WorkItemFilter, write};
use adoken_sync::*;
use support::Harness;

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

    fn engine(&self) -> Engine {
        let mut cfg = SyncConfig::new(self.h.org_url());
        cfg.max_thread_fetches = 500;
        let clock: Arc<dyn Clock> = self.h.clock.clone();
        Engine::new(self.store.clone(), self.h.transport(), clock, cfg)
    }

    async fn baseline(&self, engine: &Engine) {
        for tier in [Tier::Warm, Tier::Hot, Tier::Cold] {
            engine.run_tier(tier).await.unwrap();
        }
    }

    fn events(&self) -> Vec<SyncEvent> {
        self.store.sync_events_since(0, 100_000).unwrap()
    }

    fn assert_read_only(&self) {
        let s = self.h.state();
        assert!(s.violations.is_empty(), "{:?}", s.violations);
        assert!(
            s.log
                .iter()
                .all(|l| l.method == "GET" || l.path.ends_with("/_apis/wit/wiql"))
        );
    }
}

fn me(ds: &Dataset) -> &str {
    &ds.me
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn baseline_sync_fills_store_and_emits_no_events() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;

    assert!(f.events().is_empty(), "baseline must be silent (FR-13)");
    let active =
        f.ds.prs
            .iter()
            .filter(|p| p.status == PrStatus::Active)
            .count();
    let stored = f
        .store
        .prs(&PrFilter {
            status: Some(PrStatus::Active),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(stored.len(), active);
    assert_eq!(
        f.store
            .work_items(&WorkItemFilter::default())
            .unwrap()
            .len(),
        f.ds.work_items.len()
    );
    assert_eq!(f.store.me().unwrap().unwrap().id, f.ds.me);
    // Hot set threads (with mentions) were hydrated.
    let detail = f.store.pr(adoken_demo::RICH_PR_IDS[0]).unwrap().unwrap();
    assert_eq!(detail.threads.len(), f.ds.pr_extras[&4312].threads.len());
    assert!(!f.store.mentions(&f.ds.me, 100).unwrap().is_empty());
    assert!(!f.store.runs(&RunFilter::default()).unwrap().is_empty());
    assert!(f.store.last_sync().unwrap().is_some());
    assert_eq!(engine.status().borrow().phase, Phase::Idle);
    f.assert_read_only();
}

/// Any process template: a Scrum type and a custom state are kept, and the
/// state's category comes from `workitemtypes/{type}`, not its name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_types_and_states_take_their_category_from_type_metadata() {
    let f = Fixture::new().await;
    let id = {
        let mut s = f.h.state();
        let (id, (wi, _)) = s.work_items.iter_mut().next().unwrap();
        wi.kind = WorkItemType::new("Product Backlog Item");
        wi.state = "Shipped".into();
        let id = *id;
        s.type_states.insert(
            "Product Backlog Item".into(),
            [
                ("New", "Proposed"),
                ("Committed", "InProgress"),
                ("Shipped", "Completed"),
                ("Removed", "Removed"),
            ]
            .map(|(n, c)| (n.to_owned(), c.to_owned()))
            .to_vec(),
        );
        id
    };
    let engine = f.engine();
    f.baseline(&engine).await;

    let wi = f.store.work_item(id).unwrap().unwrap();
    assert_eq!(wi.kind.as_str(), "Product Backlog Item");
    assert_eq!(wi.kind.family(), TypeFamily::Requirement);
    assert_eq!(wi.state, "Shipped");
    // "Shipped" alone would guess InProgress; the type says Completed.
    assert_eq!(wi.state_category, StateCategory::Completed);
    assert!(!wi.is_open());
    let asked = f.h.state().requests("workitemtypes/Product");
    assert_eq!(asked.len(), 1, "states are fetched once per type per day");
    // Fresh metadata isn't fetched again on the next pass.
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(f.h.state().requests("workitemtypes/Product").len(), 1);
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn change_detection_emits_each_event_exactly_once() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let me = me(&f.ds).to_owned();
    let other_pr =
        f.ds.prs
            .iter()
            .find(|p| {
                p.status == PrStatus::Active
                    && p.author != me
                    && !p.is_reviewer(&me)
                    && !adoken_demo::SCENARIO_PRS.contains(&p.id)
            })
            .unwrap()
            .id;
    let unassigned_wi =
        f.ds.work_items
            .iter()
            .find(|w| w.assigned_to.as_deref() != Some(me.as_str()) && w.created_by != me)
            .unwrap()
            .id;
    let my_wi =
        f.ds.work_items
            .iter()
            .find(|w| w.assigned_to.as_deref() == Some(me.as_str()) && w.is_open())
            .unwrap()
            .id;

    f.h.clock.advance(2 * MINUTE);
    {
        let mut s = f.h.state();
        let now = s.now();
        let hannah = s.prs[&4290].reviewers[0].person.clone();
        // Reviewer votes on my PR.
        let pr = s.prs.get_mut(&4290).unwrap();
        pr.reviewers[0].vote = Vote::Approved;
        // I'm added as a reviewer.
        let pr = s.prs.get_mut(&other_pr).unwrap();
        pr.reviewers.push(Reviewer {
            person: me.clone(),
            vote: Vote::NoVote,
            is_required: false,
            has_declined: false,
            added_at: None,
            voted_at: None,
        });
        // Push to a PR I review.
        s.prs.get_mut(&4312).unwrap().last_source_commit = Some("feedface".into());
        // My PR gets conflicts.
        s.prs.get_mut(&4292).unwrap().merge_status = MergeStatus::Conflicts;
        // Someone mentions me on a hot PR.
        s.threads.entry(4294).or_default().push(PrThread {
            id: 900,
            pr_id: 4294,
            status: ThreadStatus::Active,
            anchor: None,
            iteration: None,
            comments: vec![Comment {
                id: 1,
                parent_id: None,
                author: hannah.clone(),
                // Clients write mention ids in any case.
                body: format!(
                    "@<{}> can you re-check the signature header parsing?",
                    me.to_ascii_uppercase()
                ),
                created: now,
                updated: now,
                is_system: false,
                is_deleted: false,
            }],
            updated: now,
        });
        // Work item assigned to me; my own edit on one of my items.
        s.touch_wi(unassigned_wi, |w| {
            w.assigned_to = Some(me.clone());
            w.changed_by = hannah.clone();
        });
        s.touch_wi(my_wi, |w| {
            w.state = "Resolved".into();
            w.changed_by = me.clone();
        });
        // A build I requested fails.
        let mut run = s.runs.values().next_back().unwrap().clone();
        run.id += 1000;
        run.status = RunStatus::Completed;
        run.result = Some(RunResult::Failed);
        run.requested_for = Some(me.clone());
        run.reason = RunReason::Manual;
        run.pr_id = None;
        run.branch = "refs/heads/main".into();
        run.finish = Some(now);
        s.runs.insert(run.id, run);
    }
    engine.run_tier(Tier::Warm).await.unwrap();
    engine.run_tier(Tier::Hot).await.unwrap();

    let events = f.events();
    let kinds: BTreeSet<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    let expected: BTreeSet<&str> = [
        SyncEventKind::PrVoteChanged,
        SyncEventKind::ReviewRequested,
        SyncEventKind::PrPushed,
        SyncEventKind::PrMergeConflict,
        SyncEventKind::MentionedInPr,
        SyncEventKind::WorkItemAssigned,
        SyncEventKind::WorkItemChanged,
        SyncEventKind::BuildCompleted,
    ]
    .iter()
    .map(|k| k.as_str())
    .collect();
    assert_eq!(kinds, expected, "{events:#?}");
    assert_eq!(events.len(), expected.len(), "{events:#?}");
    let find = |k: SyncEventKind| events.iter().find(|e| e.kind == k).unwrap();
    assert_eq!(
        find(SyncEventKind::MentionedInPr).subject,
        Subject::Pr(4294)
    );
    assert!(!find(SyncEventKind::MentionedInPr).self_caused);
    assert!(find(SyncEventKind::WorkItemChanged).self_caused, "FR-12");
    assert_eq!(
        find(SyncEventKind::ReviewRequested).subject,
        Subject::Pr(other_pr)
    );

    // Ten more passes with no server change: nothing new (AC-7).
    for _ in 0..10 {
        f.h.clock.advance(2 * MINUTE);
        engine.run_tier(Tier::Warm).await.unwrap();
        engine.run_tier(Tier::Hot).await.unwrap();
    }
    assert_eq!(f.events().len(), events.len());
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replayed_pages_are_idempotent() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let snapshot = |s: &Store| {
        (
            s.prs(&PrFilter::default()).unwrap(),
            s.work_items(&WorkItemFilter::default()).unwrap(),
            s.runs(&RunFilter::default()).unwrap(),
            s.pr(4312).unwrap(),
            s.search_docs().unwrap().len(),
        )
    };
    let before = snapshot(&f.store);

    // Lose every token/watermark but keep baselines: the whole org replays.
    f.store
        .write(|tx| {
            let rows: Vec<(String, String)> = tx
                .prepare("SELECT scope, project_id FROM sync_cursor")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            for (scope, project_id) in rows {
                write::put_cursor(
                    tx,
                    &Cursor {
                        scope,
                        project_id,
                        baseline_done: true,
                        ..Default::default()
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();
    f.baseline(&engine).await;
    f.baseline(&engine).await;
    assert_eq!(snapshot(&f.store), before);
    assert!(f.events().is_empty(), "{:#?}", f.events());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_pass_resumes_from_persisted_continuation_token() {
    let f = Fixture::new().await;
    f.h.state().revisions_page = 500;
    // First project: page 1 succeeds, page 2 fails (crash/network loss).
    f.h.state().fail_after(1, "workitemrevisions", 500, &[], 1);
    let engine = f.engine();
    engine.run_tier(Tier::Warm).await.unwrap();
    assert!(engine.run_tier(Tier::Cold).await.is_err());

    let project = f.store.projects().unwrap()[0].id.clone();
    let token = f
        .store
        .cursor("wi.revisions", &project)
        .unwrap()
        .and_then(|c| c.token)
        .expect("page 1 token committed with its page");
    drop(engine);

    // "Restart": a fresh engine resumes from the stored token.
    let seen_before = f.h.state().requests("workitemrevisions").len();
    let engine = f.engine();
    engine.run_tier(Tier::Cold).await.unwrap();
    let after = f.h.state().requests("workitemrevisions");
    let first = &after[seen_before];
    assert!(first.path.contains(&project), "{}", first.path);
    assert_eq!(first.param("continuationToken"), Some(token.as_str()));
    assert_eq!(first.param("startDateTime"), None);
    assert_eq!(
        f.store
            .work_items(&WorkItemFilter::default())
            .unwrap()
            .len(),
        f.ds.work_items.len()
    );
    assert!(
        f.store
            .cursor("wi.revisions", &project)
            .unwrap()
            .unwrap()
            .baseline_done
    );
    f.assert_read_only();
}

/// The reporting feed sends identities as `"Name <unique name>"` strings and
/// only the requested fields (as dev.azure.com does); a cold pass alone must
/// still index every item with "assigned to me" pointing at my id, and see
/// upstream deletions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn title_index_reads_real_revision_shapes() {
    let f = Fixture::new().await;
    let engine = f.engine();
    engine.run_tier(Tier::Cold).await.unwrap();

    let first = &f.h.state().requests("workitemrevisions")[0];
    assert_eq!(
        first.param("startDateTime"),
        None,
        "full index by default (Q-2)"
    );
    assert!(first.param("fields").unwrap().contains("System.IsDeleted"));
    let stored = f.store.work_items(&WorkItemFilter::default()).unwrap();
    assert_eq!(stored.len(), f.ds.work_items.len());
    let me = me(&f.ds);
    let mine = |items: &mut dyn Iterator<Item = (i64, Option<&str>)>| -> BTreeSet<i64> {
        items
            .filter(|(_, who)| *who == Some(me))
            .map(|(id, _)| id)
            .collect()
    };
    let expected = mine(
        &mut f
            .ds
            .work_items
            .iter()
            .map(|w| (w.id, w.assigned_to.as_deref())),
    );
    assert!(!expected.is_empty());
    assert_eq!(
        mine(&mut stored.iter().map(|w| (w.id, w.assigned_to.as_deref()))),
        expected
    );

    let victim = *expected.first().unwrap();
    f.h.state().delete_wi(victim);
    engine.run_tier(Tier::Cold).await.unwrap();
    assert_eq!(f.store.work_item(victim).unwrap(), None);
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_selected_projects_are_synced() {
    let f = Fixture::new().await;
    let (keep, skip) = (f.ds.projects[0].clone(), f.ds.projects[1].clone());
    let mut cfg = SyncConfig::new(f.h.org_url());
    cfg.projects = Some(vec![keep.id.clone()]);
    let clock: Arc<dyn Clock> = f.h.clock.clone();
    let engine = Engine::new(f.store.clone(), f.h.transport(), clock, cfg);
    f.baseline(&engine).await;

    assert_eq!(f.store.projects().unwrap(), vec![keep.clone()]);
    let s = f.h.state();
    assert!(
        s.log.iter().all(|l| !l.path.contains(&skip.id)),
        "no request touches an unselected project"
    );
    drop(s);
    assert!(
        f.store
            .work_items(&WorkItemFilter::default())
            .unwrap()
            .iter()
            .all(|w| w.project_id == keep.id)
    );
    assert_eq!(f.store.org().unwrap().unwrap().name, support::ORG);
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_after_blocks_the_host_for_the_whole_window() {
    let f = Fixture::new().await;
    f.h.state().fail(
        "connectionData",
        429,
        &[("Retry-After", "30"), ("X-RateLimit-Resource", "Core")],
        1,
    );
    let engine = f.engine();
    let t0 = f.h.clock.now_ms();
    engine.run_tier(Tier::Warm).await.unwrap();
    let log = f.h.state().log.clone();
    assert!(log[0].path.ends_with("connectionData"));
    assert!(log[1].path.ends_with("connectionData"));
    assert!(log[1].at >= t0 + 30 * SECOND, "retried too early");
    // Nothing else reached the host inside the window.
    assert!(log.iter().all(|l| l.at == t0 || l.at >= t0 + 30 * SECOND));
    assert_eq!(
        engine.governor().snapshot().resource.as_deref(),
        Some("Core")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tf400733_backs_off_30s_then_doubles() {
    let f = Fixture::new().await;
    f.h.state().fail("connectionData", 429, &[], 3);
    let engine = f.engine();
    let t0 = f.h.clock.now_ms();
    engine.run_tier(Tier::Warm).await.unwrap();
    let at: Vec<Millis> =
        f.h.state()
            .requests("connectionData")
            .iter()
            .map(|l| l.at - t0)
            .collect();
    assert_eq!(at, vec![0, 30 * SECOND, 90 * SECOND, 210 * SECOND]);

    // Retries exhausted → pass fails with a throttled status pill.
    f.h.state()
        .fail("pullrequests", 429, &[("X-RateLimit-Resource", "Core")], 10);
    let engine = f.engine();
    assert!(matches!(
        engine.run_tier(Tier::Warm).await,
        Err(SyncError::Gov(GovError::Throttled { .. }))
    ));
    match engine.status().borrow().phase.clone() {
        Phase::Throttled { resource, .. } => assert_eq!(resource.as_deref(), Some("Core")),
        other => panic!("expected throttled, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn low_remaining_budget_runs_hot_tier_only_at_background_cadence() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let reset_s = (f.h.clock.now_ms() + 20 * MINUTE) / 1000;
    f.h.state().headers = vec![
        ("X-RateLimit-Limit".into(), "200".into()),
        ("X-RateLimit-Remaining".into(), "40".into()),
        ("X-RateLimit-Reset".into(), reset_s.to_string()),
    ];
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(engine.governor().pressure(), Pressure::HotOnly);

    let mut sched = Scheduler::new(engine);
    let start = f.h.clock.now_ms();
    let log = sched.run_until(start + 15 * MINUTE).await;
    let ran: Vec<(Millis, Tier, bool)> = log.iter().copied().filter(|(_, _, ok)| *ok).collect();
    assert!(ran.iter().all(|(_, t, _)| *t == Tier::Hot), "{log:?}");
    let hot: Vec<Millis> = ran.iter().map(|(at, ..)| *at).collect();
    for w in hot.windows(2) {
        assert!(w[1] - w[0] >= 5 * MINUTE * 9 / 10, "{hot:?}");
    }
    // No warm/cold request reached the server (hot = threads, builds, and
    // the file lists of hot-set PRs).
    let s = f.h.state();
    assert!(s.log.iter().filter(|l| l.at > start).all(|l| {
        l.path.contains("/threads")
            || l.path.contains("/build/builds")
            || l.path.contains("/iterations")
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthorized_stops_every_tier() {
    let f = Fixture::new().await;
    f.h.state().fail("connectionData", 401, &[], 1);
    let engine = f.engine();
    assert!(matches!(
        engine.run_tier(Tier::Warm).await,
        Err(SyncError::Gov(GovError::Unauthorized))
    ));
    assert_eq!(engine.status().borrow().phase, Phase::Unauthorized);
    let mut sched = Scheduler::new(engine);
    let now = f.h.clock.now_ms();
    let log = sched.run_until(now + 5 * MINUTE).await;
    assert!(log.is_empty());
    assert_eq!(f.h.state().log.len(), 1, "no retry loop after 401");
}

/// Spec 0001 AC-13: after a 401 nothing goes out for 5 minutes; one Replace
/// token and the next pass sends requests again, without a restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replaced_token_resumes_sync_without_a_restart() {
    let f = Fixture::new().await;
    f.h.state().fail("connectionData", 401, &[], 1);
    let engine = f.engine();
    assert!(engine.run_tier(Tier::Hot).await.is_err());
    let mut sched = Scheduler::new(engine);
    let now = f.h.clock.now_ms();
    assert!(sched.run_until(now + 5 * MINUTE).await.is_empty());
    assert_eq!(f.h.state().log.len(), 1);

    sched.handle(Control::TokenReplaced);
    assert_ne!(sched.engine().status().borrow().phase, Phase::Unauthorized);
    let log = sched.run_until(now + 6 * MINUTE).await;
    assert!(
        log.iter().any(|(_, tier, ok)| *tier == Tier::Hot && *ok),
        "{log:?}"
    );
    assert!(f.h.state().log.len() > 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreground_cadence_and_budget_over_30_minutes() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let mut sched = Scheduler::new(engine);
    let start = f.h.clock.now_ms();
    let log = sched.run_until(start + 30 * MINUTE).await;
    let count = |t: Tier| log.iter().filter(|(_, x, ok)| *x == t && *ok).count();
    // 60 s ±10% → 28-34 hot passes (plus the immediate first one).
    assert!((27..=35).contains(&count(Tier::Hot)), "{log:?}");
    assert!((14..=18).contains(&count(Tier::Warm)), "{log:?}");
    assert!((1..=2).contains(&count(Tier::Cold)), "{log:?}");
    // Self-imposed budget: ≤ 300 requests in any 5-minute window.
    let at: Vec<Millis> =
        f.h.state()
            .log
            .iter()
            .filter(|l| l.at >= start)
            .map(|l| l.at)
            .collect();
    for (i, t) in at.iter().enumerate() {
        let n = at[i..].iter().take_while(|x| **x < t + 5 * MINUTE).count();
        assert!(n <= 300, "{n} requests in 5 min");
    }
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_pauses_and_reconnect_runs_hot_first() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let mut sched = Scheduler::new(engine);
    let start = f.h.clock.now_ms();
    sched.handle(Control::Online(false));
    let before = f.h.state().log.len();
    assert!(sched.run_until(start + 10 * MINUTE).await.is_empty());
    assert_eq!(f.h.state().log.len(), before, "no requests while offline");
    sched.handle(Control::Online(true));
    let back = f.h.clock.now_ms();
    let log = sched.run_until(back + 6 * SECOND).await;
    assert_eq!(log.first().map(|(_, t, _)| *t), Some(Tier::Hot));
    assert!(log[0].0 <= back + 5 * SECOND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_loop_syncs_until_stopped() {
    let f = Fixture::new().await;
    let sched = Scheduler::new(f.engine());
    let status = sched.engine().status();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(sched.run(rx));
    tx.send(Control::RefreshNow).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    tx.send(Control::Stop).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .expect("scheduler stops")
        .unwrap();
    assert!(status.borrow().last_ok_warm.is_some());
    assert!(f.store.prs(&PrFilter::default()).unwrap().len() > 50);
    f.assert_read_only();
}

/// Warm + hot rounds, 2 minutes apart, until no finished run waits for its
/// timeline (fetches are bounded per pass and by the 5-minute budget).
async fn drain_timelines(f: &Fixture, engine: &Engine) {
    for _ in 0..200 {
        let due = f
            .store
            .with_conn(|c| adoken_store::read::runs_timeline_due(c, 10_000))
            .unwrap();
        if due.iter().all(|r| r.status != RunStatus::Completed) {
            return;
        }
        f.h.clock.advance(2 * MINUTE);
        engine.run_tier(Tier::Warm).await.unwrap();
        engine.run_tier(Tier::Hot).await.unwrap();
    }
    panic!("timelines never drained");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_stages_come_from_timelines_fetched_once_per_finished_run() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    drain_timelines(&f, &engine).await;

    let runs = f.store.runs(&RunFilter::default()).unwrap();
    assert!(!runs.is_empty());
    for run in &runs {
        assert_eq!(
            f.store.run_stages(run.id).unwrap(),
            f.ds.stages[&run.id],
            "run {} ({:?})",
            run.id,
            run.status
        );
    }
    // Finished runs are never fetched again; running ones every hot pass.
    let live = runs
        .iter()
        .filter(|r| r.status != RunStatus::Completed)
        .count();
    let before = f.h.state().requests("/timeline").len();
    for _ in 0..3 {
        f.h.clock.advance(2 * MINUTE);
        engine.run_tier(Tier::Warm).await.unwrap();
        engine.run_tier(Tier::Hot).await.unwrap();
    }
    let again = f.h.state().requests("/timeline").len() - before;
    assert!(
        again <= 3 * 2 * live.min(8),
        "{again} refetches, {live} live runs"
    );
    if live == 0 {
        assert_eq!(again, 0);
    }
    assert!(f.events().is_empty(), "timelines never emit events");
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeline_fetches_stop_at_the_five_minute_budget() {
    let f = Fixture::new().await;
    // Every response says it cost 1 TSTU: the budget (2 TSTU) is spent
    // before the first timeline can go out.
    f.h.state()
        .headers
        .push(("X-RateLimit-Cost".into(), "1.0".into()));
    let engine = f.engine();
    engine.run_tier(Tier::Warm).await.unwrap();
    assert!(engine.governor().window_cost() >= adoken_sync::engine::TIMELINE_BUDGET);
    assert!(f.h.state().requests("/timeline").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_item_discussions_sync_and_new_mentions_notify_once() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    for _ in 0..10 {
        f.h.clock.advance(2 * MINUTE);
        engine.run_tier(Tier::Warm).await.unwrap();
    }
    let me = f.ds.me.clone();
    // Every comment arrived as text, mention anchors back to `@<id>`.
    for c in &f.ds.wi_comments {
        let body: String = f
            .store
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT body FROM comment WHERE source = 'wi' AND parent_id = ?1 AND ado_comment_id = ?2",
                    (c.work_item_id, c.id),
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let name = |id: &str| f.ds.person(id).unwrap().display_name.clone();
        assert_eq!(body, c.body, "comment {} ({})", c.id, name(&c.author));
    }
    let mentioned =
        f.ds.wi_comments
            .iter()
            .filter(|c| c.body.contains(&format!("@<{me}>")))
            .count();
    assert!(mentioned > 0);
    let wi_mentions = f
        .store
        .mentions(&me, 10_000)
        .unwrap()
        .into_iter()
        .filter(|m| matches!(m.source, MentionSource::WorkItem { .. }))
        .count();
    assert_eq!(wi_mentions, mentioned);
    assert!(f.events().is_empty(), "the first discussion sync is silent");

    // Someone mentions me in a new comment on one of my items.
    let item = f.ds.wi_comments[0].work_item_id;
    let author = f.ds.wi_comments[0].author.clone();
    {
        let mut s = f.h.state();
        let now = s.now();
        s.wi_comments
            .entry(item)
            .or_default()
            .push(WorkItemComment {
                id: 90_001,
                work_item_id: item,
                author,
                body: format!("@<{me}> this needs your call before Monday"),
                mentions: Vec::new(),
                created: now,
                updated: now,
                source: None,
                format: TextFormat::Html,
            });
        s.touch_wi(item, |w| w.comment_count += 1);
    }
    for _ in 0..10 {
        f.h.clock.advance(2 * MINUTE);
        engine.run_tier(Tier::Warm).await.unwrap();
    }
    let events = f.events();
    assert_eq!(events.len(), 1, "{events:#?}");
    assert_eq!(events[0].kind, SyncEventKind::MentionedInWorkItem);
    assert_eq!(events[0].subject, Subject::WorkItem(item));
    assert_eq!(events[0].dedupe_key, "90001");
    assert!(!events[0].self_caused);
    // Discussions of unchanged items aren't refetched.
    let fetched = f.h.state().requests("/comments").len();
    f.h.clock.advance(2 * MINUTE);
    engine.run_tier(Tier::Warm).await.unwrap();
    assert_eq!(f.h.state().requests("/comments").len(), fetched);
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn going_offline_shows_offline_at_once_and_sleep_pauses_until_wake() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let status = engine.status();
    let mut sched = Scheduler::new(engine);

    sched.handle(Control::Online(false));
    assert_eq!(
        status.borrow().phase,
        Phase::Offline,
        "no failed request needed"
    );
    sched.handle(Control::Online(true));
    assert_eq!(status.borrow().phase, Phase::Idle);

    sched.handle(Control::Sleep);
    let before = f.h.state().log.len();
    let start = f.h.clock.now_ms();
    assert!(sched.run_until(start + 30 * MINUTE).await.is_empty());
    assert_eq!(f.h.state().log.len(), before, "no requests while asleep");

    sched.handle(Control::Wake);
    let woke = f.h.clock.now_ms();
    let log = sched.run_until(woke + 15 * SECOND).await;
    assert_eq!(log[0], (woke, Tier::Hot, true), "hot pass right on wake");
    assert!(
        log.iter()
            .any(|(at, t, _)| *t == Tier::Warm && *at >= woke + 10 * SECOND),
        "{log:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn older_runs_load_on_request_with_their_stages() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let me = f.ds.me.clone();
    // A run of mine from 2024, far outside the 7-day window.
    let old = {
        let mut s = f.h.state();
        let mut run = s
            .runs
            .values()
            .find(|r| r.requested_for.as_deref() == Some(me.as_str()) && r.result.is_some())
            .unwrap()
            .clone();
        run.id = 1;
        run.queued = parse_time_ms("2024-04-17T12:08:50Z");
        run.started = Some(run.queued + 15 * SECOND);
        run.finish = Some(run.queued + 25 * SECOND);
        let stages = adoken_demo::demo_stages(&run);
        s.stages.insert(run.id, stages);
        s.runs.insert(run.id, run.clone());
        run
    };
    assert!(f.store.run(old.id).unwrap().is_none());

    // Each request pages further back (newest first) until the 2024 run.
    for _ in 0..20 {
        if f.store.run(old.id).unwrap().is_some() {
            break;
        }
        engine.load_older_runs().await.unwrap();
    }
    let stored = f.store.run(old.id).unwrap().expect("old run loaded");
    assert_eq!(stored.finish, old.finish);
    let pages: Vec<_> =
        f.h.state()
            .log
            .iter()
            .filter(|l| l.param("maxTime").is_some())
            .cloned()
            .collect();
    assert!(
        pages
            .iter()
            .all(|l| l.param("queryOrder") == Some("finishTimeDescending"))
    );
    for project in f.store.projects().unwrap() {
        let bounds: Vec<Millis> = pages
            .iter()
            .filter(|l| l.path.contains(&project.id))
            .map(|l| parse_time_ms(l.param("maxTime").unwrap()))
            .collect();
        assert!(!bounds.is_empty());
        assert!(bounds.windows(2).all(|w| w[1] < w[0]), "{bounds:?}");
    }
    drain_timelines(&f, &engine).await;
    assert_eq!(
        f.store.run_stages(old.id).unwrap(),
        adoken_demo::demo_stages(&old)
    );
    assert!(f.events().is_empty(), "history is never news");

    let mut sched = Scheduler::new(engine);

    // Paged to the first run: another request asks nothing.
    let paged = |f: &Fixture| {
        f.h.state()
            .log
            .iter()
            .filter(|l| l.param("maxTime").is_some())
            .count()
    };
    let before = paged(&f);
    sched.handle(Control::LoadOlderRuns);
    sched.run_until(f.h.clock.now_ms()).await;
    assert_eq!(paged(&f), before);
    f.assert_read_only();
}

fn parse_time_ms(s: &str) -> Millis {
    adoken_sync::wire::parse_time(s).unwrap()
}

// ── Settings › Sync cadence (spec 0002 FR-4a) ────────────────────────────

use adoken_sync::cadence::{Activity, Background, Cadence, Kind, Lane, Pause, Prefs};

/// Which Settings row a logged request belongs to.
fn kind_of(l: &support::Logged) -> Option<&'static str> {
    let p = l.path.as_str();
    if p.contains("/threads") || p.contains("/iterations") {
        Some("my_prs")
    } else if p.ends_with("/_apis/git/pullrequests") {
        Some("all_prs")
    } else if p.contains("/wit/wiql") || p.contains("workitemsbatch") || p.contains("/comments") {
        Some("work_items")
    } else if p.ends_with("/build/builds") && l.param("statusFilter") == Some("completed") {
        Some("done_runs")
    } else if p.ends_with("/build/builds") && l.param("statusFilter").is_some() {
        Some("live_runs")
    } else {
        None
    }
}

/// Distinct request times of `kind` in the log from entry `from` on (one
/// pass = one instant on the manual clock).
fn passes(f: &Fixture, kind: &str, from: usize) -> Vec<Millis> {
    let mut at: Vec<Millis> =
        f.h.state()
            .log
            .iter()
            .skip(from)
            .filter(|l| kind_of(l) == Some(kind))
            .map(|l| l.at)
            .collect();
    at.dedup();
    at
}

fn gaps(at: &[Millis]) -> Vec<Millis> {
    at.windows(2).map(|w| w[1] - w[0]).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_kind_keeps_its_own_interval() {
    let f = Fixture::new().await;
    // Default rotation (30 thread fetches per pass) so the governor's
    // 300-requests budget never paces this run.
    let mut cfg = SyncConfig::new(f.h.org_url());
    cfg.max_thread_fetches = 30;
    let clock: Arc<dyn Clock> = f.h.clock.clone();
    let engine = Engine::new(f.store.clone(), f.h.transport(), clock, cfg);
    f.baseline(&engine).await;
    let prefs = Prefs {
        my_prs: Cadence::Every(2 * MINUTE),
        all_prs: Cadence::Every(5 * MINUTE),
        work_items: Cadence::Manual,
        pipelines: Cadence::Every(30 * SECOND),
        ..Prefs::default()
    };
    let mut sched = Scheduler::new(engine).with_prefs(prefs);
    let start = f.h.clock.now_ms();
    let from = f.h.state().log.len();
    sched.run_until(start + 20 * MINUTE).await;

    // Each lane: its interval ±10% jitter, and it may start up to another
    // tenth early to share its tier's pass (≥ 80%).
    assert!(passes(&f, "work_items", from).is_empty(), "manual only");
    let all = passes(&f, "all_prs", from);
    assert!((4..=5).contains(&all.len()), "{all:?}");
    assert!(
        gaps(&all).iter().all(|g| (240_000..=330_000).contains(g)),
        "{all:?}"
    );
    let threads = passes(&f, "my_prs", from);
    assert!((9..=13).contains(&threads.len()), "{threads:?}");
    assert!(
        gaps(&threads)
            .iter()
            .all(|g| (96_000..=132_000).contains(g)),
        "{threads:?}"
    );
    let live = passes(&f, "live_runs", from);
    assert!((36..=51).contains(&live.len()), "{live:?}");
    assert!(
        gaps(&live).iter().all(|g| (24_000..=33_000).contains(g)),
        "{live:?}"
    );
    // Finished runs: never under 2 min, even with pipelines at 30 s.
    let done = passes(&f, "done_runs", from);
    assert!(gaps(&done).iter().all(|g| *g >= 96_000), "{done:?}");
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fastest_settings_stay_inside_the_budget() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let mut fastest = Prefs::default();
    for kind in Kind::ALL {
        // Below every minimum: clamped to 30 s / 1 min.
        fastest.set_cadence(kind, Cadence::Every(SECOND));
    }
    assert_eq!(fastest.all_prs, Cadence::Every(MINUTE));
    let mut sched = Scheduler::new(engine).with_prefs(fastest);
    let start = f.h.clock.now_ms();
    let from = f.h.state().log.len();
    sched.run_until(start + 15 * MINUTE).await;
    let at: Vec<Millis> = f.h.state().log.iter().skip(from).map(|l| l.at).collect();
    for (i, t) in at.iter().enumerate() {
        let n = at[i..].iter().take_while(|x| **x < t + 5 * MINUTE).count();
        assert!(n <= 300, "{n} requests in 5 min");
    }
    let wi = passes(&f, "work_items", from);
    assert!(gaps(&wi).iter().all(|g| *g >= 54_000), "{wi:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_same_slower_or_paused() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let mut sched = Scheduler::new(engine);
    sched.handle(Control::Foreground(false));

    // Slower (default): FR-4's 5 min hot, 10 min warm.
    let start = f.h.clock.now_ms();
    let log = sched.run_until(start + 30 * MINUTE).await;
    let count = |t: Tier| log.iter().filter(|(_, x, ok)| *x == t && *ok).count();
    assert!((6..=7).contains(&count(Tier::Hot)), "{log:?}");
    assert!((3..=4).contains(&count(Tier::Warm)), "{log:?}");

    // Same: foreground cadence while in the background.
    sched.handle(Control::Prefs(Prefs {
        background: Background::Same,
        ..Prefs::default()
    }));
    let start = f.h.clock.now_ms();
    let log = sched.run_until(start + 10 * MINUTE).await;
    let hot = log.iter().filter(|(_, t, _)| *t == Tier::Hot).count();
    assert!((9..=12).contains(&hot), "{log:?}");

    // Paused: nothing at all until adoken is in the foreground again.
    sched.handle(Control::Prefs(Prefs {
        background: Background::Paused,
        ..Prefs::default()
    }));
    let before = f.h.state().log.len();
    let start = f.h.clock.now_ms();
    assert!(sched.run_until(start + 30 * MINUTE).await.is_empty());
    assert_eq!(f.h.state().log.len(), before);
    sched.handle(Control::Foreground(true));
    let back = f.h.clock.now_ms();
    let log = sched.run_until(back + SECOND).await;
    assert!(
        log.iter().any(|(at, t, _)| *t == Tier::Hot && *at == back),
        "{log:?}"
    );
    assert!(log.iter().any(|(_, t, _)| *t == Tier::Warm), "{log:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_holds_scheduled_passes_but_not_sync_now() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    let t0 = f.h.clock.now_ms();
    let from = f.h.state().log.len();
    let mut sched = Scheduler::new(engine).with_prefs(Prefs {
        pause: Pause::Until(t0 + HOUR),
        ..Prefs::default()
    });
    // For 1 hour: no request, then the overdue lanes run at once.
    let log = sched.run_until(t0 + HOUR + 5 * SECOND).await;
    assert!(log.iter().all(|(at, ..)| *at >= t0 + HOUR), "{log:?}");
    assert!(f.h.state().log.iter().skip(from).all(|l| l.at >= t0 + HOUR));
    assert_eq!(log.first().map(|(at, ..)| *at), Some(t0 + HOUR));

    // Until resumed: nothing, except one Sync now (all hot + warm lanes,
    // manual ones included).
    let paused = Prefs {
        pause: Pause::UntilResumed,
        work_items: Cadence::Manual,
        ..Prefs::default()
    };
    sched.handle(Control::Prefs(paused));
    let start = f.h.clock.now_ms();
    assert!(sched.run_until(start + 30 * MINUTE).await.is_empty());
    sched.handle(Control::RefreshNow);
    let now = f.h.clock.now_ms();
    let from = f.h.state().log.len();
    let log = sched.run_until(now + 30 * MINUTE).await;
    assert_eq!(
        log.iter().map(|(at, t, _)| (*at, *t)).collect::<Vec<_>>(),
        [(now, Tier::Hot), (now, Tier::Warm)]
    );
    assert_eq!(passes(&f, "work_items", from), [now]);

    // Resumed: the schedule picks up at once, work items stay manual.
    sched.handle(Control::Prefs(Prefs {
        pause: Pause::Off,
        ..paused
    }));
    let resumed = f.h.clock.now_ms();
    let from = f.h.state().log.len();
    let log = sched.run_until(resumed + 10 * MINUTE).await;
    assert!(
        log.iter()
            .any(|(at, t, _)| *t == Tier::Hot && *at == resumed)
    );
    assert!(passes(&f, "work_items", from).is_empty());
    assert!(passes(&f, "all_prs", from).len() >= 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn activity_reports_last_sync_and_measured_cost_per_kind() {
    let f = Fixture::new().await;
    let engine = f.engine();
    f.baseline(&engine).await;
    f.h.state().headers = vec![("X-RateLimit-Cost".into(), "0.01".into())];
    let mut sched = Scheduler::new(engine).with_prefs(Prefs {
        pipelines: Cadence::Manual,
        ..Prefs::default()
    });
    let activity = sched.activity();
    let before = activity.borrow().last_ok(Kind::Pipelines);
    let start = f.h.clock.now_ms();
    sched.run_until(start + 3 * MINUTE).await;
    let a: Activity = activity.borrow().clone();
    for kind in [Kind::MyPrs, Kind::AllPrs, Kind::WorkItems] {
        assert!(a.last_ok(kind).is_some_and(|at| at >= start), "{kind:?}");
    }
    // Manual only: no pass since (the baseline's time persists, FR-10).
    assert_eq!(a.last_ok(Kind::Pipelines), before, "manual only");
    // One list call per project per pass at 0.01 TSTU each.
    let projects = f.store.projects().unwrap().len() as f64;
    let all_prs = a.pass_cost[Lane::AllPrs.index()].unwrap();
    assert!((all_prs - 0.01 * projects).abs() < 1e-6, "{all_prs}");
    assert!(a.window_cost > 0.0);
    let estimate = a.estimate_tstu(sched.prefs());
    assert!(
        estimate > 0.0 && estimate < a.window_cost * 10.0,
        "{estimate}"
    );
}
