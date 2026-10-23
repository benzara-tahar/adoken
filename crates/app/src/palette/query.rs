//! Palette query language (docs/design/command-palette.md §3): one scope
//! (sigil or keyword), `key:value` filters, free text, and direct jumps
//! (`!912`, `#1234`, bare ids, pasted ADO links).
//!
//! Parsing is pure and allocation-light; it runs on every keystroke before
//! the search job is spawned.

use adoken_search::DocKind;

/// A parsed palette query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub scope: Option<DocKind>,
    /// Local pins are a collection scope, independent of entity kind.
    pub pins: bool,
    /// Free text handed to the fuzzy matcher (filters removed).
    pub text: String,
    pub filters: Vec<Filter>,
    pub jump: Option<Jump>,
    /// `key:` tokens that look like filters but aren't known keys, with the
    /// closest known key when one is within two edits (§3.3).
    pub unknown: Vec<UnknownFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFilter {
    pub key: String,
    pub suggestion: Option<FilterKey>,
}

/// One `key:value[,value…]` token; values OR, different keys AND.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub key: FilterKey,
    pub values: Vec<String>,
    pub negated: bool,
}

/// Direct jump targets (§3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Jump {
    /// `!912`, `pr:912`.
    PullRequest(i64),
    /// `#1234`, `wi:1234`.
    WorkItem(i64),
    /// Bare digits: PR and work item with that id, if either exists.
    Id(i64),
    /// A pasted ADO URL.
    Link(Link),
}

/// A parsed `dev.azure.com` / `*.visualstudio.com` link (spec 0003 FR-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub org: String,
    pub target: LinkTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    PullRequest(i64),
    WorkItem(i64),
    Build(i64),
}

macro_rules! filter_keys {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// Filter keys (§3.3).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum FilterKey { $($variant),+ }
        impl FilterKey {
            pub const ALL: &'static [FilterKey] = &[$(FilterKey::$variant),+];
            pub fn name(self) -> &'static str {
                match self { $(FilterKey::$variant => $name),+ }
            }
        }
    };
}

filter_keys! {
    Is => "is",
    Author => "author",
    Reviewer => "reviewer",
    Vote => "vote",
    Repo => "repo",
    Branch => "branch",
    Target => "target",
    Type => "type",
    State => "state",
    Assigned => "assigned",
    Priority => "priority",
    Area => "area",
    Iteration => "iteration",
    Tag => "tag",
    Status => "status",
    By => "by",
    Project => "project",
    Updated => "updated",
    Created => "created",
}

impl FilterKey {
    /// Case-insensitive, with the spec's aliases (`assignee:`).
    pub fn parse(key: &str) -> Option<Self> {
        if key.eq_ignore_ascii_case("assignee") {
            return Some(FilterKey::Assigned);
        }
        FilterKey::ALL
            .iter()
            .copied()
            .find(|k| k.name().eq_ignore_ascii_case(key))
    }

    /// Kinds this key narrows; other kinds are hidden while it is set.
    pub fn applies_to(self, kind: DocKind) -> bool {
        use FilterKey as F;
        match self {
            F::Is | F::Updated | F::Created => {
                matches!(kind, DocKind::PullRequest | DocKind::WorkItem)
            }
            F::Author | F::Reviewer | F::Vote | F::Repo | F::Target => kind == DocKind::PullRequest,
            F::Branch => matches!(kind, DocKind::PullRequest | DocKind::Pipeline),
            F::Type | F::State | F::Assigned | F::Priority | F::Area | F::Iteration | F::Tag => {
                kind == DocKind::WorkItem
            }
            F::Status | F::By => kind == DocKind::Pipeline,
            F::Project => !matches!(kind, DocKind::Person | DocKind::Command),
        }
    }
}

/// Scope sigils (§3.1): scope immediately, no space needed.
const SIGILS: &[(char, DocKind)] = &[
    ('>', DocKind::Command),
    ('!', DocKind::PullRequest),
    ('#', DocKind::WorkItem),
    ('@', DocKind::Person),
];

/// Scope keywords: a chip once followed by a space; `pr:912` / `wi:912`
/// also jump directly.
const KEYWORDS: &[(&str, DocKind)] = &[
    ("cmd:", DocKind::Command),
    ("pr:", DocKind::PullRequest),
    ("wi:", DocKind::WorkItem),
    ("people:", DocKind::Person),
    ("p:", DocKind::Pipeline),
    ("pipe:", DocKind::Pipeline),
    ("r:", DocKind::Repo),
    ("repo:", DocKind::Repo),
    ("proj:", DocKind::Project),
];

impl Query {
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        let mut query = Query::default();
        if let Some(link) = parse_link(raw) {
            query.jump = Some(Jump::Link(link));
            return query;
        }
        let rest = query.take_scope(raw);
        let mut text = String::new();
        for token in tokenize(rest) {
            match classify(&token) {
                Token::Filter(filter) => query.filters.push(filter),
                Token::Unknown(key) => {
                    query.unknown.push(UnknownFilter {
                        suggestion: suggest_key(&key),
                        key,
                    });
                    push_word(&mut text, &token);
                }
                Token::Text => push_word(&mut text, &token),
            }
        }
        query.jump = match (query.scope, text.parse::<i64>()) {
            (_, Err(_)) => None,
            (Some(DocKind::PullRequest), Ok(id)) => Some(Jump::PullRequest(id)),
            (Some(DocKind::WorkItem), Ok(id)) => Some(Jump::WorkItem(id)),
            (None, Ok(id)) if query.filters.is_empty() => Some(Jump::Id(id)),
            _ => None,
        };
        query.text = text;
        query
    }

    /// Strip a leading scope, returning the remainder.
    fn take_scope<'a>(&mut self, raw: &'a str) -> &'a str {
        if raw
            .get(..4)
            .is_some_and(|head| head.eq_ignore_ascii_case("pin:"))
        {
            self.pins = true;
            return &raw[4..];
        }
        if let Some(first) = raw.chars().next()
            && let Some(&(_, kind)) = SIGILS.iter().find(|(sigil, _)| *sigil == first)
        {
            self.scope = Some(kind);
            return &raw[first.len_utf8()..];
        }
        for &(keyword, kind) in KEYWORDS {
            let Some(head) = raw.get(..keyword.len()) else {
                continue;
            };
            if !head.eq_ignore_ascii_case(keyword) {
                continue;
            }
            let rest = &raw[keyword.len()..];
            let jumps = matches!(kind, DocKind::PullRequest | DocKind::WorkItem)
                && !rest.is_empty()
                && rest.bytes().all(|b| b.is_ascii_digit());
            if rest.starts_with(char::is_whitespace) || jumps {
                self.scope = Some(kind);
                return rest;
            }
        }
        raw
    }

    /// Nothing typed: no scope, text, filters, or jump.
    pub fn is_blank(&self) -> bool {
        self.scope.is_none()
            && !self.pins
            && self.text.is_empty()
            && self.filters.is_empty()
            && self.jump.is_none()
    }

    /// Whether `kind` can appear in the results at all.
    pub fn admits(&self, kind: DocKind) -> bool {
        self.scope.is_none_or(|scope| scope == kind)
            && self.filters.iter().all(|f| f.key.applies_to(kind))
    }
}

fn push_word(text: &mut String, word: &str) {
    if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(word);
}

enum Token {
    Filter(Filter),
    Unknown(String),
    Text,
}

fn classify(token: &str) -> Token {
    let (negated, body) = match token.strip_prefix('-') {
        Some(body) if body.contains(':') => (true, body),
        _ => (false, token),
    };
    let Some((key, value)) = body.split_once(':') else {
        return Token::Text;
    };
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Token::Text;
    }
    let Some(key) = FilterKey::parse(key) else {
        return if value.is_empty() {
            // Still typing the key (`auth:` → keep as text, no warning yet).
            Token::Text
        } else {
            Token::Unknown(key.to_ascii_lowercase())
        };
    };
    let values: Vec<String> = unquote(value)
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect();
    if values.is_empty() {
        // `author:` with no value yet narrows nothing.
        return Token::Text;
    }
    Token::Filter(Filter {
        key,
        values,
        negated,
    })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .map(|v| v.strip_suffix('"').unwrap_or(v))
        .unwrap_or(value)
}

/// Whitespace-separated tokens; double quotes keep spaces (`repo:"Payments API"`).
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in input.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Closest known key within two edits (`auther` → `author`).
fn suggest_key(key: &str) -> Option<FilterKey> {
    FilterKey::ALL
        .iter()
        .map(|k| (osa_distance(key, k.name()), *k))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k)
}

/// Optimal string alignment distance (a transposition is one edit).
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// FR-7: `https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{id}`,
/// `…/_workitems/edit/{id}`, `…/_build/results?buildId={id}`, and the legacy
/// `https://{org}.visualstudio.com/…` forms.
pub fn parse_link(input: &str) -> Option<Link> {
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let (org, path) = if host == "dev.azure.com" {
        let (org, path) = path.split_once('/')?;
        (org.to_owned(), path)
    } else {
        let org = host.strip_suffix(".visualstudio.com")?;
        (org.to_owned(), path)
    };
    if org.is_empty() {
        return None;
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let id_after = |marker: &str| {
        segments
            .windows(2)
            .find(|w| w[0].eq_ignore_ascii_case(marker))
            .and_then(|w| w[1].parse::<i64>().ok())
    };
    let target = if let Some(id) = id_after("pullrequest") {
        LinkTarget::PullRequest(id)
    } else if let Some(id) = segments
        .windows(3)
        .find(|w| w[0].eq_ignore_ascii_case("_workitems") && w[1].eq_ignore_ascii_case("edit"))
        .and_then(|w| w[2].parse::<i64>().ok())
    {
        LinkTarget::WorkItem(id)
    } else if segments.iter().any(|s| s.eq_ignore_ascii_case("_build")) {
        let id = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("buildId="))?
            .parse::<i64>()
            .ok()?;
        LinkTarget::Build(id)
    } else {
        return None;
    };
    Some(Link { org, target })
}

/// A date filter value (§3.3): `<7d` (within), `>2w` (older than), or
/// `2026-09-01..2026-09-15`. Bounds are inclusive Unix ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub from: Option<i64>,
    pub to: Option<i64>,
}

impl DateRange {
    pub fn parse(value: &str, now: i64) -> Option<Self> {
        const DAY: i64 = 24 * 60 * 60 * 1000;
        if let Some((a, b)) = value.split_once("..") {
            return Some(Self {
                from: Some(parse_date(a)?),
                to: Some(parse_date(b)? + DAY - 1),
            });
        }
        let (newer, amount) = match value.as_bytes().first()? {
            b'<' => (true, &value[1..]),
            b'>' => (false, &value[1..]),
            _ => return None,
        };
        let unit = amount.chars().last()?;
        let n: i64 = amount[..amount.len() - unit.len_utf8()].parse().ok()?;
        let span = n * match unit {
            'h' => DAY / 24,
            'd' => DAY,
            'w' => 7 * DAY,
            'm' => 30 * DAY,
            'y' => 365 * DAY,
            _ => return None,
        };
        let edge = now - span;
        Some(if newer {
            Self {
                from: Some(edge),
                to: None,
            }
        } else {
            Self {
                from: None,
                to: Some(edge),
            }
        })
    }

    pub fn contains(self, at: i64) -> bool {
        self.from.is_none_or(|from| at >= from) && self.to.is_none_or(|to| at <= to)
    }
}

/// `YYYY-MM-DD` → Unix ms at 00:00 UTC.
fn parse_date(s: &str) -> Option<i64> {
    let mut parts = s.trim().splitn(3, '-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    Some(date.midnight().assume_utc().unix_timestamp() * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(key: FilterKey, values: &[&str]) -> Filter {
        Filter {
            key,
            values: values.iter().map(|v| (*v).to_owned()).collect(),
            negated: false,
        }
    }

    #[test]
    fn sigils_scope_immediately_keywords_need_a_space() {
        let q = Query::parse("!retry");
        assert_eq!(
            (q.scope, q.text.as_str()),
            (Some(DocKind::PullRequest), "retry")
        );
        assert_eq!(Query::parse("@dana").scope, Some(DocKind::Person));
        assert_eq!(Query::parse(">sync").scope, Some(DocKind::Command));
        // A keyword without the space is still being typed.
        let q = Query::parse("pr:");
        assert_eq!((q.scope, q.text.as_str()), (None, "pr:"));
        let q = Query::parse("Repo: payments");
        assert_eq!(
            (q.scope, q.text.as_str()),
            (Some(DocKind::Repo), "payments")
        );
        // `p:` must not swallow `proj:` or `people:`.
        assert_eq!(Query::parse("proj: web").scope, Some(DocKind::Project));
        assert_eq!(Query::parse("people: dana").scope, Some(DocKind::Person));
        assert_eq!(Query::parse("p: deploy").scope, Some(DocKind::Pipeline));
        assert!(Query::parse("   ").is_blank());
        assert!(!Query::parse(">").is_blank());
    }

    #[test]
    fn filters_are_pulled_out_of_the_text() {
        let q = Query::parse("!retry author:@me -is:draft idem");
        assert_eq!(q.scope, Some(DocKind::PullRequest));
        assert_eq!(q.text, "retry idem");
        assert_eq!(
            q.filters,
            [
                filter(FilterKey::Author, &["@me"]),
                Filter {
                    negated: true,
                    ..filter(FilterKey::Is, &["draft"])
                },
            ]
        );
        let q = Query::parse(r#"repo:"Payments API" type:bug,task assignee:dana"#);
        assert_eq!(q.text, "");
        assert_eq!(
            q.filters,
            [
                filter(FilterKey::Repo, &["Payments API"]),
                filter(FilterKey::Type, &["bug", "task"]),
                filter(FilterKey::Assigned, &["dana"]),
            ]
        );
    }

    #[test]
    fn unknown_keys_stay_text_with_a_suggestion() {
        let q = Query::parse("auther:dana login");
        assert_eq!(q.text, "auther:dana login");
        assert_eq!(
            q.unknown,
            [UnknownFilter {
                key: "auther".into(),
                suggestion: Some(FilterKey::Author),
            }]
        );
        assert_eq!(Query::parse("zzzzzz:1").unknown[0].suggestion, None);
        // A key still being typed, or a value-less known key, warns about nothing.
        let q = Query::parse("auth: author:");
        assert!(q.unknown.is_empty() && q.filters.is_empty());
        // Negation needs a key; a leading dash alone is text.
        assert_eq!(Query::parse("-draft").text, "-draft");
    }

    #[test]
    fn direct_ids_jump() {
        assert_eq!(Query::parse("!912").jump, Some(Jump::PullRequest(912)));
        assert_eq!(Query::parse("pr:912").jump, Some(Jump::PullRequest(912)));
        assert_eq!(Query::parse("pr:912").scope, Some(DocKind::PullRequest));
        assert_eq!(Query::parse("#48213").jump, Some(Jump::WorkItem(48213)));
        assert_eq!(Query::parse("wi: 48213").jump, Some(Jump::WorkItem(48213)));
        assert_eq!(Query::parse(" 4821 ").jump, Some(Jump::Id(4821)));
        // Text, other scopes, or a filter-narrowed id are not jumps.
        assert_eq!(Query::parse("4821 retry").jump, None);
        assert_eq!(Query::parse("@4821").jump, None);
        assert_eq!(Query::parse("4821 author:@me").jump, None);
        assert_eq!(Query::parse("!").jump, None);
    }

    #[test]
    fn links_parse_to_targets() {
        let link = |s: &str| match Query::parse(s).jump {
            Some(Jump::Link(link)) => Some((link.org, link.target)),
            _ => None,
        };
        assert_eq!(
            link("https://dev.azure.com/fabrikam/Web/_git/site/pullrequest/912"),
            Some(("fabrikam".into(), LinkTarget::PullRequest(912)))
        );
        assert_eq!(
            link("https://dev.azure.com/contoso/Payments/_workitems/edit/48213/"),
            Some(("contoso".into(), LinkTarget::WorkItem(48213)))
        );
        assert_eq!(
            link("https://dev.azure.com/contoso/Payments/_build/results?buildId=77&view=logs"),
            Some(("contoso".into(), LinkTarget::Build(77)))
        );
        assert_eq!(
            link("https://Contoso.visualstudio.com/Web/_git/site/pullRequest/5"),
            Some(("contoso".into(), LinkTarget::PullRequest(5)))
        );
        assert_eq!(link("https://dev.azure.com/contoso/Payments/_boards"), None);
        assert_eq!(link("https://example.com/pullrequest/5"), None);
    }

    #[test]
    fn filters_narrow_the_kinds_shown() {
        let q = Query::parse("type:bug login");
        assert!(q.admits(DocKind::WorkItem));
        assert!(!q.admits(DocKind::Pipeline));
        assert!(!q.admits(DocKind::PullRequest));
        let q = Query::parse("#login");
        assert!(!q.admits(DocKind::PullRequest));
        assert!(Query::parse("login").admits(DocKind::Command));
    }

    #[test]
    fn date_ranges() {
        const DAY: i64 = 86_400_000;
        let now = 100 * DAY;
        let within = DateRange::parse("<7d", now).unwrap();
        assert!(within.contains(now - 6 * DAY) && !within.contains(now - 8 * DAY));
        let older = DateRange::parse(">2w", now).unwrap();
        assert!(older.contains(now - 15 * DAY) && !older.contains(now - 13 * DAY));
        let span = DateRange::parse("2026-09-01..2026-09-15", now).unwrap();
        let sept_15_noon = parse_date("2026-09-15").unwrap() + DAY / 2;
        assert!(span.contains(sept_15_noon));
        assert!(!span.contains(parse_date("2026-09-16").unwrap()));
        assert_eq!(DateRange::parse("soon", now), None);
        assert_eq!(DateRange::parse("<7x", now), None);
    }

    #[test]
    fn pin_collection_scope_does_not_restrict_kind_and_retains_entity_filters() {
        let query = Query::parse("PIN: ssO type:bug");
        assert!(query.pins);
        assert_eq!(query.scope, None);
        assert_eq!(query.text, "ssO");
        assert!(query.admits(DocKind::WorkItem));
        assert!(!query.admits(DocKind::PullRequest));
        assert!(!Query::parse("pin:").is_blank());
        assert!(Query::parse("pin:").admits(DocKind::Pipeline));
    }
}
