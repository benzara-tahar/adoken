//! Spec 0017 acceptance harness: hand-built snapshots and an injected clock.
//! Thursday 2026-10-01, UTC+02:00, working hours Mon–Fri 09:00–18:00, all
//! times local, me = `me`, every nudge type on, data fresh, first sync done,
//! nothing parked (spec §6).

use adoken_model::time::{DAY, HOUR, MINUTE, SECOND};
use adoken_model::{
    Comment, MergeStatus, Millis, PrIteration, PrStatus, PrThread, PullRequest, Reviewer,
    StateCategory, Subject, ThreadStatus, Vote, WorkItem, WorkItemType,
};

use crate::summary::{self, Commits, Slot};
use crate::*;

const ME: &str = "me";
/// 2026-10-01 00:00 local (+02:00), a Thursday.
const THURSDAY: Millis = 1_789_948_800_000 + 10 * DAY - 2 * HOUR;

/// Local time on Thursday (`day` 0), the day before (-1) …
fn at(day: i64, hour: i64, minute: i64) -> Millis {
    THURSDAY + day * DAY + hour * HOUR + minute * MINUTE
}

fn settings() -> Settings {
    Settings::default().with_hours(WorkingHours::default().with_offset(120))
}

fn pr(id: i64, author: &str, created: Millis) -> PullRequest {
    PullRequest {
        id,
        repo_id: "repo".into(),
        project_id: "proj".into(),
        title: format!("Change {id}"),
        description: String::new(),
        author: author.into(),
        reviewers: Vec::new(),
        status: PrStatus::Active,
        is_draft: false,
        merge_status: MergeStatus::Succeeded,
        source_branch: "refs/heads/feature".into(),
        target_branch: "refs/heads/main".into(),
        last_source_commit: None,
        created,
        updated: created,
        closed: None,
        iterations: 1,
        last_push: Some(created),
    }
}

fn reviewer(person: &str, vote: Vote, required: bool, voted: Option<Millis>) -> Reviewer {
    Reviewer {
        person: person.into(),
        vote,
        is_required: required,
        has_declined: false,
        added_at: None,
        voted_at: voted,
    }
}

fn comment(id: i64, author: &str, created: Millis) -> Comment {
    Comment {
        id,
        parent_id: None,
        author: author.into(),
        body: "Looks reasonable".into(),
        created,
        updated: created,
        is_system: false,
        is_deleted: false,
    }
}

fn thread(id: i64, pr_id: i64, status: ThreadStatus, comments: Vec<Comment>) -> PrThread {
    PrThread {
        id,
        pr_id,
        status,
        anchor: None,
        iteration: None,
        updated: comments.iter().map(|c| c.updated).max().unwrap_or(0),
        comments,
    }
}

fn push(id: u32, created: Millis, count: Option<u32>) -> PrIteration {
    PrIteration {
        id,
        source_commit: format!("c{id}"),
        created,
        description: None,
        commit_count: count,
        commits_truncated: false,
    }
}

fn snap() -> Snapshot {
    let mut snap = Snapshot::empty(ME);
    for (id, name) in [("kim", "Kim Becker"), ("sam", "Sam"), (ME, "Me")] {
        snap.names.insert(id.into(), name.into());
    }
    snap
}

/// A review asked of me (no vote) on Kim's PR, created yesterday 10:00.
fn asked(id: i64) -> PullRequest {
    let mut p = pr(id, "kim", at(-1, 10, 0));
    p.reviewers.push(reviewer(ME, Vote::NoVote, false, None));
    p
}

/// My PR merged at `closed`.
fn merged(id: i64, closed: Millis) -> PullRequest {
    let mut p = pr(id, ME, at(-2, 10, 0));
    p.reviewers
        .push(reviewer("kim", Vote::Approved, true, Some(at(-1, 12, 0))));
    p.status = PrStatus::Completed;
    p.closed = Some(closed);
    p.updated = closed;
    p
}

/// My PR with a merge conflict, approved, quiet since yesterday.
fn conflicted(id: i64) -> PullRequest {
    let mut p = pr(id, ME, at(-1, 9, 0));
    p.reviewers
        .push(reviewer("kim", Vote::Approved, true, Some(at(-1, 11, 0))));
    p.merge_status = MergeStatus::Conflicts;
    p
}

fn day_with(s: &Snapshot, ledger: &mut Ledger, now: Millis) -> Day {
    let settings = settings();
    let pass = evaluate(s, &settings, ledger, now);
    today(s, &settings, &pass, ledger, now)
}

fn day(s: &Snapshot, now: Millis) -> Day {
    day_with(s, &mut Ledger::default(), now)
}

fn counts_row(day: &Day) -> String {
    day.summary
        .slots
        .iter()
        .map(|s| format!("{} {}", s.number(), s.label()))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn mood(day: &Day) -> DayMood {
    day.summary.mood
}

fn track(day: &Day, id: i64) -> &Track {
    day.tracks.iter().find(|t| t.pr_id == id).expect("listed")
}

// ── moods ─────────────────────────────────────────────────────────────────

#[test]
fn ac1_a_quiet_morning_is_a_fresh_start_until_something_is_done() {
    let mut s = snap();
    s.prs.push(asked(10));
    let morning = day(&s, at(0, 9, 40));
    assert_eq!(mood(&morning), DayMood::Mood(Mood::FreshStart));
    assert_eq!(morning.summary.headline(), "A fresh start");
    assert_eq!(morning.summary.sentence, "1 review waits on you");
    assert_eq!(counts_row(&morning), "0 merged · 0 reviews · 0 commits");
    assert!(morning.summary.slots.iter().all(|s| s.quiet()));
    assert_eq!(
        mood(&day(&s, at(0, 10, 59))),
        DayMood::Mood(Mood::FreshStart)
    );

    s.done_today = vec![48_000];
    let done = day(&s, at(0, 9, 40));
    assert_eq!(mood(&done), DayMood::Mood(Mood::FindingFlow));
    assert_eq!(counts_row(&done), "0 merged · 0 reviews · 1 done");
}

#[test]
fn ac2_first_sync_claims_no_mood_on_partial_data() {
    let mut s = snap();
    s.health.initial_sync_done = false;
    s.merged_today.push(merged(500, at(0, 14, 32)));
    s.prs.push(conflicted(400));
    let d = day(&s, at(0, 15, 30));
    assert_eq!(mood(&d), DayMood::FirstSync);
    assert_eq!(d.summary.mood.art(), Mood::FreshStart);
    assert_eq!(d.summary.headline(), "Getting your day ready");
    assert_eq!(
        d.summary.sentence,
        "First sync in progress · counts appear when it finishes"
    );
    assert_eq!(counts_row(&d), "– merged · – reviews · – commits");
}

#[test]
fn ac3_commits_pushed_today_find_the_flow() {
    let mut s = snap();
    let mut mine = pr(100, ME, at(-1, 15, 0));
    mine.iterations = 3;
    mine.last_push = Some(at(0, 10, 20));
    s.prs.push(mine);
    s.iterations_today.insert(
        100,
        vec![
            push(2, at(0, 9, 10), Some(3)),
            push(3, at(0, 10, 20), Some(2)),
        ],
    );
    let mut theirs = pr(200, "kim", at(-2, 9, 0));
    theirs
        .reviewers
        .push(reviewer(ME, Vote::Approved, false, Some(at(-1, 16, 0))));
    s.prs.push(theirs);
    let d = day(&s, at(0, 11, 0));
    assert_eq!(mood(&d), DayMood::Mood(Mood::FindingFlow));
    assert_eq!(d.summary.sentence, "5 commits pushed today");
    assert_eq!(counts_row(&d), "0 merged · 0 reviews · 5 commits");
    assert_eq!(track(&d, 100).line, "5 commits today");
}

#[test]
fn ac4_reviews_and_discussions_grow_and_ties_go_to_flow() {
    let mut s = snap();
    let mut a = pr(200, "kim", at(-2, 9, 0));
    a.reviewers
        .push(reviewer(ME, Vote::Approved, false, Some(at(0, 10, 0))));
    let mut b = pr(201, "sam", at(-2, 9, 0));
    b.reviewers.push(reviewer(
        ME,
        Vote::WaitingForAuthor,
        false,
        Some(at(0, 10, 30)),
    ));
    let mut mine = pr(100, ME, at(-1, 15, 0));
    mine.last_push = Some(at(0, 9, 30));
    s.prs.extend([a, b, mine]);
    s.iterations_today
        .insert(100, vec![push(2, at(0, 9, 30), Some(1))]);
    let mut grow = s.clone();
    grow.threads.insert(
        201,
        vec![thread(
            1,
            201,
            ThreadStatus::Active,
            vec![comment(1, ME, at(0, 10, 31))],
        )],
    );
    let d = day(&grow, at(0, 11, 0));
    let c = d.summary.counts;
    assert_eq!((c.reviews, c.discussions, c.commits), (2, 1, 1));
    assert_eq!(mood(&d), DayMood::Mood(Mood::HelpingGrow));
    assert_eq!(d.summary.sentence, "2 reviews done today");

    // One vote and one commit: a tie goes to flow.
    s.prs.retain(|p| p.id != 201);
    assert_eq!(
        mood(&day(&s, at(0, 11, 0))),
        DayMood::Mood(Mood::FindingFlow)
    );
}

#[test]
fn ac5_waiting_on_reviewers_is_neutral() {
    let mut s = snap();
    let mut mine = pr(300, ME, at(-1, 10, 0));
    mine.reviewers
        .push(reviewer("kim", Vote::NoVote, false, None));
    s.prs.push(mine);
    let d = day(&s, at(0, 14, 0));
    assert_eq!(mood(&d), DayMood::Mood(Mood::Waiting));
    assert_eq!(d.summary.headline(), "Waiting to bloom");
    assert_eq!(d.summary.sentence, "1 PR waiting on reviewers");
    assert_eq!(track(&d, 300).state, PrState::InReview, "N4 not due yet");
}

#[test]
fn ac6_a_conflict_blocks_until_it_is_snoozed() {
    let mut s = snap();
    s.prs.push(conflicted(400));
    let d = day(&s, at(0, 14, 0));
    assert_eq!(mood(&d), DayMood::Mood(Mood::Blocked));
    assert_eq!(d.summary.sentence, "1 conflict to resolve");

    let mut ledger = Ledger::default();
    ledger.snooze(&Subject::Pr(400), at(1, 9, 0));
    let snoozed = day_with(&s, &mut ledger, at(0, 14, 0));
    assert_eq!(mood(&snoozed), DayMood::Mood(Mood::Clear));
    assert_eq!(track(&snoozed, 400).quiet, Some(Quiet::Snoozed));
}

#[test]
fn ac7_a_merge_today_ships_and_yesterdays_is_gone() {
    let mut s = snap();
    s.merged_today
        .extend([merged(500, at(0, 14, 32)), merged(501, at(-1, 23, 50))]);
    let d = day(&s, at(0, 15, 0));
    assert_eq!(mood(&d), DayMood::Mood(Mood::Shipped));
    assert_eq!(d.summary.sentence, "1 PR merged today");
    let row = track(&d, 500);
    assert_eq!(row.state, PrState::MergedToday);
    assert_eq!(row.state.filled(row.stage), 4);
    assert_eq!(row.line, "Merged today at 14:32");
    assert!(d.tracks.iter().all(|t| t.pr_id != 501));
    assert_eq!(d.summary.counts.merged, 1);
}

#[test]
fn ac8_a_blocker_never_erases_an_accomplishment() {
    let mut s = snap();
    s.merged_today.push(merged(500, at(0, 14, 32)));
    s.prs.push(conflicted(400));
    let d = day(&s, at(0, 15, 0));
    assert_eq!(mood(&d), DayMood::Mood(Mood::Shipped));
    assert_eq!(
        d.summary.sentence,
        "1 PR merged today · 1 conflict to resolve"
    );
}

#[test]
fn ac9_clear_and_stale() {
    let s = snap();
    let now = at(0, 14, 0);
    let d = day(&s, now);
    assert_eq!(mood(&d), DayMood::Mood(Mood::Clear));
    assert_eq!(d.summary.sentence, "Nothing needs your attention right now");

    let lanes = SyncHealth {
        my_prs: LaneHealth {
            last_ok: Some(at(0, 13, 59)),
            interval: Some(MINUTE),
        },
        work_items: LaneHealth {
            last_ok: Some(at(0, 13, 20)),
            interval: Some(5 * MINUTE),
        },
        ..SyncHealth::default()
    };
    let mut stale = s.clone();
    stale.health = lanes.data_health(true, now);
    let d = day(&stale, now);
    assert_eq!(mood(&d), DayMood::Stale);
    assert_eq!(d.summary.mood.art(), Mood::Waiting);
    assert_eq!(d.summary.headline(), "Can't see the latest");
    assert_eq!(d.summary.sentence, "Sync delayed · counts are from 13:20");

    let mut offline = s.clone();
    offline.health = SyncHealth {
        online: false,
        ..lanes
    }
    .data_health(true, now);
    assert_eq!(
        day(&offline, now).summary.sentence,
        "Offline · counts are from 13:20"
    );
}

#[test]
fn fr10_lanes_go_stale_after_their_budget_unless_manual_or_paused() {
    let now = at(0, 14, 0);
    let lane = |last: Millis, interval| LaneHealth {
        last_ok: Some(last),
        interval,
    };
    assert!(!lane(now - 15 * MINUTE, Some(MINUTE)).is_late(now));
    assert!(lane(now - 16 * MINUTE, Some(MINUTE)).is_late(now));
    assert!(
        !lane(now - 50 * MINUTE, Some(30 * MINUTE)).is_late(now),
        "2 × the interval"
    );
    assert!(!lane(now - 5 * HOUR, None).is_late(now), "manual or paused");
    assert_eq!(SyncHealth::default().stale(now), None);
    let rejected = SyncHealth {
        token_rejected: true,
        ..SyncHealth::default()
    };
    assert_eq!(
        rejected.stale(now).map(|s| s.reason),
        Some(StaleReason::Delayed)
    );
    assert!(rejected.data_health(true, now).warm_stale);
}

#[test]
fn ac10_nothing_moved_only_after_half_a_workday_with_work_owed() {
    let mut s = snap();
    s.prs.extend([asked(10), asked(11)]);
    let d = day(&s, at(0, 13, 31));
    assert_eq!(mood(&d), DayMood::Mood(Mood::Stalled));
    assert_eq!(
        d.summary.sentence,
        "No reviews, commits or merges yet today · 2 reviews wait on you"
    );
    assert_eq!(
        mood(&day(&s, at(0, 13, 29))),
        DayMood::Mood(Mood::FreshStart)
    );
    assert_eq!(mood(&day(&s, at(0, 20, 0))), DayMood::Mood(Mood::Stalled));
    assert_eq!(
        mood(&day(&s, at(2, 15, 0))),
        DayMood::Mood(Mood::FreshStart),
        "Saturday is never stalled"
    );
}

#[test]
fn ac11_today_is_local_midnight_to_now() {
    let mut s = snap();
    // 2026-09-30 23:30 UTC is 01:30 local on Thursday.
    s.merged_today.push(merged(500, at(0, 1, 30)));
    let mut theirs = pr(200, "kim", at(-2, 9, 0));
    theirs
        .reviewers
        .push(reviewer(ME, Vote::Approved, false, Some(at(-1, 23, 55))));
    s.prs.push(theirs);
    let c = day(&s, at(0, 9, 30)).summary.counts;
    assert_eq!((c.merged, c.reviews), (1, 0));

    // The midnight timer fires on time and yesterday's counts are gone.
    let hours = settings().hours;
    let late = at(0, 23, 59) + 59 * SECOND;
    assert_eq!(summary::next_change(&hours, late), at(1, 0, 0));
    let tomorrow = day(&s, at(1, 0, 0) + SECOND).summary.counts;
    assert_eq!(tomorrow.activity(), 0);
}

#[test]
fn fr2_the_summary_recomputes_at_the_fresh_window_half_day_and_midnight() {
    let hours = settings().hours;
    assert_eq!(summary::next_change(&hours, at(0, 7, 0)), at(0, 11, 0));
    assert_eq!(summary::next_change(&hours, at(0, 11, 0)), at(0, 13, 30));
    assert_eq!(summary::next_change(&hours, at(0, 13, 30)), at(1, 0, 0));
    assert_eq!(
        summary::next_change(&hours, at(2, 10, 0)),
        at(3, 0, 0),
        "weekends only roll over"
    );
}

// ── counts and sources ────────────────────────────────────────────────────

#[test]
fn ac12_count_rules() {
    let now = at(0, 16, 0);
    let mut s = snap();
    // An abandoned PR of mine today is not merged.
    let mut abandoned = merged(600, at(0, 10, 0));
    abandoned.status = PrStatus::Abandoned;
    s.merged_today.push(abandoned);
    // My vote on a PR listed twice (active and merged today) counts once.
    let mut reviewed = pr(601, "kim", at(-2, 9, 0));
    reviewed
        .reviewers
        .push(reviewer(ME, Vote::Approved, false, Some(at(0, 9, 0))));
    s.prs.push(reviewed.clone());
    reviewed.status = PrStatus::Completed;
    reviewed.closed = Some(at(0, 12, 0));
    s.merged_today.push(reviewed);
    // My PR: my own comment counts; system and deleted comments don't; a
    // thread resolved today counts without a comment of mine.
    let mine = pr(602, ME, at(-1, 9, 0));
    let mut system = comment(2, "kim", at(0, 10, 0));
    system.is_system = true;
    let mut deleted = comment(3, ME, at(0, 10, 0));
    deleted.is_deleted = true;
    let mut resolved = thread(
        4,
        602,
        ThreadStatus::Fixed,
        vec![comment(4, "kim", at(-1, 12, 0))],
    );
    resolved.updated = at(0, 11, 0);
    s.threads.insert(
        602,
        vec![
            thread(
                1,
                602,
                ThreadStatus::Active,
                vec![comment(1, ME, at(0, 9, 0))],
            ),
            thread(2, 602, ThreadStatus::Active, vec![system]),
            thread(3, 602, ThreadStatus::Active, vec![deleted]),
            resolved,
        ],
    );
    // Others' PR: a thread resolved today counts only if I commented in it.
    let theirs = pr(603, "sam", at(-1, 9, 0));
    let mut joined = thread(
        5,
        603,
        ThreadStatus::Closed,
        vec![comment(5, ME, at(-1, 12, 0))],
    );
    joined.updated = at(0, 11, 0);
    let mut not_mine = thread(
        6,
        603,
        ThreadStatus::Closed,
        vec![comment(6, "kim", at(-1, 12, 0))],
    );
    not_mine.updated = at(0, 11, 0);
    s.threads.insert(603, vec![joined, not_mine]);
    // Iteration 1 counts only on a PR created today.
    let mut yesterday = pr(604, ME, at(-1, 22, 0));
    yesterday.last_push = Some(at(0, 8, 0));
    let mut fresh = pr(605, ME, at(0, 8, 0));
    fresh.last_push = Some(at(0, 8, 0));
    s.iterations_today
        .insert(604, vec![push(1, at(0, 8, 0), Some(4))]);
    s.iterations_today
        .insert(605, vec![push(1, at(0, 8, 0), Some(2))]);
    s.prs.extend([mine, theirs, yesterday, fresh]);

    let c = summary::counts(&s, at(0, 0, 0), now);
    assert_eq!(c.merged, 0, "abandoned never counts");
    assert_eq!(c.reviews, 1, "one per PR");
    assert_eq!(
        c.discussions, 3,
        "mine, resolved on my PR, resolved where I commented"
    );
    assert_eq!(c.commits, 2, "iteration 1 only on the PR opened today");
    assert_eq!(c.opened, 1);
}

#[test]
fn ac13_commit_counts_never_guess() {
    let mut s = snap();
    let mut p = pr(100, ME, at(-1, 9, 0));
    p.last_push = Some(at(0, 11, 0));
    s.iterations_today.insert(
        100,
        vec![push(2, at(0, 10, 0), Some(2)), push(3, at(0, 11, 0), None)],
    );
    let commits = |s: &Snapshot| summary::commits_today(s, &p, at(0, 0, 0), at(0, 12, 0));
    assert_eq!(commits(&s).text().as_deref(), Some("3+ commits today"));

    let mut truncated = s.clone();
    truncated.iterations_today.insert(100, {
        let mut it = push(2, at(0, 10, 0), Some(100));
        it.commits_truncated = true;
        vec![it]
    });
    assert_eq!(
        commits(&truncated).text().as_deref(),
        Some("100+ commits today")
    );

    let mut unknown = s.clone();
    unknown
        .iterations_today
        .insert(100, vec![push(2, at(0, 10, 0), None)]);
    assert_eq!(commits(&unknown).text().as_deref(), Some("1 push today"));

    let mut behind = s.clone();
    behind.iterations_behind.insert(100);
    assert_eq!(commits(&behind), Commits::New { known: 3 });
    assert_eq!(
        commits(&behind).text().as_deref(),
        Some("New commits today")
    );
    // The list caught up: the number again.
    assert_eq!(commits(&s).text().as_deref(), Some("3+ commits today"));
}

// ── your PRs ──────────────────────────────────────────────────────────────

#[test]
fn ac16_rows_are_most_recently_active_first() {
    let mut s = snap();
    let mut one = pr(1, ME, at(-1, 9, 0));
    one.updated = at(0, 9, 0);
    s.threads.insert(
        1,
        vec![thread(
            1,
            1,
            ThreadStatus::Active,
            vec![comment(1, "kim", at(0, 11, 0))],
        )],
    );
    let mut two = pr(2, ME, at(-1, 9, 0));
    two.updated = at(0, 10, 30);
    let mut four = conflicted(4);
    four.updated = at(0, 8, 0);
    four.last_push = Some(at(-1, 9, 0));
    s.prs.extend([one, two, four]);
    s.merged_today.push(merged(3, at(0, 10, 45)));
    let d = day(&s, at(0, 12, 0));
    let order: Vec<i64> = d.tracks.iter().map(|t| t.pr_id).collect();
    assert_eq!(order, [1, 3, 2, 4]);
    assert_eq!(track(&d, 1).activity.last, at(0, 11, 0));
}

#[test]
fn ac18_stage_bar_fill_and_label_per_state() {
    use Stage::*;
    let cases = [
        (Merged, None, PrState::MergedToday, 4, "Merged today"),
        (
            Approved,
            Some(Blocker::Conflict),
            PrState::Conflict,
            3,
            "Merge conflict",
        ),
        (
            Approved,
            Some(Blocker::BuildFailed),
            PrState::BuildFailed,
            3,
            "Build failed",
        ),
        (
            Review,
            Some(Blocker::BuildFailed),
            PrState::BuildFailed,
            2,
            "Build failed",
        ),
        (
            Review,
            Some(Blocker::ChangesRequested),
            PrState::ChangesRequested,
            2,
            "Changes requested",
        ),
        (
            Review,
            Some(Blocker::WaitingOnReviewers),
            PrState::NoReviews,
            2,
            "No reviews",
        ),
        (Approved, None, PrState::Approved, 3, "Approved"),
        (Review, None, PrState::InReview, 2, "In review"),
        (Draft, None, PrState::Draft, 1, "Draft"),
    ];
    for (stage, blocker, state, filled, label) in cases {
        let got = PrState::of(stage, blocker);
        assert_eq!(got, state, "{stage:?} {blocker:?}");
        assert_eq!(got.filled(stage), filled, "{state:?}");
        assert_eq!(got.label(), label);
    }
}

#[test]
fn ac19_activity_lines() {
    let now = at(0, 14, 32);
    let line = |s: &Snapshot, id: i64, now: Millis| track(&day(s, now), id).line.clone();

    let mut s = snap();
    let mut opened = pr(1, ME, at(0, 9, 0));
    opened.last_push = Some(at(0, 9, 0));
    s.iterations_today
        .insert(1, vec![push(1, at(0, 9, 0), Some(1))]);
    s.prs.push(opened);
    assert_eq!(line(&s, 1, now), "Opened today · 1 commit today");

    let mut s = snap();
    let mut busy = pr(2, ME, at(-1, 9, 0));
    busy.last_push = Some(at(0, 10, 0));
    s.iterations_today
        .insert(2, vec![push(2, at(0, 10, 0), Some(2))]);
    s.threads.insert(
        2,
        vec![thread(
            1,
            2,
            ThreadStatus::Active,
            vec![
                comment(1, "kim", at(0, 11, 0)),
                comment(2, ME, at(0, 11, 5)),
            ],
        )],
    );
    s.prs.push(busy);
    assert_eq!(line(&s, 2, now), "2 commits today · 1 new comment");

    let mut s = snap();
    let mut conflict = conflicted(3);
    conflict.last_push = Some(now - 5 * HOUR);
    s.prs.push(conflict);
    assert_eq!(line(&s, 3, now), "Conflict for 5h");

    let mut s = snap();
    let mut changes = pr(4, ME, at(-3, 9, 0));
    changes.reviewers.push(reviewer(
        "kim",
        Vote::WaitingForAuthor,
        true,
        Some(now - DAY),
    ));
    s.prs.push(changes);
    assert_eq!(line(&s, 4, now), "Changes requested by Kim · 1d");

    let mut s = snap();
    let mut ignored = pr(5, ME, at(-2, 9, 0));
    ignored.reviewers.extend([
        reviewer("sam", Vote::NoVote, false, None),
        reviewer("kim", Vote::NoVote, true, None),
    ]);
    s.prs.push(ignored);
    let late = at(0, 17, 30);
    assert_eq!(
        line(&s, 5, late),
        "Waiting on Kim Becker (required), Sam · 2d"
    );
    assert_eq!(track(&day(&s, late), 5).state_label, "No reviews · 2d");

    let mut s = snap();
    let mut approved = pr(6, ME, at(-1, 9, 0));
    approved
        .reviewers
        .push(reviewer("kim", Vote::Approved, true, Some(at(-1, 12, 0))));
    s.prs.push(approved);
    assert_eq!(line(&s, 6, now), "All reviewers approved");

    let mut s = snap();
    let mut idle = pr(7, ME, at(-1, 9, 0));
    idle.reviewers
        .push(reviewer("kim", Vote::NoVote, false, None));
    s.prs.push(idle);
    assert_eq!(line(&s, 7, now), "No activity today");

    let mut s = snap();
    s.merged_today.push(merged(8, at(0, 14, 32)));
    assert_eq!(line(&s, 8, at(0, 15, 0)), "Merged today at 14:32");
}

#[test]
fn ac20_group_reviewers_count_in_the_stage_and_are_marked() {
    let mut s = snap();
    s.names.insert("grp".into(), "React PR reviewers".into());
    s.groups.insert("grp".into());
    let mut p = pr(600, ME, at(-1, 9, 0));
    p.reviewers.extend([
        reviewer("kim", Vote::Approved, false, Some(at(-1, 12, 0))),
        reviewer("grp", Vote::NoVote, true, None),
    ]);
    s.prs.push(p);
    let d = day(&s, at(0, 12, 0));
    let row = track(&d, 600);
    assert_eq!(row.stage, Stage::Review, "the group still blocks Approved");
    let flags: Vec<(&str, bool)> = row
        .reviewers
        .iter()
        .map(|r| (r.person.as_str(), r.is_group))
        .collect();
    assert_eq!(flags, [("grp", true), ("kim", false)], "required first");
}

#[test]
fn loose_ends_measure_time_in_their_state() {
    let mut s = snap();
    let item = WorkItem {
        id: 48_213,
        project_id: "proj".into(),
        rev: 1,
        kind: WorkItemType::Task,
        title: "Fix SSO redirect".into(),
        state: "Active".into(),
        state_category: StateCategory::InProgress,
        assigned_to: Some(ME.into()),
        created_by: ME.into(),
        changed_by: ME.into(),
        area_path: "proj".into(),
        iteration_path: "proj".into(),
        priority: None,
        tags: Vec::new(),
        parent_id: None,
        linked_pr_ids: Vec::new(),
        description: None,
        comment_count: 0,
        created: at(-30, 9, 0),
        changed: at(-10, 9, 0),
    };
    s.work_items.push(item);
    let now = at(0, 12, 0);
    assert_eq!(
        day(&s, now).loose[0].open_since,
        at(-10, 9, 0),
        "else changed"
    );
    s.state_since.insert(48_213, at(-12, 9, 0));
    assert_eq!(day(&s, now).loose[0].open_since, at(-12, 9, 0));
    s.health.work_scope = false;
    assert!(day(&s, now).loose.is_empty(), "no vso.work, no loose ends");
}

#[test]
fn slots_hide_done_without_the_work_scope_and_keep_their_order() {
    let mut health = DataHealth::FRESH;
    let counts = Counts {
        discussions: 2,
        commits: 3,
        commits_lower_bound: true,
        done: 1,
        ..Counts::default()
    };
    let shown = |h: &DataHealth| {
        summary::slots(&counts, h)
            .iter()
            .map(|s| (s.slot, s.number()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shown(&health),
        [
            (Slot::Discussions, "2".into()),
            (Slot::Commits, "3+".into()),
            (Slot::Done, "1".into())
        ]
    );
    health.work_scope = false;
    assert_eq!(
        shown(&health),
        [
            (Slot::Merged, "0".into()),
            (Slot::Discussions, "2".into()),
            (Slot::Commits, "3+".into())
        ]
    );
}

#[test]
fn the_summary_reads_as_one_label() {
    let mut s = snap();
    s.merged_today.push(merged(500, at(0, 14, 32)));
    s.prs.push(conflicted(400));
    let d = day(&s, at(0, 15, 0));
    assert_eq!(
        d.summary.spoken(),
        "Something shipped. 1 PR merged today, 1 conflict to resolve. 1 merged, 0 reviews, 0 commits."
    );
}

#[test]
fn every_mood_and_variant_has_art_and_a_headline() {
    let ids: std::collections::HashSet<&str> = Mood::ALL.iter().map(|m| m.id()).collect();
    assert_eq!(ids.len(), 8);
    for mood in Mood::ALL {
        assert_eq!(DayMood::Mood(mood).art(), mood);
        assert!(!mood.headline().is_empty());
    }
    assert_eq!(DayMood::FirstSync.art(), Mood::FreshStart);
    assert_eq!(DayMood::Stale.art(), Mood::Waiting);
}

#[test]
fn fr13_header_date_and_times_are_local() {
    let hours = settings().hours;
    assert_eq!(copy::long_date(at(0, 0, 0), &hours), "Thursday, October 1");
    assert_eq!(
        copy::long_date(at(0, 23, 59), &hours),
        "Thursday, October 1"
    );
    assert_eq!(
        copy::long_date(at(-1, 23, 59), &hours),
        "Wednesday, September 30"
    );
    assert_eq!(copy::long_date(at(92, 12, 0), &hours), "Friday, January 1");
    assert_eq!(
        copy::long_date(at(516, 12, 0), &hours),
        "Tuesday, February 29"
    );
    assert_eq!(copy::clock(at(0, 14, 32), &hours), "14:32");
}
