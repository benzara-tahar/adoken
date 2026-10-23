//! Scope sets as data (0001 FR-5, §5.2; rows and copy from
//! `docs/design/onboarding.md` §4). Onboarding screens 2b, 2p, 2q and
//! Settings › Account render these verbatim; PAT creation joins their ids.

use serde::{Deserialize, Serialize};

/// Which scope set adoken asks for (0001 §3 step 7, Q-3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    /// Read everything + vote, comment, resolve, complete PRs, and link work
    /// items to them.
    #[default]
    Review,
    /// No PR actions.
    ReadOnly,
}

impl AccessLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Review => "Review",
            Self::ReadOnly => "Read-only",
        }
    }

    /// One line on what the level does (design §2b, §10.1).
    pub fn summary(self) -> &'static str {
        match self {
            Self::Review => "Vote, comment, and edit work items from adoken.",
            Self::ReadOnly => "Browse only. Voting, commenting and editing open in the browser.",
        }
    }

    /// The "Why these?" disclosure under the scope list (design §4).
    pub fn why_these(self) -> &'static str {
        match self {
            Self::Review => {
                "adoken reads what it shows you and writes only when you act: a vote, a comment, a pull request status, or a work item edit. It never changes code, pipelines or project settings, and it can't run pipelines or reach other organizations."
            }
            Self::ReadOnly => {
                "adoken reads what it shows you. It never writes anything, and it can't reach other organizations."
            }
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Review => Self::ReadOnly,
            Self::ReadOnly => Self::Review,
        }
    }
}

/// One row of the scope list.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Scope {
    /// `vso.*` id sent to Azure DevOps.
    pub id: &'static str,
    /// ADO's label in the PAT dialog, e.g. "Code".
    pub label: &'static str,
    /// ADO's access column, e.g. "Read & write".
    pub access: &'static str,
    /// `label (access)`, e.g. "Build (Read)".
    pub title: &'static str,
    /// Without it there is no inbox; onboarding can't finish.
    pub required: bool,
    /// "What it allows" (design §4).
    pub why: &'static str,
    /// "If missing" (design §4): the consequence Settings › Account shows
    /// when the token lacks the scope. Required read scopes block
    /// onboarding instead, so they have none.
    pub if_missing: Option<&'static str>,
}

/// Whether a token holding `granted` may call what needs `needed`: the same
/// scope, or the `_write` scope that includes the read one (§5.2: Review
/// gets `vso.code` inside `vso.code_write`).
pub fn covers(granted: &str, needed: &str) -> bool {
    granted == needed
        || granted
            .strip_suffix("_write")
            .is_some_and(|read| read == needed)
}

/// A scope row's state for the stored token (0009 FR-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeStatus {
    Granted,
    /// A pasted token's write scope before its first write (§5.3).
    NotCheckedYet,
    Missing,
}

impl ScopeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Granted => "Granted",
            Self::NotCheckedYet => "Not checked yet",
            Self::Missing => "Missing",
        }
    }
}

const BUILD: Scope = Scope {
    id: "vso.build",
    label: "Build",
    access: "Read",
    title: "Build (Read)",
    required: false,
    why: "See pipeline runs and their stages for your pull requests, branches and pins.",
    if_missing: Some("Pipelines won't sync."),
};
const CODE_WRITE: Scope = Scope {
    id: "vso.code_write",
    label: "Code",
    access: "Read & write",
    title: "Code (Read & write)",
    required: true,
    why: "Read pull requests, diffs and comments. Vote, comment, resolve threads, and complete, abandon or draft pull requests.",
    // Only a refused write (0001 FR-17) can find it missing: reads work.
    if_missing: Some("You can't vote, comment or change pull requests from adoken."),
};
const CODE_READ: Scope = Scope {
    id: "vso.code",
    label: "Code",
    access: "Read",
    title: "Code (Read)",
    required: true,
    why: "Read pull requests, diffs and comments.",
    if_missing: None,
};
const IDENTITY: Scope = Scope {
    id: "vso.identity",
    label: "Identity",
    access: "Read",
    title: "Identity (Read)",
    required: false,
    why: "Show teammates' names and pictures, and find people when you assign or mention them.",
    if_missing: Some(
        "Pictures show initials, and people search only finds people adoken has already seen.",
    ),
};
const PROJECT: Scope = Scope {
    id: "vso.project",
    label: "Project and Team",
    access: "Read",
    title: "Project and Team (Read)",
    required: true,
    why: "List your projects and teams.",
    if_missing: None,
};
const WORK: Scope = Scope {
    id: "vso.work",
    label: "Work Items",
    access: "Read",
    title: "Work Items (Read)",
    required: false,
    why: "Show work assigned to you and items linked to pull requests.",
    if_missing: Some("Work items won't sync."),
};
/// Review only (user decision 2026-09-30): editing work items and linking
/// them to pull requests.
const WORK_WRITE: Scope = Scope {
    id: "vso.work_write",
    label: "Work Items",
    access: "Read & write",
    title: "Work Items (Read & write)",
    required: false,
    why: "Read and edit work items: state, assignee, area, iteration, title, description, links and comments. Link work items to pull requests.",
    if_missing: Some("Work items won't sync, and you can't edit or link them."),
};

// In the order Azure DevOps' PAT dialog lists them.
static REVIEW: [Scope; 5] = [BUILD, CODE_WRITE, IDENTITY, PROJECT, WORK_WRITE];
static READ_ONLY: [Scope; 5] = [BUILD, CODE_READ, IDENTITY, PROJECT, WORK];

/// The scope rows for `level`, in ADO dialog order.
pub fn scopes(level: AccessLevel) -> &'static [Scope] {
    match level {
        AccessLevel::Review => &REVIEW,
        AccessLevel::ReadOnly => &READ_ONLY,
    }
}

/// Space-separated ids for the PAT create request.
pub fn scope_string(level: AccessLevel) -> String {
    scopes(level)
        .iter()
        .map(|scope| scope.id)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ids(level: AccessLevel) -> BTreeSet<&'static str> {
        scopes(level).iter().map(|scope| scope.id).collect()
    }

    #[test]
    fn scope_sets_match_spec_5_2() {
        assert_eq!(
            ids(AccessLevel::Review),
            BTreeSet::from([
                "vso.project",
                "vso.code_write",
                "vso.work_write",
                "vso.build",
                "vso.identity"
            ])
        );
        assert_eq!(
            ids(AccessLevel::ReadOnly),
            BTreeSet::from([
                "vso.project",
                "vso.code",
                "vso.work",
                "vso.build",
                "vso.identity"
            ])
        );
    }

    #[test]
    fn never_requests_full_or_high_privilege_extras() {
        for level in [AccessLevel::Review, AccessLevel::ReadOnly] {
            for id in ids(level) {
                assert!(
                    !matches!(
                        id,
                        "app_token" | "vso.build_execute" | "vso.threads_full" | "vso.work_full"
                    ),
                    "{id}"
                );
            }
        }
        // Read-only never writes: no write scope of any kind.
        assert!(
            ids(AccessLevel::ReadOnly)
                .iter()
                .all(|id| !id.ends_with("_write"))
        );
    }

    #[test]
    fn exactly_code_and_project_are_required_and_optional_rows_explain_themselves() {
        for level in [AccessLevel::Review, AccessLevel::ReadOnly] {
            let required: BTreeSet<_> = scopes(level)
                .iter()
                .filter(|scope| scope.required)
                .map(|scope| scope.label)
                .collect();
            assert_eq!(required, BTreeSet::from(["Code", "Project and Team"]));
            for scope in scopes(level) {
                if !scope.required {
                    assert!(scope.if_missing.is_some(), "{}", scope.id);
                }
                assert_eq!(scope.title, format!("{} ({})", scope.label, scope.access));
            }
        }
    }

    #[test]
    fn rows_follow_the_azure_devops_dialog_order() {
        for level in [AccessLevel::Review, AccessLevel::ReadOnly] {
            let labels: Vec<_> = scopes(level).iter().map(|scope| scope.label).collect();
            assert_eq!(
                labels,
                [
                    "Build",
                    "Code",
                    "Identity",
                    "Project and Team",
                    "Work Items"
                ]
            );
        }
    }

    #[test]
    fn a_write_scope_covers_its_read_scope_only() {
        assert!(covers("vso.code_write", "vso.code"));
        assert!(covers("vso.code", "vso.code"));
        assert!(covers("vso.work_write", "vso.work"));
        assert!(!covers("vso.code", "vso.code_write"));
        assert!(!covers("vso.code_write", "vso.work"));
        assert!(!covers("vso.build", "vso.build_execute"));
    }
}
