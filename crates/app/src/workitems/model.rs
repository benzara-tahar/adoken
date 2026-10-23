//! Pure view model for the Work items view (spec 0008 FR-2): tabs, state
//! groups, ordering, area / iteration path matching ("the node and below",
//! FR-8) and path cycling. No gpui, no IO.

use std::collections::HashMap;

use adoken_model::time::DAY;
use adoken_model::{Millis, StateCategory, WorkItem};

/// Recently changed = changed within this window.
pub const RECENT_WINDOW: Millis = 7 * DAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Open items assigned to, created by, or last changed by me.
    MyWork,
    /// Assigned to me, any state.
    Assigned,
    /// Any item changed within [`RECENT_WINDOW`].
    Recent,
    /// Open items under one area path.
    Area,
    /// Open items under one iteration path.
    Iteration,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::MyWork,
        Tab::Assigned,
        Tab::Recent,
        Tab::Area,
        Tab::Iteration,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::MyWork => "My work",
            Tab::Assigned => "Assigned",
            Tab::Recent => "Recently changed",
            Tab::Area => "By area",
            Tab::Iteration => "By iteration",
        }
    }

    /// Next/previous tab, wrapping (`←` / `→`).
    pub fn step(self, delta: isize) -> Tab {
        let ix = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) as isize;
        let n = Tab::ALL.len() as isize;
        Tab::ALL[(ix + delta).rem_euclid(n) as usize]
    }

    /// Tabs scoped to one path (the `[` / `]` selector).
    pub fn has_path(self) -> bool {
        matches!(self, Tab::Area | Tab::Iteration)
    }
}

/// The list's tab and chosen paths, as navigation history captures them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListFilter {
    pub tab: Tab,
    pub area: Option<String>,
    pub iteration: Option<String>,
}

/// One line of the grouped list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A state group: state name as ADO spells it, item count.
    Header { state: String, count: usize },
    /// Index into the loaded items.
    Item(usize),
}

/// Group order: work in flight first, then the backlog, then finished.
pub fn category_rank(category: StateCategory) -> u8 {
    match category {
        StateCategory::InProgress => 0,
        StateCategory::Proposed => 1,
        StateCategory::Resolved => 2,
        StateCategory::Completed => 3,
        StateCategory::Removed => 4,
    }
}

/// The path a path tab groups by.
pub fn path_of(item: &WorkItem, tab: Tab) -> &str {
    match tab {
        Tab::Iteration => &item.iteration_path,
        _ => &item.area_path,
    }
}

/// Is `path` the node `node` or below it? Segments compare
/// case-insensitively, as Azure DevOps compares paths (FR-8, Q-3).
pub fn under(path: &str, node: &str) -> bool {
    let mut path = path.trim_matches('\\').split('\\');
    node.trim_matches('\\').split('\\').all(|segment| {
        path.next().is_some_and(|p| {
            p.chars()
                .flat_map(char::to_lowercase)
                .eq(segment.chars().flat_map(char::to_lowercase))
        })
    })
}

/// Does `item` belong in `tab`? `path` scopes the Area / Iteration tabs to
/// that node and below (`None` there matches nothing).
pub fn matches(item: &WorkItem, tab: Tab, me: &str, now: Millis, path: Option<&str>) -> bool {
    match tab {
        Tab::MyWork => {
            item.is_open()
                && (item.assigned_to.as_deref() == Some(me)
                    || item.created_by == me
                    || item.changed_by == me)
        }
        Tab::Assigned => item.assigned_to.as_deref() == Some(me),
        Tab::Recent => now - item.changed <= RECENT_WINDOW,
        Tab::Area | Tab::Iteration => {
            item.is_open() && path.is_some_and(|node| under(path_of(item, tab), node))
        }
    }
}

/// Number of items `tab` lists (tab label counts).
pub fn count(items: &[WorkItem], tab: Tab, me: &str, now: Millis, path: Option<&str>) -> usize {
    items
        .iter()
        .filter(|item| matches(item, tab, me, now, path))
        .count()
}

/// The tab's items grouped by state (in-progress states first, then
/// proposed, resolved, done, removed; same-category states by name). Within
/// a group: priority ascending (unset last), then last change newest first;
/// Recently changed orders by last change only.
pub fn rows(items: &[WorkItem], tab: Tab, me: &str, now: Millis, path: Option<&str>) -> Vec<Row> {
    let mut picked: Vec<usize> = (0..items.len())
        .filter(|&ix| matches(&items[ix], tab, me, now, path))
        .collect();
    picked.sort_by(|&a, &b| {
        let (a, b) = (&items[a], &items[b]);
        let group = category_rank(a.state_category)
            .cmp(&category_rank(b.state_category))
            .then_with(|| a.state.cmp(&b.state));
        let priority = if tab == Tab::Recent {
            std::cmp::Ordering::Equal
        } else {
            a.priority
                .unwrap_or(u8::MAX)
                .cmp(&b.priority.unwrap_or(u8::MAX))
        };
        group
            .then(priority)
            .then_with(|| b.changed.cmp(&a.changed))
            .then_with(|| b.id.cmp(&a.id))
    });
    let mut out = Vec::with_capacity(picked.len() + 8);
    let mut start = 0;
    while start < picked.len() {
        let state = &items[picked[start]].state;
        let end = picked[start..]
            .iter()
            .position(|&ix| items[ix].state != *state)
            .map_or(picked.len(), |n| start + n);
        out.push(Row::Header {
            state: state.clone(),
            count: end - start,
        });
        out.extend(picked[start..end].iter().map(|&ix| Row::Item(ix)));
        start = end;
    }
    out
}

/// Distinct paths of open items for a path tab, most items first, then by
/// name.
pub fn paths(items: &[WorkItem], tab: Tab) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in items.iter().filter(|item| item.is_open()) {
        *counts.entry(path_of(item, tab)).or_default() += 1;
    }
    let mut paths: Vec<(&str, usize)> = counts.into_iter().collect();
    paths.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    paths.into_iter().map(|(p, _)| p.to_owned()).collect()
}

/// Starting path: the one of my most recently changed open item, else the
/// busiest path.
pub fn default_path(items: &[WorkItem], tab: Tab, me: &str, paths: &[String]) -> Option<String> {
    items
        .iter()
        .filter(|item| item.is_open() && item.assigned_to.as_deref() == Some(me))
        .max_by_key(|item| (item.changed, item.id))
        .map(|item| path_of(item, tab).to_owned())
        .or_else(|| paths.first().cloned())
}

/// The path `delta` steps from `current`, wrapping (`[` / `]`). An unknown
/// `current` starts from the first path.
pub fn cycle(paths: &[String], current: Option<&str>, delta: isize) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let n = paths.len() as isize;
    let next = match current.and_then(|c| paths.iter().position(|p| p == c)) {
        Some(ix) => (ix as isize + delta).rem_euclid(n),
        None => 0,
    };
    Some(paths[next as usize].clone())
}

/// The selectable row `delta` items away from `current`, skipping headers
/// and clamping at the ends. `None` current starts at the first item.
pub fn step(rows: &[Row], current: Option<usize>, delta: isize) -> Option<usize> {
    let items: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, Row::Item(_)))
        .map(|(ix, _)| ix)
        .collect();
    if items.is_empty() {
        return None;
    }
    let Some(current) = current else {
        return Some(items[0]);
    };
    let pos = items
        .iter()
        .position(|&ix| ix >= current)
        .unwrap_or(items.len() - 1) as isize;
    let next = (pos + delta).clamp(0, items.len() as isize - 1);
    Some(items[next as usize])
}

/// Row showing item `id`, if listed.
pub fn row_of(rows: &[Row], items: &[WorkItem], id: i64) -> Option<usize> {
    rows.iter()
        .position(|row| matches!(row, Row::Item(ix) if items[*ix].id == id))
}

/// Last segment of an ADO path (`Payments\Sprint 42` → `Sprint 42`).
pub fn leaf(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::WorkItemType;

    const ME: &str = "me";
    const NOW: Millis = 1_000 * DAY;

    fn item(id: i64, state: &str, priority: Option<u8>, changed_days_ago: i64) -> WorkItem {
        WorkItem {
            id,
            project_id: "p".into(),
            rev: 1,
            kind: WorkItemType::Task,
            title: format!("item {id}"),
            state: state.into(),
            state_category: StateCategory::of_state(state),
            assigned_to: Some(ME.into()),
            created_by: "someone".into(),
            changed_by: "someone".into(),
            area_path: "Payments\\Checkout".into(),
            iteration_path: "Payments\\Sprint 1".into(),
            priority,
            tags: Vec::new(),
            parent_id: None,
            linked_pr_ids: Vec::new(),
            description: None,
            comment_count: 0,
            created: NOW - 400 * DAY,
            changed: NOW - changed_days_ago * DAY,
        }
    }

    fn ids(items: &[WorkItem], rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Header { state, count } => format!("[{state} {count}]"),
                Row::Item(ix) => items[*ix].id.to_string(),
            })
            .collect()
    }

    #[test]
    fn my_work_is_open_items_i_own_touch_or_filed_grouped_in_progress_first() {
        let mut items = vec![
            item(1, "New", Some(2), 1),
            item(2, "Active", Some(2), 1),
            item(3, "Closed", Some(1), 1),
            item(4, "Active", Some(1), 1),
            item(5, "Resolved", Some(1), 1),
            item(6, "New", Some(1), 1),
            item(7, "New", Some(1), 2),
            item(8, "Doing", Some(1), 2),
        ];
        // Someone else's item I filed / touched still counts; untouched doesn't.
        items[5].assigned_to = Some("them".into());
        items[5].created_by = ME.into();
        items[6].assigned_to = Some("them".into());
        items[7].assigned_to = None;
        items[7].changed_by = ME.into();
        let rows = rows(&items, Tab::MyWork, ME, NOW, None);
        assert_eq!(
            ids(&items, &rows),
            [
                "[Active 2]",
                "4",
                "2",
                "[Doing 1]",
                "8",
                "[New 2]",
                "6",
                "1"
            ]
        );
        assert_eq!(count(&items, Tab::MyWork, ME, NOW, None), 5);
    }

    #[test]
    fn groups_sort_by_priority_unset_last_then_newest_change() {
        let items = vec![
            item(1, "Active", None, 1),
            item(2, "Active", Some(3), 5),
            item(3, "Active", Some(1), 9),
            item(4, "Active", Some(3), 2),
            item(5, "Active", Some(1), 3),
        ];
        let rows = rows(&items, Tab::Assigned, ME, NOW, None);
        assert_eq!(ids(&items, &rows), ["[Active 5]", "5", "3", "4", "2", "1"]);
    }

    #[test]
    fn assigned_keeps_every_state_and_orders_finished_groups_last() {
        let mut items = vec![
            item(1, "Removed", Some(1), 1),
            item(2, "Closed", Some(1), 1),
            item(3, "Resolved", Some(1), 1),
            item(4, "New", Some(1), 1),
            item(5, "Active", Some(1), 1),
        ];
        items.push({
            let mut theirs = item(6, "Active", Some(1), 1);
            theirs.assigned_to = Some("them".into());
            theirs
        });
        let rows = rows(&items, Tab::Assigned, ME, NOW, None);
        assert_eq!(
            ids(&items, &rows),
            [
                "[Active 1]",
                "5",
                "[New 1]",
                "4",
                "[Resolved 1]",
                "3",
                "[Closed 1]",
                "2",
                "[Removed 1]",
                "1"
            ]
        );
    }

    #[test]
    fn recently_changed_is_anyones_last_week_by_change_time_only() {
        let mut items = vec![
            item(1, "Active", Some(1), 6),
            item(2, "Active", Some(4), 1),
            item(3, "Active", Some(1), 8),
            item(4, "Closed", Some(1), 0),
        ];
        items[1].assigned_to = Some("them".into());
        let rows = rows(&items, Tab::Recent, ME, NOW, None);
        assert_eq!(
            ids(&items, &rows),
            ["[Active 2]", "2", "1", "[Closed 1]", "4"]
        );
        // The window boundary is inclusive.
        let edge = [item(9, "Active", None, 7)];
        assert_eq!(count(&edge, Tab::Recent, ME, NOW, None), 1);
    }

    /// AC-3: a path tab lists the open items on the node or below it, not
    /// a sibling sharing its prefix; paths compare case-insensitively.
    #[test]
    fn path_tabs_list_open_items_at_or_below_the_node() {
        let mut items = vec![
            item(1, "Active", Some(1), 1),
            item(2, "Active", Some(1), 1),
            item(3, "Closed", Some(1), 1),
            item(4, "New", Some(1), 1),
            item(5, "Active", Some(1), 1),
            item(6, "Active", Some(1), 1),
        ];
        items[1].area_path = "Payments\\Checkout\\Tax".into();
        items[2].area_path = "Payments\\Checkout\\Tax".into();
        items[3].assigned_to = None;
        items[3].iteration_path = "Payments\\Sprint 2".into();
        items[4].area_path = "Payments\\CheckoutLegacy".into();
        items[5].area_path = "Payments".into();
        let area = Some("payments\\CHECKOUT");
        assert_eq!(
            ids(&items, &rows(&items, Tab::Area, ME, NOW, area)),
            ["[Active 2]", "2", "1", "[New 1]", "4"]
        );
        let leaf = Some("Payments\\Checkout\\Tax");
        assert_eq!(
            ids(&items, &rows(&items, Tab::Area, ME, NOW, leaf)),
            ["[Active 1]", "2"]
        );
        let sprint = Some("Payments\\Sprint 2");
        assert_eq!(
            ids(&items, &rows(&items, Tab::Iteration, ME, NOW, sprint)),
            ["[New 1]", "4"]
        );
        assert!(rows(&items, Tab::Area, ME, NOW, None).is_empty());
        assert!(under("A\\B\\C", "A\\B") && under("A\\B", "A\\B"));
        assert!(!under("A", "A\\B") && !under("A\\BC", "A\\B"));
    }

    /// AC-13: `[` / `]` step through the paths with open items, most items
    /// first; from a chosen parent that isn't one of them, `]` selects the
    /// first.
    #[test]
    fn cycling_from_a_chosen_parent_starts_at_the_busiest_path() {
        let mut items = vec![
            item(1, "Active", None, 1),
            item(2, "Active", None, 1),
            item(3, "Active", None, 1),
        ];
        items[0].area_path = "P\\Web".into();
        items[1].area_path = "P\\Infra\\Queues".into();
        items[2].area_path = "P\\Infra\\Queues".into();
        let paths = paths(&items, Tab::Area);
        assert_eq!(paths, ["P\\Infra\\Queues", "P\\Web"]);
        assert_eq!(
            cycle(&paths, Some("P\\Infra"), 1).as_deref(),
            Some("P\\Infra\\Queues")
        );
        assert_eq!(
            cycle(&paths, Some("P\\Web"), 1).as_deref(),
            Some("P\\Infra\\Queues")
        );
    }

    #[test]
    fn paths_rank_by_open_items_then_name_and_cycle_wraps() {
        let mut items = vec![
            item(1, "Active", None, 1),
            item(2, "Active", None, 1),
            item(3, "Active", None, 1),
            item(4, "Closed", None, 1),
            item(5, "Closed", None, 1),
        ];
        items[0].area_path = "B".into();
        items[1].area_path = "A".into();
        items[2].area_path = "A".into();
        items[3].area_path = "D".into();
        items[4].area_path = "D".into();
        let paths = paths(&items, Tab::Area);
        assert_eq!(paths, ["A", "B"], "closed-only paths are left out");
        assert_eq!(cycle(&paths, Some("B"), 1).as_deref(), Some("A"));
        assert_eq!(cycle(&paths, Some("A"), -1).as_deref(), Some("B"));
        assert_eq!(cycle(&paths, Some("gone"), 1).as_deref(), Some("A"));
        assert_eq!(cycle(&[], Some("A"), 1), None);
    }

    #[test]
    fn default_path_follows_my_latest_open_item() {
        let mut items = vec![item(1, "Active", None, 5), item(2, "Active", None, 1)];
        items[1].iteration_path = "Payments\\Sprint 9".into();
        let paths = paths(&items, Tab::Iteration);
        assert_eq!(
            default_path(&items, Tab::Iteration, ME, &paths).as_deref(),
            Some("Payments\\Sprint 9")
        );
        assert_eq!(
            default_path(&items, Tab::Iteration, "nobody", &paths).as_deref(),
            Some(paths[0].as_str())
        );
    }

    #[test]
    fn selection_skips_headers_and_clamps() {
        let items = vec![
            item(1, "Active", Some(1), 1),
            item(2, "New", Some(1), 1),
            item(3, "New", Some(2), 1),
        ];
        let rows = rows(&items, Tab::Assigned, ME, NOW, None);
        // [Active] 1 [New] 2 3
        assert_eq!(step(&rows, None, 1), Some(1));
        assert_eq!(step(&rows, Some(1), 1), Some(3));
        assert_eq!(step(&rows, Some(3), 1), Some(4));
        assert_eq!(step(&rows, Some(4), 1), Some(4));
        assert_eq!(step(&rows, Some(3), -1), Some(1));
        assert_eq!(step(&rows, Some(1), -1), Some(1));
        assert_eq!(step(&[], None, 1), None);
        assert_eq!(row_of(&rows, &items, 3), Some(4));
    }

    #[test]
    fn tabs_step_wraps_both_ways() {
        assert_eq!(Tab::MyWork.step(-1), Tab::Iteration);
        assert_eq!(Tab::Iteration.step(1), Tab::MyWork);
        assert_eq!(Tab::Assigned.step(1), Tab::Recent);
    }
}
