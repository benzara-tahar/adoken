//! Settings → Notifications (spec 0005 flow step 5). Plain data, serialized
//! to TOML with the rest of the user's preferences.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::classify::{Priority, PriorityTable, Source};
use crate::rules::UserRule;
use crate::schedule::{Schedule, Span, TimeOfDay, Weekdays};

/// How loudly a priority is delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeliveryMode {
    InboxOnly,
    Banner,
    BannerSound,
}

impl DeliveryMode {
    pub const ALL: [Self; 3] = [Self::InboxOnly, Self::Banner, Self::BannerSound];

    pub fn label(self) -> &'static str {
        match self {
            Self::InboxOnly => "Inbox only",
            Self::Banner => "Inbox + banner",
            Self::BannerSound => "Inbox + banner + sound",
        }
    }

    pub fn banner(self) -> bool {
        self != Self::InboxOnly
    }

    pub fn sound(self) -> bool {
        self == Self::BannerSound
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryByPriority {
    pub p0: DeliveryMode,
    pub p1: DeliveryMode,
    pub p2: DeliveryMode,
    pub p3: DeliveryMode,
}

impl Default for DeliveryByPriority {
    /// P0 banner+sound, P1 banner, P2/P3 inbox only.
    fn default() -> Self {
        Self {
            p0: DeliveryMode::BannerSound,
            p1: DeliveryMode::Banner,
            p2: DeliveryMode::InboxOnly,
            p3: DeliveryMode::InboxOnly,
        }
    }
}

impl DeliveryByPriority {
    pub fn get(&self, priority: Priority) -> DeliveryMode {
        match priority {
            Priority::P0 => self.p0,
            Priority::P1 => self.p1,
            Priority::P2 => self.p2,
            Priority::P3 => self.p3,
        }
    }

    pub fn set(&mut self, priority: Priority, mode: DeliveryMode) {
        *match priority {
            Priority::P0 => &mut self.p0,
            Priority::P1 => &mut self.p1,
            Priority::P2 => &mut self.p2,
            Priority::P3 => &mut self.p3,
        } = mode;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    /// Off at install (Q-1); offered on the last onboarding screen.
    pub enabled: bool,
    pub schedule: Schedule,
    /// "Let P0 through" (default on).
    pub let_p0_through: bool,
}

impl QuietHours {
    /// Onboarding prefill: weekday evenings 19:00–08:00 plus all weekend, so
    /// quiet runs Friday 19:00 → Monday 08:00 (Sunday's evening span carries
    /// it into Monday morning).
    pub fn onboarding_prefill() -> Schedule {
        let sun_to_fri = Weekdays(0b101_1111);
        Schedule::new(vec![
            Span::new(sun_to_fri, TimeOfDay::hm(19, 0), TimeOfDay::hm(8, 0)),
            Span::new(Weekdays::WEEKEND, TimeOfDay(0), TimeOfDay(0)),
        ])
    }
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: Self::onboarding_prefill(),
            let_p0_through: true,
        }
    }
}

/// Dock badge count (flow step 6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BadgeMode {
    None,
    P0,
    #[default]
    P0P1,
    AllUnread,
}

impl BadgeMode {
    pub const ALL: [Self; 4] = [Self::None, Self::P0, Self::P0P1, Self::AllUnread];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::P0 => "P0",
            Self::P0P1 => "P0 + P1",
            Self::AllUnread => "All unread",
        }
    }

    pub fn counts(self, priority: Priority) -> bool {
        match self {
            Self::None => false,
            Self::P0 => priority == Priority::P0,
            Self::P0P1 => priority <= Priority::P1,
            Self::AllUnread => true,
        }
    }
}

/// Name of the table in `settings.toml`.
pub const SETTINGS_TABLE: &str = "notify";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifySettings {
    /// FR-2 / AC-2 "show my own activity".
    pub show_self_caused: bool,
    pub priorities: PriorityTable,
    pub rules: Vec<UserRule>,
    /// Least urgent priority a source may add to the inbox; missing = all.
    pub min_priority: BTreeMap<Source, Priority>,
    pub delivery: DeliveryByPriority,
    pub quiet_hours: QuietHours,
    /// Per-source delivery windows; missing = always.
    pub windows: BTreeMap<Source, Schedule>,
    pub badge: BadgeMode,
}

impl NotifySettings {
    pub fn admits(&self, source: Source, priority: Priority) -> bool {
        self.min_priority
            .get(&source)
            .is_none_or(|&least| priority <= least)
    }

    /// This settings as the `[notify]` table of `settings.toml`.
    pub fn to_table(&self) -> Result<toml::Table, toml::ser::Error> {
        toml::Table::try_from(self)
    }

    /// Parse `[notify]` out of a whole `settings.toml`; a missing table is
    /// the default and missing keys fill in their defaults.
    pub fn from_settings_toml(text: &str) -> Result<Self, toml::de::Error> {
        let mut document: toml::Table = toml::from_str(text)?;
        match document.remove(SETTINGS_TABLE) {
            Some(table) => table.try_into(),
            None => Ok(Self::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{Kind, RuleKey};
    use crate::rules::{RuleAction, RuleMatch};

    #[test]
    fn defaults_follow_the_spec() {
        let settings = NotifySettings::default();
        assert!(!settings.quiet_hours.enabled);
        assert!(settings.quiet_hours.let_p0_through);
        assert!(!settings.show_self_caused);
        assert_eq!(
            settings.delivery.get(Priority::P0),
            DeliveryMode::BannerSound
        );
        assert_eq!(settings.delivery.get(Priority::P1), DeliveryMode::Banner);
        assert_eq!(settings.delivery.get(Priority::P2), DeliveryMode::InboxOnly);
        assert_eq!(settings.badge, BadgeMode::P0P1);
        assert!(settings.admits(Source::Pipelines, Priority::P3));
    }

    #[test]
    fn min_priority_filters_less_urgent() {
        let mut settings = NotifySettings::default();
        settings
            .min_priority
            .insert(Source::Pipelines, Priority::P2);
        assert!(settings.admits(Source::Pipelines, Priority::P1));
        assert!(!settings.admits(Source::Pipelines, Priority::P3));
        assert!(settings.admits(Source::WorkItems, Priority::P3));
    }

    #[test]
    fn settings_round_trip_through_toml() {
        let mut settings = NotifySettings::default();
        settings.quiet_hours.enabled = true;
        settings.priorities.set(RuleKey::Push, Priority::P2);
        settings
            .min_priority
            .insert(Source::WorkItems, Priority::P2);
        settings.windows.insert(
            Source::Pipelines,
            Schedule::new(vec![Span::new(
                Weekdays::WORKDAYS,
                TimeOfDay::hm(9, 0),
                TimeOfDay::hm(18, 0),
            )]),
        );
        settings.rules.push(UserRule {
            name: "Lead mentions".into(),
            enabled: true,
            when: RuleMatch {
                kind: Some(Kind::Sync(adoken_model::SyncEventKind::MentionedInPr)),
                actor: Some("Priya Shah".into()),
                ..RuleMatch::default()
            },
            then: RuleAction::SetPriority(Priority::P0),
        });
        let mut document = toml::Table::new();
        document.insert(SETTINGS_TABLE.into(), settings.to_table().unwrap().into());
        let text = toml::to_string(&document).unwrap();
        assert_eq!(NotifySettings::from_settings_toml(&text).unwrap(), settings);
        // Other modules' tables are ignored; a missing table is the default.
        assert_eq!(
            NotifySettings::from_settings_toml("[sound]\nenabled = true\n").unwrap(),
            NotifySettings::default()
        );
        // Partial tables fill in defaults.
        assert_eq!(
            NotifySettings::from_settings_toml("[notify]\nshow_self_caused = true").unwrap(),
            NotifySettings {
                show_self_caused: true,
                ..NotifySettings::default()
            }
        );
    }
}
