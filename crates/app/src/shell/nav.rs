//! Sidebar sections (principles.md §2: Today, Inbox, Pull requests, Work
//! items, Pipelines, then Search) and their shortcuts.

use adoken_ui::IconName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nav {
    Today,
    Inbox,
    PullRequests,
    WorkItems,
    Pipelines,
    Search,
}

impl Nav {
    /// Fixed sidebar order.
    pub const ALL: [Nav; 6] = [
        Nav::Today,
        Nav::Inbox,
        Nav::PullRequests,
        Nav::WorkItems,
        Nav::Pipelines,
        Nav::Search,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Nav::Today => "Today",
            Nav::Inbox => "Inbox",
            Nav::PullRequests => "Pull requests",
            Nav::WorkItems => "Work items",
            Nav::Pipelines => "Pipelines",
            Nav::Search => "Search",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Nav::Today => IconName::Sun,
            Nav::Inbox => IconName::Inbox,
            Nav::PullRequests => IconName::GitPullRequest,
            Nav::WorkItems => IconName::ListTodo,
            Nav::Pipelines => IconName::Workflow,
            Nav::Search => IconName::Search,
        }
    }

    /// Keybinding: ⌘1…⌘6 in sidebar order (spec 0025 FR-5).
    pub fn keys(self) -> &'static str {
        match self {
            Nav::Today => "secondary-1",
            Nav::Inbox => "secondary-2",
            Nav::PullRequests => "secondary-3",
            Nav::WorkItems => "secondary-4",
            Nav::Pipelines => "secondary-5",
            Nav::Search => "secondary-6",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Nav::Today => "nav-today",
            Nav::Inbox => "nav-inbox",
            Nav::PullRequests => "nav-pull-requests",
            Nav::WorkItems => "nav-work-items",
            Nav::Pipelines => "nav-pipelines",
            Nav::Search => "nav-search",
        }
    }

    /// The section's id for per-view background settings (tokens.md §10.1).
    pub fn view_id(self) -> adoken_theme::background::ViewId {
        use adoken_theme::background::ViewId;
        match self {
            Nav::Today => ViewId::MyWork,
            Nav::Inbox => ViewId::Inbox,
            Nav::PullRequests => ViewId::PullRequests,
            Nav::WorkItems => ViewId::WorkItems,
            Nav::Pipelines => ViewId::Pipelines,
            Nav::Search => ViewId::SearchResults,
        }
    }

    /// `ADOKEN_NAV` values: `today`, `inbox`, `prs`, `workitems`,
    /// `pipelines`, `search`.
    pub fn parse(value: &str) -> Option<Nav> {
        Some(match value.to_ascii_lowercase().as_str() {
            "today" => Nav::Today,
            "inbox" => Nav::Inbox,
            "prs" | "pull-requests" | "pull_requests" => Nav::PullRequests,
            "workitems" | "work-items" | "work_items" => Nav::WorkItems,
            "pipelines" => Nav::Pipelines,
            "search" => Nav::Search,
            _ => return None,
        })
    }

    /// Next/previous section, wrapping (sidebar ↑/↓).
    pub fn step(self, delta: isize) -> Nav {
        let ix = Nav::ALL.iter().position(|n| *n == self).unwrap_or(0) as isize;
        let len = Nav::ALL.len() as isize;
        Nav::ALL[(ix + delta).rem_euclid(len) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_sections() {
        assert_eq!(Nav::parse("Inbox"), Some(Nav::Inbox));
        assert_eq!(Nav::parse("prs"), Some(Nav::PullRequests));
        assert_eq!(Nav::parse("workitems"), Some(Nav::WorkItems));
        assert_eq!(Nav::parse("pipelines"), Some(Nav::Pipelines));
        assert_eq!(Nav::parse("nope"), None);
    }

    #[test]
    fn steps_wrap_in_sidebar_order() {
        assert_eq!(Nav::Today.step(1), Nav::Inbox);
        assert_eq!(Nav::Today.step(-1), Nav::Search);
        assert_eq!(Nav::Search.step(1), Nav::Today);
    }

    #[test]
    fn shortcuts_are_distinct_and_parse() {
        let mut keys: Vec<_> = Nav::ALL.iter().map(|n| n.keys()).collect();
        for k in &keys {
            assert!(adoken_ui::Kbd::parse(k).is_ok(), "{k}");
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Nav::ALL.len());
    }

    /// Spec 0025 AC-4: ⌘1…⌘6 follow the sidebar (⌘4 Work items, ⌘5
    /// Pipelines, ⌘6 Search); ⌘\ does nothing and ⌘B toggles the sidebar.
    #[test]
    fn ac4_section_keys_follow_the_sidebar_and_only_cmd_b_toggles_it() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(crate::key_bindings());
        let workspace = [KeyContext::parse("Workspace").expect("context")];
        let resolve = |keys: &str| {
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, &workspace);
            bindings.first().map(|binding| binding.action().name())
        };
        let sections = [
            super::super::ShowToday.name(),
            super::super::ShowInbox.name(),
            super::super::ShowPullRequests.name(),
            super::super::ShowWorkItems.name(),
            super::super::ShowPipelines.name(),
            super::super::ShowSearch.name(),
        ];
        for (digit, (nav, action)) in Nav::ALL.iter().zip(sections).enumerate() {
            let keys = format!("secondary-{}", digit + 1);
            assert_eq!(nav.keys(), keys, "{nav:?}");
            assert_eq!(resolve(&keys), Some(action), "{keys}");
        }
        assert_eq!(resolve("secondary-\\"), None);
        assert_eq!(
            resolve("secondary-b"),
            Some(super::super::ToggleSidebar.name())
        );
    }
}
