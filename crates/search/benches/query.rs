//! Spec 0003 AC-1: keystroke-to-results on 100k and 500k synthetic docs.
//!
//! `cargo bench -p adoken-search --bench query` first prints p50/p95/p99 over
//! the recorded 500-query set (≥ 50 transpositions) per corpus size against
//! the budget (p50 < 10 ms, p99 < 50 ms), then runs criterion on typical
//! queries.

use std::hint::black_box;
use std::time::{Duration, Instant};

use adoken_search::{Index, synthetic};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

const SIZES: &[(usize, &str)] = &[(100_000, "100k"), (500_000, "500k")];
const SEED: u64 = 0xAD0_4E4;
const LIMIT: usize = 50;

/// Typical palette queries: words, prefixes, multi-token, ids, typos.
const TYPICAL: &[&str] = &[
    "l",
    "lo",
    "login",
    "lgoin",
    "revew login",
    "pipeline cache",
    "fix sso redirect",
    "notifcation centre",
    "48213",
    "zoe",
];

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let rank = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[rank]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn ac1_report(index: &Index, label: &str) {
    let queries = synthetic::queries(500, SEED);
    for q in &queries {
        black_box(index.query(q, LIMIT)); // warm caches and thread pools
    }
    let mut all = Vec::with_capacity(queries.len() * 3);
    let mut transposed = Vec::new();
    for _ in 0..3 {
        for (i, q) in queries.iter().enumerate() {
            let start = Instant::now();
            black_box(index.query(black_box(q), LIMIT));
            let took = start.elapsed();
            all.push(took);
            if i % 8 == 0 {
                transposed.push(took);
            }
        }
    }
    all.sort_unstable();
    transposed.sort_unstable();
    let (p50, p95, p99) = (
        percentile(&all, 0.50),
        percentile(&all, 0.95),
        percentile(&all, 0.99),
    );
    let verdict = if ms(p50) < 10.0 && ms(p99) < 50.0 {
        "PASS"
    } else {
        "FAIL"
    };
    println!(
        "AC-1 {label}: p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms \
         (transpositions p99 {:.2} ms) vs budget p50 < 10 ms, p99 < 50 ms: {verdict}",
        ms(p50),
        ms(p95),
        ms(p99),
        ms(*all.last().unwrap()),
        ms(percentile(&transposed, 0.99)),
    );
}

fn bench(c: &mut Criterion) {
    for &(size, label) in SIZES {
        let start = Instant::now();
        let index = Index::new(synthetic::corpus(size, SEED));
        println!("index build {label}: {:.1} ms", ms(start.elapsed()));
        ac1_report(&index, label);

        let mut group = c.benchmark_group(format!("query/{label}"));
        group.sample_size(20);
        group.measurement_time(Duration::from_secs(2));
        for &q in TYPICAL {
            group.bench_with_input(BenchmarkId::from_parameter(q), q, |b, q| {
                b.iter(|| index.query(black_box(q), LIMIT))
            });
        }
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
