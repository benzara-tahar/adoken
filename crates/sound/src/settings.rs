//! `settings.toml [sound]` (docs/design/sound.md §1, spec 0025 FR-13): on/off
//! and the master volume. The keys spec 0025 removed (pack, per-category and
//! per-event volumes, background play, quiet-hours options, mute) still
//! load without an issue (FR-14): they are ignored, and [`SoundSettings::to_table`]
//! never writes them back.

use std::fmt;

use toml::{Table, Value};

pub const SETTINGS_TABLE: &str = "sound";

#[derive(Debug, Clone, PartialEq)]
pub struct SoundSettings {
    /// Master opt-in; `false` after a fresh install (0007 AC-9).
    pub enabled: bool,
    pub master: f32,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            master: 0.6,
        }
    }
}

impl SoundSettings {
    /// Parse `[sound]` out of a whole `settings.toml`; a missing table is the
    /// default (sounds off). Only TOML syntax errors fail.
    pub fn from_settings_toml(source: &str) -> Result<(Self, Vec<SoundIssue>), toml::de::Error> {
        let mut document: Table = toml::from_str(source)?;
        Ok(match document.remove(SETTINGS_TABLE) {
            Some(Value::Table(table)) => Self::from_table(table),
            Some(_) => (
                Self::default(),
                vec![SoundIssue::invalid(SETTINGS_TABLE.into(), "a table")],
            ),
            None => (Self::default(), Vec::new()),
        })
    }

    pub fn from_table(table: Table) -> (Self, Vec<SoundIssue>) {
        let mut settings = Self::default();
        let mut issues = Vec::new();
        for (name, value) in table {
            let key = format!("{SETTINGS_TABLE}.{name}");
            match name.as_str() {
                "enabled" => match value {
                    Value::Boolean(on) => settings.enabled = on,
                    _ => issues.push(SoundIssue::invalid(key, "true or false")),
                },
                "master" => {
                    if let Some(gain) = gain(&key, value, &mut issues) {
                        settings.master = gain;
                    }
                }
                removed if REMOVED_KEYS.contains(&removed) => {}
                _ => issues.push(SoundIssue {
                    key,
                    kind: IssueKind::Unknown {
                        suggestion: suggest(&name, &KEYS),
                    },
                }),
            }
        }
        (settings, issues)
    }

    /// The `[sound]` table to write: the current keys only, the volume
    /// rounded to the slider's 1 % (no `0.3499999940395355`).
    pub fn to_table(&self) -> Table {
        let mut table = Table::new();
        table.insert("enabled".into(), Value::Boolean(self.enabled));
        table.insert(
            "master".into(),
            Value::Float((f64::from(self.master) * 100.0).round() / 100.0),
        );
        table
    }
}

const KEYS: [&str; 2] = ["enabled", "master"];
/// Keys spec 0025 removed: read without an issue, never written.
const REMOVED_KEYS: [&str; 7] = [
    "muted",
    "muted_until",
    "pack",
    "play_when_unfocused",
    "category",
    "events",
    "quiet_hours",
];

/// A 0.0–1.0 gain; out of range clamps with an issue.
fn gain(key: &str, value: Value, issues: &mut Vec<SoundIssue>) -> Option<f32> {
    let number = match value {
        Value::Integer(number) => number as f64,
        Value::Float(number) if number.is_finite() => number,
        _ => {
            issues.push(SoundIssue::invalid(
                key.into(),
                "a gain between 0.0 and 1.0",
            ));
            return None;
        }
    };
    let clamped = number.clamp(0.0, 1.0);
    if clamped != number {
        issues.push(SoundIssue {
            key: key.into(),
            kind: IssueKind::Clamped {
                value: number,
                clamped,
            },
        });
    }
    Some(clamped as f32)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoundIssue {
    pub key: String,
    pub kind: IssueKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueKind {
    Unknown { suggestion: Option<&'static str> },
    InvalidValue { expected: &'static str },
    Clamped { value: f64, clamped: f64 },
}

impl SoundIssue {
    fn invalid(key: String, expected: &'static str) -> Self {
        Self {
            key,
            kind: IssueKind::InvalidValue { expected },
        }
    }
}

impl fmt::Display for SoundIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = &self.key;
        match &self.kind {
            IssueKind::Unknown { suggestion } => {
                write!(f, "`{key}`: unknown, ignored.")?;
                match suggestion {
                    Some(candidate) => write!(f, " Did you mean `{candidate}`?"),
                    None => Ok(()),
                }
            }
            IssueKind::InvalidValue { expected } => {
                write!(f, "`{key}`: expected {expected}; ignored.")
            }
            IssueKind::Clamped { value, clamped } => {
                write!(f, "`{key}`: {value} is out of range; using {clamped}.")
            }
        }
    }
}

/// Nearest candidate within edit distance 2 (sound.md §6 typo guard).
fn suggest(key: &str, candidates: &[&'static str]) -> Option<&'static str> {
    candidates
        .iter()
        .map(|candidate| (edit_distance(key, candidate), *candidate))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b = b.as_bytes();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, &ca) in a.as_bytes().iter().enumerate() {
        current[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_is_off() {
        let (settings, issues) = SoundSettings::from_settings_toml("").unwrap();
        assert!(issues.is_empty());
        assert_eq!(settings, SoundSettings::default());
        assert!(!settings.enabled);
    }

    /// Spec 0025 AC-10 (sound part): a file with every removed `[sound]`
    /// key loads without an issue, keeps on/off and volume, and the table
    /// written back holds only the current keys.
    #[test]
    fn removed_keys_load_silently_and_are_never_written_back() {
        let text = r#"
[appearance]
mode = "dark"

[sound]
enabled = true
muted = true
muted_until = "2026-09-25T15:40:00Z"
pack = "arcade"
master = 0.45
play_when_unfocused = false

[sound.category]
ui = 0.35
ken = 0.0

[sound.events]
"ui.toggle_on" = true
notify.pipeline_succeeded = 0.5

[sound.quiet_hours]
urgent_breaks_through = true
ui_sounds = false
"#;
        let (settings, issues) = SoundSettings::from_settings_toml(text).unwrap();
        assert_eq!(issues, []);
        assert_eq!(
            settings,
            SoundSettings {
                enabled: true,
                master: 0.45,
            }
        );
        let written = settings.to_table();
        let mut keys: Vec<&str> = written.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["enabled", "master"]);
        assert_eq!(written["master"].as_float(), Some(0.45));
        assert_eq!(SoundSettings::from_table(written), (settings, Vec::new()));
    }

    #[test]
    fn bad_values_fall_back_with_precise_issues() {
        let (settings, issues) = SoundSettings::from_settings_toml(
            "[sound]\nenabled = \"yes\"\nmaster = 3\nvolum = 1\nmaster_volume = 0.2\n",
        )
        .unwrap();
        assert!(!settings.enabled);
        assert_eq!(settings.master, 1.0);
        let mut rendered: Vec<String> = issues.iter().map(ToString::to_string).collect();
        rendered.sort();
        assert_eq!(
            rendered,
            [
                "`sound.enabled`: expected true or false; ignored.",
                "`sound.master_volume`: unknown, ignored.",
                "`sound.master`: 3 is out of range; using 1.",
                "`sound.volum`: unknown, ignored.",
            ]
        );
        let (_, issues) = SoundSettings::from_settings_toml("[sound]\nenabld = true\n").unwrap();
        assert_eq!(
            issues[0].to_string(),
            "`sound.enabld`: unknown, ignored. Did you mean `enabled`?"
        );
    }
}
