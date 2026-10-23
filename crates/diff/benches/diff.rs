//! Spec 0004 §7: "Local diff compute, 5k-line file < 20 ms off main thread",
//! plus the per-frame work of a virtualized diff (row layout once per file,
//! tokenize + intraline for the ~60 visible rows each frame).
//!
//! `cargo bench -p adoken-diff --bench diff`

use std::collections::HashSet;
use std::hint::black_box;

use adoken_diff::{FileDiff, Text, lex, rows, words};
use criterion::{Criterion, criterion_group, criterion_main};

/// Deterministic Rust-ish file of `n` lines.
fn source(n: usize, salt: u64) -> String {
    let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ salt;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    (0..n)
        .map(|i| match i % 7 {
            0 => {
                format!("pub fn handler_{i}(ctx: &Context, retries: u32) -> Result<(), Error> {{\n")
            }
            1 => format!(
                "    let value_{i} = ctx.lookup(\"key-{}\")?;\n",
                next() % 97
            ),
            2 => format!(
                "    if value_{} > {} {{ return Err(Error::Limit); }}\n",
                i - 1,
                next() % 1000
            ),
            3 => "    // Retry with jittered backoff.\n".to_string(),
            4 => format!("    self.metrics.record({}, retries);\n", next() % 13),
            5 => "    Ok(())\n".to_string(),
            _ => "}\n".to_string(),
        })
        .collect()
}

/// ~10% of lines edited, some inserted and deleted.
fn edited(src: &str, salt: u64) -> String {
    let mut state = salt | 1;
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        match (state >> 33) % 30 {
            0 => {}
            1 => {
                out.push_str(line);
                out.push('\n');
                out.push_str("    tracing::debug!(\"inserted\");\n");
            }
            2 | 3 => {
                out.push_str(&line.replace("retries", "retry_budget"));
                out.push('\n');
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

fn bench(c: &mut Criterion) {
    let old = source(5_000, 1);
    let new = edited(&old, 7);
    let (old_text, new_text) = (Text::new(old.as_str()), Text::new(new.as_str()));

    c.bench_function("compute 5k lines (~10% edited)", |b| {
        b.iter(|| FileDiff::compute(black_box(old_text.clone()), black_box(new_text.clone())))
    });

    let rewritten = source(5_000, 99);
    let rewritten_text = Text::new(rewritten.as_str());
    c.bench_function("compute 5k lines (fully rewritten)", |b| {
        b.iter(|| {
            FileDiff::compute(
                black_box(old_text.clone()),
                black_box(rewritten_text.clone()),
            )
        })
    });

    let diff = FileDiff::compute(old_text.clone(), new_text.clone());
    let expanded = HashSet::new();
    c.bench_function("layout rows unified+split 5k", |b| {
        b.iter(|| {
            black_box(rows::unified(&diff, &expanded));
            black_box(rows::split(&diff, &expanded));
        })
    });

    // One frame's worth of per-row work: 60 visible lines.
    let visible: Vec<usize> = (0..diff.lines.len()).skip(2_000).take(60).collect();
    c.bench_function("per-frame tokenize+intraline 60 rows", |b| {
        b.iter(|| {
            for &ix in &visible {
                black_box(lex::tokenize(diff.text(ix), lex::Lang::Rust));
                if let Some(partner) = diff.lines[ix].partner() {
                    black_box(words::changed_ranges(diff.text(partner), diff.text(ix)));
                }
            }
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
