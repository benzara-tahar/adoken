//! Every Azure DevOps route adoken calls (spec 0016 FR-19): method, path
//! pattern and purpose label. The request recorder templates a path by
//! matching it against this allow-list, so a new route with a name after
//! `_apis` can't leak into the log: it's "Other request" with every
//! unknown segment replaced, and a debug assert fires (AC-B3).
//!
//! Patterns are the path after the org segment (`dev.azure.com/{org}/…`,
//! `vssps.dev.azure.com/{org}/…`; legacy `{org}.visualstudio.com/…` has
//! none). `{n}` matches digits; every other placeholder (`{project}`,
//! `{repo}`, `{name}`, `{id}` for GUIDs, `{sha}` for git object ids)
//! matches any one segment and is what the log shows in its place.
//! Literals compare case-insensitively (Azure DevOps mixes
//! `pullRequests` / `pullrequests`).

use adoken_model::Subject;

/// One allow-listed route.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub method: &'static str,
    pub pattern: &'static str,
    pub purpose: &'static str,
}

const fn r(method: &'static str, pattern: &'static str, purpose: &'static str) -> Route {
    Route {
        method,
        pattern,
        purpose,
    }
}

const PR: &str = "{project}/_apis/git/repositories/{repo}/pullRequests/{n}";

/// The table. Order matters only where two patterns could match the same
/// path (none today).
pub const ROUTES: &[Route] = &[
    // Bootstrap and structure (0002 FR-2, 0014 §5).
    r("GET", "_apis/connectionData", "Connection data"),
    r("GET", "_apis/projects", "Projects"),
    r("GET", "_apis/projects/{project}", "Project"),
    r("GET", "{project}/_apis/git/repositories", "Repositories"),
    r("GET", "_apis/git/repositories", "Repositories"),
    // Pull requests (0002, 0004, 0014).
    r("GET", "{project}/_apis/git/pullrequests", "Active PRs"),
    r("GET", "{project}/_apis/git/pullrequests/{n}", "PR"),
    r("GET", "_apis/git/pullrequests/{n}", "Pinned PR"),
    r(
        "GET",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/iterations",
        "PR iterations",
    ),
    r(
        "GET",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/iterations/{n}/changes",
        "PR files",
    ),
    r(
        "GET",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads",
        "PR threads",
    ),
    r(
        "POST",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads",
        "Comment",
    ),
    r(
        "POST",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads/{n}/comments",
        "Reply",
    ),
    r(
        "PATCH",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads/{n}",
        "Thread status",
    ),
    r(
        "PUT",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/reviewers/{id}",
        "Vote",
    ),
    r("PATCH", PR, "PR update"),
    r(
        "GET",
        "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/workitems",
        "PR work items",
    ),
    r(
        "GET",
        "{project}/_apis/git/repositories/{repo}/blobs/{sha}",
        "File content",
    ),
    // Work items (0002, 0004 FR-17, 0014, 0015).
    r("POST", "_apis/wit/wiql", "Work item query"),
    r("POST", "{project}/_apis/wit/wiql", "Work item query"),
    r("GET", "_apis/wit/workitems", "Work items"),
    r("POST", "_apis/wit/workitemsbatch", "Work items batch"),
    r("GET", "_apis/wit/workitems/{n}", "Work item"),
    r("GET", "{project}/_apis/wit/workitems/{n}", "Work item"),
    r("PATCH", "_apis/wit/workitems/{n}", "Work item update"),
    r(
        "GET",
        "{project}/_apis/wit/reporting/workitemrevisions",
        "Work item revisions",
    ),
    r(
        "GET",
        "{project}/_apis/wit/workItems/{n}/comments",
        "Work item comments",
    ),
    r(
        "POST",
        "{project}/_apis/wit/workItems/{n}/comments",
        "Work item comment",
    ),
    r(
        "GET",
        "{project}/_apis/wit/workitemtypes/{name}",
        "Work item type",
    ),
    r(
        "GET",
        "{project}/_apis/wit/workitemtypes/{name}/fields",
        "Work item fields",
    ),
    r("GET", "_apis/wit/fields/{name}", "Work item field"),
    r(
        "GET",
        "{project}/_apis/wit/classificationnodes",
        "Areas and iterations",
    ),
    r("GET", "_apis/identities", "People search"),
    // Pipelines (0002, 0008, 0014).
    r("GET", "{project}/_apis/build/builds", "Builds"),
    r("GET", "{project}/_apis/build/builds/{n}", "Build"),
    r(
        "GET",
        "{project}/_apis/build/builds/{n}/timeline",
        "Build timeline",
    ),
    r(
        "GET",
        "{project}/_apis/build/definitions/{n}",
        "Pipeline definition",
    ),
    // Avatars (0012).
    r("GET", "_api/_common/identityImage", "Avatar"),
];

/// Purpose of a path no route matches.
pub const OTHER: &str = "Other request";

/// A templated request target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matched {
    /// `dev.azure.com`, or `{org}.visualstudio.com` for legacy hosts.
    pub host: String,
    /// `/{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads`.
    pub route: String,
    pub purpose: &'static str,
    /// The item it was about (in-app only, Q-2).
    pub subject: Option<Subject>,
    /// Matched a table entry (false: "Other request").
    pub known: bool,
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn token_matches(token: &str, seg: &str) -> bool {
    match token {
        "{n}" => is_digits(seg),
        t if t.starts_with('{') => !seg.is_empty(),
        literal => literal.eq_ignore_ascii_case(seg),
    }
}

/// The host as it may be logged: a legacy `{org}.visualstudio.com` host
/// loses the org name.
fn template_host(host: &str) -> (String, bool) {
    let lower = host.to_ascii_lowercase();
    match lower.strip_suffix(".visualstudio.com") {
        Some(_) => ("{org}.visualstudio.com".to_owned(), true),
        None => (lower, false),
    }
}

/// The item a route is about, from the raw segments (in-app only).
fn subject(pattern: &[&str], segs: &[&str]) -> Option<Subject> {
    let after = |word: &str| {
        let i = pattern.iter().position(|t| t.eq_ignore_ascii_case(word))?;
        (pattern.get(i + 1) == Some(&"{n}"))
            .then(|| segs.get(i + 1)?.parse::<i64>().ok())
            .flatten()
    };
    after("pullRequests")
        .map(Subject::Pr)
        .or_else(|| after("workitems").map(Subject::WorkItem))
        .or_else(|| after("builds").map(Subject::Build))
}

/// Every literal segment of the table: what "Other request" may keep.
fn known_word(seg: &str) -> bool {
    ROUTES.iter().any(|r| {
        r.pattern
            .split('/')
            .any(|t| !t.starts_with('{') && t.eq_ignore_ascii_case(seg))
    })
}

/// Template `method host path` against the table (query strings are never
/// part of `path`: pass `Url::path()`).
pub fn match_route(method: &str, host: &str, path: &str) -> Matched {
    let (host_t, legacy) = template_host(host);
    let all: Vec<&str> = path.trim_matches('/').split('/').collect();
    // The org segment (absent on legacy hosts).
    let (org, segs) = match (legacy, all.split_first()) {
        (false, Some((_, rest))) => (true, rest),
        _ => (false, &all[..]),
    };
    let prefix = if org { "/{org}" } else { "" };
    for route in ROUTES {
        if route.method != method {
            continue;
        }
        let pattern: Vec<&str> = route.pattern.split('/').collect();
        if pattern.len() != segs.len()
            || !pattern.iter().zip(segs).all(|(t, s)| token_matches(t, s))
        {
            continue;
        }
        let mut out = String::with_capacity(prefix.len() + route.pattern.len() + 1);
        out.push_str(prefix);
        out.push('/');
        out.push_str(route.pattern);
        return Matched {
            host: host_t,
            route: out,
            purpose: route.purpose,
            subject: subject(&pattern, segs),
            known: true,
        };
    }
    // Not in the table: keep only known words, by shape otherwise.
    let apis = segs.iter().position(|s| matches!(*s, "_apis" | "_api"));
    let mut out = String::from(prefix);
    for (i, seg) in segs.iter().enumerate() {
        out.push('/');
        let t = if apis.is_none_or(|a| i < a) {
            "{project}"
        } else if crate::wire::is_guid(seg) {
            "{id}"
        } else if is_sha(seg) {
            "{sha}"
        } else if is_digits(seg) {
            "{n}"
        } else if known_word(seg) {
            seg
        } else {
            "{name}"
        };
        out.push_str(t);
    }
    Matched {
        host: host_t,
        route: out,
        purpose: OTHER,
        subject: None,
        known: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(method: &str, path: &str) -> (String, &'static str) {
        let m = match_route(method, "dev.azure.com", path);
        (m.route, m.purpose)
    }

    /// The shape-based templater's regression cases (0014 §5 and the
    /// 2026-10-01 fixes): org, project names and git object ids never
    /// reach the log.
    #[test]
    fn templates_hide_org_project_repo_ids_and_numbers() {
        assert_eq!(
            route(
                "GET",
                "/contoso/9628fca0-2412-4d44-b078-a7cda9218300/_apis/git/repositories/\
                 0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0/pullRequests/4312/threads"
            ),
            (
                "/{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads".into(),
                "PR threads"
            )
        );
        assert_eq!(
            route("GET", "/contoso/_apis/connectionData").0,
            "/{org}/_apis/connectionData"
        );
        assert_eq!(
            route("GET", "/contoso/Fabrikam/_apis/build/builds/77/timeline").0,
            "/{org}/{project}/_apis/build/builds/{n}/timeline"
        );
        assert_eq!(
            route("GET", "/contoso/_api/_common/identityImage"),
            ("/{org}/_api/_common/identityImage".into(), "Avatar")
        );
        assert_eq!(
            route("GET", "/contoso/_apis/projects/Payments%20Team").0,
            "/{org}/_apis/projects/{project}"
        );
        assert_eq!(
            route(
                "GET",
                "/contoso/9628fca0-2412-4d44-b078-a7cda9218300/_apis/git/repositories/\
                 canary-repo/blobs/95d09f2b10159347eece71399a7e2e907ea3df4f"
            ),
            (
                "/{org}/{project}/_apis/git/repositories/{repo}/blobs/{sha}".into(),
                "File content"
            )
        );
    }

    #[test]
    fn method_picks_the_purpose_and_case_is_ignored() {
        let path = "/o/p/_apis/git/repositories/r/pullrequests/9/threads";
        assert_eq!(route("GET", path).1, "PR threads");
        assert_eq!(route("POST", path).1, "Comment");
        assert_eq!(
            route("PATCH", "/o/_apis/wit/workitems/12").1,
            "Work item update"
        );
        assert_eq!(route("GET", "/o/_apis/wit/workitems/12").1, "Work item");
    }

    #[test]
    fn legacy_hosts_lose_the_org_name() {
        let m = match_route(
            "GET",
            "CanaryOrg.visualstudio.com",
            "/CanaryProject/_apis/git/pullrequests",
        );
        assert_eq!(m.host, "{org}.visualstudio.com");
        assert_eq!(m.route, "/{project}/_apis/git/pullrequests");
        assert!(m.known);
    }

    #[test]
    fn subjects_are_typed_ids_from_the_path() {
        let s = |path| match_route("GET", "dev.azure.com", path).subject;
        assert_eq!(
            s("/o/p/_apis/git/repositories/r/pullRequests/4821/threads"),
            Some(Subject::Pr(4821))
        );
        assert_eq!(
            s("/o/p/_apis/build/builds/77/timeline"),
            Some(Subject::Build(77))
        );
        assert_eq!(
            s("/o/p/_apis/wit/workItems/20101/comments"),
            Some(Subject::WorkItem(20101))
        );
        assert_eq!(s("/o/p/_apis/build/builds"), None);
    }

    /// FR-19: an unknown route keeps only vocabulary; names become
    /// `{name}`.
    #[test]
    fn unknown_routes_are_other_requests_without_names() {
        let m = match_route(
            "GET",
            "dev.azure.com",
            "/canaryorg/CanaryProject/_apis/git/repositories/canary-repo/refs/heads/canary-branch/42",
        );
        assert!(!m.known);
        assert_eq!(m.purpose, OTHER);
        assert_eq!(
            m.route,
            "/{org}/{project}/_apis/git/repositories/{name}/{name}/{name}/{name}/{n}"
        );
    }
}

/// Spec 0001 AC-17: every route mapped to the one §5.2 scope it needs, so
/// the Review token covers everything adoken calls (FR-5, FR-12). A route
/// added to [`ROUTES`] without a row here fails the test below.
#[cfg(test)]
mod scopes {
    use super::ROUTES;
    use adoken_ado::{AccessLevel, covers, scope_string, scopes};

    /// `(method, pattern, scope)`; `None` only for `connectionData` (any
    /// valid PAT).
    const ROUTE_SCOPES: &[(&str, &str, Option<&str>)] = &[
        ("GET", "_apis/connectionData", None),
        ("GET", "_apis/projects", Some("vso.project")),
        ("GET", "_apis/projects/{project}", Some("vso.project")),
        ("GET", "{project}/_apis/git/repositories", Some("vso.code")),
        ("GET", "_apis/git/repositories", Some("vso.code")),
        ("GET", "{project}/_apis/git/pullrequests", Some("vso.code")),
        (
            "GET",
            "{project}/_apis/git/pullrequests/{n}",
            Some("vso.code"),
        ),
        ("GET", "_apis/git/pullrequests/{n}", Some("vso.code")),
        (
            "GET",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/iterations",
            Some("vso.code"),
        ),
        (
            "GET",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/iterations/{n}/changes",
            Some("vso.code"),
        ),
        (
            "GET",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads",
            Some("vso.code"),
        ),
        (
            "POST",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads",
            Some("vso.code_write"),
        ),
        (
            "POST",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads/{n}/comments",
            Some("vso.code_write"),
        ),
        (
            "PATCH",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads/{n}",
            Some("vso.code_write"),
        ),
        (
            "PUT",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/reviewers/{id}",
            Some("vso.code_write"),
        ),
        (
            "PATCH",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}",
            Some("vso.code_write"),
        ),
        (
            "GET",
            "{project}/_apis/git/repositories/{repo}/pullRequests/{n}/workitems",
            Some("vso.code"),
        ),
        (
            "GET",
            "{project}/_apis/git/repositories/{repo}/blobs/{sha}",
            Some("vso.code"),
        ),
        ("POST", "_apis/wit/wiql", Some("vso.work")),
        ("POST", "{project}/_apis/wit/wiql", Some("vso.work")),
        ("GET", "_apis/wit/workitems", Some("vso.work")),
        ("POST", "_apis/wit/workitemsbatch", Some("vso.work")),
        ("GET", "_apis/wit/workitems/{n}", Some("vso.work")),
        ("GET", "{project}/_apis/wit/workitems/{n}", Some("vso.work")),
        ("PATCH", "_apis/wit/workitems/{n}", Some("vso.work_write")),
        (
            "GET",
            "{project}/_apis/wit/reporting/workitemrevisions",
            Some("vso.work"),
        ),
        (
            "GET",
            "{project}/_apis/wit/workItems/{n}/comments",
            Some("vso.work"),
        ),
        (
            "POST",
            "{project}/_apis/wit/workItems/{n}/comments",
            Some("vso.work_write"),
        ),
        (
            "GET",
            "{project}/_apis/wit/workitemtypes/{name}",
            Some("vso.work"),
        ),
        (
            "GET",
            "{project}/_apis/wit/workitemtypes/{name}/fields",
            Some("vso.work"),
        ),
        ("GET", "_apis/wit/fields/{name}", Some("vso.work")),
        (
            "GET",
            "{project}/_apis/wit/classificationnodes",
            Some("vso.work"),
        ),
        ("GET", "_apis/identities", Some("vso.identity")),
        ("GET", "{project}/_apis/build/builds", Some("vso.build")),
        ("GET", "{project}/_apis/build/builds/{n}", Some("vso.build")),
        (
            "GET",
            "{project}/_apis/build/builds/{n}/timeline",
            Some("vso.build"),
        ),
        (
            "GET",
            "{project}/_apis/build/definitions/{n}",
            Some("vso.build"),
        ),
        ("GET", "_api/_common/identityImage", Some("vso.identity")),
    ];

    /// The paste path's probes (0001 §5.3, `crates/ado/src/onboarding/
    /// api.rs`), as the route each one calls and the scope it proves.
    const PROBES: &[(&str, &str, &str)] = &[
        ("GET", "_apis/projects", "vso.project"),
        ("GET", "{project}/_apis/git/repositories", "vso.code"),
        ("POST", "{project}/_apis/wit/wiql", "vso.work"),
        ("GET", "{project}/_apis/build/builds", "vso.build"),
        ("GET", "_apis/identities", "vso.identity"),
    ];

    fn mapping(method: &str, pattern: &str) -> Vec<Option<&'static str>> {
        ROUTE_SCOPES
            .iter()
            .filter(|(m, p, _)| *m == method && *p == pattern)
            .map(|(_, _, scope)| *scope)
            .collect()
    }

    #[test]
    fn every_route_needs_exactly_one_scope_of_the_review_set() {
        let review: Vec<&str> = scopes(AccessLevel::Review).iter().map(|s| s.id).collect();
        for route in ROUTES {
            let found = mapping(route.method, route.pattern);
            assert_eq!(
                found.len(),
                1,
                "{} {} needs exactly one §5.2 scope row",
                route.method,
                route.pattern
            );
            match found[0] {
                None => assert_eq!(route.pattern, "_apis/connectionData"),
                Some(needed) => assert!(
                    review.iter().any(|granted| covers(granted, needed)),
                    "{} {} needs {needed}, which the Review token doesn't cover",
                    route.method,
                    route.pattern
                ),
            }
        }
        // No stale rows for routes that no longer exist.
        for (method, pattern, _) in ROUTE_SCOPES {
            assert!(
                ROUTES
                    .iter()
                    .any(|r| r.method == *method && r.pattern == *pattern),
                "{method} {pattern} isn't in ROUTES"
            );
        }
    }

    #[test]
    fn probes_call_routes_mapped_to_the_scope_they_prove() {
        for (method, pattern, scope) in PROBES {
            assert_eq!(
                mapping(method, pattern),
                [Some(*scope)],
                "{method} {pattern}"
            );
        }
    }

    #[test]
    fn scope_strings_are_the_5_2_sets() {
        let sorted = |level| {
            let mut ids: Vec<String> = scope_string(level).split(' ').map(str::to_owned).collect();
            ids.sort();
            ids.join(" ")
        };
        assert_eq!(
            sorted(AccessLevel::Review),
            "vso.build vso.code_write vso.identity vso.project vso.work_write"
        );
        assert_eq!(
            sorted(AccessLevel::ReadOnly),
            "vso.build vso.code vso.identity vso.project vso.work"
        );
    }
}
