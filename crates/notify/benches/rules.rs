//! Spec 0005 §7: rule evaluation < 50 µs per event with 100 rules.

use std::hint::black_box;

use adoken_model::{RunResult, Subject, SyncEventKind};
use adoken_notify::{
    EventContext, EventRef, Kind, Priority, RuleAction, RuleMatch, Source, UserRule, evaluate,
};
use criterion::{Criterion, criterion_group, criterion_main};

fn rules(count: usize) -> Vec<UserRule> {
    (0..count)
        .map(|i| UserRule {
            name: format!("rule {i}"),
            enabled: true,
            when: RuleMatch {
                source: Some(Source::ALL[i % Source::ALL.len()]),
                repo: Some(format!("repo-{i}")),
                branch: Some("main".into()),
                actor: Some(format!("person-{i}")),
                result: Some(RunResult::Failed),
                ..RuleMatch::default()
            },
            then: RuleAction::SetPriority(Priority::P0),
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let rules = rules(100);
    let ctx = EventContext {
        title: "!4821 Retry idempotency keys".into(),
        repo_id: Some("repo-id".into()),
        repo_name: Some("payments-api".into()),
        pipeline_id: Some(12),
        pipeline_name: Some("payments-ci".into()),
        branch: Some("main".into()),
        actor_name: Some("Priya Shah".into()),
        build_result: Some(RunResult::Failed),
        ..EventContext::default()
    };
    let subject = Subject::Build(99);
    // Worst case: no rule matches, all 100 are evaluated.
    let event = EventRef {
        kind: Kind::Sync(SyncEventKind::BuildCompleted),
        subject: &subject,
        actor: Some("someone-else"),
        ctx: &ctx,
    };
    c.bench_function("evaluate_100_rules_no_match", |b| {
        b.iter(|| evaluate(black_box(&rules), black_box(&event)))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
