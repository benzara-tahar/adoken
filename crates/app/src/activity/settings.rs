//! `[activity]` in `settings.toml` (spec 0016 FR-28) and the drawer's
//! geometry (FR-1). Pure: no gpui, no IO.
//!
//! ```toml
//! [activity]
//! panel_height = 300         # px, clamped to the drawer's range
//! tab = "network"            # "sync" | "network"
//! network_indicator = true   # Settings › Sync switch
//! resource_meter = false     # Settings › General switch (spec 0023)
//! ```

use adoken_theme::size;
use toml::{Table, Value};

/// Settings › Sync rows (FR-28): titles and search words.
pub const ACTIVITY_LINK: &str = "Sync activity";
pub const ACTIVITY_LINK_KEYWORDS: &str = "activity drawer passes history status log";
pub const NETWORK_INDICATOR: &str = "Show network activity in the statusbar";
pub const NETWORK_INDICATOR_KEYWORDS: &str =
    "network requests log indicator statusbar http calls failures";

pub const TABLE: &str = "activity";

/// Statusbar height (principles.md §2); the drawer sits above it.
pub const STATUSBAR: f32 = 24.0;
/// Default drawer height: this share of the window (FR-1).
pub const DEFAULT_FRACTION: f32 = 0.4;
/// Tallest drawer: this share of the window (FR-1).
pub const MAX_FRACTION: f32 = 0.7;
/// The content above the drawer never gets shorter than this (FR-1).
pub const CONTENT_FLOOR: f32 = 120.0;
/// Splitter keys: `↑`/`↓` and `⇧↑`/`⇧↓` (FR-1).
pub const STEP: f32 = 24.0;
pub const BIG_STEP: f32 = 96.0;

/// The drawer's two tabs (FR-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Tab {
    #[default]
    Sync,
    Network,
}

impl Tab {
    pub fn id(self) -> &'static str {
        match self {
            Tab::Sync => "sync",
            Tab::Network => "network",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Sync => "Sync",
            Tab::Network => "Network",
        }
    }

    pub fn other(self) -> Tab {
        match self {
            Tab::Sync => Tab::Network,
            Tab::Network => Tab::Sync,
        }
    }

    fn parse(text: &str) -> Option<Tab> {
        match text {
            "sync" => Some(Tab::Sync),
            "network" => Some(Tab::Network),
            _ => None,
        }
    }
}

/// `[activity]` as read or last written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivitySettings {
    /// Drawer height in px; `None` = the default share of the window.
    pub panel_height: Option<f32>,
    pub tab: Tab,
    /// Show the network indicator in the statusbar (FR-21, Q-3).
    pub network_indicator: bool,
    /// Show adoken's memory and CPU in the statusbar (spec 0023 FR-11).
    pub resource_meter: bool,
}

impl Default for ActivitySettings {
    fn default() -> Self {
        Self {
            panel_height: None,
            tab: Tab::Sync,
            network_indicator: true,
            resource_meter: false,
        }
    }
}

/// The drawer's height range in a window `window_height` tall: min 160,
/// max 70 % of the window and never less than 120 px of content above it
/// (at 600 px: 418).
pub fn height_range(window_height: f32) -> (f32, f32) {
    let body = window_height - size::TITLEBAR - STATUSBAR;
    let max = (window_height * MAX_FRACTION)
        .min(body - CONTENT_FLOOR)
        .max(size::DRAWER_MIN);
    (size::DRAWER_MIN, max)
}

pub fn clamp_height(height: f32, window_height: f32) -> f32 {
    let (min, max) = height_range(window_height);
    height.clamp(min, max)
}

/// The height the drawer opens at: the persisted one, else 40 %.
pub fn effective_height(persisted: Option<f32>, window_height: f32) -> f32 {
    clamp_height(
        persisted.unwrap_or(window_height * DEFAULT_FRACTION),
        window_height,
    )
}

/// A splitter key (FR-1): the top edge moves up (taller) or down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitterKey {
    Up,
    Down,
    BigUp,
    BigDown,
    /// `Home`: max.
    Max,
    /// `End`: min.
    Min,
}

pub fn step_height(height: f32, key: SplitterKey, window_height: f32) -> f32 {
    let (min, max) = height_range(window_height);
    match key {
        SplitterKey::Up => clamp_height(height + STEP, window_height),
        SplitterKey::Down => clamp_height(height - STEP, window_height),
        SplitterKey::BigUp => clamp_height(height + BIG_STEP, window_height),
        SplitterKey::BigDown => clamp_height(height - BIG_STEP, window_height),
        SplitterKey::Max => max,
        SplitterKey::Min => min,
    }
}

/// Parse `[activity]` out of the whole file. `window_height` (the main
/// window's, when known) bounds `panel_height`; out-of-range values clamp
/// and are reported (spec 0009 FR-5).
pub fn parse(document: &Table, window_height: Option<f32>) -> (ActivitySettings, Vec<String>) {
    let mut settings = ActivitySettings::default();
    let mut issues = Vec::new();
    let Some(table) = document.get(TABLE) else {
        return (settings, issues);
    };
    let Some(table) = table.as_table() else {
        issues.push(format!("{TABLE}: expected a table; using the defaults"));
        return (settings, issues);
    };
    if let Some(value) = table.get("panel_height") {
        let number = match value {
            Value::Integer(n) => Some(*n as f32),
            Value::Float(n) => Some(*n as f32),
            _ => None,
        };
        match number {
            Some(height) if height < size::DRAWER_MIN => {
                issues.push(format!(
                    "{TABLE}.panel_height: {} is below the {} px minimum; using {}",
                    px_text(height),
                    px_text(size::DRAWER_MIN),
                    px_text(size::DRAWER_MIN)
                ));
                settings.panel_height = Some(size::DRAWER_MIN);
            }
            Some(height) => match window_height.map(height_range) {
                Some((_, max)) if height > max => {
                    issues.push(format!(
                        "{TABLE}.panel_height: {} is taller than the window allows \
                         (70% and 120 px of content); using {}",
                        px_text(height),
                        px_text(max)
                    ));
                    settings.panel_height = Some(max);
                }
                _ => settings.panel_height = Some(height),
            },
            None => issues.push(format!(
                "{TABLE}.panel_height: expected a number of pixels; using the default"
            )),
        }
    }
    if let Some(value) = table.get("tab") {
        match value.as_str().and_then(Tab::parse) {
            Some(tab) => settings.tab = tab,
            None => issues.push(format!(
                "{TABLE}.tab: expected \"sync\" or \"network\"; using \"sync\""
            )),
        }
    }
    if let Some(value) = table.get("network_indicator") {
        match value.as_bool() {
            Some(on) => settings.network_indicator = on,
            None => issues.push(format!(
                "{TABLE}.network_indicator: expected true or false; using true"
            )),
        }
    }
    if let Some(value) = table.get("resource_meter") {
        match value.as_bool() {
            Some(on) => settings.resource_meter = on,
            None => issues.push(format!(
                "{TABLE}.resource_meter: expected true or false; using false"
            )),
        }
    }
    (settings, issues)
}

/// Parse from the file's text (missing or broken file: defaults).
pub fn from_toml(text: &str, window_height: Option<f32>) -> ActivitySettings {
    toml::from_str::<Table>(text)
        .map(|doc| parse(&doc, window_height).0)
        .unwrap_or_default()
}

/// `settings` as the `[activity]` table.
pub fn table(settings: &ActivitySettings) -> Table {
    let mut table = Table::new();
    if let Some(height) = settings.panel_height {
        table.insert("panel_height".into(), Value::Integer(height.round() as i64));
    }
    table.insert("tab".into(), Value::String(settings.tab.id().into()));
    table.insert(
        "network_indicator".into(),
        Value::Boolean(settings.network_indicator),
    );
    table.insert(
        "resource_meter".into(),
        Value::Boolean(settings.resource_meter),
    );
    table
}

fn px_text(value: f32) -> String {
    format!("{}", value.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn range_at_the_minimum_window_keeps_120_px_of_content() {
        // 600 px: body 538, 538 − 120 = 418 < 70 % (420).
        assert_eq!(height_range(600.0), (160.0, 418.0));
        // Tall windows: 70 % wins.
        assert_eq!(height_range(1000.0), (160.0, 700.0));
        assert_eq!(effective_height(None, 600.0), 240.0, "default 40 %");
    }

    #[test]
    fn splitter_keys_move_24_and_96_px_within_the_range() {
        let w = 600.0;
        assert_eq!(step_height(240.0, SplitterKey::Up, w), 264.0);
        assert_eq!(step_height(240.0, SplitterKey::Down, w), 216.0);
        assert_eq!(step_height(240.0, SplitterKey::BigUp, w), 336.0);
        assert_eq!(step_height(240.0, SplitterKey::BigDown, w), 160.0);
        assert_eq!(step_height(400.0, SplitterKey::BigUp, w), 418.0);
        assert_eq!(step_height(240.0, SplitterKey::Max, w), 418.0);
        assert_eq!(step_height(240.0, SplitterKey::Min, w), 160.0);
        // Never less than 120 px of content, from any start.
        for start in [160.0, 300.0, 418.0] {
            let h = step_height(start, SplitterKey::BigUp, w);
            assert!(w - size::TITLEBAR - STATUSBAR - h >= CONTENT_FLOOR);
        }
    }

    #[test]
    fn persisted_height_and_tab_round_trip() {
        // AC-S2: resized to 300 px on Network, restart reopens there.
        let saved = ActivitySettings {
            panel_height: Some(300.0),
            tab: Tab::Network,
            network_indicator: false,
            resource_meter: true,
        };
        let mut root = Table::new();
        root.insert(TABLE.into(), Value::Table(table(&saved)));
        let (read, issues) = parse(&root, Some(760.0));
        assert_eq!(read, saved);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(effective_height(read.panel_height, 760.0), 300.0);
    }

    #[test]
    fn oversized_height_clamps_to_70_percent_and_is_reported() {
        // AC-S2: panel_height = 5000 in a 760 px window → 532.
        let (read, issues) = parse(&doc("[activity]\npanel_height = 5000\n"), Some(760.0));
        assert_eq!(read.panel_height, Some(532.0));
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("5000") && issues[0].contains("532"));
        // Unknown window: kept, and the layout clamps it.
        let (read, _) = parse(&doc("[activity]\npanel_height = 5000\n"), None);
        assert_eq!(effective_height(read.panel_height, 760.0), 532.0);
    }

    #[test]
    fn bad_values_fall_back_one_by_one() {
        let (read, issues) = parse(
            &doc(
                "[activity]\npanel_height = 40\ntab = \"logs\"\nnetwork_indicator = \"no\"\n\
                 resource_meter = \"yes\"\n",
            ),
            None,
        );
        assert_eq!(read.panel_height, Some(160.0));
        assert_eq!(read.tab, Tab::Sync);
        assert!(read.network_indicator);
        assert!(!read.resource_meter, "AC-8: \"yes\" is false and reported");
        assert_eq!(issues.len(), 4, "{issues:?}");
        assert_eq!(parse(&Table::new(), None).0, ActivitySettings::default());
    }
}
