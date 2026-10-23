//! Spec 0004 review writes against a mock transport: the exact REST call
//! (method, URL, `api-version`, JSON body), error mapping, and what lands in
//! the store (server wins) and in the event stream (notify, Ken).
//!
//! AC map: AC-7 `ac7_*`, AC-9 `ac9_*`, AC-10 `ac10_*`, AC-15 `ac15_*`.
//! AC-11 and AC-13 are UI/draft behaviour, tested in `crates/app`.

mod support;

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use adoken_demo::{DEFAULT_SEED, dataset};
use adoken_model::time::MINUTE;
use adoken_model::*;
use adoken_store::{Store, write};
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::wire::format_time;
use adoken_sync::writes::{
    CompletionOptions, LineAnchor, MergeStrategy, NO_IDENTITY, PrRef, Side, apply_local,
};
use adoken_sync::*;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use support::Harness;

const ORG: &str = "https://dev.azure.com/contoso/";
/// 2026-09-30 12:00 UTC.
const NOW: Millis = 1_790_769_600_000;

/// Records every request; answers from a script (`(status, body)` in order).
#[derive(Default)]
struct MockTransport {
    sent: Mutex<Vec<ApiRequest>>,
    script: Mutex<VecDeque<(u16, Value)>>,
}

impl MockTransport {
    fn answer(&self, status: u16, body: Value) {
        self.script.lock().push_back((status, body));
    }

    fn sent(&self) -> Vec<ApiRequest> {
        self.sent.lock().clone()
    }
}

impl Transport for MockTransport {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        self.sent.lock().push(req);
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

fn person(id: &str, name: &str) -> Person {
    Person {
        id: id.into(),
        display_name: name.into(),
        unique_name: None,
        is_group: false,
        image_url: None,
    }
}

fn pr_912() -> PullRequest {
    PullRequest {
        id: 912,
        repo_id: "r1".into(),
        project_id: "p1".into(),
        title: "Fix flaky checkout".into(),
        description: String::new(),
        author: "bo".into(),
        reviewers: vec![Reviewer {
            person: "me".into(),
            vote: Vote::NoVote,
            is_required: true,
            has_declined: false,
            added_at: Some(NOW - 60 * MINUTE),
            voted_at: None,
        }],
        status: PrStatus::Active,
        is_draft: false,
        merge_status: MergeStatus::Succeeded,
        source_branch: "refs/heads/fix/checkout".into(),
        target_branch: "refs/heads/main".into(),
        last_source_commit: Some("aaaa".into()),
        created: NOW - 120 * MINUTE,
        updated: NOW - 30 * MINUTE,
        closed: None,
        iterations: 3,
        last_push: Some(NOW - 30 * MINUTE),
    }
}

fn thread_7() -> PrThread {
    PrThread {
        id: 7,
        pr_id: 912,
        status: ThreadStatus::Active,
        anchor: Some(ThreadAnchor {
            file_path: "/src/a.rs".into(),
            right: Some(LineRange { start: 4, end: 4 }),
            left: None,
            iteration_blob: None,
        }),
        iteration: Some(3),
        comments: vec![Comment {
            id: 1,
            parent_id: None,
            author: "bo".into(),
            body: "Why retry here?".into(),
            created: NOW - 20 * MINUTE,
            updated: NOW - 20 * MINUTE,
            is_system: false,
            is_deleted: false,
        }],
        updated: NOW - 20 * MINUTE,
    }
}

struct Fixture {
    store: Store,
    mock: Arc<MockTransport>,
    writer: Writer,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        store
            .write(|tx| {
                write::upsert_person(tx, &person("me", "Ada Me"), true)?;
                write::upsert_person(tx, &person("bo", "Bo Author"), false)?;
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
                        name: "checkout-web".into(),
                        default_branch: Some("refs/heads/main".into()),
                    },
                )?;
                write::upsert_pr(tx, &pr_912())?;
                write::upsert_thread(tx, &thread_7())?;
                Ok(())
            })
            .unwrap();
        let mock = Arc::new(MockTransport::default());
        let clock = Arc::new(ManualClock::new(NOW));
        let writer = Writer::new(
            store.clone(),
            mock.clone(),
            Arc::new(Governor::new(clock.clone())),
            clock,
            url::Url::parse(ORG).unwrap(),
        );
        Fixture {
            store,
            mock,
            writer,
            _dir: dir,
        }
    }

    fn only_request(&self) -> ApiRequest {
        let sent = self.mock.sent();
        assert_eq!(sent.len(), 1, "exactly one request: {sent:#?}");
        sent.into_iter().next().unwrap()
    }

    fn events(&self) -> Vec<SyncEvent> {
        self.store.sync_events_since(0, 1000).unwrap()
    }
}

fn pr_ref() -> PrRef {
    PrRef::of(&pr_912())
}

fn comment_json(id: i64, parent: i64, author: &str, body: &str) -> Value {
    json!({
        "id": id,
        "parentCommentId": parent,
        "author": { "id": author, "displayName": "Ada Me" },
        "content": body,
        "publishedDate": format_time(NOW),
        "lastUpdatedDate": format_time(NOW),
        "commentType": "text",
    })
}

/// A PR payload as `PATCH …/pullrequests/{id}` answers it.
fn pr_json(pr: &PullRequest, status: &str, closed_by: Option<&str>) -> Value {
    let mut v = json!({
        "pullRequestId": pr.id,
        "repository": { "id": pr.repo_id, "name": "checkout-web", "project": { "id": pr.project_id, "name": "Payments" } },
        "title": pr.title,
        "status": status,
        "createdBy": { "id": pr.author, "displayName": "Bo Author" },
        "creationDate": format_time(pr.created),
        "sourceRefName": pr.source_branch,
        "targetRefName": pr.target_branch,
        "mergeStatus": "succeeded",
        "lastMergeSourceCommit": { "commitId": pr.last_source_commit },
        "reviewers": pr.reviewers.iter().map(|r| json!({
            "id": r.person, "displayName": r.person, "vote": r.vote.as_i8(), "isRequired": r.is_required,
        })).collect::<Vec<_>>(),
    });
    if status == "completed" {
        v["closedDate"] = json!(format_time(NOW));
    }
    if let Some(who) = closed_by {
        v["closedBy"] = json!({ "id": who, "displayName": who });
    }
    v
}

fn assert_call(req: &ApiRequest, method: Method, url: &str, body: Value) {
    assert_eq!(req.method(), &method);
    assert_eq!(req.url().as_str(), url);
    assert_eq!(req.body(), Some(&body));
}

const PR_URL: &str = "https://dev.azure.com/contoso/p1/_apis/git/repositories/r1/pullRequests/912";

#[tokio::test]
async fn ac7_line_comment_round_trips_with_the_displayed_iteration() {
    let f = Fixture::new();
    f.mock.answer(
        200,
        json!({
            "id": 55,
            "status": "active",
            "threadContext": {
                "filePath": "/src/a.rs",
                "rightFileStart": { "line": 12, "offset": 1 },
                "rightFileEnd": { "line": 12, "offset": 1 },
            },
            "pullRequestThreadContext": {
                "iterationContext": { "firstComparingIteration": 1, "secondComparingIteration": 3 },
                "changeTrackingId": 4,
            },
            "comments": [comment_json(1, 0, "me", "Off by one?")],
            "lastUpdatedDate": format_time(NOW),
        }),
    );
    let applied = f
        .writer
        .send(ReviewWrite::CreateThread {
            pr: pr_ref(),
            // The tree shows normalized paths; the leading `/` is restored.
            anchor: Some(LineAnchor {
                path: "src/a.rs".into(),
                side: Side::Right,
                line: 12,
                change_tracking_id: Some(4),
            }),
            iteration: 3,
            body: "Off by one?".into(),
        })
        .await
        .unwrap();

    assert_call(
        &f.only_request(),
        Method::Post,
        &format!("{PR_URL}/threads?api-version=7.1"),
        json!({
            "comments": [{ "parentCommentId": 0, "content": "Off by one?", "commentType": "text" }],
            "status": "active",
            "threadContext": {
                "filePath": "/src/a.rs",
                "rightFileStart": { "line": 12, "offset": 1 },
                "rightFileEnd": { "line": 12, "offset": 1 },
            },
            // The file's `changeTrackingId`, as the web UI sends it: without
            // it Azure DevOps never tracks the thread across later pushes.
            "pullRequestThreadContext": {
                "iterationContext": { "firstComparingIteration": 1, "secondComparingIteration": 3 },
                "changeTrackingId": 4,
            },
        }),
    );
    // Server wins: the stored thread is the one the server created.
    let thread = applied.into_thread().unwrap();
    assert_eq!(thread.id, 55);
    assert_eq!(thread.iteration, Some(3));
    let anchor = thread.anchor.unwrap();
    assert_eq!(
        (anchor.file_path.as_str(), anchor.right),
        ("/src/a.rs", Some(LineRange { start: 12, end: 12 }))
    );
    let stored = f.store.pr(912).unwrap().unwrap();
    assert!(stored.threads.iter().any(|t| t.id == 55));
    let events = f.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, SyncEventKind::PrCommentAdded);
    assert_eq!(events[0].dedupe_key, "55:1");
    assert!(events[0].self_caused);
}

#[tokio::test]
async fn pr_level_comment_has_no_thread_context() {
    let f = Fixture::new();
    let req = ReviewWrite::CreateThread {
        pr: pr_ref(),
        anchor: None,
        iteration: 3,
        body: "LGTM overall".into(),
    }
    .request(&url::Url::parse(ORG).unwrap());
    assert_call(
        &req,
        Method::Post,
        &format!("{PR_URL}/threads?api-version=7.1"),
        json!({
            "comments": [{ "parentCommentId": 0, "content": "LGTM overall", "commentType": "text" }],
            "status": "active",
        }),
    );
    drop(f);
}

#[test]
fn unknown_change_tracking_id_is_omitted_not_zero() {
    let req = ReviewWrite::CreateThread {
        pr: pr_ref(),
        anchor: Some(LineAnchor {
            path: "/src/a.rs".into(),
            side: Side::Left,
            line: 3,
            change_tracking_id: None,
        }),
        iteration: 2,
        body: "Why was this removed?".into(),
    }
    .request(&url::Url::parse(ORG).unwrap());
    let body = req.body().unwrap();
    assert_eq!(
        body["pullRequestThreadContext"],
        json!({ "iterationContext": { "firstComparingIteration": 1, "secondComparingIteration": 2 } })
    );
    assert_eq!(
        body["threadContext"]["leftFileStart"],
        json!({ "line": 3, "offset": 1 })
    );
}

#[tokio::test]
async fn reply_and_resolve_send_the_documented_calls_and_update_the_thread() {
    let f = Fixture::new();
    f.mock
        .answer(200, comment_json(2, 1, "me", "Retries are idempotent."));
    f.writer
        .send(ReviewWrite::Reply {
            pr: pr_ref(),
            thread_id: 7,
            parent_comment_id: 1,
            body: "Retries are idempotent.".into(),
        })
        .await
        .unwrap();
    f.mock.answer(
        200,
        json!({ "id": 7, "status": "fixed", "lastUpdatedDate": format_time(NOW) }),
    );
    let resolved = f
        .writer
        .send(ReviewWrite::SetThreadStatus {
            pr: pr_ref(),
            thread_id: 7,
            status: ThreadStatus::Fixed,
        })
        .await
        .unwrap()
        .into_thread()
        .unwrap();

    let sent = f.mock.sent();
    assert_call(
        &sent[0],
        Method::Post,
        &format!("{PR_URL}/threads/7/comments?api-version=7.1"),
        json!({ "parentCommentId": 1, "content": "Retries are idempotent.", "commentType": "text" }),
    );
    assert_call(
        &sent[1],
        Method::Patch,
        &format!("{PR_URL}/threads/7?api-version=7.1"),
        json!({ "status": "fixed" }),
    );
    // The PATCH answer carried no comments: both stay.
    assert_eq!(resolved.status, ThreadStatus::Fixed);
    assert_eq!(resolved.comments.len(), 2);
    let kinds: Vec<(SyncEventKind, String, bool)> = f
        .events()
        .into_iter()
        .map(|e| (e.kind, e.dedupe_key, e.self_caused))
        .collect();
    assert_eq!(
        kinds,
        [
            (SyncEventKind::PrCommentAdded, "7:2".into(), true),
            (SyncEventKind::PrThreadResolved, "7:fixed".into(), true),
        ]
    );
}

#[tokio::test]
async fn ac9_approve_puts_vote_10_for_me_and_updates_the_store() {
    let f = Fixture::new();
    f.mock.answer(
        200,
        json!({ "id": "me", "displayName": "Ada Me", "vote": 10, "isRequired": true }),
    );
    let pr = f
        .writer
        .send(ReviewWrite::Vote {
            pr: pr_ref(),
            reviewer: "me".into(),
            vote: Vote::Approved,
        })
        .await
        .unwrap()
        .into_pr()
        .unwrap();
    assert_call(
        &f.only_request(),
        Method::Put,
        &format!("{PR_URL}/reviewers/me?api-version=7.1"),
        json!({ "vote": 10 }),
    );
    assert_eq!(pr.vote_of("me"), Some(Vote::Approved));
    let events = f.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, SyncEventKind::PrVoteChanged);
    // Sync's own key, so its next pass dedupes against this event.
    assert_eq!(events[0].dedupe_key, "me:10:aaaa");
    assert!(events[0].self_caused);
}

#[tokio::test]
async fn ac9_vote_403_names_code_write_and_leaves_the_store_unchanged() {
    let f = Fixture::new();
    f.mock.answer(
        403,
        json!({ "message": "TF401027: You need the Git 'Contribute' permission." }),
    );
    let error = f
        .writer
        .send(ReviewWrite::Vote {
            pr: pr_ref(),
            reviewer: "me".into(),
            vote: Vote::Approved,
        })
        .await
        .unwrap_err();
    assert_eq!(error, WriteError::MissingScope);
    assert_eq!(error.missing_scope(), Some("vso.code_write"));
    assert_eq!(
        f.store.pr(912).unwrap().unwrap().pr.vote_of("me"),
        Some(Vote::NoVote)
    );
    assert!(f.events().is_empty());
    assert_eq!(f.mock.sent().len(), 1, "no retry");
}

#[tokio::test]
async fn every_vote_value_uses_ados_scale() {
    let url = url::Url::parse(ORG).unwrap();
    for (vote, value) in [
        (Vote::Approved, 10),
        (Vote::ApprovedWithSuggestions, 5),
        (Vote::NoVote, 0),
        (Vote::WaitingForAuthor, -5),
        (Vote::Rejected, -10),
    ] {
        let req = ReviewWrite::Vote {
            pr: pr_ref(),
            reviewer: "me".into(),
            vote,
        }
        .request(&url);
        assert_eq!(req.body(), Some(&json!({ "vote": value })));
    }
}

/// The real 2xx answer to Complete (seen live 2026-09-30): Azure DevOps
/// queues the merge and completes 1-2 s later.
fn queued_json(pr: &PullRequest) -> Value {
    let mut v = pr_json(pr, "active", Some("me"));
    v["mergeStatus"] = json!("queued");
    v["closedDate"] = Value::Null;
    v
}

#[tokio::test]
async fn ac10_complete_pins_the_displayed_commit() {
    let f = Fixture::new();
    f.mock.answer(200, queued_json(&pr_912()));
    let pr = f
        .writer
        .send(ReviewWrite::Complete {
            pr: pr_ref(),
            source_commit: "aaaa".into(),
            options: CompletionOptions {
                merge_strategy: MergeStrategy::Squash,
                delete_source_branch: true,
                transition_work_items: false,
                merge_commit_message: Some("Merged !912: Fix flaky checkout".into()),
            },
        })
        .await
        .unwrap()
        .into_pr()
        .unwrap();
    assert_call(
        &f.only_request(),
        Method::Patch,
        "https://dev.azure.com/contoso/p1/_apis/git/repositories/r1/pullrequests/912?api-version=7.1",
        json!({
            "status": "completed",
            "lastMergeSourceCommit": { "commitId": "aaaa" },
            "completionOptions": {
                "mergeStrategy": "squash",
                "deleteSourceBranch": true,
                "transitionWorkItems": false,
                "mergeCommitMessage": "Merged !912: Fix flaky checkout",
            },
        }),
    );
    // Server truth: still active, merge queued. adoken doesn't pretend it's
    // completed; it remembers the Complete is in flight.
    assert_eq!(
        (pr.status, pr.merge_status),
        (PrStatus::Active, MergeStatus::Queued)
    );
    assert_eq!(f.store.completing(912).unwrap(), Some(("aaaa".into(), NOW)));
    assert!(
        f.events()
            .iter()
            .all(|e| e.kind != SyncEventKind::PrStatusChanged),
        "not completed yet"
    );
}

/// Sync's own view of the org: pull request events are live (baseline
/// done), so a re-read emits what a pass would.
fn engine(f: &Fixture) -> Engine {
    f.store
        .write(|tx| {
            write::put_cursor(
                tx,
                &adoken_store::read::Cursor {
                    scope: "pr.active".into(),
                    project_id: "p1".into(),
                    baseline_done: true,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    Engine::new(
        f.store.clone(),
        f.mock.clone(),
        Arc::new(ManualClock::new(NOW + 2_000)),
        SyncConfig::new(url::Url::parse(ORG).unwrap()),
    )
}

const PR_GET: &str = "https://dev.azure.com/contoso/p1/_apis/git/pullrequests/912?api-version=7.1";

#[tokio::test]
async fn ac10_queued_complete_lands_through_the_follow_up_refresh() {
    let f = Fixture::new();
    // Auto-complete I had set: completion must clear its guard.
    f.store
        .write(|tx| write::set_auto_complete(tx, 912, "aaaa", NOW - MINUTE))
        .unwrap();
    f.mock.answer(200, queued_json(&pr_912()));
    f.writer
        .send(ReviewWrite::Complete {
            pr: pr_ref(),
            source_commit: "aaaa".into(),
            options: CompletionOptions::default(),
        })
        .await
        .unwrap();
    let engine = engine(&f);

    // First re-read: still merging.
    f.mock.answer(200, queued_json(&pr_912()));
    assert!(engine.recheck_completing(912).await.unwrap());
    assert!(f.store.completing(912).unwrap().is_some());
    assert!(f.store.auto_complete(912).unwrap().is_some());

    // Next one: Azure DevOps finished the merge.
    let mut done = pr_912();
    done.status = PrStatus::Completed;
    f.mock.answer(200, pr_json(&done, "completed", Some("me")));
    assert!(!engine.recheck_completing(912).await.unwrap());

    let sent = f.mock.sent();
    assert_eq!(sent.len(), 3);
    for get in &sent[1..] {
        assert_eq!(get.method(), &Method::Get);
        assert_eq!(get.url().as_str(), PR_GET);
    }
    let pr = f.store.pr(912).unwrap().unwrap().pr;
    assert_eq!(
        (pr.status, pr.merge_status),
        (PrStatus::Completed, MergeStatus::Succeeded)
    );
    assert_eq!(f.store.completing(912).unwrap(), None);
    assert_eq!(f.store.auto_complete(912).unwrap(), None);
    // One completion, by me (Ken's merge moment).
    let completed: Vec<SyncEvent> = f
        .events()
        .into_iter()
        .filter(|e| e.kind == SyncEventKind::PrStatusChanged)
        .collect();
    assert_eq!(completed.len(), 1, "{completed:#?}");
    assert_eq!(completed[0].dedupe_key, "completed");
    assert_eq!(completed[0].actor.as_deref(), Some("me"));
    assert!(completed[0].self_caused);
}

#[tokio::test]
async fn ac10_queued_complete_that_fails_keeps_the_pr_active_with_the_reason() {
    let f = Fixture::new();
    f.mock.answer(200, queued_json(&pr_912()));
    f.writer
        .send(ReviewWrite::Complete {
            pr: pr_ref(),
            source_commit: "aaaa".into(),
            options: CompletionOptions::default(),
        })
        .await
        .unwrap();
    let mut rejected = queued_json(&pr_912());
    rejected["mergeStatus"] = json!("rejectedByPolicy");
    f.mock.answer(200, rejected);
    assert!(!engine(&f).recheck_completing(912).await.unwrap());
    let pr = f.store.pr(912).unwrap().unwrap().pr;
    assert_eq!(
        (pr.status, pr.merge_status),
        (PrStatus::Active, MergeStatus::RejectedByPolicy)
    );
    // No longer completing: Complete is offered again.
    assert_eq!(f.store.completing(912).unwrap(), None);
    assert!(
        f.events()
            .iter()
            .all(|e| e.kind != SyncEventKind::PrStatusChanged)
    );
}

#[tokio::test]
async fn ac10_newer_commit_is_reported_and_never_retried() {
    // Azure DevOps' real answer when `lastMergeSourceCommit` is stale.
    let f = Fixture::new();
    f.mock.answer(
        409,
        json!({
            "message": "TF401192: The source branch has been modified since the last merge attempt.",
            "typeKey": "GitPullRequestStaleException",
        }),
    );
    let error = f
        .writer
        .send(ReviewWrite::Complete {
            pr: pr_ref(),
            source_commit: "aaaa".into(),
            options: CompletionOptions::default(),
        })
        .await
        .unwrap_err();
    assert_eq!(error, WriteError::NewCommits);
    assert_eq!(error.to_string(), "New commits since you reviewed");
    assert_eq!(f.mock.sent().len(), 1, "no automatic retry");
    assert_eq!(
        f.store.pr(912).unwrap().unwrap().pr.status,
        PrStatus::Active
    );
}

#[tokio::test]
async fn offline_and_rejected_writes_explain_themselves() {
    struct Down;
    impl Transport for Down {
        fn send(&self, _: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
            Box::pin(async { Err(TransportError::Offline("dns".into())) })
        }
    }
    let f = Fixture::new();
    let clock = Arc::new(ManualClock::new(NOW));
    let writer = Writer::new(
        f.store.clone(),
        Arc::new(Down),
        Arc::new(Governor::new(clock.clone())),
        clock,
        url::Url::parse(ORG).unwrap(),
    );
    let reply = ReviewWrite::Reply {
        pr: pr_ref(),
        thread_id: 7,
        parent_comment_id: 1,
        body: "x".into(),
    };
    assert_eq!(
        writer.send(reply.clone()).await.unwrap_err(),
        WriteError::Offline
    );
    f.mock
        .answer(404, json!({ "message": "TF401180: not found" }));
    let error = f.writer.send(reply).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Azure DevOps refused (HTTP 404): TF401180: not found"
    );
}

#[tokio::test]
async fn writes_are_ledgered_with_their_method() {
    let f = Fixture::new();
    let mock = MockTransport::default();
    mock.answer(200, json!({ "id": "me", "vote": 5 }));
    let clock = Arc::new(ManualClock::new(NOW));
    let recording = Arc::new(Recording::new(mock, Arc::new(NetLog::new(clock.clone()))));
    let writer = Writer::new(
        f.store.clone(),
        recording.clone(),
        Arc::new(Governor::new(clock.clone())),
        clock,
        url::Url::parse(ORG).unwrap(),
    );
    writer
        .send(ReviewWrite::Vote {
            pr: pr_ref(),
            reviewer: "me".into(),
            vote: Vote::ApprovedWithSuggestions,
        })
        .await
        .unwrap();
    let entries = recording.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].method, "PUT");
    assert_eq!(
        entries[0].route,
        "/{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{n}/reviewers/{id}"
    );
    assert_eq!(entries[0].purpose, "Vote");
    assert_eq!(entries[0].origin.label(), "You: vote");
}

#[tokio::test]
async fn my_writes_feed_ken_like_synced_actions() {
    let f = Fixture::new();
    adoken_ken::persist::migrate(&f.store).unwrap();
    f.mock.answer(200, json!({ "id": "me", "vote": 10 }));
    f.writer
        .send(ReviewWrite::Vote {
            pr: pr_ref(),
            reviewer: "me".into(),
            vote: Vote::Approved,
        })
        .await
        .unwrap();
    f.mock.answer(200, comment_json(2, 1, "me", "Thanks!"));
    f.writer
        .send(ReviewWrite::Reply {
            pr: pr_ref(),
            thread_id: 7,
            parent_comment_id: 1,
            body: "Thanks!".into(),
        })
        .await
        .unwrap();
    let batch = adoken_ken::source::poll(&f.store, 0, NOW).unwrap();
    let names: Vec<&str> = batch
        .actions
        .iter()
        .map(|a| match a {
            adoken_ken::Action::Vote { vote, .. } => {
                assert_eq!(*vote, Vote::Approved);
                "vote"
            }
            adoken_ken::Action::Comment { .. } => "comment",
            _ => "other",
        })
        .collect();
    assert_eq!(names, ["vote", "comment"]);
}

#[test]
fn demo_writes_apply_locally_with_the_same_events() {
    let f = Fixture::new();
    let thread = apply_local(
        &f.store,
        &ReviewWrite::CreateThread {
            pr: pr_ref(),
            anchor: Some(LineAnchor {
                path: "src/a.rs".into(),
                side: Side::Right,
                line: 12,
                change_tracking_id: Some(1),
            }),
            iteration: 3,
            body: "Local".into(),
        },
        NOW,
    )
    .unwrap()
    .into_thread()
    .unwrap();
    assert_eq!(thread.id, 8, "next id after the stored thread 7");
    apply_local(
        &f.store,
        &ReviewWrite::SetThreadStatus {
            pr: pr_ref(),
            thread_id: 7,
            status: ThreadStatus::Fixed,
        },
        NOW,
    )
    .unwrap();
    apply_local(
        &f.store,
        &ReviewWrite::SetAutoComplete {
            pr: pr_ref(),
            me: "me".into(),
            source_commit: "aaaa".into(),
            options: CompletionOptions::default(),
        },
        NOW,
    )
    .unwrap();
    assert_eq!(
        f.store.auto_complete(912).unwrap(),
        Some(("aaaa".into(), NOW))
    );
    let pr = apply_local(
        &f.store,
        &ReviewWrite::Complete {
            pr: pr_ref(),
            source_commit: "aaaa".into(),
            options: CompletionOptions::default(),
        },
        NOW,
    )
    .unwrap()
    .into_pr()
    .unwrap();
    assert_eq!(pr.status, PrStatus::Completed);
    assert_eq!(f.store.auto_complete(912).unwrap(), None);
    let kinds: Vec<SyncEventKind> = f.events().into_iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            SyncEventKind::PrCommentAdded,
            SyncEventKind::PrThreadResolved,
            SyncEventKind::PrStatusChanged,
        ]
    );
    assert!(f.events().iter().all(|e| e.self_caused));
}

/// AC-15 end to end: set auto-complete through the writer, a push arrives
/// through a normal sync pass, notify turns it into one P1 row, and its
/// Cancel action is the documented PATCH.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac15_push_after_auto_complete_raises_one_p1_and_cancel_clears_it() {
    let ds = dataset(DEFAULT_SEED);
    let h = Harness::start(&ds).await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
    let clock: Arc<dyn Clock> = h.clock.clone();
    let engine = Engine::new(
        store.clone(),
        h.transport(),
        clock.clone(),
        SyncConfig::new(h.org_url()),
    );
    for tier in [Tier::Warm, Tier::Hot, Tier::Cold] {
        engine.run_tier(tier).await.unwrap();
    }
    let me = ds.me.clone();
    // Two PRs by someone else that I don't review: one gets a push.
    let mut candidates = ds.prs.iter().filter(|p| {
        p.status == PrStatus::Active
            && p.author != me
            && !p.is_reviewer(&me)
            && !adoken_demo::SCENARIO_PRS.contains(&p.id)
    });
    let pushed = candidates.next().unwrap().id;
    let quiet = candidates.next().unwrap().id;

    let mock = Arc::new(MockTransport::default());
    let writer = Writer::new(
        store.clone(),
        mock.clone(),
        Arc::new(Governor::new(clock.clone())),
        clock.clone(),
        h.org_url(),
    );
    for id in [pushed, quiet] {
        let pr = store.pr(id).unwrap().unwrap().pr;
        mock.answer(200, pr_json(&pr, "active", None));
        writer
            .send(ReviewWrite::SetAutoComplete {
                pr: PrRef::of(&pr),
                me: me.clone(),
                source_commit: pr.last_source_commit.clone().unwrap(),
                options: CompletionOptions::default(),
            })
            .await
            .unwrap();
    }
    let set = &mock.sent()[0];
    assert_eq!(set.method(), &Method::Patch);
    assert_eq!(
        set.body().unwrap()["autoCompleteSetBy"],
        json!({ "id": me.clone() })
    );
    let cursor = store
        .sync_events_since(0, 100_000)
        .unwrap()
        .last()
        .map_or(0, |e| e.id);

    h.clock.advance(2 * MINUTE);
    h.state().prs.get_mut(&pushed).unwrap().last_source_commit = Some("b0b0b0".into());
    for _ in 0..3 {
        engine.run_tier(Tier::Warm).await.unwrap();
        engine.run_tier(Tier::Hot).await.unwrap();
    }

    adoken_notify::persist::migrate(&store).unwrap();
    let batch = adoken_notify::source::fetch(&store, cursor).unwrap();
    let pushes: Vec<&adoken_notify::Incoming> = batch
        .incoming
        .iter()
        .filter(|i| i.kind == adoken_notify::Kind::Sync(SyncEventKind::PrPushed))
        .collect();
    assert_eq!(pushes.len(), 1, "exactly one push event: {pushes:#?}");
    assert_eq!(pushes[0].subject, Subject::Pr(pushed));
    assert!(pushes[0].ctx.auto_complete_mine);
    assert_eq!(
        pushes[0].summary,
        format!("New commits on !{pushed} after you set auto-complete")
    );

    let notify_clock = adoken_notify::FakeClock::new(clock.now_ms(), 0);
    let mut inbox = adoken_notify::Engine::new(Arc::new(notify_clock), Default::default());
    let env = adoken_notify::DeliveryEnv::default();
    let mut effects = adoken_notify::Effects::default();
    for incoming in batch.incoming {
        inbox.ingest(incoming, &env, &mut effects);
    }
    let rows: Vec<&adoken_notify::Notification> = inbox
        .items()
        .iter()
        .filter(|n| n.priority == adoken_notify::Priority::P1)
        .collect();
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0].subject, Subject::Pr(pushed));
    assert!(
        inbox
            .items()
            .iter()
            .all(|n| n.subject != Subject::Pr(quiet)),
        "no push, no notification"
    );

    // The row's Cancel action.
    let pr = store.pr(pushed).unwrap().unwrap().pr;
    mock.answer(200, pr_json(&pr, "active", None));
    writer
        .send(ReviewWrite::CancelAutoComplete { pr: PrRef::of(&pr) })
        .await
        .unwrap();
    let cancel = mock.sent().pop().unwrap();
    assert_call(
        &cancel,
        Method::Patch,
        &format!(
            "{}{}/_apis/git/repositories/{}/pullrequests/{pushed}?api-version=7.1",
            h.org_url(),
            pr.project_id,
            pr.repo_id
        ),
        json!({ "autoCompleteSetBy": { "id": NO_IDENTITY } }),
    );
    assert_eq!(store.auto_complete(pushed).unwrap(), None);
    assert!(store.auto_complete(quiet).unwrap().is_some());
}

// ── FR-16 status changes, FR-17 work item links ─────────────────────────

const PR_UPDATE_URL: &str =
    "https://dev.azure.com/contoso/p1/_apis/git/repositories/r1/pullrequests/912?api-version=7.1";

fn status_json(pr: &PullRequest, status: &str, draft: bool, closed_by: Option<&str>) -> Value {
    let mut v = pr_json(pr, status, closed_by);
    v["isDraft"] = json!(draft);
    if status == "abandoned" {
        v["closedDate"] = json!(format_time(NOW));
    }
    v
}

#[tokio::test]
async fn status_changes_send_the_documented_patch_and_emit_my_status_events() {
    let f = Fixture::new();
    let mut draft = pr_912();
    draft.is_draft = true;
    f.store
        .write(|tx| write::upsert_pr(tx, &draft).map(|_| ()))
        .unwrap();
    // Bodies as verified live 2026-09-30 on a disposable PR.
    let steps = [
        (
            ReviewWrite::SetDraft {
                pr: pr_ref(),
                draft: false,
            },
            json!({ "isDraft": false }),
            status_json(&draft, "active", false, None),
            (PrStatus::Active, false),
        ),
        (
            ReviewWrite::SetDraft {
                pr: pr_ref(),
                draft: true,
            },
            json!({ "isDraft": true }),
            status_json(&draft, "active", true, None),
            (PrStatus::Active, true),
        ),
        (
            ReviewWrite::Abandon { pr: pr_ref() },
            json!({ "status": "abandoned" }),
            status_json(&draft, "abandoned", true, Some("me")),
            (PrStatus::Abandoned, true),
        ),
        (
            ReviewWrite::Reactivate { pr: pr_ref() },
            json!({ "status": "active" }),
            status_json(&draft, "active", false, None),
            (PrStatus::Active, false),
        ),
    ];
    for (write, body, answer, (status, is_draft)) in steps {
        f.mock.answer(200, answer);
        let pr = f.writer.send(write).await.unwrap().into_pr().unwrap();
        assert_call(
            f.mock.sent().last().unwrap(),
            Method::Patch,
            PR_UPDATE_URL,
            body,
        );
        assert_eq!((pr.status, pr.is_draft), (status, is_draft));
        let stored = f.store.pr(912).unwrap().unwrap().pr;
        assert_eq!((stored.status, stored.is_draft), (status, is_draft));
    }
    let events: Vec<(SyncEventKind, String, bool)> = f
        .events()
        .into_iter()
        .map(|e| (e.kind, e.dedupe_key, e.self_caused))
        .collect();
    // Sync's own keys (so its next pass dedupes), each once, all mine;
    // reactivating to "published" dedupes against the first publish.
    assert_eq!(
        events,
        [
            (SyncEventKind::PrStatusChanged, "published".into(), true),
            (SyncEventKind::PrStatusChanged, "draft".into(), true),
            (SyncEventKind::PrStatusChanged, "abandoned".into(), true),
        ]
    );
}

#[tokio::test]
async fn status_change_403_explains_the_permission_and_changes_nothing() {
    let f = Fixture::new();
    f.mock.answer(
        403,
        json!({ "message": "TF401027: You need the Git 'PullRequestContribute' permission to perform this action." }),
    );
    let error = f
        .writer
        .send(ReviewWrite::Abandon { pr: pr_ref() })
        .await
        .unwrap_err();
    assert!(matches!(error, WriteError::NotAllowed { .. }), "{error:?}");
    let text = error.to_string();
    assert!(text.contains("Contribute to pull requests"), "{text}");
    assert!(
        text.contains("TF401027"),
        "Azure DevOps' reason is kept: {text}"
    );
    assert_eq!(error.missing_scope(), None, "a permission, not the token");
    assert_eq!(
        f.store.pr(912).unwrap().unwrap().pr.status,
        PrStatus::Active
    );
    assert!(f.events().is_empty());
    assert_eq!(f.mock.sent().len(), 1, "no retry");
}

fn work_item(id: i64, links: &[i64]) -> WorkItem {
    WorkItem {
        id,
        project_id: "p1".into(),
        rev: 4,
        kind: WorkItemType::Bug,
        title: "Checkout times out".into(),
        state: "Active".into(),
        state_category: StateCategory::InProgress,
        assigned_to: Some("me".into()),
        created_by: "bo".into(),
        changed_by: "bo".into(),
        area_path: "Payments".into(),
        iteration_path: "Payments".into(),
        priority: Some(2),
        tags: Vec::new(),
        parent_id: None,
        linked_pr_ids: links.to_vec(),
        description: None,
        comment_count: 0,
        created: NOW - 600 * MINUTE,
        changed: NOW - 60 * MINUTE,
    }
}

/// A work item as `PATCH`/`GET _apis/wit/workitems/{id}` answers it, with
/// `relations` (a parent link first, then PR links).
fn wi_json(id: i64, rev: i64, links: &[(&str, &str, i64)]) -> Value {
    let identity = |id: &str| json!({ "id": id, "displayName": id });
    let mut relations = vec![json!({
        "rel": "System.LinkTypes.Hierarchy-Reverse",
        "url": "https://dev.azure.com/contoso/_apis/wit/workItems/1",
        "attributes": { "name": "Parent" },
    })];
    relations.extend(links.iter().map(|(project, repo, pr)| {
        json!({
            "rel": "ArtifactLink",
            "url": format!("vstfs:///Git/PullRequestId/{project}%2F{repo}%2F{pr}"),
            "attributes": { "name": "Pull Request" },
        })
    }));
    json!({
        "id": id,
        "rev": rev,
        "fields": {
            "System.TeamProject": "Payments",
            "System.WorkItemType": "Bug",
            "System.Title": "Checkout times out",
            "System.State": "Active",
            "System.AssignedTo": identity("me"),
            "System.CreatedBy": identity("bo"),
            "System.ChangedBy": identity("me"),
            "System.AreaPath": "Payments",
            "System.IterationPath": "Payments",
            "Microsoft.VSTS.Common.Priority": 2,
            "System.CreatedDate": format_time(NOW - 600 * MINUTE),
            "System.ChangedDate": format_time(NOW),
        },
        "relations": relations,
    })
}

const WI_URL: &str = "https://dev.azure.com/contoso/_apis/wit/workitems/4821?api-version=7.1";

#[tokio::test]
async fn link_adds_the_prs_artifact_link_to_the_work_item() {
    let f = Fixture::new();
    f.store
        .write(|tx| write::upsert_work_item(tx, &work_item(4821, &[])).map(|_| ()))
        .unwrap();
    f.mock.answer(200, wi_json(4821, 5, &[("p1", "r1", 912)]));
    let item = f
        .writer
        .send(ReviewWrite::LinkWorkItem {
            pr: pr_ref(),
            work_item: 4821,
        })
        .await
        .unwrap()
        .into_work_item()
        .unwrap()
        .unwrap();
    let req = f.only_request();
    // The shape the web UI sends (verified live 2026-09-30): JSON Patch.
    assert_call(
        &req,
        Method::Patch,
        WI_URL,
        json!([{
            "op": "add",
            "path": "/relations/-",
            "value": {
                "rel": "ArtifactLink",
                "url": "vstfs:///Git/PullRequestId/p1%2Fr1%2F912",
                "attributes": { "name": "Pull Request" },
            },
        }]),
    );
    assert_eq!(req.content_type(), "application/json-patch+json");
    assert_eq!(item.linked_pr_ids, [912]);
    assert_eq!(
        f.store.work_item(4821).unwrap().unwrap().linked_pr_ids,
        [912]
    );
    assert!(f.events().is_empty(), "a relation change is no news");
}

#[tokio::test]
async fn unlink_removes_the_prs_relation_by_index_guarded_by_the_revision() {
    let f = Fixture::new();
    f.store
        .write(|tx| write::upsert_work_item(tx, &work_item(4821, &[77, 912])).map(|_| ()))
        .unwrap();
    // Relations: parent (0), another PR (1), this PR with other-case ids (2):
    // the PR id alone identifies it (unique per org).
    f.mock.answer(
        200,
        wi_json(4821, 9, &[("p1", "r1", 77), ("P1", "R1", 912)]),
    );
    f.mock.answer(200, wi_json(4821, 10, &[("p1", "r1", 77)]));
    let item = f
        .writer
        .send(ReviewWrite::UnlinkWorkItem {
            pr: pr_ref(),
            work_item: 4821,
        })
        .await
        .unwrap()
        .into_work_item()
        .unwrap()
        .unwrap();
    let sent = f.mock.sent();
    assert_eq!(sent.len(), 2, "{sent:#?}");
    assert_eq!(sent[0].method(), &Method::Get);
    assert_eq!(
        sent[0].url().as_str(),
        format!("{WI_URL}&%24expand=relations")
    );
    assert_call(
        &sent[1],
        Method::Patch,
        WI_URL,
        json!([
            { "op": "test", "path": "/rev", "value": 9 },
            { "op": "remove", "path": "/relations/2" },
        ]),
    );
    assert_eq!(sent[1].content_type(), "application/json-patch+json");
    assert_eq!(item.linked_pr_ids, [77]);
}

#[tokio::test]
async fn unlink_of_a_link_already_gone_sends_no_removal() {
    let f = Fixture::new();
    f.store
        .write(|tx| write::upsert_work_item(tx, &work_item(4821, &[912])).map(|_| ()))
        .unwrap();
    f.mock.answer(200, wi_json(4821, 9, &[]));
    let item = f
        .writer
        .send(ReviewWrite::UnlinkWorkItem {
            pr: pr_ref(),
            work_item: 4821,
        })
        .await
        .unwrap()
        .into_work_item()
        .unwrap()
        .unwrap();
    assert_eq!(f.mock.sent().len(), 1, "only the read");
    assert!(
        item.linked_pr_ids.is_empty(),
        "the server's state is stored"
    );
}

#[tokio::test]
async fn link_without_work_write_names_the_scope_and_keeps_the_token() {
    // A PAT without vso.work_write gets 401 with an empty body (seen live);
    // through the optional transport it is an ordinary status.
    let f = Fixture::new();
    f.mock.answer(401, Value::Null);
    let error = f
        .writer
        .send(ReviewWrite::LinkWorkItem {
            pr: pr_ref(),
            work_item: 4821,
        })
        .await
        .unwrap_err();
    assert!(
        matches!(error, WriteError::MissingWorkScope { .. }),
        "{error:?}"
    );
    assert_eq!(error.missing_scope(), Some("vso.work_write"));
    // The governor didn't latch "unauthorized": the next write still goes.
    f.mock.answer(200, json!({ "id": "me", "vote": 10 }));
    f.writer
        .send(ReviewWrite::Vote {
            pr: pr_ref(),
            reviewer: "me".into(),
            vote: Vote::Approved,
        })
        .await
        .unwrap();
    // 403 (e.g. area permissions) keeps Azure DevOps' reason.
    f.mock.answer(
        403,
        json!({ "message": "TF237111: The current user does not have permissions to save work items under the specified area path." }),
    );
    let error = f
        .writer
        .send(ReviewWrite::LinkWorkItem {
            pr: pr_ref(),
            work_item: 4821,
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("TF237111"), "{error}");
}

#[tokio::test]
async fn linked_work_items_reads_the_prs_refs_then_stores_the_items() {
    let f = Fixture::new();
    f.mock.answer(
        200,
        json!({ "count": 2, "value": [
            { "id": "4821", "url": "https://dev.azure.com/contoso/_apis/wit/workItems/4821" },
            { "id": "99", "url": "https://dev.azure.com/contoso/_apis/wit/workItems/99" },
        ]}),
    );
    // 99 is unreadable for this token: `errorPolicy=omit` answers null.
    f.mock.answer(
        200,
        json!({ "count": 2, "value": [wi_json(4821, 3, &[("p1", "r1", 912)]), null] }),
    );
    let ids = f.writer.linked_work_items(&pr_ref()).await.unwrap();
    assert_eq!(ids, [4821, 99]);
    let sent = f.mock.sent();
    assert_eq!(
        sent[0].url().as_str(),
        format!("{PR_URL}/workitems?api-version=7.1")
    );
    assert_eq!(
        sent[1].url().as_str(),
        "https://dev.azure.com/contoso/_apis/wit/workitems?ids=4821%2C99&%24expand=relations&errorPolicy=omit&api-version=7.1"
    );
    assert_eq!(
        f.store.work_item(4821).unwrap().unwrap().linked_pr_ids,
        [912]
    );
    assert!(f.events().is_empty());

    // None linked: one request.
    let f = Fixture::new();
    f.mock.answer(200, json!({ "count": 0, "value": [] }));
    assert!(
        f.writer
            .linked_work_items(&pr_ref())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.mock.sent().len(), 1);
}

#[test]
fn demo_status_and_link_writes_apply_locally() {
    let f = Fixture::new();
    f.store
        .write(|tx| write::upsert_work_item(tx, &work_item(4821, &[])).map(|_| ()))
        .unwrap();
    let pr = apply_local(&f.store, &ReviewWrite::Abandon { pr: pr_ref() }, NOW)
        .unwrap()
        .into_pr()
        .unwrap();
    assert_eq!(pr.status, PrStatus::Abandoned);
    assert_eq!(pr.closed, Some(NOW));
    let pr = apply_local(&f.store, &ReviewWrite::Reactivate { pr: pr_ref() }, NOW)
        .unwrap()
        .into_pr()
        .unwrap();
    assert_eq!((pr.status, pr.closed), (PrStatus::Active, None));
    let pr = apply_local(
        &f.store,
        &ReviewWrite::SetDraft {
            pr: pr_ref(),
            draft: true,
        },
        NOW,
    )
    .unwrap()
    .into_pr()
    .unwrap();
    assert!(pr.is_draft);
    let link = ReviewWrite::LinkWorkItem {
        pr: pr_ref(),
        work_item: 4821,
    };
    apply_local(&f.store, &link, NOW).unwrap();
    apply_local(&f.store, &link, NOW).unwrap();
    assert_eq!(
        f.store.work_item(4821).unwrap().unwrap().linked_pr_ids,
        [912],
        "linking twice keeps one link"
    );
    apply_local(
        &f.store,
        &ReviewWrite::UnlinkWorkItem {
            pr: pr_ref(),
            work_item: 4821,
        },
        NOW,
    )
    .unwrap();
    assert!(
        f.store
            .work_item(4821)
            .unwrap()
            .unwrap()
            .linked_pr_ids
            .is_empty()
    );
    let keys: Vec<String> = f.events().into_iter().map(|e| e.dedupe_key).collect();
    assert_eq!(keys, ["abandoned", "published", "draft"]);
    assert!(f.events().iter().all(|e| e.self_caused));
}
