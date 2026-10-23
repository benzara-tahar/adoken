//! Burst control (spec 0007 FR-7, AC-9, AC-10; sound.md §5).
//!
//! - Any event repeated within [`DEBOUNCE`] is dropped.
//! - `notify.*`: the first event of a burst plays immediately (AC-9: within
//!   50 ms of its banner), then every notify event in the next
//!   [`NOTIFY_COOLDOWN`] is folded into it (FR-7: at most one sound per 2 s;
//!   AC-10: 5 events within 1 s → exactly 1 sound). The inbox count badge is
//!   the visual counterpart of the folded events.
//!
//! sound.md §5 sketched a 1.5 s hold-then-digest window; that delays every
//! notification sound by 1.5 s, which AC-9 forbids, so the spec's
//! play-first-then-coalesce rule wins. `notify.digest` stays the sound for
//! queued items released when a delivery window opens.

use std::collections::HashMap;

use time::{Duration, OffsetDateTime};

use crate::event::{Category, SoundEvent};
use crate::gate::Silenced;

pub const DEBOUNCE: Duration = Duration::milliseconds(120);
pub const NOTIFY_COOLDOWN: Duration = Duration::seconds(2);

#[derive(Debug, Default)]
pub struct Scheduler {
    last_by_event: HashMap<SoundEvent, OffsetDateTime>,
    last_notify: Option<OffsetDateTime>,
}

impl Scheduler {
    /// Record an event that passed the gate; `Ok` means play it now.
    pub fn admit(&mut self, event: SoundEvent, now: OffsetDateTime) -> Result<(), Silenced> {
        let previous = self.last_by_event.insert(event, now);
        if previous.is_some_and(|at| now - at < DEBOUNCE) {
            return Err(Silenced::Debounced);
        }
        if event.category() == Category::Notify {
            if self
                .last_notify
                .is_some_and(|at| now - at < NOTIFY_COOLDOWN)
            {
                return Err(Silenced::Coalesced);
            }
            self.last_notify = Some(now);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const T0: OffsetDateTime = datetime!(2026-09-25 10:00 UTC);

    fn ms(value: i64) -> OffsetDateTime {
        T0 + Duration::milliseconds(value)
    }

    /// AC-10.
    #[test]
    fn five_notify_events_within_a_second_make_one_sound() {
        let mut scheduler = Scheduler::default();
        let burst = [
            (0, SoundEvent::NotifyPrComment),
            (150, SoundEvent::NotifyMention),
            (400, SoundEvent::NotifyPrApproved),
            (700, SoundEvent::NotifyPrComment),
            (950, SoundEvent::NotifyPipelineFailed),
        ];
        let played: Vec<_> = burst
            .into_iter()
            .filter(|(at, event)| scheduler.admit(*event, ms(*at)).is_ok())
            .collect();
        assert_eq!(played, [(0, SoundEvent::NotifyPrComment)]);
    }

    #[test]
    fn cooldown_is_measured_from_the_last_sound_not_the_last_event() {
        let mut scheduler = Scheduler::default();
        assert!(scheduler.admit(SoundEvent::NotifyMention, ms(0)).is_ok());
        assert_eq!(
            scheduler.admit(SoundEvent::NotifyPrComment, ms(1_900)),
            Err(Silenced::Coalesced)
        );
        // 2 s after the sound (not after the folded event) the next plays.
        assert!(
            scheduler
                .admit(SoundEvent::NotifyPrApproved, ms(2_000))
                .is_ok()
        );
    }

    #[test]
    fn own_actions_are_debounced_not_coalesced() {
        let mut scheduler = Scheduler::default();
        assert!(scheduler.admit(SoundEvent::UiActionDone, ms(0)).is_ok());
        assert_eq!(
            scheduler.admit(SoundEvent::UiActionDone, ms(100)),
            Err(Silenced::Debounced)
        );
        assert!(scheduler.admit(SoundEvent::UiActionDone, ms(250)).is_ok());
        // A different ui event right away is fine, and ui never blocks notify.
        assert!(scheduler.admit(SoundEvent::UiError, ms(251)).is_ok());
        assert!(scheduler.admit(SoundEvent::NotifyMention, ms(252)).is_ok());
    }
}
