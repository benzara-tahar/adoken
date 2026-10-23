//! The weekly fight card's numbers (ken.md §5), read from the Ken tables.

use adoken_model::Millis;
use adoken_model::time::DAY;
use adoken_notify::Clock;
use adoken_notify::schedule::{LocalTime, local_to_utc};
use adoken_store::Store;

use crate::moves::Rank;
use crate::persist::{self, KoEntry};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// One local calendar week, Monday to Monday.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Week {
    /// Local day number of its Monday.
    pub monday: i64,
    pub from: Millis,
    pub to: Millis,
}

impl Week {
    /// The week containing `at`.
    pub fn of(at: Millis, clock: &dyn Clock) -> Week {
        let local = LocalTime::of(at, clock);
        Week::starting(local.day - i64::from(local.weekday), clock)
    }

    pub fn starting(monday: i64, clock: &dyn Clock) -> Week {
        Week {
            monday,
            from: local_to_utc(monday, 0, clock),
            to: local_to_utc(monday + 7, 0, clock),
        }
    }

    pub fn previous(self, clock: &dyn Clock) -> Week {
        Week::starting(self.monday - 7, clock)
    }

    /// "Week of Sep 21".
    pub fn label(self) -> String {
        format!("Week of {}", day_label(self.monday))
    }
}

/// "Sep 21" for a local day number.
pub fn day_label(day: i64) -> String {
    let (_, month, date) = civil(day);
    format!("{} {date}", MONTHS[month as usize - 1])
}

/// "Tue 14:02" for an instant.
pub fn moment_label(at: Millis, clock: &dyn Clock) -> String {
    let local = LocalTime::of(at, clock);
    format!(
        "{} {:02}:{:02}",
        WEEKDAYS[local.weekday as usize],
        local.minute / 60,
        local.minute % 60
    )
}

/// (year, month 1-12, day 1-31) of a day number (days since 1970-01-01),
/// Howard Hinnant's `civil_from_days`.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A K.O. line on the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardKo {
    pub pr: i64,
    pub title: String,
    pub repo: Option<String>,
    /// "Tue 14:02".
    pub when: String,
    pub double: bool,
}

/// Everything the dialog and the export image show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FightCard {
    pub week: Week,
    pub hits: u32,
    pub best_combo: Option<(u32, Rank)>,
    pub kos: Vec<CardKo>,
    pub streak_days: u32,
    pub shields: u8,
    pub first_strikes: u32,
    pub comebacks: u32,
    pub assists: u32,
    pub prs_reviewed: u32,
}

impl FightCard {
    /// Read `week`'s card. Streak numbers are the current ones.
    pub fn load(
        store: &Store,
        week: Week,
        streak_days: u32,
        shields: u8,
        clock: &dyn Clock,
    ) -> adoken_store::Result<FightCard> {
        let hits = persist::hits_between(store, week.from, week.to)?;
        let best = persist::best_combo_between(store, week.from, week.to)?;
        let repos = store.repos()?;
        let kos = persist::kos_between(store, week.from, week.to)?
            .into_iter()
            .map(
                |KoEntry {
                     pr,
                     at,
                     double,
                     title,
                 }| {
                    let repo = store.pr(pr).ok().flatten().and_then(|d| {
                        repos
                            .iter()
                            .find(|r| r.id == d.pr.repo_id)
                            .map(|r| r.name.clone())
                    });
                    CardKo {
                        pr,
                        title,
                        repo,
                        when: moment_label(at, clock),
                        double,
                    }
                },
            )
            .collect();
        let count = |kind| persist::badge_count_between(store, kind, week.from, week.to);
        Ok(FightCard {
            week,
            hits: hits.hits,
            best_combo: best.and_then(|n| Rank::for_hits(n).map(|rank| (n, rank))),
            kos,
            streak_days,
            shields,
            first_strikes: count("first_strike")?,
            comebacks: count("comeback")?,
            assists: count("assist")?,
            prs_reviewed: hits.prs_reviewed,
        })
    }
}

/// First launch of a week with ≥ 1 hit last week: offer last week's card
/// once (ken.md §5). Returns the week to offer and records the offer.
pub fn offer(store: &Store, now: Millis, clock: &dyn Clock) -> adoken_store::Result<Option<Week>> {
    let this_week = Week::of(now, clock);
    if persist::offered_week(store)? == Some(this_week.monday) {
        return Ok(None);
    }
    let last = this_week.previous(clock);
    let hits = persist::hits_between(store, last.from, last.to)?.hits;
    if hits == 0 {
        return Ok(None);
    }
    persist::set_offered_week(store, this_week.monday)?;
    Ok(Some(last))
}

/// Days in a week, for tests and callers iterating.
pub const WEEK: Millis = 7 * DAY;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{HitRow, Journal};
    use crate::moves::HitKind;
    use adoken_model::time::HOUR;
    use adoken_notify::FakeClock;

    /// Fri 2026-09-25 09:30 UTC.
    const NOW: Millis = 1_790_328_600_000;

    #[test]
    fn weeks_start_on_local_monday() {
        let clock = FakeClock::new(NOW, 0);
        let week = Week::of(NOW, &clock);
        assert_eq!(week.label(), "Week of Sep 21");
        assert_eq!(week.to - week.from, WEEK);
        assert!(week.from <= NOW && NOW < week.to);
        assert_eq!(week.previous(&clock).label(), "Week of Sep 14");
        // Monday 00:30 in UTC+2 is still Sunday in UTC, but it's the new week.
        let plus2 = FakeClock::new(NOW, 2 * 3600);
        let monday_early = week.to - 90 * 60 * 1000;
        assert_eq!(Week::of(monday_early, &plus2).label(), "Week of Sep 28");
        assert_eq!(day_label(0), "Jan 1");
        assert_eq!(moment_label(NOW, &clock), "Fri 09:30");
    }

    #[test]
    fn offered_once_per_week_and_only_after_a_week_with_hits() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        persist::migrate(&store).unwrap();
        let clock = FakeClock::new(NOW, 0);
        assert_eq!(
            offer(&store, NOW, &clock).unwrap(),
            None,
            "no hits last week"
        );
        let last_week = NOW - WEEK;
        let journal = Journal {
            hits: vec![HitRow {
                at: last_week,
                kind: HitKind::Viewed,
                pr: 1,
                combo_id: 1,
            }],
            ..Journal::default()
        };
        persist::save(&store, &Default::default(), &journal, None).unwrap();
        let offered = offer(&store, NOW, &clock).unwrap().unwrap();
        assert_eq!(offered.label(), "Week of Sep 14");
        assert_eq!(offer(&store, NOW + HOUR, &clock).unwrap(), None);
        let card = FightCard::load(&store, offered, 3, 0, &clock).unwrap();
        assert_eq!(card.hits, 1);
        assert_eq!(card.prs_reviewed, 1);
    }
}
