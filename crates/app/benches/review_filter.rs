//! `cargo bench -p adoken --bench review_filter`: spec 0019 AC-A6 / §7, the
//! review's file filter per keystroke. Each keystroke of `src/auth -test`
//! parses the query and lists the tree's rows ([`filter::list`]) over the
//! L+T fixture's 300 files (< 1 ms) and the same shape at 5,000 (< 5 ms).

use std::hint::black_box;
use std::time::{Duration, Instant};

use adoken_app::review::filter::{self, FileQuery, FoldedPaths};
use adoken_app::review::fixtures;
use adoken_ui::file_tree::{self, TreeRow};
use criterion::{Criterion, criterion_group, criterion_main};

const TYPED: &str = "src/auth -test";

struct Tree {
    rows: Vec<TreeRow>,
    paths: FoldedPaths,
}

fn tree(files: usize) -> Tree {
    let fixture = fixtures::large(files, files.max(300) * 133);
    let paths: Vec<&str> = fixture
        .detail
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    Tree {
        rows: file_tree::build(&paths),
        paths: FoldedPaths::new(&paths),
    }
}

/// Every keystroke's query: `s`, `sr`, …, `src/auth -test`.
fn keystrokes() -> Vec<&'static str> {
    TYPED
        .char_indices()
        .map(|(at, ch)| &TYPED[..at + ch.len_utf8()])
        .collect()
}

fn keystroke(tree: &Tree, typed: &str) -> usize {
    let query = FileQuery::parse(typed);
    filter::list(&tree.rows, &tree.paths, &query, None)
        .rows
        .len()
}

/// p99 of one keystroke over 200 rounds of the whole typed query, checked
/// against `budget` before criterion runs (as `pins.rs`).
fn check(name: &str, tree: &Tree, budget: Duration) {
    let typed = keystrokes();
    let mut samples = Vec::with_capacity(200 * typed.len());
    for _ in 0..200 {
        for query in &typed {
            let start = Instant::now();
            black_box(keystroke(tree, query));
            samples.push(start.elapsed());
        }
    }
    samples.sort_unstable();
    let pick = |p: f64| samples[((samples.len() - 1) as f64 * p) as usize];
    let (p50, p99) = (pick(0.5), pick(0.99));
    eprintln!("review filter {name}: p50={p50:?}, p99={p99:?}, budget={budget:?}");
    assert!(
        p99 < budget,
        "{name}: a filter keystroke exceeds {budget:?}"
    );
}

fn bench(c: &mut Criterion) {
    let small = tree(300);
    let large = tree(5_000);
    let listed = filter::list(&small.rows, &small.paths, &FileQuery::parse(TYPED), None);
    assert!(
        !listed.files.is_empty() && listed.files.len() < 300,
        "`{TYPED}` lists some of the fixture's files"
    );
    check("300 files", &small, Duration::from_millis(1));
    check("5,000 files", &large, Duration::from_millis(5));

    for (name, tree) in [("300_files", &small), ("5000_files", &large)] {
        c.bench_function(&format!("review_filter/keystroke_{name}"), |b| {
            b.iter(|| black_box(keystroke(black_box(tree), black_box(TYPED))))
        });
        c.bench_function(&format!("review_filter/typing_{name}"), |b| {
            let typed = keystrokes();
            b.iter(|| {
                for query in &typed {
                    black_box(keystroke(black_box(tree), query));
                }
            })
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
