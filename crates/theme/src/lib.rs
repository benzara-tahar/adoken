//! adoken's theme domain model: semantic tokens, built-in themes, user theme
//! files. Deliberately free of gpui so it is cheap to test and reusable by
//! tooling; `adoken-ui` converts [`ColorTokens`] into paint colors once per
//! theme change.
//!
//! Contract: docs/design/tokens.md. Structure (variant/family/registry split,
//! contrast validation) adapted from zeron `crates/theme` at
//! d1010657d69c9730f8020edc36cc2649b136b6fa (MIT); see THIRD_PARTY_NOTICES.md.

mod appearance;
pub mod background;
pub mod code_font;
mod color;
pub mod draft;
mod file;
mod tokens;

use std::sync::LazyLock;

pub use appearance::{APPEARANCE_TABLE, AppearanceSettings};
pub use code_font::{CodeFont, CodeFontSettings};
pub use color::{Color, ColorParseError};
pub use file::{
    IssueKind, LoadedTheme, MAX_THEME_FILE_BYTES, MAX_USER_THEMES, Severity, THEME_FILE_SUFFIX,
    ThemeError, ThemeIssue, load_theme_dir, load_theme_file, parse_theme,
};
pub use tokens::{
    ColorTokens, FontRole, ShadowLayer, TypeStyle, elevation, motion, opacity, radius, size, space,
    typography, z,
};

/// Family id of the built-in theme.
pub const BUILTIN_FAMILY_ID: &str = "adoken";
/// Default dark variant; always present in every registry.
pub const DEFAULT_DARK_ID: &str = "adoken-dark";
/// Default light variant; always present in every registry.
pub const DEFAULT_LIGHT_ID: &str = "adoken-light";

/// Built-in theme files as `(family id, source)`, in picker order. The first
/// defines [`DEFAULT_DARK_ID`] and [`DEFAULT_LIGHT_ID`]. The first
/// [`HOUSE_FAMILIES`] are adoken's own designs; the rest are community
/// themes ported by `examples/port_zeron_themes.rs` (THIRD_PARTY_NOTICES.md).
const BUILTIN_SOURCES: &[(&str, &str)] = &[
    (
        BUILTIN_FAMILY_ID,
        include_str!("../themes/adoken.adoken-theme.toml"),
    ),
    ("arcade", include_str!("../themes/arcade.adoken-theme.toml")),
    ("sakura", include_str!("../themes/sakura.adoken-theme.toml")),
    ("zeron", include_str!("../themes/zeron.adoken-theme.toml")),
    (
        "vscode-default",
        include_str!("../themes/vscode-default.adoken-theme.toml"),
    ),
    (
        "catppuccin",
        include_str!("../themes/catppuccin.adoken-theme.toml"),
    ),
    (
        "tokyo-night",
        include_str!("../themes/tokyo-night.adoken-theme.toml"),
    ),
    (
        "dracula",
        include_str!("../themes/dracula.adoken-theme.toml"),
    ),
    ("github", include_str!("../themes/github.adoken-theme.toml")),
    ("ayu", include_str!("../themes/ayu.adoken-theme.toml")),
    (
        "gruvbox",
        include_str!("../themes/gruvbox.adoken-theme.toml"),
    ),
    (
        "rose-pine",
        include_str!("../themes/rose-pine.adoken-theme.toml"),
    ),
    ("nord", include_str!("../themes/nord.adoken-theme.toml")),
    (
        "one-dark-pro",
        include_str!("../themes/one-dark-pro.adoken-theme.toml"),
    ),
    (
        "atom-one-dark",
        include_str!("../themes/atom-one-dark.adoken-theme.toml"),
    ),
    (
        "night-owl",
        include_str!("../themes/night-owl.adoken-theme.toml"),
    ),
    (
        "winter-is-coming",
        include_str!("../themes/winter-is-coming.adoken-theme.toml"),
    ),
    (
        "palenight",
        include_str!("../themes/palenight.adoken-theme.toml"),
    ),
    (
        "synthwave-84",
        include_str!("../themes/synthwave-84.adoken-theme.toml"),
    ),
    (
        "cobalt2",
        include_str!("../themes/cobalt2.adoken-theme.toml"),
    ),
    (
        "andromeda",
        include_str!("../themes/andromeda.adoken-theme.toml"),
    ),
];

/// How many leading [`BUILTIN_SOURCES`] are adoken's own ("house") themes:
/// the set every gallery story is snapshotted in (spec 0007 FR-10).
const HOUSE_FAMILIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

impl Appearance {
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];

    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn default_variant_id(self) -> &'static str {
        match self {
            Self::Dark => DEFAULT_DARK_ID,
            Self::Light => DEFAULT_LIGHT_ID,
        }
    }
}

/// How a token was written before alias resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenSpec {
    Color(Color),
    /// `"@other.key"`, stored as the target's index in [`ColorTokens::KEYS`].
    Alias(usize),
}

/// One fully resolved palette (a light or a dark look).
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeVariant {
    pub id: String,
    pub family_id: String,
    pub name: String,
    pub appearance: Appearance,
    pub colors: ColorTokens,
    /// Unresolved definitions, index-aligned with [`ColorTokens::KEYS`]. User
    /// themes merge onto these (not onto resolved colors) so an override of
    /// `status.info.fg` also moves every built-in alias of it.
    pub(crate) specs: Vec<TokenSpec>,
}

impl ThemeVariant {
    /// Contrast checks for this variant; empty means it meets the contract.
    pub fn validate(&self) -> Vec<ContrastIssue> {
        CONTRAST_RULES
            .iter()
            .filter_map(|rule| rule.check(self))
            .collect()
    }
}

/// A group of related variants defined by one theme file.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeFamily {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    /// SPDX id, shown in the picker.
    pub license: Option<String>,
    pub variants: Vec<ThemeVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeRegistry {
    families: Vec<ThemeFamily>,
}

impl ThemeRegistry {
    /// The built-in families, parsed once from the embedded theme files.
    pub fn builtin() -> &'static Self {
        static REGISTRY: LazyLock<ThemeRegistry> = LazyLock::new(|| ThemeRegistry {
            families: BUILTIN_SOURCES
                .iter()
                .map(|(id, source)| {
                    file::parse_builtin(id, source)
                        .expect("built-in theme files are valid (see tests)")
                })
                .collect(),
        });
        &REGISTRY
    }

    /// adoken's own built-in variants (Dojo Night/Day, Arcade, Sakura), in
    /// picker order: the theme set every gallery story is snapshotted in.
    /// Ported community themes are validated by tests instead.
    pub fn house_variants() -> impl Iterator<Item = &'static ThemeVariant> {
        Self::builtin().families[..HOUSE_FAMILIES]
            .iter()
            .flat_map(|family| &family.variants)
    }

    /// Whether `family_id` is a built-in family (as opposed to a user file).
    pub fn is_builtin_family(family_id: &str) -> bool {
        BUILTIN_SOURCES.iter().any(|(id, _)| *id == family_id)
    }

    /// A mutable registry seeded with the built-ins, ready for user families.
    pub fn with_builtins() -> Self {
        Self::builtin().clone()
    }

    pub fn families(&self) -> &[ThemeFamily] {
        &self.families
    }

    pub fn variants(&self) -> impl Iterator<Item = &ThemeVariant> {
        self.families.iter().flat_map(|family| &family.variants)
    }

    pub fn variant(&self, id: &str) -> Option<&ThemeVariant> {
        self.variants().find(|variant| variant.id == id)
    }

    /// The built-in variant for `appearance`; the fallback for every lookup.
    pub fn default_variant(&self, appearance: Appearance) -> &ThemeVariant {
        let id = appearance.default_variant_id();
        self.variant(id)
            .or_else(|| Self::builtin().variant(id))
            .expect("the built-in registry defines both default variants")
    }

    /// The variant `id`, or the default for `fallback` when `id` names a
    /// missing (e.g. deleted user) theme.
    pub fn resolve(&self, id: &str, fallback: Appearance) -> &ThemeVariant {
        self.variant(id)
            .unwrap_or_else(|| self.default_variant(fallback))
    }

    /// Add a family. Rejected whole if any of its variant ids already exist,
    /// so a user file can never shadow a built-in.
    pub fn insert(&mut self, family: ThemeFamily) -> Result<(), ThemeError> {
        if self
            .families
            .iter()
            .any(|existing| existing.id == family.id)
        {
            return Err(ThemeError::DuplicateId(family.id));
        }
        if let Some(duplicate) = family
            .variants
            .iter()
            .find(|variant| self.variant(&variant.id).is_some())
        {
            return Err(ThemeError::DuplicateId(duplicate.id.clone()));
        }
        self.families.push(family);
        Ok(())
    }
}

/// A foreground/background pair below its required WCAG ratio.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastIssue {
    pub variant_id: String,
    pub foreground: &'static str,
    pub background: &'static str,
    pub actual: f32,
    pub minimum: f32,
}

impl std::fmt::Display for ContrastIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} on {} is {:.2}:1, needs {:.1}:1",
            self.foreground, self.background, self.actual, self.minimum
        )
    }
}

struct ContrastRule {
    foreground: &'static str,
    background: &'static str,
    /// Opaque base a translucent `background` (wash) is flattened over.
    over: &'static str,
    minimum: f32,
}

impl ContrastRule {
    /// The opaque color the foreground actually sits on.
    fn flattened_background(&self, colors: &ColorTokens) -> Color {
        let lookup = |key| *colors.get(key).expect("contrast rules name real keys");
        let background = lookup(self.background);
        if background.is_opaque() {
            background
        } else {
            background.blend_over(lookup(self.over).blend_over(colors.bg_app))
        }
    }

    fn check(&self, variant: &ThemeVariant) -> Option<ContrastIssue> {
        let colors = &variant.colors;
        let foreground = *colors
            .get(self.foreground)
            .expect("contrast rules name real keys");
        let actual = foreground.contrast(self.flattened_background(colors));
        (actual + f32::EPSILON < self.minimum).then(|| ContrastIssue {
            variant_id: variant.id.clone(),
            foreground: self.foreground,
            background: self.background,
            actual,
            minimum: self.minimum,
        })
    }
}

/// One required pair of [`CONTRAST_RULES`], resolved against `colors`:
/// `(foreground key, background key, flattened background, minimum)`.
pub(crate) fn contrast_pairs(
    colors: &ColorTokens,
) -> impl Iterator<Item = (&'static str, &'static str, Color, f32)> + '_ {
    CONTRAST_RULES.iter().map(|rule| {
        (
            rule.foreground,
            rule.background,
            rule.flattened_background(colors),
            rule.minimum,
        )
    })
}

/// tokens.md §9.3: text roles 4.5:1 on every surface (and, for the text
/// ramp, on hover/selection washes); syntax 4.5:1 on code and diff washes;
/// focus/control outlines and domain icons 3:1; `*.on` 4.5:1 on their solids.
static CONTRAST_RULES: LazyLock<Vec<ContrastRule>> = LazyLock::new(|| {
    const SURFACES: &[&str] = &[
        "bg.app",
        "bg.sidebar",
        "bg.surface",
        "bg.raised",
        "bg.overlay",
        "bg.input",
    ];
    const WASHES: &[&str] = &["bg.hover", "bg.active", "bg.selected", "bg.selected_muted"];
    const CODE: &[&str] = &[
        "bg.code",
        "diff.add.bg",
        "diff.add.word",
        "diff.del.bg",
        "diff.del.word",
        "diff.hunk.bg",
    ];
    const TEXT_RAMP: &[&str] = &["text.primary", "text.secondary", "text.tertiary"];
    const STATUS: &[(&str, &str)] = &[
        ("status.success.fg", "status.success.wash"),
        ("status.warning.fg", "status.warning.wash"),
        ("status.danger.fg", "status.danger.wash"),
        ("status.info.fg", "status.info.wash"),
    ];
    const DOMAIN_GROUPS: &[&str] = &["pr.", "vote.", "pipeline.", "wi.", "priority."];
    // Paired with a visible "Abandoned" label, drawn in `text.disabled`.
    const DOMAIN_EXEMPT: &[&str] = &["pr.abandoned"];

    let keys = ColorTokens::<Color>::KEYS;
    let syntax: Vec<&'static str> = keys
        .iter()
        .copied()
        .filter(|key| key.starts_with("syntax."))
        .collect();
    let domain: Vec<&'static str> = keys
        .iter()
        .copied()
        .filter(|key| DOMAIN_GROUPS.iter().any(|group| key.starts_with(group)))
        .filter(|key| !DOMAIN_EXEMPT.contains(key))
        .collect();

    let mut rules = Vec::new();
    let mut add = |foregrounds: &[&'static str],
                   backgrounds: &[&'static str],
                   over: &'static str,
                   minimum: f32| {
        for &foreground in foregrounds {
            for &background in backgrounds {
                rules.push(ContrastRule {
                    foreground,
                    background,
                    over,
                    minimum,
                });
            }
        }
    };
    add(TEXT_RAMP, SURFACES, "bg.surface", 4.5);
    add(TEXT_RAMP, WASHES, "bg.surface", 4.5);
    add(
        &["text.link", "text.match", "accent.fg"],
        SURFACES,
        "bg.surface",
        4.5,
    );
    for &(fg, wash) in STATUS {
        add(&[fg], SURFACES, "bg.surface", 4.5);
        add(&[fg], &[wash], "bg.surface", 4.5);
    }
    add(&syntax, CODE, "bg.code", 4.5);
    add(&["text.primary"], CODE, "bg.code", 4.5);
    add(
        &["border.focus", "border.control"],
        SURFACES,
        "bg.surface",
        3.0,
    );
    add(&domain, SURFACES, "bg.surface", 3.0);
    add(&["accent.on"], &["accent.solid"], "bg.surface", 4.5);
    add(
        &["status.danger.on"],
        &["status.danger.solid"],
        "bg.surface",
        4.5,
    );
    rules
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_define_both_defaults_and_meet_contrast() {
        let registry = ThemeRegistry::builtin();
        for appearance in Appearance::ALL {
            let variant = registry.default_variant(appearance);
            assert_eq!(variant.appearance, appearance);
            assert_eq!(variant.family_id, BUILTIN_FAMILY_ID);
        }
        // 0007 AC-2: every built-in, not only the defaults.
        assert!(registry.variants().count() >= 4);
        for variant in registry.variants() {
            let issues = variant.validate();
            assert!(
                issues.is_empty(),
                "{}:\n{}",
                variant.id,
                issues
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    #[test]
    fn builtin_files_parse_without_issues() {
        for (id, source) in BUILTIN_SOURCES {
            let loaded = parse_theme(id, source, None).unwrap();
            let issues: Vec<String> = loaded.issues.iter().map(ToString::to_string).collect();
            assert!(issues.is_empty(), "{id}: {}", issues.join("\n"));
        }
    }

    #[test]
    fn builtin_ids_are_unique_and_ports_carry_their_license() {
        let registry = ThemeRegistry::builtin();
        let mut ids: Vec<&str> = registry.variants().map(|v| v.id.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate built-in variant id");
        for family in &registry.families()[HOUSE_FAMILIES..] {
            assert_eq!(family.license.as_deref(), Some("MIT"), "{}", family.id);
            assert!(family.author.is_some(), "{}", family.id);
        }
        let house: Vec<&str> = ThemeRegistry::house_variants()
            .map(|v| v.id.as_str())
            .collect();
        assert_eq!(
            house,
            ["adoken-dark", "adoken-light", "arcade-dark", "sakura-light"]
        );
    }

    /// 0007 FR-2 asks for one high-contrast built-in: Arcade keeps the whole
    /// text ramp at WCAG AAA (7:1) on every surface and selection wash.
    #[test]
    fn arcade_is_high_contrast() {
        let colors = &ThemeRegistry::builtin()
            .variant("arcade-dark")
            .expect("arcade is built in")
            .colors;
        let surfaces = [
            colors.bg_app,
            colors.bg_sidebar,
            colors.bg_surface,
            colors.bg_raised,
            colors.bg_overlay,
            colors.bg_input,
        ];
        for surface in surfaces {
            for wash in [None, Some(colors.bg_hover), Some(colors.bg_selected)] {
                let background = wash.map_or(surface, |wash| wash.blend_over(surface));
                for text in [
                    colors.text_primary,
                    colors.text_secondary,
                    colors.text_tertiary,
                ] {
                    let ratio = text.contrast(background);
                    assert!(ratio >= 7.0, "{text} on {background}: {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn resolve_falls_back_when_the_theme_is_missing() {
        let registry = ThemeRegistry::builtin();
        assert_eq!(registry.resolve("dracula", Appearance::Light).id, "dracula");
        assert_eq!(
            registry.resolve("deleted-theme", Appearance::Light).id,
            DEFAULT_LIGHT_ID
        );
        assert_eq!(
            registry.resolve("deleted-theme", Appearance::Dark).id,
            DEFAULT_DARK_ID
        );
    }

    #[test]
    fn insert_rejects_families_that_would_shadow_existing_ids() {
        let mut registry = ThemeRegistry::with_builtins();
        let mut family = registry.families()[0].clone();
        assert!(matches!(
            registry.insert(family.clone()),
            Err(ThemeError::DuplicateId(id)) if id == BUILTIN_FAMILY_ID
        ));
        family.id = "copy".into();
        assert!(matches!(
            registry.insert(family.clone()),
            Err(ThemeError::DuplicateId(id)) if id == DEFAULT_DARK_ID
        ));
        for (variant, suffix) in family.variants.iter_mut().zip(["a", "b"]) {
            variant.id = format!("copy-{suffix}");
        }
        registry.insert(family).unwrap();
        assert!(registry.variant("copy-a").is_some());
    }

    #[test]
    fn validate_reports_low_contrast_pairs() {
        let mut variant = ThemeRegistry::builtin()
            .default_variant(Appearance::Dark)
            .clone();
        variant.colors.text_primary = variant.colors.bg_app;
        let issues = variant.validate();
        assert!(
            issues
                .iter()
                .any(|issue| issue.foreground == "text.primary" && issue.background == "bg.app")
        );
    }
}
