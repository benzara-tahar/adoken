use adoken_model::*;
use adoken_search::DocKind;

use super::*;

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("org/adoken.sqlite")).unwrap();
    (dir, store)
}

fn person(id: &str, name: &str) -> Person {
    Person {
        id: id.into(),
        display_name: name.into(),
        unique_name: Some(format!("{}@contoso.com", name.to_lowercase())),
        is_group: false,
        image_url: None,
    }
}

fn seed_org(s: &Store) {
    s.write(|tx| {
        write::upsert_person(tx, &person("me", "Ada Me"), true)?;
        write::upsert_person(tx, &person("bo", "Bo Other"), false)?;
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
                name: "payments-api".into(),
                default_branch: Some("refs/heads/main".into()),
            },
        )?;
        Ok(())
    })
    .unwrap();
}

fn pr(id: i64) -> PullRequest {
    PullRequest {
        id,
        repo_id: "r1".into(),
        project_id: "p1".into(),
        title: format!("Fix flaky checkout retry {id}"),
        description: "Retries the checkout call.".into(),
        author: "bo".into(),
        reviewers: vec![Reviewer {
            person: "me".into(),
            vote: Vote::NoVote,
            is_required: true,
            has_declined: false,
            added_at: Some(1_000),
            voted_at: None,
        }],
        status: PrStatus::Active,
        is_draft: false,
        merge_status: MergeStatus::Succeeded,
        source_branch: "refs/heads/fix/checkout".into(),
        target_branch: "refs/heads/main".into(),
        last_source_commit: Some("abc".into()),
        created: 1_000,
        updated: 2_000,
        closed: None,
        iterations: 1,
        last_push: Some(1_000),
    }
}

fn wi(id: i64, rev: i64, state: &str) -> WorkItem {
    WorkItem {
        id,
        project_id: "p1".into(),
        rev,
        kind: WorkItemType::Bug,
        title: "Checkout times out".into(),
        state: state.into(),
        state_category: StateCategory::of_state(state),
        assigned_to: Some("me".into()),
        created_by: "bo".into(),
        changed_by: "bo".into(),
        area_path: "Payments".into(),
        iteration_path: "Payments\\Sprint 1".into(),
        priority: Some(2),
        tags: vec!["checkout".into(), "flaky".into()],
        parent_id: None,
        linked_pr_ids: vec![7],
        description: None,
        comment_count: 0,
        created: 10,
        changed: 10 * rev,
    }
}

#[test]
fn pr_work_items_follow_the_items_current_links() {
    let (_d, s) = open();
    seed_org(&s);
    let mut other = wi(2, 1, "New");
    other.linked_pr_ids = vec![8];
    s.write(|tx| {
        write::upsert_work_item(tx, &wi(3, 1, "Active"))?;
        write::upsert_work_item(tx, &other)?;
        write::upsert_work_item(tx, &wi(1, 1, "Active"))
    })
    .unwrap();
    let ids = |pr| -> Vec<i64> { s.pr_work_items(pr).unwrap().iter().map(|w| w.id).collect() };
    assert_eq!(ids(7), [1, 3]);
    assert_eq!(ids(8), [2]);
    // A newer revision without the link (unlinked) drops it.
    let mut unlinked = wi(3, 2, "Active");
    unlinked.linked_pr_ids.clear();
    s.write(|tx| write::upsert_work_item(tx, &unlinked))
        .unwrap();
    assert_eq!(ids(7), [1]);
    // Deleted items aren't linked anymore.
    s.write(|tx| write::delete_work_item(tx, 1, 5)).unwrap();
    assert!(ids(7).is_empty());
}

#[test]
fn migrations_are_valid() {
    MIGRATIONS.validate().unwrap();
}

#[test]
fn reopen_is_noop_and_newer_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adoken.sqlite");
    let s = Store::open(&path).unwrap();
    seed_org(&s);
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.me().unwrap().unwrap().id, "me");
    s.write(|tx| {
        tx.pragma_update(None, "user_version", SCHEMA_VERSION + 1)?;
        Ok(())
    })
    .unwrap();
    drop(s);
    match Store::open(&path) {
        Err(Error::SchemaTooNew { found, .. }) => assert_eq!(found, SCHEMA_VERSION + 1),
        other => panic!("expected SchemaTooNew, got {other:?}"),
    }
}

#[test]
fn person_image_url_survives_payloads_without_one() {
    let (_d, s) = open();
    let url = "https://dev.azure.com/contoso/_api/_common/identityImage?id=bo";
    let mut bo = person("bo", "Bo Other");
    bo.image_url = Some(url.into());
    assert_eq!(
        s.write(|tx| write::upsert_person(tx, &bo, false)).unwrap(),
        write::Upsert::Inserted
    );
    // connectionData and identity strings carry no image URL.
    let bare = person("bo", "Bo Other");
    assert_eq!(
        s.write(|tx| write::upsert_person(tx, &bare, false))
            .unwrap(),
        write::Upsert::Unchanged
    );
    assert_eq!(
        s.person("bo").unwrap().unwrap().image_url.as_deref(),
        Some(url)
    );
    let mut moved = bo.clone();
    moved.image_url = Some(format!("{url}&v=2"));
    assert_eq!(
        s.write(|tx| write::upsert_person(tx, &moved, false))
            .unwrap(),
        write::Upsert::Updated
    );
    assert_eq!(s.people().unwrap(), vec![moved]);
}

#[test]
fn pr_upsert_is_idempotent_and_detects_list_changes() {
    let (_d, s) = open();
    seed_org(&s);
    let mut p = pr(7);
    assert_eq!(
        s.write(|tx| write::upsert_pr(tx, &p)).unwrap(),
        write::Upsert::Inserted
    );
    assert_eq!(
        s.write(|tx| write::upsert_pr(tx, &p)).unwrap(),
        write::Upsert::Unchanged
    );
    p.reviewers[0].vote = Vote::Approved;
    assert_eq!(
        s.write(|tx| write::upsert_pr(tx, &p)).unwrap(),
        write::Upsert::Updated
    );
    let got = s.pr(7).unwrap().unwrap().pr;
    assert_eq!(got, p);
    assert_eq!(
        s.prs(&PrFilter {
            reviewer: Some("me".into()),
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    assert!(
        s.prs(&PrFilter {
            author: Some("me".into()),
            ..Default::default()
        })
        .unwrap()
        .is_empty()
    );
}

#[test]
fn hydrated_description_survives_truncated_list_payload() {
    let (_d, s) = open();
    seed_org(&s);
    let full = "x".repeat(900);
    let mut p = pr(7);
    p.description = full.clone();
    s.write(|tx| write::upsert_pr(tx, &p)).unwrap();
    p.description = full[..400].to_owned();
    p.merge_status = MergeStatus::Conflicts;
    s.write(|tx| write::upsert_pr(tx, &p)).unwrap();
    assert_eq!(s.pr(7).unwrap().unwrap().pr.description, full);
}

#[test]
fn work_item_upserts_only_move_forward() {
    let (_d, s) = open();
    seed_org(&s);
    s.write(|tx| write::upsert_work_item(tx, &wi(1, 3, "Active")))
        .unwrap();
    assert_eq!(
        s.write(|tx| write::upsert_work_item(tx, &wi(1, 2, "New")))
            .unwrap(),
        write::Upsert::Unchanged
    );
    assert_eq!(
        s.write(|tx| write::upsert_work_item(tx, &wi(1, 3, "New")))
            .unwrap(),
        write::Upsert::Unchanged
    );
    assert_eq!(s.work_item(1).unwrap().unwrap().state, "Active");
    s.write(|tx| write::upsert_work_item(tx, &wi(1, 4, "Closed")))
        .unwrap();
    let got = s.work_item(1).unwrap().unwrap();
    assert_eq!(got, wi(1, 4, "Closed"));
    assert!(
        s.work_items(&WorkItemFilter {
            assigned_to: Some("me".into()),
            open_only: true,
            ..Default::default()
        })
        .unwrap()
        .is_empty()
    );
}

#[test]
fn threads_report_new_comments_once_and_extract_mentions() {
    let (_d, s) = open();
    seed_org(&s);
    s.write(|tx| write::upsert_pr(tx, &pr(7))).unwrap();
    let mut thread = PrThread {
        id: 1,
        pr_id: 7,
        status: ThreadStatus::Active,
        anchor: Some(ThreadAnchor {
            file_path: "/src/lib.rs".into(),
            right: Some(LineRange { start: 3, end: 4 }),
            left: None,
            iteration_blob: Some("blob1".into()),
        }),
        iteration: Some(1),
        comments: vec![Comment {
            id: 1,
            parent_id: None,
            author: "bo".into(),
            body: "@<me> can you check the retry budget here?".into(),
            created: 5_000,
            updated: 5_000,
            is_system: false,
            is_deleted: false,
        }],
        updated: 5_000,
    };
    assert_eq!(
        s.write(|tx| write::upsert_thread(tx, &thread)).unwrap(),
        vec![1]
    );
    assert!(
        s.write(|tx| write::upsert_thread(tx, &thread))
            .unwrap()
            .is_empty()
    );
    thread.comments.push(Comment {
        id: 2,
        parent_id: Some(1),
        author: "me".into(),
        body: "Looks right to me.".into(),
        created: 6_000,
        updated: 6_000,
        is_system: false,
        is_deleted: false,
    });
    thread.status = ThreadStatus::Fixed;
    thread.updated = 6_000;
    assert_eq!(
        s.write(|tx| write::upsert_thread(tx, &thread)).unwrap(),
        vec![2]
    );
    let detail = s.pr(7).unwrap().unwrap();
    assert_eq!(detail.threads, vec![thread]);

    let mentions = s.mentions(&"me".to_owned(), 10).unwrap();
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].who, "bo");
    assert_eq!(
        mentions[0].snippet,
        "@Ada Me can you check the retry budget here?"
    );
    assert_eq!(
        mentions[0].source,
        MentionSource::Pr {
            pr_id: 7,
            thread_id: 1,
            comment_id: 1
        }
    );
}

#[test]
fn events_dedupe_on_kind_subject_key() {
    let (_d, s) = open();
    let e = SyncEvent {
        id: 0,
        kind: SyncEventKind::MentionedInPr,
        subject: Subject::Pr(7),
        dedupe_key: "c:1".into(),
        actor: Some("bo".into()),
        self_caused: false,
        at: 5,
        summary: "Bo mentioned you".into(),
    };
    let first = s.write(|tx| write::insert_event(tx, &e)).unwrap();
    assert!(first.is_some());
    assert_eq!(s.write(|tx| write::insert_event(tx, &e)).unwrap(), None);
    let got = s.sync_events_since(0, 10).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].id, first.unwrap());
    assert_eq!(
        SyncEvent {
            id: 0,
            ..got[0].clone()
        },
        e
    );
    assert!(s.sync_events_since(got[0].id, 10).unwrap().is_empty());
}

#[test]
fn failed_write_rolls_back_everything() {
    let (_d, s) = open();
    seed_org(&s);
    let err = s.write(|tx| {
        write::upsert_pr(tx, &pr(7))?;
        write::put_cursor(
            tx,
            &Cursor {
                scope: "pr.active".into(),
                project_id: "p1".into(),
                baseline_done: true,
                ..Default::default()
            },
        )?;
        Err::<(), _>(Error::Corrupt("boom".into()))
    });
    assert!(err.is_err());
    assert!(s.pr(7).unwrap().is_none());
    assert!(s.cursor("pr.active", "p1").unwrap().is_none());
}

#[test]
fn ext_migrations_apply_forward_only() {
    let (_d, s) = open();
    let v1 = "CREATE TABLE notify_state (id INTEGER PRIMARY KEY, read INTEGER NOT NULL) STRICT;";
    let v2 = "ALTER TABLE notify_state ADD COLUMN snoozed_ms INTEGER;";
    s.migrate_ext("notify", &[v1]).unwrap();
    s.write(|tx| {
        tx.execute("INSERT INTO notify_state(id, read) VALUES (1, 1)", [])?;
        Ok(())
    })
    .unwrap();
    // Re-running is a no-op; adding a step applies only the new one.
    s.migrate_ext("notify", &[v1]).unwrap();
    s.migrate_ext("notify", &[v1, v2]).unwrap();
    let n: i64 = s
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM notify_state WHERE snoozed_ms IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(n, 1);
    assert!(matches!(
        s.migrate_ext("notify", &[v1]),
        Err(Error::SchemaTooNew { .. })
    ));
}

#[test]
fn readers_are_read_only_and_not_blocked_by_writer() {
    let (_d, s) = open();
    seed_org(&s);
    assert!(
        s.with_conn(|c| Ok(c.execute("DELETE FROM identity", [])?))
            .is_err()
    );
    let (tx_started, started) = std::sync::mpsc::channel();
    let (tx_release, release) = std::sync::mpsc::channel::<()>();
    let writer = {
        let s = s.clone();
        std::thread::spawn(move || {
            s.write(|tx| {
                write::upsert_pr(tx, &pr(7))?;
                tx_started.send(()).unwrap();
                release.recv().unwrap();
                Ok(())
            })
            .unwrap()
        })
    };
    started.recv().unwrap();
    // WAL: the reader sees the last committed state while the writer holds its tx.
    assert!(s.pr(7).unwrap().is_none());
    assert_eq!(s.people().unwrap().len(), 2);
    tx_release.send(()).unwrap();
    writer.join().unwrap();
    assert!(s.pr(7).unwrap().is_some());
}

#[test]
fn search_corpus_tracks_entities() {
    let (_d, s) = open();
    seed_org(&s);
    let rx = s.subscribe();
    s.write(|tx| {
        write::upsert_pr(tx, &pr(7))?;
        write::upsert_work_item(tx, &wi(42, 1, "Active"))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(rx.try_recv().unwrap(), s.generation());
    let docs = s.search_docs().unwrap();
    let pr_doc = docs
        .iter()
        .find(|d| d.kind == DocKind::PullRequest)
        .unwrap();
    assert_eq!(pr_doc.id, 7);
    assert_eq!(
        pr_doc.subtitle,
        "payments-api · Bo Other · active · fix/checkout"
    );
    assert!(
        docs.iter()
            .any(|d| d.kind == DocKind::WorkItem && d.id == 42)
    );
    let repo_doc = docs.iter().find(|d| d.kind == DocKind::Repo).unwrap();
    assert_eq!(s.entity_key(repo_doc).unwrap().as_deref(), Some("repo:r1"));
    // Renaming updates the doc instead of duplicating it.
    let mut p = pr(7);
    p.title = "Harden checkout retries".into();
    s.write(|tx| write::upsert_pr(tx, &p)).unwrap();
    let hits = s.search_text("hard retr", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entity_key, "pr:7");
    assert!(
        s.search_text("flaky", 5)
            .unwrap()
            .iter()
            .all(|h| h.entity_key != "pr:7")
    );
}

#[test]
fn mention_tokens_parse() {
    assert_eq!(
        read::mention_ids("hi @<a-1> and @<b2>, again @<a-1> @<bad id> @<"),
        vec!["a-1", "b2"]
    );
}

/// A v0.0.3 store (schema 1, the app's `pipelines` ext table with demo
/// stages, one of my items with a stored description and comments) upgrades
/// in place: stages kept and now core-owned, my discussion queued, and the
/// next `wi.hot` pass refetches my items so descriptions are re-rendered.
#[test]
fn schema_2_upgrade_keeps_stages_and_queues_rerender() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adoken.sqlite");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../migrations/0001_init.sql"))
            .unwrap();
        c.execute_batch(
            "PRAGMA user_version = 1;
             CREATE TABLE pipelines_stage (run_id INTEGER NOT NULL, ord INTEGER NOT NULL,
               name TEXT NOT NULL, outcome TEXT NOT NULL, started_ms INTEGER, finished_ms INTEGER,
               PRIMARY KEY (run_id, ord)) STRICT, WITHOUT ROWID;
             INSERT INTO pipelines_stage VALUES (7, 0, 'Build', 'succeeded', 1, 2);
             INSERT INTO ext_migration VALUES ('pipelines', 1);
             INSERT INTO identity(id, display_name, is_me) VALUES ('me', 'Ada Me', 1), ('bo', 'Bo', 0);
             INSERT INTO project(id, name) VALUES ('p1', 'Payments');
             INSERT INTO work_item(id, project_id, rev, type, title, state, state_category,
               assigned_to_id, created_by_id, changed_by_id, area_path, iteration_path,
               description, comment_count, created_ms, changed_ms)
             VALUES (1, 'p1', 9, 'Bug', 'Mine', 'Active', 'InProgress', 'me', 'bo', 'bo', 'A', 'I',
                     'text', 2, 1, 2),
                    (2, 'p1', 4, 'Bug', 'Theirs', 'Active', 'InProgress', 'bo', 'bo', 'bo', 'A', 'I',
                     NULL, 3, 1, 2);
             INSERT INTO sync_cursor(scope, project_id, high_water, baseline_done)
             VALUES ('wi.hot', '', '2026-09-25T09:30:00Z', 1);",
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(
        s.run_stages(7).unwrap(),
        [RunStage {
            name: "Build".into(),
            outcome: StageOutcome::Succeeded,
            started: Some(1),
            finished: Some(2),
        }]
    );
    s.with_conn(|c| {
        let ns: i64 = c.query_row(
            "SELECT count(*) FROM ext_migration WHERE ns = 'pipelines'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(ns, 0, "the app no longer owns the table");
        assert_eq!(
            read::work_items_comments_due(c, 10)?,
            [(1, "p1".to_owned())]
        );
        assert_eq!(
            read::work_item_rev(c, 1)?,
            Some(0),
            "refetch re-renders the description"
        );
        assert_eq!(read::work_item_rev(c, 2)?, Some(4));
        Ok(())
    })
    .unwrap();
    assert_eq!(s.cursor("wi.hot", "").unwrap().unwrap().high_water, None);
}

/// 0006 moves blobs out of SQLite: a store seeded by an older build keeps
/// its file contents (now in the file cache) and loses the table.
#[test]
fn schema_6_upgrade_moves_blobs_to_the_file_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adoken.sqlite");
    let id = "ab".repeat(20);
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../migrations/0001_init.sql"))
            .unwrap();
        c.execute_batch("PRAGMA user_version = 1;").unwrap();
        c.execute(
            "INSERT INTO blob(id, content) VALUES (?1, 'fn main() {}\n'), ('not-hex', 'x')",
            [&id],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.blob(&id).unwrap().as_deref(), Some("fn main() {}\n"));
    assert!(dir.path().join("blobs").join(&id[..2]).join(&id).is_file());
    let tables: i64 = s
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'blob'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(tables, 0);
}

/// Stages count as current only for the run state they were fetched in:
/// running runs stay due, finished ones leave the queue until they change.
#[test]
fn timeline_queue_follows_run_state() {
    let (_d, s) = open();
    seed_org(&s);
    let mut run = PipelineRun {
        id: 5,
        project_id: "p1".into(),
        pipeline_id: 1,
        pipeline_name: "CI".into(),
        run_number: "1".into(),
        status: RunStatus::InProgress,
        result: None,
        reason: RunReason::Manual,
        branch: "refs/heads/main".into(),
        source_version: None,
        requested_for: Some("me".into()),
        pr_id: None,
        queued: 10,
        started: Some(20),
        finish: None,
    };
    let stage = |outcome| RunStage {
        name: "Build".into(),
        outcome,
        started: Some(20),
        finished: None,
    };
    let due = |s: &Store| {
        s.with_conn(|c| read::runs_timeline_due(c, 10))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    s.write(|tx| write::upsert_run(tx, &run).map(|_| ()))
        .unwrap();
    assert_eq!(due(&s), [5], "never fetched");
    s.write(|tx| write::put_run_stages(tx, &run, &[stage(StageOutcome::Running)]))
        .unwrap();
    assert_eq!(due(&s), [5], "running runs stay due");

    let seen_running = run.clone();
    run.status = RunStatus::Completed;
    run.result = Some(RunResult::Succeeded);
    run.finish = Some(90);
    s.write(|tx| write::upsert_run(tx, &run).map(|_| ()))
        .unwrap();
    // A timeline fetched while it still ran doesn't settle the finished run.
    s.write(|tx| write::put_run_stages(tx, &seen_running, &[stage(StageOutcome::Running)]))
        .unwrap();
    assert_eq!(due(&s), [5]);
    s.write(|tx| write::put_run_stages(tx, &run, &[stage(StageOutcome::Succeeded)]))
        .unwrap();
    assert!(due(&s).is_empty());
    assert_eq!(s.run_stages(5).unwrap()[0].outcome, StageOutcome::Succeeded);
}

#[test]
fn auto_complete_guard_rearms_and_survives_reopen() {
    let (dir, s) = open();
    s.write(|tx| write::set_auto_complete(tx, 912, "aaa", 10))
        .unwrap();
    s.write(|tx| write::set_auto_complete(tx, 912, "bbb", 20))
        .unwrap();
    drop(s);
    let s = Store::open(dir.path().join("org/adoken.sqlite")).unwrap();
    assert_eq!(s.auto_complete(912).unwrap(), Some(("bbb".into(), 20)));
    assert_eq!(s.auto_complete(1).unwrap(), None);
    s.write(|tx| write::clear_auto_complete(tx, 912)).unwrap();
    assert_eq!(s.auto_complete(912).unwrap(), None);
}

#[test]
fn file_change_tracking_ids_round_trip_and_unknown_stays_unknown() {
    let (_dir, s) = open();
    seed_org(&s);
    let file = |path: &str, tracking| PrFileChange {
        path: path.into(),
        original_path: None,
        change_type: ChangeType::Edit,
        old_content: Some("o".into()),
        new_content: Some("n".into()),
        change_tracking_id: tracking,
    };
    let files = vec![file("/a.rs", Some(3)), file("/b.rs", None)];
    s.write(|tx| {
        write::upsert_pr(tx, &pr(912))?;
        write::put_pr_files(tx, 912, &files)
    })
    .unwrap();
    assert_eq!(s.pr(912).unwrap().unwrap().files, files);
}

#[test]
fn completing_rearms_survives_reopen_and_clears() {
    let (dir, s) = open();
    s.write(|tx| write::set_completing(tx, 912, "aaa", 10))
        .unwrap();
    s.write(|tx| write::set_completing(tx, 912, "bbb", 20))
        .unwrap();
    drop(s);
    let s = Store::open(dir.path().join("org/adoken.sqlite")).unwrap();
    assert_eq!(s.completing(912).unwrap(), Some(("bbb".into(), 20)));
    assert_eq!(s.completing(1).unwrap(), None);
    s.write(|tx| write::clear_completing(tx, 912)).unwrap();
    assert_eq!(s.completing(912).unwrap(), None);
}

/// PR list comment badge (spec 0004 §3.1): only active threads with a live
/// human comment count; resolved, system-only and deleted ones don't.
#[test]
fn active_thread_counts_skip_resolved_system_and_deleted() {
    let (_d, s) = open();
    seed_org(&s);
    let comment = |id, is_system, is_deleted| Comment {
        id,
        parent_id: None,
        author: "bo".into(),
        body: "text".into(),
        created: 5_000,
        updated: 5_000,
        is_system,
        is_deleted,
    };
    let thread = |pr_id, id, status, comments| PrThread {
        id,
        pr_id,
        status,
        anchor: None,
        iteration: None,
        comments,
        updated: 5_000,
    };
    s.write(|tx| {
        write::upsert_pr(tx, &pr(7))?;
        write::upsert_pr(tx, &pr(8))?;
        for t in [
            thread(7, 1, ThreadStatus::Active, vec![comment(1, false, false)]),
            thread(
                7,
                2,
                ThreadStatus::Active,
                vec![comment(1, true, false), comment(2, false, false)],
            ),
            thread(7, 3, ThreadStatus::Fixed, vec![comment(1, false, false)]),
            thread(7, 4, ThreadStatus::Active, vec![comment(1, true, false)]),
            thread(8, 1, ThreadStatus::Active, vec![comment(1, false, true)]),
        ] {
            write::upsert_thread(tx, &t)?;
        }
        Ok(())
    })
    .unwrap();
    let counts = s.active_thread_counts().unwrap();
    assert_eq!(counts.get(&7), Some(&2));
    assert_eq!(counts.get(&8), None, "a deleted comment isn't discussion");
}

/// PR list CI badge: the newest run per pipeline per PR, not older reruns.
#[test]
fn latest_pr_runs_keep_the_newest_run_per_pipeline() {
    let (_d, s) = open();
    seed_org(&s);
    let run = |id, pipeline_id, pr_id, queued, result| PipelineRun {
        id,
        project_id: "p1".into(),
        pipeline_id,
        pipeline_name: "CI".into(),
        run_number: id.to_string(),
        status: RunStatus::Completed,
        result: Some(result),
        reason: RunReason::PullRequest,
        branch: "refs/pull/7/merge".into(),
        source_version: None,
        requested_for: Some("me".into()),
        pr_id,
        queued,
        started: None,
        finish: None,
    };
    s.write(|tx| {
        for r in [
            run(1, 1, Some(7), 10, RunResult::Failed),
            run(2, 1, Some(7), 20, RunResult::Succeeded),
            run(3, 2, Some(7), 15, RunResult::Failed),
            run(4, 1, Some(8), 30, RunResult::Succeeded),
            run(5, 1, None, 40, RunResult::Failed),
        ] {
            write::upsert_run(tx, &r)?;
        }
        Ok(())
    })
    .unwrap();
    let mut ids: Vec<i64> = s
        .latest_pr_runs()
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [2, 3, 4]);
}

fn pin(subject: PinSubject, title: &str) -> Pin {
    Pin {
        subject,
        position: 0,
        label: None,
        title: title.into(),
        pinned_at: 100,
        opened_at: None,
        seen_result: None,
        unavailable: None,
    }
}

fn pin_keys(s: &Store) -> Vec<String> {
    s.pins()
        .unwrap()
        .iter()
        .map(|p| format!("{}@{}", p.subject.key(), p.position))
        .collect()
}

#[test]
fn pins_append_cap_at_twenty_and_ignore_duplicates() {
    let (_d, s) = open();
    let outcome = |subject| {
        s.write(|tx| write::add_pin(tx, &pin(subject, "t")))
            .unwrap()
    };
    for id in 1..=MAX_PINS as i64 {
        assert_eq!(outcome(PinSubject::Pr(id)), write::PinOutcome::Added);
    }
    assert_eq!(outcome(PinSubject::Pr(1)), write::PinOutcome::AlreadyPinned);
    // AC-10: the 21st changes nothing.
    assert_eq!(outcome(PinSubject::WorkItem(9)), write::PinOutcome::Full);
    let pins = s.pins().unwrap();
    assert_eq!(pins.len(), MAX_PINS);
    assert_eq!(pins.last().unwrap().subject, PinSubject::Pr(20));
    assert!(
        pins.iter()
            .enumerate()
            .all(|(ix, p)| p.position == ix as u32)
    );
}

#[test]
fn unpin_then_restore_keeps_position_and_label() {
    let (_d, s) = open();
    s.write(|tx| {
        for id in 1..=3 {
            write::add_pin(tx, &pin(PinSubject::Pr(id), "t"))?;
        }
        write::rename_pin(tx, &PinSubject::Pr(2), "  Payments CI  ")?;
        Ok(())
    })
    .unwrap();
    let removed = s
        .write(|tx| write::unpin(tx, &PinSubject::Pr(2)))
        .unwrap()
        .expect("was pinned");
    assert_eq!(removed.label.as_deref(), Some("Payments CI"));
    assert_eq!(pin_keys(&s), ["pr:1@0", "pr:3@1"]);
    s.write(|tx| write::restore_pin(tx, &removed)).unwrap();
    assert_eq!(pin_keys(&s), ["pr:1@0", "pr:2@1", "pr:3@2"]);
    assert_eq!(s.pins().unwrap()[1].label.as_deref(), Some("Payments CI"));
}

#[test]
fn moving_a_pin_shifts_the_ones_between() {
    let (_d, s) = open();
    s.write(|tx| {
        for id in 1..=4 {
            write::add_pin(tx, &pin(PinSubject::Pr(id), "t"))?;
        }
        Ok(())
    })
    .unwrap();
    let to = |id, pos| {
        s.write(|tx| write::move_pin(tx, &PinSubject::Pr(id), pos))
            .unwrap()
    };
    assert_eq!(to(3, 0), Some(0));
    assert_eq!(pin_keys(&s), ["pr:3@0", "pr:1@1", "pr:2@2", "pr:4@3"]);
    assert_eq!(to(3, 99), Some(3));
    assert_eq!(pin_keys(&s), ["pr:1@0", "pr:2@1", "pr:4@2", "pr:3@3"]);
    assert_eq!(to(9, 0), None);
}

#[test]
fn labels_are_trimmed_capped_and_empty_resets_to_the_title() {
    let (_d, s) = open();
    let def = PinSubject::Definition {
        project_id: "p1".into(),
        id: 7,
    };
    s.write(|tx| write::add_pin(tx, &pin(def.clone(), "payments-ci")))
        .unwrap();
    let label = |text: &str| {
        s.write(|tx| write::rename_pin(tx, &def, text)).unwrap();
        s.pins().unwrap()[0].clone()
    };
    assert_eq!(
        label("x".repeat(80).as_str()).label.unwrap().len(),
        MAX_PIN_LABEL_CHARS
    );
    assert_eq!(label("Payments CI").display_title(), "Payments CI");
    let reset = label("   ");
    assert_eq!(reset.label, None);
    assert_eq!(reset.display_title(), "payments-ci");
}

#[test]
fn opening_records_time_result_and_the_weekly_open_count() {
    let (_d, s) = open();
    let def = PinSubject::Definition {
        project_id: "p1".into(),
        id: 7,
    };
    s.write(|tx| write::add_pin(tx, &pin(def.clone(), "ci")))
        .unwrap();
    let day = adoken_model::time::DAY;
    s.write(|tx| write::mark_pin_opened(tx, &def, day, Some(RunResult::Failed), true))
        .unwrap();
    s.write(|tx| write::mark_pin_opened(tx, &def, 2 * day, None, false))
        .unwrap();
    let opened = s.pins().unwrap().remove(0);
    assert_eq!(opened.opened_at, Some(2 * day));
    // `None` keeps the last result seen.
    assert_eq!(opened.seen_result, Some(RunResult::Failed));
    let opens = |since| s.with_conn(|c| read::pin_opens_since(c, since)).unwrap();
    assert_eq!(opens(0), 1);
    // A from-pins open 8 days later prunes the old one.
    s.write(|tx| write::mark_pin_opened(tx, &def, 9 * day, None, true))
        .unwrap();
    assert_eq!(opens(0), 1);
}

#[test]
fn change_dot_reads_only_other_peoples_events_after_the_open() {
    let (_d, s) = open();
    let event = |dedupe: &str, at, self_caused| SyncEvent {
        id: 0,
        kind: SyncEventKind::PrCommentAdded,
        subject: Subject::Pr(4821),
        dedupe_key: dedupe.into(),
        actor: None,
        self_caused,
        at,
        summary: String::new(),
    };
    s.write(|tx| {
        write::insert_event(tx, &event("a", 10, false))?;
        write::insert_event(tx, &event("b", 30, true))
    })
    .unwrap();
    let changed = |after| {
        s.with_conn(|c| read::changed_since(c, &Subject::Pr(4821), after))
            .unwrap()
    };
    assert!(changed(5));
    assert!(!changed(10));
    assert!(!changed(20), "my own event doesn't light the dot");
}

/// AC-3: pins, order, labels and open state survive a restart, and a
/// migration that drops and rebuilds the cache tables (a resync) keeps
/// every pin: the `pin` table has no foreign key into them.
#[test]
fn pins_survive_restart_and_a_cache_rebuilding_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adoken.sqlite");
    {
        let s = Store::open(&path).unwrap();
        seed_org(&s);
        s.write(|tx| {
            write::upsert_pr(tx, &pr(7))?;
            write::upsert_work_item(tx, &wi(9, 1, "Active"))?;
            write::add_pin(tx, &pin(PinSubject::Pr(7), "Fix flaky checkout retry 7"))?;
            write::add_pin(tx, &pin(PinSubject::WorkItem(9), "Checkout times out"))?;
            write::rename_pin(tx, &PinSubject::WorkItem(9), "Timeouts")?;
            write::move_pin(tx, &PinSubject::WorkItem(9), 0)?;
            let definition = PinSubject::Definition {
                project_id: "p1".into(),
                id: 7,
            };
            write::add_pin(tx, &pin(definition.clone(), "Payments CI"))?;
            write::mark_pin_opened(tx, &definition, 42, Some(RunResult::Failed), true)?;
            write::set_pin_unavailable(tx, &PinSubject::Pr(7), Some(PinUnavailable::Forbidden))?;
            Ok(())
        })
        .unwrap();
    }
    let before = Store::open(&path).unwrap().pins().unwrap();
    assert_eq!(before.len(), 3);
    assert_eq!(before[0].label.as_deref(), Some("Timeouts"));

    // A future resync migration wipes cache tables without touching local state.
    let mut conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    let resync: Vec<M<'static>> = MIGRATION_SLICE
        .iter()
        .cloned()
        .chain([M::up(
            "DELETE FROM project;
             DROP TABLE work_item;
             DROP TABLE pipeline_run;
             DELETE FROM sync_cursor;",
        )])
        .collect();
    Migrations::new(resync).to_latest(&mut conn).unwrap();
    let gone: i64 = conn
        .query_row("SELECT count(*) FROM pull_request", [], |r| r.get(0))
        .unwrap();
    assert_eq!(gone, 0, "the cascade really emptied the cache");
    assert_eq!(read::pins(&conn).unwrap(), before);
    assert_eq!(read::pin_opens_since(&conn, 0).unwrap(), 1);
}

#[test]
fn work_item_terminal_transition_survives_edits_and_resets_on_reopen() {
    use adoken_model::time::{DAY, HOUR};
    let (_dir, s) = open();
    seed_org(&s);
    let mut item = wi(990, 1, "Active");
    item.changed = DAY;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    let closed = || {
        s.with_conn(|c| read::work_item_closed_at(c, item.id))
            .unwrap()
    };
    assert_eq!(closed(), None);
    item.rev += 1;
    item.state = "Closed".into();
    item.state_category = StateCategory::Completed;
    item.changed = 2 * DAY;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(closed(), Some(2 * DAY));
    item.rev += 1;
    item.title = "An ordinary terminal-state edit".into();
    item.changed += 23 * HOUR;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(
        closed(),
        Some(2 * DAY),
        "an edit must not postpone expiration"
    );
    // Moving between terminal states isn't a reopen.
    item.rev += 1;
    item.state = "Removed".into();
    item.state_category = StateCategory::Removed;
    item.changed = 3 * DAY;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(closed(), Some(2 * DAY));
    item.rev += 1;
    item.state = "Active".into();
    item.state_category = StateCategory::InProgress;
    item.changed = 4 * DAY;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(closed(), None);
    item.rev += 1;
    item.state = "Closed".into();
    item.state_category = StateCategory::Completed;
    item.changed = 5 * DAY;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(closed(), Some(5 * DAY));
}

#[test]
fn first_observed_closure_uses_wire_date_but_later_edits_cannot_rewrite_it() {
    let (_dir, s) = open();
    seed_org(&s);
    let mut item = wi(991, 1, "Closed");
    item.changed = 90;
    let closed_at = |at| write::Transitions {
        closed_at: Some(at),
        state_changed: None,
    };
    s.write(|tx| write::upsert_work_item_with_transitions(tx, &item, closed_at(30)))
        .unwrap();
    let closed = || {
        s.with_conn(|c| read::work_item_closed_at(c, item.id))
            .unwrap()
    };
    assert_eq!(closed(), Some(30));
    item.rev += 1;
    item.changed = 100;
    item.title = "Still closed".into();
    s.write(|tx| write::upsert_work_item_with_transitions(tx, &item, closed_at(99)))
        .unwrap();
    assert_eq!(closed(), Some(30));
    let mut stale = item.clone();
    stale.rev -= 1;
    stale.state = "Active".into();
    stale.state_category = StateCategory::InProgress;
    s.write(|tx| write::upsert_work_item(tx, &stale)).unwrap();
    assert_eq!(closed(), Some(30), "stale revisions cannot reopen the item");
}

/// Spec 0017 "Open Nd": the wire's `StateChangeDate` wins; without one a
/// state change observed by sync starts at `changed`, other edits keep it.
/// Read per assignee and state category.
#[test]
fn work_item_state_since_follows_state_changes_not_edits() {
    let (_dir, s) = open();
    seed_org(&s);
    let since = |s: &Store, category: StateCategory| {
        s.with_conn(|c| read::work_items_state_since(c, "me", category))
            .unwrap()
    };
    let mut item = wi(992, 1, "Active");
    item.changed = 500;
    let mut other = wi(990, 1, "Active");
    other.assigned_to = Some("bo".into());
    s.write(|tx| {
        write::upsert_work_item(tx, &item)?;
        write::upsert_work_item(tx, &other)
    })
    .unwrap();
    let active = StateCategory::InProgress;
    assert_eq!(
        since(&s, active),
        [(992, 500)],
        "unknown falls back to changed; only mine"
    );
    item.rev += 1;
    item.changed = 900;
    let wire = write::Transitions {
        closed_at: None,
        state_changed: Some(400),
    };
    s.write(|tx| write::upsert_work_item_with_transitions(tx, &item, wire))
        .unwrap();
    assert_eq!(since(&s, active), [(992, 400)], "StateChangeDate");
    item.rev += 1;
    item.title = "Renamed".into();
    item.changed = 1_000;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(
        since(&s, active),
        [(992, 400)],
        "an edit keeps the state's start"
    );
    item.rev += 1;
    item.state = "Resolved".into();
    item.state_category = StateCategory::of_state("Resolved");
    item.changed = 1_100;
    s.write(|tx| write::upsert_work_item(tx, &item)).unwrap();
    assert_eq!(
        since(&s, item.state_category),
        [(992, 1_100)],
        "a new state starts when observed"
    );
}

/// Spec 0017 FR-3 "done": Completed (not Removed) transitions since the
/// day began, by the first observed closure, not the last edit.
#[test]
fn completed_since_counts_closures_not_edits() {
    use adoken_model::time::DAY;
    let (_dir, s) = open();
    seed_org(&s);
    let mut done = wi(993, 1, "Closed");
    done.state_category = StateCategory::Completed;
    done.assigned_to = Some("me".into());
    done.changed = 2 * DAY + 10;
    let mut removed = done.clone();
    removed.id = 994;
    removed.state = "Removed".into();
    removed.state_category = StateCategory::Removed;
    let mut yesterday = done.clone();
    yesterday.id = 995;
    yesterday.changed = DAY;
    s.write(|tx| {
        write::upsert_work_item(tx, &done)?;
        write::upsert_work_item(tx, &removed)?;
        write::upsert_work_item(tx, &yesterday)
    })
    .unwrap();
    // Completed yesterday, edited today.
    yesterday.rev += 1;
    yesterday.title = "Edited".into();
    yesterday.changed = 2 * DAY + 20;
    s.write(|tx| write::upsert_work_item(tx, &yesterday))
        .unwrap();
    let ids = s
        .with_conn(|c| read::work_items_completed_since(c, "me", 2 * DAY))
        .unwrap();
    assert_eq!(ids, [993]);
}

/// Spec 0017 FR-4: commit counts round-trip; a later write without a count
/// keeps the known one; today's pushes come with the latest push's commit,
/// even when that push is older than today.
#[test]
fn iteration_commit_counts_round_trip_and_survive_unknown_rewrites() {
    let (_dir, s) = open();
    seed_org(&s);
    let it = |id: u32, created, count, truncated| PrIteration {
        id,
        source_commit: format!("c{id}"),
        created,
        description: None,
        commit_count: count,
        commits_truncated: truncated,
    };
    s.write(|tx| {
        write::upsert_pr(tx, &pr(7))?;
        write::put_pr_iterations(
            tx,
            7,
            &[it(1, 10, Some(3), false), it(2, 20, Some(100), true)],
        )
    })
    .unwrap();
    s.write(|tx| write::put_pr_iterations(tx, 7, &[it(1, 10, None, false)]))
        .unwrap();
    let since = |at| {
        s.with_conn(|c| read::pr_iterations_since(c, 7, at))
            .unwrap()
    };
    let (all, latest) = since(10);
    assert_eq!(all, [it(1, 10, Some(3), false), it(2, 20, Some(100), true)]);
    assert_eq!(latest.as_deref(), Some("c2"));
    let (today, latest) = since(15);
    assert_eq!(today.iter().map(|i| i.id).collect::<Vec<_>>(), [2]);
    assert_eq!(latest.as_deref(), Some("c2"));
    let (later, latest) = since(25);
    assert!(later.is_empty(), "nothing pushed since");
    assert_eq!(latest.as_deref(), Some("c2"), "the head before today");
    let (none, latest) = s.with_conn(|c| read::pr_iterations_since(c, 8, 0)).unwrap();
    assert!(none.is_empty() && latest.is_none(), "no stored pushes");
}

#[test]
fn first_pin_result_initialization_is_scoped_and_does_not_record_an_open() {
    let (_dir, s) = open();
    let a = PinSubject::Definition {
        project_id: "a".into(),
        id: 7,
    };
    let b = PinSubject::Definition {
        project_id: "b".into(),
        id: 7,
    };
    s.write(|tx| {
        write::add_pin(tx, &pin(a.clone(), "CI A"))?;
        write::add_pin(tx, &pin(b.clone(), "CI B"))?;
        assert!(write::initialize_pin_result(tx, &a, RunResult::Succeeded)?);
        assert!(!write::initialize_pin_result(tx, &a, RunResult::Failed)?);
        Ok(())
    })
    .unwrap();
    let pins = s.pins().unwrap();
    assert_eq!(pins[0].seen_result, Some(RunResult::Succeeded));
    assert_eq!(pins[1].seen_result, None);
    assert_eq!(pins[0].opened_at, None);
    assert_eq!(s.with_conn(|c| read::pin_opens_since(c, 0)).unwrap(), 0);
}
