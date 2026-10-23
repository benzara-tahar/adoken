//! Pure Settings logic: tabs, search, and the `settings.toml` codecs for
//! the tables Settings owns (`[general]`, `[nudges]`) plus value edits in
//! the tables it shares (`[sound]`, `[background]`, `[sidebar]`). No gpui,
//! no IO.

use adoken_nudge::{NudgeKind, Settings as NudgeSettings, Threshold, WorkingHours};
use adoken_ui::IconName;
use toml::{Table, Value};

/// The left rail, in order (spec 0009 §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    #[default]
    General,
    Appearance,
    Backgrounds,
    Sounds,
    Notifications,
    Nudges,
    Sync,
    Keyboard,
    Ken,
    Account,
    About,
}

impl Tab {
    pub const ALL: [Self; 11] = [
        Self::General,
        Self::Appearance,
        Self::Backgrounds,
        Self::Sounds,
        Self::Notifications,
        Self::Nudges,
        Self::Sync,
        Self::Keyboard,
        Self::Ken,
        Self::Account,
        Self::About,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Stable id (`ADOKEN_SETTINGS=<id>`, element ids).
    pub fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Backgrounds => "backgrounds",
            Self::Sounds => "sounds",
            Self::Notifications => "notifications",
            Self::Nudges => "nudges",
            Self::Sync => "sync",
            Self::Keyboard => "keyboard",
            Self::Ken => "ken",
            Self::Account => "account",
            Self::About => "about",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|tab| tab.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Backgrounds => "Backgrounds",
            Self::Sounds => "Sounds",
            Self::Notifications => "Notifications",
            Self::Nudges => "Nudges",
            Self::Sync => "Sync",
            Self::Keyboard => "Keyboard",
            Self::Ken => "Ken",
            Self::Account => "Account",
            Self::About => "About",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::SlidersHorizontal,
            Self::Appearance => IconName::Palette,
            Self::Backgrounds => IconName::Image,
            Self::Sounds => IconName::Volume2,
            Self::Notifications => IconName::Bell,
            Self::Nudges => IconName::Lightbulb,
            Self::Sync => IconName::RefreshCw,
            Self::Keyboard => IconName::Keyboard,
            Self::Ken => IconName::Swords,
            Self::Account => IconName::User,
            Self::About => IconName::Info,
        }
    }
}

// ── search ──────────────────────────────────────────────────────────────

/// One searchable setting: what the row says plus words people type for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub tab: Tab,
    pub title: &'static str,
    pub keywords: &'static str,
}

const fn entry(tab: Tab, title: &'static str, keywords: &'static str) -> Entry {
    Entry {
        tab,
        title,
        keywords,
    }
}

/// Search words of the Appearance › Code font rows (spec 0009 §3 Code font).
pub const CODE_FONT_KEYWORDS: &str =
    "code font editor monospace family typeface jetbrains mono sf diff";
pub const CODE_FONT_SIZE_KEYWORDS: &str = "code font editor zoom bigger smaller text diff";
pub const CODE_LIGATURES_KEYWORDS: &str = "code font editor calt arrows";

/// The search index: every row or section Settings shows.
pub const ENTRIES: &[Entry] = &[
    entry(
        Tab::General,
        "Motion",
        "animation reduce motion accessibility",
    ),
    entry(Tab::General, "Sidebar", "collapse icons rail compact"),
    entry(
        Tab::General,
        crate::resource::SETTING,
        crate::resource::SETTING_KEYWORDS,
    ),
    entry(
        Tab::General,
        "Show group reviewers",
        "group team policy reviewers required pull requests",
    ),
    entry(Tab::Appearance, "Theme", "color dark light"),
    entry(Tab::Appearance, "Code font", CODE_FONT_KEYWORDS),
    entry(Tab::Appearance, "Code font size", CODE_FONT_SIZE_KEYWORDS),
    entry(Tab::Appearance, "Ligatures", CODE_LIGATURES_KEYWORDS),
    entry(
        Tab::Backgrounds,
        "Background",
        "wallpaper image picture builtin dusk lantern ink",
    ),
    entry(
        Tab::Backgrounds,
        "Choose image…",
        "file picker upload photo wallpaper",
    ),
    entry(Tab::Backgrounds, "Dim", "darken scrim readability contrast"),
    entry(Tab::Backgrounds, "Blur", "frost soften"),
    entry(
        Tab::Backgrounds,
        "Per-view override",
        "view inbox review hide show scope",
    ),
    entry(Tab::Sounds, "Sounds", "audio enable on off mute"),
    entry(Tab::Sounds, "Volume", "master loud quiet level"),
    entry(
        Tab::Notifications,
        "Banners",
        "permission macos system alerts",
    ),
    entry(
        Tab::Notifications,
        "Priority rules",
        "event priority p0 p1 p2 p3",
    ),
    entry(Tab::Notifications, "Delivery", "banner sound inbox only"),
    entry(
        Tab::Notifications,
        "Quiet hours",
        "do not disturb night weekend let p0",
    ),
    entry(
        Tab::Notifications,
        "Delivery windows",
        "work hours schedule summary",
    ),
    entry(
        Tab::Notifications,
        "Show in inbox",
        "minimum priority source filter",
    ),
    entry(Tab::Notifications, "Show my own activity", "self own"),
    entry(Tab::Notifications, "Dock badge", "count unread"),
    entry(Tab::Notifications, "Your rules", "custom user rules"),
    entry(Tab::Notifications, "Muted", "unmute mutes"),
    entry(
        Tab::Nudges,
        "Nudge types",
        "review waiting build failing merge conflicts stale n1 n2 n3 n4 n5 n6 n7",
    ),
    entry(Tab::Nudges, "Thresholds", "hours days wait after"),
    entry(Tab::Nudges, "Daily cap", "limit per day maximum"),
    entry(
        Tab::Nudges,
        "Working hours",
        "work days schedule start end weekend",
    ),
    entry(Tab::Sync, "Sync now", "refresh reload fetch last synced"),
    entry(
        Tab::Sync,
        "My pull requests",
        "pr pull request reviews mine waiting on me interval refresh",
    ),
    entry(
        Tab::Sync,
        "All active pull requests",
        "pr pull requests all active list interval refresh",
    ),
    entry(
        Tab::Sync,
        "Work items",
        "work items tasks bugs boards interval refresh",
    ),
    entry(
        Tab::Sync,
        "Pipeline runs",
        "pipelines builds runs ci interval refresh",
    ),
    entry(
        Tab::Sync,
        "In the background",
        "background inactive unfocused slower battery",
    ),
    entry(Tab::Sync, "Pause sync", "pause resume stop hold hour"),
    entry(
        Tab::Sync,
        crate::activity::settings::ACTIVITY_LINK,
        crate::activity::settings::ACTIVITY_LINK_KEYWORDS,
    ),
    entry(
        Tab::Sync,
        crate::activity::settings::NETWORK_INDICATOR,
        crate::activity::settings::NETWORK_INDICATOR_KEYWORDS,
    ),
    entry(
        Tab::Sync,
        "Rate limit",
        "budget tstu throttle usage azure devops estimate",
    ),
    entry(
        Tab::Keyboard,
        crate::hotkey::SECTION,
        crate::hotkey::SECTION_KEYWORDS,
    ),
    entry(Tab::Keyboard, "Keyboard shortcuts", "keys bindings hotkeys"),
    entry(Tab::Ken, "Arcade mode", "ken combo ko streak playful"),
    entry(
        Tab::Account,
        "Account",
        "organization org token pat expiry sign out demo",
    ),
    entry(Tab::About, "About", "version licenses third party credits"),
];

/// Every whitespace-separated token of `query` appears in `text`
/// (case-insensitive). An empty query matches everything.
pub fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|token| text.contains(&token.to_lowercase()))
}

/// The tab's name or any of its entries matches.
pub fn tab_matches(query: &str, tab: Tab) -> bool {
    matches(query, tab.label())
        || ENTRIES
            .iter()
            .filter(|entry| entry.tab == tab)
            .any(|entry| matches(query, &format!("{} {}", entry.title, entry.keywords)))
}

/// Whether a row (`title` plus `keywords`) on `tab` shows under `query`: a
/// query naming the tab shows the whole tab.
pub fn row_visible(query: &str, tab: Tab, title: &str, keywords: &str) -> bool {
    query.trim().is_empty()
        || matches(query, tab.label())
        || matches(query, &format!("{title} {keywords}"))
}

/// First tab matching `query`.
pub fn first_match(query: &str) -> Option<Tab> {
    Tab::ALL.into_iter().find(|tab| tab_matches(query, *tab))
}

/// The next matching tab from `current` in `delta`'s direction (no wrap);
/// `current` when there is none.
pub fn step_tab(current: Tab, delta: isize, query: &str) -> Tab {
    let mut ix = current.index() as isize;
    loop {
        ix += delta.signum();
        let Some(tab) = usize::try_from(ix).ok().and_then(|ix| Tab::ALL.get(ix)) else {
            return current;
        };
        if tab_matches(query, *tab) {
            return *tab;
        }
    }
}

// ── table edits ─────────────────────────────────────────────────────────

/// Set `path` (dotted, relative to `table`) to `value`, creating tables.
pub fn set_value(table: &mut Table, path: &[&str], value: impl Into<Value>) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = table;
    for key in parents {
        let entry = current
            .entry((*key).to_owned())
            .or_insert_with(|| Value::Table(Table::new()));
        if !entry.is_table() {
            *entry = Value::Table(Table::new());
        }
        current = entry.as_table_mut().expect("just made a table");
    }
    current.insert((*last).to_owned(), value.into());
}

/// Remove `path`; empty parent tables go too.
pub fn remove_value(table: &mut Table, path: &[&str]) {
    match path {
        [] => {}
        [key] => {
            table.remove(*key);
        }
        [key, rest @ ..] => {
            if let Some(Value::Table(sub)) = table.get_mut(*key) {
                remove_value(sub, rest);
                if sub.is_empty() {
                    table.remove(*key);
                }
            }
        }
    }
}

/// `value` rounded to hundredths, as TOML (keeps the file tidy: `0.35`, not
/// `0.3499999940395355`).
pub fn tidy(value: f32) -> Value {
    Value::Float((f64::from(value) * 100.0).round() / 100.0)
}

/// `[name]` out of the whole file, or an empty table.
pub fn table_of(document: &Table, name: &str) -> Table {
    document
        .get(name)
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default()
}

// ── [background] ────────────────────────────────────────────────────────

/// Image formats the background pipeline imports (tokens.md §10.3).
pub const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

pub fn image_supported(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|ok| ok.eq_ignore_ascii_case(ext))
        })
}

/// The table equivalent of demo mode's default background
/// (`background::demo_settings`), so the first edit keeps what shows.
pub fn demo_background_table(illustration: &str) -> Table {
    let mut table = Table::new();
    table.insert("source".into(), illustration.into());
    table.insert("blur".into(), 6.into());
    table.insert("dim".into(), tidy(0.3));
    table.insert("surface_opacity".into(), tidy(0.7));
    table
}

// ── [general] ───────────────────────────────────────────────────────────

pub const GENERAL_TABLE: &str = "general";
pub const SIDEBAR_TABLE: &str = "sidebar";

/// Spec 0007 §3.6: follow macOS, or override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Motion {
    #[default]
    System,
    Reduced,
    Full,
}

impl Motion {
    pub const ALL: [Self; 3] = [Self::System, Self::Reduced, Self::Full];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Reduced => "reduced",
            Self::Full => "full",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Reduced => "Reduced",
            Self::Full => "Full",
        }
    }

    /// `None` = follow the system.
    pub fn reduce(self) -> Option<bool> {
        match self {
            Self::System => None,
            Self::Reduced => Some(true),
            Self::Full => Some(false),
        }
    }

    /// `[general] motion`; unknown values are the default, with an issue.
    pub fn from_document(document: &Table) -> (Self, Option<String>) {
        match document
            .get(GENERAL_TABLE)
            .and_then(|general| general.get("motion"))
        {
            None => (Self::System, None),
            Some(Value::String(id)) => match Self::ALL.into_iter().find(|m| m.id() == id) {
                Some(motion) => (motion, None),
                None => (
                    Self::System,
                    Some(format!(
                        "general.motion: unknown value \"{id}\"; using \"system\""
                    )),
                ),
            },
            Some(_) => (
                Self::System,
                Some("general.motion: expected \"system\", \"reduced\" or \"full\"".into()),
            ),
        }
    }
}

/// `[sidebar] collapsed` (the shell owns the table; Settings flips one key).
pub fn sidebar_collapsed(document: &Table) -> bool {
    document
        .get(SIDEBAR_TABLE)
        .and_then(|sidebar| sidebar.get("collapsed"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

// ── [nudges] ────────────────────────────────────────────────────────────

pub const NUDGES_TABLE: &str = "nudges";
pub const THRESHOLD_HOURS: (u32, u32) = (1, 720);
pub const THRESHOLD_DAYS: (u32, u32) = (1, 30);
const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Short weekday names, Monday first (bit order of `WorkingHours::days`).
pub fn weekday_label(ix: usize) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][ix]
}

/// `"now"`, `"<n>h"`, `"<n>d"`, clamped to the allowed range.
pub fn parse_threshold(text: &str) -> Option<(Threshold, bool)> {
    let text = text.trim().to_ascii_lowercase();
    if text == "now" || text == "0" || text == "0h" {
        return Some((Threshold::Immediate, false));
    }
    let clamp = |n: u32, (lo, hi): (u32, u32)| (n.clamp(lo, hi), n != n.clamp(lo, hi));
    if let Some(hours) = text.strip_suffix('h') {
        let (n, clamped) = clamp(hours.trim().parse().ok()?, THRESHOLD_HOURS);
        return Some((Threshold::WorkingHours(n), clamped));
    }
    if let Some(days) = text.strip_suffix('d') {
        let (n, clamped) = clamp(days.trim().parse().ok()?, THRESHOLD_DAYS);
        return Some((Threshold::WorkingDays(n), clamped));
    }
    None
}

pub fn threshold_text(threshold: Threshold) -> String {
    match threshold {
        Threshold::Immediate => "now".into(),
        Threshold::WorkingHours(h) => format!("{h}h"),
        Threshold::WorkingDays(d) => format!("{d}d"),
    }
}

fn parse_clock(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

pub fn clock_text(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Latest start that still leaves a half-hour working day.
const LAST_START: u16 = 23 * 60 + 30;

/// Move the start, pushing the end later to keep `start < end`. Starts
/// that leave no room are ignored.
pub fn set_work_start(hours: &mut WorkingHours, minutes: u16) {
    if minutes >= LAST_START {
        return;
    }
    hours.start_min = minutes;
    if hours.end_min <= minutes {
        hours.end_min = minutes + 30;
    }
}

/// Move the end, pulling the start earlier to keep `start < end`. Midnight
/// (an empty day) is ignored.
pub fn set_work_end(hours: &mut WorkingHours, minutes: u16) {
    if minutes == 0 {
        return;
    }
    hours.end_min = minutes;
    if hours.start_min >= minutes {
        hours.start_min = minutes.saturating_sub(30);
    }
}

/// Parse `[nudges]` (spec 0006 §3.4) over the defaults. Bad keys fall back
/// one by one; out-of-range numbers clamp. Returns the issues found.
pub fn parse_nudges(document: &Table) -> (NudgeSettings, Vec<String>) {
    let mut settings = NudgeSettings::default();
    let mut issues = Vec::new();
    let Some(value) = document.get(NUDGES_TABLE) else {
        return (settings, issues);
    };
    let Some(table) = value.as_table() else {
        issues.push("nudges: expected a table".into());
        return (settings, issues);
    };
    let mut hours = WorkingHours::default();
    for (key, value) in table {
        let path = format!("nudges.{key}");
        match key.as_str() {
            "daily_cap" => match value.as_integer() {
                Some(cap) => {
                    let clamped = cap.clamp(0, i64::from(NudgeSettings::MAX_CAP));
                    if clamped != cap {
                        issues.push(format!("{path}: {cap} is out of range; using {clamped}"));
                    }
                    settings.set_daily_cap(clamped as u8);
                }
                None => issues.push(format!("{path}: expected a number")),
            },
            "days" => match value.as_array() {
                Some(days) => {
                    let mut mask = 0u8;
                    for day in days {
                        match day
                            .as_str()
                            .and_then(|d| WEEKDAYS.iter().position(|w| w.eq_ignore_ascii_case(d)))
                        {
                            Some(ix) => mask |= 1 << ix,
                            None => issues.push(format!("{path}: unknown day {day}")),
                        }
                    }
                    hours.days = mask;
                }
                None => issues.push(format!("{path}: expected a list like [\"mon\", \"tue\"]")),
            },
            "start" | "end" => match value.as_str().and_then(parse_clock) {
                Some(minutes) if key == "start" => hours.start_min = minutes,
                Some(minutes) => hours.end_min = minutes,
                None => issues.push(format!("{path}: expected \"HH:MM\"")),
            },
            id => match (NudgeKind::parse(id), value.as_table()) {
                (Some(kind), Some(sub)) => {
                    for (field, value) in sub {
                        let path = format!("{path}.{field}");
                        match (field.as_str(), value) {
                            ("enabled", Value::Boolean(on)) => settings.set_enabled(kind, *on),
                            ("threshold", Value::String(text)) => match parse_threshold(text) {
                                Some((threshold, clamped)) => {
                                    if clamped {
                                        issues.push(format!(
                                            "{path}: \"{text}\" is out of range; using \"{}\"",
                                            threshold_text(threshold)
                                        ));
                                    }
                                    settings.set_threshold(kind, threshold);
                                }
                                None => issues.push(format!(
                                    "{path}: \"{text}\" isn't \"now\", \"<n>h\" or \"<n>d\"; using the default"
                                )),
                            },
                            _ => issues.push(format!("{path}: unknown or invalid key")),
                        }
                    }
                }
                _ => issues.push(format!("{path}: unknown key")),
            },
        }
    }
    if hours.start_min >= hours.end_min {
        issues.push("nudges: start must be before end; using 09:00–18:00".into());
        let default = WorkingHours::default();
        hours.start_min = default.start_min;
        hours.end_min = default.end_min;
    }
    settings.hours = hours;
    (settings, issues)
}

/// `settings` as the `[nudges]` table (the time zone is not persisted: it
/// always follows the machine).
pub fn nudges_table(settings: &NudgeSettings) -> Table {
    let mut table = Table::new();
    table.insert("daily_cap".into(), i64::from(settings.daily_cap()).into());
    let days: Vec<Value> = WEEKDAYS
        .iter()
        .enumerate()
        .filter(|(ix, _)| settings.hours.days & (1 << ix) != 0)
        .map(|(_, day)| Value::String((*day).into()))
        .collect();
    table.insert("days".into(), Value::Array(days));
    table.insert("start".into(), clock_text(settings.hours.start_min).into());
    table.insert("end".into(), clock_text(settings.hours.end_min).into());
    for kind in NudgeKind::ALL {
        let mut sub = Table::new();
        sub.insert("enabled".into(), settings.enabled(kind).into());
        sub.insert(
            "threshold".into(),
            threshold_text(settings.kind(kind).threshold).into(),
        );
        table.insert(kind.id().into(), Value::Table(sub));
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Table {
        toml::from_str(text).expect("valid test TOML")
    }

    #[test]
    fn tab_ids_round_trip() {
        for tab in Tab::ALL {
            assert_eq!(Tab::parse(tab.id()), Some(tab));
        }
        assert_eq!(Tab::parse(" Sounds "), Some(Tab::Sounds));
        assert_eq!(Tab::parse("nope"), None);
    }

    #[test]
    fn quiet_finds_notifications_and_sounds_only() {
        let tabs: Vec<Tab> = Tab::ALL
            .into_iter()
            .filter(|tab| tab_matches("quiet", *tab))
            .collect();
        assert_eq!(tabs, vec![Tab::Sounds, Tab::Notifications]);
        assert_eq!(first_match("QUIET"), Some(Tab::Sounds));
        assert_eq!(first_match("zzz"), None);
    }

    #[test]
    fn search_tokens_must_all_match_and_tab_names_show_the_whole_tab() {
        assert!(matches("quiet p0", "Quiet hours do not disturb let p0"));
        assert!(!matches("quiet banana", "Quiet hours"));
        assert!(row_visible("", Tab::Sounds, "Volume", ""));
        assert!(row_visible("sounds", Tab::Sounds, "Volume", "master"));
        assert!(!row_visible("dim", Tab::Sounds, "Volume", "master"));
        assert!(row_visible("dim", Tab::Backgrounds, "Dim", "darken"));
    }

    #[test]
    fn search_finds_no_theme_import_and_one_global_shortcut() {
        // Spec 0025 AC-9 (theme and Keyboard parts): FR-11 and FR-12.
        for query in ["vscode", "import theme", "summon_palette", "search open"] {
            assert!(
                !ENTRIES
                    .iter()
                    .any(|e| matches(query, &format!("{} {}", e.title, e.keywords))),
                "{query:?} still finds a setting"
            );
        }
        let global =
            |e: &&Entry| matches("global shortcut", &format!("{} {}", e.title, e.keywords));
        let rows: Vec<_> = ENTRIES.iter().filter(global).collect();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].tab, Tab::Keyboard);
    }

    #[test]
    fn sounds_page_indexes_only_on_off_and_volume() {
        // Spec 0025 AC-9 (Sounds part): FR-13.
        let sounds: Vec<&Entry> = ENTRIES.iter().filter(|e| e.tab == Tab::Sounds).collect();
        let titles: Vec<&str> = sounds.iter().map(|e| e.title).collect();
        assert_eq!(titles, ["Sounds", "Volume"]);
        for query in [
            "sound pack",
            "dojo",
            "arcade",
            "categories",
            "background",
            "unfocused",
            "urgent",
            "quiet hours",
        ] {
            assert!(
                !sounds
                    .iter()
                    .any(|e| matches(query, &format!("{} {}", e.title, e.keywords))),
                "{query:?} still finds a sound setting"
            );
        }
    }

    #[test]
    fn stepping_skips_non_matching_tabs_and_stops_at_the_ends() {
        assert_eq!(step_tab(Tab::General, 1, ""), Tab::Appearance);
        assert_eq!(step_tab(Tab::Sounds, 1, "quiet"), Tab::Notifications);
        assert_eq!(step_tab(Tab::Notifications, 1, "quiet"), Tab::Notifications);
        assert_eq!(step_tab(Tab::General, -1, ""), Tab::General);
        assert_eq!(step_tab(Tab::About, -1, "quiet"), Tab::Notifications);
    }

    #[test]
    fn set_and_remove_paths_create_and_prune_tables() {
        let mut table = Table::new();
        set_value(&mut table, &["views", "inbox", "dim"], tidy(0.349_99));
        assert_eq!(table, doc("[views.inbox]\ndim = 0.35\n"));
        set_value(&mut table, &["views", "inbox", "blur"], 12);
        remove_value(&mut table, &["views", "inbox", "dim"]);
        assert_eq!(table, doc("[views.inbox]\nblur = 12\n"));
        remove_value(&mut table, &["views", "inbox", "blur"]);
        assert!(table.is_empty(), "empty parents are pruned: {table:?}");
        // A scalar in the way becomes a table.
        let mut table = doc("category = 3\n");
        set_value(&mut table, &["category", "ui"], tidy(0.5));
        assert_eq!(table, doc("[category]\nui = 0.5\n"));
    }

    #[test]
    fn only_importable_images_are_accepted() {
        use std::path::Path;
        assert!(image_supported(Path::new("/tmp/wall.JPG")));
        assert!(image_supported(Path::new("a.webp")));
        assert!(!image_supported(Path::new("clip.mov")));
        assert!(!image_supported(Path::new("noext")));
    }

    #[test]
    fn motion_parses_with_fallback() {
        assert_eq!(Motion::from_document(&doc("")), (Motion::System, None));
        assert_eq!(
            Motion::from_document(&doc("[general]\nmotion = \"reduced\"\n")).0,
            Motion::Reduced
        );
        let (motion, issue) = Motion::from_document(&doc("[general]\nmotion = \"slow\"\n"));
        assert_eq!(motion, Motion::System);
        assert!(issue.unwrap().contains("slow"));
        assert_eq!(Motion::Full.reduce(), Some(false));
        assert_eq!(Motion::System.reduce(), None);
    }

    #[test]
    fn nudges_round_trip_through_the_table() {
        let mut settings = NudgeSettings::default();
        settings.set_daily_cap(5);
        settings.set_enabled(NudgeKind::QuietActiveItem, false);
        settings.set_threshold(NudgeKind::ReviewWaiting, Threshold::WorkingHours(8));
        settings.set_threshold(NudgeKind::MergedStillActive, Threshold::WorkingDays(2));
        settings.hours.days = 0b001_1110; // Tue–Fri
        settings.hours.start_min = 8 * 60 + 30;
        settings.hours.end_min = 17 * 60;
        let mut document = Table::new();
        document.insert(NUDGES_TABLE.into(), Value::Table(nudges_table(&settings)));
        // Through text, like the real file.
        let text = toml::to_string(&document).unwrap();
        let (parsed, issues) = parse_nudges(&doc(&text));
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(parsed, settings);
    }

    #[test]
    fn nudges_validation_clamps_and_falls_back_per_key() {
        let (settings, issues) = parse_nudges(&doc("[nudges]\n\
             daily_cap = 25\n\
             start = \"18:00\"\n\
             end = \"09:00\"\n\
             days = [\"mon\", \"funday\"]\n\
             [nudges.N1]\n\
             threshold = \"900h\"\n\
             [nudges.N7]\n\
             threshold = \"soon\"\n\
             enabled = false\n\
             [nudges.N9]\n\
             enabled = true\n"));
        assert_eq!(settings.daily_cap(), 10);
        assert_eq!(
            settings.kind(NudgeKind::ReviewWaiting).threshold,
            Threshold::WorkingHours(720)
        );
        // A bad threshold keeps the default; the same table's good key applies.
        assert_eq!(
            settings.kind(NudgeKind::QuietActiveItem).threshold,
            NudgeKind::QuietActiveItem.default_threshold()
        );
        assert!(!settings.enabled(NudgeKind::QuietActiveItem));
        assert_eq!(settings.hours.days, 1, "Monday only");
        assert_eq!(
            (settings.hours.start_min, settings.hours.end_min),
            (9 * 60, 18 * 60)
        );
        let joined = issues.join("\n");
        for needle in [
            "daily_cap",
            "900h",
            "soon",
            "funday",
            "N9",
            "start must be before end",
        ] {
            assert!(joined.contains(needle), "missing {needle}: {joined}");
        }
    }

    #[test]
    fn working_hours_edits_keep_start_before_end() {
        let mut hours = WorkingHours::default();
        set_work_start(&mut hours, 19 * 60);
        assert_eq!((hours.start_min, hours.end_min), (19 * 60, 19 * 60 + 30));
        set_work_end(&mut hours, 8 * 60);
        assert_eq!((hours.start_min, hours.end_min), (7 * 60 + 30, 8 * 60));
        set_work_end(&mut hours, 0);
        set_work_start(&mut hours, 23 * 60 + 30);
        assert_eq!(
            (hours.start_min, hours.end_min),
            (7 * 60 + 30, 8 * 60),
            "no-ops"
        );
        set_work_end(&mut hours, 30);
        assert!(hours.start_min < hours.end_min);
    }

    #[test]
    fn thresholds_parse_every_unit() {
        assert_eq!(parse_threshold("now"), Some((Threshold::Immediate, false)));
        assert_eq!(
            parse_threshold(" 4h "),
            Some((Threshold::WorkingHours(4), false))
        );
        assert_eq!(
            parse_threshold("45d"),
            Some((Threshold::WorkingDays(30), true))
        );
        assert_eq!(
            parse_threshold("0d"),
            Some((Threshold::WorkingDays(1), true))
        );
        assert_eq!(parse_threshold("4 hours"), None);
        assert_eq!(parse_threshold("h"), None);
    }
}
