//! Whether an event may play right now, and how loud (docs/design/sound.md
//! §1 gain, §2.2 priority, §4 matrix; spec 0005 FR-8; spec 0025 FR-8 and
//! FR-13).
//!
//! Fixed rules since spec 0025 removed their options: nothing plays while
//! adoken is in the background, and during quiet hours the only sounds that
//! happen to you are urgent (P0) notifications, and only when delivery
//! allows (not queued, not the launch backlog); feedback for your own
//! actions still plays (sound.md §4). Quiet mode silences everything.
//!
//! Reduce motion never silences sounds: ken.md §6 keeps them ("Sounds still
//! play if enabled"), and every sound already has a visual counterpart, so
//! the motion setting has no input here.

use time::OffsetDateTime;

use crate::event::{Category, SoundEvent};
use crate::settings::SoundSettings;

/// Notification priority from the notification center (spec 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum NotifyPriority {
    /// Never plays.
    Low,
    #[default]
    Normal,
    High,
    /// Gain × 1.2; the only sound that plays during quiet hours.
    Urgent,
}

/// Everything outside the sound settings that decides a play. The caller
/// (notify crate, app) fills it; the engine never reads clocks or OS state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayContext {
    pub now: OffsetDateTime,
    /// Spec 0005 quiet hours active.
    pub quiet_hours: bool,
    /// Outside a delivery window: notify items are queued (one
    /// `notify.digest` plays when the window opens).
    pub queued: bool,
    pub window_focused: bool,
    /// First 3 s after launch, initial sync, or backfill: backlog never plays.
    pub backlog: bool,
    /// Arcade (Ken) layer on; `ken.*` requires it.
    pub arcade_mode: bool,
    /// Quiet mode (spec 0025 FR-8): nothing plays.
    pub quiet_mode: bool,
    /// Only meaningful for `notify.*`.
    pub priority: NotifyPriority,
}

impl PlayContext {
    /// A focused, normal-priority moment with no gating conditions.
    pub fn at(now: OffsetDateTime) -> Self {
        Self {
            now,
            quiet_hours: false,
            queued: false,
            window_focused: true,
            backlog: false,
            arcade_mode: false,
            quiet_mode: false,
            priority: NotifyPriority::Normal,
        }
    }
}

/// Why nothing played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Silenced {
    /// Master toggle off.
    Disabled,
    /// Quiet mode is on.
    QuietMode,
    QuietHours,
    /// Held outside a delivery window.
    Queued,
    Unfocused,
    Backlog,
    LowPriority,
    ArcadeOff,
    /// Same event within the debounce window.
    Debounced,
    /// Folded into the notify burst that already sounded.
    Coalesced,
}

/// Gate one event. `Ok` carries `master × category × event` (× 1.2 for
/// urgent), clamped to 1.0; the pack's per-file gain is applied at playback.
pub fn decide(
    settings: &SoundSettings,
    event: SoundEvent,
    context: &PlayContext,
) -> Result<f32, Silenced> {
    if !settings.enabled {
        return Err(Silenced::Disabled);
    }
    if context.quiet_mode {
        return Err(Silenced::QuietMode);
    }
    if !context.window_focused {
        return Err(Silenced::Unfocused);
    }
    let category = event.category();
    let urgent = category == Category::Notify && context.priority == NotifyPriority::Urgent;
    match category {
        Category::Notify => {
            if context.priority == NotifyPriority::Low {
                return Err(Silenced::LowPriority);
            }
            if context.quiet_hours && !urgent {
                return Err(Silenced::QuietHours);
            }
            if context.backlog {
                return Err(Silenced::Backlog);
            }
            if context.queued {
                return Err(Silenced::Queued);
            }
        }
        Category::Ken => {
            if !context.arcade_mode {
                return Err(Silenced::ArcadeOff);
            }
            if context.quiet_hours {
                return Err(Silenced::QuietHours);
            }
            if context.backlog {
                return Err(Silenced::Backlog);
            }
        }
        // Your own actions: quiet hours are about what happens to you.
        Category::Ui | Category::Onboarding => {}
    }
    let urgent_boost = if urgent { 1.2 } else { 1.0 };
    let gain = settings.master * category.gain() * event.gain() * urgent_boost;
    if gain <= 0.0 {
        return Err(Silenced::Disabled);
    }
    Ok(gain.min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const NOW: OffsetDateTime = datetime!(2026-09-25 22:00 UTC);

    fn enabled() -> SoundSettings {
        SoundSettings {
            enabled: true,
            ..SoundSettings::default()
        }
    }

    fn context() -> PlayContext {
        PlayContext::at(NOW)
    }

    #[test]
    fn off_by_default_and_quiet_mode_silences_every_category() {
        assert_eq!(
            decide(
                &SoundSettings::default(),
                SoundEvent::NotifyMention,
                &context()
            ),
            Err(Silenced::Disabled)
        );
        let quiet = PlayContext {
            quiet_mode: true,
            arcade_mode: true,
            priority: NotifyPriority::Urgent,
            ..context()
        };
        for event in [
            SoundEvent::UiActionDone,
            SoundEvent::UiCelebrateMilestone,
            SoundEvent::NotifyMention,
            SoundEvent::KenKo,
            SoundEvent::OnboardingComplete,
        ] {
            assert_eq!(
                decide(&enabled(), event, &quiet),
                Err(Silenced::QuietMode),
                "{event}"
            );
            assert!(
                decide(
                    &enabled(),
                    event,
                    &PlayContext {
                        quiet_mode: false,
                        ..quiet
                    }
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn gain_multiplies_master_category_event() {
        let gain = decide(&enabled(), SoundEvent::NotifyMention, &context()).unwrap();
        assert!((gain - 0.6 * 0.7 * 1.0).abs() < 1e-6);
        let urgent = PlayContext {
            priority: NotifyPriority::Urgent,
            ..context()
        };
        let boosted = decide(&enabled(), SoundEvent::NotifyMention, &urgent).unwrap();
        assert!((boosted - gain * 1.2).abs() < 1e-6);
        // Urgent only boosts notifications.
        let done = decide(&enabled(), SoundEvent::UiActionDone, &context()).unwrap();
        assert_eq!(
            decide(&enabled(), SoundEvent::UiActionDone, &urgent),
            Ok(done)
        );

        let loud = SoundSettings {
            master: 1.0,
            ..enabled()
        };
        let gain = decide(&loud, SoundEvent::NotifyMention, &urgent).unwrap();
        assert!((gain - 0.7 * 1.2).abs() < 1e-6);
        let silent = SoundSettings {
            master: 0.0,
            ..enabled()
        };
        assert_eq!(
            decide(&silent, SoundEvent::NotifyMention, &context()),
            Err(Silenced::Disabled)
        );
    }

    /// Spec 0025 FR-13, sound.md §4: during quiet hours only urgent
    /// notification sounds play, and only if delivery allows; your own
    /// actions still sound.
    #[test]
    fn quiet_hours_let_only_urgent_notifications_through() {
        let quiet = PlayContext {
            quiet_hours: true,
            arcade_mode: true,
            ..context()
        };
        for event in [SoundEvent::NotifyMention, SoundEvent::KenKo] {
            assert_eq!(
                decide(&enabled(), event, &quiet),
                Err(Silenced::QuietHours),
                "{event}"
            );
        }
        assert!(decide(&enabled(), SoundEvent::UiCelebrateStep, &quiet).is_ok());
        let urgent = PlayContext {
            priority: NotifyPriority::Urgent,
            ..quiet
        };
        assert!(decide(&enabled(), SoundEvent::NotifyMention, &urgent).is_ok());
        assert_eq!(
            decide(&enabled(), SoundEvent::KenKo, &urgent),
            Err(Silenced::QuietHours),
            "urgency is a notification's"
        );
        let held = PlayContext {
            queued: true,
            ..urgent
        };
        assert_eq!(
            decide(&enabled(), SoundEvent::NotifyMention, &held),
            Err(Silenced::Queued)
        );
    }

    #[test]
    fn low_priority_backlog_and_queued_notify_never_play() {
        let settings = enabled();
        for (context, reason) in [
            (
                PlayContext {
                    priority: NotifyPriority::Low,
                    ..context()
                },
                Silenced::LowPriority,
            ),
            (
                PlayContext {
                    backlog: true,
                    ..context()
                },
                Silenced::Backlog,
            ),
            (
                PlayContext {
                    queued: true,
                    ..context()
                },
                Silenced::Queued,
            ),
        ] {
            assert_eq!(
                decide(&settings, SoundEvent::NotifyPrComment, &context),
                Err(reason)
            );
        }
        // Your own actions still sound during the backlog.
        let backlog = PlayContext {
            backlog: true,
            ..context()
        };
        assert!(decide(&settings, SoundEvent::UiActionDone, &backlog).is_ok());
    }

    /// Spec 0025 FR-13: nothing plays while adoken is in the background.
    #[test]
    fn background_window_is_silent() {
        let unfocused = PlayContext {
            window_focused: false,
            arcade_mode: true,
            priority: NotifyPriority::Urgent,
            ..context()
        };
        for event in [
            SoundEvent::UiActionDone,
            SoundEvent::NotifyMention,
            SoundEvent::KenStreak,
            SoundEvent::OnboardingComplete,
        ] {
            assert_eq!(
                decide(&enabled(), event, &unfocused),
                Err(Silenced::Unfocused),
                "{event}"
            );
        }
    }

    #[test]
    fn ken_needs_arcade_mode() {
        let settings = enabled();
        assert_eq!(
            decide(&settings, SoundEvent::KenKo, &context()),
            Err(Silenced::ArcadeOff)
        );
        let arcade = PlayContext {
            arcade_mode: true,
            ..context()
        };
        assert!(decide(&settings, SoundEvent::KenKo, &arcade).is_ok());
    }
}
