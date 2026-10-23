use std::collections::HashSet;

use adoken_store::{PrFilter, RunFilter, WorkItemFilter};

use super::*;

#[test]
fn same_seed_same_dataset() {
    assert_eq!(dataset(1), dataset(1));
    assert_ne!(dataset(1).prs, dataset(2).prs);
}

#[test]
fn shape_matches_contract() {
    let ds = dataset(DEFAULT_SEED);
    assert_eq!(ds.projects.len(), 3);
    assert_eq!(ds.prs.len(), 300);
    assert_eq!(ds.work_items.len(), 5000);
    for s in PrStatus::ALL {
        assert!(
            ds.prs.iter().filter(|p| p.status == *s).count() >= 20,
            "{s}"
        );
    }
    let awaiting_me = ds
        .prs
        .iter()
        .filter(|p| p.status == PrStatus::Active && p.vote_of(&ds.me) == Some(Vote::NoVote))
        .count();
    assert!(awaiting_me >= 5, "{awaiting_me}");
    let stale = ds
        .prs
        .iter()
        .filter(|p| p.status == PrStatus::Active && p.updated < ds.now - 7 * DAY)
        .count();
    assert!(stale >= 5, "{stale}");
    assert!(ds.runs.iter().any(|r| r.result == Some(RunResult::Failed)));
    assert!(ds.runs.iter().any(|r| r.status == RunStatus::InProgress));
    assert!(ds.mentions.len() >= 5);
    assert!(!ds.past_events.is_empty());
    assert!(
        ds.prs
            .iter()
            .all(|p| p.created <= p.updated && p.updated <= ds.now)
    );
    assert!(
        ds.work_items
            .iter()
            .all(|w| w.created <= w.changed && w.changed <= ds.now)
    );
}

#[test]
fn references_resolve() {
    let ds = dataset(DEFAULT_SEED);
    let people: HashSet<&str> = ds.people.iter().map(|p| p.id.as_str()).collect();
    let blobs: HashSet<&str> = ds.blobs.iter().map(|(id, _)| id.as_str()).collect();
    let repos: HashSet<&str> = ds.repos.iter().map(|r| r.id.as_str()).collect();
    for pr in &ds.prs {
        assert!(people.contains(pr.author.as_str()));
        assert!(repos.contains(pr.repo_id.as_str()));
        assert!(
            pr.reviewers
                .iter()
                .all(|r| people.contains(r.person.as_str()))
        );
    }
    for extra in ds.pr_extras.values() {
        for f in &extra.files {
            for b in [&f.old_content, &f.new_content].into_iter().flatten() {
                assert!(blobs.contains(b.as_str()), "{}", f.path);
            }
        }
        for t in &extra.threads {
            assert!(
                t.comments
                    .iter()
                    .all(|c| people.contains(c.author.as_str()))
            );
            if let Some(b) = t.anchor.as_ref().and_then(|a| a.iteration_blob.as_ref()) {
                assert!(blobs.contains(b.as_str()));
            }
        }
    }
    for wi in &ds.work_items {
        assert!(wi.assigned_to.iter().all(|a| people.contains(a.as_str())));
    }
}

#[test]
fn rich_prs_have_multi_file_diffs_and_anchored_threads() {
    let ds = dataset(DEFAULT_SEED);
    for id in RICH_PR_IDS {
        let extra = &ds.pr_extras[&id];
        assert!(extra.files.len() >= 2, "{id}");
        assert!(extra.threads.iter().any(|t| t.anchor.is_some()), "{id}");
    }
    let rust = &ds.pr_extras[&RICH_PR_IDS[0]];
    assert!(rust.threads.iter().any(|t| {
        t.iteration == Some(1)
            && t.anchor
                .as_ref()
                .is_some_and(|a| a.iteration_blob.is_some())
    }));
    let ts = &ds.pr_extras[&RICH_PR_IDS[1]];
    for ct in [
        ChangeType::Add,
        ChangeType::Edit,
        ChangeType::Delete,
        ChangeType::Rename,
    ] {
        assert!(ts.files.iter().any(|f| f.change_type == ct), "{ct}");
    }
}

#[test]
fn nudge_scenarios_hold() {
    let ds = dataset(DEFAULT_SEED);
    let pr = |id| ds.pr(id).unwrap();
    assert!(
        pr(4290)
            .reviewers
            .iter()
            .any(|r| r.vote == Vote::WaitingForAuthor && r.voted_at > pr(4290).last_push)
    );
    let latest = ds
        .runs
        .iter()
        .filter(|r| r.pr_id == Some(4291))
        .max_by_key(|r| r.queued)
        .unwrap();
    assert_eq!(latest.result, Some(RunResult::Failed));
    assert!(pr(4292).reviewers.iter().all(|r| r.vote == Vote::NoVote));
    assert!(
        ds.pr_extras[&4292]
            .threads
            .iter()
            .all(|t| t.comments.iter().all(|c| c.is_system))
    );
    assert_eq!(pr(4293).merge_status, MergeStatus::Conflicts);
    assert_eq!(pr(4294).vote_of(&ds.me), Some(Vote::NoVote));
    assert_eq!(pr(4295).status, PrStatus::Completed);
    let linked = ds
        .work_items
        .iter()
        .find(|w| w.linked_pr_ids == [4295])
        .unwrap();
    assert_eq!(linked.state, "Doing");
    assert!(
        ds.work_items
            .iter()
            .any(|w| w.assigned_to.as_deref() == Some(ds.me.as_str())
                && w.is_open()
                && w.changed <= ds.now - 8 * DAY
                && w.linked_pr_ids.is_empty())
    );
}

#[test]
fn events_are_ordered_and_unique() {
    let ds = dataset(3);
    let evs: Vec<SyncEvent> = events(&ds).take(200).collect();
    assert!(evs.windows(2).all(|w| w[0].at < w[1].at));
    assert!(evs[0].at > ds.now);
    let keys: HashSet<String> = evs.iter().map(SyncEvent::unique_key).collect();
    assert_eq!(keys.len(), evs.len());
    assert_eq!(evs, events(&ds).take(200).collect::<Vec<_>>());
}

#[test]
fn seeded_store_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let store = open_store_at(dir.path(), DEMO_NOW).unwrap();
    let seed_time = started.elapsed();
    let size = std::fs::metadata(dir.path().join("adoken.sqlite"))
        .unwrap()
        .len()
        + std::fs::metadata(dir.path().join("adoken.sqlite-wal"))
            .map(|m| m.len())
            .unwrap_or(0);
    eprintln!(
        "demo seed: {seed_time:?}, {:.1} MiB",
        size as f64 / 1_048_576.0
    );

    let ds = dataset(DEFAULT_SEED);
    assert_eq!(store.meta("demo").unwrap().as_deref(), Some("1"));
    assert_eq!(store.me().unwrap().unwrap(), me());
    assert!(store.last_sync().unwrap().is_some());
    assert_eq!(store.prs(&PrFilter::default()).unwrap().len(), 300);
    assert_eq!(
        store.work_items(&WorkItemFilter::default()).unwrap().len(),
        5000
    );
    assert_eq!(
        store.runs(&RunFilter::default()).unwrap().len(),
        ds.runs.len()
    );
    for id in RICH_PR_IDS {
        let detail = store.pr(id).unwrap().unwrap();
        let extra = &ds.pr_extras[&id];
        assert_eq!(&detail.pr, ds.pr(id).unwrap());
        assert_eq!(detail.files, extra.files);
        assert_eq!(detail.threads, extra.threads);
        for f in &detail.files {
            if let Some(b) = &f.new_content {
                assert!(store.blob(b).unwrap().is_some());
            }
        }
    }
    assert_eq!(
        store.mentions(&ds.me, 10_000).unwrap().len(),
        ds.mentions.len()
    );
    assert_eq!(
        store.sync_events_since(0, 10_000).unwrap().len(),
        ds.past_events.len()
    );
    let docs = store.search_docs().unwrap();
    assert!(docs.len() > 5300);
    // Re-opening doesn't reseed.
    drop(store);
    let again = open_store_at(dir.path(), DEMO_NOW + DAY).unwrap();
    assert_eq!(again.meta("demo_now").unwrap(), Some(DEMO_NOW.to_string()));
}

#[test]
fn live_feed_appends_due_events_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store_at(dir.path(), DEMO_NOW).unwrap();
    let before = store.sync_events_since(0, 100_000).unwrap().len();
    let mut feed = LiveFeed::for_store(&store).unwrap().unwrap();
    assert_eq!(feed.tick(&store, DEMO_NOW).unwrap(), 0);
    let n = feed.tick(&store, DEMO_NOW + HOUR).unwrap();
    assert!(n >= 10, "{n}");
    // A fresh feed (app restart) re-derives the same keys: no duplicates.
    let mut again = LiveFeed::for_store(&store).unwrap().unwrap();
    assert_eq!(again.tick(&store, DEMO_NOW + HOUR).unwrap(), 0);
    assert_eq!(
        store.sync_events_since(0, 100_000).unwrap().len(),
        before + n
    );
}

#[test]
fn initial_pins_have_live_status_and_local_edits_survive_reseeding() {
    let dir = tempfile::tempdir().unwrap();
    let ds = dataset(DEFAULT_SEED);
    let store = open_store_at(dir.path(), ds.now).unwrap();
    let initial = store.pins().unwrap();
    assert_eq!(initial, pins(&ds));
    let conflict = &initial[0];
    assert_eq!(ds.pr(4293).unwrap().merge_status, MergeStatus::Conflicts);
    assert!(
        store
            .with_conn(|c| read::changed_since(c, &Subject::Pr(4293), conflict.seen_at()))
            .unwrap()
    );
    let PinSubject::Pr(approved_id) = initial[1].subject else {
        panic!("approved PR")
    };
    let approved = ds.pr(approved_id).unwrap();
    assert!(!approved.is_draft);
    assert!(approved.reviewers.iter().all(|r| r.vote == Vote::Approved));
    assert!(!ds.runs.iter().any(|r| r.pr_id == Some(approved_id)));
    let PinSubject::WorkItem(wi_id) = initial[2].subject else {
        panic!("active work item")
    };
    assert_eq!(
        ds.work_items
            .iter()
            .find(|wi| wi.id == wi_id)
            .unwrap()
            .state_category,
        StateCategory::InProgress
    );
    let PinSubject::Definition { ref project_id, id } = initial[3].subject else {
        panic!("definition")
    };
    let latest = ds
        .runs
        .iter()
        .filter(|r| r.project_id == *project_id && r.pipeline_id == id)
        .max_by_key(|r| (r.queued, r.id))
        .unwrap();
    assert_eq!(latest.result, Some(RunResult::Failed));

    store
        .write(|tx| {
            write::rename_pin(tx, &conflict.subject, "My local label")?;
            write::move_pin(tx, &conflict.subject, 2)?;
            write::mark_pin_opened(tx, &conflict.subject, ds.now, None, true)?;
            write::unpin(tx, &initial[1].subject)?;
            Ok(())
        })
        .unwrap();
    let edited = store.pins().unwrap();
    seed(&store, &ds).unwrap();
    assert_eq!(store.pins().unwrap(), edited);
}
