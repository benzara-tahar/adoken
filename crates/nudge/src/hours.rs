//! Working-time math (spec 0006 FR-5): thresholds count only configured
//! working days and hours in the user's local time zone.

use adoken_model::Millis;
use adoken_model::time::{DAY, MINUTE};

/// Weekday bit for [`WorkingHours::days`]: bit 0 = Monday … bit 6 = Sunday.
pub const MON: u8 = 1 << 0;
pub const TUE: u8 = 1 << 1;
pub const WED: u8 = 1 << 2;
pub const THU: u8 = 1 << 3;
pub const FRI: u8 = 1 << 4;
pub const SAT: u8 = 1 << 5;
pub const SUN: u8 = 1 << 6;

/// Local working schedule. Same-day ranges only (`start_min < end_min`);
/// holidays are out of scope (FR-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkingHours {
    /// Weekday mask, see [`MON`] … [`SUN`].
    pub days: u8,
    /// Minutes after local midnight.
    pub start_min: u16,
    pub end_min: u16,
    /// Local offset from UTC in minutes, east positive.
    pub utc_offset_min: i32,
}

impl Default for WorkingHours {
    /// Mon–Fri 09:00–18:00, UTC. Callers set the offset from
    /// [`crate::local_utc_offset_min`].
    fn default() -> Self {
        Self {
            days: MON | TUE | WED | THU | FRI,
            start_min: 9 * 60,
            end_min: 18 * 60,
            utc_offset_min: 0,
        }
    }
}

impl WorkingHours {
    pub fn with_offset(mut self, utc_offset_min: i32) -> Self {
        self.utc_offset_min = utc_offset_min;
        self
    }

    fn offset(&self) -> Millis {
        Millis::from(self.utc_offset_min) * MINUTE
    }

    fn start(&self) -> Millis {
        Millis::from(self.start_min) * MINUTE
    }

    fn end(&self) -> Millis {
        Millis::from(self.end_min) * MINUTE
    }

    /// Length of one working day.
    pub fn day_length(&self) -> Millis {
        (self.end() - self.start()).max(0)
    }

    /// Local calendar day number (days since 1970-01-01 local).
    pub fn local_day(&self, t: Millis) -> i64 {
        (t + self.offset()).div_euclid(DAY)
    }

    /// Milliseconds since local midnight.
    fn time_of_day(&self, t: Millis) -> Millis {
        (t + self.offset()).rem_euclid(DAY)
    }

    fn is_workday_number(&self, day: i64) -> bool {
        // 1970-01-01 was a Thursday (Monday = 0).
        let weekday = (day + 3).rem_euclid(7) as u8;
        self.days & (1 << weekday) != 0
    }

    pub fn is_workday(&self, t: Millis) -> bool {
        self.is_workday_number(self.local_day(t))
    }

    /// Inside working hours on a working day.
    pub fn is_working_time(&self, t: Millis) -> bool {
        let tod = self.time_of_day(t);
        self.is_workday(t) && tod >= self.start() && tod < self.end()
    }

    /// Today is a working day and its working hours have begun.
    pub fn workday_started(&self, t: Millis) -> bool {
        self.is_workday(t) && self.time_of_day(t) >= self.start()
    }

    /// Local midnight that starts `t`'s local day.
    pub fn day_start(&self, t: Millis) -> Millis {
        self.local_day(t) * DAY - self.offset()
    }

    /// The configured working hours on `t`'s local day as instants
    /// `[start, end)`, whether or not that day is a workday.
    pub fn work_window(&self, t: Millis) -> (Millis, Millis) {
        let midnight = self.day_start(t);
        (midnight + self.start(), midnight + self.end())
    }

    /// Working time within `[from_tod, to_tod)` of local `day`.
    fn overlap(&self, day: i64, from_tod: Millis, to_tod: Millis) -> Millis {
        if !self.is_workday_number(day) {
            return 0;
        }
        (to_tod.min(self.end()) - from_tod.max(self.start())).max(0)
    }

    fn workdays_in(&self, first_day: i64, count: i64) -> i64 {
        let weeks = count / 7;
        let per_week = i64::from((self.days & 0x7f).count_ones());
        let tail = (0..count % 7)
            .filter(|i| self.is_workday_number(first_day + weeks * 7 + i))
            .count() as i64;
        weeks * per_week + tail
    }

    /// Working time elapsed between two instants; 0 when `to <= from`.
    /// Constant time in the span length (whole weeks are counted, not walked).
    pub fn working_between(&self, from: Millis, to: Millis) -> Millis {
        if to <= from || self.day_length() == 0 {
            return 0;
        }
        let (from_day, to_day) = (self.local_day(from), self.local_day(to));
        let (from_tod, to_tod) = (self.time_of_day(from), self.time_of_day(to));
        if from_day == to_day {
            return self.overlap(from_day, from_tod, to_tod);
        }
        self.overlap(from_day, from_tod, DAY)
            + self.overlap(to_day, 0, to_tod)
            + self.workdays_in(from_day + 1, to_day - from_day - 1) * self.day_length()
    }

    /// Start of working hours on the first working day after `t`'s local day.
    pub fn next_workday_start(&self, t: Millis) -> Millis {
        let mut day = self.local_day(t) + 1;
        if self.days & 0x7f != 0 {
            while !self.is_workday_number(day) {
                day += 1;
            }
        }
        day * DAY + self.start() - self.offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::time::HOUR;

    /// 2026-09-21 00:00 UTC, a Monday.
    const MONDAY: Millis = 1_789_948_800_000;

    fn at(day: i64, hour: i64, minute: i64) -> Millis {
        MONDAY + day * DAY + hour * HOUR + minute * MINUTE
    }

    #[test]
    fn monday_constant_is_a_monday() {
        let hours = WorkingHours::default();
        assert!(hours.is_workday(at(0, 12, 0)));
        assert!(hours.is_workday(at(4, 12, 0)), "Friday");
        assert!(!hours.is_workday(at(5, 12, 0)), "Saturday");
        assert!(!hours.is_workday(at(6, 12, 0)), "Sunday");
    }

    #[test]
    fn ac1_twenty_four_working_hours_from_monday_ten_ends_wednesday_sixteen() {
        let hours = WorkingHours::default();
        let added = at(0, 10, 0);
        assert_eq!(hours.working_between(added, at(1, 10, 0)), 9 * HOUR);
        assert!(hours.working_between(added, at(2, 15, 59)) < 24 * HOUR);
        assert_eq!(hours.working_between(added, at(2, 16, 0)), 24 * HOUR);
    }

    #[test]
    fn weekends_and_nights_pause_the_timer() {
        let hours = WorkingHours::default();
        // Friday 17:00 → Monday 10:00 = 1 h Friday + 1 h Monday.
        assert_eq!(hours.working_between(at(4, 17, 0), at(7, 10, 0)), 2 * HOUR);
        // Entirely outside working hours.
        assert_eq!(hours.working_between(at(0, 19, 0), at(1, 8, 0)), 0);
        assert_eq!(hours.working_between(at(5, 9, 0), at(6, 18, 0)), 0);
        // Reversed span.
        assert_eq!(hours.working_between(at(1, 0, 0), at(0, 0, 0)), 0);
    }

    #[test]
    fn long_spans_count_whole_weeks() {
        let hours = WorkingHours::default();
        // Monday 09:00 + 10 weeks = 50 working days.
        assert_eq!(
            hours.working_between(at(0, 9, 0), at(70, 9, 0)),
            50 * 9 * HOUR
        );
        // Matches a day-by-day walk for an odd span.
        let (from, to) = (at(-3, 11, 17), at(40, 16, 3));
        let walked: Millis = (hours.local_day(from)..=hours.local_day(to))
            .map(|day| {
                let lo = if day == hours.local_day(from) {
                    from
                } else {
                    day * DAY
                };
                let hi = if day == hours.local_day(to) {
                    to
                } else {
                    (day + 1) * DAY
                };
                hours.overlap(day, lo - day * DAY, hi - day * DAY)
            })
            .sum();
        assert_eq!(hours.working_between(from, to), walked);
    }

    #[test]
    fn working_hours_follow_the_local_offset() {
        // UTC+2: local Monday 10:00 is 08:00 UTC.
        let hours = WorkingHours::default().with_offset(120);
        let added = at(0, 8, 0);
        assert_eq!(hours.working_between(added, at(2, 14, 0)), 24 * HOUR);
        assert!(hours.is_working_time(at(0, 7, 0)), "09:00 local");
        assert!(!hours.is_working_time(at(0, 6, 59)));
    }

    #[test]
    fn the_local_day_and_its_working_window_follow_the_offset() {
        // UTC+2: local Tuesday starts Monday 22:00 UTC; 01:30 UTC is Tuesday.
        let hours = WorkingHours::default().with_offset(120);
        let t = at(1, 1, 30);
        assert_eq!(hours.day_start(t), at(0, 22, 0));
        assert_eq!(hours.work_window(t), (at(1, 7, 0), at(1, 16, 0)));
        // UTC-5: 03:00 UTC Tuesday is still Monday locally.
        let hours = WorkingHours::default().with_offset(-300);
        assert_eq!(hours.day_start(at(1, 3, 0)), at(0, 5, 0));
    }

    #[test]
    fn next_workday_skips_the_weekend() {
        let hours = WorkingHours::default();
        assert_eq!(hours.next_workday_start(at(0, 15, 0)), at(1, 9, 0));
        assert_eq!(hours.next_workday_start(at(4, 15, 0)), at(7, 9, 0));
        assert!(hours.workday_started(at(1, 9, 0)));
        assert!(!hours.workday_started(at(1, 8, 59)));
        assert!(!hours.workday_started(at(5, 12, 0)));
    }
}
