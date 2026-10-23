use adoken_model::time::{MINUTE, SECOND};

use super::*;
use crate::cadence::Pause;
use crate::clock::ManualClock;

const NOW: Millis = 1_790_769_600_000;

fn synced() -> SyncInputs {
    SyncInputs {
        lanes: std::array::from_fn(|_| LaneActivity {
            last_ok: Some(NOW - 40 * SECOND),
            next_due: Some(NOW + 20 * SECOND),
            ..LaneActivity::default()
        }),
        online: true,
        pins: PinsRow {
            pins: 3,
            unavailable: 1,
            last_full_refresh: Some(NOW - MINUTE),
        },
        foreground: true,
        ..SyncInputs::default()
    }
}

fn state(inputs: &SyncInputs, kind: RowKind) -> RowState {
    rows(inputs, NOW)
        .into_iter()
        .find(|r| r.kind == kind)
        .unwrap()
        .state
}

/// Every FR-8 state, one fixture each, in precedence order: applying the
/// next fixture on top of all earlier ones never changes the winner, and
/// each fixture alone produces its state. Adjacent pairs are therefore
/// ordered as the spec lists them (AC-A1).
#[test]
fn row_states_follow_fr8_precedence() {
    type Apply = fn(&mut SyncInputs);
    type Holds = fn(&RowState) -> bool;
    let fixtures: [(Apply, Holds); 12] = [
        (
            |i| i.gov.unauthorized = true,
            |s| *s == RowState::TokenRejected,
        ),
        (
            |i| i.missing_scopes = vec!["vso.work".into()],
            |s| *s == RowState::NeedsScope("vso.work"),
        ),
        (|i| i.online = false, |s| *s == RowState::Offline),
        (
            |i| {
                i.hold = Some(Hold::Paused {
                    until: NOW + 40 * SECOND,
                });
                i.gov.resource = Some("Core".into());
            },
            |s| {
                *s == RowState::WaitingForAdo {
                    until: NOW + 40 * SECOND,
                    resource: Some("Core".into()),
                }
            },
        ),
        (
            |i| {
                // A budget wait only shows without a host pause.
                if i.hold.is_none() {
                    i.hold = Some(Hold::Budget {
                        until: NOW + MINUTE,
                    });
                }
            },
            |s| matches!(s, RowState::WaitingForBudget { .. }),
        ),
        (
            |i| {
                let l = &mut i.lanes[Lane::WorkItems.index()];
                l.running = true;
                l.progress = Some(Progress {
                    done: 400,
                    total: Some(1_150),
                    unit: ProgressUnit::WorkItems,
                });
            },
            |s| matches!(s, RowState::Syncing(Some(p)) if p.done == 400),
        ),
        (
            |i| {
                let l = &mut i.lanes[Lane::WorkItems.index()];
                if !l.running {
                    l.failure = Some((
                        FailReason::Status {
                            status: 503,
                            code: None,
                            message: None,
                        },
                        Some(NOW + 30 * SECOND),
                    ));
                }
            },
            |s| {
                matches!(
                    s,
                    RowState::Failed {
                        retry_at: Some(_),
                        ..
                    }
                )
            },
        ),
        (
            |i| i.prefs.pause = Pause::Until(NOW + 60 * MINUTE),
            |s| {
                *s == RowState::Paused {
                    until: Some(NOW + 60 * MINUTE),
                }
            },
        ),
        (
            |i| {
                i.foreground = false;
                i.prefs.background = Background::Paused;
            },
            |s| *s == RowState::PausedInBackground,
        ),
        (
            |i| {
                i.pressure = Some(Pressure::Slow);
                i.remaining_pct = Some(38.0);
            },
            |s| matches!(s, RowState::Slowed { remaining_pct: Some(p), .. } if *p == 38.0),
        ),
        (
            |i| i.prefs.work_items = Cadence::Manual,
            |s| *s == RowState::ManualOnly,
        ),
        (|_| {}, |s| *s == RowState::Synced),
    ];
    // Each alone.
    for (ix, (apply, is)) in fixtures.iter().enumerate() {
        let mut i = synced();
        apply(&mut i);
        let s = state(&i, RowKind::WorkItems);
        assert!(is(&s), "fixture {ix} alone: {s:?}");
    }
    // Stacked from the lowest precedence up: the highest applied wins.
    for top in 0..fixtures.len() {
        let mut i = synced();
        for (apply, _) in fixtures[top..].iter().rev() {
            apply(&mut i);
        }
        let s = state(&i, RowKind::WorkItems);
        assert!((fixtures[top].1)(&s), "stack from {top}: {s:?}");
    }
    let mut never = synced();
    never.lanes = Default::default();
    assert_eq!(state(&never, RowKind::WorkItems), RowState::NotSyncedYet);
}

/// AC-A1 pairs the spec names: offline beats failed, a ⌘R pass beats
/// paused, a budget wait beats syncing.
#[test]
fn named_precedence_pairs() {
    let mut i = synced();
    i.lanes[Lane::AllPrs.index()].failure = Some((FailReason::Decode, None));
    i.online = false;
    assert_eq!(state(&i, RowKind::AllPrs), RowState::Offline);

    let mut i = synced();
    i.prefs.pause = Pause::UntilResumed;
    i.lanes[Lane::MyPrs.index()].running = true;
    assert_eq!(state(&i, RowKind::MyPrs), RowState::Syncing(None));

    let mut i = synced();
    i.lanes[Lane::MyPrs.index()].running = true;
    i.hold = Some(Hold::Budget { until: NOW + 1 });
    assert!(matches!(
        state(&i, RowKind::MyPrs),
        RowState::WaitingForBudget { .. }
    ));
}

/// AC-A10: a token without `vso.build` affects Pipeline runs only; a 401
/// turns every row to "Token rejected".
#[test]
fn scopes_and_token() {
    let granted: Vec<String> = ["vso.code_write", "vso.work", "vso.identity"]
        .map(String::from)
        .to_vec();
    let mut i = synced();
    i.missing_scopes = missing_scopes(&granted);
    assert_eq!(i.missing_scopes, ["vso.build"]);
    for row in rows(&i, NOW) {
        match row.kind {
            RowKind::Pipelines => assert_eq!(row.state, RowState::NeedsScope("vso.build")),
            _ => assert_eq!(row.state, RowState::Synced, "{:?}", row.kind),
        }
    }
    assert!(
        missing_scopes(&[]).is_empty(),
        "unknown scopes: nothing missing"
    );
    i.gov.unauthorized = true;
    assert!(
        rows(&i, NOW)
            .iter()
            .all(|r| r.state == RowState::TokenRejected)
    );
}

/// FR-7: Pipeline runs shows the sooner next pass and the worse state of
/// its two lanes; offline and manual rows have no next time.
#[test]
fn pipelines_combine_two_lanes() {
    let mut i = synced();
    i.lanes[Lane::LiveRuns.index()].next_due = Some(NOW + 10 * SECOND);
    i.lanes[Lane::DoneRuns.index()].next_due = Some(NOW + 90 * SECOND);
    i.lanes[Lane::DoneRuns.index()].failure = Some((FailReason::Decode, None));
    let row = &rows(&i, NOW)[RowKind::Pipelines as usize];
    assert_eq!(row.next_due, Some(NOW + 10 * SECOND));
    assert!(matches!(row.state, RowState::Failed { .. }));
    i.online = false;
    assert_eq!(rows(&i, NOW)[RowKind::Pipelines as usize].next_due, None);
}

/// FR-12: exact counts, distinct ids, 20 listed then "+N more"; the
/// strongest change of an item wins.
#[test]
fn change_sets_count_exactly_and_cap_ids() {
    let mut set = ChangeSet::default();
    for id in 0..25 {
        set.note_work_item(id, ChangeWord::Updated);
    }
    set.note_work_item(3, ChangeWord::State("Active".into()));
    let mut page = ChangeSet::default();
    page.note_pr(7, ChangeWord::VoteChanged);
    page.note_pr(7, ChangeWord::Completed);
    page.note_pr(8, ChangeWord::New);
    page.note_pr(8, ChangeWord::Pushed);
    page.note_pr_comments(9, 2);
    page.note_index_entry();
    set.merge(page);
    let c = set.finish();
    assert_eq!(c.work_items.updated, 25);
    assert_eq!(c.work_items.ids.len(), MAX_IDS);
    assert_eq!(c.work_items.more, 5);
    assert_eq!(
        c.prs.ids,
        [
            (7, ChangeWord::Completed),
            (8, ChangeWord::New),
            (9, ChangeWord::NewComment)
        ]
    );
    assert_eq!((c.prs.new, c.prs.updated, c.prs.removed), (1, 1, 1));
    assert_eq!(c.pr_comments, 2);
    assert_eq!(c.index_entries, 1);
}

/// Only committed writes count: the collector hands back what was noted
/// inside, and notes outside any collector go nowhere.
#[test]
fn collector_scopes_notes_to_one_write() {
    note(|c, _| c.note_repo());
    let ((), set) = collect_changes(|| {
        note(|c, _| c.note_project());
        as_index(|| note(|c, index| assert!(index, "{c:?}")));
        note(|_, index| assert!(!index));
    });
    assert_eq!(set.finish().projects, 1);
    assert_eq!(set.finish().repos, 0);
}

fn tracker() -> (Arc<Tracker>, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(NOW));
    (Arc::new(Tracker::new(clock.clone())), clock)
}

/// FR-11/FR-13: passes that sent or changed something are kept, newest
/// first, at most 200 or 2 hours; quiet ones fold into the row state; a
/// dropped pass is recorded as canceled.
#[test]
fn pass_history_is_bounded_and_folds_quiet_passes() {
    let (t, clock) = tracker();
    let lanes: Lanes = [Lane::AllPrs].into_iter().collect();
    // Quiet: nothing sent, nothing changed.
    t.begin_pass(Trigger::Scheduled, lanes)
        .finish(Outcome::Deferred, 0);
    assert!(t.passes().is_empty());
    for i in 0..(MAX_PASSES + 5) {
        let pass = t.begin_pass(Trigger::Scheduled, lanes);
        let mut set = ChangeSet::default();
        set.note_pr(i as i64, ChangeWord::Updated);
        pass.add_changes(set);
        assert_eq!(t.passes()[0].outcome, Outcome::Running);
        pass.finish(Outcome::Ok, 0);
        clock.advance(SECOND);
    }
    let passes = t.passes();
    assert_eq!(passes.len(), MAX_PASSES);
    assert!(passes.windows(2).all(|w| w[0].started > w[1].started));
    // Two hours later one more pass leaves only itself.
    clock.advance(2 * HOUR + MINUTE);
    let pass = t.begin_pass(Trigger::SyncNow, lanes);
    pass.add_changes({
        let mut s = ChangeSet::default();
        s.note_repo();
        s
    });
    drop(pass);
    let passes = t.passes();
    assert_eq!(passes.len(), 1);
    assert_eq!(passes[0].outcome, Outcome::Canceled);
    assert_eq!(passes[0].changes.repos, 1);
}

/// FR-15: demo-feed ticks are passes.
#[test]
fn demo_ticks_are_demo_feed_passes() {
    let (t, _) = tracker();
    let mut changes = Changes::default();
    changes.prs.updated = 1;
    changes.prs.ids.push((4821, ChangeWord::Updated));
    t.record_demo_tick(changes.clone(), 2, NOW);
    let p = &t.passes()[0];
    assert_eq!(
        (p.trigger, p.events, &p.changes),
        (Trigger::DemoFeed, 2, &changes)
    );
}

/// AC-S3: 200 requests and 40 progress updates within one second publish
/// at most 5 times; a minute with no change publishes nothing.
#[test]
fn bursts_coalesce_to_four_hertz_and_quiet_costs_nothing() {
    let mut c = Coalescer::new(PUBLISH_INTERVAL_MS);
    let mut publishes = 0;
    let mut wake: Option<Millis> = None;
    // 240 changes spread over one second (every ~4 ms).
    for i in 0..240 {
        let t = i * 1_000 / 240;
        if let Some(at) = wake.filter(|at| *at <= t) {
            publishes += c.tick(at) as u32;
        }
        wake = Some(c.change(t));
    }
    // The trailing edge: the last change still publishes.
    publishes += c.tick(wake.unwrap()) as u32;
    assert!(publishes <= 5, "{publishes}");
    assert!(publishes >= 4, "{publishes}");
    assert_eq!(c.pending(), None);
    // Sixty quiet seconds: no timer, no publish.
    for t in (1_000..61_000).step_by(250) {
        assert!(!c.tick(t));
    }
}
