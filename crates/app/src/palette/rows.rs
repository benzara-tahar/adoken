//! Customizable result rows (docs/design/command-palette.md §5, spec 0003
//! FR-8): per-type row templates read from `[palette.rows.<type>]` in
//! `settings.toml`, with per-field fallback to the defaults.
//!
//! Pure: parsing and interpolation only. Field values come from the catalog.

use std::collections::BTreeMap;

use adoken_search::DocKind;
use serde::Deserialize;

/// Row types in `[palette.rows.<type>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowType {
    PullRequest,
    WorkItem,
    PipelineRun,
    Person,
    Repository,
    Project,
    Command,
}

impl RowType {
    pub const ALL: &'static [RowType] = &[
        RowType::PullRequest,
        RowType::WorkItem,
        RowType::PipelineRun,
        RowType::Person,
        RowType::Repository,
        RowType::Project,
        RowType::Command,
    ];

    pub fn of(kind: DocKind) -> Self {
        match kind {
            DocKind::PullRequest => RowType::PullRequest,
            DocKind::WorkItem => RowType::WorkItem,
            DocKind::Pipeline => RowType::PipelineRun,
            DocKind::Person => RowType::Person,
            DocKind::Repo => RowType::Repository,
            DocKind::Project => RowType::Project,
            DocKind::Command => RowType::Command,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            RowType::PullRequest => "pull_request",
            RowType::WorkItem => "work_item",
            RowType::PipelineRun => "pipeline_run",
            RowType::Person => "person",
            RowType::Repository => "repository",
            RowType::Project => "project",
            RowType::Command => "command",
        }
    }

    /// Field catalog (§5.3).
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            RowType::PullRequest => &[
                "id",
                "title",
                "repo",
                "project",
                "author",
                "source_branch",
                "target_branch",
                "status",
                "draft",
                "my_vote",
                "reviewers_summary",
                "reviewers",
                "merge_status",
                "iterations",
                "created",
                "updated",
            ],
            RowType::WorkItem => &[
                "id",
                "title",
                "type",
                "state",
                "assigned_to",
                "priority",
                "area",
                "iteration",
                "tags",
                "parent",
                "project",
                "created",
                "updated",
            ],
            RowType::PipelineRun => &[
                "pipeline",
                "run",
                "status",
                "result",
                "branch",
                "triggered_by",
                "duration",
                "finished",
                "project",
            ],
            RowType::Person => &["name", "email", "open_prs"],
            RowType::Repository => &["name", "project", "default_branch", "open_prs"],
            RowType::Project => &["name", "open_prs"],
            RowType::Command => &["title", "shortcut"],
        }
    }

    fn default_row(self) -> RawRow {
        let (leading, title, badges, subtitle, trailing): (_, _, &[&str], _, &[&str]) = match self {
            RowType::PullRequest => (
                "status_icon",
                "{title}",
                &["draft", "my_vote"],
                "{id} · {repo} · {source_branch} → {target_branch}",
                &["reviewers_summary", "updated"],
            ),
            RowType::WorkItem => (
                "type_icon",
                "{title}",
                &["state"],
                "{id} · {type} · {area}",
                &["priority", "assigned_to", "updated"],
            ),
            RowType::PipelineRun => (
                "status_icon",
                "{pipeline} {run}",
                &[],
                "{branch} · {triggered_by}",
                &["duration", "finished"],
            ),
            RowType::Person => ("avatar", "{name}", &[], "{email}", &["open_prs"]),
            RowType::Repository => (
                "type_icon",
                "{name}",
                &[],
                "{project} · {default_branch}",
                &["open_prs"],
            ),
            RowType::Project => ("type_icon", "{name}", &[], "", &["open_prs"]),
            RowType::Command => ("type_icon", "{title}", &[], "", &["shortcut"]),
        };
        let strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect();
        RawRow {
            leading: Some(leading.to_owned()),
            title: Some(title.to_owned()),
            badges: Some(strings(badges)),
            subtitle: Some(subtitle.to_owned()),
            trailing: Some(strings(trailing)),
        }
    }
}

/// Leading slot (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leading {
    TypeIcon,
    StatusIcon,
    Avatar,
    None,
}

impl Leading {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "type_icon" => Leading::TypeIcon,
            "status_icon" => Leading::StatusIcon,
            "avatar" | "author_avatar" | "assigned_avatar" | "from_avatar" => Leading::Avatar,
            "none" => Leading::None,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Field(String),
}

/// `{field}` interpolation plus literal text (§5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
}

impl Template {
    /// Parses a template; unknown fields (per `known`) are reported and render
    /// empty.
    pub fn parse(source: &str, known: &[&str], issues: &mut Vec<String>) -> Self {
        let mut parts = Vec::new();
        let mut rest = source;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            if open > 0 {
                parts.push(Part::Literal(rest[..open].to_owned()));
            }
            let name = rest[open + 1..open + close].trim();
            if !known.contains(&name) {
                issues.push(format!("unknown field `{name}`"));
            }
            parts.push(Part::Field(name.to_owned()));
            rest = &rest[open + close + 1..];
        }
        if !rest.is_empty() {
            parts.push(Part::Literal(rest.to_owned()));
        }
        Self { parts }
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Interpolates fields. A separator literal (` · `, ` → `, ` / `, or
    /// whitespace) next to an empty field is dropped, so optional fields
    /// never leave dangling dots.
    pub fn render(&self, mut field: impl FnMut(&str) -> String) -> String {
        let values: Vec<Option<String>> = self
            .parts
            .iter()
            .map(|part| match part {
                Part::Literal(_) => None,
                Part::Field(name) => Some(field(name)),
            })
            .collect();
        let mut dropped = vec![false; self.parts.len()];
        let is_separator = |ix: usize| match &self.parts[ix] {
            Part::Literal(text) => matches!(text.trim(), "" | "·" | "→" | "/" | "|" | "-" | ","),
            Part::Field(_) => false,
        };
        for (ix, value) in values.iter().enumerate() {
            if value.as_ref().is_some_and(String::is_empty) {
                if ix > 0 && is_separator(ix - 1) && !dropped[ix - 1] {
                    dropped[ix - 1] = true;
                } else if ix + 1 < self.parts.len() && is_separator(ix + 1) {
                    dropped[ix + 1] = true;
                }
            }
        }
        let mut out = String::new();
        for (ix, part) in self.parts.iter().enumerate() {
            if dropped[ix] {
                continue;
            }
            match (part, &values[ix]) {
                (Part::Literal(text), _) => out.push_str(text),
                (Part::Field(_), Some(value)) => out.push_str(value),
                (Part::Field(_), None) => {}
            }
        }
        out.trim().to_owned()
    }
}

/// One resolved row layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowTemplate {
    pub leading: Leading,
    pub title: Template,
    pub badges: Vec<String>,
    pub subtitle: Template,
    pub trailing: Vec<String>,
}

/// Result density (`size.palette_row.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Compact,
    #[default]
    Default,
    Comfortable,
}

/// `[palette]` settings.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteSettings {
    pub density: Density,
    pub max_results: usize,
    pub recent_count: usize,
    /// `avatars` (0003 FR-8): person avatars in rows; `false` shows the
    /// type glyph and asks for no pictures (0012 FR-14).
    pub avatars: bool,
    rows: BTreeMap<RowType, RowTemplate>,
}

impl Default for PaletteSettings {
    fn default() -> Self {
        Self::from_toml("").0
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawRoot {
    #[serde(default)]
    palette: RawPalette,
}

#[derive(Debug, Default, Deserialize)]
struct RawPalette {
    density: Option<toml::Value>,
    max_results: Option<toml::Value>,
    recent_count: Option<toml::Value>,
    avatars: Option<toml::Value>,
    #[serde(default)]
    rows: BTreeMap<String, RawRow>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawRow {
    leading: Option<String>,
    title: Option<String>,
    badges: Option<Vec<String>>,
    subtitle: Option<String>,
    trailing: Option<Vec<String>>,
}

pub const DEFAULT_MAX_RESULTS: usize = 500;
pub const DEFAULT_RECENT_COUNT: usize = 8;

impl PaletteSettings {
    /// Parses `settings.toml` text. Invalid values fall back to their default
    /// per field; the returned issues are shown once ("Row issues").
    pub fn from_toml(text: &str) -> (Self, Vec<String>) {
        let mut issues = Vec::new();
        let raw: RawRoot = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                issues.push(format!("settings.toml: {}", error.message()));
                RawRoot::default()
            }
        };
        let p = raw.palette;
        let density = match p.density {
            None => Density::default(),
            Some(value) => value.try_into().unwrap_or_else(|_| {
                issues.push("palette.density: expected compact, default or comfortable".into());
                Density::default()
            }),
        };
        let mut int = |value: Option<toml::Value>,
                       name: &str,
                       range: std::ops::RangeInclusive<i64>,
                       default: usize| {
            match value {
                None => default,
                Some(toml::Value::Integer(n)) if range.contains(&n) => n as usize,
                Some(_) => {
                    issues.push(format!(
                        "palette.{name}: expected a number from {} to {}",
                        range.start(),
                        range.end()
                    ));
                    default
                }
            }
        };
        let max_results = int(p.max_results, "max_results", 10..=500, DEFAULT_MAX_RESULTS);
        let recent_count = int(p.recent_count, "recent_count", 0..=50, DEFAULT_RECENT_COUNT);
        let avatars = match p.avatars {
            None => true,
            Some(toml::Value::Boolean(on)) => on,
            Some(_) => {
                issues.push("palette.avatars: expected true or false".into());
                true
            }
        };

        for key in p.rows.keys() {
            if !RowType::ALL.iter().any(|t| t.key() == key) {
                issues.push(format!("palette.rows.{key}: unknown result type"));
            }
        }
        let rows = RowType::ALL
            .iter()
            .map(|&row_type| {
                let user = p.rows.get(row_type.key()).cloned().unwrap_or_default();
                (row_type, resolve_row(row_type, user, &mut issues))
            })
            .collect();
        (
            Self {
                density,
                max_results,
                recent_count,
                avatars,
                rows,
            },
            issues,
        )
    }

    pub fn row(&self, row_type: RowType) -> &RowTemplate {
        &self.rows[&row_type]
    }
}

/// User values over defaults, validated per slot: an invalid slot falls back
/// to its default; an unknown field inside a template renders empty.
fn resolve_row(row_type: RowType, user: RawRow, issues: &mut Vec<String>) -> RowTemplate {
    let defaults = row_type.default_row();
    let known = row_type.fields();
    let prefix = format!("palette.rows.{}", row_type.key());
    let mut slot_issues = Vec::new();

    let leading = user
        .leading
        .as_deref()
        .and_then(|value| {
            let leading = Leading::parse(value);
            if leading.is_none() {
                slot_issues.push(format!("leading: unknown value `{value}`"));
            }
            leading
        })
        .or_else(|| defaults.leading.as_deref().and_then(Leading::parse))
        .unwrap_or(Leading::None);
    let template = |value: Option<String>, default: Option<String>, issues: &mut Vec<String>| {
        Template::parse(&value.or(default).unwrap_or_default(), known, issues)
    };
    let title = template(user.title, defaults.title, &mut slot_issues);
    let subtitle = template(user.subtitle, defaults.subtitle, &mut slot_issues);
    let mut list =
        |value: Option<Vec<String>>, default: Option<Vec<String>>, max: usize, slot: &str| {
            match value {
                Some(items) => {
                    let mut valid = Vec::new();
                    for item in items {
                        if known.contains(&item.as_str()) {
                            valid.push(item);
                        } else {
                            slot_issues.push(format!("{slot}: unknown field `{item}`"));
                        }
                    }
                    valid.truncate(max);
                    valid
                }
                None => default.unwrap_or_default(),
            }
        };
    let badges = list(user.badges, defaults.badges, 2, "badges");
    let trailing = list(user.trailing, defaults.trailing, 3, "trailing");
    issues.extend(
        slot_issues
            .into_iter()
            .map(|issue| format!("{prefix}.{issue}")),
    );
    RowTemplate {
        leading,
        title,
        badges,
        subtitle,
        trailing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(source: &str, values: &[(&str, &str)]) -> String {
        let known: Vec<&str> = values.iter().map(|(k, _)| *k).collect();
        let mut issues = Vec::new();
        let template = Template::parse(source, &known, &mut issues);
        assert!(issues.is_empty(), "{issues:?}");
        template.render(|name| {
            values
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
                .unwrap_or_default()
        })
    }

    #[test]
    fn separators_next_to_empty_fields_are_dropped() {
        let t = "{id} · {repo} · {branch}";
        let all = [("id", "!1"), ("repo", "web"), ("branch", "main")];
        assert_eq!(render(t, &all), "!1 · web · main");
        assert_eq!(
            render(t, &[("id", "!1"), ("repo", ""), ("branch", "main")]),
            "!1 · main"
        );
        assert_eq!(
            render(t, &[("id", ""), ("repo", "web"), ("branch", "main")]),
            "web · main"
        );
        assert_eq!(
            render(t, &[("id", "!1"), ("repo", "web"), ("branch", "")]),
            "!1 · web"
        );
        assert_eq!(
            render(t, &[("id", ""), ("repo", ""), ("branch", "main")]),
            "main"
        );
        assert_eq!(render("{a} {b}", &[("a", "Deploy"), ("b", "")]), "Deploy");
        // Non-separator literals stay.
        assert_eq!(render("PR {id}", &[("id", "")]), "PR");
    }

    #[test]
    fn default_rows_match_the_design() {
        let settings = PaletteSettings::default();
        let pr = settings.row(RowType::PullRequest);
        assert_eq!(pr.leading, Leading::StatusIcon);
        assert_eq!(pr.badges, ["draft", "my_vote"]);
        assert_eq!(pr.trailing, ["reviewers_summary", "updated"]);
        assert_eq!(settings.density, Density::Default);
        assert_eq!(settings.max_results, DEFAULT_MAX_RESULTS);
    }

    #[test]
    fn user_rows_override_per_slot_and_bad_values_fall_back() {
        let (settings, issues) = PaletteSettings::from_toml(
            r#"
            [palette]
            density = "compact"
            max_results = 5000

            [palette.rows.pull_request]
            title = "{id} {title}"
            trailing = ["updated", "bogus"]
            leading = "sparkles"

            [palette.rows.widgets]
            title = "{x}"
            "#,
        );
        assert_eq!(settings.density, Density::Compact);
        assert_eq!(settings.max_results, DEFAULT_MAX_RESULTS);
        let pr = settings.row(RowType::PullRequest);
        assert_eq!(pr.trailing, ["updated"]);
        assert_eq!(pr.leading, Leading::StatusIcon, "bad leading falls back");
        assert_eq!(pr.badges, ["draft", "my_vote"], "unset slot keeps default");
        let title = pr.title.render(|f| f.to_uppercase());
        assert_eq!(title, "ID TITLE");
        assert_eq!(
            issues,
            [
                "palette.max_results: expected a number from 10 to 500",
                "palette.rows.widgets: unknown result type",
                "palette.rows.pull_request.leading: unknown value `sparkles`",
                "palette.rows.pull_request.trailing: unknown field `bogus`",
            ]
        );
    }

    #[test]
    fn avatars_default_on_and_bad_values_fall_back() {
        assert!(PaletteSettings::default().avatars);
        let (off, issues) = PaletteSettings::from_toml("[palette]\navatars = false\n");
        assert!(!off.avatars);
        assert!(issues.is_empty());
        let (bad, issues) = PaletteSettings::from_toml("[palette]\navatars = \"no\"\n");
        assert!(bad.avatars);
        assert_eq!(issues, ["palette.avatars: expected true or false"]);
    }

    #[test]
    fn unknown_template_fields_render_empty_with_an_issue() {
        let (settings, issues) =
            PaletteSettings::from_toml("[palette.rows.work_item]\nsubtitle = \"{id} · {nope}\"\n");
        assert_eq!(issues, ["palette.rows.work_item.unknown field `nope`"]);
        let rendered = settings.row(RowType::WorkItem).subtitle.render(|f| {
            if f == "id" {
                "#7".into()
            } else {
                String::new()
            }
        });
        assert_eq!(rendered, "#7");
    }

    #[test]
    fn invalid_toml_keeps_defaults() {
        let (settings, issues) = PaletteSettings::from_toml("[palette\n");
        assert_eq!(settings, PaletteSettings::default());
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn every_default_template_uses_known_fields() {
        for &row_type in RowType::ALL {
            let mut issues = Vec::new();
            resolve_row(row_type, RawRow::default(), &mut issues);
            assert!(issues.is_empty(), "{row_type:?}: {issues:?}");
        }
    }
}
