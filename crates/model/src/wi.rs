//! Work item detail and editing types (spec 0015): rich text sources,
//! stored relations, freshness and the metadata cache (type states and
//! transitions, classification trees, field rules).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Millis, StateCategory, time::DAY};

str_enum!(
    /// Azure DevOps' format of a long-text field or comment
    /// (`multilineFieldsFormat`, comment `format`).
    TextFormat { Html => "html", Markdown => "markdown" }
);

impl Default for TextFormat {
    /// Azure DevOps' default when the field or comment says nothing.
    fn default() -> Self {
        TextFormat::Html
    }
}

/// A long-text value as Azure DevOps stores it (FR-34).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichText {
    pub source: String,
    pub format: TextFormat,
}

/// The work item links adoken adds and removes (FR-21), in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkKind {
    Parent,
    Child,
    Related,
    Predecessor,
    Successor,
    Duplicate,
    DuplicateOf,
}

impl LinkKind {
    pub const ALL: [LinkKind; 7] = [
        LinkKind::Parent,
        LinkKind::Child,
        LinkKind::Related,
        LinkKind::Predecessor,
        LinkKind::Successor,
        LinkKind::Duplicate,
        LinkKind::DuplicateOf,
    ];

    /// The relation's reference name, as sent in `add /relations/-`.
    pub fn rel(self) -> &'static str {
        match self {
            LinkKind::Parent => "System.LinkTypes.Hierarchy-Reverse",
            LinkKind::Child => "System.LinkTypes.Hierarchy-Forward",
            LinkKind::Related => "System.LinkTypes.Related",
            LinkKind::Predecessor => "System.LinkTypes.Dependency-Reverse",
            LinkKind::Successor => "System.LinkTypes.Dependency-Forward",
            LinkKind::Duplicate => "System.LinkTypes.Duplicate-Forward",
            LinkKind::DuplicateOf => "System.LinkTypes.Duplicate-Reverse",
        }
    }

    pub fn from_rel(rel: &str) -> Option<Self> {
        LinkKind::ALL
            .into_iter()
            .find(|k| k.rel().eq_ignore_ascii_case(rel))
    }

    pub fn label(self) -> &'static str {
        match self {
            LinkKind::Parent => "Parent",
            LinkKind::Child => "Child",
            LinkKind::Related => "Related",
            LinkKind::Predecessor => "Predecessor",
            LinkKind::Successor => "Successor",
            LinkKind::Duplicate => "Duplicate",
            LinkKind::DuplicateOf => "Duplicate of",
        }
    }
}

/// One stored relation of a work item, in Azure DevOps' order (FR-22: the
/// index is what `remove /relations/{index}` takes, valid at
/// [`WorkItemExtras::links_rev`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WiLink {
    pub index: u32,
    /// Raw `rel`: `System.LinkTypes.*`, `ArtifactLink`, `Hyperlink`,
    /// `AttachedFile`, …
    pub rel: String,
    /// Set for the work item links adoken edits.
    pub kind: Option<LinkKind>,
    /// Target of a work item link.
    pub target_id: Option<i64>,
    /// Target of a pull request `ArtifactLink`.
    pub pr_id: Option<i64>,
    pub url: String,
    /// `attributes.name` ("Pull Request", "Fixed in Commit", "Branch", …).
    pub name: Option<String>,
    pub comment: Option<String>,
}

/// What spec 0015 stores per work item beyond [`super::WorkItem`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItemExtras {
    /// `None` until fetched in full since the 0015 migration (the plain
    /// `WorkItem::description` is all there is then).
    pub description: Option<RichText>,
    /// `Microsoft.VSTS.TCM.ReproSteps`, shown read-only (Q-10).
    pub repro_steps: Option<RichText>,
    /// `Microsoft.VSTS.Common.AcceptanceCriteria`, shown read-only (Q-10).
    pub acceptance: Option<RichText>,
    /// `System.Reason` as Azure DevOps set it (full answers only).
    pub reason: Option<String>,
    /// FR-32: the row was last written from an Azure DevOps response.
    pub synced_ms: Option<Millis>,
    /// FR-32: full fields and relations were last applied. `None` reads
    /// "Partly synced" (FR-29).
    pub full_synced_ms: Option<Millis>,
    /// Every relation, in server order.
    pub links: Vec<WiLink>,
    /// The revision `links` were read at.
    pub links_rev: Option<i64>,
}

impl WorkItemExtras {
    /// The work item link of `kind` to `target`, if stored.
    pub fn link(&self, kind: LinkKind, target: i64) -> Option<&WiLink> {
        self.links
            .iter()
            .find(|l| l.kind == Some(kind) && l.target_id == Some(target))
    }

    /// The parent's id, per the stored relations.
    pub fn parent(&self) -> Option<i64> {
        self.links
            .iter()
            .find(|l| l.kind == Some(LinkKind::Parent))
            .and_then(|l| l.target_id)
    }
}

// ── metadata cache (FR-33) ───────────────────────────────────────────────

/// One state of a work item type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WiState {
    pub name: String,
    pub category: StateCategory,
    /// Azure DevOps' color, hex without `#` (`"b2b2b2"`). Views map the
    /// category to theme tokens; this is informational.
    pub color: Option<String>,
}

/// States and transitions of one work item type in one project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeMeta {
    /// In Azure DevOps' order.
    pub states: Vec<WiState>,
    /// `from → [to]`; empty when Azure DevOps sent none.
    pub transitions: HashMap<String, Vec<String>>,
    pub fetched_ms: Millis,
}

impl TypeMeta {
    /// FR-11's state menu from `current`: the reachable states (every state
    /// when no transitions are known), always including `current`, in
    /// category order (Proposed → Removed), stable within a category.
    pub fn menu(&self, current: &str) -> Vec<WiState> {
        let reachable = self
            .transitions
            .iter()
            .find_map(|(from, to)| from.eq_ignore_ascii_case(current).then_some(to.as_slice()));
        let mut out: Vec<WiState> = self
            .states
            .iter()
            .filter(|s| {
                s.name.eq_ignore_ascii_case(current)
                    || self.transitions.is_empty()
                    || reachable
                        .is_some_and(|to| to.iter().any(|t| t.eq_ignore_ascii_case(&s.name)))
            })
            .cloned()
            .collect();
        if !out.iter().any(|s| s.name.eq_ignore_ascii_case(current)) && !current.is_empty() {
            out.push(WiState {
                name: current.to_owned(),
                category: StateCategory::of_state(current),
                color: None,
            });
        }
        out.sort_by_key(|s| category_rank(s.category));
        out
    }

    /// The category of `state` per this type, else the stock mapping.
    pub fn category(&self, state: &str) -> StateCategory {
        self.states
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(state))
            .map_or_else(|| StateCategory::of_state(state), |s| s.category)
    }
}

fn category_rank(c: StateCategory) -> usize {
    StateCategory::ALL.iter().position(|x| *x == c).unwrap_or(0)
}

/// Area or iteration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Area,
    Iteration,
}

/// One classification node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassNode {
    pub kind: NodeKind,
    /// The value `System.AreaPath` / `System.IterationPath` takes:
    /// `Project\Infra\Payments` (the root is the project name).
    pub path: String,
    pub name: String,
    /// 0 for the root.
    pub depth: u16,
    /// Iterations only.
    pub start_ms: Option<Millis>,
    pub finish_ms: Option<Millis>,
}

impl ClassNode {
    /// The path of the parent node, `None` for the root.
    pub fn parent_path(&self) -> Option<&str> {
        self.path.rsplit_once('\\').map(|(parent, _)| parent)
    }
}

/// A project's area and iteration trees, pre-order (each parent before its
/// children, siblings in Azure DevOps' order).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassTree {
    pub areas: Vec<ClassNode>,
    pub iterations: Vec<ClassNode>,
    pub fetched_ms: Millis,
}

/// The iteration picker's shortcuts above the tree (FR-14).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IterationShortcuts {
    /// Dates include today (the deepest such iteration when nested).
    pub current: Option<ClassNode>,
    /// The next start date after today.
    pub next: Option<ClassNode>,
    /// The last two finished, newest first.
    pub recent: Vec<ClassNode>,
}

impl ClassTree {
    /// Current / Next / Recent for `now`. An iteration's finish date is a
    /// day: it includes the whole of that day.
    pub fn shortcuts(&self, now: Millis) -> IterationShortcuts {
        let dated = || {
            self.iterations
                .iter()
                .filter_map(|n| match (n.start_ms, n.finish_ms) {
                    (Some(s), Some(f)) => Some((n, s, f + DAY)),
                    _ => None,
                })
        };
        let current = dated()
            .filter(|(_, s, end)| *s <= now && now < *end)
            .max_by_key(|(n, s, _)| (n.depth, *s))
            .map(|(n, ..)| n.clone());
        let next = dated()
            .filter(|(_, s, _)| *s > now)
            .min_by_key(|(n, s, _)| (*s, std::cmp::Reverse(n.depth)))
            .map(|(n, ..)| n.clone());
        let mut done: Vec<(&ClassNode, Millis)> = dated()
            .filter(|(_, _, end)| *end <= now)
            .map(|(n, _, end)| (n, end))
            .collect();
        done.sort_by_key(|(n, end)| (std::cmp::Reverse(*end), std::cmp::Reverse(n.depth)));
        IterationShortcuts {
            current,
            next,
            recent: done.into_iter().take(2).map(|(n, _)| n.clone()).collect(),
        }
    }
}

/// How adoken can fill a field a rule asks for (FR-9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    /// A value list.
    List(Vec<String>),
    /// A text field.
    Text,
    /// An identity field.
    Person,
    /// Anything else: Open in Azure DevOps.
    Unknown,
}

/// One field of a work item type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldMeta {
    /// `Custom.RootCause`.
    pub reference: String,
    /// Display name, as rule errors name it ("Root cause").
    pub name: String,
    pub kind: FieldKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str, category: StateCategory) -> WiState {
        WiState {
            name: name.into(),
            category,
            color: None,
        }
    }

    fn agile_bug() -> TypeMeta {
        TypeMeta {
            states: vec![
                state("Active", StateCategory::InProgress),
                state("Closed", StateCategory::Completed),
                state("New", StateCategory::Proposed),
                state("Resolved", StateCategory::Resolved),
            ],
            transitions: HashMap::from([
                (
                    "Active".to_owned(),
                    vec![
                        "Active".into(),
                        "Closed".into(),
                        "Resolved".into(),
                        "New".into(),
                    ],
                ),
                ("New".to_owned(), vec!["Active".into()]),
            ]),
            fetched_ms: 0,
        }
    }

    fn names(states: &[WiState]) -> Vec<&str> {
        states.iter().map(|s| s.name.as_str()).collect()
    }

    /// AC-3: the menu lists the reachable states in category order.
    #[test]
    fn ac3_state_menu_lists_reachable_states_in_category_order() {
        let meta = agile_bug();
        assert_eq!(
            names(&meta.menu("Active")),
            ["New", "Active", "Resolved", "Closed"]
        );
        // The current state is always there (it's checked), even when the
        // transitions don't list it.
        assert_eq!(names(&meta.menu("New")), ["New", "Active"]);
        // No transitions known: every state.
        let open = TypeMeta {
            transitions: HashMap::new(),
            ..agile_bug()
        };
        assert_eq!(
            names(&open.menu("Closed")),
            ["New", "Active", "Resolved", "Closed"]
        );
    }

    fn sprint(n: u32, start_day: i64) -> ClassNode {
        ClassNode {
            kind: NodeKind::Iteration,
            path: format!("Payments\\Sprint {n}"),
            name: format!("Sprint {n}"),
            depth: 1,
            start_ms: Some(start_day * DAY),
            finish_ms: Some((start_day + 13) * DAY),
        }
    }

    /// AC-12: Current includes today (the finish day counts), Next is the
    /// next start, Recent the last two finished.
    #[test]
    fn ac12_current_next_and_recent_iterations() {
        let root = ClassNode {
            kind: NodeKind::Iteration,
            path: "Payments".into(),
            name: "Payments".into(),
            depth: 0,
            start_ms: None,
            finish_ms: None,
        };
        let tree = ClassTree {
            iterations: vec![
                root,
                sprint(42, 100),
                sprint(43, 114),
                sprint(44, 128),
                sprint(45, 142),
                sprint(46, 156),
            ],
            ..ClassTree::default()
        };
        let at = |day: i64, hour: i64| day * DAY + hour * 3_600_000;
        let s = tree.shortcuts(at(130, 9));
        assert_eq!(s.current.unwrap().name, "Sprint 44");
        assert_eq!(s.next.unwrap().name, "Sprint 45");
        assert_eq!(
            s.recent.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["Sprint 43", "Sprint 42"]
        );
        // Late on Sprint 44's finish day it's still current.
        assert_eq!(
            tree.shortcuts(at(141, 23)).current.unwrap().name,
            "Sprint 44"
        );
        assert_eq!(
            tree.shortcuts(at(142, 0)).current.unwrap().name,
            "Sprint 45"
        );
        assert_eq!(tree.iterations[2].parent_path(), Some("Payments"));
    }
}
