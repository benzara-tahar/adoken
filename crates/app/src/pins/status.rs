//! A pin's one headline signal (spec 0014 FR-9) and the words built from it:
//! the folded summary (pins.md §2.2), tooltips (§3.8, §6.4) and the spoken
//! label (FR-17, §11.1). Pure: no gpui, no IO; the glyphs and tints live in
//! [`super::render`].

use adoken_model::{
    MergeStatus, Millis, PinSubject, PinUnavailable, PipelineRun, PrStatus, PullRequest, RunResult,
    RunStatus, StateCategory, Vote, WorkItemType,
    time::{HOUR, MINUTE},
};

use crate::pipelines::model::Outcome;

/// What a pin is, for its glyph shape and spoken kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Pr {
        draft: bool,
    },
    /// `None` until the item is stored (type unknown).
    WorkItem(Option<WorkItemType>),
    Definition,
}

impl Kind {
    pub fn of(subject: &PinSubject) -> Kind {
        match subject {
            PinSubject::Pr(_) => Kind::Pr { draft: false },
            PinSubject::WorkItem(_) => Kind::WorkItem(None),
            PinSubject::Definition { .. } => Kind::Definition,
        }
    }

    /// "pull request", "draft pull request", "work item", "pipeline".
    fn spoken(&self) -> &'static str {
        match self {
            Kind::Pr { draft: true } => "draft pull request",
            Kind::Pr { draft: false } => "pull request",
            Kind::WorkItem(_) => "work item",
            Kind::Definition => "pipeline",
        }
    }
}

/// PR headlines in FR-9 precedence order (first match wins).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrHeadline {
    Completed,
    Abandoned,
    MergeConflict,
    CiFailing,
    /// A reviewer voted Reject (`rejected`) or Wait for author.
    ChangesRequested {
        rejected: bool,
    },
    CiRunning,
    Approved,
    Draft,
    Active,
}

/// A definition's latest run (FR-9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionHeadline {
    /// Queued or running; `since` = start (or queue) time for the elapsed.
    Running {
        since: Millis,
    },
    Failed,
    Partial,
    Canceled,
    Succeeded,
    /// No run within retention.
    NoRuns,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Headline {
    Pr(PrHeadline),
    WorkItem {
        state: String,
        category: StateCategory,
    },
    Definition(DefinitionHeadline),
    /// Pinned by id or URL, not stored yet (§3.5 fetching row).
    Fetching,
    /// 404 / 403 on refresh (FR-13).
    NotAvailable(PinUnavailable),
}

/// FR-9 for a PR. `ci` is the worst latest run of its pipelines
/// ([`crate::review::data::checks`]).
pub fn pr_headline(pr: &PullRequest, ci: Option<Outcome>) -> PrHeadline {
    let votes = || pr.reviewers.iter().filter(|r| !r.has_declined);
    match pr.status {
        PrStatus::Completed => return PrHeadline::Completed,
        PrStatus::Abandoned => return PrHeadline::Abandoned,
        PrStatus::Active => {}
    }
    if pr.merge_status == MergeStatus::Conflicts {
        return PrHeadline::MergeConflict;
    }
    if ci == Some(Outcome::Failed) {
        return PrHeadline::CiFailing;
    }
    let rejected = votes().any(|r| r.vote == Vote::Rejected);
    if rejected || votes().any(|r| r.vote == Vote::WaitingForAuthor) {
        return PrHeadline::ChangesRequested { rejected };
    }
    if matches!(ci, Some(Outcome::Running | Outcome::Queued)) {
        return PrHeadline::CiRunning;
    }
    if votes().any(|r| matches!(r.vote, Vote::Approved | Vote::ApprovedWithSuggestions)) {
        return PrHeadline::Approved;
    }
    if pr.is_draft {
        PrHeadline::Draft
    } else {
        PrHeadline::Active
    }
}

/// FR-9 for a definition: its latest run, newest queued first.
pub fn definition_headline(latest: Option<&PipelineRun>) -> DefinitionHeadline {
    let Some(run) = latest else {
        return DefinitionHeadline::NoRuns;
    };
    match (run.status, run.result) {
        (_, Some(RunResult::Failed)) => DefinitionHeadline::Failed,
        (_, Some(RunResult::PartiallySucceeded)) => DefinitionHeadline::Partial,
        (_, Some(RunResult::Canceled)) => DefinitionHeadline::Canceled,
        (_, Some(RunResult::Succeeded)) | (RunStatus::Completed, None) => {
            DefinitionHeadline::Succeeded
        }
        _ => DefinitionHeadline::Running {
            since: run.started.unwrap_or(run.queued),
        },
    }
}

/// Elapsed time of a running definition: `<1m`, `12m`, `1h 5m` (§3.3).
pub fn elapsed(ms: Millis) -> String {
    let minutes = ms.max(0) / MINUTE;
    match minutes {
        0 => "<1m".to_owned(),
        m if m < 60 => format!("{m}m"),
        m => format!("{}h {}m", m / 60, m % 60),
    }
}

/// "Updated 18m ago" / "Updated 2h ago" for stale rows.
pub fn updated_ago(updated: Millis, now: Millis) -> String {
    let ago = (now - updated).max(0);
    if ago < HOUR {
        format!("Updated {}m ago", ago / MINUTE)
    } else {
        format!("Updated {}h ago", ago / HOUR)
    }
}

impl Headline {
    /// The status word: tooltips, `⌘J` subtitles, the spoken label.
    pub fn word(&self, now: Millis) -> String {
        match self {
            Headline::Pr(h) => match h {
                PrHeadline::Completed => "Completed",
                PrHeadline::Abandoned => "Abandoned",
                PrHeadline::MergeConflict => "Merge conflict",
                PrHeadline::CiFailing => "CI failing",
                PrHeadline::ChangesRequested { .. } => "Changes requested",
                PrHeadline::CiRunning => "CI running",
                PrHeadline::Approved => "Approved",
                PrHeadline::Draft => "Draft",
                PrHeadline::Active => "Active",
            }
            .to_owned(),
            Headline::WorkItem { state, .. } => state.clone(),
            Headline::Definition(h) => match h {
                DefinitionHeadline::Running { since } => {
                    format!("Running {}", elapsed(now - since))
                }
                DefinitionHeadline::Failed => "Failed".to_owned(),
                DefinitionHeadline::Partial => "Partially succeeded".to_owned(),
                DefinitionHeadline::Canceled => "Canceled".to_owned(),
                DefinitionHeadline::Succeeded => "Succeeded".to_owned(),
                DefinitionHeadline::NoRuns => "No runs".to_owned(),
            },
            Headline::Fetching => "Fetching…".to_owned(),
            Headline::NotAvailable(_) => "Not available".to_owned(),
        }
    }

    /// Counts toward "1 failing" (§2.2): red build results only.
    pub fn is_failing(&self) -> bool {
        matches!(
            self,
            Headline::Pr(PrHeadline::CiFailing) | Headline::Definition(DefinitionHeadline::Failed)
        )
    }

    /// The item closed (FR-12 candidates).
    pub fn is_closed(&self) -> bool {
        match self {
            Headline::Pr(h) => matches!(h, PrHeadline::Completed | PrHeadline::Abandoned),
            Headline::WorkItem { category, .. } => {
                matches!(category, StateCategory::Completed | StateCategory::Removed)
            }
            _ => false,
        }
    }
}

/// Red for the change dot of a definition (FR-10): failed or partially
/// succeeded; canceled runs don't count either way.
pub fn is_red(result: RunResult) -> bool {
    matches!(result, RunResult::Failed | RunResult::PartiallySucceeded)
}

/// FR-10 for a definition: the latest completed (not canceled) result
/// differs in color from the one shown when it was last opened.
pub fn definition_changed(seen: Option<RunResult>, latest: Option<RunResult>) -> bool {
    match (seen, latest) {
        (Some(seen), Some(latest)) => is_red(seen) != is_red(latest),
        _ => false,
    }
}

/// FR-12: a closed PR or work item leaves the pins 24 h after it closed;
/// reopened, it stays; definitions never auto-unpin.
pub fn auto_unpin_due(
    headline: &Headline,
    closed_at: Option<Millis>,
    now: Millis,
    enabled: bool,
) -> bool {
    enabled && headline.is_closed() && closed_at.is_some_and(|at| now - at >= 24 * HOUR)
}

/// The folded group's summary (§2.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub total: usize,
    pub changed: usize,
    pub failing: usize,
}

impl Summary {
    /// "5 · 2 changed · 1 failing", "5 · all quiet".
    pub fn full(&self) -> String {
        let mut parts = vec![self.total.to_string()];
        if self.changed > 0 {
            parts.push(format!("{} changed", self.changed));
        }
        if self.failing > 0 {
            parts.push(format!("{} failing", self.failing));
        }
        if self.changed == 0 && self.failing == 0 {
            parts.push("all quiet".to_owned());
        }
        parts.join(" · ")
    }

    /// The compact form when the full one doesn't fit: "5 · ●2 · ✕1"
    /// ("all quiet" never compacts).
    pub fn compact(&self) -> String {
        if self.changed == 0 && self.failing == 0 {
            return self.full();
        }
        let mut parts = vec![self.total.to_string()];
        if self.changed > 0 {
            parts.push(format!("●{}", self.changed));
        }
        if self.failing > 0 {
            parts.push(format!("✕{}", self.failing));
        }
        parts.join(" · ")
    }
}

/// Inputs of a pin's tooltip and spoken label.
pub struct Describe<'a> {
    pub subject: &'a PinSubject,
    pub kind: &'a Kind,
    pub label: Option<&'a str>,
    /// The Azure DevOps title (last known).
    pub title: &'a str,
    pub headline: &'a Headline,
    pub changed: bool,
    /// When the data is older than the stale threshold.
    pub stale_since: Option<Millis>,
    pub now: Millis,
}

impl Describe<'_> {
    fn ref_title(&self) -> String {
        match self.subject.reference() {
            Some(reference) if self.title.is_empty() => reference,
            Some(reference) => format!("{reference} {}", self.title),
            None => self.title.to_owned(),
        }
    }

    /// §3.8 / §6.4: "!4821 Retry idempotency keys · Merge conflict ·
    /// Changed since you opened it".
    pub fn tooltip(&self) -> String {
        let head = match self.label {
            Some(label) => format!("{label} ({})", self.ref_title()),
            None => self.ref_title(),
        };
        let mut parts = vec![head];
        match self.headline {
            Headline::Fetching => parts.push("Fetching…".to_owned()),
            Headline::NotAvailable(why) => parts.push(format!(
                "Not available: {}",
                match why {
                    PinUnavailable::Gone => "deleted or moved",
                    PinUnavailable::Forbidden => "your token can't read it",
                }
            )),
            headline => parts.push(headline.word(self.now)),
        }
        if self.changed {
            parts.push("Changed since you opened it".to_owned());
        }
        if let Some(at) = self.stale_since {
            parts.push(updated_ago(at, self.now));
        }
        parts.join(" · ")
    }

    /// FR-17: "Pinned pull request 4821, Retry idempotency keys, merge
    /// conflict, changed since you opened it".
    pub fn spoken(&self) -> String {
        let id = match self.subject {
            PinSubject::Pr(id) | PinSubject::WorkItem(id) => format!(" {id}"),
            PinSubject::Definition { .. } => String::new(),
        };
        let mut parts = vec![format!("Pinned {}{id}", self.kind.spoken())];
        if matches!(self.headline, Headline::Fetching) {
            parts.push("loading".to_owned());
            return parts.join(", ");
        }
        parts.push(self.label.unwrap_or(self.title).to_owned());
        if self.label.is_some() {
            parts.push(format!("titled {}", self.title));
        }
        if let Kind::WorkItem(Some(kind)) = self.kind {
            parts.push(kind.as_str().to_lowercase());
        }
        match self.headline {
            Headline::NotAvailable(why) => {
                parts.push("not available".to_owned());
                parts.push(
                    match why {
                        PinUnavailable::Gone => "deleted or moved",
                        PinUnavailable::Forbidden => "your token can't read it",
                    }
                    .to_owned(),
                );
            }
            Headline::Definition(DefinitionHeadline::Running { since }) => {
                parts.push("running".to_owned());
                parts.push(format!(
                    "running for {} minutes",
                    (self.now - since).max(0) / MINUTE
                ));
            }
            headline => parts.push(headline.word(self.now).to_lowercase()),
        }
        if let Some(at) = self.stale_since {
            parts.push(format!(
                "updated {} minutes ago",
                (self.now - at).max(0) / MINUTE
            ));
        }
        if self.changed {
            parts.push("changed since you opened it".to_owned());
        }
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::{Reviewer, RunReason};

    fn pr() -> PullRequest {
        PullRequest {
            id: 4821,
            repo_id: "r".into(),
            project_id: "p".into(),
            title: "Retry idempotency keys".into(),
            description: String::new(),
            author: "me".into(),
            reviewers: Vec::new(),
            status: PrStatus::Active,
            is_draft: false,
            merge_status: MergeStatus::Succeeded,
            source_branch: "refs/heads/x".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: None,
            created: 0,
            updated: 0,
            closed: None,
            iterations: 1,
            last_push: None,
        }
    }

    fn vote(vote: Vote) -> Reviewer {
        Reviewer {
            person: format!("{vote:?}"),
            vote,
            is_required: false,
            has_declined: false,
            added_at: None,
            voted_at: None,
        }
    }

    /// AC-6 and every FR-9 PR rule, in precedence order: each case adds
    /// the next lower signal and checks the higher one still wins.
    #[test]
    fn pr_headline_precedence_is_first_match_wins() {
        let mut p = pr();
        p.is_draft = true;
        assert_eq!(pr_headline(&p, None), PrHeadline::Draft);
        p.is_draft = false;
        assert_eq!(pr_headline(&p, None), PrHeadline::Active);
        p.is_draft = true;
        p.reviewers.push(vote(Vote::ApprovedWithSuggestions));
        assert_eq!(pr_headline(&p, None), PrHeadline::Approved);
        assert_eq!(
            pr_headline(&p, Some(Outcome::Running)),
            PrHeadline::CiRunning
        );
        assert_eq!(
            pr_headline(&p, Some(Outcome::Queued)),
            PrHeadline::CiRunning
        );
        p.reviewers.push(vote(Vote::WaitingForAuthor));
        assert_eq!(
            pr_headline(&p, Some(Outcome::Running)),
            PrHeadline::ChangesRequested { rejected: false }
        );
        p.reviewers.push(vote(Vote::Rejected));
        assert_eq!(
            pr_headline(&p, None),
            PrHeadline::ChangesRequested { rejected: true }
        );
        assert_eq!(
            pr_headline(&p, Some(Outcome::Failed)),
            PrHeadline::CiFailing
        );
        // AC-6: conflict + failing CI → Merge conflict; cleared → CI
        // failing; completed → Completed.
        p.merge_status = MergeStatus::Conflicts;
        assert_eq!(
            pr_headline(&p, Some(Outcome::Failed)),
            PrHeadline::MergeConflict
        );
        p.merge_status = MergeStatus::Succeeded;
        assert_eq!(
            pr_headline(&p, Some(Outcome::Failed)),
            PrHeadline::CiFailing
        );
        p.merge_status = MergeStatus::Conflicts;
        p.status = PrStatus::Completed;
        assert_eq!(
            pr_headline(&p, Some(Outcome::Failed)),
            PrHeadline::Completed
        );
        p.status = PrStatus::Abandoned;
        assert_eq!(pr_headline(&p, None), PrHeadline::Abandoned);
    }

    #[test]
    fn declined_reviewers_and_partial_ci_are_not_signals() {
        let mut p = pr();
        let mut declined = vote(Vote::Rejected);
        declined.has_declined = true;
        p.reviewers.push(declined);
        assert_eq!(pr_headline(&p, Some(Outcome::Partial)), PrHeadline::Active);
    }

    fn run(status: RunStatus, result: Option<RunResult>) -> PipelineRun {
        PipelineRun {
            id: 1,
            project_id: "p".into(),
            pipeline_id: 7,
            pipeline_name: "nightly-e2e".into(),
            run_number: "1".into(),
            status,
            result,
            reason: RunReason::Schedule,
            branch: "refs/heads/main".into(),
            source_version: None,
            requested_for: None,
            pr_id: None,
            queued: 0,
            started: Some(1_000),
            finish: None,
        }
    }

    /// AC-6: a running latest run shows "Running" and its elapsed time;
    /// every other result maps to its word.
    #[test]
    fn definition_headline_follows_the_latest_run() {
        let running = definition_headline(Some(&run(RunStatus::InProgress, None)));
        assert_eq!(running, DefinitionHeadline::Running { since: 1_000 });
        let headline = Headline::Definition(running);
        assert_eq!(headline.word(1_000 + 12 * MINUTE), "Running 12m");
        assert_eq!(headline.word(1_000 + 30_000), "Running <1m");
        assert_eq!(headline.word(1_000 + 65 * MINUTE), "Running 1h 5m");
        let word = |status, result| {
            Headline::Definition(definition_headline(Some(&run(status, result)))).word(0)
        };
        assert_eq!(
            word(RunStatus::Completed, Some(RunResult::Failed)),
            "Failed"
        );
        assert_eq!(
            word(RunStatus::Completed, Some(RunResult::PartiallySucceeded)),
            "Partially succeeded"
        );
        assert_eq!(
            word(RunStatus::Completed, Some(RunResult::Canceled)),
            "Canceled"
        );
        assert_eq!(
            word(RunStatus::Completed, Some(RunResult::Succeeded)),
            "Succeeded"
        );
        assert_eq!(
            Headline::Definition(definition_headline(None)).word(0),
            "No runs"
        );
    }

    /// AC-7 (definition half): five green runs since opening don't light
    /// the dot; a red one does; canceled runs are skipped by the caller.
    #[test]
    fn definition_dot_lights_on_color_changes_only() {
        let green = Some(RunResult::Succeeded);
        assert!(!definition_changed(green, green));
        assert!(definition_changed(green, Some(RunResult::Failed)));
        assert!(definition_changed(Some(RunResult::Failed), green));
        assert!(!definition_changed(
            Some(RunResult::Failed),
            Some(RunResult::PartiallySucceeded)
        ));
        assert!(!definition_changed(None, Some(RunResult::Failed)));
    }

    /// AC-8 with an injected clock: listed at T+23h59m, gone at T+24h; a
    /// reopened work item stays; `auto_unpin = false` keeps it at T+72h; a
    /// definition never auto-unpins.
    #[test]
    fn closed_items_leave_24h_after_closing() {
        let t = 1_000_000;
        let completed = Headline::Pr(PrHeadline::Completed);
        assert!(!auto_unpin_due(
            &completed,
            Some(t),
            t + 24 * HOUR - MINUTE,
            true
        ));
        assert!(auto_unpin_due(&completed, Some(t), t + 24 * HOUR, true));
        assert!(!auto_unpin_due(&completed, Some(t), t + 72 * HOUR, false));
        let reopened = Headline::WorkItem {
            state: "Active".into(),
            category: StateCategory::InProgress,
        };
        assert!(!auto_unpin_due(&reopened, Some(t), t + 30 * HOUR, true));
        let removed = Headline::WorkItem {
            state: "Removed".into(),
            category: StateCategory::Removed,
        };
        assert!(auto_unpin_due(&removed, Some(t), t + 24 * HOUR, true));
        let quiet = Headline::Definition(DefinitionHeadline::NoRuns);
        assert!(!auto_unpin_due(&quiet, Some(t), t + 30 * 24 * HOUR, true));
    }

    /// AC-12's line and the §2.2 variants.
    #[test]
    fn summary_reads_changed_failing_or_all_quiet() {
        let s = |total, changed, failing| Summary {
            total,
            changed,
            failing,
        };
        assert_eq!(s(5, 2, 1).full(), "5 · 2 changed · 1 failing");
        assert_eq!(s(5, 2, 0).full(), "5 · 2 changed");
        assert_eq!(s(5, 0, 1).full(), "5 · 1 failing");
        assert_eq!(s(5, 0, 0).full(), "5 · all quiet");
        assert_eq!(s(5, 2, 1).compact(), "5 · ●2 · ✕1");
        assert_eq!(s(4, 0, 0).compact(), "4 · all quiet");
        assert!(Headline::Pr(PrHeadline::CiFailing).is_failing());
        assert!(!Headline::Pr(PrHeadline::MergeConflict).is_failing());
        assert!(!Headline::Definition(DefinitionHeadline::Partial).is_failing());
    }

    #[test]
    fn tooltip_and_spoken_label_follow_the_design_copy() {
        let subject = PinSubject::Pr(4821);
        let kind = Kind::Pr { draft: false };
        let headline = Headline::Pr(PrHeadline::MergeConflict);
        let d = Describe {
            subject: &subject,
            kind: &kind,
            label: None,
            title: "Retry idempotency keys",
            headline: &headline,
            changed: true,
            stale_since: None,
            now: 0,
        };
        assert_eq!(
            d.tooltip(),
            "!4821 Retry idempotency keys · Merge conflict · Changed since you opened it"
        );
        assert_eq!(
            d.spoken(),
            "Pinned pull request 4821, Retry idempotency keys, merge conflict, changed since you opened it"
        );
        let def = PinSubject::Definition {
            project_id: "p".into(),
            id: 4,
        };
        let failed = Headline::Definition(DefinitionHeadline::Failed);
        let labeled = Describe {
            subject: &def,
            kind: &Kind::Definition,
            label: Some("Payments CI"),
            title: "payments-ci",
            headline: &failed,
            changed: false,
            stale_since: Some(0),
            now: 18 * MINUTE,
        };
        assert_eq!(
            labeled.tooltip(),
            "Payments CI (payments-ci) · Failed · Updated 18m ago"
        );
        let gone = Headline::NotAvailable(PinUnavailable::Gone);
        let missing = Describe {
            headline: &gone,
            changed: false,
            ..d
        };
        assert_eq!(
            missing.tooltip(),
            "!4821 Retry idempotency keys · Not available: deleted or moved"
        );
    }
}
