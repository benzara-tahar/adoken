//! The event → sound map (docs/design/sound.md §2). One table, one row per
//! event: id, category, gain, voice priority, duration limit. Every event
//! plays once sounds are on (spec 0025 FR-13: no per-event switches).

use std::fmt;
use std::time::Duration;

/// Which gating rules apply, and the event's fixed mix level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// Feedback for your own actions.
    Ui,
    /// Things that happened to you.
    Notify,
    /// Arcade layer (requires Arcade mode, ken.md); Arcade chip tones.
    Ken,
    Onboarding,
}

impl Category {
    /// Fixed category gain (tokens.md §8; no per-category volumes since
    /// spec 0025 FR-13).
    pub fn gain(self) -> f32 {
        match self {
            Self::Ui => 0.35,
            Self::Notify => 0.7,
            Self::Ken => 0.6,
            Self::Onboarding => 0.5,
        }
    }
}

macro_rules! events {
    ($($variant:ident => $id:literal, $category:ident, $gain:literal, $priority:literal, $max_ms:literal;)+) => {
        /// Every sound-triggering event (sound.md §2).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum SoundEvent {
            $($variant,)+
        }

        impl SoundEvent {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Stable id for logs and pack issues.
            pub fn id(self) -> &'static str {
                match self { $(Self::$variant => $id,)+ }
            }

            pub fn category(self) -> Category {
                match self { $(Self::$variant => Category::$category,)+ }
            }

            /// Per-event gain (the "Gain" column).
            pub fn gain(self) -> f32 {
                match self { $(Self::$variant => $gain,)+ }
            }

            /// Voice-stealing priority (§5); higher wins.
            pub fn priority(self) -> u8 {
                match self { $(Self::$variant => $priority,)+ }
            }

            /// Longest allowed clip (§6 validation).
            pub fn max_duration(self) -> Duration {
                match self { $(Self::$variant => Duration::from_millis($max_ms),)+ }
            }
        }
    };
}

events! {
    UiActionDone => "ui.action_done", Ui, 0.8, 2, 2000;
    UiCelebrateStep => "ui.celebrate_step", Ui, 0.8, 2, 2000;
    UiCelebrateMilestone => "ui.celebrate_milestone", Ui, 0.9, 2, 2000;
    UiError => "ui.error", Ui, 0.9, 4, 2000;
    NotifyMention => "notify.mention", Notify, 1.0, 3, 2000;
    NotifyReviewRequested => "notify.review_requested", Notify, 0.9, 3, 2000;
    NotifyPrComment => "notify.pr_comment", Notify, 0.7, 3, 2000;
    NotifyPrApproved => "notify.pr_approved", Notify, 0.8, 3, 2000;
    NotifyPrChanges => "notify.pr_changes", Notify, 0.8, 3, 2000;
    NotifyPipelineFailed => "notify.pipeline_failed", Notify, 0.9, 3, 2000;
    NotifyWorkAssigned => "notify.work_assigned", Notify, 0.7, 3, 2000;
    NotifyDigest => "notify.digest", Notify, 0.8, 3, 2000;
    KenComboStep => "ken.combo_step", Ken, 0.6, 2, 2000;
    KenComboFinish => "ken.combo_finish", Ken, 0.7, 2, 3000;
    KenKo => "ken.ko", Ken, 1.0, 3, 3000;
    KenStreak => "ken.streak", Ken, 0.7, 2, 2000;
    KenFirstStrike => "ken.first_strike", Ken, 0.6, 2, 2000;
    KenCleanSweep => "ken.clean_sweep", Ken, 0.6, 2, 2000;
    OnboardingComplete => "onboarding.complete", Onboarding, 0.8, 2, 4000;
}

impl SoundEvent {
    /// `ken.ko` attenuates every other voice by 6 dB for its length (§5).
    pub fn ducks_others(self) -> bool {
        self == Self::KenKo
    }
}

impl fmt::Display for SoundEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_sound_md() {
        use SoundEvent::*;
        let mut ids: Vec<&str> = SoundEvent::ALL.iter().map(|event| event.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SoundEvent::ALL.len(), "ids are unique");
        for &event in SoundEvent::ALL {
            assert!((0.0..=1.0).contains(&event.gain()), "{event}");
        }
        assert_eq!(UiError.priority(), 4, "errors outrank everything");
        assert_eq!(KenKo.max_duration(), Duration::from_secs(3));
        assert_eq!(OnboardingComplete.max_duration(), Duration::from_secs(4));
        assert!(KenKo.ducks_others() && !NotifyMention.ducks_others());
    }
}
