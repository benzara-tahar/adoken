//! The area and iteration filter dropdown of the work item list (spec 0008
//! FR-8): the path label on By area / By iteration opens the tree picker
//! (`adoken_ui::picker::TreePicker`, spec 0015 FR-14) over every area or
//! iteration of the listed projects, with per-node counts of loaded open
//! items.
//!
//! - [`Seen`] gathers, per project with loaded items, the paths of its open
//!   items and their counts; [`PathTree::build`] merges them into each
//!   project's stored classification tree (`wi_class_tree`, 0015 FR-33).
//!   Both run on a background executor after each list load and each tree
//!   arrival, so opening the dropdown only clones a prebuilt tree.
//! - [`due`] picks the projects whose stored tree is missing or 24 h old;
//!   the view reads them through the org's `WiWriter` at user priority,
//!   attributed "You: filter by area" / "You: filter by iteration", and
//!   merges each arrival into the open picker in place.
//! - Choosing a node filters the tab to open items at or below it
//!   ([`super::model::under`], Q-3).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use adoken_model::time::DAY;
use adoken_model::{ClassNode, ClassTree, Millis, WorkItem};
use adoken_store::Store;
use adoken_sync::attribution::UserAction;
use adoken_sync::wi::is_stale;
use adoken_theme::{size, space, typography};
use adoken_ui::action_menu::BoundsSlot;
use adoken_ui::picker::{
    DayRange, PickerSlot, Placement, TreeEvent, TreeKind, TreeModel, TreeNode, TreePicker,
};
use adoken_ui::{Icon, IconName, Kbd, Theme, TypeStyleExt};
use gpui::{Context, ScrollStrategy, SharedString, Task, Window, canvas, div, prelude::*, px};
use gpui_tokio::Tokio;

use super::data::{LIST_LIMIT, ListData};
use super::model::{self, Tab};
use super::writes::{self, WiEdits};
use super::{NEXT_PATH_KEYS, PREVIOUS_PATH_KEYS, WorkItemsView};

/// The dropdown's footer when what it shows comes only from loaded items.
pub const FROM_SYNCED_ITEMS: &str = "Showing paths from synced items";

// ── node builder (pure) ──────────────────────────────────────────────────

/// One project with loaded work items and the paths of its open ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectPaths {
    pub id: String,
    pub name: String,
    /// (path, open items on exactly that path), sorted by segment.
    pub areas: Vec<(String, u32)>,
    pub iterations: Vec<(String, u32)>,
}

/// What a list load says about paths (built off the UI thread).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// Projects with loaded items, by name.
    pub projects: Vec<ProjectPaths>,
    /// The list holds its newest-5,000 window: counts cover only that.
    pub window_full: bool,
}

impl Seen {
    /// `names`: project id → name (a project without one uses its id).
    pub fn gather(items: &[WorkItem], names: &HashMap<String, String>, window_full: bool) -> Self {
        type Counts<'a> = HashMap<&'a str, u32>;
        let mut by_project: HashMap<&str, (Counts, Counts)> = HashMap::new();
        for item in items {
            let (areas, iterations) = by_project.entry(item.project_id.as_str()).or_default();
            if !item.is_open() {
                continue;
            }
            for (counts, path) in [
                (&mut *areas, item.area_path.as_str()),
                (&mut *iterations, item.iteration_path.as_str()),
            ] {
                if !path.trim_matches('\\').is_empty() {
                    *counts.entry(path).or_default() += 1;
                }
            }
        }
        let sorted = |counts: Counts| {
            let mut paths: Vec<(String, u32)> =
                counts.into_iter().map(|(p, n)| (p.to_owned(), n)).collect();
            paths.sort_by_cached_key(|(p, _)| {
                p.split('\\').map(str::to_lowercase).collect::<Vec<_>>()
            });
            paths
        };
        let mut projects: Vec<ProjectPaths> = by_project
            .into_iter()
            .map(|(id, (areas, iterations))| ProjectPaths {
                id: id.to_owned(),
                name: names.get(id).cloned().unwrap_or_else(|| id.to_owned()),
                areas: sorted(areas),
                iterations: sorted(iterations),
            })
            .collect();
        projects.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        Self {
            projects,
            window_full,
        }
    }

    fn paths(project: &ProjectPaths, tab: Tab) -> &[(String, u32)] {
        match tab {
            Tab::Iteration => &project.iterations,
            _ => &project.areas,
        }
    }
}

fn class_nodes(tree: &ClassTree, tab: Tab) -> &[ClassNode] {
    match tab {
        Tab::Iteration => &tree.iterations,
        _ => &tree.areas,
    }
}

/// Nodes keyed by lowercased path (Azure DevOps compares paths
/// case-insensitively), parents always before children.
#[derive(Default)]
struct Builder {
    nodes: Vec<TreeNode>,
    index: HashMap<String, usize>,
    direct: Vec<u32>,
}

impl Builder {
    /// The node at `path`, created with any missing ancestors.
    fn add(&mut self, path: &str) -> Option<usize> {
        let path = path.trim_matches('\\');
        if path.is_empty() {
            return None;
        }
        let key = path.to_lowercase();
        if let Some(&ix) = self.index.get(&key) {
            return Some(ix);
        }
        let (parent, name) = match path.rsplit_once('\\') {
            Some((parent, name)) => (self.add(parent), name),
            None => (None, path),
        };
        if name.is_empty() {
            return parent;
        }
        let ix = self.nodes.len();
        self.nodes.push(TreeNode::new(name.to_owned(), parent));
        self.direct.push(0);
        self.index.insert(key, ix);
        Some(ix)
    }
}

/// The dropdown's tree for one tab, prebuilt off the UI thread.
#[derive(Clone, Debug, Default)]
pub struct PathTree {
    pub model: TreeModel,
    /// Loaded open items at or below each node.
    pub counts: Vec<u32>,
    /// Lowercased path → node.
    index: HashMap<String, usize>,
    /// Listed projects without a stored tree (loaded paths only).
    pub partial: Vec<String>,
}

impl PathTree {
    /// One root per listed project: its stored tree, merged with every path
    /// seen on a loaded open item (so no loaded item is unreachable and
    /// nodes added since the fetch still show); a project without a stored
    /// tree shows its loaded paths only (FR-8 Contents, Counts).
    pub fn build(tab: Tab, seen: &Seen, stored: &HashMap<String, Arc<ClassTree>>) -> Self {
        let mut b = Builder::default();
        let mut partial = Vec::new();
        let day = |ms: Millis| ms.div_euclid(DAY);
        for project in &seen.projects {
            let tree = stored.get(&project.id).map(|t| class_nodes(t, tab));
            match tree {
                Some(nodes) => {
                    for node in nodes {
                        if let Some(ix) = b.add(&node.path)
                            && let (Some(start), Some(finish)) = (node.start_ms, node.finish_ms)
                        {
                            b.nodes[ix].dates = Some(DayRange {
                                start: day(start),
                                finish: day(finish),
                            });
                        }
                    }
                }
                None => partial.push(project.id.clone()),
            }
            let paths = Seen::paths(project, tab);
            if paths.is_empty() && tree.is_none_or(<[ClassNode]>::is_empty) {
                b.add(&project.name);
            }
            for (path, n) in paths {
                if let Some(ix) = b.add(path) {
                    b.direct[ix] += n;
                }
            }
        }
        let mut counts = b.direct;
        // Children come after their parents: one backwards pass sums them up.
        for ix in (0..b.nodes.len()).rev() {
            if let Some(parent) = b.nodes[ix].parent {
                let below = counts[ix];
                counts[parent] += below;
            }
        }
        Self {
            model: TreeModel::new(b.nodes),
            counts,
            index: b.index,
            partial,
        }
    }

    /// The node at `path`, compared case-insensitively.
    pub fn find(&self, path: &str) -> Option<usize> {
        self.index
            .get(&path.trim_matches('\\').to_lowercase())
            .copied()
    }

    /// Whether `path` has nodes below it ("and below" in the label).
    pub fn has_children(&self, path: &str) -> bool {
        self.find(path).is_some_and(|n| self.model.has_children(n))
    }
}

/// The listed projects to read a tree for on this open (FR-8 Fetching):
/// stored tree missing or ≥ 24 h old (0015 FR-33), not already in flight.
pub fn due(
    seen: &Seen,
    stored: &HashMap<String, Arc<ClassTree>>,
    in_flight: &HashSet<String>,
    now: Millis,
) -> Vec<String> {
    seen.projects
        .iter()
        .filter(|p| !in_flight.contains(&p.id))
        .filter(|p| {
            stored
                .get(&p.id)
                .is_none_or(|tree| is_stale(tree.fetched_ms, now))
        })
        .map(|p| p.id.clone())
        .collect()
}

/// The dropdown's quiet footer line: a tree on its way, or paths from
/// loaded items only (demo, offline, a failed read), and the 5,000 window.
pub fn note(tab: Tab, partial: bool, loading: bool, window_full: bool) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if loading {
        parts.push(match tab {
            Tab::Iteration => "Loading all iterations…".into(),
            _ => "Loading all areas…".into(),
        });
    } else if partial {
        parts.push(FROM_SYNCED_ITEMS.into());
    }
    if window_full {
        parts.push(format!(
            "Counts cover the newest {} synced items",
            thousands(LIST_LIMIT)
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (ix, ch) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Both tabs' trees.
#[derive(Clone, Debug, Default)]
pub struct Trees {
    pub area: Arc<PathTree>,
    pub iteration: Arc<PathTree>,
}

impl Trees {
    pub fn build(seen: &Seen, stored: &HashMap<String, Arc<ClassTree>>) -> Self {
        Self {
            area: Arc::new(PathTree::build(Tab::Area, seen, stored)),
            iteration: Arc::new(PathTree::build(Tab::Iteration, seen, stored)),
        }
    }

    fn of(&self, tab: Tab) -> &Arc<PathTree> {
        match tab {
            Tab::Iteration => &self.iteration,
            _ => &self.area,
        }
    }
}

/// What a list load prepares for the dropdown (blocking: background only).
pub struct Prepared {
    pub seen: Arc<Seen>,
    pub stored: HashMap<String, Arc<ClassTree>>,
    pub trees: Trees,
}

impl Prepared {
    pub fn read(store: &Store, data: &ListData) -> Self {
        let seen = Seen::gather(&data.items, &data.projects, data.items.len() >= LIST_LIMIT);
        let mut stored = HashMap::new();
        for project in &seen.projects {
            match store.with_conn(|c| adoken_store::wi::class_tree(c, &project.id)) {
                Ok(Some(tree)) => {
                    stored.insert(project.id.clone(), Arc::new(tree));
                }
                Ok(None) => {}
                Err(error) => log::warn!("work items: couldn't read a stored tree: {error}"),
            }
        }
        let trees = Trees::build(&seen, &stored);
        Self {
            seen: Arc::new(seen),
            stored,
            trees,
        }
    }
}

// ── the view's dropdown ──────────────────────────────────────────────────

/// The view's dropdown state.
#[derive(Default)]
pub(crate) struct PathFilter {
    seen: Arc<Seen>,
    stored: HashMap<String, Arc<ClassTree>>,
    /// `None` until the first list load's build lands.
    trees: Option<Trees>,
    /// Projects whose tree read is in flight.
    fetching: HashSet<String>,
    picker: PickerSlot<TreePicker>,
    /// The tab the open picker filters, and whether a key opened it.
    open: Option<(Tab, bool)>,
    /// The path label's last painted bounds (the picker's anchor).
    anchor: BoundsSlot,
    _build: Option<Task<()>>,
    _events: Option<gpui::Subscription>,
}

impl PathFilter {
    pub fn element(&self) -> Option<gpui::AnyElement> {
        self.picker.element()
    }
}

impl WorkItemsView {
    /// A list load's prepared paths (FR-8 Building).
    pub(super) fn set_paths(&mut self, prepared: Prepared, cx: &mut Context<Self>) {
        self.paths.seen = prepared.seen;
        self.paths.stored = prepared.stored;
        self.paths.trees = Some(prepared.trees);
        self.refresh_path_picker(cx);
    }

    /// Opens the dropdown for `tab` (`/`, a click, the palette): switches to
    /// that tab first, keyboard in the search field, the current path
    /// expanded to and selected; reads missing or stale trees.
    pub fn open_path_filter(
        &mut self,
        tab: Tab,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !tab.has_path() {
            return;
        }
        if self.tab != tab {
            self.set_tab(tab, cx);
        }
        // The picker hands the keyboard back to whatever had it: the list.
        window.focus(&self.list_focus, cx);
        self.fetch_due_trees(tab, cx);
        let kind = match tab {
            Tab::Iteration => TreeKind::Iteration {
                today: self.now().div_euclid(DAY),
            },
            _ => TreeKind::Area,
        };
        let title = match tab {
            Tab::Iteration => "Filter by iteration",
            _ => "Filter by area",
        };
        let current = self.path().map(|p| SharedString::from(p.to_owned()));
        let placement = self.path_placement(keyboard, window);
        let picker =
            cx.new(|cx| TreePicker::new(kind, title, None, current, placement, window, cx));
        let events = cx.subscribe(&picker, move |this, _, event: &TreeEvent, cx| {
            let TreeEvent::Chosen(path) = event;
            this.choose_path(tab, path.to_string(), cx);
        });
        self.paths.open = Some((tab, keyboard));
        self.paths
            .picker
            .open(picker, |this: &mut Self| &mut this.paths.picker, cx);
        self.paths._events = Some(events);
        self.refresh_path_picker(cx);
    }

    fn path_placement(&self, keyboard: bool, window: &Window) -> Placement {
        let placement = match self.paths.anchor.get() {
            Some(bounds) => Placement::below(bounds, size::PICKER_POPOVER),
            // Not painted yet (opened from another view): the list's top;
            // the anchor moves it once the label paints.
            None => Placement::at(
                gpui::point(px(space::S12), px(size::TITLEBAR + 120.0)),
                gpui::Anchor::TopLeft,
                size::PICKER_POPOVER,
            ),
        };
        if keyboard || !window.is_window_active() {
            placement.keyboard()
        } else {
            placement
        }
    }

    /// Keeps the open picker under the label (called while rendering).
    pub(super) fn sync_path_placement(&self, window: &Window, cx: &mut Context<Self>) {
        let (Some(picker), Some((_, keyboard)), Some(_)) = (
            self.paths.picker.picker().cloned(),
            self.paths.open,
            self.paths.anchor.get(),
        ) else {
            return;
        };
        let placement = self.path_placement(keyboard, window);
        picker.update(cx, |picker, cx| picker.set_placement(placement, cx));
    }

    /// The open picker gets the latest tree, counts and footer.
    fn refresh_path_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.paths.picker.picker().cloned() else {
            self.paths.open = None;
            return;
        };
        let Some((tab, _)) = self.paths.open else {
            return;
        };
        let note = self.path_note(tab).map(SharedString::from);
        let tree = self.paths.trees.as_ref().map(|t| t.of(tab).clone());
        picker.update(cx, |picker, cx| {
            if let Some(tree) = tree {
                picker.set_model(tree.model.clone(), tree.counts.clone(), cx);
            }
            picker.set_note(note, cx);
        });
    }

    fn path_note(&self, tab: Tab) -> Option<String> {
        let partial = self
            .paths
            .trees
            .as_ref()
            .is_some_and(|t| !t.of(tab).partial.is_empty());
        let loading = self
            .paths
            .seen
            .projects
            .iter()
            .any(|p| self.paths.fetching.contains(&p.id));
        note(tab, partial, loading, self.paths.seen.window_full)
    }

    /// FR-8 Fetching: one read per listed project whose stored tree is
    /// missing or stale, at user priority; nothing in the demo org or
    /// offline. A failure keeps the loaded paths; the next open retries.
    fn fetch_due_trees(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let Some(writer) = writes::access(cx).writer() else {
            return;
        };
        if WiEdits::global(cx).is_some_and(|e| e.read(cx).offline()) {
            return;
        }
        let action = match tab {
            Tab::Iteration => UserAction::FilterByIteration,
            _ => UserAction::FilterByArea,
        };
        let now = self.now();
        for project in due(
            &self.paths.seen,
            &self.paths.stored,
            &self.paths.fetching,
            now,
        ) {
            self.paths.fetching.insert(project.clone());
            let writer = writer.clone();
            cx.spawn(async move |this, cx| {
                let p = project.clone();
                let tree = Tokio::spawn(cx, async move { writer.class_tree(&p, action).await })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()));
                this.update(cx, |this, cx| {
                    this.paths.fetching.remove(&project);
                    match tree {
                        Ok(tree) => this.tree_arrived(project, tree, cx),
                        Err(error) => {
                            log::warn!("work items: couldn't read a classification tree: {error}");
                            this.refresh_path_picker(cx);
                        }
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// A tree arrived: rebuild off the UI thread, then merge it into the
    /// open picker in place (query and selection stay).
    fn tree_arrived(&mut self, project: String, tree: ClassTree, cx: &mut Context<Self>) {
        self.paths.stored.insert(project, Arc::new(tree));
        let seen = self.paths.seen.clone();
        let stored = self.paths.stored.clone();
        self.paths._build = Some(cx.spawn(async move |this, cx| {
            let trees = cx
                .background_spawn(async move { Trees::build(&seen, &stored) })
                .await;
            this.update(cx, |this, cx| {
                this.paths.trees = Some(trees);
                this.refresh_path_picker(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// FR-8 Choosing: the tab lists open items at or below `path`, the
    /// list selects its first item; the choice lasts the session.
    fn choose_path(&mut self, tab: Tab, path: String, cx: &mut Context<Self>) {
        match tab {
            Tab::Iteration => self.iteration = Some(path),
            _ => self.area = Some(path),
        }
        self.tab = tab;
        self.refilter();
        self.selected = model::step(&self.rows, None, 0);
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.show_selected(cx);
        cx.notify();
    }

    /// The label: the full path, "and below" when the node has children.
    pub fn path_label(&self) -> Option<String> {
        let path = self.path()?;
        let below = self
            .paths
            .trees
            .as_ref()
            .is_some_and(|t| t.of(self.tab).has_children(path));
        Some(if below {
            format!("{path} and below")
        } else {
            path.to_owned()
        })
    }

    pub fn path_picker_open(&self) -> bool {
        self.paths.picker.is_open()
    }

    /// The open dropdown (smoke tests, stories).
    pub fn path_picker(&self) -> Option<gpui::Entity<TreePicker>> {
        self.paths.picker.picker().cloned()
    }

    /// The path row under the tabs: `‹` dropdown `›` and the `[` `]` keys.
    pub(super) fn render_path_picker(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = &theme.colors;
        let (paths, current) = match self.tab {
            Tab::Iteration => (&self.iteration_paths, self.iteration.as_deref()),
            _ => (&self.area_paths, self.area.as_deref()),
        };
        let position = current
            .and_then(|c| paths.iter().position(|p| p == c))
            .map_or(String::new(), |ix| format!("{} / {}", ix + 1, paths.len()));
        let chevron = |id: &'static str, icon: IconName, delta: isize| {
            div()
                .id(id)
                .flex_none()
                .size(px(size::HIT_MIN))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(adoken_theme::radius::SM))
                .cursor_pointer()
                .hover(|s| s.bg(colors.bg_hover))
                .on_click(cx.listener(move |this, _, _, cx| this.cycle_path(delta, cx)))
                .child(
                    Icon::new(icon)
                        .size(size::ICON_SM)
                        .color(colors.text_secondary),
                )
        };
        let label = self.path_label().unwrap_or_else(|| "Choose a path".into());
        let what = match self.tab {
            Tab::Iteration => "Iteration",
            _ => "Area",
        };
        let tab = self.tab;
        let open = self.paths.picker.is_open();
        let anchor = self.paths.anchor.clone();
        let view = cx.entity().downgrade();
        let button = div()
            .id("wi-path-dropdown")
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(size::HIT_MIN))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S4))
            .rounded(px(adoken_theme::radius::SM))
            .cursor_pointer()
            .when(open, |d| d.bg(colors.bg_active))
            .hover(|s| s.bg(colors.bg_hover))
            .role(gpui::Role::Button)
            .aria_label(format!("{what}: {label}"))
            .aria_expanded(open)
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    let keyboard = matches!(event, gpui::ClickEvent::Keyboard(_));
                    this.open_path_filter(tab, keyboard, window, cx);
                }),
            )
            .child(
                // Records the label's bounds; a move re-renders so an open
                // picker follows it.
                canvas(
                    move |bounds, _, cx| {
                        if anchor.get() != Some(bounds) {
                            anchor.set(Some(bounds));
                            view.update(cx, |_, cx| cx.notify()).ok();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY_STRONG, theme)
                    .child(label),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary),
            );
        div()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .child(chevron("wi-path-prev", IconName::ChevronLeft, -1))
            .child(button)
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(position),
            )
            .child(chevron("wi-path-next", IconName::ChevronRight, 1))
            .child(Kbd::parse(PREVIOUS_PATH_KEYS).expect("static binding"))
            .child(Kbd::parse(NEXT_PATH_KEYS).expect("static binding"))
    }
}

#[cfg(test)]
mod tests;

/// Gallery stories and smoke tests: a store with projects P1 (Storefront,
/// stored trees) and P2 (Payments, stored trees only when asked).
pub mod story {
    use adoken_model::time::HOUR;
    use adoken_model::{NodeKind, Person, Project, StateCategory, WorkItemType};
    use adoken_store::write;

    use super::*;

    pub const P1: &str = "p1";
    pub const P2: &str = "p2";
    const ME: &str = "me";

    /// A work item on `area` / `iteration`, open unless `state` says so.
    pub fn item(
        id: i64,
        project: &str,
        area: &str,
        iteration: &str,
        state: &str,
        changed: Millis,
    ) -> WorkItem {
        WorkItem {
            id,
            project_id: project.into(),
            rev: 1,
            kind: if id % 3 == 0 {
                WorkItemType::Bug
            } else {
                WorkItemType::Task
            },
            title: format!("Work item {id}"),
            state: state.into(),
            state_category: StateCategory::of_state(state),
            assigned_to: (id % 2 == 0).then(|| ME.into()),
            created_by: ME.into(),
            changed_by: ME.into(),
            area_path: area.into(),
            iteration_path: iteration.into(),
            priority: Some((id % 4 + 1) as u8),
            tags: Vec::new(),
            parent_id: None,
            linked_pr_ids: Vec::new(),
            description: None,
            comment_count: 0,
            created: changed - 30 * DAY,
            changed,
        }
    }

    fn node(kind: NodeKind, path: &str, dates: Option<(i64, i64)>) -> ClassNode {
        ClassNode {
            kind,
            path: path.into(),
            name: path.rsplit('\\').next().unwrap_or(path).into(),
            depth: path.matches('\\').count() as u16,
            start_ms: dates.map(|(s, _)| s * DAY),
            finish_ms: dates.map(|(_, f)| f * DAY),
        }
    }

    /// `root`'s areas (`children` under it, `\`-separated) and four
    /// two-week sprints numbered from `first`, the third current at `now`.
    pub fn tree(root: &str, areas: &[&str], first: u32, now: Millis) -> ClassTree {
        let today = now.div_euclid(DAY);
        let mut tree = ClassTree {
            areas: vec![node(NodeKind::Area, root, None)],
            iterations: vec![node(NodeKind::Iteration, root, None)],
            fetched_ms: now - 2 * HOUR,
        };
        for area in areas {
            tree.areas
                .push(node(NodeKind::Area, &format!("{root}\\{area}"), None));
        }
        for ix in 0..4u32 {
            let start = today - 4 + (i64::from(ix) - 2) * 14;
            tree.iterations.push(node(
                NodeKind::Iteration,
                &format!("{root}\\Sprint {}", first + ix),
                Some((start, start + 11)),
            ));
        }
        tree
    }

    pub const P1_AREAS: &[&str] = &[
        "Checkout",
        "Checkout\\Cart",
        "Checkout\\Payments",
        "Infra",
        "Infra\\Queues",
        "Infra\\Search",
        "Infra\\Storage",
        "Web",
        "Web\\Catalog",
        "Mobile",
    ];

    /// Seeds `dir`'s store: P1 with its trees and items under Infra and
    /// Checkout; with `two_projects`, P2 too, its trees stored when
    /// `p2_tree`. The clock is `now`.
    pub fn seed(dir: &std::path::Path, two_projects: bool, p2_tree: bool, now: Millis) -> Store {
        let store = Store::open(dir.join("adoken.sqlite")).expect("open the story store");
        let mut items = vec![
            item(
                101,
                P1,
                "Storefront\\Infra\\Queues",
                "Storefront\\Sprint 45",
                "Active",
                now - HOUR,
            ),
            item(
                102,
                P1,
                "Storefront\\Infra\\Queues",
                "Storefront\\Sprint 45",
                "New",
                now - 2 * HOUR,
            ),
            item(
                103,
                P1,
                "Storefront\\Infra\\Search",
                "Storefront\\Sprint 45",
                "Active",
                now - 3 * HOUR,
            ),
            item(
                104,
                P1,
                "Storefront\\Infra",
                "Storefront\\Sprint 44",
                "New",
                now - 4 * HOUR,
            ),
            item(
                105,
                P1,
                "Storefront\\Checkout\\Cart",
                "Storefront\\Sprint 45",
                "Active",
                now - 5 * HOUR,
            ),
            item(
                106,
                P1,
                "Storefront\\Checkout\\Payments",
                "Storefront\\Sprint 46",
                "New",
                now - 6 * HOUR,
            ),
            item(
                107,
                P1,
                "Storefront\\Checkout",
                "Storefront\\Sprint 45",
                "Closed",
                now - 7 * HOUR,
            ),
            // Added in Azure DevOps after the tree was stored.
            item(
                108,
                P1,
                "Storefront\\Infra\\Edge",
                "Storefront\\Sprint 45",
                "Active",
                now - 8 * HOUR,
            ),
        ];
        if two_projects {
            items.extend([
                item(
                    201,
                    P2,
                    "Payments\\Ledger",
                    "Payments\\Sprint 7",
                    "Active",
                    now - HOUR,
                ),
                item(
                    202,
                    P2,
                    "Payments\\Ledger",
                    "Payments\\Sprint 7",
                    "New",
                    now - 2 * HOUR,
                ),
                item(
                    203,
                    P2,
                    "Payments\\Payouts",
                    "Payments\\Sprint 8",
                    "Active",
                    now - 3 * HOUR,
                ),
                item(
                    204,
                    P2,
                    "Payments\\Infra",
                    "Payments\\Sprint 7",
                    "New",
                    now - 4 * HOUR,
                ),
            ]);
        }
        store
            .write(|tx| {
                write::upsert_person(
                    tx,
                    &Person {
                        id: ME.into(),
                        display_name: "Ada Me".into(),
                        unique_name: Some("ada@contoso.com".into()),
                        is_group: false,
                        image_url: None,
                    },
                    true,
                )?;
                write::upsert_project(
                    tx,
                    &Project {
                        id: P1.into(),
                        name: "Storefront".into(),
                    },
                )?;
                adoken_store::wi::put_class_tree(tx, P1, &tree("Storefront", P1_AREAS, 43, now))?;
                if two_projects {
                    write::upsert_project(
                        tx,
                        &Project {
                            id: P2.into(),
                            name: "Payments".into(),
                        },
                    )?;
                    if p2_tree {
                        adoken_store::wi::put_class_tree(tx, P2, &self::p2_tree(now))?;
                    }
                }
                for item in &items {
                    write::upsert_work_item(tx, item)?;
                }
                Ok(())
            })
            .expect("seed the story store");
        store
    }

    /// A view over [`seed`]'s store on `tab`, clock pinned at `now`, edits
    /// local (nothing is ever sent), `hook` run once the first item's
    /// detail has painted (the list and its paths are loaded by then).
    pub fn view(
        store: Store,
        tab: Tab,
        now: Millis,
        hook: Option<super::super::StoryHook>,
        window: &mut Window,
        cx: &mut Context<WorkItemsView>,
    ) -> WorkItemsView {
        writes::set_access(writes::Access::Local, cx);
        let edits = WiEdits::install(store.clone(), cx);
        edits.update(cx, |edits, cx| edits.set_clock(now, cx));
        let mut view = WorkItemsView::new(store, window, cx);
        view.clock = Some(now);
        view.tab = tab;
        view.story_hook = hook;
        view
    }

    /// P2's stored tree (the one a read would bring).
    pub fn p2_tree(now: Millis) -> ClassTree {
        tree("Payments", &["Ledger", "Payouts", "Infra", "Risk"], 5, now)
    }

    impl WorkItemsView {
        /// Open the dropdown once the list has loaded, `query` typed
        /// (stories).
        pub fn story_path_filter(
            &mut self,
            tab: Tab,
            query: &str,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.open_path_filter(tab, true, window, cx);
            if let Some(picker) = self.paths.picker.picker().cloned() {
                picker.update(cx, |picker, cx| picker.set_query(query, cx));
            }
        }

        /// Mark `project`'s tree read as in flight (the loading story).
        pub fn story_tree_loading(&mut self, project: &str, cx: &mut Context<Self>) {
            self.paths.fetching.insert(project.to_owned());
            self.refresh_path_picker(cx);
        }

        /// `project`'s tree read answered `tree` (smoke tests: the arrival
        /// merges into the open dropdown).
        pub fn story_tree_arrived(
            &mut self,
            project: &str,
            tree: ClassTree,
            cx: &mut Context<Self>,
        ) {
            self.paths.fetching.remove(project);
            self.tree_arrived(project.to_owned(), tree, cx);
        }

        /// The listed items' ids, top to bottom, and the selected one
        /// (smoke tests).
        pub fn listed(&self) -> (Vec<i64>, Option<i64>) {
            let ids = self
                .rows
                .iter()
                .filter_map(|row| match row {
                    model::Row::Item(ix) => self.list.items.get(*ix).map(|i| i.id),
                    model::Row::Header { .. } => None,
                })
                .collect();
            (ids, self.selected_id())
        }

        /// Whether the first list load's paths are built (stories wait).
        pub fn paths_ready(&self) -> bool {
            self.paths.trees.is_some()
        }
    }
}
