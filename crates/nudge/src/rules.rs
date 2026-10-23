//! Rules N1–N7 (spec 0006 FR-1). Each rule checks its *condition* only and
//! reports how long it has held; thresholds, opt-outs, caps and staleness
//! are applied by [`crate::engine`]. The Today card reads the same
//! conditions without thresholds.

use std::collections::{HashMap, HashSet};

use adoken_model::{
    MergeStatus, Millis, PipelineRun, PrStatus, PrThread, PullRequest, RunResult, RunStatus,
    StateCategory, Subject, Vote, WorkItem,
};

use crate::copy;
use crate::hours::WorkingHours;
use crate::kind::NudgeKind;
use crate::snapshot::Snapshot;

/// The one primary action a nudge offers (§3.2). adoken never acts on
/// anyone's behalf: every action opens something locally or in ADO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NudgeAction {
    ReviewNow,
    Open,
    /// Opens the failing run.
    OpenFailingRun(i64),
    /// N6: v0.1 has no work item write scope, so closing happens in ADO.
    OpenInAdo,
    /// N7: opens the item in ADO.
    StillOnIt,
}

impl NudgeAction {
    pub fn label(self) -> &'static str {
        match self {
            NudgeAction::ReviewNow => "Review now",
            NudgeAction::Open => "Open",
            NudgeAction::OpenFailingRun(_) => "Open failing run",
            NudgeAction::OpenInAdo => "Open in ADO",
            NudgeAction::StillOnIt => "Still on it?",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nudge {
    pub kind: NudgeKind,
    pub subject: Subject,
    /// When the condition started holding.
    pub since: Millis,
    /// Working time since `since` (FR-5).
    pub waited: Millis,
    /// `!912 Add login throttle` / `#48213 Fix SSO redirect`.
    pub label: String,
    /// One sentence per §3.2.
    pub message: String,
    /// Second line, e.g. N4's "Waiting on Kim (required), Sam".
    pub detail: Option<String>,
    pub action: NudgeAction,
    /// N4 only: reviewers without a vote, required first (FR-9). Names only;
    /// "Copy names" is the most adoken does with them.
    pub ping: Vec<String>,
    /// N3: the failing run; N6: the merged PR.
    pub related: Option<Subject>,
}

impl Nudge {
    /// Stable identity of this nudge: `N1:pr:912`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.id(), self.subject.key())
    }
}

/// Lookup tables built once per evaluation.
struct Index<'a> {
    snap: &'a Snapshot,
    prs: HashMap<i64, &'a PullRequest>,
    latest_run: HashMap<i64, &'a PipelineRun>,
    no_threads: Vec<PrThread>,
}

impl<'a> Index<'a> {
    fn new(snap: &'a Snapshot) -> Self {
        let prs = snap.prs.iter().map(|pr| (pr.id, pr)).collect();
        let mut latest_run: HashMap<i64, &PipelineRun> = HashMap::new();
        for run in &snap.runs {
            let Some(pr_id) = run.pr_id else { continue };
            let slot = latest_run.entry(pr_id).or_insert(run);
            if (run.queued, run.id) > (slot.queued, slot.id) {
                *slot = run;
            }
        }
        Self {
            snap,
            prs,
            latest_run,
            no_threads: Vec::new(),
        }
    }

    fn threads(&self, pr: i64) -> &[PrThread] {
        self.snap.threads.get(&pr).unwrap_or(&self.no_threads)
    }

    fn is_me(&self, id: &str) -> bool {
        id == self.snap.me
    }

    /// Human, non-deleted comments on `pr`.
    fn human_comments(&self, pr: i64) -> impl Iterator<Item = &adoken_model::Comment> {
        self.threads(pr)
            .iter()
            .flat_map(|t| &t.comments)
            .filter(|c| !c.is_system && !c.is_deleted)
    }
}

fn pr_label(pr: &PullRequest) -> String {
    format!("!{} {}", pr.id, pr.title)
}

fn wi_label(wi: &WorkItem) -> String {
    format!("#{} {}", wi.id, wi.title)
}

fn mine_active(ix: &Index, pr: &PullRequest) -> bool {
    pr.status == PrStatus::Active && ix.is_me(&pr.author)
}

struct Found {
    kind: NudgeKind,
    subject: Subject,
    since: Millis,
    label: String,
    message: String,
    detail: Option<String>,
    action: NudgeAction,
    ping: Vec<String>,
    related: Option<Subject>,
}

impl Found {
    fn pr(kind: NudgeKind, pr: &PullRequest, since: Millis, message: String) -> Self {
        Self {
            kind,
            subject: Subject::Pr(pr.id),
            since,
            label: pr_label(pr),
            message,
            detail: None,
            action: NudgeAction::Open,
            ping: Vec::new(),
            related: None,
        }
    }

    fn wi(
        kind: NudgeKind,
        wi: &WorkItem,
        since: Millis,
        message: String,
        action: NudgeAction,
    ) -> Self {
        Self {
            kind,
            subject: Subject::WorkItem(wi.id),
            since,
            label: wi_label(wi),
            message,
            detail: None,
            action,
            ping: Vec::new(),
            related: None,
        }
    }
}

/// N1: I'm a reviewer with no vote on an active, non-draft PR; timed from
/// when I was added, else from PR creation (FR-2).
fn review_waiting(
    ix: &Index,
    pr: &PullRequest,
    hours: &WorkingHours,
    now: Millis,
) -> Option<Found> {
    if pr.status != PrStatus::Active || pr.is_draft || ix.is_me(&pr.author) {
        return None;
    }
    let me = pr
        .reviewers
        .iter()
        .find(|r| ix.is_me(&r.person) && r.vote == Vote::NoVote && !r.has_declined)?;
    let since = me.added_at.unwrap_or(pr.created);
    let waited = hours.working_between(since, now);
    let mut found = Found::pr(
        NudgeKind::ReviewWaiting,
        pr,
        since,
        format!(
            "Review requested {} ago: {}",
            copy::working_span(waited, hours),
            pr_label(pr)
        ),
    );
    found.action = NudgeAction::ReviewNow;
    Some(found)
}

/// N2: someone voted wait-for-author or reject on my PR and I haven't pushed
/// or replied since that vote.
fn waiting_for_author(
    ix: &Index,
    pr: &PullRequest,
    hours: &WorkingHours,
    now: Millis,
) -> Option<Found> {
    if !mine_active(ix, pr) {
        return None;
    }
    let my_reply = ix
        .human_comments(pr.id)
        .filter(|c| ix.is_me(&c.author))
        .map(|c| c.created)
        .max();
    let responded = pr.last_push.max(my_reply);
    let mut blocking: Vec<(Millis, &adoken_model::Reviewer)> = pr
        .reviewers
        .iter()
        .filter(|r| {
            !ix.is_me(&r.person) && matches!(r.vote, Vote::WaitingForAuthor | Vote::Rejected)
        })
        .map(|r| (r.voted_at.unwrap_or(pr.updated), r))
        .filter(|(voted, _)| responded.is_none_or(|t| t < *voted))
        .collect();
    blocking.sort_by_key(|(voted, r)| (*voted, r.person.clone()));
    let &(since, first) = blocking.first()?;
    let vote = if first.vote == Vote::Rejected {
        "reject"
    } else {
        "wait-for-author"
    };
    let who = match blocking.len() {
        1 => ix.snap.name(&first.person).to_owned(),
        2 => format!(
            "{} and {}",
            ix.snap.name(&first.person),
            ix.snap.name(&blocking[1].1.person)
        ),
        n => format!("{} and {} others", ix.snap.name(&first.person), n - 1),
    };
    Some(Found::pr(
        NudgeKind::WaitingForAuthor,
        pr,
        since,
        format!(
            "!{} has a {vote} vote from {who} {}",
            pr.id,
            copy::since(since, now, hours)
        ),
    ))
}

/// N3: the latest build on my PR failed and no push since it finished.
fn build_failing(ix: &Index, pr: &PullRequest, hours: &WorkingHours, now: Millis) -> Option<Found> {
    if !mine_active(ix, pr) {
        return None;
    }
    let run = ix.latest_run.get(&pr.id)?;
    if run.status != RunStatus::Completed || run.result != Some(RunResult::Failed) {
        return None;
    }
    let failed = run.finish.unwrap_or(run.queued);
    if pr.last_push.is_some_and(|push| push > failed) {
        return None;
    }
    let mut found = Found::pr(
        NudgeKind::BuildFailing,
        pr,
        failed,
        format!(
            "!{} build failed {} and nobody has pushed",
            pr.id,
            copy::ago(failed, now, hours)
        ),
    );
    found.detail = Some(format!("{} #{}", run.pipeline_name, run.run_number));
    found.action = NudgeAction::OpenFailingRun(run.id);
    found.related = Some(Subject::Build(run.id));
    Some(found)
}

/// N4: my active, non-draft PR has no votes and no human comments from
/// others. Names the reviewers to ping, required first (FR-9).
fn no_reviewer_activity(
    ix: &Index,
    pr: &PullRequest,
    hours: &WorkingHours,
    now: Millis,
) -> Option<Found> {
    if !mine_active(ix, pr) || pr.is_draft {
        return None;
    }
    let others = || pr.reviewers.iter().filter(|r| !ix.is_me(&r.person));
    if others().any(|r| r.vote != Vote::NoVote) {
        return None;
    }
    if ix.human_comments(pr.id).any(|c| !ix.is_me(&c.author)) {
        return None;
    }
    let waiting: Vec<(String, bool)> = others()
        .filter(|r| !r.has_declined && r.is_required)
        .chain(others().filter(|r| !r.has_declined && !r.is_required))
        .map(|r| (ix.snap.name(&r.person).to_owned(), r.is_required))
        .collect();
    let since = pr.created;
    let mut found = Found::pr(
        NudgeKind::NoReviewerActivity,
        pr,
        since,
        format!(
            "!{} has no votes or comments after {}",
            pr.id,
            copy::working_span(hours.working_between(since, now), hours)
        ),
    );
    found.detail = Some(if waiting.is_empty() {
        "No reviewers added yet".to_owned()
    } else {
        format!("Waiting on {}", copy::names(&waiting))
    });
    found.ping = waiting.into_iter().map(|(name, _)| name).collect();
    Some(found)
}

/// N5: my PR has merge conflicts.
fn merge_conflicts(ix: &Index, pr: &PullRequest) -> Option<Found> {
    if !mine_active(ix, pr) || pr.merge_status != MergeStatus::Conflicts {
        return None;
    }
    let since = pr.last_push.unwrap_or(pr.created);
    Some(Found::pr(
        NudgeKind::MergeConflicts,
        pr,
        since,
        format!(
            "!{} has merge conflicts with {}",
            pr.id,
            adoken_model::short_branch(&pr.target_branch)
        ),
    ))
}

fn mine_in_progress(ix: &Index, wi: &WorkItem) -> bool {
    wi.state_category == StateCategory::InProgress
        && wi.assigned_to.as_deref().is_some_and(|who| ix.is_me(who))
}

/// N6: in-progress item whose linked PRs are all closed and at least one
/// merged; timed from the latest merge.
fn merged_still_active(
    ix: &Index,
    wi: &WorkItem,
    hours: &WorkingHours,
    now: Millis,
) -> Option<Found> {
    if !mine_in_progress(ix, wi) {
        return None;
    }
    let linked: Vec<&PullRequest> = wi
        .linked_pr_ids
        .iter()
        .filter_map(|id| ix.prs.get(id).copied())
        .collect();
    if linked.iter().any(|pr| pr.status == PrStatus::Active) {
        return None;
    }
    let (merged, pr) = linked
        .iter()
        .filter(|pr| pr.status == PrStatus::Completed)
        .filter_map(|pr| pr.closed.map(|closed| (closed, *pr)))
        .max_by_key(|(closed, pr)| (*closed, pr.id))?;
    let mut found = Found::wi(
        NudgeKind::MergedStillActive,
        wi,
        merged,
        format!(
            "#{} is {} but its PR !{} merged {}",
            wi.id,
            wi.state,
            pr.id,
            copy::ago(merged, now, hours)
        ),
        NudgeAction::OpenInAdo,
    );
    found.related = Some(Subject::Pr(pr.id));
    Some(found)
}

/// N7: in-progress item with no change and no linked PR activity.
fn quiet_active_item(
    ix: &Index,
    wi: &WorkItem,
    hours: &WorkingHours,
    now: Millis,
) -> Option<Found> {
    if !mine_in_progress(ix, wi) {
        return None;
    }
    let pr_activity = wi
        .linked_pr_ids
        .iter()
        .filter_map(|id| ix.prs.get(id))
        .map(|pr| pr.updated.max(pr.closed.unwrap_or(0)))
        .max();
    let since = wi.changed.max(pr_activity.unwrap_or(0));
    let quiet = copy::working_span(hours.working_between(since, now), hours);
    let message = if wi.linked_pr_ids.is_empty() {
        format!(
            "#{} has been {} with no linked PR or change for {quiet}",
            wi.id, wi.state
        )
    } else {
        format!(
            "#{} has been {} with no change or PR activity for {quiet}",
            wi.id, wi.state
        )
    };
    Some(Found::wi(
        NudgeKind::QuietActiveItem,
        wi,
        since,
        message,
        NudgeAction::StillOnIt,
    ))
}

/// Every rule whose condition holds right now, regardless of threshold,
/// sorted by FR-4 priority then age (oldest first).
pub fn conditions(snap: &Snapshot, hours: &WorkingHours, now: Millis) -> Vec<Nudge> {
    let ix = Index::new(snap);
    let mut found = Vec::new();
    for pr in &snap.prs {
        found.extend(review_waiting(&ix, pr, hours, now));
        found.extend(waiting_for_author(&ix, pr, hours, now));
        found.extend(build_failing(&ix, pr, hours, now));
        found.extend(no_reviewer_activity(&ix, pr, hours, now));
        found.extend(merge_conflicts(&ix, pr));
    }
    for wi in &snap.work_items {
        found.extend(merged_still_active(&ix, wi, hours, now));
        found.extend(quiet_active_item(&ix, wi, hours, now));
    }
    let mut nudges: Vec<Nudge> = found
        .into_iter()
        .map(|f| Nudge {
            waited: hours.working_between(f.since, now),
            kind: f.kind,
            subject: f.subject,
            since: f.since,
            label: f.label,
            message: f.message,
            detail: f.detail,
            action: f.action,
            ping: f.ping,
            related: f.related,
        })
        .collect();
    sort(&mut nudges);
    nudges
}

/// FR-4 order: priority, then oldest first, then subject for determinism.
pub fn sort(nudges: &mut [Nudge]) {
    nudges.sort_by_key(|n| (n.kind.rank(), n.since, subject_order(&n.subject)));
}

/// Allocation-free total order on subjects.
pub(crate) fn subject_order(subject: &Subject) -> (u8, i64) {
    match subject {
        Subject::Pr(id) => (0, *id),
        Subject::WorkItem(id) => (1, *id),
        Subject::Build(id) => (2, *id),
        Subject::Account => (3, 0),
    }
}

/// Keeps the highest-priority nudge per subject (input must be [`sort`]ed):
/// one subject never yields two nudges at once.
pub fn best_per_subject(nudges: Vec<Nudge>) -> Vec<Nudge> {
    let mut seen = HashSet::new();
    nudges
        .into_iter()
        .filter(|n| seen.insert(n.subject.clone()))
        .collect()
}
