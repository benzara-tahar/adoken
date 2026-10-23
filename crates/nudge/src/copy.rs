//! Copy helpers (docs/design/principles.md §5): relative times, counts, ids
//! as ADO writes them.

use adoken_model::Millis;
use adoken_model::time::{DAY, HOUR, MINUTE};

use crate::hours::WorkingHours;

/// `1 review` / `3 reviews`.
pub fn count(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {plural}")
    }
}

/// A wall-clock span: `12 min`, `5 h`, `2 days`.
pub fn span(ms: Millis) -> String {
    let ms = ms.max(0);
    if ms < HOUR {
        format!("{} min", (ms / MINUTE).max(1))
    } else if ms < DAY {
        format!("{} h", ms / HOUR)
    } else {
        count((ms / DAY) as usize, "day", "days")
    }
}

/// A glanceable wall-clock age for badges: `now`, `12m`, `5h`, `2d`.
pub fn compact(ms: Millis) -> String {
    match ms.max(0) {
        ms if ms < MINUTE => "now".into(),
        ms if ms < HOUR => format!("{}m", ms / MINUTE),
        ms if ms < DAY => format!("{}h", ms / HOUR),
        ms => format!("{}d", ms / DAY),
    }
}

/// A working-time span: `26 working hours` below three working days, then
/// `7 working days`.
pub fn working_span(ms: Millis, hours: &WorkingHours) -> String {
    let day = hours.day_length().max(HOUR);
    if ms < 3 * day {
        count((ms / HOUR).max(1) as usize, "working hour", "working hours")
    } else {
        count((ms / day) as usize, "working day", "working days")
    }
}

/// `just now`, `12 min ago`, `3 h ago`, `yesterday`, `4 days ago`.
pub fn ago(since: Millis, now: Millis, hours: &WorkingHours) -> String {
    let elapsed = now - since;
    if elapsed < MINUTE {
        return "just now".into();
    }
    if elapsed < DAY && hours.local_day(since) == hours.local_day(now) {
        return format!("{} ago", span(elapsed));
    }
    match hours.local_day(now) - hours.local_day(since) {
        ..=1 => "yesterday".into(),
        days => format!("{days} days ago"),
    }
}

/// `for 3 h`, `since yesterday`, `for 4 days`.
pub fn since(since: Millis, now: Millis, hours: &WorkingHours) -> String {
    let elapsed = (now - since).max(0);
    match hours.local_day(now) - hours.local_day(since) {
        ..=0 => format!("for {}", span(elapsed)),
        1 => "since yesterday".into(),
        days => format!("for {days} days"),
    }
}

/// `Kim (required), Sam` — FR-9 order is the caller's.
pub fn names(people: &[(String, bool)]) -> String {
    people
        .iter()
        .map(|(name, required)| {
            if *required {
                format!("{name} (required)")
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `14:32`: local wall-clock time under `hours`' offset.
pub fn clock(t: Millis, hours: &WorkingHours) -> String {
    let minutes = (t - hours.day_start(t)) / MINUTE;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `Thursday, October 1`: the local long date (Today's header, 0017 FR-13).
pub fn long_date(t: Millis, hours: &WorkingHours) -> String {
    const WEEKDAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let days = hours.local_day(t);
    // 1970-01-01 was a Thursday (Monday = 0).
    let weekday = (days + 3).rem_euclid(7) as usize;
    // Month and day from days since the epoch (Howard Hinnant's
    // `civil_from_days`, public domain).
    let doe = (days + 719_468).rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 2 } else { mp - 10 };
    format!("{}, {} {day}", WEEKDAYS[weekday], MONTHS[month as usize])
}

/// [`count`] with a lower bound: `3+ commits` (plural) when `plus`.
pub fn count_plus(n: u32, plus: bool, singular: &str, plural: &str) -> String {
    if plus {
        format!("{n}+ {plural}")
    } else {
        count(n as usize, singular, plural)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONDAY: Millis = 1_789_948_800_000;

    #[test]
    fn relative_times_use_calendar_days() {
        let hours = WorkingHours::default();
        let now = MONDAY + DAY + 10 * HOUR; // Tue 10:00
        assert_eq!(ago(now - 30 * 1000, now, &hours), "just now");
        assert_eq!(ago(now - 3 * HOUR, now, &hours), "3 h ago");
        assert_eq!(ago(now - 12 * HOUR, now, &hours), "yesterday");
        assert_eq!(ago(now - 3 * DAY, now, &hours), "3 days ago");
        assert_eq!(since(now - 2 * HOUR, now, &hours), "for 2 h");
        assert_eq!(since(now - 20 * HOUR, now, &hours), "since yesterday");
        assert_eq!(since(now - 4 * DAY, now, &hours), "for 4 days");
    }

    #[test]
    fn working_spans_switch_to_days_after_three_days() {
        let hours = WorkingHours::default();
        assert_eq!(working_span(26 * HOUR, &hours), "26 working hours");
        assert_eq!(working_span(HOUR, &hours), "1 working hour");
        assert_eq!(working_span(7 * 9 * HOUR, &hours), "7 working days");
    }

    #[test]
    fn compact_ages_floor_to_the_largest_unit() {
        assert_eq!(compact(30 * 1000), "now");
        assert_eq!(compact(59 * MINUTE), "59m");
        assert_eq!(compact(HOUR), "1h");
        assert_eq!(compact(DAY - 1), "23h");
        assert_eq!(compact(18 * DAY + 5 * HOUR), "18d");
    }

    #[test]
    fn required_reviewers_are_marked() {
        let list = [("Kim".to_owned(), true), ("Sam".to_owned(), false)];
        assert_eq!(names(&list), "Kim (required), Sam");
    }
}
