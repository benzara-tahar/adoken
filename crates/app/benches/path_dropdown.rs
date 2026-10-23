//! `cargo bench -p adoken --bench path_dropdown`: the work item list's area
//! dropdown (spec 0008 §7) — the background node build for 5 projects ×
//! ~11,000 stored nodes and 5,000 loaded items (budget < 20 ms), the clone
//! an open pays (one frame), and a keystroke's filter over all of them.

use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;

use adoken_app::workitems::model::Tab;
use adoken_app::workitems::paths::{PathTree, Seen, story};
use adoken_model::{ClassNode, ClassTree, NodeKind};
use criterion::{Criterion, criterion_group, criterion_main};

const NOW: i64 = 1_790_769_600_000;

fn node(path: String) -> ClassNode {
    ClassNode {
        kind: NodeKind::Area,
        name: path.rsplit('\\').next().unwrap().to_owned(),
        depth: path.matches('\\').count() as u16,
        path,
        start_ms: None,
        finish_ms: None,
    }
}

/// 10 groups × 10 teams × 10 components × 10 leaves under the root: 11,111
/// nodes, pre-order.
fn tree(root: &str) -> ClassTree {
    let mut areas = vec![node(root.to_owned())];
    for g in 0..10 {
        let group = format!("{root}\\Group {g}");
        areas.push(node(group.clone()));
        for t in 0..10 {
            let team = format!("{group}\\Team {t}");
            areas.push(node(team.clone()));
            for c in 0..10 {
                let component = format!("{team}\\Component {c}");
                areas.push(node(component.clone()));
                for l in 0..10 {
                    areas.push(node(format!("{component}\\Service {l}")));
                }
            }
        }
    }
    ClassTree {
        areas,
        iterations: Vec::new(),
        fetched_ms: NOW,
    }
}

fn bench(c: &mut Criterion) {
    let projects: Vec<String> = (0..5).map(|p| format!("P{p}")).collect();
    let names: HashMap<String, String> = projects.iter().map(|p| (p.clone(), p.clone())).collect();
    let stored: HashMap<String, Arc<ClassTree>> = projects
        .iter()
        .map(|p| (p.clone(), Arc::new(tree(p))))
        .collect();
    let items: Vec<_> = (0..5_000i64)
        .map(|id| {
            let p = &projects[(id % 5) as usize];
            let area = format!(
                "{p}\\Group {}\\Team {}\\Component {}\\Service {}",
                id % 10,
                (id / 10) % 10,
                (id / 100) % 10,
                (id / 7) % 12
            );
            story::item(id, p, &area, p, "Active", NOW)
        })
        .collect();
    let seen = Seen::gather(&items, &names, true);
    let built = PathTree::build(Tab::Area, &seen, &stored);
    assert!(built.model.len() > 50_000);

    c.bench_function("path dropdown: build 5 × 11k nodes + 5k items", |b| {
        b.iter(|| black_box(PathTree::build(Tab::Area, &seen, &stored)))
    });
    c.bench_function("path dropdown: gather 5k items", |b| {
        b.iter(|| black_box(Seen::gather(&items, &names, true)))
    });
    c.bench_function("path dropdown: open (clone model + counts)", |b| {
        b.iter(|| black_box((built.model.clone(), built.counts.clone())))
    });
    let mut model = built.model.clone();
    c.bench_function("path dropdown: keystroke filter (5 × 11k nodes)", |b| {
        b.iter(|| black_box(model.rows("service 7")))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
