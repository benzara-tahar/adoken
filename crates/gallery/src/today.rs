//! Today stories (spec 0017 AC-31, `docs/design/today.md` §9): hand-built
//! days over the demo org's people (fake Contoso data) with a fixed clock,
//! so snapshots never drift. The story day is Friday 2026-09-25, working
//! hours 09:00–18:00 UTC.

use std::sync::{Arc, LazyLock};

use adoken_app::today::TodayView;
use adoken_app::today::model::Loaded;
use adoken_demo::Dataset;
use adoken_model::time::{DAY, HOUR, MINUTE};
use adoken_model::{
    Comment, MergeStatus, Millis, PersonId, PipelineRun, PrIteration, PrStatus, PrThread,
    PullRequest, Reviewer, RunReason, RunResult, RunStatus, StateCategory, Subject, ThreadStatus,
    Vote, WorkItem, WorkItemType,
};
use adoken_nudge::{
    DataHealth, FixedClock, LaneHealth, Ledger, PrState, Settings, Snapshot, SyncHealth,
};
use adoken_theme::{space, typography};
use adoken_ui::{ActiveTheme, StageBar, TypeStyleExt};
use gpui::{AnyElement, App, Context, Window, div, prelude::*, px};

static DEMO: LazyLock<Dataset> = LazyLock::new(|| adoken_demo::dataset(adoken_demo::DEFAULT_SEED));

/// Friday 00:00 UTC of the story day.
fn midnight() -> Millis {
    Settings::default().hours.day_start(adoken_demo::DEMO_NOW)
}

/// `hour:minute` on the story day (`day` 0), or days before (-1 …).
fn at(day: i64, hour: i64, minute: i64) -> Millis {
    midnight() + day * DAY + hour * HOUR + minute * MINUTE
}

/// The demo's people: me, then colleagues (no groups).
struct Cast {
    me: PersonId,
    others: Vec<PersonId>,
}

fn cast() -> Cast {
    let ds = &*DEMO;
    Cast {
        me: ds.me.clone(),
        others: ds
            .people
            .iter()
            .filter(|p| !p.is_group && p.id != ds.me)
            .map(|p| p.id.clone())
            .take(6)
            .collect(),
    }
}

fn reviewer(person: &str, vote: Vote, required: bool, voted: Option<Millis>) -> Reviewer {
    Reviewer {
        person: person.to_owned(),
        vote,
        is_required: required,
        has_declined: false,
        added_at: None,
        voted_at: voted,
    }
}

fn pr(id: i64, title: &str, author: &str, created: Millis) -> PullRequest {
    let repo = &DEMO.repos[0];
    PullRequest {
        id,
        repo_id: repo.id.clone(),
        project_id: repo.project_id.clone(),
        title: title.to_owned(),
        description: String::new(),
        author: author.to_owned(),
        reviewers: Vec::new(),
        status: PrStatus::Active,
        is_draft: false,
        merge_status: MergeStatus::Succeeded,
        source_branch: format!("refs/heads/feature/{id}"),
        target_branch: "refs/heads/main".to_owned(),
        last_source_commit: None,
        created,
        updated: created,
        closed: None,
        iterations: 1,
        last_push: Some(created),
    }
}

fn comment(id: i64, author: &str, created: Millis) -> Comment {
    Comment {
        id,
        parent_id: None,
        author: author.to_owned(),
        body: "Looks reasonable".to_owned(),
        created,
        updated: created,
        is_system: false,
        is_deleted: false,
    }
}

fn thread(pr_id: i64, comments: Vec<Comment>) -> PrThread {
    PrThread {
        id: 1,
        pr_id,
        status: ThreadStatus::Active,
        anchor: None,
        iteration: None,
        updated: comments.iter().map(|c| c.updated).max().unwrap_or(0),
        comments,
    }
}

fn push(id: u32, created: Millis, count: u32) -> PrIteration {
    PrIteration {
        id,
        source_commit: format!("{id:040x}"),
        created,
        description: None,
        commit_count: Some(count),
        commits_truncated: false,
    }
}

fn work_item(id: i64, title: &str, kind: WorkItemType, changed: Millis, links: &[i64]) -> WorkItem {
    let me = DEMO.me.clone();
    WorkItem {
        id,
        project_id: DEMO.repos[0].project_id.clone(),
        rev: 1,
        kind,
        title: title.to_owned(),
        state: "Active".to_owned(),
        state_category: StateCategory::InProgress,
        assigned_to: Some(me.clone()),
        created_by: me.clone(),
        changed_by: me,
        area_path: String::new(),
        iteration_path: String::new(),
        priority: None,
        tags: Vec::new(),
        parent_id: None,
        linked_pr_ids: links.to_vec(),
        description: None,
        comment_count: 0,
        created: changed,
        changed,
    }
}

/// A day under construction.
struct Day {
    snap: Snapshot,
    ledger: Ledger,
    now: Millis,
}

impl Day {
    fn new(now: Millis) -> Self {
        let mut snap = Snapshot::empty(DEMO.me.clone());
        for p in &DEMO.people {
            snap.names.insert(p.id.clone(), p.display_name.clone());
            if p.is_group {
                snap.groups.insert(p.id.clone());
            }
        }
        Day {
            snap,
            ledger: Ledger::default(),
            now,
        }
    }

    fn pr(&mut self, pr: PullRequest) -> &mut Self {
        self.snap.prs.push(pr);
        self
    }

    /// A review asked of me on a colleague's PR.
    fn asked(&mut self, id: i64, title: &str, by: usize, since: Millis) -> &mut Self {
        let c = cast();
        let mut p = pr(id, title, &c.others[by], since);
        p.reviewers
            .push(reviewer(&c.me, Vote::NoVote, by.is_multiple_of(2), None));
        p.reviewers
            .push(reviewer(&c.others[(by + 1) % 6], Vote::NoVote, false, None));
        self.pr(p)
    }

    /// My PR, in review with two colleagues.
    fn mine(&mut self, id: i64, title: &str, created: Millis, votes: [Vote; 2]) -> PullRequest {
        let c = cast();
        let mut p = pr(id, title, &c.me, created);
        for (i, vote) in votes.into_iter().enumerate() {
            let voted = (vote != Vote::NoVote).then_some(created + 2 * HOUR);
            p.reviewers
                .push(reviewer(&c.others[i + 2], vote, i == 0, voted));
        }
        p
    }

    fn pushes(&mut self, id: i64, pushes: Vec<PrIteration>) -> &mut Self {
        if let Some(p) = self.snap.prs.iter_mut().find(|p| p.id == id) {
            p.last_push = pushes.iter().map(|it| it.created).max();
            p.iterations = pushes.iter().map(|it| it.id).max().unwrap_or(1);
        }
        self.snap.iterations_today.insert(id, pushes);
        self
    }

    fn merged(&mut self, id: i64, title: &str, closed: Millis) -> &mut Self {
        let mut p = self.mine(id, title, at(-2, 10, 0), [Vote::Approved, Vote::Approved]);
        p.status = PrStatus::Completed;
        p.closed = Some(closed);
        p.updated = closed;
        self.snap.merged_today.push(p);
        self
    }

    fn loaded(&self, settings: &Settings) -> Loaded {
        Loaded::compute(
            Arc::new(self.snap.clone()),
            self.ledger.clone(),
            settings,
            self.now,
        )
    }

    fn build(&self) -> Loaded {
        self.loaded(&Settings::default())
    }
}

fn view(
    key: &'static str,
    loaded: impl FnOnce() -> Loaded,
    tweak: impl FnOnce(TodayView) -> TodayView,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |_, cx: &mut Context<TodayView>| {
        let loaded = loaded();
        let clock = Arc::new(FixedClock::new(loaded.now));
        tweak(TodayView::with_loaded(
            loaded,
            clock,
            Settings::default(),
            cx,
        ))
    });
    div().size_full().child(view).into_any_element()
}

fn plain(
    key: &'static str,
    loaded: impl FnOnce() -> Loaded,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    view(key, loaded, |v| v, window, cx)
}

// ── moods ─────────────────────────────────────────────────────────────────

/// 09:40, nothing done yet, one review asked yesterday.
fn fresh_start() -> Loaded {
    let mut d = Day::new(at(0, 9, 40));
    d.asked(
        4821,
        "Fix login throttle on the token endpoint",
        0,
        at(-1, 14, 0),
    );
    let mine = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 11, 0),
        [Vote::NoVote, Vote::NoVote],
    );
    d.pr(mine);
    d.build()
}

/// 11:00, five commits pushed to my PR.
fn finding_flow() -> Loaded {
    let mut d = Day::new(at(0, 11, 0));
    let mine = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 11, 0),
        [Vote::NoVote, Vote::NoVote],
    );
    d.pr(mine);
    d.pushes(
        5101,
        vec![push(2, at(0, 9, 10), 3), push(3, at(0, 10, 20), 2)],
    );
    d.asked(
        4821,
        "Fix login throttle on the token endpoint",
        0,
        at(0, 8, 30),
    );
    d.build()
}

/// 11:30, two reviews and a discussion, one commit.
fn helping_grow() -> Loaded {
    let c = cast();
    let mut d = Day::new(at(0, 11, 30));
    let mut a = pr(
        4790,
        "Harden OIDC token refresh",
        &c.others[0],
        at(-2, 9, 0),
    );
    a.reviewers
        .push(reviewer(&c.me, Vote::Approved, false, Some(at(0, 10, 0))));
    let mut b = pr(
        4793,
        "Split cart merge after sign-in",
        &c.others[1],
        at(-2, 9, 0),
    );
    b.reviewers.push(reviewer(
        &c.me,
        Vote::WaitingForAuthor,
        false,
        Some(at(0, 10, 30)),
    ));
    d.snap.threads.insert(
        4793,
        vec![thread(4793, vec![comment(1, &c.me, at(0, 10, 31))])],
    );
    d.pr(a).pr(b);
    let mine = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 11, 0),
        [Vote::Approved, Vote::NoVote],
    );
    d.pr(mine);
    d.pushes(5101, vec![push(2, at(0, 9, 30), 1)]);
    d.build()
}

/// 14:00, my PRs wait on reviewers, nothing owed.
fn waiting() -> Loaded {
    let mut d = Day::new(at(0, 14, 0));
    for (id, title) in [
        (5101, "Retry idempotency keys on payout timeouts"),
        (5102, "Ledger export for auditors"),
    ] {
        let p = d.mine(id, title, at(-1, 10, 0), [Vote::NoVote, Vote::NoVote]);
        d.pr(p);
    }
    d.build()
}

/// 14:00, a conflict and a failed build, nothing done yet.
fn blocked() -> Loaded {
    let mut d = Day::new(at(0, 14, 0));
    let mut conflict = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 9, 0),
        [Vote::Approved, Vote::NoVote],
    );
    conflict.merge_status = MergeStatus::Conflicts;
    conflict.last_push = Some(at(0, 9, 0) - 30 * MINUTE);
    let build = d.mine(
        5102,
        "Ledger export for auditors",
        at(-1, 9, 0),
        [Vote::Approved, Vote::Approved],
    );
    d.pr(conflict).pr(build);
    d.snap.runs.push(failed_run(90_001, 5102, at(0, 12, 10)));
    d.build()
}

fn failed_run(id: i64, pr_id: i64, finish: Millis) -> PipelineRun {
    PipelineRun {
        id,
        project_id: DEMO.repos[0].project_id.clone(),
        pipeline_id: 1,
        pipeline_name: "payments-ci".to_owned(),
        run_number: "20260925.7".to_owned(),
        status: RunStatus::Completed,
        result: Some(RunResult::Failed),
        reason: RunReason::PullRequest,
        branch: format!("refs/pull/{pr_id}/merge"),
        source_version: None,
        requested_for: Some(DEMO.me.clone()),
        pr_id: Some(pr_id),
        queued: finish - 20 * MINUTE,
        started: Some(finish - 19 * MINUTE),
        finish: Some(finish),
    }
}

/// 15:00, a PR of mine merged at 14:32.
fn shipped() -> Loaded {
    let mut d = Day::new(at(0, 15, 0));
    d.merged(5099, "Bump azure-identity to 1.17", at(0, 14, 32));
    let mine = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 11, 0),
        [Vote::Approved, Vote::NoVote],
    );
    d.pr(mine);
    d.build()
}

/// 14:00, nothing needs me: my PR is approved.
fn clear() -> Loaded {
    let mut d = Day::new(at(0, 14, 0));
    let mine = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 11, 0),
        [Vote::Approved, Vote::ApprovedWithSuggestions],
    );
    d.pr(mine);
    d.build()
}

/// 15:00, two reviews owed since yesterday, nothing recorded today.
fn stalled() -> Loaded {
    let mut d = Day::new(at(0, 15, 0));
    d.asked(
        4821,
        "Fix login throttle on the token endpoint",
        0,
        at(-1, 10, 0),
    )
    .asked(4347, "Fix Terraform state locking", 1, at(-1, 11, 0));
    let mine = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-1, 11, 0),
        [Vote::NoVote, Vote::NoVote],
    );
    d.pr(mine);
    d.build()
}

/// The eight mood stories' days in FR-7 order (smoke runs switch Today
/// through them, AC-28/AC-30).
pub fn mood_days() -> [Loaded; 8] {
    [
        fresh_start(),
        finding_flow(),
        helping_grow(),
        waiting(),
        blocked(),
        shipped(),
        clear(),
        stalled(),
    ]
}

pub fn today_mood_fresh_start_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-fresh-start", fresh_start, window, cx)
}

pub fn today_mood_finding_flow_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-finding-flow", finding_flow, window, cx)
}

pub fn today_mood_helping_grow_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-helping-grow", helping_grow, window, cx)
}

pub fn today_mood_waiting_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-waiting", waiting, window, cx)
}

pub fn today_mood_blocked_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-blocked", blocked, window, cx)
}

pub fn today_mood_shipped_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-shipped", shipped, window, cx)
}

pub fn today_mood_clear_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-clear", clear, window, cx)
}

pub fn today_mood_stalled_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-mood-stalled", stalled, window, cx)
}

// ── the busy day and its variants ─────────────────────────────────────────

/// AC-8's mixed day with every panel filled: a merge at 14:32, a conflict,
/// a PR with today's commits and comments, a draft, three reviews waiting
/// (one snoozed), two loose ends.
fn busy_day() -> Day {
    let c = cast();
    let mut d = Day::new(at(0, 15, 30));
    d.merged(5099, "Add keyboard shortcuts to PR review", at(0, 14, 32));
    let mut conflict = d.mine(
        5101,
        "Retry idempotency keys on payout timeouts",
        at(-2, 9, 0),
        [Vote::Approved, Vote::NoVote],
    );
    conflict.merge_status = MergeStatus::Conflicts;
    conflict.last_push = Some(at(0, 10, 0));
    conflict.updated = at(0, 10, 0);
    let mut active = d.mine(
        5104,
        "Fix session timeout on reconnect",
        at(-1, 16, 0),
        [Vote::Approved, Vote::NoVote],
    );
    active.updated = at(0, 15, 10);
    let mut changes = d.mine(
        5103,
        "Improve supplier search",
        at(-3, 9, 0),
        [Vote::WaitingForAuthor, Vote::NoVote],
    );
    changes.reviewers[0].voted_at = Some(at(-1, 9, 0));
    changes.last_push = Some(at(-2, 9, 0));
    let mut draft = d.mine(
        5105,
        "Sketch ledger export for auditors",
        at(0, 9, 30),
        [Vote::NoVote, Vote::NoVote],
    );
    draft.is_draft = true;
    d.pr(conflict).pr(active).pr(changes).pr(draft);
    d.pushes(5104, vec![push(3, at(0, 13, 40), 2)]);
    d.pushes(5105, vec![push(1, at(0, 9, 30), 1)]);
    d.snap.threads.insert(
        5104,
        vec![thread(5104, vec![comment(1, &c.others[2], at(0, 14, 50))])],
    );
    d.asked(
        4821,
        "Fix login throttle on the token endpoint",
        0,
        at(-2, 10, 0),
    )
    .asked(4347, "Fix Terraform state locking", 1, at(-6, 11, 0))
    .asked(
        4902,
        "Review offline retry for the mobile cart",
        3,
        at(0, 13, 30),
    );
    // One merged PR still has its work item active (N6); another item has
    // been quiet for a week (N7).
    let mut linked = d.mine(
        4990,
        "Rotate payout webhooks secret",
        at(-9, 9, 0),
        [Vote::Approved, Vote::Approved],
    );
    linked.status = PrStatus::Completed;
    linked.closed = Some(at(-3, 12, 0));
    d.pr(linked);
    d.snap.work_items.push(work_item(
        48213,
        "Fix SSO redirect after password reset",
        WorkItemType::Bug,
        at(-6, 10, 0),
        &[4990],
    ));
    d.snap.work_items.push(work_item(
        48230,
        "Update release notes template",
        WorkItemType::Task,
        at(-9, 10, 0),
        &[],
    ));
    d.snap.state_since.insert(48213, at(-8, 9, 0));
    d.snap.state_since.insert(48230, at(-12, 9, 0));
    let snooze_until = Settings::default().hours.next_workday_start(d.now);
    d.ledger.snooze(&Subject::Pr(4902), snooze_until);
    d
}

pub fn today_busy_day_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-busy-day", || busy_day().build(), window, cx)
}

pub fn today_narrow_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain("today-narrow", || busy_day().build(), window, cx)
}

/// `e` hid Up next: Waiting on you moves to the top of the right column.
pub fn today_up_next_hidden_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain(
        "today-up-next-hidden",
        || {
            let mut d = busy_day();
            let settings = Settings::default();
            d.ledger.dismiss_today(d.now, &settings.hours);
            d.loaded(&settings)
        },
        window,
        cx,
    )
}

/// The pass failed after a good one: the banner over the last good day.
pub fn today_error_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "today-error",
        || busy_day().build(),
        |v| v.with_error("database is locked"),
        window,
        cx,
    )
}

// ── states ────────────────────────────────────────────────────────────────

/// "Me" is known, the first sync is still running: rows that synced show,
/// no mood is claimed.
pub fn today_first_sync_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain(
        "today-first-sync",
        || {
            let mut d = Day::new(at(0, 15, 30));
            d.snap.health = DataHealth {
                initial_sync_done: false,
                ..DataHealth::FRESH
            };
            let mine = d.mine(
                5101,
                "Retry idempotency keys on payout timeouts",
                at(-1, 11, 0),
                [Vote::NoVote, Vote::NoVote],
            );
            d.pr(mine);
            d.build()
        },
        window,
        cx,
    )
}

/// The Work items lane last succeeded 40 minutes ago: no "Clear for now"
/// on stale data.
pub fn today_stale_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain(
        "today-stale",
        || {
            let now = at(0, 14, 0);
            let sync = SyncHealth {
                my_prs: LaneHealth {
                    last_ok: Some(now - MINUTE),
                    interval: Some(MINUTE),
                },
                work_items: LaneHealth {
                    last_ok: Some(at(0, 13, 20)),
                    interval: Some(5 * MINUTE),
                },
                ..SyncHealth::default()
            };
            let mut d = Day::new(now);
            d.snap.health = sync.data_health(true, now);
            let mine = d.mine(
                5101,
                "Retry idempotency keys on payout timeouts",
                at(-1, 11, 0),
                [Vote::Approved, Vote::Approved],
            );
            d.pr(mine);
            d.build()
        },
        window,
        cx,
    )
}

/// First open, past 150 ms: the skeleton.
pub fn today_loading_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("today-loading", cx, |_, cx| {
        TodayView::placeholder(
            false,
            Arc::new(FixedClock::new(at(0, 9, 40))),
            Settings::default(),
            cx,
        )
    });
    div().size_full().child(view).into_any_element()
}

/// No account yet: the empty state, no card.
pub fn today_no_account_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("today-no-account", cx, |_, cx| {
        TodayView::placeholder(
            true,
            Arc::new(FixedClock::new(at(0, 9, 40))),
            Settings::default(),
            cx,
        )
    });
    div().size_full().child(view).into_any_element()
}

/// An empty org: the card as computed, one quiet line per panel.
pub fn today_empty_org_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain(
        "today-empty-org",
        || Day::new(at(0, 14, 0)).build(),
        window,
        cx,
    )
}

/// C1 on: the group shows as initials after the people.
pub fn today_group_reviewers_shown_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "today-group-reviewers-shown",
        || {
            let c = cast();
            let mut d = Day::new(at(0, 14, 0));
            let group = DEMO
                .people
                .iter()
                .find(|p| p.is_group)
                .map(|p| p.id.clone())
                .expect("the demo org has a group");
            let mut p = pr(
                5600,
                "Migrate checkout to the new pricing service",
                &c.me,
                at(-1, 9, 0),
            );
            p.reviewers.extend([
                reviewer(&c.others[2], Vote::Approved, false, Some(at(-1, 15, 0))),
                reviewer(&group, Vote::NoVote, true, None),
            ]);
            d.pr(p);
            d.build()
        },
        |v| v.with_group_reviewers(true),
        window,
        cx,
    )
}

/// A 140-character title wraps to two lines; a 400-character one stops at
/// three with an ellipsis.
pub fn today_long_titles_story(window: &mut Window, cx: &mut App) -> AnyElement {
    plain(
        "today-long-titles",
        || {
            let mut d = Day::new(at(0, 14, 0));
            let two = "Retry idempotency keys on payout timeouts when the acquiring bank \
                       answers late, and record each retry in the payout audit trail";
            let long = two.repeat(3);
            let mut a = d.mine(5101, two, at(-1, 11, 0), [Vote::Approved, Vote::NoVote]);
            a.updated = at(0, 13, 0);
            let b = d.mine(
                5102,
                &long[..400.min(long.len())],
                at(-1, 11, 0),
                [Vote::NoVote, Vote::NoVote],
            );
            d.pr(a).pr(b);
            d.build()
        },
        window,
        cx,
    )
}

// ── the stage bar component ───────────────────────────────────────────────

/// Every FR-16 state on the segmented stage bar, with its glyph and label.
pub fn stage_bar_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    use adoken_nudge::Stage;
    let theme = cx.theme().clone();
    let colors = &theme.colors;
    let states = [
        (PrState::MergedToday, Stage::Merged),
        (PrState::Conflict, Stage::Approved),
        (PrState::BuildFailed, Stage::Review),
        (PrState::ChangesRequested, Stage::Review),
        (PrState::NoReviews, Stage::Review),
        (PrState::Approved, Stage::Approved),
        (PrState::InReview, Stage::Review),
        (PrState::Draft, Stage::Draft),
    ];
    div()
        .size_full()
        .p(px(space::S24))
        .bg(colors.bg_surface)
        .text_color(colors.text_primary)
        .type_style(typography::SMALL, &theme)
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .children(states.map(|(state, stage)| {
            let (icon, tone) = adoken_app::today::state_visual(state, colors);
            div()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(StageBar::new(state.filled(stage), tone))
                .child(
                    adoken_ui::Icon::new(icon)
                        .size(adoken_theme::size::ICON_SM)
                        .color(tone),
                )
                .child(div().text_color(tone).child(state.label()))
        }))
        .into_any_element()
}
