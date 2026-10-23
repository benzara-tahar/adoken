//! Spec 0016 §7 budgets:
//! - recorder overhead per request < 20 µs (start + finish of one entry
//!   through [`Recording`], ring full and evicting);
//! - FR-12 change counting within 0002 §7's store-write budget: a page of
//!   1,000 changed work items applied with change counting, and the
//!   counting alone, against < 20 ms per 1,000 changed rows.
//!
//! `cargo bench -p adoken-sync --bench activity --profile dev`

use std::sync::Arc;

use adoken_model::Project;
use adoken_store::{Store, write};
use adoken_sync::activity::{ChangeSet, ChangeWord};
use adoken_sync::attribution::Origin;
use adoken_sync::cadence::Lane;
use adoken_sync::engine::apply_work_item_page;
use adoken_sync::ledger::{MAX_ENTRIES, NetLog};
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::wire::{WireWorkItem, format_time};
use adoken_sync::{ApiRequest, ManualClock, Recording, Transport};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use futures::future::BoxFuture;
use serde_json::json;

const T0: i64 = 1_790_769_600_000;
const PAGE: usize = 1_000;

/// Answers at once with a small JSON body and a cost header.
struct Instant;

impl Transport for Instant {
    fn send(&self, _req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async {
            Ok(ApiResponse {
                status: 200,
                rate: RateHeaders {
                    cost: Some(0.004),
                    ..RateHeaders::default()
                },
                body: br#"{"count":0,"value":[]}"#.to_vec(),
                content_type: Some("application/json".into()),
                validators: Default::default(),
            })
        })
    }
}

fn recorder(c: &mut Criterion) {
    let log = Arc::new(NetLog::new(Arc::new(ManualClock::new(T0))));
    let rec = Recording::new(Instant, log.clone());
    let req = ApiRequest::get(
        url::Url::parse(
            "https://dev.azure.com/contoso/9628fca0-2412-4d44-b078-a7cda9218300/_apis/git/\
             repositories/0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0/pullRequests/4312/threads\
             ?api-version=7.1",
        )
        .unwrap(),
    )
    .with_origin(Origin::Sync {
        lane: Lane::MyPrs,
        pinned: false,
        sync_now: false,
    });
    // Steady state: the ring is full and every request evicts one.
    for _ in 0..MAX_ENTRIES {
        futures::executor::block_on(rec.send(req.clone())).unwrap();
    }
    c.bench_function("recorder: one request (start + finish, full ring)", |b| {
        b.iter(|| futures::executor::block_on(rec.send(req.clone())).unwrap())
    });
    // The transport alone, to subtract.
    c.bench_function("recorder baseline: transport without recording", |b| {
        b.iter(|| futures::executor::block_on(Instant.send(req.clone())).unwrap())
    });
}

fn page(rev: i64, title: &str) -> Vec<WireWorkItem> {
    (0..PAGE as i64)
        .map(|i| {
            serde_json::from_value(json!({
                "id": 10_000 + i,
                "rev": rev,
                "fields": {
                    "System.Title": format!("{title} {i}"),
                    "System.WorkItemType": "Bug",
                    "System.State": "Active",
                    "System.TeamProject": "Payments",
                    "System.CreatedBy": { "id": "me", "displayName": "Me", "uniqueName": "me@example.com" },
                    "System.AssignedTo": { "id": "me", "displayName": "Me", "uniqueName": "me@example.com" },
                    "System.CreatedDate": format_time(T0 - 86_400_000),
                    "System.ChangedDate": format_time(T0 + rev),
                }
            }))
            .unwrap()
        })
        .collect()
}

fn store_with_page() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("b.sqlite")).unwrap();
    store
        .write(|tx| {
            write::upsert_project(
                tx,
                &Project {
                    id: "p".into(),
                    name: "Payments".into(),
                },
            )
        })
        .unwrap();
    let changes = apply_work_item_page(&store, "me", page(1, "Checkout times out"), T0).unwrap();
    assert_eq!(changes.work_items.new as usize, PAGE);
    (dir, store)
}

fn change_counting(c: &mut Criterion) {
    // Sanity: the measured page really changes every row.
    {
        let (_dir, store) = store_with_page();
        let changes = apply_work_item_page(&store, "me", page(2, "Renamed"), T0).unwrap();
        assert_eq!(changes.work_items.updated as usize, PAGE);
        assert_eq!(changes.work_items.ids.len(), 20);
    }
    let mut group = c.benchmark_group("fr12");
    group.sample_size(20);
    group.bench_function("1,000 changed work items: page commit with counting", |b| {
        b.iter_batched(
            store_with_page,
            |(dir, store)| {
                apply_work_item_page(&store, "me", page(2, "Renamed"), T0).unwrap();
                (dir, store)
            },
            BatchSize::PerIteration,
        )
    });
    group.bench_function("1,000 changes: counting alone (note + finish)", |b| {
        b.iter(|| {
            let mut set = ChangeSet::default();
            for id in 0..PAGE as i64 {
                set.note_work_item(id, ChangeWord::State("Resolved".into()));
            }
            set.finish()
        })
    });
    group.finish();
}

criterion_group!(benches, recorder, change_counting);
criterion_main!(benches);
