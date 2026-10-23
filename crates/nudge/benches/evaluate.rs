//! Spec 0006 §7 / 0017 AC-32: full nudge evaluation plus Today (summary,
//! mood, per-PR activity) over 1k PRs + 5k work items, 3,000 iterations,
//! 20,000 thread comments, 200 completed work items and 100 merges today,
//! must stay under 20 ms p99 off the main thread. Also times, on a temp
//! store holding the corpus, the store reads 0017 added (`store::load_day`:
//! today's pushes, comments on my merges today, done items, state since;
//! spec 0017 §7, 5 ms p99), and the whole `load_snapshot` for reference.
//!
//! `cargo bench -p adoken-nudge --bench evaluate` first prints p50/p99 over
//! 200 runs against the budgets, then runs criterion.

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

use adoken_model::time::{DAY, HOUR, MINUTE};
use adoken_model::{
    Comment, MergeStatus, Millis, Person, PipelineRun, PrIteration, PrStatus, PrThread, Project,
    PullRequest, Repo, Reviewer, RunReason, RunResult, RunStatus, StateCategory, ThreadStatus,
    Vote, WorkItem, WorkItemType,
};
use adoken_nudge::{DataHealth, Ledger, Settings, Snapshot, evaluate, today};
use adoken_store::{Store, write};
use criterion::{Criterion, criterion_group, criterion_main};

const NOW: Millis = 1_789_948_800_000 + 17 * DAY + 11 * HOUR;
const BUDGET: Duration = Duration::from_millis(20);
/// Spec 0017 §7: the added store reads.
const READ_BUDGET: Duration = Duration::from_millis(5);
const COMMENTS_PER_PR: i64 = 20;
const ITERATIONS: usize = 3_000;
const DONE_TODAY: i64 = 200;
const MERGES_TODAY: usize = 100;

/// Deterministic xorshift, no dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

struct Corpus {
    snap: Snapshot,
    people: Vec<String>,
    iterations: Vec<(i64, PrIteration)>,
    done: Vec<WorkItem>,
}

fn corpus() -> Corpus {
    let mut rng = Rng(0xAD0_4E4);
    let people: Vec<String> = (0..60).map(|i| format!("p{i}")).collect();
    let mut snap = Snapshot::empty("p0");
    for p in &people {
        snap.names.insert(p.clone(), format!("Person {p}"));
    }
    let votes = [
        Vote::NoVote,
        Vote::NoVote,
        Vote::Approved,
        Vote::WaitingForAuthor,
        Vote::Rejected,
    ];
    let day_start = Settings::default().hours.day_start(NOW);
    for id in 0..1_000i64 {
        let created = NOW - rng.below(40 * DAY as u64) as Millis;
        let author = people[rng.below(8) as usize].clone();
        let reviewers = (0..1 + rng.below(4))
            .map(|_| Reviewer {
                person: people[rng.below(12) as usize].clone(),
                vote: votes[rng.below(5) as usize],
                is_required: rng.below(3) == 0,
                has_declined: false,
                added_at: Some(created + rng.below(HOUR as u64) as Millis),
                voted_at: Some(created + rng.below(DAY as u64) as Millis),
            })
            .collect();
        snap.prs.push(PullRequest {
            id,
            repo_id: "r".into(),
            project_id: "p".into(),
            title: format!("Change number {id}"),
            description: String::new(),
            author,
            reviewers,
            status: if rng.below(5) == 0 {
                PrStatus::Completed
            } else {
                PrStatus::Active
            },
            is_draft: rng.below(10) == 0,
            merge_status: if rng.below(15) == 0 {
                MergeStatus::Conflicts
            } else {
                MergeStatus::Succeeded
            },
            source_branch: "refs/heads/f".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: None,
            created,
            updated: created + DAY,
            closed: Some(created + 2 * DAY),
            iterations: 2,
            last_push: Some(created + HOUR),
        });
        // 20,000 comments over the hot set, some today.
        let comments = (0..COMMENTS_PER_PR)
            .map(|c| {
                let at = NOW - rng.below(3 * DAY as u64) as Millis;
                Comment {
                    id: c,
                    parent_id: None,
                    author: people[rng.below(12) as usize].clone(),
                    body: String::new(),
                    created: at,
                    updated: at,
                    is_system: rng.below(4) == 0,
                    is_deleted: false,
                }
            })
            .collect();
        snap.threads.insert(
            id,
            vec![PrThread {
                id: 1,
                pr_id: id,
                status: if rng.below(4) == 0 {
                    ThreadStatus::Fixed
                } else {
                    ThreadStatus::Active
                },
                anchor: None,
                iteration: None,
                comments,
                updated: NOW - rng.below(2 * DAY as u64) as Millis,
            }],
        );
        for r in 0..2 {
            snap.runs.push(PipelineRun {
                id: id * 2 + r,
                project_id: "p".into(),
                pipeline_id: 1,
                pipeline_name: "ci".into(),
                run_number: String::new(),
                status: RunStatus::Completed,
                result: Some(if rng.below(3) == 0 {
                    RunResult::Failed
                } else {
                    RunResult::Succeeded
                }),
                reason: RunReason::PullRequest,
                branch: String::new(),
                source_version: None,
                requested_for: None,
                pr_id: Some(id),
                queued: created + r * HOUR,
                started: None,
                finish: Some(created + r * HOUR + 600_000),
            });
        }
    }
    for id in 0..5_000i64 {
        let changed = NOW - rng.below(60 * DAY as u64) as Millis;
        snap.work_items.push(WorkItem {
            id: 10_000 + id,
            project_id: "p".into(),
            rev: 1,
            kind: WorkItemType::Task,
            title: format!("Item {id}"),
            state: "Active".into(),
            state_category: if rng.below(3) == 0 {
                StateCategory::Proposed
            } else {
                StateCategory::InProgress
            },
            assigned_to: Some("p0".into()),
            created_by: "p0".into(),
            changed_by: "p0".into(),
            area_path: String::new(),
            iteration_path: String::new(),
            priority: None,
            tags: Vec::new(),
            parent_id: None,
            linked_pr_ids: vec![rng.below(1_000) as i64],
            description: None,
            comment_count: 0,
            created: changed,
            changed,
        });
    }
    for wi in &snap.work_items {
        snap.state_since.insert(wi.id, wi.changed - DAY);
    }
    // 100 merges today, half of them mine.
    snap.merged_today = snap
        .prs
        .iter()
        .filter(|pr| pr.status == PrStatus::Completed)
        .take(MERGES_TODAY)
        .cloned()
        .enumerate()
        .map(|(n, mut pr)| {
            pr.closed = Some(NOW - (n as Millis + 1) * 5 * MINUTE);
            pr.updated = pr.closed.unwrap_or(pr.updated);
            if n % 2 == 0 {
                pr.author = "p0".into();
            }
            pr
        })
        .collect();
    // 3,000 iterations on my PRs, today and older.
    let mine: Vec<i64> = snap
        .prs
        .iter()
        .chain(&snap.merged_today)
        .filter(|pr| pr.author == "p0")
        .map(|pr| pr.id)
        .collect();
    let mut iterations = Vec::with_capacity(ITERATIONS);
    for n in 0..ITERATIONS {
        let pr = mine[n % mine.len()];
        let created = NOW - rng.below(3 * DAY as u64) as Millis;
        iterations.push((
            pr,
            PrIteration {
                id: (n / mine.len()) as u32 + 1,
                source_commit: format!("{n:040x}"),
                created,
                description: None,
                commit_count: (rng.below(5) != 0).then(|| 1 + rng.below(4) as u32),
                commits_truncated: rng.below(50) == 0,
            },
        ));
    }
    for (pr, it) in &iterations {
        if it.created >= day_start {
            snap.iterations_today
                .entry(*pr)
                .or_default()
                .push(it.clone());
        }
    }
    // My PRs' heads: the latest stored push, one in seven a push ahead.
    let mut heads: HashMap<i64, &PrIteration> = HashMap::new();
    for (pr, it) in &iterations {
        if heads.get(pr).is_none_or(|head| head.id < it.id) {
            heads.insert(*pr, it);
        }
    }
    for pr in snap.prs.iter_mut().chain(&mut snap.merged_today) {
        if pr.author == "p0" {
            pr.last_source_commit = heads.get(&pr.id).map(|head| {
                if pr.id % 7 == 0 {
                    "f".repeat(40)
                } else {
                    head.source_commit.clone()
                }
            });
        }
    }
    let done: Vec<WorkItem> = (0..DONE_TODAY)
        .map(|n| {
            let mut wi = snap.work_items[0].clone();
            wi.id = 100_000 + n;
            wi.state = "Closed".into();
            wi.state_category = StateCategory::Completed;
            wi.changed = NOW - (n + 1) * MINUTE;
            wi.linked_pr_ids.clear();
            wi
        })
        .collect();
    snap.done_today = done.iter().map(|wi| wi.id).collect();
    Corpus {
        snap,
        people,
        iterations,
        done,
    }
}

fn report(what: &str, budget: Duration, mut run: impl FnMut()) {
    report_with(what, Some(budget), || (), |_| run());
}

/// [`report`] where each run works on a fresh `setup()` value, built and
/// dropped outside the timed part.
fn report_with<S>(
    what: &str,
    budget: Option<Duration>,
    mut setup: impl FnMut() -> S,
    mut run: impl FnMut(&mut S),
) {
    let mut samples: Vec<Duration> = (0..200)
        .map(|_| {
            let mut input = setup();
            let start = Instant::now();
            run(black_box(&mut input));
            let elapsed = start.elapsed();
            drop(input);
            elapsed
        })
        .collect();
    samples.sort_unstable();
    let p = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    let verdict = match budget {
        Some(budget) if p(0.99) < budget => format!("(budget {} ms) ok", budget.as_millis()),
        Some(budget) => format!("(budget {} ms) OVER BUDGET", budget.as_millis()),
        None => String::new(),
    };
    println!(
        "{what}: p50 {:.2} ms, p99 {:.2} ms {verdict}",
        p(0.5).as_secs_f64() * 1e3,
        p(0.99).as_secs_f64() * 1e3,
    );
}

/// The corpus written to a temp store, as sync would have left it.
fn store(corpus: &Corpus) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = Store::open(dir.path().join("adoken.sqlite")).expect("store");
    store
        .write(|tx| {
            for p in &corpus.people {
                let person = Person {
                    id: p.clone(),
                    display_name: format!("Person {p}"),
                    unique_name: None,
                    is_group: false,
                    image_url: None,
                };
                write::upsert_person(tx, &person, p == "p0")?;
            }
            write::upsert_project(
                tx,
                &Project {
                    id: "p".into(),
                    name: "P".into(),
                },
            )?;
            write::upsert_repo(
                tx,
                &Repo {
                    id: "r".into(),
                    project_id: "p".into(),
                    name: "r".into(),
                    default_branch: None,
                },
            )?;
            let snap = &corpus.snap;
            for pr in snap
                .prs
                .iter()
                .filter(|pr| !snap.merged_today.iter().any(|m| m.id == pr.id))
            {
                write::upsert_pr(tx, pr)?;
            }
            for pr in &snap.merged_today {
                write::upsert_pr(tx, pr)?;
            }
            for threads in snap.threads.values() {
                for t in threads {
                    write::upsert_thread(tx, t)?;
                }
            }
            for (pr, it) in &corpus.iterations {
                write::put_pr_iterations(tx, *pr, std::slice::from_ref(it))?;
            }
            for wi in snap.work_items.iter().chain(&corpus.done) {
                write::upsert_work_item(tx, wi)?;
            }
            for run in &snap.runs {
                write::upsert_run(tx, run)?;
            }
            Ok(())
        })
        .expect("seed");
    adoken_nudge::store::migrate(&store).expect("migrate");
    (dir, store)
}

fn bench(c: &mut Criterion) {
    let corpus = corpus();
    let snap = &corpus.snap;
    let settings = Settings::default();
    report(
        "nudge evaluate+today (summary, mood, activity), 0017 corpus",
        BUDGET,
        || {
            let mut ledger = Ledger::default();
            let pass = evaluate(snap, &settings, &mut ledger, NOW);
            black_box(today(snap, &settings, &pass, &ledger, NOW));
        },
    );
    let (_dir, store) = store(&corpus);
    let day_start = settings.hours.day_start(NOW);
    let full = adoken_nudge::store::load_snapshot(&store, DataHealth::FRESH, day_start)
        .expect("read")
        .expect("me");
    // Merges involving me: my half of them plus the ones I reviewed.
    let shape = (
        full.merged_today.len() >= MERGES_TODAY / 2,
        full.done_today.len(),
        full.iterations_today.is_empty(),
        full.state_since.is_empty(),
    );
    assert_eq!(
        shape,
        (true, DONE_TODAY as usize, false, false),
        "the temp store holds the corpus's day (my merges, done, no pushes, no state since)"
    );
    // The snapshot as it stands before the 0017 reads: those fields empty,
    // no threads for my merges unless another read already brought them.
    let mut base = full.clone();
    base.iterations_today.clear();
    base.iterations_behind.clear();
    base.done_today.clear();
    base.state_since.clear();
    let merged: HashSet<i64> = base.merged_today.iter().map(|pr| pr.id).collect();
    let read_before: HashSet<i64> = base.prs.iter().map(|pr| pr.id).collect();
    base.threads
        .retain(|id, _| !merged.contains(id) || read_before.contains(id));
    report_with(
        "0017 added store reads (iterations, merged-today comments, done, state since)",
        Some(READ_BUDGET),
        || base.clone(),
        |snap| adoken_nudge::store::load_day(&store, snap, day_start).expect("read"),
    );
    report_with(
        "load_snapshot, the whole read (reference)",
        None,
        || (),
        |_| {
            black_box(
                adoken_nudge::store::load_snapshot(&store, DataHealth::FRESH, day_start)
                    .expect("read"),
            );
        },
    );
    c.bench_function("evaluate_today_0017_corpus", |b| {
        b.iter(|| {
            let mut ledger = Ledger::default();
            let pass = evaluate(black_box(snap), &settings, &mut ledger, NOW);
            black_box(today(snap, &settings, &pass, &ledger, NOW))
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
