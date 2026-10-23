use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;

use adoken_model::*;
use adoken_store::write;
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::{ApiRequest, Governor, ManualClock, Transport};
use serde_json::{Value, json};

use super::*;

const T: Millis = 1_790_769_600_000;

fn person(id: &str, name: &str) -> Person {
    Person {
        id: id.into(),
        display_name: name.into(),
        unique_name: Some(format!("{id}@contoso.com")),
        is_group: false,
        image_url: None,
    }
}

fn state_write(base_rev: i64, to: &str, base: &str) -> WiWrite {
    WiWrite {
        id: 42,
        base_rev,
        change: WiChange::State {
            to: to.into(),
            base: base.into(),
        },
        extra: Vec::new(),
    }
}

fn assign_write(base_rev: i64) -> WiWrite {
    WiWrite {
        id: 42,
        base_rev,
        change: WiChange::Assign {
            to: Some(person("dana", "Dana Ruiz")),
            base: Some("me".into()),
        },
        extra: Vec::new(),
    }
}

fn applied() -> Result<WiOutcome, WiError> {
    Ok(WiOutcome::Applied(None))
}

/// FR-10 (state machine): a second edit of the item waits for the first's
/// answer; another item isn't held up.
#[test]
fn edits_of_one_item_run_one_at_a_time_in_order() {
    let mut e = Edits::default();
    let first = e.submit(state_write(7, "Resolved", "Active"), T);
    assert!(matches!(first, Some(Job::Write(_))));
    assert_eq!(e.submit(assign_write(7), T + 30), None, "queued, not sent");
    assert!(e.status(42, Field::AssignedTo).unwrap().is_sending());
    let mut other = state_write(1, "Closed", "Active");
    other.id = 7;
    assert!(
        e.submit(other, T + 40).is_some(),
        "another item runs in parallel"
    );
    let next = e.answered(
        42,
        Answer::Write(state_write(7, "Resolved", "Active"), applied()),
    );
    assert!(matches!(next, Some(Job::Write(w)) if w.change.field() == Field::AssignedTo));
    assert_eq!(
        e.status(42, Field::State),
        None,
        "confirmed: the pending line goes"
    );
    assert!(e.in_flight(42));
}

/// AC-8 (controller): a missing-scope answer switches the item to
/// read-only, fails what was queued behind it without sending, and keeps
/// every value as a Retry.
#[test]
fn ac8_missing_scope_flips_the_item_read_only_and_keeps_retries() {
    let mut e = Edits::default();
    e.submit(state_write(7, "Resolved", "Active"), T);
    e.submit(assign_write(7), T);
    let (local, _) = e.submit_comment(42, "please verify".into(), T);
    let next = e.answered(
        42,
        Answer::Write(
            state_write(7, "Resolved", "Active"),
            Err(WiError::MissingScope),
        ),
    );
    assert_eq!(next, None, "nothing else is sent");
    assert!(e.is_read_only(42));
    for field in [Field::State, Field::AssignedTo] {
        assert_eq!(
            e.status(42, field).and_then(EditStatus::failure),
            Some(&WiError::MissingScope)
        );
    }
    assert!(matches!(
        e.pending_comments(42)[0].status,
        CommentStatus::Failed { .. }
    ));
    // After a replacement: Retry sends my value again.
    assert!(
        matches!(e.retry(42, Field::State, T + 5), Some(Job::Write(w)) if w.change == state_write(7, "Resolved", "Active").change)
    );
    assert_eq!(e.take_comment(42, local).as_deref(), Some("please verify"));
}

fn conflict() -> Conflict {
    Conflict {
        server_rev: 8,
        theirs: Some("Closed".into()),
        by: "dana".into(),
        by_name: "Dana Ruiz".into(),
        at: T - MINUTE,
        rebased: WiChange::State {
            to: "Resolved".into(),
            base: "Closed".into(),
        },
    }
}

/// AC-6 (controller): Apply mine resends my value on their revision;
/// Keep theirs sends nothing and clears the notice.
#[test]
fn ac6_apply_mine_resends_on_their_revision_and_keep_theirs_sends_nothing() {
    let mut e = Edits::default();
    let write = state_write(7, "Resolved", "Active");
    e.submit(write.clone(), T);
    e.answered(
        42,
        Answer::Write(write.clone(), Ok(WiOutcome::Conflict(conflict()))),
    );
    let shown = e.status(42, Field::State).unwrap();
    assert_eq!(shown.conflict().unwrap().by_name, "Dana Ruiz");
    assert_eq!(shown.value(), "Resolved", "my value is kept until I choose");
    let Some(Job::Write(resent)) = e.apply_mine(42, Field::State, T) else {
        panic!("Apply mine sends");
    };
    assert_eq!(resent.base_rev, 8);
    assert_eq!(
        resent.change,
        WiChange::State {
            to: "Resolved".into(),
            base: "Closed".into()
        }
    );

    let mut e = Edits::default();
    e.submit(write.clone(), T);
    e.answered(
        42,
        Answer::Write(write, Ok(WiOutcome::Conflict(conflict()))),
    );
    e.dismiss(42, Field::State);
    assert_eq!(e.status(42, Field::State), None);
    assert!(!e.in_flight(42));
}

/// AC-7 (controller): choosing the value a rule asked for resends the
/// original change with that field, on the server's revision.
#[test]
fn ac7_rule_value_resends_the_change_with_the_field() {
    let mut e = Edits::default();
    let write = state_write(7, "Resolved", "Active");
    e.submit(write.clone(), T);
    let rule = RuleViolation {
        field: "Root cause".into(),
        reference: Some("Custom.RootCause".into()),
        kind: FieldKind::List(vec!["Configuration".into(), "Code".into()]),
        message: "TF401320: Rule Error for field Root cause. Error code: Required.".into(),
        server_rev: 7,
    };
    e.answered(42, Answer::Write(write, Ok(WiOutcome::Rule(rule.clone()))));
    assert_eq!(e.status(42, Field::State).unwrap().rule(), Some(&rule));
    let Some(Job::Write(resent)) = e.resend_with_rule(42, Field::State, json!("Configuration"), T)
    else {
        panic!("resend");
    };
    assert_eq!(
        resent.extra,
        [("Custom.RootCause".to_owned(), json!("Configuration"))]
    );
    // A field adoken can't name can't be sent: the notice stays.
    let mut e = Edits::default();
    let write = state_write(7, "Resolved", "Active");
    e.submit(write.clone(), T);
    let unknown = RuleViolation {
        reference: None,
        kind: FieldKind::Unknown,
        ..rule
    };
    e.answered(42, Answer::Write(write, Ok(WiOutcome::Rule(unknown))));
    assert_eq!(e.resend_with_rule(42, Field::State, json!("x"), T), None);
    assert!(e.status(42, Field::State).unwrap().rule().is_some());
}

/// AC-9 (controller): pending reads "Waiting for Azure DevOps…" after 3 s.
#[test]
fn ac9_pending_turns_waiting_after_three_seconds() {
    let mut e = Edits::default();
    e.submit(state_write(7, "Resolved", "Active"), T);
    let s = e.status(42, Field::State).unwrap();
    assert!(!s.waiting(T + 2_999));
    assert!(s.waiting(T + 3_000));
}

/// AC-23 / AC-24 (controller): a second press within 10 s sends nothing;
/// passing over items sends nothing; resting on a partly synced one sends
/// once per 10 minutes; never offline or in the demo org.
#[test]
fn ac23_ac24_item_sync_debounce_and_auto_fetch() {
    let mut s = ItemSyncs::default();
    assert_eq!(s.press(42, T, false, false), SyncPress::Sent);
    assert_eq!(s.press(42, T + 100, false, false), SyncPress::Busy);
    s.done(42, &Ok(()), T + 400);
    assert_eq!(s.press(42, T + 5_000, false, false), SyncPress::JustSynced);
    assert_eq!(s.press(42, T + 10_400, false, false), SyncPress::Sent);
    s.done(42, &Err(WiError::Gone), T + 10_600);
    assert!(s.is_gone(42), "404: deleted / lost access");
    assert!(matches!(s.press(1, T, true, false), SyncPress::Disabled(_)));
    assert!(matches!(s.press(1, T, false, true), SyncPress::Disabled(_)));

    // Arrowing past five items calls `rested` for none of them; resting on
    // one sends once, and not again within 10 minutes.
    let mut s = ItemSyncs::default();
    assert!(s.rested(7, true, T, false, false));
    s.done(7, &Ok(()), T + 300);
    assert!(!s.rested(7, true, T + 9 * MINUTE, false, false));
    assert!(s.rested(7, true, T + 10 * MINUTE, false, false));
    assert!(
        !s.rested(8, false, T, false, false),
        "fully synced: nothing"
    );
    assert!(!s.rested(9, true, T, true, false), "offline: nothing");
}

fn label(input: &FreshnessInput, now: Millis) -> Freshness {
    freshness_with(
        input,
        &FreshCtx {
            now,
            offline: false,
            demo: false,
            sync: &ItemSyncState::Idle,
            lane_every: Some(2 * MINUTE),
            offset: ::time::UtcOffset::UTC,
        },
    )
}

/// AC-22: the later of the item's fetch and the covering lane's pass;
/// stale at 15 min; partly synced; full timestamp and lane in the tooltip.
#[test]
fn ac22_freshness_labels() {
    let mine = FreshnessInput {
        synced_ms: Some(T),
        full_synced_ms: Some(T),
        coverage: Coverage::Mine,
        lane_last_ok: Some(T + 5 * MINUTE),
    };
    let f = label(&mine, T + 6 * MINUTE);
    assert_eq!(
        (f.kind, f.label.as_str()),
        (FreshKind::Fresh, "Synced 1m ago")
    );
    assert_eq!(
        f.tooltip,
        "Last confirmed 12:05:00 · refreshes every 2 min with Work items · Sync this work item  ⌘⇧R"
    );
    assert_eq!(label(&mine, T + 5 * MINUTE + 10).label, "Synced just now");

    let uncovered = FreshnessInput {
        coverage: Coverage::None,
        ..mine.clone()
    };
    let f = label(&uncovered, T + 15 * MINUTE);
    assert_eq!(
        (f.kind, f.label.as_str()),
        (FreshKind::Stale, "Updated 15m ago")
    );
    assert!(
        f.tooltip
            .starts_with("Last confirmed 12:00:00 · only when you sync it")
    );

    let partly = FreshnessInput {
        synced_ms: Some(T),
        full_synced_ms: None,
        coverage: Coverage::None,
        lane_last_ok: None,
    };
    assert_eq!(label(&partly, T).kind, FreshKind::Partly);
    assert_eq!(label(&partly, T).label, "Partly synced");

    let offline = freshness_with(
        &mine,
        &FreshCtx {
            now: T + 2 * HOUR,
            offline: true,
            demo: false,
            sync: &ItemSyncState::Idle,
            lane_every: Some(2 * MINUTE),
            offset: ::time::UtcOffset::UTC,
        },
    );
    assert_eq!(offline.label, "Offline · synced 1h ago");
    let syncing = ItemSyncState::Syncing { since: T };
    let waiting = freshness_with(
        &mine,
        &FreshCtx {
            now: T + 3 * SECOND,
            offline: false,
            demo: false,
            sync: &syncing,
            lane_every: Some(2 * MINUTE),
            offset: ::time::UtcOffset::UTC,
        },
    );
    assert_eq!(waiting.label, "Waiting for Azure DevOps…");
}

/// The tooltip's time is local: +10:00 turns 12:05 UTC on Sep 30 into
/// 22:05, and a confirmation late the previous local day gets its date.
#[test]
fn freshness_timestamps_are_local_time() {
    let plus_ten = ::time::UtcOffset::from_hms(10, 0, 0).unwrap();
    assert_eq!(
        timestamp(T + 5 * MINUTE, T + 6 * MINUTE, plus_ten),
        "22:05:00"
    );
    assert_eq!(
        timestamp(T + 5 * MINUTE, T + 6 * MINUTE, ::time::UtcOffset::UTC),
        "12:05:00"
    );
    // 13:30 UTC is 23:30 local; 14:30 UTC is already the next local day.
    assert_eq!(
        timestamp(T + 90 * MINUTE, T + 150 * MINUTE, plus_ten),
        "Sep 30 23:30:00"
    );
}

#[test]
fn assignable_people_exclude_groups_and_unknown_sign_ins() {
    let mut group = person("g", "[Payments]\\Team");
    group.is_group = true;
    let mut nameless = person("n", "No Sign-in");
    nameless.unique_name = None;
    let people = assignable([person("dana", "Dana Ruiz"), group, nameless]);
    assert_eq!(people.len(), 1);
}

#[test]
fn fallback_tree_keeps_ancestors_of_seen_paths() {
    let tree = fallback_tree(
        ["Payments\\Infra\\Settlement", "Payments"],
        ["Payments\\Sprint 44"],
    );
    assert_eq!(
        tree.areas
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["Payments", "Payments\\Infra", "Payments\\Infra\\Settlement"]
    );
    assert_eq!(tree.iterations[1].parent_path(), Some("Payments"));
}

// ── AC-10 end to end through the writer ──────────────────────────────────

/// Records requests and the most PATCHes ever in flight at once.
#[derive(Default)]
struct Mock {
    sent: Mutex<Vec<ApiRequest>>,
    script: Mutex<VecDeque<(u16, Value)>>,
    in_flight: Mutex<(usize, usize)>,
}

impl Transport for Mock {
    fn send(
        &self,
        req: ApiRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, TransportError>> + Send + '_>> {
        self.sent.lock().push(req);
        let (status, body) = self.script.lock().pop_front().expect("scripted");
        Box::pin(async move {
            {
                let mut f = self.in_flight.lock();
                f.0 += 1;
                f.1 = f.1.max(f.0);
            }
            tokio::task::yield_now().await;
            self.in_flight.lock().0 -= 1;
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

fn item_json(rev: i64, state: &str, assigned: &str) -> Value {
    json!({
        "id": 42, "rev": rev,
        "fields": {
            "System.TeamProject": "Payments", "System.WorkItemType": "Bug",
            "System.Title": "Timezone bug", "System.State": state,
            "System.AssignedTo": {"id": assigned, "displayName": assigned, "uniqueName": format!("{assigned}@contoso.com")},
            "System.CreatedBy": {"id": "me", "displayName": "Ada Me"},
            "System.ChangedBy": {"id": "me", "displayName": "Ada Me"},
            "System.AreaPath": "Payments", "System.IterationPath": "Payments",
            "System.CreatedDate": "2026-09-01T00:00:00Z", "System.ChangedDate": "2026-09-30T12:00:00Z"
        },
        "relations": []
    })
}

/// AC-10: state, then assignee within 50 ms: the second PATCH goes out only
/// after the first's 200, with `test /rev` = that answer's rev; never two
/// PATCHes for #42 in flight.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac10_second_edit_waits_for_the_first_answer_and_uses_its_rev() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
    store
        .write(|tx| {
            write::upsert_person(tx, &person("me", "Ada Me"), true)?;
            write::upsert_project(
                tx,
                &Project {
                    id: "p1".into(),
                    name: "Payments".into(),
                },
            )
        })
        .unwrap();
    let mock = Arc::new(Mock::default());
    mock.script.lock().extend([
        (200, item_json(8, "Resolved", "me")),
        (200, item_json(9, "Resolved", "dana")),
    ]);
    let clock = Arc::new(ManualClock::new(T));
    let writer = adoken_sync::WiWriter::new(
        store.clone(),
        mock.clone(),
        Arc::new(Governor::new(clock.clone())),
        clock,
        url::Url::parse("https://dev.azure.com/contoso/").unwrap(),
    );
    store
        .write(|tx| {
            write::upsert_work_item(
                tx,
                &WorkItem {
                    id: 42,
                    project_id: "p1".into(),
                    rev: 7,
                    kind: WorkItemType::Bug,
                    title: "Timezone bug".into(),
                    state: "Active".into(),
                    state_category: StateCategory::InProgress,
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
                    created: T - DAY,
                    changed: T - HOUR,
                },
            )
        })
        .unwrap();

    let mut e = Edits::default();
    let mut job = e.submit(state_write(7, "Resolved", "Active"), T);
    assert_eq!(e.submit(assign_write(7), T + 50), None);
    while let Some(Job::Write(w)) = job {
        let result = writer.send(&w).await;
        job = e.answered(42, Answer::Write(w, result));
    }
    let sent = mock.sent.lock().clone();
    assert_eq!(sent.len(), 2);
    let rev = |r: &ApiRequest| r.body().unwrap()[0]["value"].as_i64().unwrap();
    assert_eq!((rev(&sent[0]), rev(&sent[1])), (7, 8));
    assert_eq!(mock.in_flight.lock().1, 1);
    assert_eq!(e.status(42, Field::AssignedTo), None);
}
