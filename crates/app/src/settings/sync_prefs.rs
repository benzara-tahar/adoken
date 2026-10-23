//! `[sync]` in `settings.toml` (spec 0002 FR-4a, spec 0009 Sync tab; user
//! decision 2026-09-30) and the Sync tab's copy. Pure: no gpui, no IO.
//!
//! ```toml
//! [sync]
//! my_prs = "1m"          # 30s 1m 2m 5m 15m 30m manual
//! all_prs = "2m"
//! work_items = "2m"
//! pipelines = "1m"
//! background = "slower"  # same | slower | paused
//! paused = true          # until resumed
//! paused_until = 2026-09-30T14:00:00Z
//! ```
//!
//! Missing keys are FR-4's cadence; bad keys fall back one by one, values
//! under a kind's minimum clamp up to it, and every adjustment is reported.

use adoken_model::Millis;
use adoken_model::time::{HOUR, MINUTE, SECOND};
use adoken_sync::cadence::{
    Background, Cadence, Kind, Pause, Prefs, RATE_LIMIT_TSTU, STEADY_TARGET_TSTU, percent_of_limit,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use toml::{Table, Value};

pub const SYNC_TABLE: &str = "sync";
/// "Pause for 1 hour".
pub const PAUSE_FOR: Millis = HOUR;

pub fn key(kind: Kind) -> &'static str {
    match kind {
        Kind::MyPrs => "my_prs",
        Kind::AllPrs => "all_prs",
        Kind::WorkItems => "work_items",
        Kind::Pipelines => "pipelines",
    }
}

/// Row title.
pub fn title(kind: Kind) -> &'static str {
    match kind {
        Kind::MyPrs => "My pull requests",
        Kind::AllPrs => "All active pull requests",
        Kind::WorkItems => "Work items",
        Kind::Pipelines => "Pipeline runs",
    }
}

/// What the row syncs, and why its minimum is what it is.
pub fn description(kind: Kind) -> &'static str {
    match kind {
        Kind::MyPrs => {
            "Comments, votes and pushes on PRs you wrote or review. Shortest is 30 s: one \
             request per PR you're on."
        }
        Kind::AllPrs => {
            "Every active PR in your projects; new PRs and review requests appear on this \
             cadence. Shortest is 1 min: it lists every project each time."
        }
        Kind::WorkItems => {
            "Items assigned to or created by you, and their discussions. Shortest is 1 min: \
             it queries every project each time."
        }
        Kind::Pipelines => {
            "Your running pipelines. Finished runs are checked at most every 2 min. \
             Shortest is 30 s."
        }
    }
}

/// Words people type for the row (Settings search).
pub fn keywords(kind: Kind) -> &'static str {
    match kind {
        Kind::MyPrs => "pr pull request reviews mine waiting on me interval refresh",
        Kind::AllPrs => "pr pull requests all active list interval refresh",
        Kind::WorkItems => "work items tasks bugs boards interval refresh",
        Kind::Pipelines => "pipelines builds runs ci interval refresh",
    }
}

/// `"30s"`, `"1m"`, …, `"manual"`.
pub fn cadence_id(cadence: Cadence) -> String {
    match cadence {
        Cadence::Manual => "manual".into(),
        Cadence::Every(ms) if ms % MINUTE == 0 => format!("{}m", ms / MINUTE),
        Cadence::Every(ms) => format!("{}s", ms / SECOND),
    }
}

/// `"30 s"`, `"1 min"`, …, `"Manual only"`.
pub fn cadence_label(cadence: Cadence) -> String {
    match cadence {
        Cadence::Manual => "Manual only".into(),
        Cadence::Every(ms) => interval_label(ms),
    }
}

/// `"30 s"`, `"5 min"`, `"1 h"`.
pub fn interval_label(ms: Millis) -> String {
    if ms < MINUTE {
        format!("{} s", ms / SECOND)
    } else if ms < HOUR || ms % HOUR != 0 {
        format!("{} min", ms / MINUTE)
    } else {
        format!("{} h", ms / HOUR)
    }
}

pub fn background_id(background: Background) -> &'static str {
    match background {
        Background::Same => "same",
        Background::Slower => "slower",
        Background::Paused => "paused",
    }
}

pub fn background_label(background: Background) -> &'static str {
    match background {
        Background::Same => "Same",
        Background::Slower => "Slower (×5)",
        Background::Paused => "Paused",
    }
}

/// `"12s ago"`, `"3 min ago"`, `"Never"`.
pub fn last_sync_label(last: Option<Millis>, now: Millis) -> String {
    let Some(at) = last else {
        return "Not synced yet".into();
    };
    let ago = (now - at).max(0);
    if ago < MINUTE {
        format!("Synced {}s ago", ago / SECOND)
    } else if ago < HOUR {
        format!("Synced {} min ago", ago / MINUTE)
    } else {
        format!("Synced {} h ago", ago / HOUR)
    }
}

/// `"~3% of your Azure DevOps rate limit"` and the detail line.
pub fn budget_copy(tstu: f64, measured: f64) -> (String, String) {
    let pct = percent_of_limit(tstu);
    let headline = if pct < 1.0 {
        "Under 1% of your Azure DevOps rate limit".to_owned()
    } else {
        format!("~{}% of your Azure DevOps rate limit", pct.round() as i64)
    };
    let mut detail = format!(
        "About {} TSTU per 5 min with these settings (Azure DevOps allows {} per user, \
         shared with your browser, az and git). Measured in the last 5 min: {}.",
        tstu_text(tstu),
        RATE_LIMIT_TSTU as i64,
        tstu_text(measured)
    );
    if tstu > STEADY_TARGET_TSTU {
        detail.push_str(&format!(
            " Above adoken's {} TSTU target: the rate governor will slow sync down.",
            STEADY_TARGET_TSTU as i64
        ));
    }
    (headline, detail)
}

fn tstu_text(tstu: f64) -> String {
    if tstu < 0.1 {
        format!("{tstu:.3}")
    } else if tstu < 10.0 {
        format!("{tstu:.1}")
    } else {
        format!("{}", tstu.round() as i64)
    }
}

/// `"Paused until 14:05"` (local time) / `"Paused until you resume"`.
pub fn pause_label(pause: Pause, now: Millis, offset: time::UtcOffset) -> Option<String> {
    match pause {
        _ if !pause.active(now) => None,
        Pause::UntilResumed => Some("Paused until you resume".into()),
        Pause::Until(at) => {
            let local = OffsetDateTime::from_unix_timestamp(at / 1000)
                .ok()?
                .to_offset(offset);
            Some(format!(
                "Paused until {:02}:{:02}",
                local.hour(),
                local.minute()
            ))
        }
        Pause::Off => None,
    }
}

// ── codec ───────────────────────────────────────────────────────────────

/// `"45s"`, `"2m"`, `"2min"`, `"1h"` → milliseconds.
fn parse_duration(text: &str) -> Option<Millis> {
    let text = text.trim().to_ascii_lowercase();
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = text.split_at(split);
    let n: Millis = n.parse().ok()?;
    let unit_ms = match unit.trim() {
        "s" | "sec" => SECOND,
        "m" | "min" => MINUTE,
        "h" => HOUR,
        _ => return None,
    };
    Some(n * unit_ms)
}

fn parse_cadence(kind: Kind, value: &Value, issues: &mut Vec<String>) -> Cadence {
    let name = format!("{SYNC_TABLE}.{}", key(kind));
    let fallback = kind.default_cadence();
    let Some(text) = value.as_str() else {
        issues.push(format!(
            "{name}: expected a string like \"2m\"; using the default"
        ));
        return fallback;
    };
    if text.trim().eq_ignore_ascii_case("manual") {
        return Cadence::Manual;
    }
    let Some(ms) = parse_duration(text) else {
        issues.push(format!(
            "{name}: \"{text}\" isn't an interval; using the default"
        ));
        return fallback;
    };
    if ms < kind.minimum() {
        issues.push(format!(
            "{name}: {} is the shortest allowed; using it",
            interval_label(kind.minimum())
        ));
        return Cadence::Every(kind.minimum());
    }
    let nearest = Cadence::nearest(kind, ms);
    if nearest != Cadence::Every(ms) {
        issues.push(format!(
            "{name}: \"{text}\" isn't a choice; using {}",
            cadence_label(nearest)
        ));
    }
    nearest
}

fn datetime_ms(value: &Value) -> Option<Millis> {
    let text = match value {
        Value::Datetime(dt) => dt.to_string(),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    let at = OffsetDateTime::parse(&text, &Rfc3339).ok()?;
    Some((at.unix_timestamp_nanos() / 1_000_000) as Millis)
}

/// Parse `[sync]` out of the whole file over the defaults.
pub fn parse(document: &Table) -> (Prefs, Vec<String>) {
    let mut prefs = Prefs::default();
    let mut issues = Vec::new();
    let Some(table) = document.get(SYNC_TABLE) else {
        return (prefs, issues);
    };
    let Some(table) = table.as_table() else {
        issues.push(format!(
            "{SYNC_TABLE}: expected a table; using the defaults"
        ));
        return (prefs, issues);
    };
    for kind in Kind::ALL {
        if let Some(value) = table.get(key(kind)) {
            prefs.set_cadence(kind, parse_cadence(kind, value, &mut issues));
        }
    }
    if let Some(value) = table.get("background") {
        match Background::ALL
            .into_iter()
            .find(|b| value.as_str() == Some(background_id(*b)))
        {
            Some(background) => prefs.background = background,
            None => issues.push(format!(
                "{SYNC_TABLE}.background: expected \"same\", \"slower\" or \"paused\"; \
                 using \"slower\""
            )),
        }
    }
    if let Some(value) = table.get("paused_until") {
        match datetime_ms(value) {
            Some(at) => prefs.pause = Pause::Until(at),
            None => issues.push(format!(
                "{SYNC_TABLE}.paused_until: expected a date-time; not paused"
            )),
        }
    }
    match table.get("paused") {
        None => {}
        Some(Value::Boolean(true)) => prefs.pause = Pause::UntilResumed,
        Some(Value::Boolean(false)) => {}
        Some(_) => issues.push(format!("{SYNC_TABLE}.paused: expected true or false")),
    }
    (prefs, issues)
}

/// `prefs` as the `[sync]` table. Every key is written so the file shows
/// what's in effect.
pub fn table(prefs: &Prefs) -> Table {
    let mut table = Table::new();
    for kind in Kind::ALL {
        table.insert(
            key(kind).into(),
            Value::String(cadence_id(prefs.cadence(kind))),
        );
    }
    table.insert(
        "background".into(),
        Value::String(background_id(prefs.background).into()),
    );
    match prefs.pause {
        Pause::Off => {}
        Pause::UntilResumed => {
            table.insert("paused".into(), Value::Boolean(true));
        }
        Pause::Until(at) => {
            if let Some(dt) = OffsetDateTime::from_unix_timestamp(at / 1000)
                .ok()
                .and_then(|t| t.format(&Rfc3339).ok())
                .and_then(|s| s.parse::<toml::value::Datetime>().ok())
            {
                table.insert("paused_until".into(), Value::Datetime(dt));
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn missing_table_is_fr4_default() {
        let (prefs, issues) = parse(&Table::new());
        assert_eq!(prefs, Prefs::default());
        assert!(issues.is_empty());
        assert_eq!(prefs.my_prs, Cadence::Every(MINUTE));
        assert_eq!(prefs.all_prs, Cadence::Every(2 * MINUTE));
        assert_eq!(prefs.background, Background::Slower);
    }

    #[test]
    fn full_table_round_trips() {
        let prefs = Prefs {
            my_prs: Cadence::Every(30 * SECOND),
            all_prs: Cadence::Every(15 * MINUTE),
            work_items: Cadence::Manual,
            pipelines: Cadence::Every(5 * MINUTE),
            background: Background::Paused,
            pause: Pause::Until(1_790_769_600_000),
        };
        let text = toml::to_string(&table(&prefs)).unwrap();
        let mut document = Table::new();
        document.insert(
            SYNC_TABLE.into(),
            Value::Table(toml::from_str(&text).unwrap()),
        );
        assert_eq!(parse(&document), (prefs, Vec::new()));
        assert!(
            text.contains("paused_until = 2026-09-30T12:00:00Z"),
            "{text}"
        );
        let resumed = Prefs {
            pause: Pause::UntilResumed,
            ..prefs
        };
        let mut document = Table::new();
        document.insert(SYNC_TABLE.into(), Value::Table(table(&resumed)));
        assert_eq!(parse(&document).0.pause, Pause::UntilResumed);
    }

    #[test]
    fn values_under_the_minimum_clamp_up_and_are_reported() {
        let (prefs, issues) = parse(&doc("[sync]\nall_prs = \"10s\"\nmy_prs = \"5s\"\n"));
        assert_eq!(prefs.all_prs, Cadence::Every(MINUTE));
        assert_eq!(prefs.my_prs, Cadence::Every(30 * SECOND));
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[1].contains("1 min is the shortest"), "{issues:?}");
    }

    #[test]
    fn off_grid_values_snap_to_the_nearest_choice() {
        let (prefs, issues) = parse(&doc("[sync]\nwork_items = \"4min\"\npipelines = \"2h\"\n"));
        assert_eq!(prefs.work_items, Cadence::Every(5 * MINUTE));
        assert_eq!(prefs.pipelines, Cadence::Every(30 * MINUTE));
        assert_eq!(issues.len(), 2, "{issues:?}");
        // Exact choices are silent.
        let (prefs, issues) = parse(&doc("[sync]\nwork_items = \"15m\"\n"));
        assert_eq!(prefs.work_items, Cadence::Every(15 * MINUTE));
        assert!(issues.is_empty());
    }

    #[test]
    fn invalid_values_fall_back_per_key() {
        let (prefs, issues) = parse(&doc(
            "[sync]\nmy_prs = 30\nall_prs = \"often\"\nwork_items = \"manual\"\n\
             background = \"sometimes\"\npaused = \"yes\"\npaused_until = 12\n",
        ));
        assert_eq!(prefs.my_prs, Kind::MyPrs.default_cadence());
        assert_eq!(prefs.all_prs, Kind::AllPrs.default_cadence());
        assert_eq!(prefs.work_items, Cadence::Manual, "good keys still apply");
        assert_eq!(prefs.background, Background::Slower);
        assert_eq!(prefs.pause, Pause::Off);
        assert_eq!(issues.len(), 5, "{issues:?}");
        let (prefs, issues) = parse(&doc("sync = 3\n"));
        assert_eq!(prefs, Prefs::default());
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn paused_true_wins_over_a_date() {
        let (prefs, _) = parse(&doc(
            "[sync]\npaused = true\npaused_until = 2026-09-30T12:00:00Z\n",
        ));
        assert_eq!(prefs.pause, Pause::UntilResumed);
        let (prefs, _) = parse(&doc(
            "[sync]\npaused = false\npaused_until = 2026-09-30T12:00:00Z\n",
        ));
        assert_eq!(prefs.pause, Pause::Until(1_790_769_600_000));
    }

    #[test]
    fn copy() {
        assert_eq!(cadence_label(Cadence::Every(30 * SECOND)), "30 s");
        assert_eq!(cadence_label(Cadence::Every(15 * MINUTE)), "15 min");
        assert_eq!(cadence_label(Cadence::Manual), "Manual only");
        assert_eq!(interval_label(HOUR), "1 h");
        assert_eq!(cadence_id(Cadence::Every(30 * SECOND)), "30s");
        assert_eq!(cadence_id(Cadence::Every(2 * MINUTE)), "2m");
        assert_eq!(last_sync_label(None, 0), "Not synced yet");
        assert_eq!(last_sync_label(Some(0), 12 * SECOND), "Synced 12s ago");
        assert_eq!(last_sync_label(Some(0), 3 * MINUTE), "Synced 3 min ago");
        let (headline, detail) = budget_copy(6.0, 0.07);
        assert_eq!(headline, "~3% of your Azure DevOps rate limit");
        assert!(detail.contains("About 6.0 TSTU"), "{detail}");
        assert!(detail.contains("last 5 min: 0.070"), "{detail}");
        assert!(!detail.contains("governor"));
        assert_eq!(
            budget_copy(0.5, 0.0).0,
            "Under 1% of your Azure DevOps rate limit"
        );
        assert!(budget_copy(12.0, 0.0).1.contains("governor will slow"));
        let utc = time::UtcOffset::UTC;
        assert_eq!(
            pause_label(Pause::Until(1_790_769_900_000), 1_790_769_600_000, utc).as_deref(),
            Some("Paused until 12:05")
        );
        assert_eq!(pause_label(Pause::Until(10), 20, utc), None, "expired");
        assert_eq!(
            pause_label(Pause::UntilResumed, 0, utc).as_deref(),
            Some("Paused until you resume")
        );
    }
}
