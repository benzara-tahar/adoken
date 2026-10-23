//! Review files of real PRs (spec 0004 FR-1..FR-3) against the mock ADO
//! server: paged change lists, merge-base blobs, the content-addressed
//! blob cache (AC-4), and the hot tier's file lists after a push.

mod support;

use std::sync::Arc;

use adoken_demo::{DEFAULT_SEED, Dataset, dataset};
use adoken_model::*;
use adoken_store::Store;
use adoken_store::read;
use adoken_sync::pr_files::{PrFiles, RepoRef, files_due, git_blob_id};
use adoken_sync::*;
use support::{Harness, merge_base};

struct Fixture {
    ds: Dataset,
    h: Harness,
    store: Store,
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Fixture {
        let ds = dataset(DEFAULT_SEED);
        let h = Harness::start(&ds).await;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        // The PR rows (list level) as the warm tier would have stored them.
        store
            .write(|tx| {
                for p in &ds.people {
                    adoken_store::write::upsert_person(tx, p, p.id == ds.me)?;
                }
                for p in &ds.projects {
                    adoken_store::write::upsert_project(tx, p)?;
                }
                for r in &ds.repos {
                    adoken_store::write::upsert_repo(tx, r)?;
                }
                for pr in &ds.prs {
                    adoken_store::write::upsert_pr(tx, pr)?;
                }
                Ok(())
            })
            .unwrap();
        Fixture { ds, h, store, dir }
    }

    fn files(&self) -> Arc<PrFiles> {
        self.files_for(self.store.clone())
    }

    fn files_for(&self, store: Store) -> Arc<PrFiles> {
        let clock: Arc<dyn Clock> = self.h.clock.clone();
        Arc::new(PrFiles::new(
            store,
            self.h.transport(),
            Arc::new(Governor::new(clock.clone())),
            clock,
            self.h.org_url(),
        ))
    }

    /// A demo PR with file content (rich PRs carry edits and renames).
    fn pr_with_edits(&self) -> PullRequest {
        let id = *self
            .ds
            .pr_extras
            .iter()
            .find(|(_, e)| {
                e.files
                    .iter()
                    .any(|f| f.change_type == ChangeType::Edit && f.old_content.is_some())
            })
            .map(|(id, _)| id)
            .expect("a PR with an edited file");
        self.ds.prs.iter().find(|p| p.id == id).unwrap().clone()
    }

    fn blob_requests(&self) -> usize {
        self.h.state().requests("/blobs/").len()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn change_lists_over_100_files_are_read_page_by_page() {
    let f = Fixture::new().await;
    let pr = f.pr_with_edits();
    let many: Vec<PrFileChange> = (0..250)
        .map(|i| PrFileChange {
            path: format!("/src/gen/file_{i:03}.rs"),
            original_path: None,
            change_type: ChangeType::Add,
            old_content: None,
            new_content: Some(git_blob_id(format!("// {i}\n").as_bytes())),
            change_tracking_id: Some(i + 1),
        })
        .collect();
    {
        let mut s = f.h.state();
        s.pr_files.insert(pr.id, many.clone());
        // What tools that stop at the first page see: ADO's default of 100.
        s.changes_page = 100;
    }
    let synced = f.files().sync_files(Tier::User, &pr).await.unwrap();
    assert_eq!(synced.files, 250);
    let skips: Vec<String> =
        f.h.state()
            .requests("/changes")
            .iter()
            .map(|l| l.param("$skip").unwrap().to_owned())
            .collect();
    assert_eq!(skips, ["0", "100", "200"]);
    let stored = f.store.pr(pr.id).unwrap().unwrap().files;
    assert_eq!(stored, many, "every page, in order, with tracking ids");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn base_side_is_the_merge_base_blob_of_the_latest_iteration() {
    let f = Fixture::new().await;
    let pr = f.pr_with_edits();
    let served = f.h.state().pr_files[&pr.id].clone();
    let latest = f.h.state().iterations[&pr.id].last().unwrap().id;
    let synced = f.files().sync_files(Tier::User, &pr).await.unwrap();
    assert_eq!(synced.iteration, latest);

    let s = f.h.state();
    let changes = s.requests("/changes");
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].param("$compareTo"),
        Some("0"),
        "against the merge base"
    );
    assert!(
        changes[0]
            .path
            .ends_with(&format!("/iterations/{latest}/changes"))
    );
    drop(s);

    let detail = f.store.pr(pr.id).unwrap().unwrap();
    // Upper-case ids on the wire, lower-case in the store; the edit's base
    // side is `originalObjectId` (the merge-base blob).
    assert_eq!(detail.files, served);
    let sync = f
        .store
        .with_conn(|c| read::pr_file_sync(c, pr.id))
        .unwrap()
        .unwrap();
    assert_eq!(sync.base_commit, Some(merge_base(pr.id)));
    assert_eq!(Some(sync.source_commit.clone()), pr.last_source_commit);
    assert_eq!(
        detail.iterations.iter().map(|it| it.id).max(),
        Some(latest),
        "the displayed iteration (comment context) is the latest"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cached_blobs_cost_zero_requests_even_after_reopen() {
    let f = Fixture::new().await;
    let pr = f.pr_with_edits();
    let files = f.files();
    files.sync_files(Tier::User, &pr).await.unwrap();
    let detail = f.store.pr(pr.id).unwrap().unwrap();
    let ids: Vec<String> = detail
        .files
        .iter()
        .flat_map(|f| [f.old_content.clone(), f.new_content.clone()])
        .flatten()
        .collect();
    let distinct: std::collections::BTreeSet<&String> = ids.iter().collect();
    let repo = RepoRef::of(&pr);

    // Two callers at once (selection + prefetch): one request per blob.
    let (a, b) = tokio::join!(
        files.ensure_blobs(Tier::User, &repo, &ids),
        files.ensure_blobs(Tier::User, &repo, &ids)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(f.blob_requests(), distinct.len());
    let first = &detail.files[0];
    let text = f
        .store
        .blob(
            first
                .new_content
                .as_deref()
                .or(first.old_content.as_deref())
                .unwrap(),
        )
        .unwrap();
    assert!(text.is_some_and(|t| !t.is_empty()));

    // AC-4: re-open (a new store handle on the same directory).
    let reopened = Store::open(f.dir.path().join("adoken.sqlite")).unwrap();
    f.files_for(reopened)
        .ensure_blobs(Tier::User, &repo, &ids)
        .await
        .unwrap();
    assert_eq!(f.blob_requests(), distinct.len(), "zero new blob requests");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_that_does_not_match_its_id_is_not_cached() {
    let f = Fixture::new().await;
    let pr = f.pr_with_edits();
    let id = git_blob_id(b"the real content\n");
    f.h.state()
        .blobs
        .insert(id.clone(), b"<html>sign in</html>".to_vec());
    let files = f.files();
    let result = files
        .ensure_blobs(Tier::User, &RepoRef::of(&pr), std::slice::from_ref(&id))
        .await;
    assert!(result.is_err());
    assert!(!f.store.blobs().contains(&id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hot_tier_fetches_file_lists_once_per_push() {
    let f = Fixture::new().await;
    let mut cfg = SyncConfig::new(f.h.org_url());
    cfg.max_thread_fetches = 500;
    let clock: Arc<dyn Clock> = f.h.clock.clone();
    let engine = Engine::new(f.store.clone(), f.h.transport(), clock, cfg);
    for tier in [Tier::Warm, Tier::Hot] {
        engine.run_tier(tier).await.unwrap();
    }
    let me = f.ds.me.clone();
    let hot: Vec<PullRequest> = f
        .store
        .prs(&adoken_store::PrFilter {
            status: Some(PrStatus::Active),
            involving: Some(me),
            ..Default::default()
        })
        .unwrap();
    let with_iterations: Vec<&PullRequest> = hot
        .iter()
        .filter(|p| f.h.state().iterations.contains_key(&p.id))
        .collect();
    assert!(!with_iterations.is_empty());
    // Drain the queue at the foreground hot cadence (≤ 5 lists per pass,
    // paused while the 5-minute window is at the file-list budget).
    for _ in 0..hot.len() {
        f.h.clock.advance(60 * adoken_model::time::SECOND);
        engine.run_tier(Tier::Hot).await.unwrap();
        assert!(engine.governor().window_cost() <= engine::FILE_LIST_BUDGET + 0.1);
    }
    let synced = |pr: &PullRequest| f.store.with_conn(|c| read::pr_file_sync(c, pr.id)).unwrap();
    for pr in &with_iterations {
        assert!(!files_due(pr, synced(pr).as_ref()), "!{} synced", pr.id);
    }
    let iteration_lists = || {
        f.h.state()
            .requests("/iterations")
            .into_iter()
            .filter(|l| l.path.ends_with("/iterations"))
            .map(|l| (l.path.clone(), l.param("includeCommits").map(str::to_owned)))
            .collect::<Vec<_>>()
    };
    let lists_before = iteration_lists().len();
    let before = f.h.state().requests("/changes").len();
    engine.run_tier(Tier::Hot).await.unwrap();
    assert_eq!(
        f.h.state().requests("/changes").len(),
        before,
        "no push, no refetch"
    );

    // A push: the next hot pass refetches that PR's list only.
    let pushed = with_iterations[0].clone();
    let mut moved = pushed.clone();
    moved.last_source_commit = Some(git_blob_id(b"new head"));
    f.store
        .write(|tx| adoken_store::write::upsert_pr(tx, &moved))
        .unwrap();
    engine.run_tier(Tier::Hot).await.unwrap();
    let after = f.h.state().requests("/changes");
    assert_eq!(after.len(), before + 1);
    assert!(
        after
            .last()
            .unwrap()
            .path
            .contains(&format!("/{}/iterations/", pushed.id))
    );
    assert_eq!(f.blob_requests(), 0, "blobs wait for the review view");
    // Spec 0017 AC-14: the same one iterations request, now with commits.
    let lists = iteration_lists();
    assert_eq!(lists.len(), lists_before + 1, "one iterations request");
    assert_eq!(
        lists.last().unwrap().1.as_deref(),
        Some("true"),
        "includeCommits on the existing request"
    );
}

/// Spec 0017 AC-14: a push's commit count is its commits minus the previous
/// iteration's, whether Azure DevOps lists every commit so far or only the
/// push's; `hasMoreCommits` marks the count as a lower bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn iterations_store_commits_added_per_push() {
    let f = Fixture::new().await;
    let pr = f.pr_with_edits();
    let ids: Vec<u32> = f.h.state().iterations[&pr.id]
        .iter()
        .map(|it| it.id)
        .collect();
    assert!(ids.len() >= 2, "a PR with two pushes");
    let commit = |n: u32| format!("{n:040x}");
    let stored = |f: &Fixture| -> Vec<(Option<u32>, bool)> {
        f.store
            .pr(pr.id)
            .unwrap()
            .unwrap()
            .iterations
            .iter()
            .take(2)
            .map(|it| (it.commit_count, it.commits_truncated))
            .collect()
    };
    // Cumulative lists: 3, then 5 commits so far.
    {
        let mut s = f.h.state();
        s.iteration_commits
            .insert((pr.id, ids[0]), ((1..=3).map(commit).collect(), false));
        s.iteration_commits
            .insert((pr.id, ids[1]), ((1..=5).map(commit).collect(), false));
    }
    f.files().sync_files(Tier::User, &pr).await.unwrap();
    assert_eq!(stored(&f), [(Some(3), false), (Some(2), false)]);

    // Per-push lists: 3, then the push's own 2 (upper-case ids on the wire).
    {
        let mut s = f.h.state();
        s.iteration_commits.insert(
            (pr.id, ids[1]),
            ((4..=5).map(|n| commit(n).to_uppercase()).collect(), true),
        );
    }
    f.files().sync_files(Tier::User, &pr).await.unwrap();
    assert_eq!(stored(&f), [(Some(3), false), (Some(2), true)]);
}
