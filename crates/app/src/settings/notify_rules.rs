//! Settings › Notifications (spec 0005 flow step 5, spec 0009): the rows,
//! their values, and edits of `NotifySettings`. Pure model; the Settings
//! page paints it (choices as segmented controls or selects, toggles as
//! switches, mutes with an Unmute button).

use adoken_notify::{
    BadgeMode, DeliveryMode, Mute, NotifySettings, Priority, RuleKey, Schedule, Source, Span,
    TimeOfDay, Weekdays,
};

/// One editable line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    Priority(RuleKey),
    Delivery(Priority),
    QuietEnabled,
    QuietLetP0,
    Window(Source),
    MinPriority(Source),
    ShowSelf,
    Badge,
    Rule(usize),
    Mute(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Header(&'static str, &'static str),
    Setting(Setting),
}

/// Value shown for a setting: a segmented choice or a checkbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Choice {
        options: Vec<String>,
        selected: usize,
    },
    Toggle(bool),
    /// Action on `Space` (unmute).
    Action(&'static str),
}

/// Delivery window preset offered in Settings; other schedules come from
/// `settings.toml [notify.windows]` and are shown by their label.
pub fn work_hours() -> Schedule {
    Schedule::new(vec![Span::new(
        Weekdays::WORKDAYS,
        TimeOfDay::hm(9, 0),
        TimeOfDay::hm(18, 0),
    )])
}

pub fn lines(settings: &NotifySettings, mutes: &[Mute]) -> Vec<Line> {
    let mut out = vec![Line::Header(
        "Priority rules",
        "Event → priority. P0 is yours to assign; no default uses it.",
    )];
    out.extend(RuleKey::ALL.map(|k| Line::Setting(Setting::Priority(k))));
    out.push(Line::Header(
        "Delivery",
        "Sounds play only when sounds are on in Settings → Sounds.",
    ));
    out.extend(Priority::ALL.map(|p| Line::Setting(Setting::Delivery(p))));
    out.push(Line::Header(
        "Quiet hours",
        "No banners or sounds; items still land in the inbox.",
    ));
    out.push(Line::Setting(Setting::QuietEnabled));
    out.push(Line::Setting(Setting::QuietLetP0));
    out.push(Line::Header(
        "Delivery windows",
        "Outside a window, banners wait and arrive as one summary.",
    ));
    out.extend(Source::ALL.map(|s| Line::Setting(Setting::Window(s))));
    out.push(Line::Header(
        "Show in inbox",
        "Least urgent priority each source may add.",
    ));
    out.extend(Source::ALL.map(|s| Line::Setting(Setting::MinPriority(s))));
    out.push(Line::Header(
        "Other",
        "adoken checks every minute while open.",
    ));
    out.push(Line::Setting(Setting::ShowSelf));
    out.push(Line::Setting(Setting::Badge));
    if !settings.rules.is_empty() {
        out.push(Line::Header(
            "Your rules",
            "First match wins. Edit conditions in settings.toml [[notify.rules]].",
        ));
        out.extend((0..settings.rules.len()).map(|i| Line::Setting(Setting::Rule(i))));
    }
    if !mutes.is_empty() {
        out.push(Line::Header(
            "Muted",
            "Unmuting brings back future activity.",
        ));
        out.extend((0..mutes.len()).map(|i| Line::Setting(Setting::Mute(i))));
    }
    out
}

fn priority_options() -> Vec<String> {
    Priority::ALL.map(|p| p.label().to_owned()).to_vec()
}

pub fn label(setting: &Setting, settings: &NotifySettings, mutes: &[Mute]) -> String {
    match setting {
        Setting::Priority(key) => key.label().into(),
        Setting::Delivery(p) => format!("{} · {}", p.label(), p.description()),
        Setting::QuietEnabled => format!("Quiet hours · {}", settings.quiet_hours.schedule.label()),
        Setting::QuietLetP0 => "Let P0 through".into(),
        Setting::Window(source) => source.label().into(),
        Setting::MinPriority(source) => source.label().into(),
        Setting::ShowSelf => "Show my own activity".into(),
        Setting::Badge => "Dock badge".into(),
        Setting::Rule(i) => settings
            .rules
            .get(*i)
            .map(|r| format!("{} — {} → {}", r.name, r.when.label(), r.then.label()))
            .unwrap_or_default(),
        Setting::Mute(i) => mutes.get(*i).map(Mute::label).unwrap_or_default(),
    }
}

pub fn value(setting: &Setting, settings: &NotifySettings) -> Value {
    match setting {
        Setting::Priority(key) => Value::Choice {
            options: priority_options(),
            selected: settings.priorities.get(*key).index(),
        },
        Setting::Delivery(p) => Value::Choice {
            options: vec!["Inbox".into(), "Banner".into(), "Banner + sound".into()],
            selected: DeliveryMode::ALL
                .iter()
                .position(|m| *m == settings.delivery.get(*p))
                .unwrap_or(0),
        },
        Setting::QuietEnabled => Value::Toggle(settings.quiet_hours.enabled),
        Setting::QuietLetP0 => Value::Toggle(settings.quiet_hours.let_p0_through),
        Setting::Window(source) => {
            let current = settings.windows.get(source);
            let custom = current.filter(|w| **w != work_hours());
            let mut options = vec!["Always".to_owned(), work_hours().label()];
            let selected = match (current, custom) {
                (None, _) => 0,
                (Some(_), None) => 1,
                (Some(w), Some(_)) => {
                    options.push(w.label());
                    2
                }
            };
            Value::Choice { options, selected }
        }
        Setting::MinPriority(source) => Value::Choice {
            options: vec!["All".into(), "P0–P2".into(), "P0–P1".into(), "P0".into()],
            selected: 3 - settings
                .min_priority
                .get(source)
                .copied()
                .unwrap_or(Priority::P3)
                .index(),
        },
        Setting::ShowSelf => Value::Toggle(settings.show_self_caused),
        Setting::Badge => Value::Choice {
            options: BadgeMode::ALL.map(|b| b.label().to_owned()).to_vec(),
            selected: BadgeMode::ALL
                .iter()
                .position(|b| *b == settings.badge)
                .unwrap_or(0),
        },
        Setting::Rule(i) => Value::Toggle(settings.rules.get(*i).is_some_and(|r| r.enabled)),
        Setting::Mute(_) => Value::Action("Unmute"),
    }
}

/// Pick option `index` of a choice setting.
pub fn choose(setting: &Setting, index: usize, settings: &mut NotifySettings) {
    match setting {
        Setting::Priority(key) => {
            if let Some(p) = Priority::ALL.get(index) {
                settings.priorities.set(*key, *p);
            }
        }
        Setting::Delivery(p) => {
            if let Some(mode) = DeliveryMode::ALL.get(index) {
                settings.delivery.set(*p, *mode);
            }
        }
        Setting::Window(source) => match index {
            0 => {
                settings.windows.remove(source);
            }
            1 => {
                settings.windows.insert(*source, work_hours());
            }
            // The custom schedule stays as it is.
            _ => {}
        },
        Setting::MinPriority(source) => {
            let least = Priority::ALL.get(3usize.saturating_sub(index)).copied();
            match least {
                Some(Priority::P3) | None => {
                    settings.min_priority.remove(source);
                }
                Some(p) => {
                    settings.min_priority.insert(*source, p);
                }
            }
        }
        Setting::Badge => {
            if let Some(mode) = BadgeMode::ALL.get(index) {
                settings.badge = *mode;
            }
        }
        _ => {}
    }
}

/// Flip a toggle row. Returns false for rows that aren't toggles (mutes
/// are handled by the inbox engine).
pub fn toggle(setting: &Setting, settings: &mut NotifySettings) -> bool {
    match setting {
        Setting::QuietEnabled => settings.quiet_hours.enabled ^= true,
        Setting::QuietLetP0 => settings.quiet_hours.let_p0_through ^= true,
        Setting::ShowSelf => settings.show_self_caused ^= true,
        Setting::Rule(i) => match settings.rules.get_mut(*i) {
            Some(rule) => rule.enabled ^= true,
            None => return false,
        },
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_edit_values_and_ignore_out_of_range_picks() {
        let mut settings = NotifySettings::default();
        let push = Setting::Priority(RuleKey::Push);
        choose(&push, 2, &mut settings);
        assert_eq!(settings.priorities.get(RuleKey::Push), Priority::P2);
        choose(&push, 9, &mut settings);
        assert_eq!(settings.priorities.get(RuleKey::Push), Priority::P2);
        choose(&push, 3, &mut settings);
        assert!(settings.priorities.0.is_empty(), "back at the default");

        let p2 = Setting::Delivery(Priority::P2);
        choose(&p2, 1, &mut settings);
        assert_eq!(settings.delivery.get(Priority::P2), DeliveryMode::Banner);
        assert_eq!(
            value(&p2, &settings),
            Value::Choice {
                options: vec!["Inbox".into(), "Banner".into(), "Banner + sound".into()],
                selected: 1
            }
        );
    }

    #[test]
    fn windows_and_minimums() {
        let mut settings = NotifySettings::default();
        let pipelines = Setting::Window(Source::Pipelines);
        choose(&pipelines, 1, &mut settings);
        assert_eq!(
            settings.windows.get(&Source::Pipelines),
            Some(&work_hours())
        );
        choose(&pipelines, 0, &mut settings);
        assert!(settings.windows.is_empty());

        let min = Setting::MinPriority(Source::WorkItems);
        choose(&min, 1, &mut settings);
        assert_eq!(
            settings.min_priority.get(&Source::WorkItems),
            Some(&Priority::P2)
        );
        choose(&min, 0, &mut settings);
        assert!(settings.min_priority.is_empty());
    }

    #[test]
    fn toggles_and_layout() {
        let mut settings = NotifySettings::default();
        assert!(toggle(&Setting::QuietEnabled, &mut settings));
        assert!(settings.quiet_hours.enabled);
        assert!(!toggle(&Setting::Badge, &mut settings));
        let lines = lines(&settings, &[]);
        let rows = lines
            .iter()
            .filter(|line| matches!(line, Line::Setting(_)))
            .count();
        assert!(matches!(lines[0], Line::Header(..)));
        assert_eq!(rows, RuleKey::ALL.len() + 4 + 2 + 5 + 5 + 2);
    }
}
