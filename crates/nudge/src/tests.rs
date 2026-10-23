//! Spec 0006 acceptance harness: store-shaped fixtures + injected clock +
//! fake notification sink.

use adoken_model::time::{DAY, HOUR, MINUTE};
use adoken_model::{
    Comment, MergeStatus, Millis, PipelineRun, PrStatus, PrThread, PullRequest, Reviewer,
    RunReason, RunResult, RunStatus, StateCategory, Subject, Vote, WorkItem, WorkItemType,
};

use crate::*;

/// 2026-09-21 00:00 UTC, a Monday.
const MONDAY: Millis = 1_789_948_800_000;
const ME: &str = "me";

fn at(day: i64, hour: i64) -> Millis {
    MONDAY + day * DAY + hour * HOUR
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

fn reviewer(person: &str, vote: Vote, required: bool) -> Reviewer {
    Reviewer {
        person: person.into(),
        vote,
        is_required: required,
        has_declined: false,
        added_at: None,
        voted_at: None,
    }
}

fn comment(author: &str, created: Millis) -> Comment {
    Comment {
        id: created,
        parent_id: None,
        author: author.into(),
        body: "Looks reasonable".into(),
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
        status: adoken_model::ThreadStatus::Active,
        anchor: None,
        iteration: None,
        updated: comments.iter().map(|c| c.updated).max().unwrap_or(0),
        comments,
    }
}

fn run(id: i64, pr_id: i64, result: RunResult, finish: Millis) -> PipelineRun {
    PipelineRun {
        id,
        project_id: "proj".into(),
        pipeline_id: 1,
        pipeline_name: "ci".into(),
        run_number: format!("20260921.{id}"),
        status: RunStatus::Completed,
        result: Some(result),
        reason: RunReason::PullRequest,
        branch: "refs/pull/1/merge".into(),
        source_version: None,
        requested_for: Some(ME.into()),
        pr_id: Some(pr_id),
        queued: finish - 10 * MINUTE,
        started: Some(finish - 9 * MINUTE),
        finish: Some(finish),
    }
}

fn wi(id: i64, state: &str, category: StateCategory, changed: Millis) -> WorkItem {
    WorkItem {
        id,
        project_id: "proj".into(),
        rev: 1,
        kind: WorkItemType::Task,
        title: format!("Item {id}"),
        state: state.into(),
        state_category: category,
        assigned_to: Some(ME.into()),
        created_by: ME.into(),
        changed_by: ME.into(),
        area_path: "proj".into(),
        iteration_path: "proj".into(),
        priority: Some(2),
        tags: Vec::new(),
        parent_id: None,
        linked_pr_ids: Vec::new(),
        description: None,
        comment_count: 0,
        created: changed,
        changed,
    }
}

fn snap() -> Snapshot {
    let mut snap = Snapshot::empty(ME);
    for (id, name) in [("kim", "Kim"), ("sam", "Sam"), ("lee", "Lee"), (ME, "Me")] {
        snap.names.insert(id.into(), name.into());
    }
    snap
}

fn kinds(nudges: &[Nudge]) -> Vec<(NudgeKind, Subject)> {
    nudges.iter().map(|n| (n.kind, n.subject.clone())).collect()
}

fn fire(snap: &Snapshot, ledger: &mut Ledger, now: Millis) -> Vec<(NudgeKind, Subject)> {
    kinds(&evaluate(snap, &Settings::default(), ledger, now).fired)
}

/// A PR by Kim with me as a reviewer added Monday 10:00.
fn review_request() -> Snapshot {
    let mut s = snap();
    let mut p = pr(912, "kim", at(0, 9));
    let mut r = reviewer(ME, Vote::NoVote, false);
    r.added_at = Some(at(0, 10));
    p.reviewers.push(r);
    s.prs.push(p);
    s
}

// ── N1 ──────────────────────────────────────────────────────────────────

#[test]
fn ac1_n1_fires_after_24_working_hours_since_i_was_added() {
    let s = review_request();
    let mut ledger = Ledger::default();
    assert!(fire(&s, &mut ledger, at(1, 10)).is_empty(), "Tue 10:00");
    assert!(
        fire(&s, &mut ledger, at(2, 16) - MINUTE).is_empty(),
        "Wed 15:59"
    );
    assert_eq!(
        fire(&s, &mut ledger, at(2, 16)),
        [(NudgeKind::ReviewWaiting, Subject::Pr(912))]
    );
}

#[test]
fn n1_falls_back_to_creation_and_skips_drafts_declines_and_my_own_prs() {
    let mut s = review_request();
    s.prs[0].reviewers[0].added_at = None; // created Mon 09:00 → 24 wh = Wed 15:00
    let mut ledger = Ledger::default();
    let pass = evaluate(&s, &Settings::default(), &mut ledger, at(2, 15));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::ReviewWaiting, Subject::Pr(912))]
    );
    assert_eq!(
        pass.fired[0].message,
        "Review requested 24 working hours ago: !912 Change 912"
    );
    assert_eq!(pass.fired[0].action.label(), "Review now");

    for tweak in [
        |p: &mut PullRequest| p.is_draft = true,
        |p: &mut PullRequest| p.reviewers[0].has_declined = true,
        |p: &mut PullRequest| p.author = ME.into(),
        |p: &mut PullRequest| p.status = PrStatus::Completed,
    ] {
        let mut s = s.clone();
        tweak(&mut s.prs[0]);
        let fired = fire(&s, &mut Ledger::default(), at(4, 12));
        assert!(
            !fired.iter().any(|(k, _)| *k == NudgeKind::ReviewWaiting),
            "{fired:?}"
        );
    }
}

#[test]
fn ac2_voting_before_the_threshold_prevents_and_after_it_resolves() {
    let mut voted = review_request();
    voted.prs[0].reviewers[0].vote = Vote::Approved;
    assert!(fire(&voted, &mut Ledger::default(), at(3, 12)).is_empty());

    let s = review_request();
    let mut ledger = Ledger::default();
    assert_eq!(fire(&s, &mut ledger, at(2, 16)).len(), 1);
    let pass = evaluate(&voted, &Settings::default(), &mut ledger, at(2, 17));
    assert_eq!(
        pass.resolved,
        [(Subject::Pr(912), NudgeKind::ReviewWaiting)]
    );
    assert!(pass.fired.is_empty());
}

// ── N2 ──────────────────────────────────────────────────────────────────

fn blocked_pr() -> Snapshot {
    let mut s = snap();
    let mut p = pr(870, ME, at(0, 9));
    let mut kim = reviewer("kim", Vote::WaitingForAuthor, false);
    kim.voted_at = Some(at(0, 11));
    p.reviewers.push(kim);
    s.prs.push(p);
    s
}

#[test]
fn n2_fires_four_working_hours_after_a_wait_for_author_vote() {
    let s = blocked_pr();
    let mut ledger = Ledger::default();
    assert!(fire(&s, &mut ledger, at(0, 15) - MINUTE).is_empty());
    let pass = evaluate(&s, &Settings::default(), &mut ledger, at(1, 10));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::WaitingForAuthor, Subject::Pr(870))]
    );
    assert_eq!(
        pass.fired[0].message,
        "!870 has a wait-for-author vote from Kim since yesterday"
    );
}

#[test]
fn n2_is_answered_by_a_push_or_my_reply_after_the_vote() {
    let mut pushed = blocked_pr();
    pushed.prs[0].last_push = Some(at(0, 12));
    assert!(fire(&pushed, &mut Ledger::default(), at(2, 12)).is_empty());

    let mut replied = blocked_pr();
    replied
        .threads
        .insert(870, vec![thread(870, vec![comment(ME, at(0, 12))])]);
    assert!(fire(&replied, &mut Ledger::default(), at(2, 12)).is_empty());

    // A reply *before* the vote doesn't count.
    let mut early = blocked_pr();
    early
        .threads
        .insert(870, vec![thread(870, vec![comment(ME, at(0, 10))])]);
    assert_eq!(fire(&early, &mut Ledger::default(), at(2, 12)).len(), 1);
}

// ── N3 ──────────────────────────────────────────────────────────────────

fn failing_pr() -> Snapshot {
    let mut s = snap();
    let mut p = pr(871, ME, at(0, 9));
    p.reviewers.push(reviewer("kim", Vote::Approved, false));
    s.prs.push(p);
    s.runs
        .push(run(1, 871, RunResult::Succeeded, at(0, 9) + 20 * MINUTE));
    s.runs.push(run(2, 871, RunResult::Failed, at(0, 10)));
    s
}

#[test]
fn n3_fires_two_working_hours_after_the_latest_build_failed() {
    let s = failing_pr();
    assert!(fire(&s, &mut Ledger::default(), at(0, 12) - MINUTE).is_empty());
    let pass = evaluate(&s, &Settings::default(), &mut Ledger::default(), at(0, 12));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::BuildFailing, Subject::Pr(871))]
    );
    assert_eq!(
        pass.fired[0].message,
        "!871 build failed 2 h ago and nobody has pushed"
    );
    assert_eq!(pass.fired[0].action, NudgeAction::OpenFailingRun(2));
}

#[test]
fn n3_ignores_older_failures_and_failures_followed_by_a_push() {
    let mut green = failing_pr();
    green
        .runs
        .push(run(3, 871, RunResult::Succeeded, at(0, 11)));
    assert!(fire(&green, &mut Ledger::default(), at(1, 12)).is_empty());

    let mut pushed = failing_pr();
    pushed.prs[0].last_push = Some(at(0, 10) + MINUTE);
    assert!(fire(&pushed, &mut Ledger::default(), at(1, 12)).is_empty());
}

// ── N4 ──────────────────────────────────────────────────────────────────

fn ignored_pr() -> Snapshot {
    let mut s = snap();
    let mut p = pr(872, ME, at(0, 9));
    p.reviewers.push(reviewer("sam", Vote::NoVote, false));
    p.reviewers.push(reviewer("kim", Vote::NoVote, true));
    s.prs.push(p);
    s
}

#[test]
fn ac11_n4_names_reviewers_to_ping_required_first() {
    let s = ignored_pr();
    assert!(fire(&s, &mut Ledger::default(), at(2, 15) - MINUTE).is_empty());
    let pass = evaluate(&s, &Settings::default(), &mut Ledger::default(), at(2, 15));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::NoReviewerActivity, Subject::Pr(872))]
    );
    let nudge = &pass.fired[0];
    assert_eq!(
        nudge.detail.as_deref(),
        Some("Waiting on Kim (required), Sam")
    );
    assert_eq!(nudge.ping, ["Kim", "Sam"]);
}

#[test]
fn n4_is_silenced_by_any_vote_or_human_comment_from_others() {
    let mut voted = ignored_pr();
    voted.prs[0].reviewers[0].vote = Vote::ApprovedWithSuggestions;
    assert!(fire(&voted, &mut Ledger::default(), at(4, 12)).is_empty());

    let mut commented = ignored_pr();
    commented
        .threads
        .insert(872, vec![thread(872, vec![comment("lee", at(0, 12))])]);
    assert!(fire(&commented, &mut Ledger::default(), at(4, 12)).is_empty());

    // My own and system comments are not reviewer activity.
    let mut system = comment("kim", at(0, 12));
    system.is_system = true;
    let mut quiet = ignored_pr();
    quiet
        .threads
        .insert(872, vec![thread(872, vec![system, comment(ME, at(0, 13))])]);
    assert_eq!(fire(&quiet, &mut Ledger::default(), at(4, 12)).len(), 1);
}

// ── N5 ──────────────────────────────────────────────────────────────────

fn conflicted_pr() -> Snapshot {
    let mut s = snap();
    let mut p = pr(873, ME, at(0, 9));
    p.reviewers.push(reviewer("kim", Vote::Approved, false));
    p.merge_status = MergeStatus::Conflicts;
    s.prs.push(p);
    s
}

#[test]
fn n5_fires_immediately_and_only_once_per_episode() {
    let s = conflicted_pr();
    let mut ledger = Ledger::default();
    assert_eq!(
        fire(&s, &mut ledger, at(0, 9) + MINUTE),
        [(NudgeKind::MergeConflicts, Subject::Pr(873))]
    );
    // Still conflicted two days later: open, not re-sent.
    assert!(fire(&s, &mut ledger, at(2, 12)).is_empty());

    let mut clean = s.clone();
    clean.prs[0].merge_status = MergeStatus::Succeeded;
    let pass = evaluate(&clean, &Settings::default(), &mut ledger, at(2, 13));
    assert_eq!(
        pass.resolved,
        [(Subject::Pr(873), NudgeKind::MergeConflicts)]
    );

    // A new conflict episode fires again.
    assert_eq!(fire(&s, &mut ledger, at(3, 10)).len(), 1);
}

// ── N6 / N7 ─────────────────────────────────────────────────────────────

fn merged_item() -> Snapshot {
    let mut s = snap();
    let mut p = pr(901, ME, at(0, 9));
    p.status = PrStatus::Completed;
    p.closed = Some(at(0, 10));
    s.prs.push(p);
    let mut item = wi(48213, "Doing", StateCategory::InProgress, at(0, 9));
    item.linked_pr_ids.push(901);
    s.work_items.push(item);
    s
}

#[test]
fn n6_fires_a_day_after_the_linked_pr_merged() {
    let s = merged_item();
    // Merged Mon 10:00 → 24 working hours = Wed 16:00.
    assert!(fire(&s, &mut Ledger::default(), at(2, 16) - MINUTE).is_empty());
    let pass = evaluate(&s, &Settings::default(), &mut Ledger::default(), at(2, 16));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::MergedStillActive, Subject::WorkItem(48213))]
    );
    assert_eq!(
        pass.fired[0].message,
        "#48213 is Doing but its PR !901 merged 2 days ago"
    );
    assert_eq!(pass.fired[0].action.label(), "Open in ADO");
}

#[test]
fn n6_waits_while_another_linked_pr_is_active_or_the_item_is_done() {
    let mut open = merged_item();
    open.prs.push(pr(902, ME, at(0, 9)));
    open.work_items[0].linked_pr_ids.push(902);
    assert!(
        !fire(&open, &mut Ledger::default(), at(3, 10))
            .iter()
            .any(|(k, _)| *k == NudgeKind::MergedStillActive)
    );

    let mut done = merged_item();
    done.work_items[0].state_category = StateCategory::Completed;
    assert!(fire(&done, &mut Ledger::default(), at(3, 10)).is_empty());
}

#[test]
fn ac5_n7_treats_a_custom_in_progress_state_as_active() {
    let mut s = snap();
    s.work_items
        .push(wi(47001, "Doing", StateCategory::InProgress, at(0, 12)));
    s.work_items
        .push(wi(47002, "New", StateCategory::Proposed, at(0, 12)));
    // 5 working days = 45 wh from Mon 12:00 → next Mon 12:00.
    assert!(fire(&s, &mut Ledger::default(), at(7, 12) - MINUTE).is_empty());
    let pass = evaluate(&s, &Settings::default(), &mut Ledger::default(), at(7, 12));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::QuietActiveItem, Subject::WorkItem(47001))]
    );
    assert_eq!(
        pass.fired[0].message,
        "#47001 has been Doing with no linked PR or change for 5 working days"
    );
}

#[test]
fn n7_counts_linked_pr_activity_as_activity() {
    let mut s = snap();
    let mut item = wi(47003, "Active", StateCategory::InProgress, at(0, 12));
    item.linked_pr_ids.push(950);
    s.work_items.push(item);
    let mut p = pr(950, ME, at(0, 9));
    p.updated = at(5 + 2, 9); // touched the following Monday
    p.reviewers.push(reviewer("kim", Vote::Approved, false));
    s.prs.push(p);
    assert!(fire(&s, &mut Ledger::default(), at(7, 12)).is_empty());
}

// ── cap, priority, dedupe (FR-4) ────────────────────────────────────────

/// Six subjects, each eligible for a different nudge on Friday 12:00.
fn six_eligible() -> Snapshot {
    let mut s = snap();
    for part in [
        review_request(),
        blocked_pr(),
        failing_pr(),
        ignored_pr(),
        conflicted_pr(),
        merged_item(),
    ] {
        s.prs.extend(part.prs);
        s.runs.extend(part.runs);
        s.work_items.extend(part.work_items);
        s.threads.extend(part.threads);
    }
    s
}

#[test]
fn ac3_cap_fires_three_by_priority_and_the_today_card_lists_the_rest() {
    let s = six_eligible();
    let friday = at(4, 12);
    let mut ledger = Ledger::default();
    let settings = Settings::default();
    let pass = evaluate(&s, &settings, &mut ledger, friday);
    assert_eq!(
        kinds(&pass.fired),
        [
            (NudgeKind::MergeConflicts, Subject::Pr(873)),
            (NudgeKind::BuildFailing, Subject::Pr(871)),
            (NudgeKind::WaitingForAuthor, Subject::Pr(870)),
        ]
    );
    assert_eq!(
        kinds(&pass.held),
        [
            (NudgeKind::ReviewWaiting, Subject::Pr(912)),
            (NudgeKind::MergedStillActive, Subject::WorkItem(48213)),
            (NudgeKind::NoReviewerActivity, Subject::Pr(872)),
        ]
    );
    // Later the same day nothing more fires: excess isn't queued.
    assert!(
        evaluate(&s, &settings, &mut ledger, friday + HOUR)
            .fired
            .is_empty()
    );

    // Next workday morning: Today carries all six, each on its subject's card.
    let monday = at(7, 9) + MINUTE;
    assert!(ledger.today_visible(monday, &settings.hours));
    let pass = evaluate(&s, &settings, &mut ledger, monday);
    let day = today(&s, &settings, &pass, &ledger, monday);
    assert_eq!(pass.active.len(), 6);
    let on_cards = carded(&day);
    for nudge in &pass.active {
        assert!(on_cards.contains(&nudge.subject), "{:?}", nudge.subject);
    }
    assert_eq!(pass.fired.len(), 3, "the held three fire on the new day");
}

#[test]
fn a_zero_cap_fires_nothing_but_keeps_the_card_informed() {
    let s = six_eligible();
    let mut settings = Settings::default();
    settings.set_daily_cap(0);
    let pass = evaluate(&s, &settings, &mut Ledger::default(), at(4, 12));
    assert!(pass.fired.is_empty());
    assert_eq!(pass.held.len(), 6);
}

#[test]
fn one_subject_yields_one_nudge_the_highest_priority() {
    // !870 is both blocked (N2) and failing (N3).
    let mut s = blocked_pr();
    s.runs.push(run(9, 870, RunResult::Failed, at(0, 10)));
    let mut ledger = Ledger::default();
    let pass = evaluate(&s, &Settings::default(), &mut ledger, at(1, 12));
    assert_eq!(
        kinds(&pass.fired),
        [(NudgeKind::BuildFailing, Subject::Pr(870))]
    );
    assert_eq!(pass.active.len(), 1);
}

#[test]
fn ac4_a_subject_never_gets_two_nudges_within_24h_even_across_restarts() {
    // N5 fires Monday 10:00; N2 becomes eligible Monday 15:00 on the same PR.
    let mut s = conflicted_pr();
    s.prs[0].reviewers.push({
        let mut kim = reviewer("kim", Vote::WaitingForAuthor, false);
        kim.voted_at = Some(at(0, 11));
        kim
    });
    let mut ledger = Ledger::default();
    assert_eq!(fire(&s, &mut ledger, at(0, 10)).len(), 1);

    // "Restart": rebuild the ledger from its persisted parts.
    let mut restored = Ledger::from_parts(
        ledger.subjects().map(|(k, v)| (k.to_owned(), v.clone())),
        ledger.fired().to_vec(),
        ledger.today_dismissed_day(),
    );
    assert_eq!(restored, ledger);
    // Conflict resolved; the wait-for-author vote is now the top nudge.
    s.prs[0].merge_status = MergeStatus::Succeeded;
    assert!(fire(&s, &mut restored, at(1, 10) - MINUTE).is_empty());
    assert_eq!(
        fire(&s, &mut restored, at(1, 10)),
        [(NudgeKind::WaitingForAuthor, Subject::Pr(873))]
    );
}

// ── opt-outs, snooze, dismiss (FR-8, §3.3) ──────────────────────────────

#[test]
fn ac6_not_for_this_item_silences_only_that_pr() {
    let s = six_eligible();
    let mut settings = Settings::default();
    settings.set_daily_cap(10);
    let mut ledger = Ledger::default();
    ledger.not_for_this_item(&Subject::Pr(873));
    let pass = evaluate(&s, &settings, &mut ledger, at(4, 12));
    assert_eq!(pass.fired.len(), 5);
    assert!(!pass.fired.iter().any(|n| n.subject == Subject::Pr(873)));
    assert!(!pass.active.iter().any(|n| n.subject == Subject::Pr(873)));
}

#[test]
fn turning_a_type_off_stops_it_and_resolves_open_ones() {
    let s = conflicted_pr();
    let mut settings = Settings::default();
    let mut ledger = Ledger::default();
    assert_eq!(
        evaluate(&s, &settings, &mut ledger, at(0, 12)).fired.len(),
        1
    );
    settings.set_enabled(NudgeKind::MergeConflicts, false);
    let pass = evaluate(&s, &settings, &mut ledger, at(0, 13));
    assert_eq!(
        pass.resolved,
        [(Subject::Pr(873), NudgeKind::MergeConflicts)]
    );
    assert!(pass.active.is_empty());
    let day = today(&s, &settings, &pass, &ledger, at(0, 13));
    assert_eq!(day.focus, FocusState::Clear);
    assert_eq!(
        day.tracks[0].blocker, None,
        "a type turned off blocks nothing"
    );
}

#[test]
fn snoozed_subjects_return_when_the_snooze_ends() {
    let s = review_request();
    let mut ledger = Ledger::default();
    ledger.snooze(&Subject::Pr(912), at(3, 9));
    assert!(fire(&s, &mut ledger, at(2, 16)).is_empty());
    assert!(fire(&s, &mut ledger, at(3, 9) - MINUTE).is_empty());
    assert_eq!(fire(&s, &mut ledger, at(3, 9)).len(), 1);
}

#[test]
fn dismissed_nudges_stay_quiet_until_their_condition_resolves() {
    let s = conflicted_pr();
    let mut ledger = Ledger::default();
    assert_eq!(fire(&s, &mut ledger, at(0, 10)).len(), 1);
    ledger.dismiss(&Subject::Pr(873), NudgeKind::MergeConflicts);
    let pass = evaluate(&s, &Settings::default(), &mut ledger, at(2, 10));
    assert!(pass.fired.is_empty() && pass.active.is_empty());

    let mut clean = s.clone();
    clean.prs[0].merge_status = MergeStatus::Succeeded;
    evaluate(&clean, &Settings::default(), &mut ledger, at(2, 11));
    assert_eq!(fire(&s, &mut ledger, at(2, 12)).len(), 1);
}

// ── data health (§3 states, AC-8) ───────────────────────────────────────

#[test]
fn nothing_fires_before_the_first_full_sync() {
    let mut s = six_eligible();
    s.health.initial_sync_done = false;
    assert!(fire(&s, &mut Ledger::default(), at(4, 12)).is_empty());
}

#[test]
fn ac8_stale_tiers_suppress_dependent_nudges_and_never_resolve_them() {
    let mut s = six_eligible();
    s.health.warm_stale = true;
    let mut ledger = Ledger::default();
    assert!(fire(&s, &mut ledger, at(4, 12)).is_empty());

    // Hot tier stale: only warm-only kinds (N5, N6, N7) may fire.
    s.health = DataHealth {
        hot_stale: true,
        ..DataHealth::FRESH
    };
    let fired = fire(&s, &mut ledger, at(4, 12));
    assert_eq!(
        fired,
        [
            (NudgeKind::MergeConflicts, Subject::Pr(873)),
            (NudgeKind::MergedStillActive, Subject::WorkItem(48213)),
        ]
    );

    // A stale warm tier with an emptied snapshot resolves nothing.
    let mut empty = snap();
    empty.health.warm_stale = true;
    let pass = evaluate(&empty, &Settings::default(), &mut ledger, at(4, 13));
    assert!(pass.resolved.is_empty());
}

// ── delivery seam ───────────────────────────────────────────────────────

#[derive(Default)]
struct FakeSink {
    delivered: Vec<(String, Millis)>,
    resolved: Vec<(Subject, NudgeKind)>,
}

impl NudgeSink for FakeSink {
    fn deliver(&mut self, nudge: &Nudge, at: Millis) {
        self.delivered.push((nudge.key(), at));
    }
    fn resolve(&mut self, subject: &Subject, kind: NudgeKind, _at: Millis) {
        self.resolved.push((subject.clone(), kind));
    }
}

#[test]
fn run_pass_delivers_at_the_injected_time() {
    let clock = FixedClock::new(at(2, 16));
    let mut sink = FakeSink::default();
    let mut ledger = Ledger::default();
    let mut s = review_request();
    run_pass(&s, &Settings::default(), &mut ledger, &clock, &mut sink);
    assert_eq!(sink.delivered, [("N1:pr:912".to_owned(), at(2, 16))]);

    s.prs[0].reviewers[0].vote = Vote::Approved;
    clock.advance(2 * MINUTE);
    run_pass(&s, &Settings::default(), &mut ledger, &clock, &mut sink);
    assert_eq!(
        sink.resolved,
        [(Subject::Pr(912), NudgeKind::ReviewWaiting)]
    );
}

// ── Today card (§3.1, AC-9) ─────────────────────────────────────────────

#[test]
fn ac9_today_card_shows_once_per_workday_after_hours_start() {
    let hours = WorkingHours::default();
    let mut ledger = Ledger::default();
    assert!(!ledger.today_visible(at(1, 8), &hours), "before 09:00");
    assert!(ledger.today_visible(at(1, 9), &hours));
    ledger.dismiss_today(at(1, 9), &hours);
    assert!(!ledger.today_visible(at(1, 17), &hours));
    assert!(!ledger.today_visible(at(5, 10), &hours), "Saturday");
    assert!(ledger.today_visible(at(2, 9), &hours), "next workday");
}

/// Subjects that have a card on Today.
fn carded(day: &Day) -> Vec<Subject> {
    day.waiting
        .iter()
        .map(|c| Subject::Pr(c.pr_id))
        .chain(day.tracks.iter().map(|t| Subject::Pr(t.pr_id)))
        .chain(day.loose.iter().map(|l| Subject::WorkItem(l.id)))
        .collect()
}

fn focus(day: &Day) -> &Focus {
    match &day.focus {
        FocusState::Item(focus) => focus,
        other => panic!("expected a focus, got {other:?}"),
    }
}

#[test]
fn focus_is_the_first_due_nudge_in_fr4_order_and_skips_snoozed_ones() {
    let s = six_eligible();
    let settings = Settings::default();
    let now = at(4, 12);
    let mut ledger = Ledger::default();
    let pass = evaluate(&s, &settings, &mut ledger, now);
    let day = today(&s, &settings, &pass, &ledger, now);
    let f = focus(&day);
    assert!(f.due);
    assert_eq!(f.nudge.subject, Subject::Pr(873), "N5 outranks everything");
    assert_eq!(f.headline, "Your PR has a conflict");
    assert_eq!(f.who, None, "my own PR: a glyph, not a face");

    ledger.snooze(&Subject::Pr(873), settings.hours.next_workday_start(now));
    ledger.snooze(&Subject::Pr(871), settings.hours.next_workday_start(now));
    let pass = evaluate(&s, &settings, &mut ledger, now);
    let day = today(&s, &settings, &pass, &ledger, now);
    let f = focus(&day);
    assert_eq!(f.nudge.subject, Subject::Pr(870), "then N2");
    assert_eq!(f.headline, "Kim asked for changes");
    assert_eq!(f.who.as_deref(), Some("kim"));
    let snoozed = day.tracks.iter().find(|t| t.pr_id == 873).unwrap();
    assert_eq!(snoozed.quiet, Some(Quiet::Snoozed));
}

#[test]
fn focus_falls_back_to_the_oldest_review_then_clear_and_waits_for_sync() {
    let settings = Settings::default();
    // Asked Monday 10:00; at 11:00 it's far under N1's 24 working hours.
    let s = review_request();
    let now = at(0, 11);
    let mut ledger = Ledger::default();
    let pass = evaluate(&s, &settings, &mut ledger, now);
    assert!(pass.active.is_empty());
    let day = today(&s, &settings, &pass, &ledger, now);
    let f = focus(&day);
    assert!(!f.due, "calm: not a nudge yet");
    assert_eq!(f.headline, "Kim is blocked on you");
    assert_eq!(f.label, "!912 Change 912");
    assert_eq!(f.age.heat(), Heat::Cool);

    // Marked done: nothing else waits on me.
    ledger.dismiss(&Subject::Pr(912), NudgeKind::ReviewWaiting);
    let pass = evaluate(&s, &settings, &mut ledger, now);
    let day = today(&s, &settings, &pass, &ledger, now);
    assert_eq!(day.focus, FocusState::Clear);
    assert_eq!(
        day.waiting[0].quiet,
        Some(Quiet::Done),
        "still shown, parked"
    );

    let mut partial = review_request();
    partial.health.initial_sync_done = false;
    let pass = evaluate(&partial, &settings, &mut Ledger::default(), at(4, 12));
    let day = today(&partial, &settings, &pass, &Ledger::default(), at(4, 12));
    assert_eq!(day.focus, FocusState::Syncing, "no claims on partial data");
}

#[test]
fn waiting_cards_sort_blocked_longest_first_and_parked_last() {
    let mut s = snap();
    for (id, author, added) in [
        (1, "kim", at(1, 10)),
        (2, "sam", at(0, 10)),
        (3, "lee", at(2, 9)),
    ] {
        let mut p = pr(id, author, at(0, 9));
        let mut me = reviewer(ME, Vote::NoVote, id == 3);
        me.added_at = Some(added);
        p.reviewers.push(me);
        s.prs.push(p);
    }
    s.threads
        .insert(1, vec![thread(1, vec![comment("kim", at(1, 11))])]);
    s.file_counts.insert(1, 7);
    let settings = Settings::default();
    let now = at(4, 12);
    let mut ledger = Ledger::default();
    ledger.snooze(&Subject::Pr(2), at(7, 9));
    let pass = evaluate(&s, &settings, &mut ledger, now);
    let day = today(&s, &settings, &pass, &ledger, now);
    let order: Vec<i64> = day.waiting.iter().map(|c| c.pr_id).collect();
    assert_eq!(order, [1, 3, 2], "oldest ask first, snoozed at the end");
    let first = &day.waiting[0];
    assert_eq!((first.files, first.threads), (Some(7), 1));
    assert_eq!(first.author_name, "Kim");
    assert!(day.waiting[1].required);
    assert_eq!(day.waiting[2].quiet, Some(Quiet::Snoozed));
    // Tue 10:00 → Fri 12:00 = 29 working hours: past 24, under 72.
    assert_eq!(first.age.heat(), Heat::Warm);
    assert_eq!(
        focus(&day).nudge.subject,
        Subject::Pr(1),
        "not the snoozed one"
    );
}

#[test]
fn age_heat_warms_at_the_threshold_and_burns_at_three() {
    let age = |waited| Age {
        since: 0,
        waited,
        threshold: 10,
    };
    assert_eq!(age(9).heat(), Heat::Cool);
    assert_eq!(age(10).heat(), Heat::Warm);
    assert_eq!(age(29).heat(), Heat::Warm);
    assert_eq!(age(30).heat(), Heat::Hot);
    let immediate = Age {
        since: 0,
        waited: 0,
        threshold: 0,
    };
    assert_eq!(immediate.heat(), Heat::Hot);
}

#[test]
fn track_stage_follows_votes_required_reviewers_and_status() {
    let stage = |tweak: fn(&mut PullRequest)| {
        let mut p = pr(1, ME, at(0, 9));
        p.reviewers.push(reviewer("kim", Vote::NoVote, true));
        p.reviewers.push(reviewer("sam", Vote::NoVote, false));
        tweak(&mut p);
        Stage::of(&p, ME)
    };
    assert_eq!(stage(|_| {}), Stage::Review);
    assert_eq!(stage(|p| p.is_draft = true), Stage::Draft);
    assert_eq!(
        stage(|p| p.reviewers[1].vote = Vote::Approved),
        Stage::Review,
        "the required reviewer hasn't approved"
    );
    assert_eq!(
        stage(|p| p.reviewers[0].vote = Vote::ApprovedWithSuggestions),
        Stage::Approved
    );
    assert_eq!(
        stage(|p| {
            p.reviewers[0].vote = Vote::Approved;
            p.reviewers[1].vote = Vote::WaitingForAuthor;
        }),
        Stage::Review,
        "a wait-for-author vote holds it back"
    );
    assert_eq!(
        stage(|p| {
            p.reviewers[0].has_declined = true;
            p.reviewers[1].vote = Vote::Approved;
        }),
        Stage::Approved,
        "declined reviewers don't count"
    );
    assert_eq!(stage(|p| p.status = PrStatus::Completed), Stage::Merged);
}

#[test]
fn tracks_carry_one_blocker_in_priority_order() {
    let s = six_eligible();
    let settings = Settings::default();
    let blockers = |now| {
        let mut ledger = Ledger::default();
        let pass = evaluate(&s, &settings, &mut ledger, now);
        let mut blockers = today(&s, &settings, &pass, &ledger, now)
            .tracks
            .iter()
            .map(|t| (t.pr_id, t.blocker))
            .collect::<Vec<_>>();
        blockers.sort_unstable();
        blockers
    };
    assert_eq!(
        blockers(at(4, 12)),
        [
            (870, Some(Blocker::ChangesRequested)),
            (871, Some(Blocker::BuildFailed)),
            (872, Some(Blocker::WaitingOnReviewers)),
            (873, Some(Blocker::Conflict)),
        ]
    );
    // Monday noon: N4 hasn't crossed its 24 working hours, so no blocker yet.
    let monday = blockers(at(0, 12));
    assert!(monday.contains(&(872, None)), "{monday:?}");

    let mut ledger = Ledger::default();
    let pass = evaluate(&s, &settings, &mut ledger, at(4, 12));
    let day = today(&s, &settings, &pass, &ledger, at(4, 12));
    let ignored = day.tracks.iter().find(|t| t.pr_id == 872).unwrap();
    let names: Vec<&str> = ignored.reviewers.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Kim", "Sam"], "required first");
    assert_eq!(ignored.nudge.as_ref().unwrap().ping, ["Kim", "Sam"]);
    let conflicted = day.tracks.iter().find(|t| t.pr_id == 873).unwrap();
    assert_eq!(conflicted.stage, Stage::Approved);
}

#[test]
fn loose_ends_are_merged_items_and_quiet_items_past_threshold() {
    let mut s = merged_item();
    s.work_items
        .push(wi(47001, "Doing", StateCategory::InProgress, at(0, 12)));
    let settings = Settings::default();
    let loose = |now| {
        let mut ledger = Ledger::default();
        let pass = evaluate(&s, &settings, &mut ledger, now);
        today(&s, &settings, &pass, &ledger, now).loose
    };
    let tuesday = loose(at(1, 12));
    assert_eq!(tuesday.len(), 1, "N7 isn't due yet");
    assert_eq!((tuesday[0].id, tuesday[0].merged_pr), (48213, Some(901)));
    let later = loose(at(7, 12));
    assert_eq!(
        later.iter().map(|l| l.id).collect::<Vec<_>>(),
        [48213, 47001],
        "FR-4 order: N6 before N7"
    );
}

#[test]
fn evaluation_is_deterministic() {
    let s = six_eligible();
    let a = evaluate(&s, &Settings::default(), &mut Ledger::default(), at(4, 12));
    let b = evaluate(&s, &Settings::default(), &mut Ledger::default(), at(4, 12));
    assert_eq!(a, b);
}

// ── store adapter (FR-7, AC-4 persistence) ──────────────────────────────

mod store_io {
    use super::*;
    use adoken_model::{Person, Project, Repo};
    use adoken_store::{Store, write};

    fn person(id: &str, name: &str) -> Person {
        Person {
            id: id.into(),
            display_name: name.into(),
            unique_name: None,
            is_group: false,
            image_url: None,
        }
    }

    fn seeded() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        store
            .write(|c| {
                write::upsert_person(c, &person(ME, "Me"), true)?;
                write::upsert_person(c, &person("kim", "Kim"), false)?;
                write::upsert_project(
                    c,
                    &Project {
                        id: "proj".into(),
                        name: "Proj".into(),
                    },
                )?;
                write::upsert_repo(
                    c,
                    &Repo {
                        id: "repo".into(),
                        project_id: "proj".into(),
                        name: "repo".into(),
                        default_branch: None,
                    },
                )?;
                for p in merged_item().prs.iter().chain(&review_request().prs) {
                    write::upsert_pr(c, p)?;
                }
                // An unrelated active PR must not be loaded.
                write::upsert_pr(c, &pr(5, "kim", at(0, 9)))?;
                for w in &merged_item().work_items {
                    write::upsert_work_item(c, w)?;
                }
                Ok(())
            })
            .unwrap();
        crate::store::migrate(&store).unwrap();
        (dir, store)
    }

    #[test]
    fn snapshot_loads_my_prs_and_linked_prs_only() {
        let (_dir, store) = seeded();
        let snap = crate::store::load_snapshot(&store, DataHealth::FRESH, at(2, 0))
            .unwrap()
            .expect("me is known");
        let mut ids: Vec<i64> = snap.prs.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [901, 912]);
        assert_eq!(snap.name("kim"), "Kim");
        let fired = fire(&snap, &mut Ledger::default(), at(2, 16));
        assert_eq!(
            fired,
            [
                (NudgeKind::ReviewWaiting, Subject::Pr(912)),
                (NudgeKind::MergedStillActive, Subject::WorkItem(48213)),
            ]
        );
    }

    #[test]
    fn snapshot_loads_todays_merges_pushes_comments_done_items_and_file_counts() {
        let (_dir, store) = seeded();
        let iteration = |id: u32, created, count| adoken_model::PrIteration {
            id,
            source_commit: format!("c{id}"),
            created,
            description: None,
            commit_count: Some(count),
            commits_truncated: false,
        };
        store
            .write(|c| {
                // 901 (mine) merged Monday 10:00: pushed Sunday and Monday,
                // Kim commented Monday.
                write::put_pr_iterations(
                    c,
                    901,
                    &[iteration(1, at(-1, 12), 3), iteration(2, at(0, 9), 2)],
                )?;
                write::upsert_thread(c, &thread(901, vec![comment("kim", at(0, 9))]))?;
                let mut done = wi(47002, "Closed", StateCategory::Completed, at(0, 9));
                done.linked_pr_ids.clear();
                write::upsert_work_item(c, &done)?;
                let file = adoken_model::PrFileChange {
                    path: "/src/lib.rs".into(),
                    original_path: None,
                    change_type: adoken_model::ChangeType::Edit,
                    old_content: None,
                    new_content: None,
                    change_tracking_id: None,
                };
                write::put_pr_files(c, 912, &[file.clone(), file])?;
                Ok(())
            })
            .unwrap();
        let monday = crate::store::load_snapshot(&store, DataHealth::FRESH, at(0, 0))
            .unwrap()
            .unwrap();
        let merged: Vec<i64> = monday.merged_today.iter().map(|p| p.id).collect();
        assert_eq!(merged, [901]);
        let pushes: Vec<u32> = monday.iterations_today[&901].iter().map(|i| i.id).collect();
        assert_eq!(pushes, [2], "Sunday's push isn't today's");
        assert!(
            monday.threads.contains_key(&901),
            "today's merge keeps its threads"
        );
        assert_eq!(monday.done_today, [47002]);
        assert_eq!(monday.file_counts.get(&912), Some(&2));
        let counts = crate::summary::counts(&monday, at(0, 0), at(0, 12));
        assert_eq!((counts.merged, counts.commits, counts.done), (1, 2, 1));

        let tuesday = crate::store::load_snapshot(&store, DataHealth::FRESH, at(1, 0))
            .unwrap()
            .unwrap();
        assert!(tuesday.merged_today.is_empty() && tuesday.done_today.is_empty());
    }

    #[test]
    fn ledger_survives_a_restart() {
        let (dir, store) = seeded();
        let snap = crate::store::load_snapshot(&store, DataHealth::FRESH, at(2, 0))
            .unwrap()
            .unwrap();
        let mut ledger = crate::store::load_ledger(&store).unwrap();
        assert_eq!(ledger, Ledger::default());
        assert_eq!(fire(&snap, &mut ledger, at(2, 16)).len(), 2);
        ledger.not_for_this_item(&Subject::Pr(5));
        ledger.dismiss_today(at(2, 16), &WorkingHours::default());
        crate::store::save_ledger(&store, &ledger).unwrap();
        drop(store);

        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        crate::store::migrate(&store).unwrap(); // idempotent on reopen
        let mut restored = crate::store::load_ledger(&store).unwrap();
        assert_eq!(restored, ledger);
        assert!(fire(&snap, &mut restored, at(2, 17)).is_empty(), "AC-4");
    }
}

mod records {
    use super::*;

    #[test]
    fn in_memory_selection_matches_the_store_adapter() {
        let mut all = six_eligible();
        all.prs.push(pr(5, "kim", at(0, 9))); // not mine, not linked
        all.runs.push(run(77, 5, RunResult::Failed, at(0, 10)));
        let snap = Snapshot::from_records(
            ME,
            &[],
            &all.prs,
            all.threads.clone(),
            &all.runs,
            &all.work_items,
        );
        let mut ids: Vec<i64> = snap.prs.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [870, 871, 872, 873, 901, 912]);
        assert_eq!(
            snap.runs.iter().map(|r| r.id).collect::<Vec<_>>(),
            [2],
            "latest run of my PRs only"
        );
        let fired = fire(&snap, &mut Ledger::default(), at(4, 12));
        assert_eq!(fired, fire(&all, &mut Ledger::default(), at(4, 12)));
    }

    #[test]
    fn inbox_sink_queues_nudges_and_resolutions_locally() {
        let dir = tempfile::tempdir().unwrap();
        let store = adoken_store::Store::open(dir.path().join("adoken.sqlite")).unwrap();
        let s = review_request();
        let clock = FixedClock::new(at(2, 16));
        let mut ledger = Ledger::default();
        let mut sink = crate::store::InboxSink::new(&store);
        run_pass(&s, &Settings::default(), &mut ledger, &clock, &mut sink);
        sink.finish().unwrap();
        let queued = adoken_notify::persist::pending(&store).unwrap();
        assert_eq!(queued.len(), 1);
    }
}

mod demo {
    use super::*;

    #[test]
    fn the_demo_org_triggers_every_rule_for_me() {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let snap = Snapshot::from_records(
            &ds.me,
            &ds.people,
            &ds.prs,
            ds.pr_extras.iter().map(|(id, e)| (*id, e.threads.clone())),
            &ds.runs,
            &ds.work_items,
        );
        let settings = Settings::default();
        let mut ledger = Ledger::default();
        let pass = evaluate(&snap, &settings, &mut ledger, ds.now);
        let day = today(&snap, &settings, &pass, &ledger, ds.now);
        let mut kinds: Vec<_> = pass.active.iter().map(|n| n.kind).collect();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds, NudgeKind::ALL, "every rule past its threshold");
        assert_eq!(pass.fired.len(), 3, "daily cap");
        assert!(
            kinds_of(&pass.fired)
                .windows(2)
                .all(|w| w[0].rank() <= w[1].rank()),
            "fired in priority order"
        );
        assert!(
            day.tracks.iter().any(|t| {
                t.blocker == Some(Blocker::WaitingOnReviewers)
                    && t.nudge.as_ref().is_some_and(|n| !n.ping.is_empty())
            }),
            "N4 names reviewers to ping"
        );
        assert!(matches!(day.focus, FocusState::Item(_)));
        assert!(ledger.today_visible(ds.now, &settings.hours));
    }

    fn kinds_of(nudges: &[Nudge]) -> Vec<NudgeKind> {
        nudges.iter().map(|n| n.kind).collect()
    }
}
