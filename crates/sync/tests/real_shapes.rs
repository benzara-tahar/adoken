//! Payload shapes recorded from a real dev.azure.com org (REST 7.1,
//! 2026-09-25), sanitized: names, emails, ids and free text replaced, the
//! structure (field presence, nulls, string vs object, date precision) kept.
//! Each test pins a place where reality differed from the docs-based mock.

use adoken_model::{
    ChangeType, PrFileChange, PrStatus, RunReason, RunResult, RunStage, RunStatus, StageOutcome,
    ThreadStatus, WorkItemType,
};
use adoken_sync::wire::{
    self, ConnectionData, GitPullRequest, List, RevisionsPage, WireBuild, WireThread, WireWorkItem,
    parse_time,
};
use serde::de::DeserializeOwned;

const ME: &str = "6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d";

fn fixture<T: DeserializeOwned>(name: &str) -> T {
    let path = format!("{}/tests/fixtures/real/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn connection_data_carries_sign_in_name_and_org_id() {
    let c: ConnectionData = fixture("connection_data.json");
    assert_eq!(c.authenticated_user.id, ME);
    // WIQL and `requestedFor` match people by sign-in name, not id.
    assert_eq!(c.authenticated_user.account(), Some("dana@example.com"));
    assert_eq!(
        c.instance_id.as_deref(),
        Some("00000000-0000-0000-0000-0000c0ffee00")
    );
}

#[test]
fn reporting_revisions_identities_are_strings() {
    let page: RevisionsPage = fixture("reporting_revisions.json");
    assert!(page.is_last_batch);
    assert!(page.continuation_token.is_some());
    let mut rows = page.values.into_iter();
    let mut first: WireWorkItem = rows.next().unwrap();
    assert!(first.has_identity_strings());
    // Unresolved people fall back to the lowercased unique name; resolved
    // ones get the known id.
    first.normalize_identities(|unique, _| {
        (unique == Some("dana@example.com")).then(|| ME.to_owned())
    });
    let people = first.people();
    assert!(
        people
            .iter()
            .all(|p| p.id == ME && p.display_name == "Dana Example")
    );
    let wi = first
        .into_model("p".into())
        .expect("string identities convert");
    assert_eq!(wi.created_by, ME);
    assert_eq!(wi.assigned_to.as_deref(), Some(ME));
    assert_eq!(wi.kind, WorkItemType::Issue);

    let mut second = rows.next().unwrap();
    second.normalize_identities(|_, _| None);
    let wi = second.into_model("p".into()).unwrap();
    assert_eq!(wi.created_by, "dana@example.com");
    assert_eq!(wi.assigned_to, None, "unassigned rows omit the field");
}

#[test]
fn identity_strings_split_display_and_unique_name() {
    assert_eq!(
        wire::split_identity("Dana Example <dana@example.com>"),
        ("Dana Example", Some("dana@example.com"))
    );
    assert_eq!(
        wire::split_identity("Build Service"),
        ("Build Service", None)
    );
    assert_eq!(wire::split_identity("Odd <>"), ("Odd", None));
}

#[test]
fn work_items_batch_sends_identity_objects() {
    let list: List<WireWorkItem> = fixture("workitems_batch.json");
    let w = list.value.into_iter().next().unwrap();
    assert!(!w.has_identity_strings());
    let wi = w.into_model("p".into()).unwrap();
    assert_eq!(wi.created_by, ME);
    assert_eq!(wi.priority, Some(2));
    assert_eq!(wi.comment_count, 3);
    // Live descriptions are styled HTML; the store keeps plain text.
    assert_eq!(
        wi.description.as_deref(),
        Some("Receipts upload from the mobile app.")
    );
}

#[test]
fn completed_pull_requests_parse_with_seven_digit_fractions() {
    let list: List<GitPullRequest> = fixture("pull_requests_completed.json");
    assert_eq!(list.value.len(), 2);
    let pr = &list.value[0];
    assert_eq!(pr.status(), PrStatus::Completed);
    assert_eq!(
        pr.created(),
        parse_time("2024-03-29T01:30:19.422Z").unwrap()
    );
    assert!(pr.closed().is_some());
    assert_eq!(pr.created_by.id, ME);
}

#[test]
fn system_threads_have_null_contexts_and_no_status() {
    let list: List<WireThread> = fixture("pr_threads_system.json");
    let t = list.value.into_iter().next().unwrap().into_model(2);
    assert_eq!(t.status, ThreadStatus::Unknown);
    assert!(t.anchor.is_none() && t.iteration.is_none());
    assert!(t.comments[0].is_system);
    assert!(
        !t.comments[0].is_deleted,
        "`isDeleted` is absent on live comments"
    );
}

#[test]
fn builds_parse_ci_runs() {
    let list: List<WireBuild> = fixture("builds.json");
    let run = list.value.into_iter().next().unwrap().into_model();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.result, Some(RunResult::Succeeded));
    assert_eq!(run.reason, RunReason::IndividualCi);
    assert_eq!(run.requested_for.as_deref(), Some(ME));
    assert_eq!(run.pr_id, None);
    assert!(run.finish.unwrap() > run.started.unwrap());
}

/// Spec 0012: PR payloads from the personal org (2026-09-30) point
/// `imageUrl` at the identity-image route (captured) and `_links.avatar` at
/// the GraphProfile route that 401s with a PAT (ignored).
#[test]
fn pull_request_identities_capture_the_identity_image_url() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Pr {
        created_by: wire::IdentityRef,
    }
    let list: List<Pr> = fixture("pull_requests_identity_image.json");
    let person = list.value[0].created_by.person();
    let url = format!("https://dev.azure.com/contoso/_api/_common/identityImage?id={ME}");
    assert_eq!(person.image_url.as_deref(), Some(url.as_str()));
    // The fetch is built from the id on the org host, never from payloads.
    let org = url::Url::parse("https://dev.azure.com/contoso/").unwrap();
    assert_eq!(
        adoken_sync::avatars::avatar_url(&org, &person.id, person.is_group)
            .unwrap()
            .as_str(),
        url
    );
}

/// Other orgs' PR, build and work item payloads carry the GraphProfile form
/// in `imageUrl`: nothing is captured, and the picture comes from the id.
#[test]
fn member_avatar_urls_are_not_captured() {
    let prs: List<GitPullRequest> = fixture("pull_requests_completed.json");
    let author = prs.value[0].created_by.person();
    let reviewer = prs
        .value
        .iter()
        .find_map(|pr| pr.reviewers.first())
        .expect("a reviewed PR")
        .identity
        .person();
    let builds: List<WireBuild> = fixture("builds.json");
    let requested_for = builds.value[0].requested_for.as_ref().unwrap().person();
    let items: List<WireWorkItem> = fixture("workitems_batch.json");
    let people = items.value[0].people();
    assert!(!people.is_empty());
    for p in [&author, &reviewer, &requested_for]
        .into_iter()
        .chain(&people)
    {
        assert_eq!(p.image_url, None, "{}", p.display_name);
        assert!(!p.is_group);
    }
}

/// A one-stage YAML run: a single implicit `__default` stage, records
/// unordered, a Checkpoint with `order: null`, a task at `i32::MAX`. The run
/// detail lists the job instead of `__default`.
#[test]
fn single_stage_timeline_shows_its_job() {
    let t: wire::WireTimeline = fixture("build_timeline.json");
    let records = t.records.unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.kind == "Checkpoint" && r.order.is_none())
    );
    let stages = wire::timeline_stages(&records);
    assert_eq!(
        stages,
        [RunStage {
            name: "Job".into(),
            outcome: StageOutcome::Succeeded,
            started: parse_time("2024-04-17T12:09:05.0066667Z"),
            finished: parse_time("2024-04-17T12:09:15.1333333Z"),
        }]
    );
}

/// Work item comments (7.1-preview.4): HTML text, `createdBy` objects,
/// `mentions[]` present (empty here), newest first.
#[test]
fn work_item_comments_become_plain_text() {
    let list: wire::WireCommentList = fixture("wi_comments.json");
    assert_eq!(list.comments.len(), 3);
    assert!(list.continuation_token.is_none());
    let c = list.comments.into_iter().nth(1).unwrap().into_model();
    assert_eq!(c.work_item_id, 14);
    assert_eq!(c.author, ME);
    assert_eq!(c.body, "xxxxxxx xxxxxxx", "`<div><br></div>` adds nothing");
    assert!(c.mentions.is_empty());
    assert_eq!(c.created, parse_time("2024-04-15T20:56:23.33Z").unwrap());
    assert_eq!(c.updated, c.created);
}

/// Iteration changes (7.1): the 7.1 docs' shape plus the fields Azure
/// DevOps sends alongside (`gitObjectType`, `commitId`, `url`), a combined
/// `"edit, rename"` flag set with `originalPath`, and a delete whose
/// `item.path` is `null` with the path in `originalPath` (seen live
/// 2026-09-30: every delete in a real PR has this shape). `changeTrackingId`
/// is what the web UI sends with new line comments (spec 0004 FR-6).
#[test]
fn iteration_changes_carry_change_tracking_ids() {
    let page: wire::IterationChanges = fixture("pr_iteration_changes.json");
    assert_eq!(page.next_skip, 0, "last page");
    let files: Vec<PrFileChange> = page
        .change_entries
        .into_iter()
        .filter_map(wire::ChangeEntry::into_model)
        .collect();
    let summary: Vec<(&str, ChangeType, Option<u32>, bool, bool)> = files
        .iter()
        .map(|f| {
            (
                f.path.as_str(),
                f.change_type,
                f.change_tracking_id,
                f.old_content.is_some(),
                f.new_content.is_some(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "/src/checkout/retry.rs",
                ChangeType::Edit,
                Some(1),
                true,
                true
            ),
            (
                "/src/checkout/backoff.rs",
                ChangeType::Add,
                Some(2),
                false,
                true
            ),
            (
                "/src/checkout/client.rs",
                ChangeType::Rename,
                Some(3),
                true,
                true
            ),
            (
                "/src/checkout/legacy.rs",
                ChangeType::Delete,
                Some(4),
                true,
                false
            ),
        ]
    );
    assert_eq!(
        files[2].original_path.as_deref(),
        Some("/src/checkout/http.rs")
    );
    assert_eq!(
        files[3].old_content.as_deref(),
        Some("4e6a8c0e2a4c6e8a0c2e4a6c8e0a2c4e6a8c0e2a")
    );
    // An entry without an id (or with 0) stays unknown: never sent as 0.
    let bare: wire::ChangeEntry = serde_json::from_value(serde_json::json!({
        "changeTrackingId": 0,
        "item": { "objectId": "a", "path": "/x" },
        "changeType": "add",
    }))
    .unwrap();
    assert_eq!(bare.into_model().unwrap().change_tracking_id, None);
    // An entry naming no file is dropped, not a failure of the whole list.
    let nameless: wire::ChangeEntry = serde_json::from_value(serde_json::json!({
        "item": { "objectId": "a", "path": null },
        "changeType": "edit",
    }))
    .unwrap();
    assert!(nameless.into_model().is_none());
}

/// Iterations (7.1, recorded 2026-09-30 on a two-push PR): each carries
/// `sourceRefCommit`, `targetRefCommit` and `commonRefCommit` (the merge
/// base the `$compareTo=0` changes are against), `push` only when the
/// iteration came from a push.
#[test]
fn iterations_carry_the_merge_base() {
    let list: List<wire::WireIteration> = fixture("pr_iterations.json");
    assert_eq!(list.value.len(), 2);
    let latest = list.value.iter().max_by_key(|it| it.id).unwrap().clone();
    assert_eq!(latest.id, 2);
    assert_eq!(
        latest.common_ref_commit.as_ref().unwrap().commit_id,
        "55df94856ce8592136e6ab3f7211ae23adf79819"
    );
    let model = latest.into_model(None);
    assert_eq!(
        model.source_commit,
        "1bb846f649afbe5d14b2c4091cc51f9006be9cbe"
    );
    assert_eq!(
        model.description.as_deref(),
        Some("adoken smoke: second commit")
    );
    assert!(model.created > 0, "seven-digit fractions parse");
}

/// `blobs/{objectId}?$format=octetstream` answers the raw bytes; they hash
/// to the object id git-style, which is what the cache verifies.
#[test]
fn blob_bytes_hash_to_their_object_id() {
    let path = format!(
        "{}/tests/fixtures/real/blob_c2ba21f8.bin",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        adoken_sync::pr_files::git_blob_id(&bytes),
        "c2ba21f8ffd0675e80ce0e805bf99ccc94e30444"
    );
}
