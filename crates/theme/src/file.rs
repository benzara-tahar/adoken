//! Theme files: `<family-id>.adoken-theme.toml` (docs/design/tokens.md §9).
//!
//! ```toml
//! name = "Sumi"                 # family display name
//! author = "Your Name"          # optional
//! license = "CC0-1.0"           # optional SPDX id
//!
//! [[variant]]
//! appearance = "dark"           # required: "dark" | "light"
//! name = "Sumi Ink"             # optional, default "<name> Dark"
//! id = "sumi-ink"               # optional, default "<family-id>-<appearance>"
//!
//! [variant.colors]
//! bg.app = "#0B0B0C"
//! pr.completed = "@accent.fg"   # alias, resolved after merging
//! ```
//!
//! Resolution per variant: built-in variant of the same appearance → file
//! `colors` → alias resolution → contrast validation. Per §9.3, a bad hex,
//! unknown alias target or alias cycle is an error for that key only (it
//! falls back to the built-in); a missing `appearance` or duplicate id skips
//! that variant; unknown keys and low contrast are warnings.
//!
//! Loading is blocking file IO: call it from a background executor.

use std::fmt;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::tokens::key_index;
use crate::{
    Appearance, Color, ColorTokens, ContrastIssue, ThemeFamily, ThemeRegistry, ThemeVariant,
    TokenSpec,
};

pub const THEME_FILE_SUFFIX: &str = ".adoken-theme.toml";
/// tokens.md §9.1 limits.
pub const MAX_THEME_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_USER_THEMES: usize = 64;
const MAX_ID_LEN: usize = 48;

/// The file could not be used at all.
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{}: theme files must be named `<id>{suffix}`", .0.display(), suffix = THEME_FILE_SUFFIX)]
    FileName(PathBuf),
    #[error("{}: {bytes} bytes exceeds the {max} byte limit", path.display(), max = MAX_THEME_FILE_BYTES)]
    TooLarge { path: PathBuf, bytes: u64 },
    #[error("{}: more than {max} theme files; this one was not loaded", .0.display(), max = MAX_USER_THEMES)]
    TooMany(PathBuf),
    #[error("invalid theme id `{0}`: use 1-48 of a-z, 0-9 and `-`")]
    InvalidId(String),
    #[error("theme id `{0}` is already in use")]
    DuplicateId(String),
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("{0}")]
    Invalid(String),
}

/// A family parsed from one file, plus every tolerated problem.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTheme {
    pub family: ThemeFamily,
    pub issues: Vec<ThemeIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeIssue {
    /// `None` for file-level issues.
    pub variant: Option<String>,
    pub kind: IssueKind,
}

impl ThemeIssue {
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueKind {
    /// Ignored; `suggestion` is the nearest known key within edit distance 2.
    UnknownKey {
        key: String,
        suggestion: Option<&'static str>,
    },
    /// The key falls back to the built-in value.
    InvalidValue {
        key: String,
        reason: String,
    },
    /// The key falls back to the built-in value.
    UnresolvedAlias {
        key: String,
        target: String,
    },
    /// The key falls back to the built-in value.
    AliasCycle {
        key: &'static str,
    },
    /// The whole variant was not loaded.
    SkippedVariant {
        index: usize,
        reason: String,
    },
    Contrast(ContrastIssue),
}

impl IssueKind {
    pub fn severity(&self) -> Severity {
        match self {
            Self::UnknownKey { .. } | Self::Contrast(_) => Severity::Warning,
            Self::InvalidValue { .. }
            | Self::UnresolvedAlias { .. }
            | Self::AliasCycle { .. }
            | Self::SkippedVariant { .. } => Severity::Error,
        }
    }
}

impl fmt::Display for ThemeIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(variant) = &self.variant {
            write!(f, "{variant}: ")?;
        }
        match &self.kind {
            IssueKind::UnknownKey { key, suggestion } => {
                write!(f, "unknown key `{key}` ignored")?;
                if let Some(suggestion) = suggestion {
                    write!(f, ". Did you mean `{suggestion}`?")?;
                }
                Ok(())
            }
            IssueKind::InvalidValue { key, reason } => {
                write!(f, "`{key}`: {reason}; using the built-in value")
            }
            IssueKind::UnresolvedAlias { key, target } => {
                write!(
                    f,
                    "`{key}`: unknown alias `@{target}`; using the built-in value"
                )
            }
            IssueKind::AliasCycle { key } => {
                write!(f, "`{key}`: alias cycle; using the built-in value")
            }
            IssueKind::SkippedVariant { index, reason } => {
                write!(f, "variant #{}: {reason}; variant skipped", index + 1)
            }
            IssueKind::Contrast(issue) => write!(f, "low contrast: {issue}"),
        }
    }
}

const FAMILY_FIELDS: &[&str] = &["name", "author", "license", "variant"];
const VARIANT_FIELDS: &[&str] = &["appearance", "name", "id", "colors"];

/// Parse one theme file's contents.
///
/// `base` supplies the fallback variant per appearance (normally
/// [`ThemeRegistry::builtin`]). With `None` the file is strict: every key must
/// be defined validly and any error-severity issue fails the load. That is how
/// the embedded built-in theme is loaded.
pub fn parse_theme(
    family_id: &str,
    source: &str,
    base: Option<&ThemeRegistry>,
) -> Result<LoadedTheme, ThemeError> {
    parse(family_id, source, base, true)
}

/// The embedded built-ins: strict, without contrast validation (tests hold
/// every built-in to the contract, so the launch path skips ~500 checks per
/// variant).
pub(crate) fn parse_builtin(family_id: &str, source: &str) -> Result<ThemeFamily, ThemeError> {
    parse(family_id, source, None, false).map(|loaded| loaded.family)
}

fn parse(
    family_id: &str,
    source: &str,
    base: Option<&ThemeRegistry>,
    validate: bool,
) -> Result<LoadedTheme, ThemeError> {
    validate_id(family_id)?;
    let mut doc: Table = toml::from_str(source)?;
    let mut issues = Vec::new();
    let mut file_issue = |kind| {
        issues.push(ThemeIssue {
            variant: None,
            kind,
        })
    };
    let name = take_string(&mut doc, "name", &mut file_issue).unwrap_or_else(|| family_id.into());
    let author = take_string(&mut doc, "author", &mut file_issue);
    let license = take_string(&mut doc, "license", &mut file_issue);
    let items = match doc.remove("variant") {
        Some(Value::Array(items)) => items,
        Some(_) => {
            return Err(ThemeError::Invalid(
                "`variant` must be an array of tables ([[variant]])".into(),
            ));
        }
        None => Vec::new(),
    };
    for (key, _) in doc {
        let suggestion = suggest(&key, FAMILY_FIELDS);
        file_issue(IssueKind::UnknownKey { key, suggestion });
    }

    let mut variants: Vec<ThemeVariant> = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let parsed = match item {
            Value::Table(table) => parse_variant(family_id, &name, table, base),
            _ => Err("must be a table".to_owned()),
        };
        let (variant, variant_issues) = match parsed {
            Ok(parsed) => parsed,
            Err(reason) => {
                issues.push(ThemeIssue {
                    variant: None,
                    kind: IssueKind::SkippedVariant { index, reason },
                });
                continue;
            }
        };
        if variants.iter().any(|existing| existing.id == variant.id) {
            issues.push(ThemeIssue {
                variant: None,
                kind: IssueKind::SkippedVariant {
                    index,
                    reason: format!("duplicate variant id `{}`", variant.id),
                },
            });
            continue;
        }
        let tagged = |kind| ThemeIssue {
            variant: Some(variant.id.clone()),
            kind,
        };
        issues.extend(variant_issues.into_iter().map(tagged));
        if validate {
            issues.extend(
                variant
                    .validate()
                    .into_iter()
                    .map(|issue| tagged(IssueKind::Contrast(issue))),
            );
        }
        variants.push(variant);
    }

    let strict = base.is_none();
    if strict
        && issues
            .iter()
            .any(|issue| issue.severity() == Severity::Error)
    {
        let messages: Vec<String> = issues.iter().map(ToString::to_string).collect();
        return Err(ThemeError::Invalid(messages.join("\n")));
    }
    if variants.is_empty() {
        let mut message = "theme defines no usable [[variant]]".to_owned();
        for issue in &issues {
            message.push_str(&format!("\n{issue}"));
        }
        return Err(ThemeError::Invalid(message));
    }

    Ok(LoadedTheme {
        family: ThemeFamily {
            id: family_id.to_owned(),
            name,
            author,
            license,
            variants,
        },
        issues,
    })
}

/// A string field, or an issue if present with another type.
fn take_string(table: &mut Table, key: &str, issue: &mut impl FnMut(IssueKind)) -> Option<String> {
    match table.remove(key)? {
        Value::String(value) => Some(value),
        _ => {
            issue(IssueKind::InvalidValue {
                key: key.to_owned(),
                reason: "expected a string".into(),
            });
            None
        }
    }
}

/// `Err` skips the variant with that reason.
fn parse_variant(
    family_id: &str,
    family_name: &str,
    mut table: Table,
    base: Option<&ThemeRegistry>,
) -> Result<(ThemeVariant, Vec<IssueKind>), String> {
    let appearance = match table.remove("appearance") {
        Some(Value::String(value)) if value == "dark" => Appearance::Dark,
        Some(Value::String(value)) if value == "light" => Appearance::Light,
        _ => return Err("`appearance` must be \"dark\" or \"light\"".into()),
    };
    let id = match table.remove("id") {
        None => format!("{family_id}-{}", appearance.as_str()),
        Some(Value::String(id)) => id,
        Some(_) => return Err("`id` must be a string".into()),
    };
    validate_id(&id).map_err(|error| error.to_string())?;
    let mut problems = Vec::new();
    let name =
        take_string(&mut table, "name", &mut |kind| problems.push(kind)).unwrap_or_else(|| {
            let suffix = if appearance.is_dark() {
                "Dark"
            } else {
                "Light"
            };
            format!("{family_name} {suffix}")
        });
    let colors = match table.remove("colors") {
        None => Table::new(),
        Some(Value::Table(colors)) => colors,
        Some(_) => return Err("`colors` must be a table".into()),
    };
    for (key, _) in table {
        let suggestion = suggest(&key, VARIANT_FIELDS);
        problems.push(IssueKind::UnknownKey { key, suggestion });
    }

    let keys = ColorTokens::<Color>::KEYS;
    let mut specs: Vec<Option<TokenSpec>> = vec![None; keys.len()];
    let mut entries = Vec::new();
    flatten("", colors, &mut entries);
    for (key, value) in entries {
        let Some(slot) = key_index(&key) else {
            let suggestion = suggest(&key, keys);
            problems.push(IssueKind::UnknownKey { key, suggestion });
            continue;
        };
        match parse_spec(key, value) {
            Ok(spec) => specs[slot] = Some(spec),
            Err(problem) => problems.push(problem),
        }
    }

    // Merge onto the fallback's *unresolved* specs, so aliases resolve after
    // every layer is applied (overriding `status.info.fg` moves `pr.active`).
    let fallback = base.map(|registry| registry.default_variant(appearance));
    let mut missing = Vec::new();
    let mut merged: Vec<TokenSpec> = specs
        .into_iter()
        .enumerate()
        .map(|(slot, spec)| {
            spec.or_else(|| fallback.map(|variant| variant.specs[slot]))
                .unwrap_or_else(|| {
                    missing.push(keys[slot]);
                    TokenSpec::Color(Color::BLACK)
                })
        })
        .collect();
    if !missing.is_empty() {
        return Err(format!("missing keys: {}", missing.join(", ")));
    }

    let mut resolved = Vec::with_capacity(keys.len());
    let mut unresolvable = Vec::new();
    for (slot, key) in keys.iter().enumerate() {
        let color = match resolve(slot, &merged) {
            Some(color) => color,
            None => {
                problems.push(IssueKind::AliasCycle { key });
                unresolvable.push(slot);
                // Strict mode reports the cycle as an error; the placeholder
                // color is never painted because the load fails.
                fallback.map_or(Color::BLACK, |variant| {
                    *variant.colors.get(key).expect("key from KEYS")
                })
            }
        };
        resolved.push(color);
    }
    // Keep specs consistent with what is painted, so this variant is a sound
    // merge base: a cyclic alias becomes the concrete fallback it resolved to.
    for slot in unresolvable {
        merged[slot] = TokenSpec::Color(resolved[slot]);
    }

    Ok((
        ThemeVariant {
            id,
            family_id: family_id.to_owned(),
            name,
            appearance,
            colors: ColorTokens::from_fn(|slot| resolved[slot]),
            specs: merged,
        },
        problems,
    ))
}

/// Flatten nested TOML tables into dotted keys: `[bg] app = …` and
/// `bg.app = …` both yield `("bg.app", …)`.
fn flatten(prefix: &str, table: Table, out: &mut Vec<(String, Value)>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Table(nested) => flatten(&path, nested, out),
            value => out.push((path, value)),
        }
    }
}

fn parse_spec(key: String, value: Value) -> Result<TokenSpec, IssueKind> {
    let Value::String(text) = value else {
        return Err(IssueKind::InvalidValue {
            key,
            reason: "expected a \"#rrggbb\" color or an \"@key\" alias".into(),
        });
    };
    if let Some(target) = text.strip_prefix('@') {
        return key_index(target)
            .map(TokenSpec::Alias)
            .ok_or_else(|| IssueKind::UnresolvedAlias {
                key,
                target: target.to_owned(),
            });
    }
    text.parse()
        .map(TokenSpec::Color)
        .map_err(|error: crate::ColorParseError| IssueKind::InvalidValue {
            key,
            reason: error.to_string(),
        })
}

/// Follow an alias chain to a color; `None` on a cycle.
pub(crate) fn resolve(start: usize, specs: &[TokenSpec]) -> Option<Color> {
    let mut slot = start;
    // A chain longer than the key count must revisit a key.
    for _ in 0..=specs.len() {
        match specs[slot] {
            TokenSpec::Color(color) => return Some(color),
            TokenSpec::Alias(target) => slot = target,
        }
    }
    None
}

/// Nearest candidate within edit distance 2 (tokens.md §9.3 typo guard).
pub(crate) fn suggest(key: &str, candidates: &[&'static str]) -> Option<&'static str> {
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

/// tokens.md §9.1: `[a-z0-9-]{1,48}`.
pub(crate) fn validate_id(id: &str) -> Result<(), ThemeError> {
    let valid = (1..=MAX_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(ThemeError::InvalidId(id.to_owned()))
    }
}

/// Load `<dir>/<id>.adoken-theme.toml`; the family id is the file stem.
pub fn load_theme_file(path: &Path, base: &ThemeRegistry) -> Result<LoadedTheme, ThemeError> {
    let family_id = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(THEME_FILE_SUFFIX))
        .ok_or_else(|| ThemeError::FileName(path.to_owned()))?;
    let io_error = |source| ThemeError::Io {
        path: path.to_owned(),
        source,
    };
    let bytes = std::fs::metadata(path).map_err(io_error)?.len();
    if bytes > MAX_THEME_FILE_BYTES {
        return Err(ThemeError::TooLarge {
            path: path.to_owned(),
            bytes,
        });
    }
    let source = std::fs::read_to_string(path).map_err(io_error)?;
    parse_theme(family_id, &source, Some(base))
}

/// Load every theme file in `dir`, sorted by path. A missing directory is
/// simply "no user themes". Each file succeeds or fails independently; files
/// beyond [`MAX_USER_THEMES`] fail with [`ThemeError::TooMany`].
pub fn load_theme_dir(
    dir: &Path,
    base: &ThemeRegistry,
) -> Vec<(PathBuf, Result<LoadedTheme, ThemeError>)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(source) => {
            let error = ThemeError::Io {
                path: dir.to_owned(),
                source,
            };
            return vec![(dir.to_owned(), Err(error))];
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(THEME_FILE_SUFFIX))
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let result = if index < MAX_USER_THEMES {
                load_theme_file(&path, base)
            } else {
                Err(ThemeError::TooMany(path.clone()))
            };
            (path, result)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_DARK_ID, DEFAULT_LIGHT_ID};

    fn builtin() -> &'static ThemeRegistry {
        ThemeRegistry::builtin()
    }

    fn non_contrast(loaded: &LoadedTheme) -> Vec<&IssueKind> {
        loaded
            .issues
            .iter()
            .map(|issue| &issue.kind)
            .filter(|kind| !matches!(kind, IssueKind::Contrast(_)))
            .collect()
    }

    /// A complete theme where every key is `#000000` except `overrides`.
    fn complete_source(appearance: &str, overrides: &[(&str, &str)]) -> String {
        let mut source = format!("[[variant]]\nappearance = \"{appearance}\"\n[variant.colors]\n");
        for key in ColorTokens::<Color>::KEYS {
            let value = overrides
                .iter()
                .find(|(k, _)| k == key)
                .map_or("#000000", |(_, v)| v);
            source.push_str(&format!("\"{key}\" = \"{value}\"\n"));
        }
        source
    }

    fn registry_of(variant: ThemeVariant) -> ThemeRegistry {
        ThemeRegistry {
            families: vec![ThemeFamily {
                id: "adoken".into(),
                name: "Test base".into(),
                author: None,
                license: None,
                variants: vec![variant],
            }],
        }
    }

    #[test]
    fn partial_theme_overrides_keys_and_falls_back_for_the_rest() {
        let source = r##"
            name = "Sumi"
            author = "Someone"
            license = "CC0-1.0"
            [[variant]]
            appearance = "dark"
            [variant.colors]
            accent.solid = "#c8b37a"
            bg = { app = "#0b0b0c" }
        "##;
        let loaded = parse_theme("sumi", source, Some(builtin())).unwrap();
        assert!(non_contrast(&loaded).is_empty(), "{:#?}", loaded.issues);
        assert_eq!(loaded.family.author.as_deref(), Some("Someone"));
        assert_eq!(loaded.family.license.as_deref(), Some("CC0-1.0"));
        let variant = &loaded.family.variants[0];
        assert_eq!(variant.id, "sumi-dark");
        assert_eq!(variant.name, "Sumi Dark");
        assert_eq!(variant.colors.accent_solid, Color::hex("#c8b37a"));
        assert_eq!(variant.colors.bg_app, Color::hex("#0b0b0c"));
        let base = builtin().default_variant(Appearance::Dark);
        assert_eq!(variant.colors.text_primary, base.colors.text_primary);
    }

    #[test]
    fn aliases_resolve_after_merge_so_base_aliases_follow_user_overrides() {
        let base_source = complete_source("dark", &[("pr.active", "@status.info.fg")]);
        let base_family = parse_theme("adoken", &base_source, None).unwrap().family;
        let base = registry_of(base_family.variants[0].clone());
        assert_eq!(base.default_variant(Appearance::Dark).id, DEFAULT_DARK_ID);

        let user =
            "[[variant]]\nappearance = \"dark\"\n[variant.colors]\nstatus.info.fg = \"#123456\"";
        let loaded = parse_theme("user", user, Some(&base)).unwrap();
        assert_eq!(
            loaded.family.variants[0].colors.pr_active,
            Color::hex("#123456")
        );
    }

    #[test]
    fn bad_values_are_key_errors_that_fall_back_to_the_builtin() {
        let source = r##"
            surprise = true
            [[variant]]
            appearance = "light"
            nmae = "typo"
            [variant.colors]
            bg.app = "blue"
            bg.surface = 3
            bg.raised = "@bg.nope"
            text.link = "@text.match"
            text.match = "@text.link"
            text.primery = "#000000"
        "##;
        let loaded = parse_theme("broken", source, Some(builtin())).unwrap();
        let base = builtin().default_variant(Appearance::Light);
        let colors = &loaded.family.variants[0].colors;
        for (key, color) in colors.iter() {
            assert_eq!(color, base.colors.get(key).unwrap(), "{key}");
        }
        let kinds = non_contrast(&loaded);
        let has = |predicate: &dyn Fn(&IssueKind) -> bool| kinds.iter().any(|k| predicate(k));
        assert!(has(
            &|k| matches!(k, IssueKind::UnknownKey { key, suggestion: None } if key == "surprise")
        ));
        assert!(has(
            &|k| matches!(k, IssueKind::UnknownKey { key, suggestion: Some("name") } if key == "nmae")
        ));
        assert!(has(
            &|k| matches!(k, IssueKind::UnknownKey { key, suggestion: Some("text.primary") } if key == "text.primery")
        ));
        assert!(has(
            &|k| matches!(k, IssueKind::InvalidValue { key, .. } if key == "bg.app")
        ));
        assert!(has(
            &|k| matches!(k, IssueKind::InvalidValue { key, .. } if key == "bg.surface")
        ));
        assert!(has(
            &|k| matches!(k, IssueKind::UnresolvedAlias { key, target } if key == "bg.raised" && target == "bg.nope")
        ));
        assert!(has(&|k| matches!(
            k,
            IssueKind::AliasCycle { key: "text.link" }
        )));
        assert!(has(&|k| matches!(
            k,
            IssueKind::AliasCycle { key: "text.match" }
        )));
        let severity = |key: &str| {
            loaded
                .issues
                .iter()
                .find(|issue| issue.to_string().contains(key))
                .map(ThemeIssue::severity)
        };
        assert_eq!(severity("surprise"), Some(Severity::Warning));
        assert_eq!(severity("bg.app"), Some(Severity::Error));

        // The loaded variant is itself a sound merge base: no cycle survives.
        let rebased = registry_of(ThemeVariant {
            id: DEFAULT_LIGHT_ID.into(),
            ..loaded.family.variants[0].clone()
        });
        let again = parse_theme(
            "again",
            "[[variant]]\nappearance = \"light\"",
            Some(&rebased),
        )
        .unwrap();
        assert!(non_contrast(&again).is_empty(), "{:#?}", again.issues);
    }

    #[test]
    fn broken_variants_are_skipped_and_siblings_still_load() {
        let source = r##"
            [[variant]]
            name = "no appearance"
            [[variant]]
            appearance = "sepia"
            [[variant]]
            appearance = "dark"
            id = "Bad Id"
            [[variant]]
            appearance = "dark"
            [[variant]]
            appearance = "dark"
        "##;
        let loaded = parse_theme("mixed", source, Some(builtin())).unwrap();
        let ids: Vec<_> = loaded
            .family
            .variants
            .iter()
            .map(|v| v.id.as_str())
            .collect();
        assert_eq!(ids, ["mixed-dark"]);
        let skipped: Vec<usize> = loaded
            .issues
            .iter()
            .filter_map(|issue| match issue.kind {
                IssueKind::SkippedVariant { index, .. } => Some(index),
                _ => None,
            })
            .collect();
        assert_eq!(skipped, [0, 1, 2, 4]);
    }

    #[test]
    fn low_contrast_is_a_warning_and_does_not_block_loading() {
        let source =
            "[[variant]]\nappearance = \"dark\"\n[variant.colors]\ntext.primary = \"@bg.app\"";
        let loaded = parse_theme("murky", source, Some(builtin())).unwrap();
        let issue = loaded
            .issues
            .iter()
            .find(|issue| matches!(&issue.kind, IssueKind::Contrast(c) if c.foreground == "text.primary"))
            .expect("contrast issue");
        assert_eq!(issue.severity(), Severity::Warning);
    }

    #[test]
    fn file_level_problems_are_errors() {
        let cases: &[(&str, &str)] = &[
            ("ok", "name = \"no variants\""),
            ("ok", "variant = 1"),
            ("ok", "[[variant]]\nappearance = \"sepia\""),
            ("Not-Valid", "[[variant]]\nappearance = \"dark\""),
            (&"x".repeat(49), "[[variant]]\nappearance = \"dark\""),
            ("ok", "not toml ="),
        ];
        for (id, source) in cases {
            assert!(
                parse_theme(id, source, Some(builtin())).is_err(),
                "{id}: {source}"
            );
        }
    }

    #[test]
    fn strict_mode_requires_every_key_and_rejects_errors() {
        assert!(parse_theme("full", &complete_source("dark", &[]), None).is_ok());
        let partial = "[[variant]]\nappearance = \"dark\"\n[variant.colors]\nbg.app = \"#000\"";
        let error = parse_theme("partial", partial, None).unwrap_err();
        assert!(error.to_string().contains("missing keys"), "{error}");
        let cyclic = complete_source("dark", &[("bg.app", "@bg.app")]);
        assert!(parse_theme("cyclic", &cyclic, None).is_err());
        let bad_hex = complete_source("dark", &[("bg.app", "#zzz")]);
        assert!(parse_theme("bad", &bad_hex, None).is_err());
    }

    #[test]
    fn directory_loading_uses_file_stems_and_enforces_limits() {
        let dir = std::env::temp_dir().join(format!("adoken-theme-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let theme = "[[variant]]\nappearance = \"light\"\n";
        std::fs::write(dir.join("zen.adoken-theme.toml"), theme).unwrap();
        std::fs::write(dir.join("Bad Name.adoken-theme.toml"), theme).unwrap();
        std::fs::write(dir.join("notes.toml"), theme).unwrap();
        let huge = format!("{theme}# {}", "x".repeat(MAX_THEME_FILE_BYTES as usize));
        std::fs::write(dir.join("huge.adoken-theme.toml"), huge).unwrap();

        let results = load_theme_dir(&dir, builtin());
        std::fs::remove_dir_all(&dir).unwrap();

        let by_name = |name: &str| {
            &results
                .iter()
                .find(|(path, _)| path.file_name().unwrap() == name)
                .unwrap()
                .1
        };
        assert_eq!(results.len(), 3, "{results:#?}");
        assert!(matches!(
            by_name("Bad Name.adoken-theme.toml"),
            Err(ThemeError::InvalidId(_))
        ));
        assert!(matches!(
            by_name("huge.adoken-theme.toml"),
            Err(ThemeError::TooLarge { .. })
        ));
        let zen = by_name("zen.adoken-theme.toml").as_ref().unwrap();
        assert_eq!(zen.family.id, "zen");
        assert_eq!(zen.family.variants[0].id, "zen-light");

        assert!(load_theme_dir(&dir.join("missing"), builtin()).is_empty());
    }
}
