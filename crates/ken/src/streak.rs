//! Daily review streak with shields and pause (ken.md §3.4).
//!
//! Days are local-calendar day numbers (days since 1970-01-01 in local
//! time, `adoken_notify::schedule::LocalTime::day`). Only working days
//! count: a working day with ≥ 1 hit extends the streak, a missed working
//! day costs a shield or silently resets it, other days are skipped.

use adoken_notify::Weekdays;
use serde::{Deserialize, Serialize};

/// Shields held at most.
pub const MAX_SHIELDS: u8 = 2;
/// Consecutive streak days per earned shield.
pub const DAYS_PER_SHIELD: u32 = 5;
/// Badge-toast milestones.
pub const MILESTONES: [u32; 6] = [5, 10, 25, 50, 100, 250];

/// Monday = 0 for a local day number (1970-01-01 was a Thursday).
pub fn weekday(day: i64) -> u8 {
    (day + 3).rem_euclid(7) as u8
}

/// What happened to a day, for `ken_streak` rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayState {
    /// A working day with a hit: the streak grew.
    Hit,
    /// A missed working day covered by a shield.
    Shielded,
    /// A missed working day with no shield: the streak went back to 0.
    Reset,
}

impl DayState {
    pub fn as_str(self) -> &'static str {
        match self {
            DayState::Hit => "hit",
            DayState::Shielded => "shielded",
            DayState::Reset => "reset",
        }
    }
}

/// The streak grew today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extended {
    pub days: u32,
    /// One of [`MILESTONES`]: toast + sound; otherwise sound only.
    pub milestone: bool,
    /// A shield was earned by this extension.
    pub shield_earned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Streak {
    pub days: u32,
    pub shields: u8,
    /// Last local day that counted as a streak day.
    pub last_day: Option<i64>,
    /// Every working day up to and including this one is settled (hit,
    /// shielded or reset); `None` before the first hit.
    pub settled_through: Option<i64>,
    /// "Pause streak": the day it was paused; `None` while running.
    pub paused_since: Option<i64>,
}

impl Streak {
    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    /// Settle every working day before `today` that had no hit: each costs
    /// a shield; with none left the streak resets silently. Returns the
    /// per-day outcomes for the journal. Idempotent for the same `today`.
    pub fn settle(&mut self, today: i64, working: Weekdays) -> Vec<(i64, DayState)> {
        let mut out = Vec::new();
        if self.is_paused() {
            return out;
        }
        let Some(from) = self.settled_through else {
            return out;
        };
        for day in from + 1..today {
            self.settled_through = Some(day);
            if !working.contains(weekday(day)) || self.last_day == Some(day) {
                continue;
            }
            if self.days == 0 {
                // Nothing left to protect or break.
                continue;
            }
            if self.shields > 0 {
                self.shields -= 1;
                out.push((day, DayState::Shielded));
            } else {
                self.days = 0;
                out.push((day, DayState::Reset));
            }
        }
        out
    }

    /// A hit on local `day`. Returns `Some` when the streak grew (the first
    /// hit of a working day); hits on non-working days, while paused, or
    /// after today's first hit change nothing.
    pub fn hit(
        &mut self,
        day: i64,
        working: Weekdays,
        journal: &mut Vec<(i64, DayState)>,
    ) -> Option<Extended> {
        if self.is_paused() || self.last_day.is_some_and(|last| day <= last) {
            return None;
        }
        journal.extend(self.settle(day, working));
        if !working.contains(weekday(day)) {
            return None;
        }
        self.days += 1;
        self.last_day = Some(day);
        self.settled_through = Some(self.settled_through.map_or(day, |s| s.max(day)));
        journal.push((day, DayState::Hit));
        let shield_earned = self.days.is_multiple_of(DAYS_PER_SHIELD) && self.shields < MAX_SHIELDS;
        if shield_earned {
            self.shields += 1;
        }
        Some(Extended {
            days: self.days,
            milestone: MILESTONES.contains(&self.days),
            shield_earned,
        })
    }

    /// "Pause streak" (vacation): freezes the count. Days up to yesterday are
    /// settled first, so pausing never forgives an already-missed day.
    pub fn pause(&mut self, today: i64, working: Weekdays) -> Vec<(i64, DayState)> {
        if self.is_paused() {
            return Vec::new();
        }
        let settled = self.settle(today, working);
        self.paused_since = Some(today);
        settled
    }

    /// "Resume streak": the paused days are skipped (neither extend nor
    /// break); counting resumes today.
    pub fn resume(&mut self, today: i64) {
        if self.paused_since.take().is_some() && self.settled_through.is_some() {
            self.settled_through = Some(today - 1);
        }
    }

    /// Current count as the statusbar shows it on `today` (a day without a
    /// hit yet doesn't break anything until it's over).
    pub fn current(&self, today: i64, working: Weekdays) -> u32 {
        let mut probe = self.clone();
        probe.settle(today, working);
        probe.days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-21, a Monday, as a local day number.
    const MON: i64 = 20_717;
    const WORK: Weekdays = Weekdays::WORKDAYS;

    fn hit(streak: &mut Streak, day: i64) -> Option<Extended> {
        let mut journal = Vec::new();
        streak.hit(day, WORK, &mut journal)
    }

    #[test]
    fn day_numbers_have_the_right_weekday() {
        assert_eq!(weekday(MON), 0);
        assert_eq!(weekday(MON + 5), 5, "Saturday");
    }

    #[test]
    fn consecutive_working_days_extend_once_per_day() {
        let mut s = Streak::default();
        assert_eq!(hit(&mut s, MON).map(|e| e.days), Some(1));
        assert_eq!(hit(&mut s, MON), None, "second hit the same day");
        assert_eq!(hit(&mut s, MON + 1).map(|e| e.days), Some(2));
        assert_eq!(s.days, 2);
    }

    #[test]
    fn weekends_are_skipped_neither_extend_nor_break() {
        let mut s = Streak::default();
        for day in MON..MON + 5 {
            hit(&mut s, day);
        }
        assert_eq!(s.days, 5);
        assert_eq!(hit(&mut s, MON + 5), None, "Saturday hit");
        assert_eq!(s.days, 5);
        let next_monday = MON + 7;
        assert_eq!(hit(&mut s, next_monday).map(|e| e.days), Some(6));
        assert_eq!(s.shields, 1, "no shield spent over the weekend");
    }

    #[test]
    fn every_five_days_earns_a_shield_capped_at_two() {
        let mut s = Streak::default();
        let mut day = MON;
        let mut earned = Vec::new();
        while s.days < 15 {
            if WORK.contains(weekday(day))
                && let Some(e) = hit(&mut s, day)
                && e.shield_earned
            {
                earned.push(e.days);
            }
            day += 1;
        }
        assert_eq!(earned, vec![5, 10]);
        assert_eq!(s.shields, MAX_SHIELDS, "day 15 earns nothing past the cap");
    }

    #[test]
    fn missed_day_consumes_a_shield_then_resets_silently() {
        let mut s = Streak::default();
        for day in MON..MON + 5 {
            hit(&mut s, day);
        }
        assert_eq!((s.days, s.shields), (5, 1));
        // Miss Monday next week; hit Tuesday.
        let mut journal = Vec::new();
        let e = s.hit(MON + 8, WORK, &mut journal).unwrap();
        assert_eq!(journal[0], (MON + 7, DayState::Shielded));
        assert_eq!((e.days, s.shields), (6, 0));
        // Miss Wednesday with no shield: reset, then Thursday starts at 1.
        let mut journal = Vec::new();
        let e = s.hit(MON + 10, WORK, &mut journal).unwrap();
        assert_eq!(journal[0], (MON + 9, DayState::Reset));
        assert_eq!(e.days, 1);
    }

    #[test]
    fn today_without_a_hit_does_not_break_until_it_is_over() {
        let mut s = Streak::default();
        hit(&mut s, MON);
        assert_eq!(s.current(MON + 1, WORK), 1, "Tuesday still in progress");
        assert_eq!(s.current(MON + 2, WORK), 0, "Tuesday missed, no shield");
        assert_eq!(s.days, 1, "current() doesn't mutate");
    }

    #[test]
    fn pause_freezes_and_resume_skips_the_paused_days() {
        let mut s = Streak::default();
        for day in MON..MON + 3 {
            hit(&mut s, day);
        }
        s.pause(MON + 3, WORK);
        assert!(s.is_paused());
        assert_eq!(hit(&mut s, MON + 3), None, "hits don't extend while paused");
        assert!(s.settle(MON + 20, WORK).is_empty());
        s.resume(MON + 21);
        assert_eq!(s.current(MON + 21, WORK), 3);
        assert_eq!(hit(&mut s, MON + 21).map(|e| e.days), Some(4));
    }

    #[test]
    fn pause_does_not_forgive_an_already_missed_day() {
        let mut s = Streak::default();
        hit(&mut s, MON);
        let settled = s.pause(MON + 2, WORK);
        assert_eq!(settled, vec![(MON + 1, DayState::Reset)]);
        assert_eq!(s.days, 0);
    }

    #[test]
    fn milestones_are_flagged() {
        let mut s = Streak::default();
        let mut day = MON;
        let mut milestones = Vec::new();
        while s.days < 26 {
            if let Some(e) = hit(&mut s, day)
                && e.milestone
            {
                milestones.push(e.days);
            }
            day += 1;
        }
        assert_eq!(milestones, vec![5, 10, 25]);
    }
}
