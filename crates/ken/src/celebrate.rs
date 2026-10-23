//! Celebrations for finished work (spec 0022, ken.md §12): the pure,
//! clock-injected rules. `settings.toml [celebrations]` with per-key
//! fallback, the tier of a state (FR-2), forward-move ranking with the 24 h
//! item memory (FR-3), the per-surface switches (FR-4), the K.O. handoff
//! (FR-6), the style (FR-7), Reduce Motion (FR-9), quiet hours and Quiet
//! mode (FR-10, spec 0025 FR-8) and the rate limits (FR-11).
//!
//! [`decide`] maps one trigger to one [`Decision`]; the app renders it.
//! Celebrations apply with Ken on or off: only the arcade style and the
//! K.O. handoff read Ken's state, through [`Conditions`].

use std::collections::{HashMap, VecDeque};

use adoken_model::time::{HOUR, MINUTE};
use adoken_model::{Millis, StateCategory};

/// The `[celebrations]` table name in `settings.toml`.
pub const SETTINGS_TABLE: &str = "celebrations";
/// FR-3: an item doesn't celebrate the same or a lower tier again within.
pub const ITEM_MEMORY_MS: Millis = 24 * HOUR;
/// FR-11: a full effect starts at most once per this gap…
pub const MIN_GAP_MS: Millis = 1_500;
/// …and at most [`MAX_PER_WINDOW`] times per rolling window.
pub const RATE_WINDOW_MS: Millis = 10 * MINUTE;
pub const MAX_PER_WINDOW: usize = 5;
/// FR-9: the static `sparkles` glyph stays this long.
pub const GLYPH_MS: Millis = 3_000;

/// FR-14 defaults.
pub const DEFAULT_STEP_STATES: [&str; 4] =
    ["Code Review", "In Review", "Ready for Review", "Review"];
pub const DEFAULT_MILESTONE_STATES: [&str; 2] = ["Committed", "Done"];

/// How big a finished piece of work is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Into review, or a resolved state: a puff.
    Step,
    /// Committed, Done, a completed state, or my PR merged: a burst.
    Milestone,
}

/// `settings.toml [celebrations]` (spec 0022 FR-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CelebrationSettings {
    /// Master switch (Q-1: on by default, calm style).
    pub enabled: bool,
    pub pull_requests: bool,
    pub work_items: bool,
    pub step_states: Vec<String>,
    pub milestone_states: Vec<String>,
}

impl Default for CelebrationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            pull_requests: true,
            work_items: true,
            step_states: DEFAULT_STEP_STATES.map(str::to_owned).to_vec(),
            milestone_states: DEFAULT_MILESTONE_STATES.map(str::to_owned).to_vec(),
        }
    }
}

impl CelebrationSettings {
    /// `[celebrations]` out of a whole `settings.toml`. A file that doesn't
    /// parse means the defaults (the Settings window reports the syntax
    /// error); see [`Self::parse`] for the table itself.
    pub fn from_settings_toml(text: &str) -> (Self, Vec<String>) {
        match text.parse::<toml::Table>() {
            Ok(document) => Self::parse(&document),
            Err(_) => (Self::default(), Vec::new()),
        }
    }

    /// Per-key fallback (0009 FR-5): a key of the wrong type restores that
    /// key's default; non-string list entries are dropped. Returns the
    /// issues found, for Settings to report.
    pub fn parse(document: &toml::Table) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut issues = Vec::new();
        let Some(value) = document.get(SETTINGS_TABLE) else {
            return (settings, issues);
        };
        let Some(table) = value.as_table() else {
            issues.push(format!(
                "{SETTINGS_TABLE}: expected a table; using the defaults"
            ));
            return (settings, issues);
        };
        for (key, value) in table {
            let path = format!("{SETTINGS_TABLE}.{key}");
            match key.as_str() {
                "enabled" | "pull_requests" | "work_items" => {
                    let slot = match key.as_str() {
                        "enabled" => &mut settings.enabled,
                        "pull_requests" => &mut settings.pull_requests,
                        _ => &mut settings.work_items,
                    };
                    match value.as_bool() {
                        Some(on) => *slot = on,
                        None => issues
                            .push(format!("{path}: expected true or false; using the default")),
                    }
                }
                "step_states" | "milestone_states" => {
                    let slot = if key == "step_states" {
                        &mut settings.step_states
                    } else {
                        &mut settings.milestone_states
                    };
                    let Some(entries) = value.as_array() else {
                        issues.push(format!(
                            "{path}: expected a list like [\"Done\"]; using the default"
                        ));
                        continue;
                    };
                    let names: Vec<String> = entries
                        .iter()
                        .filter_map(|entry| entry.as_str())
                        .map(|name| name.trim().to_owned())
                        .filter(|name| !name.is_empty())
                        .collect();
                    let dropped = entries.iter().filter(|entry| !entry.is_str()).count();
                    if dropped > 0 {
                        issues.push(format!(
                            "{path}: {dropped} entr{} not a state name; ignored",
                            if dropped == 1 { "y is" } else { "ies are" }
                        ));
                    }
                    *slot = names;
                }
                _ => issues.push(format!("{path}: unknown key")),
            }
        }
        (settings, issues)
    }

    /// The `[celebrations]` table to write back (every key, so the file
    /// shows what applies).
    pub fn to_table(&self) -> toml::Table {
        let list = |names: &[String]| {
            toml::Value::Array(names.iter().cloned().map(toml::Value::String).collect())
        };
        let mut table = toml::Table::new();
        table.insert("enabled".into(), self.enabled.into());
        table.insert("pull_requests".into(), self.pull_requests.into());
        table.insert("work_items".into(), self.work_items.into());
        table.insert("step_states".into(), list(&self.step_states));
        table.insert("milestone_states".into(), list(&self.milestone_states));
        table
    }

    /// FR-2: the tier of `state` whose category is `category`.
    pub fn tier(&self, state: &str, category: StateCategory) -> Option<Tier> {
        if matches!(category, StateCategory::Proposed | StateCategory::Removed) {
            return None;
        }
        let state = state.trim();
        let listed = |names: &[String]| names.iter().any(|n| n.trim().eq_ignore_ascii_case(state));
        if listed(&self.milestone_states) || category == StateCategory::Completed {
            Some(Tier::Milestone)
        } else if listed(&self.step_states) || category == StateCategory::Resolved {
            Some(Tier::Step)
        } else {
            None
        }
    }

    /// Settings › General's read-only summary of the two lists (ken.md
    /// §12.5): "Small: Code Review, …, and resolved states. Big: Committed,
    /// Done, and completed states."
    pub fn summary(&self) -> String {
        let part = |label: &str, names: &[String], category: &str| {
            if names.is_empty() {
                format!("{label}: {category} states.")
            } else {
                format!("{label}: {}, and {category} states.", names.join(", "))
            }
        };
        format!(
            "{} {}",
            part("Small", &self.step_states, "resolved"),
            part("Big", &self.milestone_states, "completed")
        )
    }
}

/// FR-3's rank, compared tier first, then category order (Removed <
/// Proposed < In Progress < Resolved < Completed). Tier first keeps AC-2:
/// Committed (a Milestone) → Resolved (a Step) isn't a forward move even
/// though Resolved sits in a later category; Committed → Done still is.
fn rank(category: StateCategory, tier: Option<Tier>) -> (u8, u8) {
    let category = match category {
        StateCategory::Removed => 0,
        StateCategory::Proposed => 1,
        StateCategory::InProgress => 2,
        StateCategory::Resolved => 3,
        StateCategory::Completed => 4,
    };
    let tier = match tier {
        None => 0,
        Some(Tier::Step) => 1,
        Some(Tier::Milestone) => 2,
    };
    (tier, category)
}

/// A confirmed state write of mine: the base state the write started from
/// and the state Azure DevOps answered with (server wins).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateMove<'a> {
    pub id: i64,
    pub from: &'a str,
    pub from_category: StateCategory,
    pub to: &'a str,
    pub to_category: StateCategory,
}

/// What may celebrate (FR-1): nothing sync observes is a trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger<'a> {
    /// `WiEdits::answered` with `Ok(Applied)` for a state change.
    WorkItem(StateMove<'a>),
    /// A PR whose Complete I pressed here, confirmed `completed` while the
    /// completing mark was set. `ko`: Ken's K.O. applies (ken.md §3.3: Ken
    /// on, `ko` on, I authored it, closed within 24 h).
    PullRequest { id: i64, ko: bool },
    /// "Preview celebration": a Milestone ignoring the master switch and
    /// the rate limits (FR-15).
    Preview,
}

/// The moment besides the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Conditions {
    /// adoken's window is key and not minimized (FR-8).
    pub window_active: bool,
    pub reduce_motion: bool,
    /// Spec 0005 quiet hours.
    pub quiet_hours: bool,
    /// Quiet mode (spec 0025 FR-8; works with Ken off).
    pub quiet_mode: bool,
    /// Ken on at `arcade` intensity with `particles` on (FR-7).
    pub ken_arcade: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Confetti (default; Ken off or `subtle`, quiet hours).
    Calm,
    /// Sparks, and the seal stamp for a Milestone.
    Arcade,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Full(Style),
    /// The `sparkles` glyph for 3 s (+ the toast for a Milestone).
    /// `limited`: over a rate limit (plays `ui.action_done`), else Reduce
    /// Motion (plays the celebration sound).
    Static {
        limited: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// No celebration: the normal write feedback.
    None,
    /// Ken's K.O. is the celebration; it starts now (FR-6).
    Ko,
    Celebrate {
        tier: Tier,
        show: Show,
    },
}

/// The one sound for a decision (FR-6, FR-13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// The write's usual feedback (`ui.action_done` for a work item; a PR
    /// confirmation has none).
    Plain,
    /// The K.O. plays `ken.ko` itself.
    KoOnly,
    CelebrateStep,
    CelebrateMilestone,
}

impl Decision {
    pub fn sound(self) -> Sound {
        match self {
            Decision::None => Sound::Plain,
            Decision::Ko => Sound::KoOnly,
            Decision::Celebrate {
                show: Show::Static { limited: true },
                ..
            } => Sound::Plain,
            Decision::Celebrate {
                tier: Tier::Step, ..
            } => Sound::CelebrateStep,
            Decision::Celebrate {
                tier: Tier::Milestone,
                ..
            } => Sound::CelebrateMilestone,
        }
    }

    /// Anything shows (effect, glyph or toast).
    pub fn celebrates(self) -> bool {
        matches!(self, Decision::Celebrate { .. })
    }
}

/// Per-session, in-memory history (FR-3, FR-11). Allocates nothing until
/// the first celebration.
#[derive(Clone, Debug, Default)]
pub struct Memory {
    /// Item → the highest tier celebrated and when.
    items: HashMap<i64, (Tier, Millis)>,
    /// Start times of full effects in the last [`RATE_WINDOW_MS`].
    full: VecDeque<Millis>,
}

impl Memory {
    fn celebrated(&self, id: i64, now: Millis) -> Option<Tier> {
        self.items
            .get(&id)
            .filter(|(_, at)| now - at < ITEM_MEMORY_MS)
            .map(|(tier, _)| *tier)
    }

    fn remember(&mut self, id: i64, tier: Tier, now: Millis) {
        self.items.retain(|_, (_, at)| now - *at < ITEM_MEMORY_MS);
        self.items.insert(id, (tier, now));
    }

    fn limited(&mut self, now: Millis) -> bool {
        while self
            .full
            .front()
            .is_some_and(|at| now - at >= RATE_WINDOW_MS)
        {
            self.full.pop_front();
        }
        self.full.back().is_some_and(|last| now - last < MIN_GAP_MS)
            || self.full.len() >= MAX_PER_WINDOW
    }
}

/// One trigger → one decision. Updates `memory` when something celebrates.
pub fn decide(
    trigger: Trigger<'_>,
    settings: &CelebrationSettings,
    conditions: Conditions,
    memory: &mut Memory,
    now: Millis,
) -> Decision {
    let preview = trigger == Trigger::Preview;
    let surface_on = match trigger {
        Trigger::WorkItem(_) => settings.work_items,
        Trigger::PullRequest { .. } => settings.pull_requests,
        Trigger::Preview => true,
    };
    if !preview && !(settings.enabled && surface_on) {
        return Decision::None;
    }
    if conditions.quiet_mode || !conditions.window_active {
        return Decision::None;
    }
    let tier = match trigger {
        Trigger::WorkItem(StateMove {
            id,
            from,
            from_category,
            to,
            to_category,
        }) => {
            let Some(tier) = settings.tier(to, to_category) else {
                return Decision::None;
            };
            let base = rank(from_category, settings.tier(from, from_category));
            if rank(to_category, Some(tier)) <= base
                || memory.celebrated(id, now).is_some_and(|done| done >= tier)
            {
                return Decision::None;
            }
            memory.remember(id, tier, now);
            tier
        }
        Trigger::PullRequest { ko: true, .. } => return Decision::Ko,
        Trigger::PullRequest { .. } | Trigger::Preview => Tier::Milestone,
    };
    if conditions.reduce_motion {
        return Decision::Celebrate {
            tier,
            show: Show::Static { limited: false },
        };
    }
    if !preview {
        if memory.limited(now) {
            return Decision::Celebrate {
                tier,
                show: Show::Static { limited: true },
            };
        }
        memory.full.push_back(now);
    }
    let style = if conditions.ken_arcade && !conditions.quiet_hours {
        Style::Arcade
    } else {
        Style::Calm
    };
    Decision::Celebrate {
        tier,
        show: Show::Full(style),
    }
}

#[cfg(test)]
mod tests;
