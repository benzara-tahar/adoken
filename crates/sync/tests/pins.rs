//! Spec 0014 against the mock ADO server: pinned items stay fresh (FR-11,
//! AC-9), unreadable ones show "Not available" (FR-13, AC-13), pinned
//! definitions raise their failures (FR-16, AC-14), labels stay local
//! (AC-15). Requests are counted from the mock's log.

mod support;

use std::sync::Arc;

use adoken_demo::{DEFAULT_SEED, Dataset, dataset};
use adoken_model::time::MINUTE;
use adoken_model::*;
use adoken_store::{Store, write};
use adoken_sync::*;
use support::{Harness, Logged};

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

    fn engine_for(&self, projects: Option<Vec<String>>) -> Engine {
        let mut cfg = SyncConfig::new(self.h.org_url());
        cfg.max_thread_fetches = 500;
        cfg.projects = projects;
        let clock: Arc<dyn Clock> = self.h.clock.clone();
        Engine::new(self.store.clone(), self.h.transport(), clock, cfg)
    }

    async fn baseline(&self, engine: &Engine) {
        for tier in [Tier::Warm, Tier::Hot, Tier::Cold] {
            engine.run_tier(tier).await.unwrap();
        }
    }

    /// One more warm pass, two minutes later; returns its requests.
    async fn warm_pass(&self, engine: &Engine) -> Vec<Logged> {
        self.h.clock.advance(2 * MINUTE);
        let before = self.h.state().log.len();
        engine.run_tier(Tier::Warm).await.unwrap();
        self.h.state().log[before..].to_vec()
    }

    fn pin(&self, subject: PinSubject) {
        let now = self.h.clock.now_ms();
        self.store
            .write(|tx| {
                write::add_pin(
                    tx,
                    &Pin {
                        subject: subject.clone(),
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

    fn unpin(&self, subject: &PinSubject) {
        self.store.write(|tx| write::unpin(tx, subject)).unwrap();
    }

    fn pin_of(&self, subject: &PinSubject) -> Pin {
        self.store
            .pins()
            .unwrap()
            .into_iter()
            .find(|p| p.subject == *subject)
            .expect("still pinned")
    }

    fn assert_read_only(&self) {
        let s = self.h.state();
        assert!(s.violations.is_empty(), "{:?}", s.violations);
        assert!(s.log.iter().all(|l| l.method == "GET"
            || l.path.ends_with("/_apis/wit/wiql")
            || l.path.ends_with("/_apis/wit/workitemsbatch")));
    }
}

fn batches(log: &[Logged]) -> Vec<Vec<i64>> {
    log.iter()
        .filter(|l| l.path.ends_with("/_apis/wit/workitemsbatch"))
        .inspect(|l| {
            let body = l.body.as_ref().unwrap();
            assert_eq!(l.param("api-version"), Some("7.1"));
            assert_eq!(body["$expand"], "relations");
            assert_eq!(body["errorPolicy"], "omit");
            assert!(
                body.get("fields").is_none(),
                "ADO rejects fields with $expand"
            );
        })
        .map(|l| {
            l.body.as_ref().unwrap()["ids"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(serde_json::Value::as_i64)
                .collect()
        })
        .collect()
}

fn definition_requests(log: &[Logged]) -> Vec<String> {
    log.iter()
        .filter(|l| l.path.ends_with("/_apis/build/builds") && l.param("definitions").is_some())
        .inspect(|l| assert_eq!(l.param("maxBuildsPerDefinition"), Some("1")))
        .map(|l| l.param("definitions").unwrap().to_owned())
        .collect()
}

/// AC-9: a pinned work item that isn't mine comes in one `workitemsbatch`
/// per warm pass, its state change lands within one pass, and unpinning
/// takes it out of the next pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_work_item_not_mine_is_batched_each_warm_pass() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let me = f.ds.me.clone();
    let other =
        f.ds.work_items
            .iter()
            .find(|w| {
                w.assigned_to.as_deref() != Some(me.as_str()) && w.created_by != me && w.is_open()
            })
            .unwrap()
            .id;
    let subject = PinSubject::WorkItem(other);
    f.pin(subject.clone());

    let pass = f.warm_pass(&engine).await;
    assert_eq!(batches(&pass), vec![vec![other]]);
    assert!(
        pass.iter().all(|l| !l.path.contains("/_apis/projects/")),
        "known project resolves locally"
    );

    f.h.state().touch_wi(other, |w| w.state = "Resolved".into());
    let pass = f.warm_pass(&engine).await;
    assert_eq!(batches(&pass), vec![vec![other]]);
    assert_eq!(f.store.work_item(other).unwrap().unwrap().state, "Resolved");
    assert!(!f.pin_of(&subject).title.is_empty(), "title kept for FR-13");

    f.unpin(&subject);
    let pass = f.warm_pass(&engine).await;
    assert!(batches(&pass).is_empty());
    f.assert_read_only();
}

/// AC-9: a pinned PR in a project that isn't synced gets one GET by id per
/// warm pass, without syncing that project.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_pr_in_an_unselected_project_is_fetched_by_id() {
    let f = Fixture::new().await;
    let (keep, skip) = (f.ds.projects[0].clone(), f.ds.projects[1].clone());
    let pr =
        f.ds.prs
            .iter()
            .find(|p| p.project_id == skip.id && p.status == PrStatus::Active)
            .unwrap()
            .clone();
    let engine = f.engine_for(Some(vec![keep.id.clone()]));
    f.baseline(&engine).await;
    assert!(f.store.pr(pr.id).unwrap().is_none());
    f.pin(PinSubject::Pr(pr.id));

    for _ in 0..2 {
        let pass = f.warm_pass(&engine).await;
        let by_id: Vec<&Logged> = pass
            .iter()
            .filter(|l| l.path == format!("/{}/_apis/git/pullrequests/{}", support::ORG, pr.id))
            .collect();
        assert_eq!(by_id.len(), 1, "one GET by id per warm pass");
        assert!(pass.iter().all(|l| !l.path.contains(&skip.id)));
    }
    assert_eq!(f.store.pr(pr.id).unwrap().unwrap().pr.title, pr.title);
    assert_eq!(f.store.synced_projects().unwrap(), vec![keep]);
    assert_eq!(f.pin_of(&PinSubject::Pr(pr.id)).title, pr.title);

    // Pinned PRs rank first in the hot set: its threads are fetched.
    f.h.clock.advance(MINUTE);
    let before = f.h.state().log.len();
    engine.run_tier(Tier::Hot).await.unwrap();
    let threads = format!("/pullRequests/{}/threads", pr.id);
    assert!(
        f.h.state().log[before..]
            .iter()
            .any(|l| l.path.ends_with(&threads))
    );
    f.assert_read_only();
}

/// AC-9: three pinned definitions in one project cost one builds request
/// per pass; unpinning removes the id from the next pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_definitions_cost_one_request_per_project() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let project = f.ds.pipelines[0].project_id.clone();
    let defs: Vec<i64> =
        f.ds.pipelines
            .iter()
            .filter(|p| p.project_id == project)
            .take(3)
            .map(|p| p.id)
            .collect();
    assert_eq!(defs.len(), 3, "demo project has three pipelines");
    let subject = |id| PinSubject::Definition {
        project_id: project.clone(),
        id,
    };
    for id in &defs {
        f.pin(subject(*id));
    }
    let ids = |ids: &[i64]| ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let pass = f.warm_pass(&engine).await;
    assert_eq!(definition_requests(&pass), vec![ids(&defs)]);

    f.unpin(&subject(defs[1]));
    let pass = f.warm_pass(&engine).await;
    assert_eq!(definition_requests(&pass), vec![ids(&[defs[0], defs[2]])]);

    f.unpin(&subject(defs[0]));
    f.unpin(&subject(defs[2]));
    let pass = f.warm_pass(&engine).await;
    assert!(definition_requests(&pass).is_empty());
    f.assert_read_only();
}

/// AC-13: a deleted pinned work item (omitted by the batch) and a pinned PR
/// answering 404 show "Not available" with their last title and stay
/// pinned; 403 says the token can't read it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreadable_pins_are_not_available_and_stay() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let me = f.ds.me.clone();
    let wi =
        f.ds.work_items
            .iter()
            .find(|w| w.assigned_to.as_deref() != Some(me.as_str()) && w.created_by != me)
            .unwrap()
            .clone();
    let wi_pin = PinSubject::WorkItem(wi.id);
    let gone_pr = PinSubject::Pr(999_999);
    let hidden_pr = PinSubject::Pr(999_998);
    f.pin(wi_pin.clone());
    f.pin(gone_pr.clone());
    f.pin(hidden_pr.clone());
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&wi_pin).unavailable, None);
    assert_eq!(f.pin_of(&wi_pin).title, wi.title);
    assert_eq!(f.pin_of(&gone_pr).unavailable, Some(PinUnavailable::Gone));
    f.h.state().fail("/_apis/wit/workitemsbatch", 403, &[], 1);
    f.warm_pass(&engine).await;
    assert_eq!(
        f.pin_of(&wi_pin).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    assert_eq!(f.pin_of(&wi_pin).title, wi.title);
    assert_eq!(f.store.work_item(wi.id).unwrap().unwrap().title, wi.title);
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&wi_pin).unavailable, None, "readability recovered");

    f.h.state().delete_wi(wi.id);
    f.h.state()
        .fail("/_apis/git/pullrequests/999998", 403, &[], 1);
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&wi_pin).unavailable, Some(PinUnavailable::Gone));
    assert_eq!(f.pin_of(&wi_pin).title, wi.title, "last title kept");
    assert_eq!(
        f.pin_of(&hidden_pr).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    assert_eq!(f.store.pins().unwrap().len(), 3, "never removed");
    f.assert_read_only();
}

/// AC-14: a failed run (someone else's) of a pinned definition emits one
/// `BuildCompleted`; after unpinning the next pass asks for no runs of it
/// and a later failure emits nothing. Pinning a PR emits nothing by itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_definition_failures_are_events_until_unpinned() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let me = f.ds.me.clone();
    let def =
        f.ds.pipelines
            .iter()
            .find(|p| p.name.contains("Nightly"))
            .unwrap_or(&f.ds.pipelines[0])
            .clone();
    let other = f.ds.people.iter().find(|p| p.id != me).unwrap().id.clone();
    let subject = PinSubject::Definition {
        project_id: def.project_id.clone(),
        id: def.id,
    };
    let pr =
        f.ds.prs
            .iter()
            .find(|p| p.status == PrStatus::Active && p.author != me && !p.is_reviewer(&me))
            .unwrap()
            .id;
    let events_before = f.store.sync_events_since(0, 100_000).unwrap().len();
    f.pin(subject.clone());
    f.pin(PinSubject::Pr(pr));
    f.warm_pass(&engine).await;
    assert_eq!(
        f.store.sync_events_since(0, 100_000).unwrap().len(),
        events_before,
        "pinning raises nothing by itself"
    );

    let fail = |id: i64, at: Millis| PipelineRun {
        id,
        project_id: def.project_id.clone(),
        pipeline_id: def.id,
        pipeline_name: def.name.clone(),
        run_number: format!("x.{id}"),
        status: RunStatus::Completed,
        result: Some(RunResult::Failed),
        reason: RunReason::Schedule,
        branch: "refs/heads/main".into(),
        source_version: None,
        requested_for: Some(other.clone()),
        pr_id: None,
        queued: at - 10 * MINUTE,
        started: Some(at - 9 * MINUTE),
        finish: Some(at),
    };
    let now = f.h.clock.now_ms();
    f.h.state()
        .runs
        .insert(990_001, fail(990_001, now + MINUTE));
    f.warm_pass(&engine).await;
    let builds: Vec<SyncEvent> = f
        .store
        .sync_events_since(0, 100_000)
        .unwrap()
        .into_iter()
        .filter(|e| e.subject == Subject::Build(990_001))
        .collect();
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].kind, SyncEventKind::BuildCompleted);
    assert!(!builds[0].self_caused);

    f.unpin(&subject);
    let now = f.h.clock.now_ms();
    f.h.state()
        .runs
        .insert(990_002, fail(990_002, now + MINUTE));
    let pass = f.warm_pass(&engine).await;
    assert!(definition_requests(&pass).is_empty());
    assert!(
        f.store
            .sync_events_since(0, 100_000)
            .unwrap()
            .iter()
            .all(|e| e.subject != Subject::Build(990_002))
    );
    f.assert_read_only();
}

/// AC-15: renaming is a local write: the mock sees no request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renaming_a_pin_sends_nothing() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let subject = PinSubject::Definition {
        project_id: f.ds.pipelines[0].project_id.clone(),
        id: f.ds.pipelines[0].id,
    };
    f.pin(subject.clone());
    let before = f.h.state().log.len();
    f.store
        .write(|tx| write::rename_pin(tx, &subject, "Payments CI"))
        .unwrap();
    assert_eq!(f.h.state().log.len(), before);
    assert_eq!(f.pin_of(&subject).label.as_deref(), Some("Payments CI"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_work_items_resolve_unknown_project_once_and_keep_refreshing() {
    let f = Fixture::new().await;
    let selected = f.ds.projects[0].clone();
    let engine = f.engine_for(Some(vec![selected.id.clone()]));
    f.baseline(&engine).await;
    // A project created after structure discovery, not in the selected set.
    let project = Project {
        id: "new-project".into(),
        name: "New project".into(),
    };
    let other =
        f.ds.people
            .iter()
            .find(|p| p.id != f.ds.me)
            .unwrap()
            .id
            .clone();
    let mut wi = f.ds.work_items[0].clone();
    wi.id = 990_100;
    wi.project_id = project.id.clone();
    wi.assigned_to = Some(other.clone());
    wi.created_by = other.clone();
    wi.changed_by = other;
    let mut mine = wi.clone();
    mine.id += 1;
    mine.assigned_to = Some(f.ds.me.clone());
    {
        let mut state = f.h.state();
        state.projects.push(project.clone());
        state.work_items.insert(wi.id, (wi.clone(), 0));
        state.work_items.insert(mine.id, (mine.clone(), 0));
    }
    f.pin(PinSubject::WorkItem(wi.id));
    f.pin(PinSubject::WorkItem(mine.id));
    let pass = f.warm_pass(&engine).await;
    assert_eq!(batches(&pass), vec![vec![wi.id, mine.id]]);
    let lookups: Vec<_> = pass
        .iter()
        .filter(|l| l.path.contains("/_apis/projects/"))
        .collect();
    assert_eq!(lookups.len(), 1, "one lookup for a shared unknown name");
    assert!(lookups[0].path.ends_with("/New%20project"));
    assert_eq!(lookups[0].param("api-version"), Some("7.1"));
    for item in [&wi, &mine] {
        let stored = f.store.work_item(item.id).unwrap().unwrap();
        assert_eq!(stored.project_id, project.id);
        assert_eq!(stored.title, item.title, "stored on the first warm pass");
        assert_eq!(f.pin_of(&PinSubject::WorkItem(item.id)).title, item.title);
    }
    assert!(f.store.projects().unwrap().contains(&project));
    assert_eq!(f.store.synced_projects().unwrap(), vec![selected]);

    f.h.state()
        .touch_wi(wi.id, |w| w.title = "Updated outside selection".into());
    f.h.state()
        .touch_wi(mine.id, |w| w.title = "My item outside selection".into());
    let pass = f.warm_pass(&engine).await;
    assert_eq!(batches(&pass), vec![vec![wi.id, mine.id]]);
    assert!(pass.iter().all(|l| !l.path.contains("/_apis/projects/")));
    assert_eq!(
        f.store.work_item(wi.id).unwrap().unwrap().title,
        "Updated outside selection"
    );
    assert_eq!(
        f.store.work_item(mine.id).unwrap().unwrap().title,
        "My item outside selection"
    );
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_other_subject_activity_lights_dots_but_never_notifies() {
    use adoken_notify::{DeliveryEnv, Effects};
    let f = Fixture::new().await;
    let me = f.ds.me.clone();
    let pr =
        f.ds.prs
            .iter()
            .find(|p| {
                p.status == PrStatus::Active
                    && p.author != me
                    && !p.is_reviewer(&me)
                    && !adoken_demo::SCENARIO_PRS.contains(&p.id)
            })
            .unwrap()
            .clone();
    let wi =
        f.ds.work_items
            .iter()
            .find(|w| w.assigned_to.as_deref() != Some(me.as_str()) && w.created_by != me)
            .unwrap()
            .clone();
    // This PR has no participation relationship, including in old discussions.
    f.h.state().threads.remove(&pr.id);
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    f.pin(PinSubject::Pr(pr.id));
    f.pin(PinSubject::WorkItem(wi.id));
    f.warm_pass(&engine).await;
    engine.run_tier(Tier::Hot).await.unwrap();
    let opened = f.h.clock.now_ms();
    f.store
        .write(|tx| {
            write::mark_pin_opened(tx, &PinSubject::Pr(pr.id), opened, None, false)?;
            write::mark_pin_opened(tx, &PinSubject::WorkItem(wi.id), opened, None, false)
        })
        .unwrap();
    f.h.clock.advance(MINUTE);
    {
        let mut state = f.h.state();
        let now = state.now();
        state.push_pr(pr.id, "other-push", &pr.author);
        state.threads.entry(pr.id).or_default().push(PrThread {
            id: 990_200,
            pr_id: pr.id,
            status: ThreadStatus::Active,
            anchor: None,
            iteration: None,
            updated: now,
            comments: vec![Comment {
                id: 1,
                parent_id: None,
                author: pr.author.clone(),
                body: "An ordinary comment without a mention".into(),
                created: now,
                updated: now,
                is_system: false,
                is_deleted: false,
            }],
        });
        state.touch_wi(wi.id, |w| {
            w.title = "Changed by a teammate".into();
            w.changed_by = pr.author.clone();
        });
    }
    f.warm_pass(&engine).await;
    engine.run_tier(Tier::Hot).await.unwrap();
    let events: Vec<_> = f
        .store
        .sync_events_since(0, 100_000)
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.at > opened
                && (e.subject == Subject::Pr(pr.id) || e.subject == Subject::WorkItem(wi.id))
        })
        .collect();
    assert_eq!(events.len(), 3, "{events:#?}");
    for kind in [
        SyncEventKind::PrPushed,
        SyncEventKind::PrCommentAdded,
        SyncEventKind::WorkItemChanged,
    ] {
        let event = events.iter().find(|e| e.kind == kind).unwrap();
        assert!(!event.self_caused);
        assert_eq!(event.actor.as_deref(), Some(pr.author.as_str()));
    }
    for subject in [Subject::Pr(pr.id), Subject::WorkItem(wi.id)] {
        assert!(
            f.store
                .with_conn(|c| adoken_store::read::changed_since(c, &subject, opened))
                .unwrap()
        );
    }
    let enricher = adoken_notify::source::Enricher::load(&f.store).unwrap();
    let clock = adoken_notify::FakeClock::new(f.h.clock.now_ms(), 0);
    let mut settings = adoken_notify::NotifySettings {
        show_self_caused: true,
        ..Default::default()
    };
    for rule in [
        adoken_notify::RuleKey::PrComment,
        adoken_notify::RuleKey::Push,
        adoken_notify::RuleKey::WorkItemChanged,
    ] {
        settings.priorities.set(rule, adoken_notify::Priority::P0);
    }
    let mut notifications = adoken_notify::Engine::new(Arc::new(clock), settings);
    let mut effects = Effects::default();
    for event in &events {
        let ctx = enricher.context(&f.store, event).unwrap();
        assert!(ctx.pin_only);
        assert_eq!(
            notifications.ingest(
                adoken_notify::Incoming::from_sync(event, ctx),
                &DeliveryEnv::default(),
                &mut effects
            ),
            adoken_notify::Outcome::PinOnly
        );
    }
    assert!(
        notifications
            .list(&adoken_notify::Filter::default())
            .is_empty()
    );
    assert_eq!(effects, Effects::default());
    // Replayed passes must not turn the same activity into new events.
    f.warm_pass(&engine).await;
    engine.run_tier(Tier::Hot).await.unwrap();
    let replay: Vec<_> = f
        .store
        .sync_events_since(0, 100_000)
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.at > opened
                && (e.subject == Subject::Pr(pr.id) || e.subject == Subject::WorkItem(wi.id))
        })
        .collect();
    assert_eq!(replay, events);
    // Removing the pin before notification polling can't promote its queued
    // activity into inbox entries.
    f.unpin(&PinSubject::Pr(pr.id));
    f.unpin(&PinSubject::WorkItem(wi.id));
    let late_enricher = adoken_notify::source::Enricher::load(&f.store).unwrap();
    for event in &events {
        let ctx = late_enricher.context(&f.store, event).unwrap();
        assert!(ctx.pin_only);
        assert_eq!(
            notifications.ingest(
                adoken_notify::Incoming::from_sync(event, ctx),
                &DeliveryEnv::default(),
                &mut effects
            ),
            adoken_notify::Outcome::PinOnly
        );
    }
    assert!(
        notifications
            .list(&adoken_notify::Filter::default())
            .is_empty()
    );
    assert_eq!(effects, Effects::default());
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn own_activity_keeps_dots_clear_and_pins_preserve_involved_notifications() {
    let f = Fixture::new().await;
    let me = f.ds.me.clone();
    let other = f.ds.people.iter().find(|p| p.id != me).unwrap().id.clone();
    let pr =
        f.ds.prs
            .iter()
            .find(|p| p.status == PrStatus::Active && p.author != me && p.is_reviewer(&me))
            .unwrap()
            .id;
    let wi =
        f.ds.work_items
            .iter()
            .find(|w| w.assigned_to.as_deref() == Some(me.as_str()) && w.is_open())
            .unwrap()
            .id;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    f.pin(PinSubject::Pr(pr));
    f.pin(PinSubject::WorkItem(wi));
    let opened = f.h.clock.now_ms();
    f.h.clock.advance(2 * MINUTE);
    {
        let mut state = f.h.state();
        let now = state.now();
        let reviewer = state
            .prs
            .get_mut(&pr)
            .unwrap()
            .reviewers
            .iter_mut()
            .find(|r| r.person == me)
            .unwrap();
        reviewer.vote = if reviewer.vote == Vote::Approved {
            Vote::WaitingForAuthor
        } else {
            Vote::Approved
        };
        state.threads.entry(pr).or_default().push(PrThread {
            id: 990_300,
            pr_id: pr,
            status: ThreadStatus::Active,
            anchor: None,
            iteration: None,
            updated: now,
            comments: vec![Comment {
                id: 1,
                parent_id: None,
                author: me.clone(),
                body: "My own comment".into(),
                created: now,
                updated: now,
                is_system: false,
                is_deleted: false,
            }],
        });
        state.touch_wi(wi, |w| {
            w.title = "My own edit".into();
            w.changed_by = me.clone();
        });
    }
    engine.run_tier(Tier::Warm).await.unwrap();
    engine.run_tier(Tier::Hot).await.unwrap();
    let events: Vec<_> = f
        .store
        .sync_events_since(0, 100_000)
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.at > opened && (e.subject == Subject::Pr(pr) || e.subject == Subject::WorkItem(wi))
        })
        .collect();
    assert_eq!(events.len(), 3, "{events:#?}");
    for kind in [
        SyncEventKind::PrVoteChanged,
        SyncEventKind::PrCommentAdded,
        SyncEventKind::WorkItemChanged,
    ] {
        let event = events.iter().find(|e| e.kind == kind).unwrap();
        assert!(event.self_caused);
        assert_eq!(event.actor.as_deref(), Some(me.as_str()));
    }
    for subject in [Subject::Pr(pr), Subject::WorkItem(wi)] {
        assert!(
            !f.store
                .with_conn(|c| adoken_store::read::changed_since(c, &subject, opened))
                .unwrap()
        );
    }

    // Other people's activity retains the normal inbox behavior on subjects
    // that already relate to me; pinning doesn't suppress it.
    let after_self = f.h.clock.now_ms();
    f.h.clock.advance(2 * MINUTE);
    {
        let mut state = f.h.state();
        let now = state.now();
        let thread = state
            .threads
            .get_mut(&pr)
            .unwrap()
            .iter_mut()
            .find(|t| t.id == 990_300)
            .unwrap();
        thread.comments.push(Comment {
            id: 2,
            parent_id: Some(1),
            author: other.clone(),
            body: "A teammate's reply".into(),
            created: now,
            updated: now,
            is_system: false,
            is_deleted: false,
        });
        thread.updated = now;
        state.touch_wi(wi, |w| {
            w.title = "A teammate's edit".into();
            w.changed_by = other.clone();
        });
    }
    engine.run_tier(Tier::Warm).await.unwrap();
    engine.run_tier(Tier::Hot).await.unwrap();
    let events: Vec<_> = f
        .store
        .sync_events_since(0, 100_000)
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.at > after_self
                && (e.subject == Subject::Pr(pr) || e.subject == Subject::WorkItem(wi))
        })
        .collect();
    assert_eq!(events.len(), 2, "{events:#?}");
    let enricher = adoken_notify::source::Enricher::load(&f.store).unwrap();
    let mut notifications = adoken_notify::Engine::new(
        Arc::new(adoken_notify::FakeClock::new(f.h.clock.now_ms(), 0)),
        adoken_notify::NotifySettings::default(),
    );
    for event in &events {
        assert!(!event.self_caused);
        let ctx = enricher.context(&f.store, event).unwrap();
        assert!(!ctx.pin_only);
        assert!(matches!(
            notifications.ingest_silently(adoken_notify::Incoming::from_sync(event, ctx)),
            adoken_notify::Outcome::Added(_)
        ));
        assert!(
            f.store
                .with_conn(|c| adoken_store::read::changed_since(c, &event.subject, after_self))
                .unwrap()
        );
    }
    assert_eq!(
        notifications.list(&adoken_notify::Filter::default()).len(),
        2
    );
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_pr_wins_a_full_hot_set_before_thread_rotation() {
    let f = Fixture::new().await;
    let pr =
        f.ds.prs
            .iter()
            .find(|p| {
                p.status == PrStatus::Active
                    && p.author != f.ds.me
                    && !p.is_reviewer(&f.ds.me)
                    && !adoken_demo::SCENARIO_PRS.contains(&p.id)
            })
            .unwrap()
            .id;
    let mut cfg = SyncConfig::new(f.h.org_url());
    cfg.hot_cap = 1;
    cfg.max_thread_fetches = 1;
    let engine = Engine::new(f.store.clone(), f.h.transport(), f.h.clock.clone(), cfg);
    f.baseline(&engine).await;
    f.pin(PinSubject::Pr(pr));
    f.h.clock.advance(MINUTE);
    let before = f.h.state().log.len();
    engine.run_tier(Tier::Hot).await.unwrap();
    let threads: Vec<_> = f.h.state().log[before..]
        .iter()
        .filter(|l| l.path.ends_with("/threads"))
        .map(|l| l.path.clone())
        .collect();
    assert_eq!(threads.len(), 1);
    assert!(threads[0].ends_with(&format!("/pullRequests/{pr}/threads")));
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_push_identity_not_pr_authorship_controls_change_dots() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let me = f.ds.me.clone();
    let other = f.ds.people.iter().find(|p| p.id != me).unwrap().id.clone();
    let mine =
        f.ds.prs
            .iter()
            .find(|p| p.author == me && p.status == PrStatus::Active)
            .unwrap()
            .id;
    let theirs =
        f.ds.prs
            .iter()
            .find(|p| p.author != me && !p.is_reviewer(&me) && p.status == PrStatus::Active)
            .unwrap()
            .id;
    f.pin(PinSubject::Pr(mine));
    f.pin(PinSubject::Pr(theirs));
    let opened = f.h.clock.now_ms();
    f.h.clock.advance(MINUTE);
    f.h.state().push_pr(mine, "teammate-on-my-pr", &other);
    f.h.state().push_pr(theirs, "me-on-their-pr", &me);
    let pass = f.warm_pass(&engine).await;
    for (id, actor, self_caused, changed) in [
        (mine, other.as_str(), false, true),
        (theirs, me.as_str(), true, false),
    ] {
        let events: Vec<_> = f
            .store
            .sync_events_since(0, 100_000)
            .unwrap()
            .into_iter()
            .filter(|e| {
                e.subject == Subject::Pr(id) && e.kind == SyncEventKind::PrPushed && e.at > opened
            })
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].actor.as_deref(), Some(actor));
        assert_eq!(events[0].self_caused, self_caused);
        assert_eq!(
            f.store
                .with_conn(|c| adoken_store::read::changed_since(c, &Subject::Pr(id), opened))
                .unwrap(),
            changed
        );
        assert_eq!(
            pass.iter()
                .filter(|l| l.path.ends_with(&format!("/pullRequests/{id}/iterations")))
                .count(),
            1
        );
    }
    let pass = f.warm_pass(&engine).await;
    assert!(
        pass.iter().all(|l| !l.path.ends_with("/iterations")),
        "no actor lookup on unchanged heads"
    );
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_pr_denials_retain_pins_and_do_not_abort_other_refreshes() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let pr = f
        .ds
        .prs
        .iter()
        .find(|p| p.author != f.ds.me && !p.is_reviewer(&f.ds.me) && p.status == PrStatus::Active)
        .unwrap()
        .clone();
    f.pin(PinSubject::Pr(pr.id));
    let wi =
        f.ds.work_items
            .iter()
            .find(|w| w.assigned_to.as_deref() != Some(f.ds.me.as_str()) && w.created_by != f.ds.me)
            .unwrap()
            .id;
    f.pin(PinSubject::WorkItem(wi));
    f.warm_pass(&engine).await;
    f.h.state().fail(
        &format!("/{}/_apis/git/pullrequests", pr.project_id),
        403,
        &[],
        1,
    );
    f.warm_pass(&engine).await;
    assert_eq!(
        f.pin_of(&PinSubject::Pr(pr.id)).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&PinSubject::Pr(pr.id)).unavailable, None);
    f.h.state().prs.remove(&pr.id);
    f.h.state()
        .touch_wi(wi, |w| w.title = "Refreshed despite missing PR".into());
    f.warm_pass(&engine).await;
    assert_eq!(
        f.pin_of(&PinSubject::Pr(pr.id)).unavailable,
        Some(PinUnavailable::Gone)
    );
    assert_eq!(f.pin_of(&PinSubject::Pr(pr.id)).title, pr.title);
    assert_eq!(f.store.pr(pr.id).unwrap().unwrap().pr.title, pr.title);
    assert_eq!(
        f.store.work_item(wi).unwrap().unwrap().title,
        "Refreshed despite missing PR"
    );
    f.h.state().prs.insert(pr.id, pr.clone());
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&PinSubject::Pr(pr.id)).unavailable, None);
    f.h.clock.advance(MINUTE);
    f.h.state()
        .fail(&format!("/pullRequests/{}/threads", pr.id), 403, &[], 1);
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(
        f.pin_of(&PinSubject::Pr(pr.id)).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    f.h.clock.advance(MINUTE);
    f.h.state()
        .fail(&format!("/pullRequests/{}/threads", pr.id), 404, &[], 1);
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(
        f.pin_of(&PinSubject::Pr(pr.id)).unavailable,
        Some(PinUnavailable::Gone)
    );
    f.h.clock.advance(MINUTE);
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(f.pin_of(&PinSubject::Pr(pr.id)).unavailable, None);
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unresolved_work_item_project_keeps_availability_failure_until_applied() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let project = Project {
        id: "denied-project".into(),
        name: "Denied project".into(),
    };
    let mut wi =
        f.ds.work_items
            .iter()
            .find(|w| w.created_by != f.ds.me && w.assigned_to.as_deref() != Some(f.ds.me.as_str()))
            .unwrap()
            .clone();
    wi.id = 990_400;
    wi.project_id = project.id.clone();
    f.h.state().projects.push(project.clone());
    f.h.state().work_items.insert(wi.id, (wi.clone(), 0));
    let subject = PinSubject::WorkItem(wi.id);
    f.pin(subject.clone());
    f.store
        .write(|tx| write::set_pin_title(tx, &subject, "Last known title"))
        .unwrap();
    f.h.state()
        .fail("/_apis/projects/Denied%20project", 403, &[], 1);
    f.warm_pass(&engine).await;
    assert!(f.store.work_item(wi.id).unwrap().is_none());
    assert_eq!(
        f.pin_of(&subject).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    assert_eq!(f.pin_of(&subject).title, "Last known title");
    f.h.state()
        .fail("/_apis/projects/Denied%20project", 404, &[], 1);
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&subject).unavailable, Some(PinUnavailable::Gone));
    assert_eq!(f.pin_of(&subject).title, "Last known title");
    f.warm_pass(&engine).await;
    assert_eq!(
        f.store.work_item(wi.id).unwrap().unwrap().project_id,
        project.id
    );
    assert_eq!(f.pin_of(&subject).unavailable, None);
    assert_eq!(f.pin_of(&subject).title, wi.title);
    assert_eq!(
        f.h.state()
            .requests("/_apis/projects/Denied%20project")
            .len(),
        3
    );
    f.warm_pass(&engine).await;
    assert_eq!(
        f.h.state()
            .requests("/_apis/projects/Denied%20project")
            .len(),
        3
    );
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_definition_project_baselines_then_watches_running_pin_at_hot_cadence() {
    let f = Fixture::new().await;
    let engine = f.engine_for(Some(vec![f.ds.projects[0].id.clone()]));
    f.baseline(&engine).await;
    let project = Project {
        id: "pipeline-project".into(),
        name: "Pipeline project".into(),
    };
    let def = Pipeline {
        id: 990_500,
        project_id: project.id.clone(),
        name: "External CI".into(),
        folder: "\\".into(),
    };
    let other =
        f.ds.people
            .iter()
            .find(|p| p.id != f.ds.me)
            .unwrap()
            .id
            .clone();
    let mut run = f.ds.runs[0].clone();
    run.id = 990_501;
    run.project_id = project.id.clone();
    run.pipeline_id = def.id;
    run.pipeline_name = def.name.clone();
    run.pr_id = None;
    run.requested_for = Some(other);
    run.queued = f.h.clock.now_ms() - 2 * MINUTE;
    run.finish = Some(f.h.clock.now_ms() - MINUTE);
    run.status = RunStatus::Completed;
    run.result = Some(RunResult::Succeeded);
    {
        let mut state = f.h.state();
        state.projects.push(project.clone());
        state.pipelines.insert(def.id, def.clone());
        state.runs.insert(run.id, run.clone());
    }
    let subject = PinSubject::Definition {
        project_id: project.id.clone(),
        id: def.id,
    };
    f.pin(subject.clone());
    f.warm_pass(&engine).await;
    assert!(f.store.projects().unwrap().contains(&project));
    assert!(!f.store.synced_projects().unwrap().contains(&project));
    assert_eq!(
        f.store.run(run.id).unwrap().unwrap().pipeline_name,
        def.name
    );
    assert!(
        f.store
            .sync_events_since(0, 100_000)
            .unwrap()
            .iter()
            .all(|e| e.subject != Subject::Build(run.id))
    );
    assert!(
        f.store
            .cursor("build.pins", &project.id)
            .unwrap()
            .unwrap()
            .baseline_done
    );
    run.id += 1;
    run.queued = f.h.clock.now_ms();
    run.started = Some(run.queued);
    run.finish = None;
    run.status = RunStatus::InProgress;
    run.result = None;
    f.h.state().runs.insert(run.id, run.clone());
    f.warm_pass(&engine).await;
    run.status = RunStatus::Completed;
    run.result = Some(RunResult::Failed);
    f.h.clock.advance(MINUTE);
    run.finish = Some(f.h.clock.now_ms());
    f.h.state().runs.insert(run.id, run.clone());
    let before = f.h.state().log.len();
    engine.run_tier(Tier::Hot).await.unwrap();
    assert!(
        f.h.state().log[before..]
            .iter()
            .any(|l| l.path.ends_with(&format!("/_apis/build/builds/{}", run.id)))
    );
    assert_eq!(
        f.store.run(run.id).unwrap().unwrap().result,
        Some(RunResult::Failed)
    );
    let events: Vec<_> = f
        .store
        .sync_events_since(0, 100_000)
        .unwrap()
        .into_iter()
        .filter(|e| e.subject == Subject::Build(run.id))
        .collect();
    assert_eq!(events.len(), 1);
    let ctx = adoken_notify::source::Enricher::load(&f.store)
        .unwrap()
        .context(&f.store, &events[0])
        .unwrap();
    assert!(ctx.pinned);
    assert_eq!(
        adoken_notify::RuleKey::of(adoken_notify::Kind::Sync(events[0].kind), &ctx),
        adoken_notify::RuleKey::BuildFailedOther
    );
    let mut notifications = adoken_notify::Engine::new(
        Arc::new(adoken_notify::FakeClock::new(f.h.clock.now_ms(), 0)),
        adoken_notify::NotifySettings::default(),
    );
    assert!(matches!(
        notifications.ingest_silently(adoken_notify::Incoming::from_sync(&events[0], ctx)),
        adoken_notify::Outcome::Added(_)
    ));
    let inbox = notifications.list(&adoken_notify::Filter::default());
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].priority, adoken_notify::Priority::P2);
    f.unpin(&subject);
    f.h.clock.advance(MINUTE);
    let before = f.h.state().log.len();
    engine.run_tier(Tier::Hot).await.unwrap();
    assert!(
        f.h.state().log[before..]
            .iter()
            .all(|l| !l.path.contains(&project.id))
    );
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn definition_denial_and_deleted_definition_differ_from_valid_no_runs() {
    let f = Fixture::new().await;
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let def = f.ds.pipelines[0].clone();
    let subject = PinSubject::Definition {
        project_id: def.project_id.clone(),
        id: def.id,
    };
    f.pin(subject.clone());
    f.warm_pass(&engine).await;
    f.h.state().fail(
        &format!("/{}/_apis/build/builds", def.project_id),
        403,
        &[],
        2,
    );
    f.warm_pass(&engine).await;
    assert_eq!(
        f.pin_of(&subject).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    assert_eq!(f.pin_of(&subject).title, def.name);
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&subject).unavailable, None);
    f.h.state()
        .runs
        .retain(|_, run| run.project_id != def.project_id || run.pipeline_id != def.id);
    f.warm_pass(&engine).await;
    assert_eq!(
        f.pin_of(&subject).unavailable,
        None,
        "a valid definition with no runs is readable"
    );
    assert!(
        f.store
            .with_conn(|c| adoken_store::read::definition_runs(c, &def.project_id, def.id, 1))
            .unwrap()
            .is_empty()
    );
    f.h.state()
        .fail(&format!("/_apis/build/definitions/{}", def.id), 403, &[], 1);
    f.warm_pass(&engine).await;
    assert_eq!(
        f.pin_of(&subject).unavailable,
        Some(PinUnavailable::Forbidden)
    );
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&subject).unavailable, None);
    f.h.state().pipelines.remove(&def.id);
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&subject).unavailable, Some(PinUnavailable::Gone));
    assert_eq!(f.pin_of(&subject).title, def.name);
    assert_eq!(f.store.pins().unwrap().len(), 1);
    f.h.state().pipelines.insert(def.id, def.clone());
    f.warm_pass(&engine).await;
    assert_eq!(f.pin_of(&subject).unavailable, None);
    f.assert_read_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_sync_terminal_date_precedes_later_field_edit_date() {
    use adoken_model::time::{DAY, HOUR};
    let f = Fixture::new().await;
    let id =
        f.ds.work_items
            .iter()
            .find(|w| w.created_by != f.ds.me && w.assigned_to.as_deref() != Some(f.ds.me.as_str()))
            .unwrap()
            .id;
    let closed = f.h.clock.now_ms() - DAY;
    {
        let mut state = f.h.state();
        state.touch_wi(id, |w| {
            w.state = "Closed".into();
            w.state_category = StateCategory::Completed;
        });
        // ADO often has no ClosedDate but does expose StateChangeDate.
        state
            .wi_transition_dates
            .insert(id, (Some(0), Some(closed)));
    }
    let engine = f.engine_for(None);
    f.baseline(&engine).await;
    let read_closed = || {
        f.store
            .with_conn(|c| adoken_store::read::work_item_closed_at(c, id))
            .unwrap()
    };
    assert_eq!(read_closed(), Some(closed));
    f.pin(PinSubject::WorkItem(id));
    f.h.clock.advance(23 * HOUR);
    f.h.state()
        .touch_wi(id, |w| w.title = "A much later field edit".into());
    f.warm_pass(&engine).await;
    assert_eq!(read_closed(), Some(closed));
    f.h.state().touch_wi(id, |w| {
        w.state = "Active".into();
        w.state_category = StateCategory::InProgress;
    });
    f.warm_pass(&engine).await;
    assert_eq!(read_closed(), None);
    let reclosed = f.h.clock.now_ms();
    f.h.state()
        .wi_transition_dates
        .insert(id, (Some(reclosed), Some(reclosed)));
    f.h.state().touch_wi(id, |w| {
        w.state = "Closed".into();
        w.state_category = StateCategory::Completed;
    });
    f.warm_pass(&engine).await;
    assert_eq!(read_closed(), Some(reclosed));
    f.assert_read_only();
}
