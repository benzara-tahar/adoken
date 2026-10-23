//! Search latency runner for spec 0003 AC-1 / AC-10: builds the index over the
//! synthetic corpus and times every query of the synthetic query set, then
//! reports p50/p99 (criterion's bench in `crates/search` reports means; the
//! budgets are percentiles).
//!
//! Usage: perf-search [--items N] [--queries Q] [--rounds R] [--builds B] [--seed S]

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

use adoken_perf::{Budget, Samples};
use adoken_search::{Index, synthetic};

/// Rows the palette lays out (spec 0003 AC-1: "top-50 layout").
const LIMIT: usize = 50;

struct Args {
    items: usize,
    queries: usize,
    rounds: usize,
    builds: usize,
    seed: u64,
}

const USAGE: &str =
    "usage: perf-search [--items N] [--queries Q] [--rounds R] [--builds B] [--seed S]";

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        items: 100_000,
        queries: 500,
        rounds: 3,
        builds: 3,
        seed: 0xad0_4e11,
    };
    while let Some(flag) = args.next() {
        let raw = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        let value: u64 = raw
            .parse()
            .map_err(|_| format!("{flag}: not a number: {raw}"))?;
        match flag.as_str() {
            "--items" => parsed.items = value as usize,
            "--queries" => parsed.queries = value as usize,
            "--rounds" => parsed.rounds = value as usize,
            "--builds" => parsed.builds = value as usize,
            "--seed" => parsed.seed = value,
            _ => return Err(format!("unknown flag {flag}\n{USAGE}")),
        }
    }
    if parsed.queries == 0 || parsed.rounds == 0 || parsed.builds == 0 {
        return Err("--queries, --rounds and --builds must be at least 1".into());
    }
    Ok(parsed)
}

fn run(args: &Args) -> Result<(), String> {
    let corpus = synthetic::corpus(args.items, args.seed);
    let queries = synthetic::queries(args.queries, args.seed);
    if queries.is_empty() {
        return Err("synthetic::queries returned no queries".into());
    }
    println!(
        "search: {} docs, {} queries × {} rounds, top-{LIMIT}",
        corpus.len(),
        queries.len(),
        args.rounds
    );

    let mut builds = Samples::with_capacity("index build", args.builds);
    let mut index = None;
    for _ in 0..args.builds {
        let docs = corpus.clone();
        let started = Instant::now();
        let built = Index::new(docs);
        builds.record(started.elapsed());
        index = Some(black_box(built));
    }
    let index = index.expect("builds >= 1");
    let builds = builds.summary().expect("builds >= 1");
    println!("{builds}");

    // One untimed pass so first-touch page faults and lazy init don't land
    // in the tail.
    let mut hits = 0usize;
    for query in &queries {
        hits += black_box(index.query(query, LIMIT)).len();
    }
    if hits == 0 {
        return Err("no query returned any hit; the index is not searching".into());
    }

    let mut latency = Samples::with_capacity("search query", queries.len() * args.rounds);
    for _ in 0..args.rounds {
        for query in &queries {
            let started = Instant::now();
            let result = index.query(black_box(query), LIMIT);
            latency.record(started.elapsed());
            black_box(result);
        }
    }
    let latency = latency.summary().expect("queries >= 1");
    println!("{latency}");

    println!("{}", Budget::SEARCH_P50.check(&latency).row());
    println!("{}", Budget::SEARCH_P99.check(&latency).row());
    println!("{}", Budget::INDEX_BUILD.check(&builds).row());
    Ok(())
}

fn main() -> ExitCode {
    match parse_args().and_then(|args| run(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("perf-search: {message}");
            ExitCode::FAILURE
        }
    }
}
