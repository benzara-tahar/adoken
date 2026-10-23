//! Today as "my day" (spec 0017, replacing 0006 §3.1's layout; visuals in
//! `docs/design/today.md`), computed from one pass:
//!
//! - **Summary**: how the day is going: a mood, one sentence and counts
//!   ([`crate::summary`]).
//! - **Your PRs**: each active PR of mine and each merged today, most
//!   recently active first, with its stage, state, reviewers and what
//!   happened on it today.
//! - **Up next**: the single most important next action (0006 FR-10's
//!   Focus). The first nudge in FR-4 order that is due and not snoozed, done
//!   or muted; failing that, the review that has waited longest (still under
//!   its threshold); else the day is clear.
//! - **Waiting on you**: reviews I owe, blocked-longest first.
//! - **Loose ends**: my in-progress work items a nudge flags (N6, N7).
//!
//! Nudges are not a list here: they pick Up next, set each PR's blocker, and
//! attach to their subject's row (so `s`/`d`/`x` work on any row and the
//! cap-held rest stay visible, 0006 AC-3).

use std::collections::{HashMap, HashSet};

use adoken_model::{
    Millis, PersonId, PrStatus, PullRequest, Subject, ThreadStatus, Vote, WorkItemType,
};

use crate::copy;
use crate::engine::{Ledger, Pass};
use crate::kind::{NudgeKind, Settings};
use crate::rules::Nudge;
use crate::snapshot::Snapshot;
use crate::summary::{self, Attention, Commits, Phase, Summary};

/// How warm an age reads: cool under its threshold, warm up to
/// [`Age::HOT_AFTER`] thresholds, hot after.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Heat {
    Cool,
    Warm,
    Hot,
}

/// How long something has waited, measured against its nudge threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Age {
    /// When the wait began (wall clock).
    pub since: Millis,
    /// Working time waited (FR-5).
    pub waited: Millis,
    /// The nudge threshold it is measured against.
    pub threshold: Millis,
}

impl Age {
    /// Thresholds after which an age is hot.
    pub const HOT_AFTER: Millis = 3;

    pub fn heat(&self) -> Heat {
        if self.threshold <= 0 || self.waited >= Self::HOT_AFTER * self.threshold {
            Heat::Hot
        } else if self.waited >= self.threshold {
            Heat::Warm
        } else {
            Heat::Cool
        }
    }
}

/// Why a row is parked: set from the ledger, shown as a glyph, excluded from
/// Up next and the mood.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quiet {
    /// Snoozed until the next workday.
    Snoozed,
    /// Marked done until the condition changes.
    Done,
    /// "Not for this item".
    Muted,
}

fn quiet(
    ledger: &Ledger,
    subject: &Subject,
    kind: Option<NudgeKind>,
    now: Millis,
) -> Option<Quiet> {
    let state = ledger.subject(subject)?;
    if state.opted_out {
        Some(Quiet::Muted)
    } else if state.snoozed_until.is_some_and(|until| until > now) {
        Some(Quiet::Snoozed)
    } else if kind.is_some_and(|k| state.dismissed.contains(&k)) {
        Some(Quiet::Done)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub nudge: Nudge,
    /// Past its threshold. `false` for the calm fallback: the oldest review
    /// still under it.
    pub due: bool,
    /// Whose face goes on the panel: the author blocked on me (N1), the
    /// reviewer who asked for changes (N2). `None`: a glyph instead.
    pub who: Option<PersonId>,
    pub who_name: Option<String>,
    /// `Mara is blocked on you`.
    pub headline: String,
    /// `!4821 Fix login throttle`.
    pub label: String,
    pub files: Option<u32>,
    pub age: Age,
    /// Today's activity on the subject PR, e.g. `2 new comments`.
    pub activity: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FocusState {
    /// Hot and warm tiers haven't completed once: nothing is claimed yet.
    Syncing,
    /// Nothing waits on me.
    Clear,
    Item(Box<Focus>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitCard {
    pub pr_id: i64,
    pub title: String,
    pub author: PersonId,
    pub author_name: String,
    /// Since I was asked (FR-2), against the N1 threshold.
    pub age: Age,
    pub files: Option<u32>,
    /// Active discussion threads with a human comment.
    pub threads: u32,
    /// I'm a required reviewer.
    pub required: bool,
    pub quiet: Option<Quiet>,
    /// The N1 condition: triage target.
    pub nudge: Nudge,
}

/// A PR's position: Draft → Review → Approved → Merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Draft,
    Review,
    Approved,
    Merged,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Draft, Stage::Review, Stage::Approved, Stage::Merged];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Draft => "Draft",
            Stage::Review => "Review",
            Stage::Approved => "Approved",
            Stage::Merged => "Merged",
        }
    }

    /// 1-based position on the four-segment bar.
    pub fn number(self) -> u8 {
        self as u8 + 1
    }

    /// Where `pr` stands: merged when completed, draft while draft, approved
    /// once someone approved, every required reviewer approved and nobody
    /// is waiting on the author or rejecting; otherwise in review. Declined
    /// reviewers and I don't count; groups do.
    pub fn of(pr: &PullRequest, me: &str) -> Stage {
        if pr.status == PrStatus::Completed {
            return Stage::Merged;
        }
        if pr.is_draft {
            return Stage::Draft;
        }
        let others = || {
            pr.reviewers
                .iter()
                .filter(|r| r.person != me && !r.has_declined)
        };
        let approves = |v: Vote| matches!(v, Vote::Approved | Vote::ApprovedWithSuggestions);
        let approved = others().any(|r| approves(r.vote))
            && others().filter(|r| r.is_required).all(|r| approves(r.vote))
            && !others().any(|r| matches!(r.vote, Vote::WaitingForAuthor | Vote::Rejected));
        if approved {
            Stage::Approved
        } else {
            Stage::Review
        }
    }
}

/// The one thing holding a PR of mine, highest priority first (FR-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocker {
    /// N5.
    Conflict,
    /// N3.
    BuildFailed,
    /// N2: wait-for-author or reject vote, unanswered.
    ChangesRequested,
    /// N4 past its threshold: no votes or comments from anyone.
    WaitingOnReviewers,
}

impl Blocker {
    fn of(kind: NudgeKind) -> Option<Self> {
        match kind {
            NudgeKind::MergeConflicts => Some(Blocker::Conflict),
            NudgeKind::BuildFailing => Some(Blocker::BuildFailed),
            NudgeKind::WaitingForAuthor => Some(Blocker::ChangesRequested),
            NudgeKind::NoReviewerActivity => Some(Blocker::WaitingOnReviewers),
            _ => None,
        }
    }

    /// Spoken and tooltip form.
    pub fn label(self) -> &'static str {
        match self {
            Blocker::Conflict => "Merge conflict",
            Blocker::BuildFailed => "Build failed",
            Blocker::ChangesRequested => "Changes requested",
            Blocker::WaitingOnReviewers => "Waiting on reviewers",
        }
    }
}

/// A PR row's state on its stage bar (spec 0017 FR-16), first that applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrState {
    MergedToday,
    Conflict,
    BuildFailed,
    ChangesRequested,
    /// N4 past its threshold.
    NoReviews,
    Approved,
    InReview,
    Draft,
}

impl PrState {
    pub fn of(stage: Stage, blocker: Option<Blocker>) -> Self {
        match (stage, blocker) {
            (Stage::Merged, _) => PrState::MergedToday,
            (_, Some(Blocker::Conflict)) => PrState::Conflict,
            (_, Some(Blocker::BuildFailed)) => PrState::BuildFailed,
            (_, Some(Blocker::ChangesRequested)) => PrState::ChangesRequested,
            (_, Some(Blocker::WaitingOnReviewers)) => PrState::NoReviews,
            (Stage::Approved, None) => PrState::Approved,
            (Stage::Review, None) => PrState::InReview,
            (Stage::Draft, None) => PrState::Draft,
        }
    }

    /// Filled segments of four: a blocker fills up to the PR's stage.
    pub fn filled(self, stage: Stage) -> u8 {
        match self {
            PrState::MergedToday => 4,
            PrState::Conflict | PrState::BuildFailed => stage.number(),
            PrState::ChangesRequested | PrState::NoReviews | PrState::InReview => 2,
            PrState::Approved => 3,
            PrState::Draft => 1,
        }
    }

    /// The label beside the bar (`No reviews` takes its age after it).
    pub fn label(self) -> &'static str {
        match self {
            PrState::MergedToday => "Merged today",
            PrState::Conflict => "Merge conflict",
            PrState::BuildFailed => "Build failed",
            PrState::ChangesRequested => "Changes requested",
            PrState::NoReviews => "No reviews",
            PrState::Approved => "Approved",
            PrState::InReview => "In review",
            PrState::Draft => "Draft",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackReviewer {
    pub person: PersonId,
    pub name: String,
    pub vote: Vote,
    pub required: bool,
    /// `Person.is_group`; an identity not synced yet counts as a person.
    pub is_group: bool,
}

/// What happened on a PR of mine today (spec 0017 FR-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrActivity {
    pub commits: Commits,
    /// Others' comments today.
    pub comments: u32,
    pub opened: bool,
    pub merged_at: Option<Millis>,
    pub last: Millis,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub pr_id: i64,
    pub title: String,
    pub stage: Stage,
    pub blocker: Option<Blocker>,
    /// How long the blocker has held.
    pub blocked: Option<Age>,
    pub state: PrState,
    /// `In review`, `No reviews · 2d`.
    pub state_label: String,
    /// Reviewers other than me, not declined; required first.
    pub reviewers: Vec<TrackReviewer>,
    /// The PR's top condition (FR-4): triage target, N4's names to copy.
    pub nudge: Option<Nudge>,
    pub quiet: Option<Quiet>,
    pub activity: PrActivity,
    /// FR-19: what follows `!id` on line 3.
    pub line: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LooseEnd {
    pub id: i64,
    pub title: String,
    pub kind: WorkItemType,
    pub state: String,
    /// N6 names the merged PR.
    pub merged_pr: Option<i64>,
    pub age: Age,
    /// When the item entered its current state ("Open 3d"; owner decision
    /// on spec 0017 Q-2), else its last change.
    pub open_since: Millis,
    pub quiet: Option<Quiet>,
    pub nudge: Nudge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub summary: Summary,
    pub focus: FocusState,
    pub waiting: Vec<WaitCard>,
    /// Your PRs, most recently active first.
    pub tracks: Vec<Track>,
    pub loose: Vec<LooseEnd>,
}

impl Day {
    /// No rows at all: a brand-new account or an empty org.
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty() && self.tracks.is_empty() && self.loose.is_empty()
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// Active discussion threads with at least one live human comment.
fn discussion_threads(snap: &Snapshot, pr: i64) -> u32 {
    snap.threads.get(&pr).map_or(0, |threads| {
        threads
            .iter()
            .filter(|t| t.status == ThreadStatus::Active)
            .filter(|t| t.comments.iter().any(|c| !c.is_system && !c.is_deleted))
            .count() as u32
    })
}

fn age(settings: &Settings, nudge: &Nudge) -> Age {
    Age {
        since: nudge.since,
        waited: nudge.waited,
        threshold: settings.threshold_ms(nudge.kind),
    }
}

/// The reviewer whose wait-for-author or reject vote is oldest (N2's who).
fn changes_requested_by<'a>(pr: &'a PullRequest, me: &str) -> Option<&'a PersonId> {
    pr.reviewers
        .iter()
        .filter(|r| r.person != me && matches!(r.vote, Vote::WaitingForAuthor | Vote::Rejected))
        .min_by_key(|r| (r.voted_at.unwrap_or(pr.updated), r.person.clone()))
        .map(|r| &r.person)
}

/// `2 commits today · 1 new comment`: today's parts, in FR-19 order.
fn activity_parts(activity: &PrActivity) -> Vec<String> {
    let mut parts = Vec::new();
    if activity.opened {
        parts.push("Opened today".to_owned());
    }
    parts.extend(activity.commits.text());
    if activity.comments > 0 {
        parts.push(copy::count(
            activity.comments as usize,
            "new comment",
            "new comments",
        ));
    }
    parts
}

fn focus_of(
    snap: &Snapshot,
    settings: &Settings,
    prs: &HashMap<i64, &PullRequest>,
    activity: &HashMap<i64, PrActivity>,
    nudge: &Nudge,
    due: bool,
) -> Focus {
    let pr = match nudge.subject {
        Subject::Pr(id) => prs.get(&id).copied(),
        _ => None,
    };
    let who = match (nudge.kind, pr) {
        (NudgeKind::ReviewWaiting, Some(pr)) => Some(pr.author.clone()),
        (NudgeKind::WaitingForAuthor, Some(pr)) => changes_requested_by(pr, &snap.me).cloned(),
        _ => None,
    };
    let who_name = who.as_deref().map(|id| snap.name(id).to_owned());
    let first = who_name.as_deref().map(first_name).unwrap_or("Someone");
    let headline = match nudge.kind {
        NudgeKind::ReviewWaiting => format!("{first} is blocked on you"),
        NudgeKind::WaitingForAuthor => format!("{first} asked for changes"),
        NudgeKind::BuildFailing => "Your build failed".to_owned(),
        NudgeKind::NoReviewerActivity => "No one has reviewed yet".to_owned(),
        NudgeKind::MergeConflicts => "Your PR has a conflict".to_owned(),
        NudgeKind::MergedStillActive => "Merged, still open".to_owned(),
        NudgeKind::QuietActiveItem => "Still on it?".to_owned(),
    };
    let today = pr
        .and_then(|pr| activity.get(&pr.id))
        .map(activity_parts)
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join(" · "));
    Focus {
        nudge: nudge.clone(),
        due,
        who,
        who_name,
        headline,
        label: nudge.label.clone(),
        files: pr.and_then(|pr| snap.file_counts.get(&pr.id).copied()),
        age: age(settings, nudge),
        activity: today,
    }
}

/// FR-19: `Merged today at 14:32`, today's parts, else the blocker with how
/// long it has held, else the approval, else nothing today.
#[allow(clippy::too_many_arguments)]
fn activity_line(
    snap: &Snapshot,
    settings: &Settings,
    pr: &PullRequest,
    stage: Stage,
    blocker: Option<Blocker>,
    nudge: Option<&Nudge>,
    activity: &PrActivity,
    now: Millis,
) -> String {
    if let Some(at) = activity.merged_at {
        return format!("Merged today at {}", copy::clock(at, &settings.hours));
    }
    let parts = activity_parts(activity);
    if !parts.is_empty() {
        return parts.join(" · ");
    }
    let held = || nudge.map_or_else(String::new, |n| copy::compact(now - n.since));
    match blocker {
        Some(Blocker::Conflict) => format!("Conflict for {}", held()),
        Some(Blocker::BuildFailed) => format!("Build failed {} ago", held()),
        Some(Blocker::ChangesRequested) => {
            let who = changes_requested_by(pr, &snap.me)
                .map(|id| first_name(snap.name(id)).to_owned())
                .unwrap_or_else(|| "a reviewer".to_owned());
            format!("Changes requested by {who} · {}", held())
        }
        Some(Blocker::WaitingOnReviewers) => {
            let detail = nudge
                .and_then(|n| n.detail.clone())
                .unwrap_or_else(|| "Waiting on reviewers".to_owned());
            format!("{detail} · {}", held())
        }
        None if stage == Stage::Approved => "All reviewers approved".to_owned(),
        None => "No activity today".to_owned(),
    }
}

/// Builds Today at `now` from one pass. `ledger` is the one the pass left.
pub fn today(
    snap: &Snapshot,
    settings: &Settings,
    pass: &Pass,
    ledger: &Ledger,
    now: Millis,
) -> Day {
    let prs: HashMap<i64, &PullRequest> = snap.prs.iter().map(|pr| (pr.id, pr)).collect();
    let on = |kind| settings.enabled(kind);
    let day_start = settings.hours.day_start(now);

    // Waiting on you: every N1 condition, blocked longest first; parked last.
    let mut waiting: Vec<WaitCard> = pass
        .conditions
        .iter()
        .filter(|n| n.kind == NudgeKind::ReviewWaiting)
        .filter_map(|n| {
            let Subject::Pr(id) = n.subject else {
                return None;
            };
            let pr = prs.get(&id)?;
            Some(WaitCard {
                pr_id: id,
                title: pr.title.clone(),
                author: pr.author.clone(),
                author_name: snap.name(&pr.author).to_owned(),
                age: age(settings, n),
                files: snap.file_counts.get(&id).copied(),
                threads: discussion_threads(snap, id),
                required: pr
                    .reviewers
                    .iter()
                    .any(|r| r.person == snap.me && r.is_required),
                quiet: quiet(ledger, &n.subject, Some(n.kind), now),
                nudge: n.clone(),
            })
        })
        .collect();
    waiting.sort_by_key(|c| (c.quiet.is_some(), c.age.since, c.pr_id));

    // Your PRs: my active PRs and today's merges. Each PR's top condition
    // (conditions are in FR-4 order; N4 counts once due).
    let mut top: HashMap<i64, &Nudge> = HashMap::new();
    for n in &pass.conditions {
        if let Subject::Pr(id) = n.subject
            && on(n.kind)
            && (n.kind != NudgeKind::NoReviewerActivity
                || n.waited >= settings.threshold_ms(n.kind))
        {
            top.entry(id).or_insert(n);
        }
    }
    let mine: Vec<&PullRequest> = snap
        .prs
        .iter()
        .filter(|pr| pr.status == PrStatus::Active && pr.author == snap.me)
        .chain(
            snap.merged_today
                .iter()
                .filter(|pr| pr.author == snap.me && pr.closed.is_some_and(|t| t >= day_start)),
        )
        .collect();
    let activity: HashMap<i64, PrActivity> = snap
        .prs
        .iter()
        .chain(&snap.merged_today)
        .map(|pr| {
            let merged_at = pr
                .closed
                .filter(|t| pr.status == PrStatus::Completed && *t >= day_start && *t <= now);
            (
                pr.id,
                PrActivity {
                    commits: if pr.author == snap.me {
                        summary::commits_today(snap, pr, day_start, now)
                    } else {
                        Commits::None
                    },
                    comments: summary::comments_today(snap, pr.id, day_start, now),
                    opened: pr.author == snap.me && pr.created >= day_start && pr.created <= now,
                    merged_at,
                    last: summary::last_activity(snap, pr),
                },
            )
        })
        .collect();
    let mut seen_tracks = HashSet::new();
    let mut tracks: Vec<Track> = mine
        .into_iter()
        .filter(|pr| seen_tracks.insert(pr.id))
        .map(|pr| {
            let nudge = if pr.status == PrStatus::Active {
                top.get(&pr.id).copied()
            } else {
                None
            };
            let blocker = nudge.and_then(|n| Blocker::of(n.kind));
            let mut reviewers: Vec<TrackReviewer> = pr
                .reviewers
                .iter()
                .filter(|r| r.person != snap.me && !r.has_declined)
                .map(|r| TrackReviewer {
                    person: r.person.clone(),
                    name: snap.name(&r.person).to_owned(),
                    vote: r.vote,
                    required: r.is_required,
                    is_group: snap.is_group(&r.person),
                })
                .collect();
            reviewers.sort_by_key(|r| !r.required);
            let stage = Stage::of(pr, &snap.me);
            let state = PrState::of(stage, blocker);
            let state_label = match (state, nudge) {
                (PrState::NoReviews, Some(n)) => {
                    format!("{} · {}", state.label(), copy::compact(now - n.since))
                }
                _ => state.label().to_owned(),
            };
            let pr_activity = activity[&pr.id];
            Track {
                pr_id: pr.id,
                title: pr.title.clone(),
                stage,
                blocker,
                blocked: blocker.and(nudge).map(|n| age(settings, n)),
                state,
                state_label,
                reviewers,
                quiet: nudge.and_then(|n| quiet(ledger, &n.subject, Some(n.kind), now)),
                line: activity_line(snap, settings, pr, stage, blocker, nudge, &pr_activity, now),
                nudge: nudge.cloned(),
                activity: pr_activity,
            }
        })
        .collect();
    // FR-14: most recently active first; blockers show on the row, not by
    // position.
    tracks.sort_by(|a, b| {
        b.activity
            .last
            .cmp(&a.activity.last)
            .then(a.pr_id.cmp(&b.pr_id))
    });

    // Loose ends: work items with N6 (any age) or a due N7, FR-4 order.
    let items: HashMap<i64, &adoken_model::WorkItem> =
        snap.work_items.iter().map(|wi| (wi.id, wi)).collect();
    let mut seen = HashSet::new();
    let loose: Vec<LooseEnd> = pass
        .conditions
        .iter()
        .filter(|n| on(n.kind))
        .filter(|n| match n.kind {
            NudgeKind::MergedStillActive => true,
            NudgeKind::QuietActiveItem => n.waited >= settings.threshold_ms(n.kind),
            _ => false,
        })
        .filter_map(|n| {
            let Subject::WorkItem(id) = n.subject else {
                return None;
            };
            let wi = items.get(&id)?;
            seen.insert(id).then(|| LooseEnd {
                id,
                title: wi.title.clone(),
                kind: wi.kind.clone(),
                state: wi.state.clone(),
                merged_pr: match n.related {
                    Some(Subject::Pr(pr)) => Some(pr),
                    _ => None,
                },
                age: age(settings, n),
                open_since: snap.state_since.get(&id).copied().unwrap_or(wi.changed),
                quiet: quiet(ledger, &n.subject, Some(n.kind), now),
                nudge: n.clone(),
            })
        })
        .collect();

    // Up next: the first due nudge (already FR-4 ordered and unsuppressed),
    // else the oldest review still under its threshold.
    let focus = if !snap.health.initial_sync_done {
        FocusState::Syncing
    } else if let Some(n) = pass.active.first() {
        FocusState::Item(Box::new(focus_of(snap, settings, &prs, &activity, n, true)))
    } else if let Some(card) = waiting
        .iter()
        .find(|c| c.quiet.is_none() && on(NudgeKind::ReviewWaiting))
    {
        FocusState::Item(Box::new(focus_of(
            snap,
            settings,
            &prs,
            &activity,
            &card.nudge,
            false,
        )))
    } else {
        FocusState::Clear
    };

    // The summary (FR-3 attention inputs, FR-8 mood, FR-11, FR-12).
    let unparked = |t: &&Track| t.quiet.is_none() && t.stage != Stage::Merged;
    let blocked_by = |b: Blocker| {
        tracks
            .iter()
            .filter(unparked)
            .filter(|t| t.blocker == Some(b))
            .count() as u32
    };
    let reviews_waiting = if on(NudgeKind::ReviewWaiting) {
        waiting.iter().filter(|c| c.quiet.is_none()).count() as u32
    } else {
        0
    };
    let attention = Attention {
        conflicts: blocked_by(Blocker::Conflict),
        failed_builds: blocked_by(Blocker::BuildFailed),
        reviews_waiting,
        owed: reviews_waiting
            + loose.iter().filter(|l| l.quiet.is_none()).count() as u32
            + blocked_by(Blocker::ChangesRequested),
        waiting_on_reviewers: tracks
            .iter()
            .filter(|t| {
                t.stage == Stage::Review
                    && prs.get(&t.pr_id).is_some_and(|pr| !pr.is_draft)
                    && matches!(t.blocker, None | Some(Blocker::WaitingOnReviewers))
            })
            .count() as u32,
    };
    let counts = summary::counts(snap, day_start, now);
    let health = &snap.health;
    let mood = summary::mood(&counts, &attention, health, Phase::at(&settings.hours, now));
    let summary = Summary {
        mood,
        sentence: summary::sentence(mood, &counts, &attention, health, &settings.hours),
        slots: summary::slots(&counts, health),
        counts,
        attention,
        next_change: summary::next_change(&settings.hours, now),
    };

    Day {
        summary,
        focus,
        waiting,
        tracks,
        loose: if health.work_scope { loose } else { Vec::new() },
    }
}
