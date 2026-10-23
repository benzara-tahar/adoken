//! Run stages for the fake org (spec 0008 FR-5, AC-6): deterministic from
//! each run's result, seeded with the runs so Pipelines shows stages in demo
//! mode like it does for synced build timelines.

use adoken_model::{Millis, PipelineRun, RunResult, RunStage, RunStatus, StageOutcome};

/// Stage names by pipeline kind (fake data; plausible YAML stage names).
fn stage_names(pipeline: &str) -> &'static [&'static str] {
    let name = pipeline.to_ascii_lowercase();
    if name.contains("deploy") || name.contains("release") {
        &["Build", "Test", "Deploy staging", "Deploy production"]
    } else if name.contains("nightly") || name.contains("reindex") {
        &["Prepare", "Run", "Report"]
    } else {
        &["Build", "Test", "Publish"]
    }
}

/// Deterministic stages for a demo run: the run's time split across its
/// stages; a failed / canceled / partial run ends at one stage chosen from
/// its id, everything after it skipped; a running run is mid-way.
pub fn demo_stages(run: &PipelineRun) -> Vec<RunStage> {
    let names = stage_names(&run.pipeline_name);
    let n = names.len();
    // Test-ish stages fail more than builds: pick among stages 1.. by id.
    let pick = if n > 1 {
        1 + (run.id as usize % (n - 1))
    } else {
        0
    };
    let (stop, stop_outcome) = match (run.status, run.result) {
        (_, Some(RunResult::Succeeded)) | (RunStatus::Completed, None) => {
            (n, StageOutcome::Succeeded)
        }
        (_, Some(RunResult::Failed)) => (pick, StageOutcome::Failed),
        (_, Some(RunResult::Canceled)) => (pick, StageOutcome::Canceled),
        (_, Some(RunResult::PartiallySucceeded)) => (pick, StageOutcome::Partial),
        (RunStatus::InProgress | RunStatus::Cancelling, None) => (pick, StageOutcome::Running),
        _ => (0, StageOutcome::Pending),
    };
    // Split the run's span evenly over the stages that ran.
    let ran = match stop_outcome {
        StageOutcome::Succeeded | StageOutcome::Partial => n,
        _ => stop + 1,
    };
    let span = match (run.started, run.finish) {
        (Some(start), Some(end)) => Some((start, end)),
        _ => None,
    };
    let slice = |ix: usize| -> (Option<Millis>, Option<Millis>) {
        match span {
            Some((start, end)) => {
                let step = (end - start) / ran.max(1) as Millis;
                let from = start + step * ix as Millis;
                let to = if ix + 1 == ran { end } else { from + step };
                (Some(from), Some(to))
            }
            None => (None, None),
        }
    };
    names
        .iter()
        .enumerate()
        .map(|(ix, name)| {
            let (outcome, (started, finished)) = if ix < stop {
                (StageOutcome::Succeeded, slice(ix))
            } else if ix == stop {
                match stop_outcome {
                    StageOutcome::Running => (StageOutcome::Running, (run.started, None)),
                    StageOutcome::Pending => (StageOutcome::Pending, (None, None)),
                    outcome => (outcome, slice(ix)),
                }
            } else {
                match stop_outcome {
                    // A partially succeeded run still ran every stage.
                    StageOutcome::Partial => (StageOutcome::Succeeded, slice(ix)),
                    StageOutcome::Running | StageOutcome::Pending => {
                        (StageOutcome::Pending, (None, None))
                    }
                    _ => (StageOutcome::Skipped, (None, None)),
                }
            };
            RunStage {
                name: (*name).to_owned(),
                outcome,
                started,
                finished,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::RunReason;
    use adoken_model::time::MINUTE;

    fn run(id: i64, name: &str, status: RunStatus, result: Option<RunResult>) -> PipelineRun {
        PipelineRun {
            id,
            project_id: "p".into(),
            pipeline_id: 1,
            pipeline_name: name.into(),
            run_number: "1".into(),
            status,
            result,
            reason: RunReason::Manual,
            branch: "refs/heads/main".into(),
            source_version: None,
            requested_for: None,
            pr_id: None,
            queued: 0,
            started: (status != RunStatus::NotStarted).then_some(MINUTE),
            finish: result.map(|_| 13 * MINUTE),
        }
    }

    fn outcomes(stages: &[RunStage]) -> Vec<StageOutcome> {
        stages.iter().map(|s| s.outcome).collect()
    }

    #[test]
    fn failed_runs_fail_one_stage_and_skip_the_rest() {
        for id in 0..6 {
            let stages = demo_stages(&run(
                id,
                "Payments CI",
                RunStatus::Completed,
                Some(RunResult::Failed),
            ));
            let failed = stages
                .iter()
                .filter(|s| s.outcome == StageOutcome::Failed)
                .count();
            assert_eq!(failed, 1, "run {id}");
            let at = stages
                .iter()
                .position(|s| s.outcome == StageOutcome::Failed)
                .expect("failed stage");
            assert!(at > 0, "builds pass; later stages fail");
            assert!(
                stages[..at]
                    .iter()
                    .all(|s| s.outcome == StageOutcome::Succeeded)
            );
            assert!(
                stages[at + 1..]
                    .iter()
                    .all(|s| s.outcome == StageOutcome::Skipped)
            );
            assert_eq!(stages[at].finished, Some(13 * MINUTE), "the run ends there");
        }
    }

    #[test]
    fn succeeded_runs_split_their_time_over_every_stage() {
        let stages = demo_stages(&run(
            1,
            "Checkout deploy",
            RunStatus::Completed,
            Some(RunResult::Succeeded),
        ));
        assert_eq!(stages.len(), 4);
        assert!(
            outcomes(&stages)
                .iter()
                .all(|o| *o == StageOutcome::Succeeded)
        );
        assert_eq!(stages[0].started, Some(MINUTE));
        assert_eq!(stages[3].finished, Some(13 * MINUTE));
        assert_eq!(stages[0].finished, stages[1].started);
    }

    #[test]
    fn running_and_queued_runs_wait_after_the_current_stage() {
        let running = demo_stages(&run(2, "Payments CI", RunStatus::InProgress, None));
        assert_eq!(
            outcomes(&running),
            [
                StageOutcome::Succeeded,
                StageOutcome::Running,
                StageOutcome::Pending
            ]
        );
        assert_eq!(running[1].finished, None);
        let queued = demo_stages(&run(3, "Payments CI", RunStatus::NotStarted, None));
        assert!(
            outcomes(&queued)
                .iter()
                .all(|o| *o == StageOutcome::Pending)
        );
    }

    #[test]
    fn demo_store_holds_every_runs_stages() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = crate::open_store_at(dir.path(), crate::DEMO_NOW).expect("demo store");
        let runs = store
            .runs(&adoken_store::RunFilter::default())
            .expect("runs");
        let failed = runs
            .iter()
            .find(|r| r.result == Some(RunResult::Failed))
            .expect("a failed demo run");
        assert_eq!(
            store.run_stages(failed.id).expect("read"),
            demo_stages(failed)
        );
        assert!(
            runs.iter()
                .all(|r| !store.run_stages(r.id).unwrap().is_empty())
        );
    }
}
