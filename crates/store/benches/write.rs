//! ADR 0001 gate B4 (shape): one sync page = one transaction of upserts with
//! search_doc/FTS maintenance. Budget: 1,000-row page commit ≤ 150 ms
//! (spec 0002 §7: < 20 ms per 1,000 changed rows for the diff + events).

use adoken_model::*;
use adoken_store::{Store, write};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

fn wi(id: i64, rev: i64) -> WorkItem {
    WorkItem {
        id,
        project_id: "p".into(),
        rev,
        kind: WorkItemType::Bug,
        title: format!("Checkout times out when retrying card {id}"),
        state: "Active".into(),
        state_category: StateCategory::InProgress,
        assigned_to: Some("u".into()),
        created_by: "u".into(),
        changed_by: "u".into(),
        area_path: "P\\Checkout".into(),
        iteration_path: "P\\Sprint 42".into(),
        priority: Some(2),
        tags: vec!["perf".into()],
        parent_id: None,
        linked_pr_ids: Vec::new(),
        description: None,
        comment_count: 0,
        created: 0,
        changed: rev,
    }
}

fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("b.sqlite")).unwrap();
    store
        .write(|tx| {
            write::upsert_project(
                tx,
                &Project {
                    id: "p".into(),
                    name: "P".into(),
                },
            )?;
            write::upsert_person(
                tx,
                &Person {
                    id: "u".into(),
                    display_name: "Una User".into(),
                    unique_name: None,
                    is_group: false,
                    image_url: None,
                },
                true,
            )?;
            for id in 0..20_000 {
                write::upsert_work_item(tx, &wi(id, 1))?;
            }
            Ok(())
        })
        .unwrap();
    (dir, store)
}

fn bench(c: &mut Criterion) {
    let (_dir, store) = setup();
    let mut rev = 1;
    c.bench_function("page_1000_work_items_changed", |b| {
        b.iter_batched(
            || {
                rev += 1;
                (0..1000).map(|i| wi(i * 7, rev)).collect::<Vec<_>>()
            },
            |page| {
                store
                    .write(|tx| {
                        for w in &page {
                            write::upsert_work_item(tx, w)?;
                        }
                        Ok(())
                    })
                    .unwrap()
            },
            BatchSize::SmallInput,
        )
    });
    c.bench_function("page_1000_work_items_replayed", |b| {
        let page: Vec<WorkItem> = (0..1000).map(|i| wi(i * 7, 1)).collect();
        b.iter(|| {
            store
                .write(|tx| {
                    for w in &page {
                        write::upsert_work_item(tx, w)?;
                    }
                    Ok(())
                })
                .unwrap()
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
