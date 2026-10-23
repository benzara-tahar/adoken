//! Pure view model for the Pipelines view (spec 0008 FR-4): tabs, run
//! filters, definition rows with their sparkline, and run durations. No
//! gpui, no IO.

use std::collections::HashMap;

use adoken_model::{Millis, Pipeline, PipelineRun, RunResult, RunStatus, short_branch};

/// Runs per definition sparkline.
pub const SPARK_RUNS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Pipeline definitions.
    Pipelines,
    /// Runs requested for me.
    Mine,
    /// Failed or partially succeeded runs.
    Failed,
    /// Every run.
    All,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Pipelines, Tab::Mine, Tab::Failed, Tab::All];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Pipelines => "Pipelines",
            Tab::Mine => "Mine",
            Tab::Failed => "Failed",
            Tab::All => "All runs",
        }
    }

    /// Next/previous tab, wrapping (`←` / `→`).
    pub fn step(self, delta: isize) -> Tab {
        let ix = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) as isize;
        let n = Tab::ALL.len() as isize;
        Tab::ALL[(ix + delta).rem_euclid(n) as usize]
    }

    pub fn lists_runs(self) -> bool {
        self != Tab::Pipelines
    }
}

/// Filters layered on the run tabs; they compose with the tab.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    /// Definition id.
    pub pipeline: Option<i64>,
    /// Full ref.
    pub branch: Option<String>,
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        self.pipeline.is_none() && self.branch.is_none()
    }
}

/// Status class of a run: icon, color, sparkline bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Succeeded,
    Partial,
    Failed,
    Canceled,
    Running,
    Queued,
}

impl Outcome {
    pub fn of(run: &PipelineRun) -> Outcome {
        match (run.status, run.result) {
            (_, Some(RunResult::Succeeded)) => Outcome::Succeeded,
            (_, Some(RunResult::PartiallySucceeded)) => Outcome::Partial,
            (_, Some(RunResult::Failed)) => Outcome::Failed,
            (_, Some(RunResult::Canceled)) => Outcome::Canceled,
            (RunStatus::InProgress | RunStatus::Cancelling, None) => Outcome::Running,
            (RunStatus::Completed, None) => Outcome::Succeeded,
            _ => Outcome::Queued,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Outcome::Succeeded => "Succeeded",
            Outcome::Partial => "Partially succeeded",
            Outcome::Failed => "Failed",
            Outcome::Canceled => "Canceled",
            Outcome::Running => "Running",
            Outcome::Queued => "Queued",
        }
    }
}

pub fn is_failed(run: &PipelineRun) -> bool {
    matches!(Outcome::of(run), Outcome::Failed | Outcome::Partial)
}

fn in_tab(run: &PipelineRun, tab: Tab, me: &str) -> bool {
    match tab {
        Tab::Pipelines | Tab::All => true,
        Tab::Mine => run.requested_for.as_deref() == Some(me),
        Tab::Failed => is_failed(run),
    }
}

/// Indices of the runs a run tab lists, in store order (newest queued
/// first). The Pipelines tab lists definitions, not runs.
pub fn runs(runs: &[PipelineRun], tab: Tab, me: &str, filters: &Filters) -> Vec<usize> {
    if !tab.lists_runs() {
        return Vec::new();
    }
    (0..runs.len())
        .filter(|&ix| {
            let run = &runs[ix];
            in_tab(run, tab, me)
                && filters.pipeline.is_none_or(|p| run.pipeline_id == p)
                && filters.branch.as_ref().is_none_or(|b| run.branch == *b)
        })
        .collect()
}

/// Branches seen in a tab's runs (after the pipeline filter), most runs
/// first, then by name.
pub fn branches(runs: &[PipelineRun], tab: Tab, me: &str, pipeline: Option<i64>) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for run in runs
        .iter()
        .filter(|r| in_tab(r, tab, me) && pipeline.is_none_or(|p| r.pipeline_id == p))
    {
        *counts.entry(run.branch.as_str()).or_default() += 1;
    }
    let mut out: Vec<(&str, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    out.into_iter().map(|(b, _)| b.to_owned()).collect()
}

/// Branch filter `delta` steps from `current`: `None` (all branches) sits
/// before the first branch, so stepping cycles all → b1 → … → bn → all.
pub fn cycle_branch(branches: &[String], current: Option<&str>, delta: isize) -> Option<String> {
    let n = branches.len() as isize + 1;
    let at = current
        .and_then(|c| branches.iter().position(|b| b == c))
        .map_or(0, |ix| ix as isize + 1);
    let next = (at + delta).rem_euclid(n);
    (next > 0).then(|| branches[next as usize - 1].clone())
}

/// Wall time a run took, or has taken so far while running.
pub fn duration(run: &PipelineRun, now: Millis) -> Option<Millis> {
    let start = run.started?;
    Some((run.finish.unwrap_or(now) - start).max(0))
}

/// One sparkline bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub run_id: i64,
    pub outcome: Outcome,
    /// 0..=1, relative to the longest run in the sparkline.
    pub height: f32,
}

/// A definition row: the definition, its runs newest first, and the
/// sparkline of its last [`SPARK_RUNS`] runs oldest → newest.
#[derive(Clone, Debug)]
pub struct Definition {
    /// Index into the pipelines.
    pub pipeline: usize,
    /// Indices into the runs, newest queued first.
    pub runs: Vec<usize>,
    pub spark: Vec<Bar>,
}

impl Definition {
    pub fn latest(&self) -> Option<usize> {
        self.runs.first().copied()
    }
}

/// Bars for `recent` (newest first): oldest → newest, height by duration.
/// Queued runs get zero height (drawn as a stub).
pub fn sparkline(recent: &[&PipelineRun], now: Millis) -> Vec<Bar> {
    let picked: Vec<&PipelineRun> = recent.iter().take(SPARK_RUNS).rev().copied().collect();
    let longest = picked
        .iter()
        .filter_map(|r| duration(r, now))
        .max()
        .unwrap_or(0)
        .max(1);
    picked
        .into_iter()
        .map(|run| Bar {
            run_id: run.id,
            outcome: Outcome::of(run),
            height: duration(run, now).map_or(0.0, |d| d as f32 / longest as f32),
        })
        .collect()
}

/// Definitions ordered by latest run (most recent first); never-run
/// definitions last, by name. `runs` must be newest-queued first.
pub fn definitions(pipelines: &[Pipeline], runs: &[PipelineRun], now: Millis) -> Vec<Definition> {
    let mut by_def: HashMap<(&str, i64), Vec<usize>> = HashMap::new();
    for (ix, run) in runs.iter().enumerate() {
        by_def
            .entry((run.project_id.as_str(), run.pipeline_id))
            .or_default()
            .push(ix);
    }
    let mut out: Vec<Definition> = pipelines
        .iter()
        .enumerate()
        .map(|(pix, p)| {
            let own = by_def
                .remove(&(p.project_id.as_str(), p.id))
                .unwrap_or_default();
            let recent: Vec<&PipelineRun> = own.iter().map(|&ix| &runs[ix]).collect();
            Definition {
                pipeline: pix,
                spark: sparkline(&recent, now),
                runs: own,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        let latest = |d: &Definition| d.latest().map(|ix| runs[ix].queued);
        latest(b)
            .cmp(&latest(a))
            .then_with(|| pipelines[a.pipeline].name.cmp(&pipelines[b.pipeline].name))
    });
    out
}

/// Summary for the definition detail.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub runs: usize,
    /// Succeeded / (succeeded + partial + failed); canceled runs don't count.
    pub pass_rate: Option<f32>,
    /// Median duration of finished runs.
    pub median: Option<Millis>,
}

pub fn stats(runs: &[&PipelineRun]) -> Stats {
    let decided: Vec<Outcome> = runs
        .iter()
        .map(|r| Outcome::of(r))
        .filter(|o| matches!(o, Outcome::Succeeded | Outcome::Partial | Outcome::Failed))
        .collect();
    let passed = decided.iter().filter(|o| **o == Outcome::Succeeded).count();
    let mut finished: Vec<Millis> = runs
        .iter()
        .filter(|r| r.finish.is_some())
        .filter_map(|r| duration(r, 0))
        .collect();
    finished.sort_unstable();
    Stats {
        runs: runs.len(),
        pass_rate: (!decided.is_empty()).then(|| passed as f32 / decided.len() as f32),
        median: finished.get(finished.len() / 2).copied(),
    }
}

/// Display branch: `main`, or `PR !4291` for PR merge refs.
pub fn branch_label(full: &str) -> String {
    match full
        .strip_prefix("refs/pull/")
        .and_then(|rest| rest.strip_suffix("/merge"))
    {
        Some(id) => format!("PR !{id}"),
        None => short_branch(full).to_owned(),
    }
}

/// Human reason (ADO's `reason`).
pub fn reason_label(reason: adoken_model::RunReason) -> &'static str {
    use adoken_model::RunReason;
    match reason {
        RunReason::Manual => "Manual",
        RunReason::IndividualCi | RunReason::BatchedCi => "CI",
        RunReason::PullRequest => "Pull request",
        RunReason::Schedule => "Scheduled",
        RunReason::Other => "Other",
    }
}

/// `38s`, `4m 12s`, `1h 3m`.
pub fn format_duration(ms: Millis) -> String {
    let secs = ms / 1000;
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::RunReason;
    use adoken_model::time::{HOUR, MINUTE};

    const ME: &str = "me";
    const NOW: Millis = 100 * HOUR;

    fn run(id: i64, pipeline: i64, result: Option<RunResult>, mins: i64) -> PipelineRun {
        let queued = NOW - (1000 - id) * MINUTE;
        PipelineRun {
            id,
            project_id: "p".into(),
            pipeline_id: pipeline,
            pipeline_name: format!("pipe {pipeline}"),
            run_number: format!("2026.{id}"),
            status: if result.is_some() {
                RunStatus::Completed
            } else {
                RunStatus::InProgress
            },
            result,
            reason: RunReason::IndividualCi,
            branch: "refs/heads/main".into(),
            source_version: None,
            requested_for: Some("them".into()),
            pr_id: None,
            queued,
            started: Some(queued),
            finish: result.map(|_| queued + mins * MINUTE),
        }
    }

    fn pipeline(id: i64, name: &str) -> Pipeline {
        Pipeline {
            id,
            project_id: "p".into(),
            name: name.into(),
            folder: "\\".into(),
        }
    }

    /// Store order: newest queued first.
    fn newest_first(mut runs: Vec<PipelineRun>) -> Vec<PipelineRun> {
        runs.sort_by_key(|r| std::cmp::Reverse(r.queued));
        runs
    }

    #[test]
    fn tabs_and_filters_compose_and_clearing_restores_the_tab() {
        let mut all = vec![
            run(1, 1, Some(RunResult::Succeeded), 5),
            run(2, 1, Some(RunResult::Failed), 5),
            run(3, 2, Some(RunResult::PartiallySucceeded), 5),
            run(4, 2, Some(RunResult::Canceled), 5),
            run(5, 1, None, 0),
        ];
        all[1].requested_for = Some(ME.into());
        all[2].requested_for = Some(ME.into());
        all[2].branch = "refs/heads/feature".into();
        let all = newest_first(all);
        let ids = |ixs: Vec<usize>| ixs.into_iter().map(|ix| all[ix].id).collect::<Vec<_>>();
        let none = Filters::default();
        assert_eq!(ids(runs(&all, Tab::All, ME, &none)), [5, 4, 3, 2, 1]);
        assert_eq!(ids(runs(&all, Tab::Mine, ME, &none)), [3, 2]);
        assert_eq!(ids(runs(&all, Tab::Failed, ME, &none)), [3, 2]);
        assert!(runs(&all, Tab::Pipelines, ME, &none).is_empty());
        let pipe1 = Filters {
            pipeline: Some(1),
            branch: None,
        };
        assert_eq!(ids(runs(&all, Tab::Failed, ME, &pipe1)), [2]);
        let feature = Filters {
            pipeline: None,
            branch: Some("refs/heads/feature".into()),
        };
        assert_eq!(ids(runs(&all, Tab::Mine, ME, &feature)), [3]);
        let both = Filters {
            pipeline: Some(1),
            branch: Some("refs/heads/feature".into()),
        };
        assert!(runs(&all, Tab::All, ME, &both).is_empty());
        assert!(!both.is_empty() && none.is_empty());
    }

    #[test]
    fn branches_rank_by_runs_and_cycle_through_all() {
        let mut all = vec![
            run(1, 1, Some(RunResult::Succeeded), 5),
            run(2, 1, Some(RunResult::Succeeded), 5),
            run(3, 2, Some(RunResult::Succeeded), 5),
        ];
        all[0].branch = "refs/pull/7/merge".into();
        let branches = branches(&all, Tab::All, ME, None);
        assert_eq!(branches, ["refs/heads/main", "refs/pull/7/merge"]);
        assert_eq!(
            super::branches(&all, Tab::All, ME, Some(2)),
            ["refs/heads/main"]
        );
        assert_eq!(
            cycle_branch(&branches, None, 1).as_deref(),
            Some("refs/heads/main")
        );
        assert_eq!(
            cycle_branch(&branches, Some("refs/heads/main"), 1).as_deref(),
            Some("refs/pull/7/merge")
        );
        assert_eq!(cycle_branch(&branches, Some("refs/pull/7/merge"), 1), None);
        assert_eq!(
            cycle_branch(&branches, None, -1).as_deref(),
            Some("refs/pull/7/merge")
        );
        assert_eq!(cycle_branch(&[], None, 1), None);
        assert_eq!(branch_label("refs/pull/7/merge"), "PR !7");
        assert_eq!(branch_label("refs/heads/release/1.2"), "release/1.2");
    }

    #[test]
    fn sparkline_is_last_twelve_oldest_first_scaled_to_the_longest() {
        let mut all: Vec<PipelineRun> = (1..=14)
            .map(|id| run(id, 1, Some(RunResult::Succeeded), id))
            .collect();
        all[13].result = Some(RunResult::Failed);
        let all = newest_first(all);
        let recent: Vec<&PipelineRun> = all.iter().collect();
        let bars = sparkline(&recent, NOW);
        assert_eq!(bars.len(), SPARK_RUNS);
        assert_eq!(bars.first().map(|b| b.run_id), Some(3));
        assert_eq!(bars.last().map(|b| b.run_id), Some(14));
        assert_eq!(bars.last().map(|b| b.outcome), Some(Outcome::Failed));
        assert_eq!(bars.last().map(|b| b.height), Some(1.0));
        assert!((bars[0].height - 3.0 / 14.0).abs() < 1e-6);
    }

    #[test]
    fn running_bars_use_elapsed_time_and_queued_bars_are_flat() {
        let mut running = run(2, 1, None, 0);
        running.started = Some(NOW - 10 * MINUTE);
        let mut queued = run(3, 1, None, 0);
        queued.status = RunStatus::NotStarted;
        queued.started = None;
        let done = run(1, 1, Some(RunResult::Succeeded), 5);
        let bars = sparkline(&[&queued, &running, &done], NOW);
        assert_eq!(
            bars.iter().map(|b| b.outcome).collect::<Vec<_>>(),
            [Outcome::Succeeded, Outcome::Running, Outcome::Queued]
        );
        assert_eq!(bars[1].height, 1.0);
        assert_eq!(bars[0].height, 0.5);
        assert_eq!(bars[2].height, 0.0);
    }

    #[test]
    fn definitions_order_by_latest_run_and_never_run_last() {
        let pipelines = [pipeline(1, "api"), pipeline(2, "web"), pipeline(3, "docs")];
        let all = newest_first(vec![
            run(1, 1, Some(RunResult::Succeeded), 5),
            run(2, 2, Some(RunResult::Failed), 5),
            run(3, 1, Some(RunResult::Succeeded), 5),
        ]);
        let defs = definitions(&pipelines, &all, NOW);
        let names: Vec<&str> = defs
            .iter()
            .map(|d| pipelines[d.pipeline].name.as_str())
            .collect();
        assert_eq!(names, ["api", "web", "docs"]);
        assert_eq!(defs[0].latest().map(|ix| all[ix].id), Some(3));
        assert_eq!(defs[0].runs.len(), 2);
        assert!(defs[2].spark.is_empty());
    }

    #[test]
    fn stats_ignore_canceled_and_unfinished_runs() {
        let all = [
            run(1, 1, Some(RunResult::Succeeded), 4),
            run(2, 1, Some(RunResult::Failed), 10),
            run(3, 1, Some(RunResult::Canceled), 1),
            run(4, 1, Some(RunResult::Succeeded), 6),
            run(5, 1, None, 0),
        ];
        let refs: Vec<&PipelineRun> = all.iter().collect();
        let s = stats(&refs);
        assert_eq!(s.runs, 5);
        assert_eq!(s.pass_rate, Some(2.0 / 3.0));
        // Finished: 1, 4, 6, 10 minutes → upper median.
        assert_eq!(s.median, Some(6 * MINUTE));
        assert_eq!(stats(&[]), Stats::default());
    }

    #[test]
    fn durations_format_compactly() {
        assert_eq!(format_duration(38_000), "38s");
        assert_eq!(format_duration(4 * MINUTE + 12_000), "4m 12s");
        assert_eq!(format_duration(HOUR + 3 * MINUTE), "1h 3m");
        let mut live = run(1, 1, None, 0);
        live.started = Some(NOW - 90_000);
        assert_eq!(duration(&live, NOW), Some(90_000));
        live.started = None;
        assert_eq!(duration(&live, NOW), None);
    }
}
