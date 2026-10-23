//! `settings.toml [appearance]` theme key (docs/design/tokens.md §9.4): the
//! one theme adoken paints. Other `[appearance]` keys (accent, fonts) belong
//! to their own owners: the settings writer replaces a table's whole key
//! set, so [`AppearanceSettings::write_into`] updates only the theme key of
//! the table as last read and the rest is written back unchanged.
//!
//! Older files paired a light and a dark theme and could follow macOS
//! (`mode`, `light_theme`, `dark_theme`; removed by user decision
//! 2026-09-30). They still load: the pair resolves to the theme that was
//! showing, and the next write replaces those keys with `theme`.

use toml::{Table, Value};

use crate::{Appearance, DEFAULT_DARK_ID};

/// Name of the settings table.
pub const APPEARANCE_TABLE: &str = "appearance";
/// The chosen variant id.
const THEME_KEY: &str = "theme";
/// Keys of the old light/dark pairing, read once and dropped on write.
const LEGACY_KEYS: [&str; 3] = ["mode", "light_theme", "dark_theme"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceSettings {
    /// The variant id to paint.
    pub theme: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: DEFAULT_DARK_ID.into(),
        }
    }
}

impl AppearanceSettings {
    /// Parse `[appearance]` out of a whole `settings.toml`. `system` (the
    /// macOS appearance) picks the default when no theme was ever chosen,
    /// and resolves an old "match system" pair. A wrong value falls back
    /// with an issue; only a TOML syntax error fails.
    pub fn from_settings_toml(
        source: &str,
        system: Appearance,
    ) -> Result<(Self, Vec<String>), toml::de::Error> {
        let document: Table = toml::from_str(source)?;
        Ok(match document.get(APPEARANCE_TABLE) {
            Some(Value::Table(table)) => Self::from_table(table, system),
            Some(_) => (
                Self::default_for(system),
                vec![format!("[{APPEARANCE_TABLE}] must be a table")],
            ),
            None => (Self::default_for(system), Vec::new()),
        })
    }

    pub fn from_table(table: &Table, system: Appearance) -> (Self, Vec<String>) {
        let mut issues = Vec::new();
        match table.get(THEME_KEY) {
            Some(Value::String(id)) if !id.trim().is_empty() => {
                return (
                    Self {
                        theme: id.trim().to_owned(),
                    },
                    issues,
                );
            }
            Some(value) => issues.push(format!(
                "appearance.{THEME_KEY} = {value}: expected a theme id; using the default"
            )),
            None => {}
        }
        (Self::legacy(table, system), issues)
    }

    /// Nothing chosen yet: the house default for the macOS appearance.
    pub fn default_for(system: Appearance) -> Self {
        Self {
            theme: system.default_variant_id().to_owned(),
        }
    }

    /// The theme the old light/dark pair was showing: `mode` (default
    /// `"system"`, so `system`) picks the slot. Bad values are ignored; the
    /// keys are rewritten on the next save anyway.
    fn legacy(table: &Table, system: Appearance) -> Self {
        let appearance = match table.get("mode").and_then(Value::as_str) {
            Some("light") => Appearance::Light,
            Some("dark") => Appearance::Dark,
            _ => system,
        };
        let key = match appearance {
            Appearance::Light => "light_theme",
            Appearance::Dark => "dark_theme",
        };
        match table.get(key).and_then(Value::as_str).map(str::trim) {
            Some(id) if !id.is_empty() => Self { theme: id.into() },
            _ => Self::default_for(appearance),
        }
    }

    /// Set the theme key in `table` (the `[appearance]` table as read),
    /// dropping the old pairing keys and keeping every other key.
    pub fn write_into(&self, table: &mut Table) {
        for key in LEGACY_KEYS {
            table.remove(key);
        }
        table.insert(THEME_KEY.into(), self.theme.clone().into());
    }

    /// The `[appearance]` table of a whole `settings.toml` (empty when
    /// missing or not a table).
    pub fn table_of(source: &str) -> Result<Table, toml::de::Error> {
        let mut document: Table = toml::from_str(source)?;
        Ok(match document.remove(APPEARANCE_TABLE) {
            Some(Value::Table(table)) => table,
            _ => Table::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_LIGHT_ID;

    fn parse(source: &str, system: Appearance) -> (AppearanceSettings, Vec<String>) {
        AppearanceSettings::from_settings_toml(source, system).unwrap()
    }

    #[test]
    fn round_trips_through_the_settings_file() {
        let settings = AppearanceSettings {
            theme: "dracula".into(),
        };
        // Other owners' keys in the same table survive the write.
        let source = "[appearance]\naccent = \"shu\"\ntheme = \"nord\"\n";
        let mut table = AppearanceSettings::table_of(source).unwrap();
        settings.write_into(&mut table);
        assert_eq!(table["accent"].as_str(), Some("shu"));
        let mut document = Table::new();
        document.insert(APPEARANCE_TABLE.into(), table.into());
        let text = toml::to_string(&document).unwrap();
        // The chosen theme wins whatever macOS shows.
        let (parsed, issues) = parse(&text, Appearance::Light);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(parsed, settings);
    }

    #[test]
    fn nothing_chosen_uses_the_default_for_the_system_appearance() {
        let (parsed, issues) = parse("", Appearance::Light);
        assert_eq!(parsed.theme, DEFAULT_LIGHT_ID);
        assert!(issues.is_empty());
        assert_eq!(
            parse("[general]\n", Appearance::Dark).0.theme,
            DEFAULT_DARK_ID
        );

        let (parsed, issues) = parse("appearance = 3\n", Appearance::Dark);
        assert_eq!(parsed.theme, DEFAULT_DARK_ID);
        assert_eq!(issues.len(), 1, "{issues:?}");

        let (parsed, issues) = parse("[appearance]\ntheme = 3\n", Appearance::Light);
        assert_eq!(parsed.theme, DEFAULT_LIGHT_ID);
        assert_eq!(issues.len(), 1, "{issues:?}");

        assert!(AppearanceSettings::from_settings_toml("[appearance", Appearance::Dark).is_err());
    }

    #[test]
    fn an_old_match_system_pair_resolves_to_the_theme_macos_was_showing() {
        let source = "[appearance]\nmode = \"system\"\nlight_theme = \"github-light\"\ndark_theme = \"dracula\"\n";
        assert_eq!(
            parse(source, Appearance::Light),
            (
                AppearanceSettings {
                    theme: "github-light".into()
                },
                Vec::new()
            )
        );
        assert_eq!(parse(source, Appearance::Dark).0.theme, "dracula");
        // `mode` was optional ("system" by default).
        let source = "[appearance]\ndark_theme = \"dracula\"\n";
        assert_eq!(parse(source, Appearance::Dark).0.theme, "dracula");
        assert_eq!(parse(source, Appearance::Light).0.theme, DEFAULT_LIGHT_ID);
    }

    #[test]
    fn an_old_fixed_mode_resolves_to_its_slot_whatever_macos_shows() {
        let source = "[appearance]\nmode = \"light\"\nlight_theme = \"catppuccin-latte\"\ndark_theme = \"dracula\"\n";
        assert_eq!(parse(source, Appearance::Dark).0.theme, "catppuccin-latte");
        let source = "[appearance]\nmode = \"dark\"\nlight_theme = \"catppuccin-latte\"\n";
        assert_eq!(parse(source, Appearance::Light).0.theme, DEFAULT_DARK_ID);
        // Junk in old keys never fails the load or reports.
        let source = "[appearance]\nmode = \"sepia\"\ndark_theme = 3\nlight_theme = [1]\n";
        assert_eq!(
            parse(source, Appearance::Dark),
            (
                AppearanceSettings {
                    theme: DEFAULT_DARK_ID.into()
                },
                Vec::new()
            )
        );
        // `theme` wins over leftover pairing keys.
        let source = "[appearance]\ntheme = \"nord\"\nmode = \"light\"\nlight_theme = \"x\"\n";
        assert_eq!(parse(source, Appearance::Light).0.theme, "nord");
    }

    #[test]
    fn writing_replaces_the_old_pair_with_one_theme_key() {
        let source = "[appearance]\naccent = \"shu\"\nmode = \"system\"\nlight_theme = \"github-light\"\ndark_theme = \"dracula\"\nfuture_key = 1\n";
        let (settings, _) = parse(source, Appearance::Dark);
        let mut table = AppearanceSettings::table_of(source).unwrap();
        settings.write_into(&mut table);
        let mut keys: Vec<&str> = table.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["accent", "future_key", "theme"]);
        assert_eq!(table["theme"].as_str(), Some("dracula"));
    }
}
