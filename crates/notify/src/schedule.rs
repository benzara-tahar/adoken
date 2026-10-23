//! Weekly schedules in local time: quiet hours and per-source delivery
//! windows (spec 0005 flow step 5, FR-6). Overnight spans (`19:00 → 08:00`)
//! belong to the day they start on.

use std::fmt;

use adoken_model::Millis;
use adoken_model::time::{DAY, MINUTE};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::clock::Clock;

/// Monday-first weekday index, 0..=6.
pub const WEEKDAY_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const WEEKDAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Local wall-clock position of an instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    /// Days since 1970-01-01 in local time.
    pub day: i64,
    /// Monday = 0.
    pub weekday: u8,
    /// Minutes since local midnight.
    pub minute: u16,
}

impl LocalTime {
    pub fn of(at: Millis, clock: &dyn Clock) -> Self {
        let local = at + Millis::from(clock.offset_at(at)) * 1000;
        let day = local.div_euclid(DAY);
        Self {
            day,
            // 1970-01-01 was a Thursday (Monday-first index 3).
            weekday: (day + 3).rem_euclid(7) as u8,
            minute: (local.rem_euclid(DAY) / MINUTE) as u16,
        }
    }
}

/// UTC instant of `minute` on local `day`, using the offset in effect then.
pub fn local_to_utc(day: i64, minute: u16, clock: &dyn Clock) -> Millis {
    let local = day * DAY + Millis::from(minute) * MINUTE;
    let guess = local - Millis::from(clock.offset_at(local)) * 1000;
    local - Millis::from(clock.offset_at(guess)) * 1000
}

/// "08:00" today, "tomorrow 09:00", else "Mon 09:00" (within a week) —
/// for "Quiet hours until …" and snooze return times.
pub fn clock_label(at: Millis, now: Millis, clock: &dyn Clock) -> String {
    let target = LocalTime::of(at, clock);
    let today = LocalTime::of(now, clock);
    let time = TimeOfDay(target.minute);
    match target.day - today.day {
        0 => time.to_string(),
        1 => format!("tomorrow {time}"),
        _ => format!("{} {time}", WEEKDAY_LABELS[target.weekday as usize]),
    }
}

/// Set of weekdays, bit 0 = Monday. Serialized as `["mon", "tue", ...]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Weekdays(pub u8);

impl Weekdays {
    pub const NONE: Self = Self(0);
    pub const WORKDAYS: Self = Self(0b001_1111);
    pub const WEEKEND: Self = Self(0b110_0000);
    pub const ALL: Self = Self(0b111_1111);

    pub fn contains(self, weekday: u8) -> bool {
        weekday < 7 && self.0 & (1 << weekday) != 0
    }

    pub fn with(self, weekday: u8, on: bool) -> Self {
        if on {
            Self(self.0 | (1 << weekday))
        } else {
            Self(self.0 & !(1 << weekday))
        }
    }

    /// "Mon–Fri", "Sat, Sun", "Every day".
    pub fn label(self) -> String {
        match self {
            Self::ALL => return "Every day".into(),
            Self::WORKDAYS => return "Mon–Fri".into(),
            Self::NONE => return "Never".into(),
            _ => {}
        }
        (0..7)
            .filter(|&d| self.contains(d))
            .map(|d| WEEKDAY_LABELS[d as usize])
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Serialize for Weekdays {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let names: Vec<&str> = (0..7u8)
            .filter(|&d| self.contains(d))
            .map(|d| WEEKDAY_NAMES[d as usize])
            .collect();
        names.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Weekdays {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(deserializer)?;
        let mut days = Self::NONE;
        for name in names {
            let index = WEEKDAY_NAMES
                .iter()
                .position(|day| name.eq_ignore_ascii_case(day))
                .ok_or_else(|| serde::de::Error::custom(format!("unknown weekday `{name}`")))?;
            days = days.with(index as u8, true);
        }
        Ok(days)
    }
}

/// Minutes since local midnight, serialized as `"HH:MM"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfDay(pub u16);

impl TimeOfDay {
    pub const fn hm(hour: u16, minute: u16) -> Self {
        Self(hour * 60 + minute)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let (h, m) = text.trim().split_once(':')?;
        let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(Self::hm(h, m))
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.0 / 60, self.0 % 60)
    }
}

impl Serialize for TimeOfDay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("bad time `{text}`")))
    }
}

/// `days` at `from → to`. `to <= from` runs past midnight into the next day;
/// `from == to` is the whole day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub days: Weekdays,
    pub from: TimeOfDay,
    pub to: TimeOfDay,
}

impl Span {
    pub fn new(days: Weekdays, from: TimeOfDay, to: TimeOfDay) -> Self {
        Self { days, from, to }
    }

    /// Length in minutes (1..=1440).
    fn minutes(self) -> i64 {
        let (from, to) = (i64::from(self.from.0), i64::from(self.to.0));
        if to > from {
            to - from
        } else {
            to - from + 1440
        }
    }

    fn contains(self, now: LocalTime) -> bool {
        let minute = i64::from(now.minute);
        let from = i64::from(self.from.0);
        let len = self.minutes();
        // Started today…
        if self.days.contains(now.weekday) && minute >= from && minute < from + len {
            return true;
        }
        // …or yesterday and still running.
        let yesterday = (now.weekday + 6) % 7;
        self.days.contains(yesterday) && minute + 1440 < from + len
    }

    /// "Mon–Fri 09:00–18:00".
    pub fn label(self) -> String {
        if self.from == self.to {
            format!("{} all day", self.days.label())
        } else {
            format!("{} {}–{}", self.days.label(), self.from, self.to)
        }
    }
}

/// A union of spans. Empty = never.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    #[serde(default)]
    pub spans: Vec<Span>,
}

impl Schedule {
    pub fn new(spans: Vec<Span>) -> Self {
        Self { spans }
    }

    pub fn contains(&self, at: Millis, clock: &dyn Clock) -> bool {
        let local = LocalTime::of(at, clock);
        self.spans.iter().any(|span| span.contains(local))
    }

    /// Every span start/end instant in the 9 local days around `at`.
    fn boundaries(&self, at: Millis, clock: &dyn Clock, starts: bool) -> Vec<Millis> {
        let today = LocalTime::of(at, clock).day;
        let mut out = Vec::new();
        for day in today - 1..=today + 8 {
            let weekday = (day + 3).rem_euclid(7) as u8;
            for span in &self.spans {
                if !span.days.contains(weekday) {
                    continue;
                }
                let start = local_to_utc(day, span.from.0, clock);
                out.push(if starts {
                    start
                } else {
                    start + span.minutes() * MINUTE
                });
            }
        }
        out
    }

    /// Earliest instant ≥ `at` inside the schedule; `None` if it never opens.
    pub fn next_start(&self, at: Millis, clock: &dyn Clock) -> Option<Millis> {
        if self.contains(at, clock) {
            return Some(at);
        }
        self.boundaries(at, clock, true)
            .into_iter()
            .filter(|&t| t > at && self.contains(t, clock))
            .min()
    }

    /// Earliest instant ≥ `at` outside the schedule; `None` if it never
    /// closes (every day, all day).
    pub fn next_end(&self, at: Millis, clock: &dyn Clock) -> Option<Millis> {
        if !self.contains(at, clock) {
            return Some(at);
        }
        // A covered stretch always ends on some span's end boundary, and the
        // first boundary that isn't covered by another span is that end.
        self.boundaries(at, clock, false)
            .into_iter()
            .filter(|&t| t > at && !self.contains(t, clock))
            .min()
    }

    pub fn label(&self) -> String {
        if self.spans.is_empty() {
            return "Never".into();
        }
        self.spans
            .iter()
            .map(|s| s.label())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::clock::FakeClock;

    /// 2026-09-21 is a Monday.
    pub(crate) const MONDAY: i64 = 20_717;

    pub(crate) fn at(day: i64, hour: u16, minute: u16) -> Millis {
        (MONDAY + day) * DAY + Millis::from(hour * 60 + minute) * MINUTE
    }

    fn utc() -> FakeClock {
        FakeClock::new(0, 0)
    }

    #[test]
    fn monday_constant_is_a_monday() {
        assert_eq!(LocalTime::of(at(0, 12, 0), &utc()).weekday, 0);
        assert_eq!(LocalTime::of(at(5, 12, 0), &utc()).weekday, 5);
    }

    #[test]
    fn overnight_span_belongs_to_its_start_day() {
        let quiet = Schedule::new(vec![Span::new(
            Weekdays::WORKDAYS,
            TimeOfDay::hm(18, 0),
            TimeOfDay::hm(8, 0),
        )]);
        let c = utc();
        assert!(quiet.contains(at(0, 22, 0), &c));
        assert!(quiet.contains(at(1, 7, 59), &c));
        assert!(!quiet.contains(at(1, 8, 0), &c));
        assert!(!quiet.contains(at(0, 17, 59), &c));
        // Friday night runs into Saturday morning; Sunday night isn't covered.
        assert!(quiet.contains(at(5, 3, 0), &c));
        assert!(!quiet.contains(at(6, 22, 0), &c));
        assert_eq!(quiet.next_end(at(0, 22, 0), &c), Some(at(1, 8, 0)));
        assert_eq!(quiet.next_start(at(1, 9, 0), &c), Some(at(1, 18, 0)));
    }

    #[test]
    fn adjacent_spans_merge_when_finding_the_end() {
        // Onboarding default: Sun–Fri evenings + all weekend = quiet from
        // Friday 19:00 straight through to Monday 08:00.
        let quiet = Schedule::new(vec![
            Span::new(
                Weekdays(0b101_1111),
                TimeOfDay::hm(19, 0),
                TimeOfDay::hm(8, 0),
            ),
            Span::new(Weekdays::WEEKEND, TimeOfDay(0), TimeOfDay(0)),
        ]);
        let c = utc();
        assert_eq!(quiet.next_end(at(4, 20, 0), &c), Some(at(7, 8, 0)));
    }

    #[test]
    fn window_opens_next_workday_morning() {
        let window = Schedule::new(vec![Span::new(
            Weekdays::WORKDAYS,
            TimeOfDay::hm(9, 0),
            TimeOfDay::hm(18, 0),
        )]);
        let c = utc();
        assert_eq!(window.next_start(at(5, 10, 0), &c), Some(at(7, 9, 0)));
        assert_eq!(window.next_start(at(2, 10, 0), &c), Some(at(2, 10, 0)));
        assert_eq!(window.next_start(at(2, 18, 0), &c), Some(at(3, 9, 0)));
        assert_eq!(Schedule::default().next_start(at(0, 0, 0), &c), None);
    }

    #[test]
    fn full_week_never_ends() {
        let always = Schedule::new(vec![Span::new(Weekdays::ALL, TimeOfDay(0), TimeOfDay(0))]);
        assert_eq!(always.next_end(at(0, 0, 0), &utc()), None);
    }

    #[test]
    fn local_offset_shifts_the_schedule() {
        // UTC+2: 06:00 UTC is 08:00 local.
        let c = FakeClock::new(0, 7_200);
        let quiet = Schedule::new(vec![Span::new(
            Weekdays::ALL,
            TimeOfDay::hm(18, 0),
            TimeOfDay::hm(8, 0),
        )]);
        assert!(quiet.contains(at(1, 5, 59), &c));
        assert!(!quiet.contains(at(1, 6, 0), &c));
        assert_eq!(quiet.next_end(at(1, 1, 0), &c), Some(at(1, 6, 0)));
    }

    #[test]
    fn serde_uses_readable_days_and_times() {
        let span = Span::new(
            Weekdays::WEEKEND,
            TimeOfDay::hm(9, 30),
            TimeOfDay::hm(17, 0),
        );
        let text = serde_json::to_string(&span).unwrap();
        assert_eq!(
            text,
            r#"{"days":["sat","sun"],"from":"09:30","to":"17:00"}"#
        );
        assert_eq!(serde_json::from_str::<Span>(&text).unwrap(), span);
        assert!(serde_json::from_str::<TimeOfDay>(r#""24:00""#).is_err());
    }

    #[test]
    fn clock_labels_are_relative_to_today() {
        let c = utc();
        assert_eq!(clock_label(at(1, 8, 0), at(1, 1, 0), &c), "08:00");
        assert_eq!(clock_label(at(1, 8, 0), at(0, 22, 0), &c), "tomorrow 08:00");
        assert_eq!(clock_label(at(7, 9, 0), at(5, 10, 0), &c), "Mon 09:00");
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(
            Span::new(
                Weekdays::WORKDAYS,
                TimeOfDay::hm(9, 0),
                TimeOfDay::hm(18, 0)
            )
            .label(),
            "Mon–Fri 09:00–18:00"
        );
        assert_eq!(Weekdays(0b000_0101).label(), "Mon, Wed");
    }
}
