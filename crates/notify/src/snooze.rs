//! Snooze menu choices (flow step 4, FR-5).

use adoken_model::Millis;
use adoken_model::time::HOUR;
use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::schedule::{LocalTime, TimeOfDay, local_to_utc};

/// Default "morning" when no working-hours start is set (0006 settings).
pub const DEFAULT_MORNING: TimeOfDay = TimeOfDay::hm(9, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnoozeChoice {
    OneHour,
    ThreeHours,
    TomorrowMorning,
    NextMonday,
    UntilChanged,
    Until(Millis),
}

/// Resolved snooze stored on a notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Snooze {
    Until(Millis),
    /// Resurfaces on the next non-self-caused event for the subject.
    UntilChanged,
}

impl SnoozeChoice {
    /// Menu order; `Until` (custom) is opened separately.
    pub const MENU: [Self; 5] = [
        Self::OneHour,
        Self::ThreeHours,
        Self::TomorrowMorning,
        Self::NextMonday,
        Self::UntilChanged,
    ];

    pub fn label(self, morning: TimeOfDay) -> String {
        match self {
            Self::OneHour => "1 hour".into(),
            Self::ThreeHours => "3 hours".into(),
            Self::TomorrowMorning => format!("Tomorrow {morning}"),
            Self::NextMonday => format!("Monday {morning}"),
            Self::UntilChanged => "Until it changes".into(),
            Self::Until(_) => "Custom date/time".into(),
        }
    }

    pub fn resolve(self, now: Millis, morning: TimeOfDay, clock: &dyn Clock) -> Snooze {
        let today = LocalTime::of(now, clock);
        match self {
            Self::OneHour => Snooze::Until(now + HOUR),
            Self::ThreeHours => Snooze::Until(now + 3 * HOUR),
            Self::TomorrowMorning => Snooze::Until(local_to_utc(today.day + 1, morning.0, clock)),
            Self::NextMonday => {
                // Always a future Monday: from Monday, that's next week's.
                let days = 7 - i64::from(today.weekday);
                Snooze::Until(local_to_utc(today.day + days, morning.0, clock))
            }
            Self::UntilChanged => Snooze::UntilChanged,
            Self::Until(at) => Snooze::Until(at.max(now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::schedule::tests::at;

    #[test]
    fn tomorrow_and_monday_use_the_morning_time() {
        let clock = FakeClock::new(0, 0);
        let wed_17 = at(2, 17, 0);
        assert_eq!(
            SnoozeChoice::TomorrowMorning.resolve(wed_17, DEFAULT_MORNING, &clock),
            Snooze::Until(at(3, 9, 0))
        );
        assert_eq!(
            SnoozeChoice::NextMonday.resolve(wed_17, TimeOfDay::hm(8, 30), &clock),
            Snooze::Until(at(7, 8, 30))
        );
        // From Monday morning, "Monday" means next week.
        assert_eq!(
            SnoozeChoice::NextMonday.resolve(at(0, 7, 0), DEFAULT_MORNING, &clock),
            Snooze::Until(at(7, 9, 0))
        );
        assert_eq!(
            SnoozeChoice::OneHour.resolve(wed_17, DEFAULT_MORNING, &clock),
            Snooze::Until(at(2, 18, 0))
        );
    }

    #[test]
    fn morning_is_local_time() {
        // UTC-5: 09:00 local = 14:00 UTC.
        let clock = FakeClock::new(0, -5 * 3_600);
        assert_eq!(
            SnoozeChoice::TomorrowMorning.resolve(at(2, 22, 0), DEFAULT_MORNING, &clock),
            Snooze::Until(at(3, 14, 0))
        );
        assert_eq!(
            SnoozeChoice::TomorrowMorning.label(DEFAULT_MORNING),
            "Tomorrow 09:00"
        );
    }
}
