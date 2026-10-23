use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use adoken_model::time::{DAY, HOUR};
use adoken_model::{ClassNode, ClassTree, Millis, NodeKind, WorkItem};
use adoken_store::Store;
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::{ApiRequest, Governor, ManualClock, NetLog, Recording, Tier, Transport};
use futures::future::BoxFuture;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::story::{P1, P2, item};
use super::*;

/// 2026-09-30 12:00 UTC.
const NOW: Millis = 1_790_769_600_000;

fn area(path: &str) -> ClassNode {
    ClassNode {
        kind: NodeKind::Area,
        path: path.into(),
        name: path.rsplit('\\').next().unwrap().into(),
        depth: path.matches('\\').count() as u16,
        start_ms: None,
        finish_ms: None,
    }
}

/// P1's stored area tree: the root, 3 groups × 3 teams × 3 leaves = 40
/// nodes, pre-order.
fn p1_tree(fetched_ms: Millis) -> ClassTree {
    let mut areas = vec![area("P1")];
    for group in ["Infra", "Web", "Data"] {
        areas.push(area(&format!("P1\\{group}")));
        for team in 1..=3 {
            areas.push(area(&format!("P1\\{group}\\Team {team}")));
            for leaf in ["Api", "Jobs", "Ui"] {
                areas.push(area(&format!("P1\\{group}\\Team {team}\\{leaf}")));
            }
        }
    }
    ClassTree {
        areas,
        iterations: Vec::new(),
        fetched_ms,
    }
}

fn open(id: i64, project: &str, area: &str) -> WorkItem {
    item(
        id,
        project,
        area,
        &format!("{project}\\Sprint 1"),
        "Active",
        NOW - HOUR,
    )
}

/// AC-9's loaded items: 6 of P1's stored nodes carry open items, one P1
/// item sits on a path the stored tree doesn't have, P2 has open items on
/// 3 paths, and a closed item counts nowhere.
fn ac9_items() -> Vec<WorkItem> {
    let mut items = vec![
        open(1, P1, "P1\\Infra"),
        open(2, P1, "P1\\Infra\\Team 1"),
        open(3, P1, "P1\\Infra\\Team 1\\Api"),
        open(4, P1, "P1\\Infra\\Team 1\\Api"),
        open(5, P1, "p1\\infra\\team 2\\JOBS"),
        open(6, P1, "P1\\Web\\Team 3"),
        open(7, P1, "P1\\Data"),
        open(8, P1, "P1\\Infra\\Edge"),
        open(20, P2, "P2\\Ledger"),
        open(21, P2, "P2\\Ledger\\Close"),
        open(22, P2, "P2\\Payouts"),
    ];
    let mut closed = open(9, P1, "P1\\Web\\Team 1");
    closed.state = "Closed".into();
    closed.state_category = adoken_model::StateCategory::Completed;
    items.push(closed);
    items
}

fn names() -> HashMap<String, String> {
    HashMap::from([(P1.into(), "P1".into()), (P2.into(), "P2".into())])
}

fn stored(trees: &[(&str, ClassTree)]) -> HashMap<String, Arc<ClassTree>> {
    trees
        .iter()
        .map(|(p, t)| ((*p).to_owned(), Arc::new(t.clone())))
        .collect()
}

fn count_of(tree: &PathTree, path: &str) -> u32 {
    tree.counts[tree.find(path).unwrap_or_else(|| panic!("{path} listed"))]
}

/// AC-9: two roots; P1 lists all 40 stored nodes with counts (0 where
/// nothing is open) plus the item path missing from its tree; P2 (no
/// stored tree) lists its 3 loaded paths and their ancestors.
#[test]
fn ac9_dropdown_merges_stored_trees_with_loaded_paths_and_counts_below() {
    let seen = Seen::gather(&ac9_items(), &names(), false);
    let tree = PathTree::build(Tab::Area, &seen, &stored(&[(P1, p1_tree(NOW))]));
    let model = &tree.model;
    assert_eq!(model.root_count(), 2);
    let roots: Vec<String> = (0..model.len())
        .filter(|&n| model.parent(n).is_none())
        .map(|n| model.path(n))
        .collect();
    assert_eq!(roots, ["P1", "P2"]);
    let in_p1 = |n: &usize| model.nodes()[model.root(*n)].name.as_ref() == "P1";
    assert_eq!(
        (0..model.len()).filter(in_p1).count(),
        41,
        "40 stored + Edge"
    );
    for node in &p1_tree(NOW).areas {
        assert!(tree.find(&node.path).is_some(), "{} listed", node.path);
    }
    // Counts: at or below, case-insensitive paths, closed items left out.
    assert_eq!(count_of(&tree, "P1"), 8);
    assert_eq!(count_of(&tree, "P1\\Infra"), 6);
    assert_eq!(count_of(&tree, "P1\\Infra\\Team 1"), 3);
    assert_eq!(count_of(&tree, "P1\\Infra\\Team 1\\Api"), 2);
    assert_eq!(count_of(&tree, "P1\\Infra\\Team 2\\Jobs"), 1);
    assert_eq!(count_of(&tree, "P1\\Web\\Team 1"), 0, "nothing open: 0");
    assert_eq!(
        count_of(&tree, "P1\\Infra\\Edge"),
        1,
        "not in the stored tree"
    );
    let edge = tree.find("P1\\Infra\\Edge").unwrap();
    assert_eq!(model.path(model.parent(edge).unwrap()), "P1\\Infra");
    // P2: its loaded paths only.
    let mut p2: Vec<String> = (0..model.len())
        .filter(|n| !in_p1(n))
        .map(|n| model.path(n))
        .collect();
    p2.sort();
    assert_eq!(p2, ["P2", "P2\\Ledger", "P2\\Ledger\\Close", "P2\\Payouts"]);
    assert_eq!(count_of(&tree, "P2"), 3);
    assert_eq!(tree.partial, [P2]);
    assert!(tree.has_children("p1\\infra") && !tree.has_children("P1\\Infra\\Edge"));
}

#[test]
fn a_project_with_only_closed_items_still_gets_its_root() {
    let mut closed = open(1, P1, "P1\\Web");
    closed.state = "Closed".into();
    closed.state_category = adoken_model::StateCategory::Completed;
    let seen = Seen::gather(&[closed], &names(), false);
    let tree = PathTree::build(Tab::Iteration, &seen, &HashMap::new());
    assert_eq!(tree.model.len(), 1);
    assert_eq!(tree.model.path(0), "P1");
    assert_eq!(tree.counts, [0]);
}

/// Iteration nodes carry their stored dates (days), so the picker's
/// Current / Next / Recent work per project (AC-12).
#[test]
fn iteration_nodes_keep_their_dates() {
    let tree_p1 = story::tree("P1", &[], 43, NOW);
    let seen = Seen::gather(&[open(1, P1, "P1")], &names(), false);
    let tree = PathTree::build(Tab::Iteration, &seen, &stored(&[(P1, tree_p1.clone())]));
    let current = tree.model.shortcuts(NOW.div_euclid(DAY));
    let first = current.first().expect("a current sprint");
    assert_eq!(tree.model.path(first.1), "P1\\Sprint 45");
    let node = &tree_p1.iterations[3];
    let dates = tree.model.nodes()[tree.find(&node.path).unwrap()]
        .dates
        .unwrap();
    assert_eq!(dates.start, node.start_ms.unwrap().div_euclid(DAY));
}

#[test]
fn due_projects_are_missing_or_a_day_old_and_not_in_flight() {
    let seen = Seen::gather(&ac9_items(), &names(), false);
    let fresh = stored(&[(P1, p1_tree(NOW - 2 * HOUR))]);
    assert_eq!(due(&seen, &fresh, &HashSet::new(), NOW), [P2]);
    let in_flight = HashSet::from([P2.to_owned()]);
    assert!(due(&seen, &fresh, &in_flight, NOW).is_empty());
    let old = stored(&[(P1, p1_tree(NOW - 24 * HOUR))]);
    assert_eq!(due(&seen, &old, &HashSet::new(), NOW), [P1, P2]);
}

#[test]
fn footer_says_where_paths_and_counts_come_from() {
    assert_eq!(note(Tab::Area, false, false, false), None);
    assert_eq!(
        note(Tab::Area, true, false, false).as_deref(),
        Some(FROM_SYNCED_ITEMS)
    );
    assert_eq!(
        note(Tab::Iteration, true, true, false).as_deref(),
        Some("Loading all iterations…")
    );
    assert_eq!(
        note(Tab::Area, false, false, true).as_deref(),
        Some("Counts cover the newest 5,000 synced items")
    );
}

// ── AC-11 against a mock server ─────────────────────────────────────────

/// Answers every request with one fixed reply and keeps what was sent.
struct Mock {
    sent: Mutex<Vec<ApiRequest>>,
    reply: Mutex<(u16, Value)>,
}

struct Shared(Arc<Mock>);

impl Transport for Shared {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        self.0.sent.lock().push(req);
        let (status, body) = self.0.reply.lock().clone();
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

struct Fixture {
    store: Store,
    mock: Arc<Mock>,
    log: Arc<NetLog>,
    writer: adoken_sync::WiWriter,
    _dir: tempfile::TempDir,
}

/// P1's tree stored 2 h ago, P2's missing, AC-9's items loaded.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
    store
        .write(|tx| {
            adoken_store::write::upsert_person(
                tx,
                &adoken_model::Person {
                    id: "me".into(),
                    display_name: "Ada Me".into(),
                    unique_name: Some("ada@contoso.com".into()),
                    is_group: false,
                    image_url: None,
                },
                true,
            )?;
            for (id, name) in [(P1, "P1"), (P2, "P2")] {
                adoken_store::write::upsert_project(
                    tx,
                    &adoken_model::Project {
                        id: id.into(),
                        name: name.into(),
                    },
                )?;
            }
            adoken_store::wi::put_class_tree(tx, P1, &p1_tree(NOW - 2 * HOUR))?;
            for item in ac9_items() {
                adoken_store::write::upsert_work_item(tx, &item)?;
            }
            Ok(())
        })
        .unwrap();
    let clock = Arc::new(ManualClock::new(NOW));
    let mock = Arc::new(Mock {
        sent: Mutex::new(Vec::new()),
        reply: Mutex::new((
            200,
            json!({"count": 2, "value": [
                {"name": "P2", "structureType": "area", "children": [
                    {"name": "Ledger", "structureType": "area"},
                    {"name": "Risk", "structureType": "area"}
                ]},
                {"name": "P2", "structureType": "iteration"}
            ]}),
        )),
    });
    let log = Arc::new(NetLog::new(clock.clone()));
    let writer = adoken_sync::WiWriter::new(
        store.clone(),
        Arc::new(Recording::new(Shared(mock.clone()), log.clone())),
        Arc::new(Governor::new(clock.clone())),
        clock,
        url::Url::parse("https://dev.azure.com/contoso/").unwrap(),
    );
    Fixture {
        store,
        mock,
        log,
        writer,
        _dir: dir,
    }
}

/// What an open of the dropdown does: read the store as a list load does,
/// then read every due tree with the dropdown's attribution.
async fn open_dropdown(
    f: &Fixture,
) -> (Prepared, Vec<Result<ClassTree, adoken_sync::wi::WiError>>) {
    let data = super::super::data::ListData::read(&f.store).unwrap();
    let prepared = Prepared::read(&f.store, &data);
    let mut answers = Vec::new();
    for project in due(&prepared.seen, &prepared.stored, &HashSet::new(), NOW) {
        answers.push(
            f.writer
                .class_tree(&project, UserAction::FilterByArea)
                .await,
        );
    }
    (prepared, answers)
}

/// AC-11: the first open sends exactly one `classificationnodes` GET, for
/// P2, at user priority as "You: filter by area"; the tree merges; a second
/// open the same day sends none.
#[tokio::test]
async fn ac11_first_open_reads_only_the_missing_tree_once() {
    let f = fixture();
    let (before, answers) = open_dropdown(&f).await;
    let lines: Vec<String> = f
        .mock
        .sent
        .lock()
        .iter()
        .map(|r| format!("{} {}", r.method().as_str(), r.url().path()))
        .collect();
    assert_eq!(lines, ["GET /contoso/p2/_apis/wit/classificationnodes"]);
    let entries = f.log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].origin.label(), "You: filter by area");
    assert_eq!(entries[0].tier, Some(Tier::User));
    let tree = answers.into_iter().next().unwrap().expect("P2's tree");

    // The arrival merges: P2's stored node shows next to its loaded paths.
    let mut merged = before.stored.clone();
    merged.insert(P2.into(), Arc::new(tree));
    let after = PathTree::build(Tab::Area, &before.seen, &merged);
    assert!(after.find("P2\\Risk").is_some());
    assert!(
        after.find("P2\\Ledger\\Close").is_some(),
        "loaded path kept"
    );
    assert!(after.partial.is_empty());

    f.mock.sent.lock().clear();
    let (_, answers) = open_dropdown(&f).await;
    assert!(answers.is_empty() && f.mock.sent.lock().is_empty());
}

/// AC-11: a 500 keeps the loaded paths and the next open tries again.
#[tokio::test]
async fn ac11_a_failed_read_keeps_loaded_paths_and_retries_next_open() {
    let f = fixture();
    *f.mock.reply.lock() = (500, json!({"message": "boom"}));
    let (prepared, answers) = open_dropdown(&f).await;
    assert!(answers[0].is_err());
    let tree = &prepared.trees.area;
    assert!(tree.find("P2\\Ledger\\Close").is_some());
    assert_eq!(tree.partial, [P2]);
    let sent = f.mock.sent.lock().len();
    assert!(sent >= 1);

    *f.mock.reply.lock() = (200, json!({"count": 0, "value": []}));
    let (_, answers) = open_dropdown(&f).await;
    assert_eq!(answers.len(), 1, "the next open retries P2");
    assert!(f.mock.sent.lock().len() > sent);
}
