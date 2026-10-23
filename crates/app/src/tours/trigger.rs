//! The first-use trigger (spec 0020 FR-2) as a pure decision over a
//! snapshot of the window, so every gate is testable with an injected
//! clock (AC-2).

use std::time::Duration;

/// FR-2 item 6: no input for this long (the palette tour: 0 s).
pub const IDLE: Duration = Duration::from_secs(1);
/// FR-2 item 7: no tour ended this recently (Q-3).
pub const COOLDOWN: Duration = Duration::from_secs(180);
/// FR-2 item 8.
pub const MIN_VISIBLE_STEPS: usize = 2;

/// What the window looks like right now, for one candidate tour.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// `[tours] auto_start`.
    pub auto_start: bool,
    /// The tour's id is in `[tours] seen`.
    pub seen: bool,
    /// The post-onboarding hint shows (item 1).
    pub hint: bool,
    /// Something other than the view owns the keyboard (item 4).
    pub overlay: bool,
    /// The main window is key and visible (item 5).
    pub window_active: bool,
    /// Since the window went quiet: the later of the last input and the
    /// moment every other gate opened; `None` = none yet.
    pub since_input: Option<Duration>,
    /// Since the last tour ended; `None` = none yet this session.
    pub since_tour_ended: Option<Duration>,
    /// Steps whose anchor is visible (items 3 and 8).
    pub visible_steps: usize,
    /// Required quiet time ([`IDLE`], or zero for the palette).
    pub idle: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Show it now.
    Start,
    /// Every gate but time holds: check again after this long.
    Wait(Duration),
    /// A gate that only a change in the window can open.
    Blocked,
    /// Never by itself (switched off, or already seen).
    Never,
}

pub fn decide(s: &Snapshot) -> Decision {
    if !s.auto_start || s.seen {
        return Decision::Never;
    }
    if s.hint || s.overlay || !s.window_active || s.visible_steps < MIN_VISIBLE_STEPS {
        return Decision::Blocked;
    }
    let remaining = |since: Option<Duration>, needed: Duration| {
        since.map_or(Duration::ZERO, |since| needed.saturating_sub(since))
    };
    let wait = remaining(s.since_tour_ended, COOLDOWN).max(remaining(s.since_input, s.idle));
    if wait.is_zero() {
        Decision::Start
    } else {
        Decision::Wait(wait)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Snapshot {
        Snapshot {
            auto_start: true,
            seen: false,
            hint: false,
            overlay: false,
            window_active: true,
            since_input: Some(Duration::from_secs(5)),
            since_tour_ended: None,
            visible_steps: 5,
            idle: IDLE,
        }
    }

    #[test]
    fn starts_when_every_gate_holds() {
        assert_eq!(decide(&ready()), Decision::Start);
        assert_eq!(
            decide(&Snapshot {
                since_input: None,
                ..ready()
            }),
            Decision::Start
        );
    }

    #[test]
    fn off_or_seen_never_starts() {
        for s in [
            Snapshot {
                auto_start: false,
                ..ready()
            },
            Snapshot {
                seen: true,
                ..ready()
            },
        ] {
            assert_eq!(decide(&s), Decision::Never);
        }
    }

    #[test]
    fn hint_overlay_inactive_window_and_one_anchor_block() {
        for s in [
            Snapshot {
                hint: true,
                ..ready()
            },
            Snapshot {
                overlay: true,
                ..ready()
            },
            Snapshot {
                window_active: false,
                ..ready()
            },
            Snapshot {
                visible_steps: 1,
                ..ready()
            },
            // Inbox zero: no list anchor, nothing to show yet.
            Snapshot {
                visible_steps: 0,
                ..ready()
            },
        ] {
            assert_eq!(decide(&s), Decision::Blocked, "{s:?}");
        }
    }

    #[test]
    fn a_key_half_a_second_ago_waits_until_one_second_after_it() {
        let s = Snapshot {
            since_input: Some(Duration::from_millis(500)),
            ..ready()
        };
        assert_eq!(decide(&s), Decision::Wait(Duration::from_millis(500)));
        let palette = Snapshot {
            idle: Duration::ZERO,
            since_input: Some(Duration::ZERO),
            ..ready()
        };
        assert_eq!(decide(&palette), Decision::Start);
    }

    #[test]
    fn cooldown_after_a_tour_ends() {
        // Today's tour ended at T; the Inbox shows at T + 60 s.
        let at_60 = Snapshot {
            since_tour_ended: Some(Duration::from_secs(60)),
            ..ready()
        };
        assert_eq!(decide(&at_60), Decision::Wait(Duration::from_secs(120)));
        // T + 181 s: the Inbox tour starts.
        let at_181 = Snapshot {
            since_tour_ended: Some(Duration::from_secs(181)),
            ..ready()
        };
        assert_eq!(decide(&at_181), Decision::Start);
        // The cooldown applies to the palette's 0 s idle too.
        let palette = Snapshot {
            idle: Duration::ZERO,
            ..at_60
        };
        assert!(matches!(decide(&palette), Decision::Wait(_)));
    }
}
