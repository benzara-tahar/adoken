//! `settings.toml [appearance]` code font keys (spec 0009 §3 Code font, user
//! decision 2026-09-30): which family renders code (diffs, code blocks,
//! inline code), at what size, with or without ligatures.
//!
//! ```toml
//! [appearance]
//! code_font = "JetBrains Mono"   # bundled default; "system", or any installed family
//! code_font_size = 12            # 11–16
//! code_ligatures = false
//! ```
//!
//! The `[appearance]` table is shared with the theme key
//! ([`crate::AppearanceSettings`]): [`CodeFontSettings::write_into`] sets
//! only these keys in the table as last read.

use toml::{Table, Value};

use crate::APPEARANCE_TABLE;

pub const CODE_FONT_KEY: &str = "code_font";
pub const CODE_FONT_SIZE_KEY: &str = "code_font_size";
pub const CODE_LIGATURES_KEY: &str = "code_ligatures";

/// Family name of the bundled code font (`assets/fonts/jetbrains-mono`).
pub const BUNDLED_CODE_FONT: &str = "JetBrains Mono";
/// `code_font` value for the platform's monospace face.
const SYSTEM_VALUE: &str = "system";

pub const MIN_CODE_FONT_SIZE: u8 = 11;
pub const MAX_CODE_FONT_SIZE: u8 = 16;
/// `type.code` size (tokens.md §4).
pub const DEFAULT_CODE_FONT_SIZE: u8 = 12;

/// The platform's monospace family as the text system names it (macOS: SF
/// Mono, which has no public family name).
pub fn system_mono_family() -> &'static str {
    if cfg!(target_os = "macos") {
        ".AppleSystemUIFontMonospaced"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// [`system_mono_family`] as people know it.
pub fn system_mono_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "SF Mono"
    } else {
        system_mono_family()
    }
}

/// Which family renders code.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CodeFont {
    /// JetBrains Mono, embedded in the binary.
    #[default]
    Bundled,
    /// The platform's monospace face ([`system_mono_family`]).
    System,
    /// Any other installed family, by name.
    Installed(String),
}

impl CodeFont {
    /// A `code_font` value: `system`, the bundled family's name, or an
    /// installed family (case kept; matching is exact in the text system).
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        if value.eq_ignore_ascii_case(SYSTEM_VALUE) {
            Self::System
        } else if value.eq_ignore_ascii_case(BUNDLED_CODE_FONT) {
            Self::Bundled
        } else {
            Self::Installed(value.to_owned())
        }
    }

    /// What `code_font` stores.
    pub fn setting_value(&self) -> &str {
        match self {
            Self::Bundled => BUNDLED_CODE_FONT,
            Self::System => SYSTEM_VALUE,
            Self::Installed(name) => name,
        }
    }

    /// The family the text system is asked for.
    pub fn family(&self) -> &str {
        match self {
            Self::Bundled => BUNDLED_CODE_FONT,
            Self::System => system_mono_family(),
            Self::Installed(name) => name,
        }
    }

    /// How Settings names it.
    pub fn label(&self) -> String {
        match self {
            Self::Bundled => format!("{BUNDLED_CODE_FONT} (bundled)"),
            Self::System => format!("System monospace ({})", system_mono_name()),
            Self::Installed(name) => name.clone(),
        }
    }
}

/// The code font keys of `[appearance]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeFontSettings {
    pub font: CodeFont,
    /// Pixels, [`MIN_CODE_FONT_SIZE`]..=[`MAX_CODE_FONT_SIZE`].
    pub size: u8,
    /// Programming ligatures (`calt`); off by default so a review shows the
    /// exact characters (`!=`, not `≠`).
    pub ligatures: bool,
}

impl Default for CodeFontSettings {
    fn default() -> Self {
        Self {
            font: CodeFont::Bundled,
            size: DEFAULT_CODE_FONT_SIZE,
            ligatures: false,
        }
    }
}

impl CodeFontSettings {
    /// The keys out of a whole parsed `settings.toml`.
    pub fn from_document(document: &Table) -> (Self, Vec<String>) {
        match document.get(APPEARANCE_TABLE) {
            Some(Value::Table(table)) => Self::from_table(table),
            _ => (Self::default(), Vec::new()),
        }
    }

    /// The keys out of the `[appearance]` table. A wrong value falls back to
    /// its default with an issue; a size outside the range is clamped.
    pub fn from_table(table: &Table) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut issues = Vec::new();
        match table.get(CODE_FONT_KEY) {
            None => {}
            Some(Value::String(name)) if !name.trim().is_empty() => {
                settings.font = CodeFont::parse(name);
            }
            Some(value) => issues.push(format!(
                "appearance.{CODE_FONT_KEY} = {value}: expected a font family; using {BUNDLED_CODE_FONT}"
            )),
        }
        let size = match table.get(CODE_FONT_SIZE_KEY) {
            None => None,
            Some(Value::Integer(size)) => Some(*size as f64),
            Some(Value::Float(size)) if size.is_finite() => Some(*size),
            Some(value) => {
                issues.push(format!(
                    "appearance.{CODE_FONT_SIZE_KEY} = {value}: expected a number; using {DEFAULT_CODE_FONT_SIZE}"
                ));
                None
            }
        };
        if let Some(size) = size {
            let clamped = size
                .round()
                .clamp(f64::from(MIN_CODE_FONT_SIZE), f64::from(MAX_CODE_FONT_SIZE));
            if clamped != size {
                issues.push(format!(
                    "appearance.{CODE_FONT_SIZE_KEY} = {size}: using {clamped} ({MIN_CODE_FONT_SIZE}–{MAX_CODE_FONT_SIZE})"
                ));
            }
            settings.size = clamped as u8;
        }
        match table.get(CODE_LIGATURES_KEY) {
            None => {}
            Some(Value::Boolean(on)) => settings.ligatures = *on,
            Some(value) => issues.push(format!(
                "appearance.{CODE_LIGATURES_KEY} = {value}: expected true or false; using false"
            )),
        }
        (settings, issues)
    }

    /// Set the code font keys in `table` (the `[appearance]` table as read),
    /// keeping every other key.
    pub fn write_into(&self, table: &mut Table) {
        table.insert(
            CODE_FONT_KEY.into(),
            self.font.setting_value().to_owned().into(),
        );
        table.insert(CODE_FONT_SIZE_KEY.into(), i64::from(self.size).into());
        table.insert(CODE_LIGATURES_KEY.into(), self.ligatures.into());
    }

    /// The size `steps` pixels bigger (negative: smaller), clamped.
    pub fn stepped(&self, steps: i8) -> Self {
        let size = (i16::from(self.size) + i16::from(steps))
            .clamp(i16::from(MIN_CODE_FONT_SIZE), i16::from(MAX_CODE_FONT_SIZE));
        Self {
            size: size as u8,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> (CodeFontSettings, Vec<String>) {
        CodeFontSettings::from_document(&toml::from_str(source).unwrap())
    }

    #[test]
    fn missing_keys_mean_bundled_jetbrains_mono_at_twelve_without_ligatures() {
        for source in ["", "[appearance]\ntheme = \"nord\"\n"] {
            let (settings, issues) = parse(source);
            assert_eq!(settings, CodeFontSettings::default());
            assert_eq!(settings.font.family(), "JetBrains Mono");
            assert_eq!(settings.size, 12);
            assert!(!settings.ligatures);
            assert!(issues.is_empty());
        }
    }

    #[test]
    fn round_trips_every_choice_and_keeps_other_keys() {
        for font in [
            CodeFont::Bundled,
            CodeFont::System,
            CodeFont::Installed("Fira Code".into()),
        ] {
            let settings = CodeFontSettings {
                font,
                size: 15,
                ligatures: true,
            };
            let mut table: Table = toml::from_str("theme = \"dracula\"\n").unwrap();
            settings.write_into(&mut table);
            assert_eq!(table["theme"].as_str(), Some("dracula"));
            assert_eq!(CodeFontSettings::from_table(&table), (settings, Vec::new()));
        }
    }

    #[test]
    fn font_values_are_case_insensitive_for_keywords_and_exact_otherwise() {
        assert_eq!(CodeFont::parse(" System "), CodeFont::System);
        assert_eq!(CodeFont::parse("jetbrains mono"), CodeFont::Bundled);
        assert_eq!(
            CodeFont::parse("Iosevka Term"),
            CodeFont::Installed("Iosevka Term".into())
        );
    }

    #[test]
    fn bad_values_fall_back_with_an_issue_each() {
        let (settings, issues) = parse(
            "[appearance]\ncode_font = 3\ncode_font_size = \"big\"\ncode_ligatures = \"yes\"\n",
        );
        assert_eq!(settings, CodeFontSettings::default());
        assert_eq!(issues.len(), 3, "{issues:?}");
        let (settings, issues) = parse("[appearance]\ncode_font = \"  \"\n");
        assert_eq!(settings.font, CodeFont::Bundled);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn sizes_clamp_to_the_range_and_round_to_whole_pixels() {
        let size = |source: &str| parse(&format!("[appearance]\ncode_font_size = {source}\n"));
        let (settings, issues) = size("40");
        assert_eq!((settings.size, issues.len()), (16, 1));
        assert_eq!(size("3").0.size, 11);
        assert_eq!(size("13.6").0.size, 14);
        let (settings, issues) = size("14");
        assert_eq!(settings.size, 14);
        assert!(issues.is_empty());
    }

    #[test]
    fn stepping_stays_in_range() {
        let settings = CodeFontSettings::default();
        assert_eq!(settings.stepped(1).size, 13);
        assert_eq!(settings.stepped(-1).size, 11);
        assert_eq!(settings.stepped(-1).stepped(-1).size, 11);
        assert_eq!(settings.stepped(20).size, 16);
    }
}
