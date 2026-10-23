//! Today's day summary (spec 0017 §4.1–4.2): what happened since local
//! midnight, counted from the snapshot (FR-3, FR-4), and the mood, sentence
//! and counts row it reads as (FR-7–FR-12). Pure functions of the snapshot,
//! the settings, the pass, the ledger, data health and `now`; computed in the
//! background `Day` pass, never while rendering.

use std::collections::{HashMap, HashSet};

use adoken_model::time::{DAY, HOUR};
use adoken_model::{Millis, PrIteration, PrStatus, PullRequest, Vote};

use crate::copy;
use crate::hours::WorkingHours;
use crate::snapshot::{DataHealth, Snapshot, StaleReason};

/// FR-7: one illustration and one headline each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    FreshStart,
    FindingFlow,
    HelpingGrow,
    Waiting,
    Blocked,
    Shipped,
    Clear,
    Stalled,
}

impl Mood {
    pub const ALL: [Mood; 8] = [
        Mood::FreshStart,
        Mood::FindingFlow,
        Mood::HelpingGrow,
        Mood::Waiting,
        Mood::Blocked,
        Mood::Shipped,
        Mood::Clear,
        Mood::Stalled,
    ];

    /// Spec id, e.g. `fresh-start`.
    pub fn id(self) -> &'static str {
        match self {
            Mood::FreshStart => "fresh-start",
            Mood::FindingFlow => "finding-flow",
            Mood::HelpingGrow => "helping-grow",
            Mood::Waiting => "waiting",
            Mood::Blocked => "blocked",
            Mood::Shipped => "shipped",
            Mood::Clear => "clear",
            Mood::Stalled => "stalled",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            Mood::FreshStart => "A fresh start",
            Mood::FindingFlow => "Finding your flow",
            Mood::HelpingGrow => "Helping things grow",
            Mood::Waiting => "Waiting to bloom",
            Mood::Blocked => "Blocked",
            Mood::Shipped => "Something shipped",
            Mood::Clear => "Clear for now",
            Mood::Stalled => "Nothing moved today",
        }
    }
}

/// What the card shows: a mood, or one of the two variants that reuse an
/// illustration without claiming its mood (FR-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DayMood {
    Mood(Mood),
    /// First sync incomplete: fresh-start art, no claim.
    FirstSync,
    /// Data stale: waiting art, absence not claimed.
    Stale,
}

impl DayMood {
    /// Which illustration paints (C2: one per [`Mood`]).
    pub fn art(self) -> Mood {
        match self {
            DayMood::Mood(mood) => mood,
            DayMood::FirstSync => Mood::FreshStart,
            DayMood::Stale => Mood::Waiting,
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            DayMood::Mood(mood) => mood.headline(),
            DayMood::FirstSync => "Getting your day ready",
            DayMood::Stale => "Can't see the latest",
        }
    }

    /// Spec id: the mood's, `first-sync` or `stale`.
    pub fn id(self) -> &'static str {
        match self {
            DayMood::Mood(mood) => mood.id(),
            DayMood::FirstSync => "first-sync",
            DayMood::Stale => "stale",
        }
    }
}

/// FR-3 metrics since local midnight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub merged: u32,
    pub reviews: u32,
    pub discussions: u32,
    pub commits: u32,
    /// `commits` is a lower bound (FR-4: truncated, unknown or not synced).
    pub commits_lower_bound: bool,
    pub done: u32,
    pub opened: u32,
}

impl Counts {
    /// FR-8 `A`.
    pub fn activity(&self) -> u32 {
        self.merged + self.reviews + self.discussions + self.commits + self.done + self.opened
    }

    pub fn growth(&self) -> u32 {
        self.reviews + self.discussions
    }

    pub fn flow(&self) -> u32 {
        self.commits + self.opened + self.done
    }
}

/// FR-3 attention inputs, from the `Day`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attention {
    /// My active PRs with an unparked N5 blocker.
    pub conflicts: u32,
    /// … with an unparked N3 blocker.
    pub failed_builds: u32,
    /// Unparked Waiting on you rows.
    pub reviews_waiting: u32,
    /// Waiting on you + loose ends + changes requested, unparked.
    pub owed: u32,
    /// My active non-draft PRs in review, unblocked or waiting on reviewers.
    pub waiting_on_reviewers: u32,
}

impl Attention {
    /// FR-8 `B`.
    pub fn blocked(&self) -> u32 {
        self.conflicts + self.failed_builds
    }
}

/// FR-9 time phases: working time since today's working hours began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phase {
    pub workday: bool,
    /// The first 2 working hours, or any time before them.
    pub fresh_window: bool,
    /// Half the working day has passed.
    pub past_half: bool,
}

/// FR-9: the fresh window's length in working time.
pub const FRESH_WINDOW: Millis = 2 * HOUR;

impl Phase {
    pub fn at(hours: &WorkingHours, now: Millis) -> Self {
        let (start, _) = hours.work_window(now);
        let elapsed = hours.working_between(start, now);
        let day = hours.day_length();
        Phase {
            workday: hours.is_workday(now),
            fresh_window: elapsed < FRESH_WINDOW,
            past_half: day > 0 && 2 * elapsed >= day,
        }
    }
}

/// FR-2: the next instant the summary changes with time alone: the next
/// local midnight, the end of the fresh window or the half-day mark,
/// whichever comes first after `now`.
pub fn next_change(hours: &WorkingHours, now: Millis) -> Millis {
    let midnight = hours.day_start(now) + DAY;
    if !hours.is_workday(now) || hours.day_length() == 0 {
        return midnight;
    }
    let (start, end) = hours.work_window(now);
    [
        (start + FRESH_WINDOW).min(end),
        start + hours.day_length() / 2,
        midnight,
    ]
    .into_iter()
    .filter(|t| *t > now)
    .min()
    .unwrap_or(midnight)
}

/// FR-8, first match wins.
pub fn mood(counts: &Counts, attention: &Attention, health: &DataHealth, phase: Phase) -> DayMood {
    let fresh = health.stale.is_none();
    let would_stall = phase.workday && phase.past_half && attention.owed >= 1;
    let would_clear = attention.owed == 0;
    if !health.initial_sync_done {
        DayMood::FirstSync
    } else if counts.merged >= 1 {
        DayMood::Mood(Mood::Shipped)
    } else if counts.activity() >= 1 && counts.growth() > counts.flow() {
        DayMood::Mood(Mood::HelpingGrow)
    } else if counts.activity() >= 1 {
        DayMood::Mood(Mood::FindingFlow)
    } else if attention.blocked() >= 1 {
        DayMood::Mood(Mood::Blocked)
    } else if phase.workday && phase.fresh_window {
        DayMood::Mood(Mood::FreshStart)
    } else if would_stall && fresh {
        DayMood::Mood(Mood::Stalled)
    } else if attention.waiting_on_reviewers >= 1 {
        DayMood::Mood(Mood::Waiting)
    } else if would_clear && fresh {
        DayMood::Mood(Mood::Clear)
    } else if !fresh && (would_stall || would_clear) {
        DayMood::Stale
    } else {
        DayMood::Mood(Mood::FreshStart)
    }
}

/// FR-11: one line, at most three parts joined by " · ".
pub fn sentence(
    mood: DayMood,
    counts: &Counts,
    attention: &Attention,
    health: &DataHealth,
    hours: &WorkingHours,
) -> String {
    let mood = match mood {
        DayMood::FirstSync => {
            return "First sync in progress · counts appear when it finishes".to_owned();
        }
        DayMood::Stale => {
            let stale = health.stale;
            let what = match stale.map(|s| s.reason) {
                Some(StaleReason::Offline) => "Offline",
                _ => "Sync delayed",
            };
            return match stale.and_then(|s| s.since) {
                Some(at) => format!("{what} · counts are from {}", copy::clock(at, hours)),
                None => what.to_owned(),
            };
        }
        DayMood::Mood(Mood::Clear) => return "Nothing needs your attention right now".to_owned(),
        DayMood::Mood(mood) => mood,
    };
    let n = |v: u32| v as usize;
    let lead = match mood {
        Mood::Shipped => Some(format!(
            "{} merged today",
            copy::count(n(counts.merged), "PR", "PRs")
        )),
        Mood::HelpingGrow if counts.reviews >= 1 => Some(format!(
            "{} done today",
            copy::count(n(counts.reviews), "review", "reviews")
        )),
        Mood::HelpingGrow => Some(format!(
            "{} today",
            copy::count(n(counts.discussions), "discussion", "discussions")
        )),
        Mood::FindingFlow if counts.commits >= 1 => Some(format!(
            "{} pushed today",
            copy::count_plus(
                counts.commits,
                counts.commits_lower_bound,
                "commit",
                "commits"
            )
        )),
        Mood::FindingFlow if counts.opened >= 1 => Some(format!(
            "{} opened today",
            copy::count(n(counts.opened), "PR", "PRs")
        )),
        Mood::FindingFlow => Some(format!(
            "{} done today",
            copy::count(n(counts.done), "work item", "work items")
        )),
        Mood::Waiting => Some(format!(
            "{} waiting on reviewers",
            copy::count(n(attention.waiting_on_reviewers), "PR", "PRs")
        )),
        Mood::Stalled => Some("No reviews, commits or merges yet today".to_owned()),
        Mood::Blocked | Mood::FreshStart | Mood::Clear => None,
    };
    let mut parts: Vec<String> = lead.into_iter().collect();
    let attention_parts = [
        (attention.conflicts > 0).then(|| {
            format!(
                "{} to resolve",
                copy::count(n(attention.conflicts), "conflict", "conflicts")
            )
        }),
        (attention.failed_builds > 0).then(|| {
            format!(
                "{} to fix",
                copy::count(n(attention.failed_builds), "failed build", "failed builds")
            )
        }),
        (attention.reviews_waiting > 0).then(|| match attention.reviews_waiting {
            1 => "1 review waits on you".to_owned(),
            k => format!("{k} reviews wait on you"),
        }),
    ];
    for part in attention_parts.into_iter().flatten() {
        if parts.len() >= 3 {
            break;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return "Nothing needs you yet".to_owned();
    }
    parts.join(" · ")
}

/// A slot of the counts row (FR-12), in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Merged,
    Reviews,
    Discussions,
    Commits,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountSlot {
    pub slot: Slot,
    /// `None`: not counted yet (first sync, "–").
    pub value: Option<u32>,
    pub lower_bound: bool,
}

impl CountSlot {
    /// `5`, `3+`, `–`.
    pub fn number(&self) -> String {
        match self.value {
            None => "–".to_owned(),
            Some(n) if self.lower_bound => format!("{n}+"),
            Some(n) => n.to_string(),
        }
    }

    /// Singular at exactly 1 ("1 review").
    pub fn label(&self) -> &'static str {
        let one = self.value == Some(1) && !self.lower_bound;
        match (self.slot, one) {
            (Slot::Merged, _) => "merged",
            (Slot::Reviews, true) => "review",
            (Slot::Reviews, false) => "reviews",
            (Slot::Discussions, true) => "discussion",
            (Slot::Discussions, false) => "discussions",
            (Slot::Commits, true) => "commit",
            (Slot::Commits, false) => "commits",
            (Slot::Done, _) => "done",
        }
    }

    /// Zero or not counted: drawn in the quiet tone.
    pub fn quiet(&self) -> bool {
        self.value.is_none_or(|n| n == 0)
    }

    /// The design's per-slot tooltip (§3.3).
    pub fn tooltip(&self) -> &'static str {
        match self.slot {
            Slot::Merged => "Your pull requests merged today",
            Slot::Reviews => "Pull requests you voted on today",
            Slot::Discussions => "Discussions you joined or that were resolved today",
            Slot::Commits => "Commits pushed to your pull requests today",
            Slot::Done => "Your work items completed today",
        }
    }
}

/// FR-12: every non-zero metric, topped up to three with zero merged, then
/// reviews, then commits; fixed order; done hidden without `vso.work`.
pub fn slots(counts: &Counts, health: &DataHealth) -> Vec<CountSlot> {
    if !health.initial_sync_done {
        return [Slot::Merged, Slot::Reviews, Slot::Commits]
            .into_iter()
            .map(|slot| CountSlot {
                slot,
                value: None,
                lower_bound: false,
            })
            .collect();
    }
    let all = [
        (Slot::Merged, counts.merged, false),
        (Slot::Reviews, counts.reviews, false),
        (Slot::Discussions, counts.discussions, false),
        (Slot::Commits, counts.commits, counts.commits_lower_bound),
        (Slot::Done, counts.done, false),
    ];
    let mut shown: Vec<CountSlot> = all
        .into_iter()
        .filter(|(slot, n, _)| *n > 0 && (*slot != Slot::Done || health.work_scope))
        .map(|(slot, n, lower_bound)| CountSlot {
            slot,
            value: Some(n),
            lower_bound,
        })
        .collect();
    for slot in [Slot::Merged, Slot::Reviews, Slot::Commits] {
        if shown.len() >= 3 {
            break;
        }
        if !shown.iter().any(|s| s.slot == slot) {
            shown.push(CountSlot {
                slot,
                value: Some(0),
                lower_bound: false,
            });
        }
    }
    shown.sort_by_key(|s| s.slot);
    shown
}

/// The card, ready to lay out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub mood: DayMood,
    pub sentence: String,
    pub slots: Vec<CountSlot>,
    pub counts: Counts,
    pub attention: Attention,
    /// FR-2: recompute at this instant even if nothing syncs.
    pub next_change: Millis,
}

impl Summary {
    pub fn headline(&self) -> &'static str {
        self.mood.headline()
    }

    /// FR-28: the "Day summary" region's label.
    pub fn spoken(&self) -> String {
        let counts = self
            .slots
            .iter()
            .map(|s| format!("{} {}", s.number(), s.label()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}. {}. {counts}.",
            self.headline(),
            self.sentence.replace(" · ", ", ")
        )
    }
}

// ── counting (FR-3, FR-4, FR-5) ─────────────────────────────────────────

/// Today's pushes to one PR of mine, as its activity line says them (FR-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commits {
    None,
    /// `2 commits today`; `3+` when a count is unknown or truncated.
    Known {
        n: u32,
        lower_bound: bool,
    },
    /// Every push today predates commit counts: `1 push today`.
    Pushes(u32),
    /// The head moved today but the iteration list hasn't synced:
    /// `New commits today`. `known` counts pushes already listed.
    New {
        known: u32,
    },
}

impl Commits {
    /// Contribution to the summary's commits metric.
    pub fn count(self) -> (u32, bool) {
        match self {
            Commits::None => (0, false),
            Commits::Known { n, lower_bound } => (n, lower_bound),
            Commits::Pushes(n) => (n, true),
            Commits::New { known } => (known.max(1), true),
        }
    }

    /// The activity line's part, if any.
    pub fn text(self) -> Option<String> {
        match self {
            Commits::None => None,
            Commits::Known { n, lower_bound } => Some(format!(
                "{} today",
                copy::count_plus(n, lower_bound, "commit", "commits")
            )),
            Commits::Pushes(n) => Some(format!(
                "{} today",
                copy::count(n as usize, "push", "pushes")
            )),
            Commits::New { .. } => Some("New commits today".to_owned()),
        }
    }
}

fn in_day(t: Millis, day_start: Millis, now: Millis) -> bool {
    t >= day_start && t <= now
}

/// FR-4 for one PR of mine: pushes in `[day_start, now]` (iteration 1 only
/// when the PR was created today), counted in commits.
pub fn commits_today(snap: &Snapshot, pr: &PullRequest, day_start: Millis, now: Millis) -> Commits {
    let pushes: Vec<&PrIteration> = snap
        .iterations_today
        .get(&pr.id)
        .map(|its| {
            its.iter()
                .filter(|it| in_day(it.created, day_start, now))
                .filter(|it| it.id > 1 || in_day(pr.created, day_start, now))
                .collect()
        })
        .unwrap_or_default();
    let known: u32 = pushes
        .iter()
        .map(|it| match it.commit_count {
            Some(n) if it.commits_truncated => n.max(1),
            Some(n) => n,
            None => 1,
        })
        .sum();
    let lower_bound = pushes
        .iter()
        .any(|it| it.commit_count.is_none() || it.commits_truncated);
    let pushed_today = pr.last_push.is_some_and(|t| in_day(t, day_start, now));
    if pushed_today && snap.iterations_behind.contains(&pr.id) {
        return Commits::New { known };
    }
    if pushes.is_empty() {
        Commits::None
    } else if pushes.iter().all(|it| it.commit_count.is_none()) {
        Commits::Pushes(pushes.len() as u32)
    } else {
        Commits::Known {
            n: known,
            lower_bound,
        }
    }
}

/// FR-5: non-system, non-deleted comments created today by anyone but me.
pub fn comments_today(snap: &Snapshot, pr: i64, day_start: Millis, now: Millis) -> u32 {
    snap.threads.get(&pr).map_or(0, |threads| {
        threads
            .iter()
            .flat_map(|t| &t.comments)
            .filter(|c| !c.is_system && !c.is_deleted && c.author != snap.me)
            .filter(|c| in_day(c.created, day_start, now))
            .count() as u32
    })
}

/// FR-5 `last_activity`: the PR's update, newest push, newest human
/// comment, closure.
pub fn last_activity(snap: &Snapshot, pr: &PullRequest) -> Millis {
    let pushed = snap
        .iterations_today
        .get(&pr.id)
        .and_then(|its| its.iter().map(|it| it.created).max());
    let commented = snap.threads.get(&pr.id).and_then(|threads| {
        threads
            .iter()
            .flat_map(|t| &t.comments)
            .filter(|c| !c.is_system && !c.is_deleted)
            .map(|c| c.created)
            .max()
    });
    [Some(pr.updated), pr.last_push, pushed, commented, pr.closed]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(pr.updated)
}

/// FR-3: today's metrics over the snapshot.
pub fn counts(snap: &Snapshot, day_start: Millis, now: Millis) -> Counts {
    let me = snap.me.as_str();
    let today = |t: Millis| in_day(t, day_start, now);
    let mut counts = Counts::default();

    let mine_merged: Vec<&PullRequest> = snap
        .merged_today
        .iter()
        .filter(|pr| pr.author == me && pr.status == PrStatus::Completed)
        .collect();
    counts.merged = mine_merged
        .iter()
        .filter(|pr| pr.closed.is_some_and(today))
        .count() as u32;

    let mut voted = HashSet::new();
    for pr in snap.prs.iter().chain(&snap.merged_today) {
        if pr.author == me || voted.contains(&pr.id) {
            continue;
        }
        let mine = pr
            .reviewers
            .iter()
            .find(|r| r.person == me && r.vote != Vote::NoVote);
        if mine.and_then(|r| r.voted_at).is_some_and(today) {
            voted.insert(pr.id);
        }
    }
    counts.reviews = voted.len() as u32;

    let authors: HashMap<i64, &str> = snap
        .prs
        .iter()
        .chain(&snap.merged_today)
        .map(|pr| (pr.id, pr.author.as_str()))
        .collect();
    counts.discussions = snap
        .threads
        .iter()
        .flat_map(|(pr, threads)| threads.iter().map(move |t| (*pr, t)))
        .filter(|(pr, thread)| {
            let human = || {
                thread
                    .comments
                    .iter()
                    .filter(|c| !c.is_system && !c.is_deleted && c.author == me)
            };
            let posted_today = human().any(|c| today(c.created));
            let resolved_today = thread.status.is_resolved()
                && today(thread.updated)
                && (authors.get(pr) == Some(&me) || human().next().is_some());
            posted_today || resolved_today
        })
        .count() as u32;

    let mine: Vec<&PullRequest> = snap
        .prs
        .iter()
        .filter(|pr| pr.author == me && pr.status == PrStatus::Active)
        .chain(mine_merged.iter().copied())
        .collect();
    for pr in &mine {
        let (n, lower) = commits_today(snap, pr, day_start, now).count();
        counts.commits += n;
        counts.commits_lower_bound |= lower && n > 0;
        if today(pr.created) {
            counts.opened += 1;
        }
    }
    counts.done = snap.done_today.len() as u32;
    counts
}
