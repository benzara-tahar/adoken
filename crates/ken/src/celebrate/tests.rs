use super::*;
use adoken_model::StateCategory::{Completed, InProgress, Proposed, Removed, Resolved};

const T: Millis = 1_790_000_000_000;

fn on() -> Conditions {
    Conditions {
        window_active: true,
        ..Conditions::default()
    }
}

fn wi<'a>(
    id: i64,
    from: &'a str,
    fc: StateCategory,
    to: &'a str,
    tc: StateCategory,
) -> Trigger<'a> {
    Trigger::WorkItem(StateMove {
        id,
        from,
        from_category: fc,
        to,
        to_category: tc,
    })
}

fn full(tier: Tier) -> Decision {
    Decision::Celebrate {
        tier,
        show: Show::Full(Style::Calm),
    }
}

/// Fresh memory, default settings.
fn once(trigger: Trigger<'_>) -> Decision {
    decide(
        trigger,
        &CelebrationSettings::default(),
        on(),
        &mut Memory::default(),
        T,
    )
}

/// AC-1: every cell of §3.1's process table, plus the custom cases.
#[test]
fn tier_table_covers_the_stock_processes() {
    let step = full(Tier::Step);
    let milestone = full(Tier::Milestone);
    let none = Decision::None;
    let cells = [
        // Agile · User Story, Bug
        ("Active", InProgress, "Resolved", Resolved, step),
        ("Resolved", Resolved, "Closed", Completed, milestone),
        ("Active", InProgress, "Closed", Completed, milestone),
        ("Active", InProgress, "New", Proposed, none),
        ("New", Proposed, "Active", InProgress, none),
        ("Active", InProgress, "Removed", Removed, none),
        // Scrum · Product Backlog Item, Bug (Q-3: Committed is a Milestone)
        ("Approved", Proposed, "Committed", InProgress, milestone),
        ("Approved", Proposed, "Done", Completed, milestone),
        ("Committed", InProgress, "Done", Completed, milestone),
        ("Committed", InProgress, "New", Proposed, none),
        ("New", Proposed, "Approved", Proposed, none),
        ("Committed", InProgress, "Removed", Removed, none),
        // Scrum · Task
        ("In Progress", InProgress, "Done", Completed, milestone),
        ("In Progress", InProgress, "To Do", Proposed, none),
        ("To Do", Proposed, "In Progress", InProgress, none),
        // Basic · Issue, Task
        ("Doing", InProgress, "Done", Completed, milestone),
        ("Doing", InProgress, "To Do", Proposed, none),
        ("To Do", Proposed, "Doing", InProgress, none),
        // CMMI · Requirement, Bug
        ("Active", InProgress, "Resolved", Resolved, step),
        ("Resolved", Resolved, "Closed", Completed, milestone),
        ("Active", InProgress, "Proposed", Proposed, none),
        ("Proposed", Proposed, "Active", InProgress, none),
        // The owner's custom process
        ("Active", InProgress, "Code Review", InProgress, step),
        (
            "Code Review",
            InProgress,
            "Committed",
            InProgress,
            milestone,
        ),
        ("Code Review", InProgress, "Done", Completed, milestone),
        ("Code Review", InProgress, "Active", InProgress, none),
    ];
    for (from, fc, to, tc, expected) in cells {
        assert_eq!(once(wi(1, from, fc, to, tc)), expected, "{from} → {to}");
    }
    // A custom resolved state, a sloppy name, a custom review state that
    // its metadata files under Proposed, and Removed even when listed.
    assert_eq!(
        once(wi(1, "Active", InProgress, "Ready for QA", Resolved)),
        step
    );
    assert_eq!(
        once(wi(1, "Active", InProgress, "done ", InProgress)),
        milestone
    );
    assert_eq!(
        once(wi(1, "Active", InProgress, "Code Review", Proposed)),
        none
    );
    let mut listed = CelebrationSettings::default();
    listed.milestone_states.push("Removed".into());
    assert_eq!(listed.tier("Removed", Removed), None);
    // No metadata: the stock-name fallback.
    assert_eq!(
        once(wi(
            1,
            "Active",
            StateCategory::of_state("Active"),
            "Closed",
            StateCategory::of_state("Closed")
        )),
        milestone
    );
    assert_eq!(
        once(wi(
            1,
            "Active",
            StateCategory::of_state("Active"),
            "Code Review",
            StateCategory::of_state("Code Review")
        )),
        step
    );
}

/// AC-2 (decision part): backward and sideways moves never celebrate and
/// keep the plain write sound.
#[test]
fn backward_and_sideways_moves_stay_plain() {
    for (from, fc, to, tc) in [
        ("Done", Completed, "Active", InProgress),
        ("Code Review", InProgress, "Active", InProgress),
        ("Closed", Completed, "Resolved", Resolved),
        ("New", Proposed, "Approved", Proposed),
        ("Committed", InProgress, "Resolved", Resolved),
    ] {
        let decision = once(wi(7, from, fc, to, tc));
        assert_eq!(decision, Decision::None, "{from} → {to}");
        assert_eq!(decision.sound(), Sound::Plain);
    }
}

/// AC-3: the 24 h item memory.
#[test]
fn an_item_celebrates_each_tier_once_a_day() {
    let settings = CelebrationSettings::default();
    let mut memory = Memory::default();
    let mut at = |trigger, now| decide(trigger, &settings, on(), &mut memory, now);
    assert_eq!(
        at(wi(42, "Active", InProgress, "Done", Completed), T),
        full(Tier::Milestone)
    );
    assert_eq!(
        at(wi(42, "Done", Completed, "Active", InProgress), T + MINUTE),
        Decision::None
    );
    assert_eq!(
        at(
            wi(42, "Active", InProgress, "Done", Completed),
            T + 2 * MINUTE
        ),
        Decision::None
    );
    assert_eq!(
        at(
            wi(42, "Active", InProgress, "Code Review", InProgress),
            T + 3 * MINUTE
        ),
        Decision::None,
        "a Milestone was celebrated within 24 h"
    );
    assert_eq!(
        at(
            wi(42, "Active", InProgress, "Done", Completed),
            T + ITEM_MEMORY_MS + MINUTE
        ),
        full(Tier::Milestone)
    );

    let mut memory = Memory::default();
    let mut at = |trigger, now| decide(trigger, &settings, on(), &mut memory, now);
    assert_eq!(
        at(wi(43, "Active", InProgress, "Code Review", InProgress), T),
        full(Tier::Step)
    );
    assert_eq!(
        at(
            wi(43, "Code Review", InProgress, "Committed", InProgress),
            T + 10 * MINUTE
        ),
        full(Tier::Milestone)
    );
    assert_eq!(
        at(
            wi(43, "Committed", InProgress, "Done", Completed),
            T + 20 * MINUTE
        ),
        Decision::None
    );
}

/// AC-9 (decision part) and FR-6: the K.O. is the celebration of my own
/// merge; someone else's merge is a Milestone.
#[test]
fn the_ko_replaces_the_pr_celebration() {
    let mine = once(Trigger::PullRequest { id: 4821, ko: true });
    assert_eq!(mine, Decision::Ko);
    assert_eq!(mine.sound(), Sound::KoOnly);
    let theirs = decide(
        Trigger::PullRequest {
            id: 4822,
            ko: false,
        },
        &CelebrationSettings::default(),
        Conditions {
            ken_arcade: true,
            ..on()
        },
        &mut Memory::default(),
        T,
    );
    assert_eq!(
        theirs,
        Decision::Celebrate {
            tier: Tier::Milestone,
            show: Show::Full(Style::Arcade)
        }
    );
    assert_eq!(theirs.sound(), Sound::CelebrateMilestone);
}

/// AC-10 (decision part): Reduce Motion shows the static glyph and keeps
/// the celebration sound; it isn't counted by the rate limits.
#[test]
fn reduce_motion_is_static_with_its_sound() {
    let settings = CelebrationSettings::default();
    let reduced = Conditions {
        reduce_motion: true,
        ken_arcade: true,
        ..on()
    };
    let mut memory = Memory::default();
    for i in 0..8 {
        let decision = decide(
            Trigger::PullRequest { id: i, ko: false },
            &settings,
            reduced,
            &mut memory,
            T + i,
        );
        assert_eq!(
            decision,
            Decision::Celebrate {
                tier: Tier::Milestone,
                show: Show::Static { limited: false }
            }
        );
        assert_eq!(decision.sound(), Sound::CelebrateMilestone);
    }
    let step = decide(
        wi(1, "Active", InProgress, "Resolved", Resolved),
        &settings,
        reduced,
        &mut memory,
        T,
    );
    assert_eq!(step.sound(), Sound::CelebrateStep);
}

/// AC-11: one full effect per 1.5 s, five per rolling 10 min.
#[test]
fn rate_limits_fall_back_to_the_static_glyph() {
    let settings = CelebrationSettings::default();
    let milestone = |memory: &mut Memory, id: i64, now: Millis| {
        decide(
            wi(id, "Active", InProgress, "Done", Completed),
            &settings,
            on(),
            memory,
            now,
        )
    };
    let limited = Decision::Celebrate {
        tier: Tier::Milestone,
        show: Show::Static { limited: true },
    };
    let mut memory = Memory::default();
    assert_eq!(milestone(&mut memory, 1, T), full(Tier::Milestone));
    assert_eq!(milestone(&mut memory, 2, T + 1_000), limited);
    assert_eq!(limited.sound(), Sound::Plain);
    assert_eq!(milestone(&mut memory, 3, T + 2_000), full(Tier::Milestone));

    let mut memory = Memory::default();
    let shows: Vec<Decision> = (0..7)
        .map(|i| milestone(&mut memory, 10 + i, T + i * 2_000))
        .collect();
    assert_eq!(shows[..5], [full(Tier::Milestone); 5]);
    assert_eq!(shows[5..], [limited; 2]);
    assert_eq!(
        milestone(&mut memory, 99, T + RATE_WINDOW_MS + 1_000),
        full(Tier::Milestone),
        "the first one left the 10 min window"
    );
    // The item memory applies before the limits: a repeat doesn't use one.
    let mut memory = Memory::default();
    assert_eq!(milestone(&mut memory, 5, T), full(Tier::Milestone));
    assert_eq!(milestone(&mut memory, 5, T + 2_000), Decision::None);
    assert_eq!(milestone(&mut memory, 6, T + 2_000), full(Tier::Milestone));
}

/// AC-12 (spec 0025 AC-7): Quiet mode hides everything; quiet hours force
/// the calm style.
#[test]
fn quiet_mode_and_quiet_hours() {
    let settings = CelebrationSettings::default();
    let silenced = decide(
        wi(1, "Active", InProgress, "Done", Completed),
        &settings,
        Conditions {
            quiet_mode: true,
            ..on()
        },
        &mut Memory::default(),
        T,
    );
    assert_eq!(silenced, Decision::None);
    assert_eq!(silenced.sound(), Sound::Plain);
    let quiet = decide(
        wi(1, "Active", InProgress, "Done", Completed),
        &settings,
        Conditions {
            quiet_hours: true,
            ken_arcade: true,
            ..on()
        },
        &mut Memory::default(),
        T,
    );
    assert_eq!(quiet, full(Tier::Milestone), "calm style, no seal");
    // Quiet mode suppresses even a K.O. handoff and the preview.
    for trigger in [Trigger::PullRequest { id: 1, ko: true }, Trigger::Preview] {
        let decision = decide(
            trigger,
            &settings,
            Conditions {
                quiet_mode: true,
                ..on()
            },
            &mut Memory::default(),
            T,
        );
        assert_eq!(decision, Decision::None);
    }
}

/// AC-13: defaults, switches, live lists, per-key fallback.
#[test]
fn settings_parse_with_per_key_fallback() {
    let (defaults, issues) = CelebrationSettings::from_settings_toml("[ken]\nenabled = true\n");
    assert_eq!(defaults, CelebrationSettings::default());
    assert!(defaults.enabled && defaults.pull_requests && defaults.work_items);
    assert!(issues.is_empty());

    let (off, _) = CelebrationSettings::from_settings_toml("[celebrations]\nenabled = false\n");
    let pr = Trigger::PullRequest { id: 1, ko: false };
    let move_ = wi(1, "Active", InProgress, "Done", Completed);
    for trigger in [pr, move_] {
        assert_eq!(
            decide(trigger, &off, on(), &mut Memory::default(), T),
            Decision::None
        );
    }
    let (no_items, _) =
        CelebrationSettings::from_settings_toml("[celebrations]\nwork_items = false\n");
    assert_eq!(
        decide(move_, &no_items, on(), &mut Memory::default(), T),
        Decision::None
    );
    assert!(decide(pr, &no_items, on(), &mut Memory::default(), T).celebrates());

    let (qa, _) =
        CelebrationSettings::from_settings_toml("[celebrations]\nstep_states = [\"QA\"]\n");
    assert_eq!(
        decide(
            wi(1, "Active", InProgress, "QA", InProgress),
            &qa,
            on(),
            &mut Memory::default(),
            T
        ),
        full(Tier::Step)
    );
    assert_eq!(
        qa.tier("Code Review", InProgress),
        None,
        "the list replaced the default"
    );

    let (bad, issues) = CelebrationSettings::from_settings_toml(
        "[celebrations]\nmilestone_states = \"Done\"\nenabled = \"yes\"\nstep_states = [\"QA\", 3]\nconfetti = 1\n",
    );
    assert_eq!(
        bad.milestone_states,
        CelebrationSettings::default().milestone_states
    );
    assert!(bad.enabled);
    assert_eq!(bad.step_states, ["QA"]);
    assert_eq!(issues.len(), 4, "{issues:?}");
    assert!(issues.iter().any(|i| i.contains("milestone_states")));

    let (both, _) = CelebrationSettings::from_settings_toml(
        "[celebrations]\nstep_states = [\"Done\"]\nmilestone_states = [\"Done\"]\n",
    );
    assert_eq!(both.tier("Done", Completed), Some(Tier::Milestone));

    // Round trip through the table we write.
    let custom = CelebrationSettings {
        enabled: false,
        step_states: vec!["QA".into()],
        ..CelebrationSettings::default()
    };
    let text = toml::to_string(&toml::Table::from_iter([(
        SETTINGS_TABLE.to_owned(),
        toml::Value::Table(custom.to_table()),
    )]))
    .unwrap();
    assert_eq!(CelebrationSettings::from_settings_toml(&text).0, custom);
}

#[test]
fn summary_reads_like_the_copy() {
    assert_eq!(
        CelebrationSettings::default().summary(),
        "Small: Code Review, In Review, Ready for Review, Review, and resolved states. \
         Big: Committed, Done, and completed states."
    );
}

/// AC-14: a confirmation landing while the window isn't key shows nothing
/// and remembers nothing for later.
#[test]
fn background_window_shows_nothing() {
    let mut memory = Memory::default();
    let settings = CelebrationSettings::default();
    let away = Conditions {
        window_active: false,
        ..Conditions::default()
    };
    let trigger = wi(1, "Active", InProgress, "Done", Completed);
    assert_eq!(
        decide(trigger, &settings, away, &mut memory, T),
        Decision::None
    );
    // Refocused: the next confirmed write still celebrates.
    assert!(decide(trigger, &settings, on(), &mut memory, T + 1).celebrates());
}

/// AC-18 (decision part): the preview ignores the master switch and the
/// rate limits, and isn't counted by them.
#[test]
fn preview_ignores_the_master_switch_and_the_limits() {
    let (off, _) = CelebrationSettings::from_settings_toml("[celebrations]\nenabled = false\n");
    let mut memory = Memory::default();
    for i in 0..10 {
        assert_eq!(
            decide(Trigger::Preview, &off, on(), &mut memory, T + i),
            full(Tier::Milestone)
        );
    }
    let reduced = Conditions {
        reduce_motion: true,
        ..on()
    };
    assert_eq!(
        decide(Trigger::Preview, &off, reduced, &mut memory, T),
        Decision::Celebrate {
            tier: Tier::Milestone,
            show: Show::Static { limited: false }
        }
    );
    let settings = CelebrationSettings::default();
    assert_eq!(
        decide(
            Trigger::PullRequest { id: 1, ko: false },
            &settings,
            on(),
            &mut memory,
            T + 20
        ),
        full(Tier::Milestone),
        "previews used no rate-limit slot"
    );
}
