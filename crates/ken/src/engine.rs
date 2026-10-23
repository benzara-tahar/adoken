//! The arcade state machine (ken.md §3): hits with the anti-gaming rules,
//! combos, K.O.s, streak and badges. Pure and clock-injected: the caller
//! feeds [`Action`]s (built from the store by `source`) and a [`Ctx`], and
//! gets back [`Effect`]s for the UI plus a [`Journal`] of rows for the local
//! tables (`persist`).

use std::collections::{BTreeMap, HashMap, HashSet};

use adoken_model::time::{DAY, HOUR, MINUTE};
use adoken_model::{Millis, Vote};
use adoken_notify::schedule::LocalTime;
use adoken_notify::{Clock, Weekdays};
use serde::{Deserialize, Serialize};

use crate::moves::{HitKind, Rank};
use crate::streak::{DayState, Streak};

/// Each hit keeps the combo alive this long (§3.2).
pub const COMBO_WINDOW: Millis = 10 * MINUTE;
/// Anti-gaming: at most this many hits per PR per combo (§3.1).
pub const MAX_HITS_PER_PR: u8 = 3;
/// Actions seen by sync (taken in the browser) count only this fresh.
pub const SYNC_RECENCY: Millis = 15 * MINUTE;
/// ...and only if you opened that PR's files in adoken this recently.
pub const ENGAGED_WITHIN: Millis = DAY;
/// K.O.s for merges older than this go to the fight card only (§3.3).
pub const KO_MAX_AGE: Millis = DAY;
/// Two of your PRs merging this close together are one "DOUBLE K.O.".
pub const DOUBLE_KO_WINDOW: Millis = 10 * MINUTE;
/// First Strike: first reviewer within this long of publishing (§3.5).
pub const FIRST_STRIKE_WINDOW: Millis = HOUR;
/// A comment shorter than this (trimmed, in characters) isn't a hit.
pub const MIN_COMMENT_CHARS: usize = 12;
/// Combos at or above this get the finish flourish, sound and toast.
pub const FLOURISH_MIN_HITS: u32 = 3;
/// A finish noticed later than this (app was asleep) ends silently.
pub const FINISH_GRACE: Millis = MINUTE;
/// Anti-gaming memory older than this is forgotten.
const MEMORY: Millis = 7 * DAY;

/// Where an action was observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Done in adoken and confirmed.
    App,
    /// Seen by sync (or the demo event stream): done in the browser.
    Sync,
}

/// What the rules need to know about the PR an action touched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrFacts {
    pub id: i64,
    pub title: String,
    /// I authored it.
    pub mine: bool,
    pub is_draft: bool,
    /// When it was published (created, for PRs never drafted).
    pub published_at: Option<Millis>,
    /// Earliest vote or comment by a reviewer other than me and the author.
    pub others_engaged_at: Option<Millis>,
    /// Last time I opened its files in adoken (`view_state`).
    pub opened_at: Option<Millis>,
}

impl PrFacts {
    pub fn new(id: i64, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Vote {
        pr: PrFacts,
        vote: Vote,
        /// Source commit the vote was cast on, when known.
        commit: Option<String>,
        at: Millis,
        origin: Origin,
    },
    Comment {
        pr: PrFacts,
        /// `None` when only the event is known (the demo stream carries no
        /// comment text): length and duplicate checks can't apply.
        body: Option<String>,
        at: Millis,
        origin: Origin,
    },
    Resolve {
        pr: PrFacts,
        /// The thread has ≥ 1 comment from someone else.
        others_commented: bool,
        at: Millis,
        origin: Origin,
    },
    /// I completed the PR.
    Complete {
        pr: PrFacts,
        at: Millis,
        origin: Origin,
    },
    /// A changed file (at one version) newly marked Viewed in adoken.
    Viewed { pr: PrFacts, at: Millis },
    /// A PR I authored or reviewed merged; `at` is its `closedDate`.
    Merged {
        pr: PrFacts,
        at: Millis,
        /// I voted 10 or 5 on it.
        i_approved: bool,
    },
    /// A pipeline run for a PR finished.
    Build {
        pr: PrFacts,
        succeeded: bool,
        at: Millis,
    },
    /// The inbox reached zero through my triage.
    InboxCleared { at: Millis },
}

/// Why an action didn't count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Nothing counts during the first sync / backfill.
    Backfill,
    /// Votes (and viewing files) on your own PR don't count.
    OwnPr,
    /// Seen by sync more than 15 min after it happened.
    Stale,
    /// Vote without having opened the PR's files or commented on it; sync
    /// action without having opened its files in the last 24 h.
    NotEngaged,
    /// Already 3 hits on this PR in this combo.
    PrCap,
    /// Same vote / completion counted before (flip-flopping counts once).
    Repeat,
    ShortComment,
    DuplicateComment,
    /// Resolving a thread nobody else commented on.
    NoOtherCommenter,
    /// Resetting a vote isn't a review action.
    NotAHit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Badge {
    FirstStrike { pr: i64, title: String },
    CleanSweep,
    Comeback { pr: i64, title: String },
    Assist { pr: i64, title: String },
    Streak { days: u32 },
}

impl Badge {
    pub fn kind(&self) -> &'static str {
        match self {
            Badge::FirstStrike { .. } => "first_strike",
            Badge::CleanSweep => "clean_sweep",
            Badge::Comeback { .. } => "comeback",
            Badge::Assist { .. } => "assist",
            Badge::Streak { .. } => "streak",
        }
    }

    pub fn pr(&self) -> Option<i64> {
        match self {
            Badge::FirstStrike { pr, .. }
            | Badge::Comeback { pr, .. }
            | Badge::Assist { pr, .. } => Some(*pr),
            Badge::CleanSweep | Badge::Streak { .. } => None,
        }
    }

    /// Toast copy (ken.md §3.5).
    pub fn toast(&self) -> String {
        match self {
            Badge::FirstStrike { pr, title } => format!("First strike · !{pr} {title}"),
            Badge::CleanSweep => "Clean sweep · inbox zero".to_owned(),
            Badge::Comeback { pr, .. } => format!("Comeback · build green on !{pr}"),
            Badge::Assist { pr, .. } => format!("Assist · !{pr} merged"),
            Badge::Streak { days } => format!("{days}-day streak"),
        }
    }
}

/// What the UI shows or plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// A hit landed; `combo` is the running count (1 = not a combo yet).
    Hit {
        kind: HitKind,
        pr: i64,
        combo: u32,
    },
    /// The combo counter moved (×2 and up).
    Combo {
        count: u32,
        rank: Rank,
        /// The caption changed (×5, ×8, ×12).
        rank_up: bool,
        /// Window end.
        until: Millis,
    },
    /// The window expired.
    ComboFinished {
        count: u32,
        rank: Option<Rank>,
        prs: usize,
        duration: Millis,
        /// ×3+ noticed on time: flourish, sound, toast. Otherwise a quiet fade.
        flourish: bool,
    },
    /// One overlay per group; `double` = two or more within 10 min.
    Ko {
        prs: Vec<(i64, String)>,
        double: bool,
    },
    Badge(Badge),
    /// The streak grew today (first hit of a working day).
    Streak {
        days: u32,
        milestone: bool,
    },
}

/// The combo in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combo {
    pub id: i64,
    pub started_at: Millis,
    pub last_hit_at: Millis,
    pub hits: u32,
    pub per_pr: BTreeMap<i64, u8>,
    pub last_kind: HitKind,
}

impl Combo {
    pub fn until(&self) -> Millis {
        self.last_hit_at + COMBO_WINDOW
    }
}

/// Rows for the local tables, drained by `persist::save`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Journal {
    pub hits: Vec<HitRow>,
    pub combos: Vec<ComboRow>,
    pub kos: Vec<KoRow>,
    pub badges: Vec<BadgeRow>,
    pub streak: Vec<(i64, DayState)>,
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
            && self.combos.is_empty()
            && self.kos.is_empty()
            && self.badges.is_empty()
            && self.streak.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitRow {
    pub at: Millis,
    pub kind: HitKind,
    pub pr: i64,
    pub combo_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboRow {
    pub id: i64,
    pub started_at: Millis,
    pub ended_at: Millis,
    pub hits: u32,
    pub best_rank: Option<Rank>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KoRow {
    pub pr: i64,
    pub at: Millis,
    pub double: bool,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeRow {
    pub at: Millis,
    pub kind: &'static str,
    pub pr: Option<i64>,
}

/// Per-call environment.
pub struct Ctx<'a> {
    pub now: Millis,
    pub clock: &'a dyn Clock,
    pub working_days: Weekdays,
    /// First sync / backfill in progress.
    pub backfilling: bool,
}

/// What the HUD and chip read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboView {
    pub count: u32,
    pub rank: Rank,
    pub until: Millis,
    pub last_kind: HitKind,
}

/// Engine state; serialized whole into `ken_kv` between runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ken {
    pub combo: Option<Combo>,
    pub streak: Streak,
    next_combo_id: i64,
    /// PR → last time I viewed one of its files in adoken.
    viewed: HashMap<i64, Millis>,
    /// PR → last comment hit of mine.
    commented: HashMap<i64, Millis>,
    /// Vote dedupe key (`pr` or `pr:commit`) → when it counted.
    votes: HashMap<String, Millis>,
    /// Normalized text of my previous comment.
    last_comment: Option<String>,
    completes: HashSet<i64>,
    kos: HashSet<i64>,
    assists: HashSet<i64>,
    first_strikes: HashSet<i64>,
    /// PR → last pipeline run succeeded.
    builds: HashMap<i64, bool>,
    clean_sweep_day: Option<i64>,
    #[serde(skip)]
    journal: Journal,
}

impl Ken {
    pub fn take_journal(&mut self) -> Journal {
        std::mem::take(&mut self.journal)
    }

    /// The live combo (×2+, window not expired at `now`).
    pub fn combo_view(&self, now: Millis) -> Option<ComboView> {
        let combo = self.combo.as_ref().filter(|c| now <= c.until())?;
        Some(ComboView {
            count: combo.hits,
            rank: Rank::for_hits(combo.hits)?,
            until: combo.until(),
            last_kind: combo.last_kind,
        })
    }

    /// Apply a batch in order. K.O.s in one batch are grouped (DOUBLE K.O.).
    /// Rejections are returned alongside for callers that log them.
    pub fn apply(&mut self, actions: Vec<Action>, ctx: &Ctx<'_>) -> Applied {
        let mut applied = Applied::default();
        let mut merges = Vec::new();
        for action in actions {
            match action {
                Action::Merged { pr, at, i_approved } => {
                    self.merged(pr, at, i_approved, ctx, &mut merges, &mut applied.effects)
                }
                Action::Build { pr, succeeded, at } => {
                    self.build(pr, succeeded, at, ctx, &mut applied.effects)
                }
                Action::InboxCleared { at } => self.inbox_cleared(at, ctx, &mut applied.effects),
                hit => match self.hit(hit, ctx, &mut applied.effects) {
                    Ok(kind) => applied.accepted.push(kind),
                    Err(rejection) => applied.rejected.push(rejection),
                },
            }
        }
        self.group_kos(merges, &mut applied.effects);
        applied.effects.extend(self.tick(ctx));
        applied
    }

    /// Time passing: finish an expired combo, settle missed streak days,
    /// forget old anti-gaming memory.
    pub fn tick(&mut self, ctx: &Ctx<'_>) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.combo.as_ref().is_some_and(|c| ctx.now > c.until()) {
            effects.extend(self.finish(ctx.now));
        }
        let today = LocalTime::of(ctx.now, ctx.clock).day;
        let settled = self.streak.settle(today, ctx.working_days);
        self.journal.streak.extend(settled);
        let horizon = ctx.now - MEMORY;
        self.viewed.retain(|_, at| *at > horizon);
        self.commented.retain(|_, at| *at > horizon);
        self.votes.retain(|_, at| *at > horizon);
        effects
    }

    /// Why `action` would be rejected, without changing anything.
    pub fn check(&self, action: &Action, ctx: &Ctx<'_>) -> Result<HitKind, Rejection> {
        self.clone().validate(action, ctx).map(|(kind, _)| kind)
    }

    fn hit(
        &mut self,
        action: Action,
        ctx: &Ctx<'_>,
        effects: &mut Vec<Effect>,
    ) -> Result<HitKind, Rejection> {
        let (kind, pr) = self.validate(&action, ctx)?;
        let at = action_at(&action);
        // Record the hit in the combo.
        if self.combo.as_ref().is_some_and(|c| at > c.until()) {
            effects.extend(self.finish(ctx.now));
        }
        let prev_rank = self.combo.as_ref().and_then(|c| Rank::for_hits(c.hits));
        if self.combo.is_none() {
            self.next_combo_id += 1;
            self.combo = Some(Combo {
                id: self.next_combo_id,
                started_at: at,
                last_hit_at: at,
                hits: 0,
                per_pr: BTreeMap::new(),
                last_kind: kind,
            });
        }
        let combo = self.combo.as_mut().expect("combo just ensured");
        combo.hits += 1;
        combo.last_hit_at = combo.last_hit_at.max(at);
        combo.last_kind = kind;
        *combo.per_pr.entry(pr.id).or_default() += 1;
        let (count, until, combo_id) = (combo.hits, combo.until(), combo.id);
        self.journal.hits.push(HitRow {
            at,
            kind,
            pr: pr.id,
            combo_id,
        });
        effects.push(Effect::Hit {
            kind,
            pr: pr.id,
            combo: count,
        });
        if let Some(rank) = Rank::for_hits(count) {
            effects.push(Effect::Combo {
                count,
                rank,
                rank_up: prev_rank.is_some_and(|prev| rank > prev),
                until,
            });
        }
        // Streak: first hit of a working day.
        let day = LocalTime::of(at, ctx.clock).day;
        if let Some(extended) = self
            .streak
            .hit(day, ctx.working_days, &mut self.journal.streak)
        {
            effects.push(Effect::Streak {
                days: extended.days,
                milestone: extended.milestone,
            });
            if extended.milestone {
                self.badge(
                    Badge::Streak {
                        days: extended.days,
                    },
                    at,
                    effects,
                );
            }
        }
        // First Strike: first reviewer on a teammate's fresh, published PR.
        if (kind.is_vote() || kind == HitKind::Comment)
            && !pr.mine
            && !pr.is_draft
            && pr
                .published_at
                .is_some_and(|p| at >= p && at - p <= FIRST_STRIKE_WINDOW)
            && pr.others_engaged_at.is_none_or(|other| other > at)
            && self.first_strikes.insert(pr.id)
        {
            self.badge(
                Badge::FirstStrike {
                    pr: pr.id,
                    title: pr.title.clone(),
                },
                at,
                effects,
            );
        }
        Ok(kind)
    }

    /// The anti-gaming rules. Records engagement (viewing, commenting) and
    /// dedupe memory as a side effect only for actions that pass, except
    /// that engagement still counts when only the per-PR cap rejects.
    fn validate<'a>(
        &mut self,
        action: &'a Action,
        ctx: &Ctx<'_>,
    ) -> Result<(HitKind, &'a PrFacts), Rejection> {
        if ctx.backfilling {
            return Err(Rejection::Backfill);
        }
        let (kind, pr, at, origin) = match action {
            Action::Vote {
                pr,
                vote,
                at,
                origin,
                ..
            } => {
                let kind = match vote {
                    Vote::Approved => HitKind::Approve,
                    Vote::ApprovedWithSuggestions => HitKind::ApproveWithSuggestions,
                    Vote::WaitingForAuthor => HitKind::WaitForAuthor,
                    Vote::Rejected => HitKind::Reject,
                    Vote::NoVote => return Err(Rejection::NotAHit),
                };
                (kind, pr, *at, *origin)
            }
            Action::Comment { pr, at, origin, .. } => (HitKind::Comment, pr, *at, *origin),
            Action::Resolve { pr, at, origin, .. } => (HitKind::Resolve, pr, *at, *origin),
            Action::Complete { pr, at, origin } => (HitKind::Complete, pr, *at, *origin),
            Action::Viewed { pr, at } => (HitKind::Viewed, pr, *at, Origin::App),
            Action::Merged { .. } | Action::Build { .. } | Action::InboxCleared { .. } => {
                return Err(Rejection::NotAHit);
            }
        };
        if pr.mine && (kind.is_vote() || kind == HitKind::Viewed) {
            return Err(Rejection::OwnPr);
        }
        if origin == Origin::Sync && ctx.now - at > SYNC_RECENCY {
            return Err(Rejection::Stale);
        }
        let opened = self
            .viewed
            .get(&pr.id)
            .copied()
            .max(pr.opened_at)
            .is_some_and(|opened| at - opened <= ENGAGED_WITHIN && opened <= at + SYNC_RECENCY);
        let commented = self
            .commented
            .get(&pr.id)
            .is_some_and(|c| at - *c <= ENGAGED_WITHIN);
        let engaged = match (kind, origin) {
            (HitKind::Viewed, _) => true,
            (k, _) if k.is_vote() => opened || commented,
            (_, Origin::Sync) => opened,
            (_, Origin::App) => true,
        };
        if !engaged {
            return Err(Rejection::NotEngaged);
        }
        match action {
            Action::Vote { commit, .. } => {
                let key = match commit {
                    Some(commit) => format!("{}:{commit}", pr.id),
                    None => pr.id.to_string(),
                };
                if self.votes.contains_key(&key) {
                    return Err(Rejection::Repeat);
                }
            }
            Action::Comment {
                body: Some(body), ..
            } => {
                let normalized = normalize(body);
                if normalized.chars().count() < MIN_COMMENT_CHARS {
                    return Err(Rejection::ShortComment);
                }
                if self.last_comment.as_deref() == Some(normalized.as_str()) {
                    return Err(Rejection::DuplicateComment);
                }
            }
            Action::Resolve {
                others_commented: false,
                ..
            } => return Err(Rejection::NoOtherCommenter),
            Action::Complete { .. } if self.completes.contains(&pr.id) => {
                return Err(Rejection::Repeat);
            }
            _ => {}
        }
        // Engagement counts even when the cap below rejects the hit.
        match action {
            Action::Viewed { .. } => {
                self.viewed.insert(pr.id, at);
            }
            Action::Comment { body, .. } => {
                self.commented.insert(pr.id, at);
                if let Some(body) = body {
                    self.last_comment = Some(normalize(body));
                }
            }
            _ => {}
        }
        let capped = self.combo.as_ref().is_some_and(|c| {
            at <= c.until() && c.per_pr.get(&pr.id).copied().unwrap_or(0) >= MAX_HITS_PER_PR
        });
        if capped {
            return Err(Rejection::PrCap);
        }
        match action {
            Action::Vote { commit, .. } => {
                let key = match commit {
                    Some(commit) => format!("{}:{commit}", pr.id),
                    None => pr.id.to_string(),
                };
                self.votes.insert(key, at);
            }
            Action::Complete { .. } => {
                self.completes.insert(pr.id);
            }
            _ => {}
        }
        Ok((kind, pr))
    }

    fn finish(&mut self, now: Millis) -> Option<Effect> {
        let combo = self.combo.take()?;
        let ended_at = combo.until();
        let rank = Rank::for_hits(combo.hits);
        if combo.hits >= 2 {
            self.journal.combos.push(ComboRow {
                id: combo.id,
                started_at: combo.started_at,
                ended_at,
                hits: combo.hits,
                best_rank: rank,
            });
        } else {
            return None;
        }
        Some(Effect::ComboFinished {
            count: combo.hits,
            rank,
            prs: combo.per_pr.len(),
            duration: combo.last_hit_at - combo.started_at,
            flourish: combo.hits >= FLOURISH_MIN_HITS && now - ended_at <= FINISH_GRACE,
        })
    }

    fn merged(
        &mut self,
        pr: PrFacts,
        at: Millis,
        i_approved: bool,
        ctx: &Ctx<'_>,
        merges: &mut Vec<(PrFacts, Millis, bool)>,
        effects: &mut Vec<Effect>,
    ) {
        let fresh = ctx.now - at <= KO_MAX_AGE && !ctx.backfilling;
        if pr.mine {
            if self.kos.insert(pr.id) {
                merges.push((pr, at, fresh));
            }
        } else if i_approved && fresh && self.assists.insert(pr.id) {
            self.badge(
                Badge::Assist {
                    pr: pr.id,
                    title: pr.title,
                },
                at,
                effects,
            );
        }
    }

    /// Fresh merges within [`DOUBLE_KO_WINDOW`] of each other share one
    /// overlay; every merge gets a `ken_kos` row (stale ones: fight card only).
    fn group_kos(&mut self, mut merges: Vec<(PrFacts, Millis, bool)>, effects: &mut Vec<Effect>) {
        merges.sort_by_key(|(_, at, _)| *at);
        let mut groups: Vec<Vec<(PrFacts, Millis, bool)>> = Vec::new();
        for merge in merges {
            match groups.last_mut() {
                Some(group)
                    if merge.2 && group[0].2 && merge.1 - group[0].1 <= DOUBLE_KO_WINDOW =>
                {
                    group.push(merge)
                }
                _ => groups.push(vec![merge]),
            }
        }
        for group in groups {
            let double = group.len() > 1;
            let fresh = group[0].2;
            for (pr, at, _) in &group {
                self.journal.kos.push(KoRow {
                    pr: pr.id,
                    at: *at,
                    double,
                    title: pr.title.clone(),
                });
            }
            if fresh {
                effects.push(Effect::Ko {
                    prs: group
                        .into_iter()
                        .map(|(pr, _, _)| (pr.id, pr.title))
                        .collect(),
                    double,
                });
            }
        }
    }

    fn build(
        &mut self,
        pr: PrFacts,
        succeeded: bool,
        at: Millis,
        ctx: &Ctx<'_>,
        effects: &mut Vec<Effect>,
    ) {
        if !pr.mine {
            return;
        }
        let previous = self.builds.insert(pr.id, succeeded);
        if previous == Some(false) && succeeded && !ctx.backfilling && ctx.now - at <= KO_MAX_AGE {
            self.badge(
                Badge::Comeback {
                    pr: pr.id,
                    title: pr.title,
                },
                at,
                effects,
            );
        }
    }

    fn inbox_cleared(&mut self, at: Millis, ctx: &Ctx<'_>, effects: &mut Vec<Effect>) {
        let day = LocalTime::of(at, ctx.clock).day;
        if ctx.backfilling || self.clean_sweep_day == Some(day) {
            return;
        }
        self.clean_sweep_day = Some(day);
        self.badge(Badge::CleanSweep, at, effects);
    }

    fn badge(&mut self, badge: Badge, at: Millis, effects: &mut Vec<Effect>) {
        self.journal.badges.push(BadgeRow {
            at,
            kind: badge.kind(),
            pr: badge.pr(),
        });
        effects.push(Effect::Badge(badge));
    }

    /// "Pause streak" / "Resume streak".
    pub fn set_streak_paused(&mut self, paused: bool, ctx: &Ctx<'_>) {
        let today = LocalTime::of(ctx.now, ctx.clock).day;
        if paused {
            let settled = self.streak.pause(today, ctx.working_days);
            self.journal.streak.extend(settled);
        } else {
            self.streak.resume(today);
        }
    }
}

/// Outcome of [`Ken::apply`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub effects: Vec<Effect>,
    pub accepted: Vec<HitKind>,
    pub rejected: Vec<Rejection>,
}

fn action_at(action: &Action) -> Millis {
    match action {
        Action::Vote { at, .. }
        | Action::Comment { at, .. }
        | Action::Resolve { at, .. }
        | Action::Complete { at, .. }
        | Action::Viewed { at, .. }
        | Action::Merged { at, .. }
        | Action::Build { at, .. }
        | Action::InboxCleared { at } => *at,
    }
}

/// Whitespace-collapsed, case-folded comment text.
fn normalize(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests;
