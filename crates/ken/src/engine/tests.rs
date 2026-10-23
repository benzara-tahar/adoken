use adoken_notify::FakeClock;

use super::*;

/// Fri 2026-09-25 09:30 UTC (the demo org's "now").
const NOW: Millis = 1_790_328_600_000;
const TEAMMATE_PR: i64 = 4312;

struct World {
    ken: Ken,
    clock: FakeClock,
    backfilling: bool,
}

impl World {
    fn new() -> Self {
        Self {
            ken: Ken::default(),
            clock: FakeClock::new(NOW, 0),
            backfilling: false,
        }
    }

    fn now(&self) -> Millis {
        self.clock.now()
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            now: self.clock.now(),
            clock: &self.clock,
            working_days: Weekdays::WORKDAYS,
            backfilling: self.backfilling,
        }
    }

    fn apply(&mut self, actions: Vec<Action>) -> Applied {
        let ctx = Ctx {
            now: self.clock.now(),
            clock: &self.clock,
            working_days: Weekdays::WORKDAYS,
            backfilling: self.backfilling,
        };
        self.ken.apply(actions, &ctx)
    }

    fn one(&mut self, action: Action) -> Result<HitKind, Rejection> {
        let applied = self.apply(vec![action]);
        match (applied.accepted.first(), applied.rejected.first()) {
            (Some(kind), _) => Ok(*kind),
            (None, Some(rejection)) => Err(*rejection),
            (None, None) => panic!("not a hit action"),
        }
    }

    fn advance(&mut self, by: Millis) -> Vec<Effect> {
        self.clock.advance(by);
        let ctx = Ctx {
            now: self.clock.now(),
            clock: &self.clock,
            working_days: Weekdays::WORKDAYS,
            backfilling: self.backfilling,
        };
        self.ken.tick(&ctx)
    }
}

fn pr(id: i64) -> PrFacts {
    PrFacts::new(id, format!("PR {id}"))
}

fn my_pr(id: i64) -> PrFacts {
    PrFacts {
        mine: true,
        ..pr(id)
    }
}

fn viewed(pr_id: i64, at: Millis) -> Action {
    Action::Viewed { pr: pr(pr_id), at }
}

fn comment(pr_id: i64, body: &str, at: Millis) -> Action {
    Action::Comment {
        pr: pr(pr_id),
        body: Some(body.to_owned()),
        at,
        origin: Origin::Sync,
    }
}

fn vote(facts: PrFacts, vote: Vote, at: Millis) -> Action {
    Action::Vote {
        pr: facts,
        vote,
        commit: None,
        at,
        origin: Origin::Sync,
    }
}

fn counts(effects: &[Effect]) -> Vec<u32> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Combo { count, .. } => Some(*count),
            _ => None,
        })
        .collect()
}

// ── hit counting ─────────────────────────────────────────────────────────

#[test]
fn viewed_files_are_hits_and_a_combo_starts_at_the_second() {
    let mut w = World::new();
    let first = w.apply(vec![viewed(1, NOW)]);
    assert_eq!(first.accepted, vec![HitKind::Viewed]);
    assert!(counts(&first.effects).is_empty(), "one hit is not a combo");
    assert_eq!(w.ken.combo_view(NOW), None);

    w.clock.advance(2 * MINUTE);
    let second = w.apply(vec![viewed(2, w.now())]);
    assert_eq!(counts(&second.effects), vec![2]);
    let view = w.ken.combo_view(w.now()).unwrap();
    assert_eq!((view.count, view.rank), (2, Rank::Flurry));
    assert_eq!(view.until, w.now() + COMBO_WINDOW);
}

#[test]
fn each_hit_resets_the_window_and_expiry_finishes_the_combo() {
    let mut w = World::new();
    for step in 0..7 {
        w.apply(vec![viewed(100 + step, w.now())]);
        w.clock.advance(9 * MINUTE);
    }
    // 7 hits 9 min apart: still one combo.
    let view = w.ken.combo_view(w.now()).unwrap();
    assert_eq!((view.count, view.rank), (7, Rank::Gale));

    // Last hit was 9 min ago: 1 min left, then the flourish.
    assert!(
        w.advance(MINUTE).is_empty(),
        "window end itself still counts"
    );
    let effects = w.advance(1);
    assert_eq!(
        effects,
        vec![Effect::ComboFinished {
            count: 7,
            rank: Some(Rank::Gale),
            prs: 7,
            duration: 6 * 9 * MINUTE,
            flourish: true,
        }]
    );
    assert_eq!(w.ken.combo_view(w.now()), None);
    let journal = w.ken.take_journal();
    assert_eq!(journal.hits.len(), 7);
    assert_eq!(journal.combos.len(), 1);
    assert_eq!(journal.combos[0].best_rank, Some(Rank::Gale));
}

#[test]
fn a_hit_after_the_window_starts_a_new_combo() {
    let mut w = World::new();
    w.apply(vec![viewed(1, NOW), viewed(2, NOW + MINUTE)]);
    w.clock.advance(12 * MINUTE);
    let applied = w.apply(vec![viewed(3, w.now())]);
    assert!(
        applied.effects.iter().any(|e| matches!(
            e,
            Effect::ComboFinished {
                count: 2,
                flourish: false,
                ..
            }
        )),
        "×2 fades without a flourish: {:?}",
        applied.effects
    );
    assert_eq!(w.ken.combo.as_ref().unwrap().hits, 1);
}

#[test]
fn rank_up_is_flagged_at_each_threshold() {
    let mut w = World::new();
    let mut rank_ups = Vec::new();
    for i in 0..12 {
        let applied = w.apply(vec![viewed(i, w.now())]);
        for effect in applied.effects {
            if let Effect::Combo {
                count,
                rank_up: true,
                ..
            } = effect
            {
                rank_ups.push(count);
            }
        }
        w.clock.advance(MINUTE);
    }
    assert_eq!(rank_ups, vec![5, 8, 12]);
}

#[test]
fn late_finish_is_silent() {
    let mut w = World::new();
    w.apply(vec![viewed(1, NOW), viewed(2, NOW), viewed(3, NOW)]);
    let effects = w.advance(COMBO_WINDOW + FINISH_GRACE + 1);
    assert!(matches!(
        effects[..],
        [Effect::ComboFinished {
            count: 3,
            flourish: false,
            ..
        }]
    ));
}

// ── anti-gaming ──────────────────────────────────────────────────────────

#[test]
fn at_most_three_hits_per_pr_per_combo() {
    let mut w = World::new();
    let results: Vec<_> = (0..5)
        .map(|i| {
            w.one(comment(
                TEAMMATE_PR,
                &format!("Consider a retry budget, take {i}"),
                NOW,
            ))
        })
        .collect();
    // No file opened in adoken: sync-seen comments don't count at all.
    assert!(results.iter().all(|r| *r == Err(Rejection::NotEngaged)));

    let mut w = World::new();
    w.one(viewed(TEAMMATE_PR, NOW)).unwrap();
    let results: Vec<_> = (0..4)
        .map(|i| {
            w.one(comment(
                TEAMMATE_PR,
                &format!("Consider a retry budget, take {i}"),
                NOW,
            ))
        })
        .collect();
    assert_eq!(
        results,
        vec![
            Ok(HitKind::Comment),
            Ok(HitKind::Comment),
            Err(Rejection::PrCap),
            Err(Rejection::PrCap),
        ]
    );
    // Other PRs still count in the same combo.
    assert_eq!(w.one(viewed(TEAMMATE_PR + 1, NOW)), Ok(HitKind::Viewed));
    // A new combo resets the cap.
    w.advance(COMBO_WINDOW + 1);
    assert_eq!(
        w.one(comment(
            TEAMMATE_PR,
            "Thanks, the new budget reads well",
            w.now()
        )),
        Ok(HitKind::Comment)
    );
}

#[test]
fn vote_flip_flop_counts_once() {
    let mut w = World::new();
    w.one(viewed(TEAMMATE_PR, NOW)).unwrap();
    assert_eq!(
        w.one(vote(pr(TEAMMATE_PR), Vote::WaitingForAuthor, NOW)),
        Ok(HitKind::WaitForAuthor)
    );
    assert_eq!(
        w.one(vote(pr(TEAMMATE_PR), Vote::Approved, NOW)),
        Err(Rejection::Repeat)
    );
    assert_eq!(
        w.one(vote(pr(TEAMMATE_PR), Vote::WaitingForAuthor, NOW)),
        Err(Rejection::Repeat)
    );
    // A vote on a new iteration (commit) is real review work again.
    let on_new_commit = Action::Vote {
        pr: pr(TEAMMATE_PR),
        vote: Vote::Approved,
        commit: Some("c0ffee".into()),
        at: NOW,
        origin: Origin::Sync,
    };
    assert_eq!(w.one(on_new_commit.clone()), Ok(HitKind::Approve));
    assert_eq!(w.one(on_new_commit), Err(Rejection::Repeat));
    assert_eq!(
        w.one(vote(pr(TEAMMATE_PR), Vote::NoVote, NOW)),
        Err(Rejection::NotAHit)
    );
}

#[test]
fn votes_need_opened_files_or_a_comment() {
    let mut w = World::new();
    assert_eq!(
        w.one(vote(pr(1), Vote::Approved, NOW)),
        Err(Rejection::NotEngaged)
    );
    // Opened in adoken (view_state) within 24 h.
    let opened = PrFacts {
        opened_at: Some(NOW - 2 * HOUR),
        ..pr(2)
    };
    assert_eq!(
        w.one(vote(opened, Vote::Approved, NOW)),
        Ok(HitKind::Approve)
    );
    let stale_open = PrFacts {
        opened_at: Some(NOW - 25 * HOUR),
        ..pr(3)
    };
    assert_eq!(
        w.one(vote(stale_open, Vote::Approved, NOW)),
        Err(Rejection::NotEngaged)
    );
    // An in-app comment is engagement too.
    let app_comment = Action::Comment {
        pr: pr(4),
        body: Some("Looks right to me, one nit below".into()),
        at: NOW,
        origin: Origin::App,
    };
    assert_eq!(w.one(app_comment), Ok(HitKind::Comment));
    assert_eq!(
        w.one(vote(pr(4), Vote::Approved, NOW)),
        Ok(HitKind::Approve)
    );
}

#[test]
fn own_pr_votes_and_views_do_not_count() {
    let mut w = World::new();
    let mine = PrFacts {
        opened_at: Some(NOW),
        ..my_pr(9)
    };
    assert_eq!(
        w.one(vote(mine.clone(), Vote::Approved, NOW)),
        Err(Rejection::OwnPr)
    );
    assert_eq!(
        w.one(Action::Viewed {
            pr: mine.clone(),
            at: NOW
        }),
        Err(Rejection::OwnPr)
    );
    // Resolving a reviewer's thread on your own PR is still review work.
    let resolve = Action::Resolve {
        pr: mine,
        others_commented: true,
        at: NOW,
        origin: Origin::Sync,
    };
    assert_eq!(w.one(resolve), Ok(HitKind::Resolve));
}

#[test]
fn nothing_counts_during_backfill() {
    let mut w = World::new();
    w.backfilling = true;
    assert_eq!(w.one(viewed(1, NOW)), Err(Rejection::Backfill));
    let merged = w.apply(vec![Action::Merged {
        pr: my_pr(5),
        at: NOW,
        i_approved: false,
    }]);
    assert!(merged.effects.is_empty(), "no K.O. overlay during backfill");
    assert_eq!(w.ken.take_journal().kos.len(), 1, "still on the fight card");
}

#[test]
fn sync_actions_must_be_fresh() {
    let mut w = World::new();
    w.one(viewed(TEAMMATE_PR, NOW - HOUR))
        .unwrap_or(HitKind::Viewed);
    let old = comment(
        TEAMMATE_PR,
        "An old comment seen late by sync",
        NOW - 16 * MINUTE,
    );
    assert_eq!(w.one(old), Err(Rejection::Stale));
    let fresh = comment(
        TEAMMATE_PR,
        "A fresh comment seen by sync",
        NOW - 14 * MINUTE,
    );
    assert_eq!(w.one(fresh), Ok(HitKind::Comment));
}

#[test]
fn comments_must_be_substantive_and_not_repeated() {
    let mut w = World::new();
    w.one(viewed(TEAMMATE_PR, NOW)).unwrap();
    assert_eq!(
        w.one(comment(TEAMMATE_PR, "  lgtm 👍  ", NOW)),
        Err(Rejection::ShortComment)
    );
    let body = "Please add a test for the retry path";
    assert_eq!(w.one(comment(TEAMMATE_PR, body, NOW)), Ok(HitKind::Comment));
    w.one(viewed(TEAMMATE_PR + 1, NOW)).unwrap();
    assert_eq!(
        w.one(comment(
            TEAMMATE_PR + 1,
            "please add a test   for the RETRY path",
            NOW
        )),
        Err(Rejection::DuplicateComment),
        "duplicate of the previous comment, even on another PR"
    );
    // The demo stream has no bodies: only the other rules apply.
    let bodiless = Action::Comment {
        pr: pr(TEAMMATE_PR),
        body: None,
        at: NOW,
        origin: Origin::Sync,
    };
    assert_eq!(w.one(bodiless), Ok(HitKind::Comment));
}

#[test]
fn resolving_needs_someone_elses_comment() {
    let mut w = World::new();
    w.one(viewed(TEAMMATE_PR, NOW)).unwrap();
    let resolve = |others_commented| Action::Resolve {
        pr: pr(TEAMMATE_PR),
        others_commented,
        at: NOW,
        origin: Origin::Sync,
    };
    assert_eq!(w.one(resolve(false)), Err(Rejection::NoOtherCommenter));
    assert_eq!(w.one(resolve(true)), Ok(HitKind::Resolve));
}

#[test]
fn check_is_side_effect_free() {
    let w = World::new();
    let before = w.ken.clone();
    let ctx = w.ctx();
    assert_eq!(w.ken.check(&viewed(1, NOW), &ctx), Ok(HitKind::Viewed));
    assert_eq!(w.ken, before);
}

// ── K.O. and badges ──────────────────────────────────────────────────────

#[test]
fn ko_on_my_merge_and_double_within_ten_minutes() {
    let mut w = World::new();
    let applied = w.apply(vec![
        Action::Merged {
            pr: my_pr(4821),
            at: NOW - 5 * MINUTE,
            i_approved: false,
        },
        Action::Merged {
            pr: my_pr(4822),
            at: NOW,
            i_approved: false,
        },
    ]);
    assert_eq!(
        applied.effects,
        vec![Effect::Ko {
            prs: vec![(4821, "PR 4821".into()), (4822, "PR 4822".into())],
            double: true,
        }]
    );
    // The same merge seen again is not another K.O.
    let again = w.apply(vec![Action::Merged {
        pr: my_pr(4821),
        at: NOW,
        i_approved: false,
    }]);
    assert!(again.effects.is_empty());
    let kos = w.ken.take_journal().kos;
    assert_eq!(kos.len(), 2);
    assert!(kos.iter().all(|k| k.double));
}

#[test]
fn old_merges_go_to_the_fight_card_only() {
    let mut w = World::new();
    let applied = w.apply(vec![
        Action::Merged {
            pr: my_pr(1),
            at: NOW - 25 * HOUR,
            i_approved: false,
        },
        Action::Merged {
            pr: my_pr(2),
            at: NOW - 30 * MINUTE,
            i_approved: false,
        },
    ]);
    assert_eq!(
        applied.effects,
        vec![Effect::Ko {
            prs: vec![(2, "PR 2".into())],
            double: false,
        }]
    );
    assert_eq!(w.ken.take_journal().kos.len(), 2);
}

#[test]
fn assist_when_a_pr_i_approved_merges() {
    let mut w = World::new();
    let applied = w.apply(vec![
        Action::Merged {
            pr: pr(4817),
            at: NOW,
            i_approved: true,
        },
        Action::Merged {
            pr: pr(4818),
            at: NOW,
            i_approved: false,
        },
    ]);
    assert_eq!(
        applied.effects,
        vec![Effect::Badge(Badge::Assist {
            pr: 4817,
            title: "PR 4817".into()
        })]
    );
    assert_eq!(
        Badge::Assist {
            pr: 4817,
            title: String::new()
        }
        .toast(),
        "Assist · !4817 merged"
    );
}

#[test]
fn first_strike_only_for_the_first_reviewer_within_an_hour() {
    let mut w = World::new();
    let fresh = PrFacts {
        published_at: Some(NOW - 20 * MINUTE),
        opened_at: Some(NOW - MINUTE),
        ..pr(4830)
    };
    let applied = w.apply(vec![vote(fresh.clone(), Vote::Approved, NOW)]);
    assert!(applied.effects.contains(&Effect::Badge(Badge::FirstStrike {
        pr: 4830,
        title: "PR 4830".into()
    })));

    let beaten = PrFacts {
        others_engaged_at: Some(NOW - 5 * MINUTE),
        ..fresh.clone()
    };
    let old = PrFacts {
        published_at: Some(NOW - 2 * HOUR),
        ..fresh.clone()
    };
    let draft = PrFacts {
        is_draft: true,
        ..fresh
    };
    for (id, facts) in [(1, beaten), (2, old), (3, draft)] {
        let facts = PrFacts { id, ..facts };
        let applied = w.apply(vec![vote(facts, Vote::Approved, NOW)]);
        assert!(
            !applied
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Badge(Badge::FirstStrike { .. }))),
            "pr {id}"
        );
    }
}

#[test]
fn comeback_when_my_failed_build_goes_green() {
    let mut w = World::new();
    let build = |succeeded| Action::Build {
        pr: my_pr(4821),
        succeeded,
        at: NOW,
    };
    assert!(w.apply(vec![build(true)]).effects.is_empty());
    assert!(w.apply(vec![build(false)]).effects.is_empty());
    let applied = w.apply(vec![build(true)]);
    assert!(matches!(
        applied.effects[..],
        [Effect::Badge(Badge::Comeback { pr: 4821, .. })]
    ));
}

#[test]
fn clean_sweep_at_most_once_a_day() {
    let mut w = World::new();
    let first = w.apply(vec![Action::InboxCleared { at: NOW }]);
    assert_eq!(first.effects, vec![Effect::Badge(Badge::CleanSweep)]);
    let again = w.apply(vec![Action::InboxCleared { at: NOW + HOUR }]);
    assert!(again.effects.is_empty());
    w.clock.advance(DAY);
    let tomorrow = w.apply(vec![Action::InboxCleared { at: w.now() }]);
    assert_eq!(tomorrow.effects, vec![Effect::Badge(Badge::CleanSweep)]);
}

// ── streak through the engine ────────────────────────────────────────────

#[test]
fn first_hit_of_a_working_day_extends_the_streak_once() {
    let mut w = World::new(); // Friday
    let applied = w.apply(vec![viewed(1, NOW), viewed(2, NOW + MINUTE)]);
    let streaks: Vec<_> = applied
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::Streak { .. }))
        .collect();
    assert_eq!(
        streaks,
        vec![&Effect::Streak {
            days: 1,
            milestone: false
        }]
    );
    // Saturday: counts as a hit, not a streak day.
    w.clock.advance(DAY);
    let saturday = w.apply(vec![viewed(3, w.now())]);
    assert!(
        !saturday
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Streak { .. }))
    );
    // Monday: 2 (the weekend didn't break it).
    w.clock.advance(2 * DAY);
    let monday = w.apply(vec![viewed(4, w.now())]);
    assert!(monday.effects.contains(&Effect::Streak {
        days: 2,
        milestone: false
    }));
}

#[test]
fn streak_days_follow_the_local_calendar() {
    // 23:30 UTC Friday is already Saturday in UTC+2: no streak day.
    let mut w = World::new();
    w.clock = FakeClock::new(NOW + 14 * HOUR, 2 * 3600);
    let applied = w.apply(vec![viewed(1, w.now())]);
    assert!(
        !applied
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Streak { .. }))
    );
}

#[test]
fn state_survives_a_json_round_trip() {
    let mut w = World::new();
    w.apply(vec![viewed(1, NOW), viewed(2, NOW)]);
    let json = serde_json::to_string(&w.ken).unwrap();
    let restored: Ken = serde_json::from_str(&json).unwrap();
    let mut original = w.ken.clone();
    original.take_journal();
    assert_eq!(restored, original);
}
