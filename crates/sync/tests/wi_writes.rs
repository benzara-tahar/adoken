//! Spec 0015 work item writes, comments, per-item sync and metadata against
//! a scripted mock transport behind the network recorder (so every request
//! must match the route table and carry an origin, spec 0016 FR-18/19):
//! the exact REST calls, the FR-8 decisions, and what lands in the store.
//!
//! AC map: AC-2 `ac2_*`, AC-3 `ac3_*`, AC-5 `ac5_*`, AC-6 `ac6_*`, AC-7
//! `ac7_*`, AC-8 `ac8_*`, AC-9 `ac9_*`, AC-11 `ac11_*`, AC-12 `ac12_*`,
//! AC-13/14 `ac13_ac14_*`, AC-17 `ac17_*`, AC-18 `ac18_*`, AC-19 `ac19_*`,
//! AC-20 `ac20_*`, AC-23 `ac23_*`. AC-4, AC-10, AC-22, AC-24 (controller)
//! are in `crates/app/src/workitems/writes/tests.rs`; AC-16 in the store.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use adoken_model::time::{HOUR, MINUTE, SECOND};
use adoken_model::*;
use adoken_store::wi::{self as store_wi, FullFields};
use adoken_store::{Store, write};
use adoken_sync::activity::{Tracker, Trigger};
use adoken_sync::attribution::{Origin, with_origin};
use adoken_sync::cadence::Lane;
use adoken_sync::ledger::NetLog;
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::wi::{
    WiChange, WiOutcome, WiWrite, apply_local, comment_local, is_stale, link_guard,
};
use adoken_sync::*;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::Notify;

const ORG: &str = "https://dev.azure.com/contoso/";
/// 2026-09-30 12:00 UTC.
const NOW: Millis = 1_790_769_600_000;
const DANA: &str = "d2910000-0000-0000-0000-000000000001";

struct Reply {
    status: u16,
    body: Value,
    rate: RateHeaders,
    hang: Option<Arc<Notify>>,
}

/// Records every request with the clock time it arrived; answers from a
/// script, in order.
struct Mock {
    clock: Arc<ManualClock>,
    sent: Mutex<Vec<(Millis, ApiRequest)>>,
    script: Mutex<VecDeque<Reply>>,
}

impl Mock {
    fn answer(&self, status: u16, body: Value) {
        self.script.lock().push_back(Reply {
            status,
            body,
            rate: RateHeaders::default(),
            hang: None,
        });
    }

    fn sent(&self) -> Vec<ApiRequest> {
        self.sent.lock().iter().map(|(_, r)| r.clone()).collect()
    }

    /// `METHOD /path` of each request.
    fn lines(&self) -> Vec<String> {
        self.sent()
            .iter()
            .map(|r| format!("{} {}", r.method().as_str(), r.url().path()))
            .collect()
    }
}

impl Transport for Mock {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        self.sent.lock().push((self.clock.now_ms(), req.clone()));
        let reply = self
            .script
            .lock()
            .pop_front()
            .unwrap_or_else(|| panic!("unscripted request {}", req.url()));
        Box::pin(async move {
            if let Some(hang) = reply.hang {
                hang.notified().await;
            }
            Ok(ApiResponse {
                status: reply.status,
                rate: reply.rate,
                body: serde_json::to_vec(&reply.body).unwrap(),
                content_type: Some("application/json".into()),
                validators: Default::default(),
            })
        })
    }
}

/// The mock behind the recorder, shared with the test.
struct Shared(Arc<Mock>);

impl Transport for Shared {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        self.0.send(req)
    }
}

fn person(id: &str, name: &str, unique: &str) -> Person {
    Person {
        id: id.into(),
        display_name: name.into(),
        unique_name: Some(unique.into()),
        is_group: false,
        image_url: None,
    }
}

fn identity(id: &str) -> Value {
    match id {
        "me" => json!({"id": "me", "displayName": "Ada Me", "uniqueName": "ada@contoso.com"}),
        DANA => json!({"id": DANA, "displayName": "Dana Ruiz", "uniqueName": "dana@contoso.com"}),
        other => json!({"id": other, "displayName": other}),
    }
}

/// #42 as Azure DevOps answers it.
fn item(rev: i64, state: &str, changed_by: &str, extra: &[(&str, Value)]) -> Value {
    let mut fields = json!({
        "System.TeamProject": "Payments",
        "System.WorkItemType": "Bug",
        "System.Title": "Timezone bug in daily settlement job",
        "System.State": state,
        "System.AssignedTo": identity("me"),
        "System.CreatedBy": identity("me"),
        "System.ChangedBy": identity(changed_by),
        "System.AreaPath": "Payments",
        "System.IterationPath": "Payments\\Sprint 44",
        "System.CreatedDate": "2026-09-01T09:00:00Z",
        "System.ChangedDate": format!("2026-09-30T11:{:02}:00Z", 50 + rev.min(9)),
        "System.CommentCount": 0,
    });
    for (k, v) in extra {
        fields[*k] = v.clone();
    }
    json!({"id": 42, "rev": rev, "fields": fields, "relations": relations(&[])})
}

fn wi_url(id: i64) -> String {
    format!("{ORG}_apis/wit/workItems/{id}")
}

/// Relations in this order: PR, commit, Parent #7, Related #9 and any more.
fn relations(more: &[(&str, i64)]) -> Value {
    let mut out = vec![
        json!({"rel": "ArtifactLink", "url": "vstfs:///Git/PullRequestId/p1%2Fr1%2F4821", "attributes": {"name": "Pull Request"}}),
        json!({"rel": "ArtifactLink", "url": "vstfs:///Git/Commit/p1%2Fr1%2Fabc", "attributes": {"name": "Fixed in Commit"}}),
        json!({"rel": "System.LinkTypes.Hierarchy-Reverse", "url": wi_url(7), "attributes": {"name": "Parent"}}),
        json!({"rel": "System.LinkTypes.Related", "url": wi_url(9), "attributes": {"name": "Related"}}),
    ];
    for (rel, id) in more {
        out.push(json!({"rel": rel, "url": wi_url(*id), "attributes": {}}));
    }
    Value::Array(out)
}

struct Fixture {
    store: Store,
    mock: Arc<Mock>,
    clock: Arc<ManualClock>,
    gov: Arc<Governor>,
    log: Arc<NetLog>,
    writer: WiWriter,
    _dir: tempfile::TempDir,
}

impl Fixture {
    /// Me, Dana, project Payments and #42 at rev 7 (State Active), stored
    /// from a full answer (so its relations and freshness are too).
    async fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        store
            .write(|tx| {
                write::upsert_person(tx, &person("me", "Ada Me", "ada@contoso.com"), true)?;
                write::upsert_person(tx, &person(DANA, "Dana Ruiz", "dana@contoso.com"), false)?;
                write::upsert_project(
                    tx,
                    &Project {
                        id: "p1".into(),
                        name: "Payments".into(),
                    },
                )
            })
            .unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let mock = Arc::new(Mock {
            clock: clock.clone(),
            sent: Mutex::new(Vec::new()),
            script: Mutex::new(VecDeque::new()),
        });
        let log = Arc::new(NetLog::new(clock.clone()));
        let gov = Arc::new(Governor::new(clock.clone()));
        let writer = WiWriter::new(
            store.clone(),
            Arc::new(Recording::new(Shared(mock.clone()), log.clone())),
            gov.clone(),
            clock.clone(),
            url::Url::parse(ORG).unwrap(),
        );
        let f = Fixture {
            store,
            mock,
            clock,
            gov,
            log,
            writer,
            _dir: dir,
        };
        // Seed through a per-item sync (one item GET, one empty comment page).
        f.mock.answer(200, item(7, "Active", "me", &[]));
        f.mock.answer(200, json!({"comments": [], "count": 0}));
        f.seed_row();
        f.writer.sync_item(42).await.unwrap();
        f.mock.sent.lock().clear();
        f
    }

    /// The title-index row a per-item sync needs to know #42's project.
    fn seed_row(&self) {
        self.store
            .write(|tx| {
                write::upsert_work_item(
                    tx,
                    &WorkItem {
                        id: 42,
                        project_id: "p1".into(),
                        rev: 1,
                        kind: WorkItemType::Bug,
                        title: "Timezone bug in daily settlement job".into(),
                        state: "New".into(),
                        state_category: StateCategory::Proposed,
                        assigned_to: Some("me".into()),
                        created_by: "me".into(),
                        changed_by: "me".into(),
                        area_path: "Payments".into(),
                        iteration_path: "Payments".into(),
                        priority: None,
                        tags: Vec::new(),
                        parent_id: None,
                        linked_pr_ids: Vec::new(),
                        description: None,
                        comment_count: 0,
                        created: NOW - 30 * 24 * HOUR,
                        changed: NOW - 30 * 24 * HOUR,
                    },
                )
            })
            .unwrap();
    }

    fn stored(&self) -> (WorkItem, WorkItemExtras) {
        self.store
            .with_conn(|c| {
                Ok((
                    adoken_store::read::work_item(c, 42)?.unwrap(),
                    store_wi::extras(c, 42)?.unwrap(),
                ))
            })
            .unwrap()
    }

    fn origins(&self) -> Vec<String> {
        self.log
            .entries()
            .iter()
            .map(|e| e.origin.label())
            .collect()
    }
}

fn state(to: &str, base: &str) -> WiWrite {
    WiWrite {
        id: 42,
        base_rev: 7,
        change: WiChange::State {
            to: to.into(),
            base: base.into(),
        },
        extra: Vec::new(),
    }
}

fn query(r: &ApiRequest) -> Vec<(String, String)> {
    r.url()
        .query_pairs()
        .map(|(a, b)| (a.into_owned(), b.into_owned()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

/// AC-2: exactly one PATCH with `test /rev` and the state; the answer is
/// the re-read (no GET follows), applied with `synced_ms` = the clock and
/// self-caused events only.
#[tokio::test]
async fn ac2_state_write_is_one_patch_and_its_answer_is_the_re_read() {
    let f = Fixture::new().await;
    f.clock.advance(MINUTE);
    f.mock.answer(
        200,
        item(8, "Resolved", "me", &[("System.Reason", json!("Fixed"))]),
    );
    let outcome = f.writer.send(&state("Resolved", "Active")).await.unwrap();
    let sent = f.mock.sent();
    assert_eq!(f.mock.lines(), ["PATCH /contoso/_apis/wit/workitems/42"]);
    assert_eq!(
        query(&sent[0]),
        pairs(&[("$expand", "relations"), ("api-version", "7.1")])
    );
    assert_eq!(sent[0].content_type(), "application/json-patch+json");
    assert_eq!(
        sent[0].body().unwrap(),
        &json!([
            {"op": "test", "path": "/rev", "value": 7},
            {"op": "add", "path": "/fields/System.State", "value": "Resolved"}
        ])
    );
    let WiOutcome::Applied(Some(applied)) = outcome else {
        panic!("applied: {outcome:?}");
    };
    assert_eq!((applied.rev, applied.state.as_str()), (8, "Resolved"));
    assert_eq!(applied.state_category, StateCategory::Resolved);
    let (_, extras) = f.stored();
    assert_eq!(extras.synced_ms, Some(NOW + MINUTE));
    assert_eq!(extras.full_synced_ms, Some(NOW + MINUTE));
    assert_eq!(extras.reason.as_deref(), Some("Fixed"), "set by the server");
    let events = f.store.sync_events_since(0, 100).unwrap();
    assert!(events.iter().all(|e| e.self_caused), "{events:#?}");
    assert_eq!(f.origins().last().unwrap(), "You: change state");
}

/// AC-5: a refused PATCH, a re-read where only Tags moved, one resend on
/// the new rev: PATCH, GET, PATCH, and no conflict.
#[tokio::test]
async fn ac5_untouched_field_resends_once_on_the_new_rev() {
    let f = Fixture::new().await;
    f.mock.answer(400, json!({"message": "TF26071: This work item has been changed by someone else since you opened it."}));
    f.mock.answer(
        200,
        item(8, "Active", DANA, &[("System.Tags", json!("infra"))]),
    );
    f.mock.answer(200, item(9, "Resolved", "me", &[]));
    let outcome = f.writer.send(&state("Resolved", "Active")).await.unwrap();
    assert!(matches!(&outcome, WiOutcome::Applied(Some(i)) if i.rev == 9));
    assert_eq!(
        f.mock.lines(),
        [
            "PATCH /contoso/_apis/wit/workitems/42",
            "GET /contoso/p1/_apis/wit/workitems/42",
            "PATCH /contoso/_apis/wit/workitems/42"
        ]
    );
    assert_eq!(f.mock.sent()[2].body().unwrap()[0]["value"], json!(8));
    let origins = f.origins();
    assert_eq!(origins[origins.len() - 2], "You: change state · re-read");
}

/// AC-6: Dana changed the state meanwhile: their value is stored, the
/// conflict names her, nothing is resent; Apply mine sends on rev 8.
#[tokio::test]
async fn ac6_conflict_names_who_changed_it_and_apply_mine_sends_on_their_rev() {
    let f = Fixture::new().await;
    f.mock
        .answer(400, json!({"message": "TF26071: changed by someone else"}));
    f.mock.answer(200, item(8, "Closed", DANA, &[]));
    let outcome = f.writer.send(&state("Resolved", "Active")).await.unwrap();
    let WiOutcome::Conflict(conflict) = outcome else {
        panic!("conflict: {outcome:?}");
    };
    assert_eq!(conflict.by_name, "Dana Ruiz");
    assert_eq!(conflict.theirs.as_deref(), Some("Closed"));
    assert_eq!(conflict.server_rev, 8);
    assert_eq!(f.mock.sent().len(), 2, "nothing resent");
    assert_eq!(f.stored().0.state, "Closed");

    f.mock.sent.lock().clear();
    f.mock.answer(200, item(9, "Resolved", "me", &[]));
    f.writer
        .send(&WiWrite {
            id: 42,
            base_rev: conflict.server_rev,
            change: conflict.rebased,
            extra: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(
        f.mock.sent()[0].body().unwrap(),
        &json!([
            {"op": "test", "path": "/rev", "value": 8},
            {"op": "add", "path": "/fields/System.State", "value": "Resolved"}
        ])
    );
}

/// AC-7: a rule error at the same rev names the field; with allowed values
/// it's a list, and the chosen value goes in the same PATCH as the state;
/// a field of another type is Unknown (Open in Azure DevOps only).
#[tokio::test]
async fn ac7_rule_violation_names_the_field_and_resends_with_it() {
    let f = Fixture::new().await;
    let rule = "TF401320: Rule Error for field Root cause. Error code: Required, HasValues, LimitedToValues, AllowsOldValue, InvalidEmpty.";
    f.mock.answer(400, json!({"message": rule}));
    f.mock.answer(200, item(7, "Active", "me", &[]));
    f.mock.answer(
        200,
        json!({"count": 2, "value": [
            {"referenceName": "Custom.RootCause", "name": "Root cause", "allowedValues": ["Configuration", "Code"]},
            {"referenceName": "Custom.FixedOn", "name": "Fixed on", "allowedValues": []}
        ]}),
    );
    let outcome = f.writer.send(&state("Resolved", "Active")).await.unwrap();
    let WiOutcome::Rule(violation) = outcome else {
        panic!("rule: {outcome:?}");
    };
    assert_eq!(violation.field, "Root cause");
    assert_eq!(violation.reference.as_deref(), Some("Custom.RootCause"));
    assert_eq!(
        violation.kind,
        FieldKind::List(vec!["Configuration".into(), "Code".into()])
    );
    assert_eq!(f.stored().0.state, "Active", "the chip reverts");
    assert_eq!(
        f.mock.lines()[2],
        "GET /contoso/p1/_apis/wit/workitemtypes/Bug/fields"
    );

    f.mock.sent.lock().clear();
    f.mock.answer(200, item(8, "Resolved", "me", &[]));
    let mut resend = state("Resolved", "Active");
    resend.extra = vec![("Custom.RootCause".into(), json!("Configuration"))];
    f.writer.send(&resend).await.unwrap();
    assert_eq!(f.mock.sent().len(), 1, "one PATCH");
    assert_eq!(
        f.mock.sent()[0].body().unwrap()[2],
        json!({"op": "add", "path": "/fields/Custom.RootCause", "value": "Configuration"})
    );

    // A date field: the fields list is cached now; its definition says
    // what it is.
    f.mock.sent.lock().clear();
    let rule = "TF401320: Rule Error for field Fixed on. Error code: Required.";
    f.mock.answer(400, json!({"message": rule}));
    f.mock.answer(200, item(8, "Resolved", "me", &[]));
    f.mock.answer(
        200,
        json!({"referenceName": "Custom.FixedOn", "name": "Fixed on", "type": "dateTime"}),
    );
    let mut close = state("Closed", "Resolved");
    close.base_rev = 8;
    let WiOutcome::Rule(violation) = f.writer.send(&close).await.unwrap() else {
        panic!("rule");
    };
    assert_eq!(violation.kind, FieldKind::Unknown);
    assert_eq!(
        f.mock.lines()[2],
        "GET /contoso/_apis/wit/fields/Custom.FixedOn"
    );
}

/// AC-8: 401 on the PATCH through the optional work item transport means
/// a missing `vso.work_write`: the token isn't rejected and sync goes on.
#[tokio::test]
async fn ac8_401_on_a_write_is_a_missing_scope_never_a_rejected_token() {
    let f = Fixture::new().await;
    f.mock.answer(401, json!({}));
    let error = f
        .writer
        .send(&state("Resolved", "Active"))
        .await
        .unwrap_err();
    assert_eq!(error, WiError::MissingScope);
    assert!(!f.gov.is_unauthorized());
    // The next pass still runs (it would fail fast if the token were latched).
    f.mock.answer(200, json!({"value": []}));
    let origin = Origin::Sync {
        lane: Lane::WorkItems,
        pinned: false,
        sync_now: false,
    };
    let pass = with_origin(
        origin,
        f.gov.send(
            f.mock.as_ref(),
            Tier::Warm,
            ApiRequest::get(
                url::Url::parse(&format!("{ORG}_apis/wit/workitems?ids=1&api-version=7.1"))
                    .unwrap(),
            ),
        ),
    )
    .await;
    assert!(pass.is_ok());
}

/// AC-9 (request side): with `Retry-After: 30` from the host, the write
/// reaches Azure DevOps only after the window.
#[tokio::test]
async fn ac9_a_write_waits_for_retry_after() {
    let f = Fixture::new().await;
    f.mock.answer(200, item(7, "Active", "me", &[]));
    f.mock.script.lock().push_back(Reply {
        status: 200,
        body: json!({"comments": []}),
        rate: RateHeaders {
            retry_after_ms: Some(30 * SECOND),
            ..RateHeaders::default()
        },
        hang: None,
    });
    f.writer.sync_item(42).await.unwrap();
    let paused_at = f.clock.now_ms();
    f.mock.answer(200, item(8, "Resolved", "me", &[]));
    f.writer.send(&state("Resolved", "Active")).await.unwrap();
    let sent = f.mock.sent.lock();
    let (patched_at, patch) = sent.last().unwrap();
    assert_eq!(patch.method().as_str(), "PATCH");
    assert!(*patched_at >= paused_at + 30 * SECOND - 1);
}

/// AC-11 (send side): an empty title sends nothing; a change sends the
/// title.
#[tokio::test]
async fn ac11_title_empty_is_refused_locally() {
    let f = Fixture::new().await;
    let title = |to: &str| WiWrite {
        id: 42,
        base_rev: 7,
        change: WiChange::Title {
            to: to.into(),
            base: "Timezone bug in daily settlement job".into(),
        },
        extra: Vec::new(),
    };
    assert_eq!(
        f.writer.send(&title("  ")).await.unwrap_err(),
        WiError::Invalid("Title can't be empty".into())
    );
    assert!(f.mock.sent().is_empty());
    f.mock.answer(200, item(8, "Active", "me", &[]));
    f.writer
        .send(&title("Timezone bug in settlement"))
        .await
        .unwrap();
    assert_eq!(
        f.mock.sent()[0].body().unwrap()[1],
        json!({"op": "add", "path": "/fields/System.Title", "value": "Timezone bug in settlement"})
    );
}

/// AC-4 (send side): the distinct name, and `""` to unassign.
#[tokio::test]
async fn ac4_assign_sends_the_distinct_name_and_unassign_sends_empty() {
    let f = Fixture::new().await;
    f.mock.answer(200, item(8, "Active", "me", &[]));
    f.mock.answer(200, item(9, "Active", "me", &[]));
    let assign = |to: Option<Person>, rev| WiWrite {
        id: 42,
        base_rev: rev,
        change: WiChange::Assign {
            to,
            base: Some("me".into()),
        },
        extra: Vec::new(),
    };
    f.writer
        .send(&assign(
            Some(person(DANA, "Dana Ruiz", "dana@contoso.com")),
            7,
        ))
        .await
        .unwrap();
    f.writer.send(&assign(None, 8)).await.unwrap();
    let values: Vec<Value> = f
        .mock
        .sent()
        .iter()
        .map(|r| r.body().unwrap()[1]["value"].clone())
        .collect();
    assert_eq!(values, [json!("Dana Ruiz <dana@contoso.com>"), json!("")]);
}

/// AC-3 (fetch and cache): one type GET fills the cache; the cache is
/// fresh for 24 h, then stale (paint from cache, refresh in background).
#[tokio::test]
async fn ac3_type_metadata_is_fetched_once_and_cached_for_a_day() {
    let f = Fixture::new().await;
    f.mock.answer(
        200,
        json!({
            "name": "Bug",
            "states": [
                {"name": "New", "color": "b2b2b2", "category": "Proposed"},
                {"name": "Active", "color": "007acc", "category": "InProgress"},
                {"name": "Resolved", "color": "ff9d00", "category": "Resolved"},
                {"name": "Closed", "color": "339933", "category": "Completed"}
            ],
            "transitions": {
                "": [{"to": "New"}],
                "Active": [{"to": "Active"}, {"to": "Closed"}, {"to": "Resolved"}, {"to": "New"}]
            }
        }),
    );
    f.writer
        .type_meta(
            "p1",
            "Bug",
            adoken_sync::attribution::UserAction::ChangeState,
        )
        .await
        .unwrap();
    assert_eq!(
        f.mock.lines(),
        ["GET /contoso/p1/_apis/wit/workitemtypes/Bug"]
    );
    let cached = f
        .store
        .with_conn(|c| store_wi::type_meta(c, "p1", "Bug"))
        .unwrap()
        .unwrap();
    let menu: Vec<String> = cached.menu("Active").into_iter().map(|s| s.name).collect();
    assert_eq!(menu, ["New", "Active", "Resolved", "Closed"]);
    assert!(!is_stale(cached.fetched_ms, cached.fetched_ms + 23 * HOUR));
    assert!(is_stale(cached.fetched_ms, cached.fetched_ms + 24 * HOUR));
}

/// AC-12: one `classificationnodes?$depth=14` GET per project, cached;
/// `↩` sends the full path.
#[tokio::test]
async fn ac12_trees_come_in_one_call_and_paths_are_sent_whole() {
    let f = Fixture::new().await;
    f.mock.answer(
        200,
        json!({"count": 2, "value": [
            {"name": "Payments", "structureType": "area", "children": [{"name": "Infra", "structureType": "area"}]},
            {"name": "Payments", "structureType": "iteration", "children": [
                {"name": "Sprint 44", "structureType": "iteration", "attributes": {"startDate": "2026-09-28T00:00:00Z", "finishDate": "2026-10-09T00:00:00Z"}},
                {"name": "Sprint 45", "structureType": "iteration", "attributes": {"startDate": "2026-10-12T00:00:00Z", "finishDate": "2026-10-23T00:00:00Z"}}
            ]}
        ]}),
    );
    let tree = f
        .writer
        .class_tree("p1", adoken_sync::attribution::UserAction::ChangeArea)
        .await
        .unwrap();
    let sent = f.mock.sent();
    assert_eq!(
        f.mock.lines(),
        ["GET /contoso/p1/_apis/wit/classificationnodes"]
    );
    assert_eq!(
        query(&sent[0]),
        pairs(&[("$depth", "14"), ("api-version", "7.1")])
    );
    let cached = f
        .store
        .with_conn(|c| store_wi::class_tree(c, "p1"))
        .unwrap()
        .unwrap();
    assert_eq!(cached, tree);
    assert!(
        !is_stale(cached.fetched_ms, NOW + 8 * HOUR),
        "a second picker that day sends none"
    );
    let shortcuts = cached.shortcuts(NOW);
    assert_eq!(shortcuts.current.unwrap().path, "Payments\\Sprint 44");
    assert_eq!(shortcuts.next.unwrap().path, "Payments\\Sprint 45");

    f.mock.answer(200, item(8, "Active", "me", &[]));
    f.writer
        .send(&WiWrite {
            id: 42,
            base_rev: 7,
            change: WiChange::Area {
                to: "Payments\\Infra".into(),
                base: "Payments".into(),
            },
            extra: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(
        f.mock.sent()[1].body().unwrap()[1],
        json!({"op": "add", "path": "/fields/System.AreaPath", "value": "Payments\\Infra"})
    );
}

/// AC-13 / AC-14 (send side): a Markdown field sends its format op; an HTML
/// field sends only the (converted) HTML. The raw source and format land in
/// the store.
#[tokio::test]
async fn ac13_ac14_description_format_ops() {
    let f = Fixture::new().await;
    let md = "## Steps\n\n- open settlement\n- wait for midnight";
    let mut answer = item(8, "Active", "me", &[("System.Description", json!(md))]);
    answer["multilineFieldsFormat"] = json!({"System.Description": "Markdown"});
    f.mock.answer(200, answer);
    f.writer
        .send(&WiWrite {
            id: 42,
            base_rev: 7,
            change: WiChange::Description {
                to: md.into(),
                format: TextFormat::Markdown,
                base: None,
            },
            extra: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(
        f.mock.sent()[0].body().unwrap(),
        &json!([
            {"op": "test", "path": "/rev", "value": 7},
            {"op": "add", "path": "/fields/System.Description", "value": md},
            {"op": "add", "path": "/multilineFieldsFormat/System.Description", "value": "Markdown"}
        ])
    );
    let stored = f.stored().1.description.unwrap();
    assert_eq!(
        (stored.source.as_str(), stored.format),
        (md, TextFormat::Markdown)
    );

    let html = "<div>Total is wrong.</div><div>Added line</div>";
    f.mock.answer(
        200,
        item(9, "Active", "me", &[("System.Description", json!(html))]),
    );
    f.writer
        .send(&WiWrite {
            id: 42,
            base_rev: 8,
            change: WiChange::Description {
                to: html.into(),
                format: TextFormat::Html,
                base: Some(md.into()),
            },
            extra: Vec::new(),
        })
        .await
        .unwrap();
    let ops = f.mock.sent()[1].body().unwrap().as_array().unwrap().clone();
    assert_eq!(ops.len(), 2, "no multilineFieldsFormat op: {ops:?}");
    assert_eq!(f.stored().1.description.unwrap().format, TextFormat::Html);
}

/// AC-17 (request side): a Markdown comment with an `@<id>` mention, then
/// one item GET; stored with its source and format; only self-caused
/// events.
#[tokio::test]
async fn ac17_comment_posts_markdown_then_reads_the_item_once() {
    let f = Fixture::new().await;
    let text = format!("@<{DANA}> please verify");
    f.mock.answer(
        200,
        json!({
            "id": 501, "workItemId": 42, "text": text, "format": "markdown",
            "createdBy": identity("me"), "createdDate": "2026-09-30T12:00:00Z",
            "mentions": [{"mentionedArtifact": DANA, "type": "person"}]
        }),
    );
    f.mock.answer(
        200,
        item(8, "Active", "me", &[("System.CommentCount", json!(1))]),
    );
    let comment = f.writer.comment(42, &text).await.unwrap();
    let sent = f.mock.sent();
    assert_eq!(
        f.mock.lines(),
        [
            "POST /contoso/p1/_apis/wit/workItems/42/comments",
            "GET /contoso/p1/_apis/wit/workitems/42"
        ]
    );
    assert_eq!(
        query(&sent[0]),
        pairs(&[("format", "markdown"), ("api-version", "7.1-preview.4")])
    );
    assert_eq!(sent[0].body().unwrap(), &json!({"text": text}));
    assert_eq!(comment.format, TextFormat::Markdown);
    assert_eq!(comment.source.as_deref(), Some(text.as_str()));
    assert_eq!(f.stored().0.rev, 8);
    let events = f.store.sync_events_since(0, 100).unwrap();
    assert!(events.iter().all(|e| e.self_caused), "{events:#?}");
    assert_eq!(
        f.origins()[f.origins().len() - 2..],
        ["You: comment", "You: comment · re-read"]
    );
}

/// AC-18: Child adds `Hierarchy-Forward` with the target's URL; Change
/// parent removes the old parent (index 2) and adds the new one in one
/// PATCH.
#[tokio::test]
async fn ac18_add_link_and_change_parent_in_one_patch() {
    let f = Fixture::new().await;
    let (_, extras) = f.stored();
    assert_eq!(extras.parent(), Some(7));
    assert_eq!(extras.link(LinkKind::Parent, 7).unwrap().index, 2);
    f.mock.answer(200, item(8, "Active", "me", &[]));
    f.mock.answer(200, item(9, "Active", "me", &[]));
    f.writer
        .send(&WiWrite {
            id: 42,
            base_rev: 7,
            change: WiChange::AddLink {
                kind: LinkKind::Child,
                target: 123,
                replace_parent: None,
            },
            extra: Vec::new(),
        })
        .await
        .unwrap();
    f.writer
        .send(&WiWrite {
            id: 42,
            base_rev: 8,
            change: WiChange::AddLink {
                kind: LinkKind::Parent,
                target: 9,
                replace_parent: Some(7),
            },
            extra: Vec::new(),
        })
        .await
        .unwrap();
    let sent = f.mock.sent();
    assert_eq!(
        sent[0].body().unwrap()[1],
        json!({"op": "add", "path": "/relations/-", "value": {
            "rel": "System.LinkTypes.Hierarchy-Forward",
            "url": "https://dev.azure.com/contoso/_apis/wit/workItems/123",
            "attributes": {}
        }})
    );
    assert_eq!(
        sent[1].body().unwrap(),
        &json!([
            {"op": "test", "path": "/rev", "value": 8},
            {"op": "remove", "path": "/relations/2"},
            {"op": "add", "path": "/relations/-", "value": {
                "rel": "System.LinkTypes.Hierarchy-Reverse",
                "url": "https://dev.azure.com/contoso/_apis/wit/workItems/9",
                "attributes": {}
            }}
        ])
    );
    assert_eq!(f.origins().last().unwrap(), "You: change parent");
}

/// AC-19: remove the Related link by its stored index; when the item moved
/// on and the link is now at index 2, resend once with index 2 and rev 8.
#[tokio::test]
async fn ac19_remove_link_follows_a_shifted_index_once() {
    let f = Fixture::new().await;
    let remove = WiWrite {
        id: 42,
        base_rev: 7,
        change: WiChange::RemoveLink {
            kind: LinkKind::Related,
            target: 9,
        },
        extra: Vec::new(),
    };
    f.mock
        .answer(400, json!({"message": "TF26071: changed by someone else"}));
    let mut moved = item(8, "Active", DANA, &[]);
    let mut rels = relations(&[]).as_array().unwrap().clone();
    rels.remove(1); // the commit link went: Related is now at 2
    moved["relations"] = Value::Array(rels);
    f.mock.answer(200, moved);
    f.mock.answer(200, item(9, "Active", "me", &[]));
    f.writer.send(&remove).await.unwrap();
    let sent = f.mock.sent();
    assert_eq!(
        sent[0].body().unwrap(),
        &json!([{"op": "test", "path": "/rev", "value": 7}, {"op": "remove", "path": "/relations/3"}])
    );
    assert_eq!(
        sent[2].body().unwrap(),
        &json!([{"op": "test", "path": "/rev", "value": 8}, {"op": "remove", "path": "/relations/2"}])
    );
    assert_eq!(sent.len(), 3);
}

/// AC-20: linking to itself, a child as parent, or an existing Related
/// link is refused locally with the reason.
#[tokio::test]
async fn ac20_local_link_guards() {
    let f = Fixture::new().await;
    // #43 is a child of #42.
    f.store
        .write(|tx| {
            let mut child = f.stored().0;
            child.id = 43;
            child.parent_id = Some(42);
            write::upsert_work_item(tx, &child)?;
            store_wi::record_answer(tx, 43, child.rev, NOW, Some(&FullFields::default()))
        })
        .unwrap();
    let (item, extras) = f.stored();
    let guard = |kind, target| {
        f.store
            .with_conn(|c| {
                Ok(link_guard(
                    c,
                    &item,
                    &extras,
                    &WiChange::AddLink {
                        kind,
                        target,
                        replace_parent: extras.parent(),
                    },
                ))
            })
            .unwrap()
    };
    assert!(guard(LinkKind::Related, 42).unwrap_err().contains("itself"));
    assert!(guard(LinkKind::Parent, 43).unwrap_err().contains("loop"));
    assert!(
        guard(LinkKind::Related, 9)
            .unwrap_err()
            .contains("already linked")
    );
    assert!(guard(LinkKind::Related, 77).is_ok());
    assert!(f.mock.sent().is_empty());
}

/// AC-23: with the 3 sync slots busy the item GET goes out at once (the
/// user's slot), then the comments; it's a "You: Sync #42" pass; a 404
/// keeps the stored data.
#[tokio::test]
async fn ac23_sync_this_item_uses_the_user_slot_and_records_a_pass() {
    let f = Fixture::new().await;
    let tracker = Arc::new(Tracker::new(f.clock.clone()));
    let writer = f.writer.clone().with_tracker(tracker.clone());
    let release = Arc::new(Notify::new());
    for _ in 0..3 {
        f.mock.script.lock().push_back(Reply {
            status: 200,
            body: json!({"value": []}),
            rate: RateHeaders::default(),
            hang: Some(release.clone()),
        });
    }
    let origin = Origin::Sync {
        lane: Lane::WorkItems,
        pinned: false,
        sync_now: false,
    };
    let mut busy = Vec::new();
    for _ in 0..3 {
        let (gov, mock) = (f.gov.clone(), f.mock.clone());
        busy.push(tokio::spawn(with_origin(origin, async move {
            let url = url::Url::parse(&format!("{ORG}_apis/wit/workitems?ids=1&api-version=7.1"))
                .unwrap();
            gov.send(mock.as_ref(), Tier::Warm, ApiRequest::get(url))
                .await
        })));
    }
    while f.mock.sent().len() < 3 {
        tokio::task::yield_now().await;
    }
    f.mock.answer(200, item(8, "Active", DANA, &[]));
    f.mock.answer(200, json!({"comments": []}));
    let synced = writer.sync_item(42).await.unwrap();
    assert_eq!(synced.item.unwrap().rev, 8);
    let lines = f.mock.lines();
    assert_eq!(
        lines[3..],
        [
            "GET /contoso/p1/_apis/wit/workitems/42",
            "GET /contoso/p1/_apis/wit/workItems/42/comments"
        ]
    );
    let passes = tracker.passes();
    assert_eq!(passes[0].trigger, Trigger::ItemSync(42));
    release.notify_waiters();
    for b in busy {
        b.await.unwrap().unwrap();
    }

    f.mock.answer(
        404,
        json!({"message": "TF401232: Work item 42 does not exist"}),
    );
    assert_eq!(writer.sync_item(42).await.unwrap_err(), WiError::Gone);
    assert_eq!(f.stored().0.rev, 8, "the last data is kept");
}

/// FR-37: the demo org applies writes locally: a new revision by me through
/// the real apply path (self-caused events), links and comments included.
#[tokio::test]
async fn demo_writes_apply_locally_with_the_same_effects() {
    let f = Fixture::new().await;
    let applied = apply_local(&f.store, &state("Resolved", "Active"), NOW + MINUTE).unwrap();
    assert_eq!((applied.rev, applied.state.as_str()), (8, "Resolved"));
    assert_eq!(applied.changed_by, "me");
    apply_local(
        &f.store,
        &WiWrite {
            id: 42,
            base_rev: 8,
            change: WiChange::AddLink {
                kind: LinkKind::Parent,
                target: 11,
                replace_parent: Some(7),
            },
            extra: Vec::new(),
        },
        NOW + 2 * MINUTE,
    )
    .unwrap();
    let (stored, extras) = f.stored();
    assert_eq!((stored.parent_id, extras.parent()), (Some(11), Some(11)));
    assert!(
        extras.link(LinkKind::Related, 9).is_some(),
        "other links kept"
    );
    let comment = comment_local(&f.store, 42, "Looks good", NOW + 3 * MINUTE).unwrap();
    assert_eq!(comment.format, TextFormat::Markdown);
    assert_eq!(f.stored().0.comment_count, 1);
    let events = f.store.sync_events_since(0, 100).unwrap();
    assert!(events.iter().all(|e| e.self_caused));
    assert!(f.mock.sent().is_empty(), "nothing on the network");
}
