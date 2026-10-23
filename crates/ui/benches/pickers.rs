//! Spec 0015 §7 picker budgets, per keystroke:
//! - area/iteration filter over 10,000 nodes ≤ 5 ms (AC-12);
//! - people filter over 5,000 stored identities ≤ 2 ms.
//!
//! `cargo bench -p adoken-ui --bench pickers` prints the slowest of a few
//! typed prefixes against each budget, then runs criterion.

use std::hint::black_box;
use std::time::{Duration, Instant};

use adoken_ui::picker::{PeopleModel, Person, TreeModel, TreeNode};
use criterion::{Criterion, criterion_group, criterion_main};

/// 10 areas × 10 teams × 10 components × 10 services: 11,111 nodes.
fn tree_fixture() -> Vec<TreeNode> {
    let mut nodes = vec![TreeNode::new("Storefront", None)];
    let tops = [
        "Infra", "Web", "Mobile", "Data", "Payments", "Search", "Identity", "Growth", "Support",
        "Platform",
    ];
    for top in tops {
        let t = nodes.len();
        nodes.push(TreeNode::new(top, Some(0)));
        for team in 0..10 {
            let m = nodes.len();
            nodes.push(TreeNode::new(format!("Team {top} {team}"), Some(t)));
            for comp in 0..10 {
                let c = nodes.len();
                nodes.push(TreeNode::new(format!("Component {comp}"), Some(m)));
                for leaf in 0..10 {
                    nodes.push(TreeNode::new(format!("Service {leaf}"), Some(c)));
                }
            }
        }
    }
    nodes
}

fn people_fixture() -> Vec<Person> {
    const FIRST: [&str; 10] = [
        "Ada", "Dana", "Dan", "Kwame", "Mei", "Lars", "Priya", "Tomás", "Yuki", "Zoe",
    ];
    const LAST: [&str; 10] = [
        "Lindqvist",
        "Kim",
        "Okafor",
        "Mensah",
        "Tanaka",
        "Ruiz",
        "Novak",
        "Haddad",
        "Silva",
        "Berg",
    ];
    (0..5_000)
        .map(|i| {
            let first = FIRST[i % 10];
            let last = LAST[(i / 10) % 10];
            Person::new(
                i.to_string(),
                format!("{first} {last} {i}"),
                format!(
                    "{}.{}{i}@contoso.com",
                    first.to_lowercase(),
                    last.to_lowercase()
                ),
            )
        })
        .collect()
}

/// Slowest of `queries` (best of 5 each: the budget is about the work, not
/// noise). `reset` runs untimed before each timed `run`.
fn slowest(mut reset: impl FnMut(), mut run: impl FnMut(&str), queries: &[&str]) -> Duration {
    queries
        .iter()
        .map(|q| {
            (0..5)
                .map(|_| {
                    reset();
                    let start = Instant::now();
                    run(q);
                    start.elapsed()
                })
                .min()
                .unwrap_or_default()
        })
        .max()
        .unwrap_or_default()
}

fn report(name: &str, took: Duration, budget_ms: f64) {
    let ms = took.as_secs_f64() * 1e3;
    let verdict = if ms <= budget_ms { "ok" } else { "OVER" };
    println!("{name}: slowest keystroke {ms:.3} ms (budget {budget_ms} ms) {verdict}");
}

fn bench(c: &mut Criterion) {
    let mut tree = TreeModel::new(tree_fixture());
    let tree_queries = ["i", "in", "inf", "infr", "infra", "infra 3", "service 9"];
    report(
        "tree 11k",
        slowest(
            || {},
            |q| {
                black_box(tree.rows(q));
            },
            &tree_queries,
        ),
        5.0,
    );
    c.bench_function("tree_filter_11k_infra", |b| {
        b.iter(|| black_box(tree.rows(black_box("infra"))))
    });

    // Filtering moves from one query to the next; the rebuild for the
    // emptied query is the previous keystroke's work, not this one's.
    let people = std::cell::RefCell::new(PeopleModel::new(false));
    people.borrow_mut().set_people(people_fixture());
    let people_queries = ["d", "da", "dan", "dana", "dana r", "kim 42", "zoe@"];
    report(
        "people 5k",
        slowest(
            || people.borrow_mut().set_query("~"),
            |q| people.borrow_mut().set_query(q),
            &people_queries,
        ),
        2.0,
    );
    let mut people = people.into_inner();
    let mut flip = false;
    c.bench_function("people_filter_5k_dan_dana", |b| {
        b.iter(|| {
            flip = !flip;
            people.set_query(black_box(if flip { "dan" } else { "dana" }));
            black_box(people.rows().len())
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
