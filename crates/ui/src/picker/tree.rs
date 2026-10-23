//! Area / iteration tree picker (spec 0015 FR-14; design §4.5).
//!
//! [`TreeModel`] holds a classification tree as a flat node list and
//! answers the visible rows: unfiltered, the expanded branches; filtered, the
//! nodes whose **path** matches every query token (any segment may match any
//! token, so typing a parent's name lists its subtree) together with their
//! ancestors, which stay visible as context (AC-12: ≤ 5 ms on 10,000 nodes).
//! The iteration picker adds Current / Next / Recent shortcuts from node
//! dates above the tree.

use std::ops::Range;

use adoken_theme::{size, space, typography};
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Role,
    ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use super::{
    Choose, Close, LoadingGate, Placement, ReturnFocus, SelectNext, SelectPrevious, VISIBLE_ROWS,
    card, find_ci, footer_hint, group_label, highlighted, list_height, message_row, now_ms,
    overlay, row_frame, schedule, search_field,
};
use crate::icon::{Icon, IconName};
use crate::text_input::{self, TextInput, TextInputEvent};
use crate::theme::{ActiveTheme, TypeStyleExt};

/// Inclusive day range, in days since 1970-01-01 (iteration start and
/// finish dates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayRange {
    pub start: i64,
    pub finish: i64,
}

impl DayRange {
    pub fn contains(&self, day: i64) -> bool {
        self.start <= day && day <= self.finish
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub name: SharedString,
    /// Index of the parent node; parents come before their children.
    pub parent: Option<usize>,
    pub dates: Option<DayRange>,
}

impl TreeNode {
    pub fn new(name: impl Into<SharedString>, parent: Option<usize>) -> Self {
        Self {
            name: name.into(),
            parent,
            dates: None,
        }
    }

    pub fn dates(mut self, dates: DayRange) -> Self {
        self.dates = Some(dates);
        self
    }
}

/// Builds a tree from `\`-separated paths (FR-14: the paths seen in loaded
/// work items, until the classification tree arrives). First-seen order.
pub fn nodes_from_paths<S: AsRef<str>>(paths: &[S]) -> Vec<TreeNode> {
    let mut nodes: Vec<TreeNode> = Vec::new();
    for path in paths {
        let mut parent = None;
        for segment in path.as_ref().split('\\').filter(|s| !s.is_empty()) {
            let found = nodes
                .iter()
                .position(|n| n.parent == parent && n.name.as_ref() == segment);
            parent = Some(found.unwrap_or_else(|| {
                nodes.push(TreeNode::new(segment.to_owned(), parent));
                nodes.len() - 1
            }));
        }
    }
    nodes
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// (month 1–12, day 1–31) of a day number; Howard Hinnant's
/// `civil_from_days` (as `crates/ken/src/week.rs`).
fn month_day(days: i64) -> (usize, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    (month, day)
}

/// "Sep 29 – Oct 10"; `spoken`: "September 29 to October 10".
pub fn format_day_range(range: DayRange, spoken: bool) -> String {
    let (m1, d1) = month_day(range.start);
    let (m2, d2) = month_day(range.finish);
    if spoken {
        format!(
            "{} {d1} to {} {d2}",
            MONTHS_LONG[m1 - 1],
            MONTHS_LONG[m2 - 1]
        )
    } else {
        format!("{} {d1} \u{2013} {} {d2}", MONTHS[m1 - 1], MONTHS[m2 - 1])
    }
}

/// Iteration shortcut groups above the tree (FR-14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shortcut {
    /// Dates include today.
    Current,
    /// The next start date.
    Next,
    /// The last two finished.
    Recent,
}

impl Shortcut {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Next => "Next",
            Self::Recent => "Recent",
        }
    }
}

/// One visible tree row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub node: usize,
    pub depth: u16,
    pub has_children: bool,
    pub expanded: bool,
    /// False for ancestors kept only as context of a filtered match.
    pub matched: bool,
}

/// The tree and its expansion state.
#[derive(Clone, Debug, Default)]
pub struct TreeModel {
    nodes: Vec<TreeNode>,
    /// Lowercased names, for filtering.
    lower: Vec<String>,
    depth: Vec<u16>,
    /// Node indices, parents before children, siblings in input order.
    preorder: Vec<u32>,
    child_count: Vec<u32>,
    expanded: Vec<bool>,
    /// Filter scratch, reused across keystrokes.
    mask: Vec<u64>,
    keep: Vec<bool>,
}

impl TreeModel {
    /// A node whose parent index isn't before it is treated as a root.
    pub fn new(mut nodes: Vec<TreeNode>) -> Self {
        let n = nodes.len();
        for (ix, node) in nodes.iter_mut().enumerate() {
            if node.parent.is_some_and(|p| p >= ix) {
                node.parent = None;
            }
        }
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut roots = Vec::new();
        for (ix, node) in nodes.iter().enumerate() {
            match node.parent {
                Some(p) => children[p].push(ix as u32),
                None => roots.push(ix as u32),
            }
        }
        let mut depth = vec![0u16; n];
        for ix in 0..n {
            if let Some(p) = nodes[ix].parent {
                depth[ix] = depth[p] + 1;
            }
        }
        let mut preorder = Vec::with_capacity(n);
        let mut stack: Vec<u32> = roots.into_iter().rev().collect();
        while let Some(ix) = stack.pop() {
            preorder.push(ix);
            stack.extend(children[ix as usize].iter().rev());
        }
        let mut expanded = vec![false; n];
        for (ix, node) in nodes.iter().enumerate() {
            // Roots start open: the first level is always in view.
            if node.parent.is_none() {
                expanded[ix] = true;
            }
        }
        Self {
            lower: nodes.iter().map(|n| n.name.to_lowercase()).collect(),
            child_count: children.iter().map(|c| c.len() as u32).collect(),
            nodes,
            depth,
            preorder,
            expanded,
            mask: vec![0; n],
            keep: vec![false; n],
        }
    }

    pub fn nodes(&self) -> &[TreeNode] {
        &self.nodes
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn parent(&self, node: usize) -> Option<usize> {
        self.nodes[node].parent
    }

    pub fn depth(&self, node: usize) -> u16 {
        self.depth[node]
    }

    pub fn has_children(&self, node: usize) -> bool {
        self.child_count[node] > 0
    }

    pub fn is_expanded(&self, node: usize) -> bool {
        self.expanded[node]
    }

    pub fn set_expanded(&mut self, node: usize, expanded: bool) {
        self.expanded[node] = expanded;
    }

    /// Opens every ancestor of `node`.
    pub fn expand_to(&mut self, node: usize) {
        let mut at = self.parent(node);
        while let Some(p) = at {
            self.expanded[p] = true;
            at = self.parent(p);
        }
    }

    /// The top-level ancestor of `node` (its project).
    pub fn root(&self, node: usize) -> usize {
        let mut at = node;
        while let Some(p) = self.parent(at) {
            at = p;
        }
        at
    }

    /// Number of top-level nodes.
    pub fn root_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.parent.is_none()).count()
    }

    /// Lowercased full paths of every node, built parent first.
    fn lower_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = Vec::with_capacity(self.nodes.len());
        for (ix, node) in self.nodes.iter().enumerate() {
            let path = match node.parent {
                Some(p) => format!("{}\\{}", paths[p], self.lower[ix]),
                None => self.lower[ix].clone(),
            };
            paths.push(path);
        }
        paths
    }

    /// Takes over `old`'s open / closed branches for the nodes both trees
    /// share (by path), so a tree that grew in place keeps its shape.
    pub fn carry_expansion(&mut self, old: &TreeModel) {
        let old_paths = old.lower_paths();
        let open: std::collections::HashMap<&str, bool> = old_paths
            .iter()
            .enumerate()
            .map(|(ix, p)| (p.as_str(), old.expanded[ix]))
            .collect();
        for (ix, path) in self.lower_paths().iter().enumerate() {
            if let Some(&expanded) = open.get(path.as_str()) {
                self.expanded[ix] = expanded;
            }
        }
    }

    /// The `\`-joined path Azure DevOps uses (`Storefront\Infra\Payments`).
    pub fn path(&self, node: usize) -> String {
        let mut segments = vec![self.nodes[node].name.as_ref()];
        let mut at = self.parent(node);
        while let Some(p) = at {
            segments.push(self.nodes[p].name.as_ref());
            at = self.parent(p);
        }
        segments.reverse();
        segments.join("\\")
    }

    /// The node at `path` (segments compared case-insensitively, as Azure
    /// DevOps does).
    pub fn find_path(&self, path: &str) -> Option<usize> {
        let mut at: Option<usize> = None;
        for segment in path.split('\\').filter(|s| !s.is_empty()) {
            let segment = segment.to_lowercase();
            at = Some(
                (0..self.nodes.len())
                    .find(|&ix| self.nodes[ix].parent == at && self.lower[ix] == segment)?,
            );
        }
        at
    }

    /// Visible rows for `query` (see the module docs).
    pub fn rows(&mut self, query: &str) -> Vec<TreeRow> {
        let tokens: Vec<String> = query
            .split_whitespace()
            .take(64)
            .map(str::to_lowercase)
            .collect();
        if tokens.is_empty() {
            return self.expanded_rows();
        }
        let full: u64 = if tokens.len() == 64 {
            u64::MAX
        } else {
            (1u64 << tokens.len()) - 1
        };
        for &ix in &self.preorder {
            let ix = ix as usize;
            let mut mask = self.nodes[ix].parent.map_or(0, |p| self.mask[p]);
            if mask != full {
                let name = &self.lower[ix];
                for (bit, token) in tokens.iter().enumerate() {
                    if mask & (1 << bit) == 0 && name.contains(token.as_str()) {
                        mask |= 1 << bit;
                    }
                }
            }
            self.mask[ix] = mask;
            self.keep[ix] = mask == full;
        }
        // Ancestors of a kept node are kept (children come after parents).
        for &ix in self.preorder.iter().rev() {
            let ix = ix as usize;
            if self.keep[ix]
                && let Some(p) = self.nodes[ix].parent
            {
                self.keep[p] = true;
            }
        }
        self.preorder
            .iter()
            .map(|&ix| ix as usize)
            .filter(|&ix| self.keep[ix])
            .map(|ix| TreeRow {
                node: ix,
                depth: self.depth[ix],
                has_children: self.child_count[ix] > 0,
                expanded: true,
                matched: self.mask[ix] == full,
            })
            .collect()
    }

    fn expanded_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        // Depth of the shallowest collapsed ancestor being skipped.
        let mut hidden_below: Option<u16> = None;
        for &ix in &self.preorder {
            let ix = ix as usize;
            let depth = self.depth[ix];
            if let Some(d) = hidden_below {
                if depth > d {
                    continue;
                }
                hidden_below = None;
            }
            rows.push(TreeRow {
                node: ix,
                depth,
                has_children: self.child_count[ix] > 0,
                expanded: self.expanded[ix],
                matched: true,
            });
            if !self.expanded[ix] {
                hidden_below = Some(depth);
            }
        }
        rows
    }

    /// Byte ranges of `query` tokens in `node`'s own name.
    pub fn highlights(&self, node: usize, query: &str) -> Vec<Range<usize>> {
        let mut ranges: Vec<Range<usize>> = query
            .split_whitespace()
            .filter_map(|t| find_ci(&self.nodes[node].name, t))
            .collect();
        ranges.sort_by_key(|r| r.start);
        ranges.dedup_by(|b, a| {
            if b.start <= a.end {
                a.end = a.end.max(b.end);
                true
            } else {
                false
            }
        });
        ranges
    }

    /// Current (dates include `today`), Next (the next start) and Recent
    /// (the last two finished) leaves, in that order (FR-14), computed per
    /// root (one project each, spec 0008 FR-8): every root's Current, then
    /// every root's Next, then every root's Recent, roots in tree order.
    pub fn shortcuts(&self, today: i64) -> Vec<(Shortcut, usize)> {
        let n = self.nodes.len();
        let mut root_of = vec![0usize; n];
        for ix in 0..n {
            root_of[ix] = self.nodes[ix].parent.map_or(ix, |p| root_of[p]);
        }
        let mut current = Vec::new();
        let mut next = Vec::new();
        let mut recent = Vec::new();
        for root in (0..n).filter(|&ix| self.nodes[ix].parent.is_none()) {
            let dated: Vec<(usize, DayRange)> = (0..n)
                .filter(|&ix| root_of[ix] == root && !self.has_children(ix))
                .filter_map(|ix| self.nodes[ix].dates.map(|d| (ix, d)))
                .collect();
            let mut now: Vec<(usize, DayRange)> = dated
                .iter()
                .copied()
                .filter(|(_, d)| d.contains(today))
                .collect();
            now.sort_by_key(|(_, d)| d.start);
            current.extend(now.into_iter().map(|(ix, _)| ix));
            next.extend(
                dated
                    .iter()
                    .copied()
                    .filter(|(_, d)| d.start > today)
                    .min_by_key(|(_, d)| d.start)
                    .map(|(ix, _)| ix),
            );
            let mut past: Vec<(usize, DayRange)> = dated
                .iter()
                .copied()
                .filter(|(_, d)| d.finish < today)
                .collect();
            past.sort_by_key(|(_, d)| std::cmp::Reverse(d.finish));
            recent.extend(past.into_iter().take(2).map(|(ix, _)| ix));
        }
        current
            .into_iter()
            .map(|ix| (Shortcut::Current, ix))
            .chain(next.into_iter().map(|ix| (Shortcut::Next, ix)))
            .chain(recent.into_iter().map(|ix| (Shortcut::Recent, ix)))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    Area,
    /// `today` in days since 1970-01-01, for the shortcuts.
    Iteration {
        today: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEvent {
    /// The chosen node's full path (`Storefront\Infra\Payments`).
    Chosen(SharedString),
}

/// One row of the picker list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickRow {
    Label(Shortcut),
    Shortcut(Shortcut, usize),
    Node(TreeRow),
}

pub struct TreePicker {
    kind: TreeKind,
    title: SharedString,
    model: Option<TreeModel>,
    pending: Option<Vec<TreeNode>>,
    loading: LoadingGate,
    current: Option<SharedString>,
    current_node: Option<usize>,
    rows: Vec<PickRow>,
    selected: Option<usize>,
    input: Entity<TextInput>,
    placement: Placement,
    scroll: UniformListScrollHandle,
    return_focus: ReturnFocus,
    /// Per-node counts (spec 0008 FR-8, the list's path filter). Their
    /// presence marks the filter use: a root means its whole project, so an
    /// iteration root doesn't read "No sprint".
    counts: Option<Vec<u32>>,
    /// A quiet line above the key hints ("Showing paths from synced items").
    note: Option<SharedString>,
    /// More than one root (one per project): shortcut rows name theirs.
    multi_root: bool,
    _timer: Option<Task<()>>,
    _input_events: Subscription,
}

impl EventEmitter<TreeEvent> for TreePicker {}
impl EventEmitter<DismissEvent> for TreePicker {}

impl Focusable for TreePicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl TreePicker {
    /// Opens with the keyboard in its search field. `nodes: None` shows
    /// the loading row until [`Self::set_nodes`]; `current` is the item's
    /// path, expanded to and selected.
    pub fn new(
        kind: TreeKind,
        title: impl Into<SharedString>,
        nodes: Option<Vec<TreeNode>>,
        current: Option<SharedString>,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match kind {
            TreeKind::Area => "Search areas",
            TreeKind::Iteration { .. } => "Search iterations",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        let input_events = cx.subscribe(&input, |this, _, event, cx| {
            if *event == TextInputEvent::Changed {
                this.rebuild(None, cx);
            }
        });
        let return_focus = ReturnFocus::capture(window, cx);
        window.focus(&input.focus_handle(cx), cx);
        let mut picker = Self {
            kind,
            title: title.into(),
            model: None,
            pending: None,
            loading: LoadingGate::default(),
            current,
            current_node: None,
            rows: Vec::new(),
            selected: None,
            input,
            placement,
            scroll: UniformListScrollHandle::new(),
            return_focus,
            counts: None,
            note: None,
            multi_root: false,
            _timer: None,
            _input_events: input_events,
        };
        picker.set_nodes(nodes, cx);
        picker
    }

    /// The classification tree arrived (or `None`: loading again).
    pub fn set_nodes(&mut self, nodes: Option<Vec<TreeNode>>, cx: &mut Context<Self>) {
        match nodes {
            None => {
                self.model = None;
                self.rows.clear();
                self.selected = None;
                self.loading = LoadingGate::start(now_ms());
                if let Some(at) = self.loading.show_at() {
                    self._timer = Some(schedule(at, cx, |this: &mut Self, _| {
                        this.loading.tick(now_ms());
                    }));
                }
            }
            Some(nodes) => {
                let now = now_ms();
                let at = self.loading.ready_at(now);
                if at > now {
                    self.pending = Some(nodes);
                    self._timer = Some(schedule(at, cx, |this: &mut Self, cx| {
                        if let Some(nodes) = this.pending.take() {
                            this.apply(nodes, cx);
                        }
                    }));
                } else {
                    self.apply(nodes, cx);
                }
            }
        }
        cx.notify();
    }

    fn apply(&mut self, nodes: Vec<TreeNode>, cx: &mut Context<Self>) {
        self.apply_model(TreeModel::new(nodes), cx);
    }

    fn apply_model(&mut self, mut model: TreeModel, cx: &mut Context<Self>) {
        self._timer = None;
        self.pending = None;
        self.loading = LoadingGate::default();
        self.current_node = self.current.as_deref().and_then(|p| model.find_path(p));
        if let Some(node) = self.current_node {
            model.expand_to(node);
        }
        self.multi_root = model.root_count() > 1;
        self.model = Some(model);
        self.selected = None;
        self.rebuild(self.current_node.map(|n| (n, false)), cx);
        self.scroll
            .scroll_to_item(self.selected.unwrap_or(0), ScrollStrategy::Center);
    }

    /// A prebuilt tree (built off the UI thread) with per-node `counts`
    /// (spec 0008 FR-8; aligned with the model's nodes). The first call
    /// shows it like [`Self::set_nodes`]; later calls merge in place: the
    /// query, the open branches and the highlighted node (by path) stay.
    pub fn set_model(&mut self, model: TreeModel, counts: Vec<u32>, cx: &mut Context<Self>) {
        debug_assert_eq!(model.len(), counts.len(), "one count per node");
        self.counts = Some(counts);
        let Some(old) = self.model.take() else {
            self.apply_model(model, cx);
            return;
        };
        let mut model = model;
        model.carry_expansion(&old);
        let focus = self.selected.and_then(|row| {
            let (node, tree) = match self.rows.get(row)? {
                PickRow::Shortcut(_, node) => (*node, false),
                PickRow::Node(r) => (r.node, true),
                PickRow::Label(_) => return None,
            };
            model.find_path(&old.path(node)).map(|n| (n, tree))
        });
        self.current_node = self.current.as_deref().and_then(|p| model.find_path(p));
        // Old rows index the old model.
        self.rows.clear();
        self.selected = None;
        self.multi_root = model.root_count() > 1;
        self.model = Some(model);
        self.rebuild(focus, cx);
    }

    /// Moves the picker (the host's anchor painted after it opened).
    pub fn set_placement(&mut self, placement: Placement, cx: &mut Context<Self>) {
        if self.placement != placement {
            self.placement = placement;
            cx.notify();
        }
    }

    /// Full paths of the tree rows as shown, top to bottom (shortcut rows
    /// left out; smoke tests, stories).
    pub fn visible_paths(&self) -> Vec<String> {
        let Some(model) = &self.model else {
            return Vec::new();
        };
        self.rows
            .iter()
            .filter_map(|r| match r {
                PickRow::Node(row) => Some(model.path(row.node)),
                _ => None,
            })
            .collect()
    }

    /// The highlighted node's full path.
    pub fn selected_path(&self) -> Option<String> {
        Some(self.model.as_ref()?.path(self.selected_node()?))
    }

    /// The quiet line above the key hints; `None` hides it.
    pub fn set_note(&mut self, note: Option<SharedString>, cx: &mut Context<Self>) {
        if self.note != note {
            self.note = note;
            cx.notify();
        }
    }

    /// Shows "Loading…" without its delay (the host already waited).
    pub fn show_loading_now(&mut self, cx: &mut Context<Self>) {
        if self.model.is_none() {
            self.loading = LoadingGate::start(0);
            self.loading.tick(u64::from(super::LOADING_DELAY));
            cx.notify();
        }
    }

    pub fn model(&self) -> Option<&TreeModel> {
        self.model.as_ref()
    }

    /// Replaces the search text, as typing would.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text(query, cx));
    }

    /// Expands or collapses `node`, as `→`/`←` or a disclosure click do.
    pub fn set_expanded(&mut self, node: usize, expanded: bool, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            model.set_expanded(node, expanded);
            self.rebuild(Some((node, true)), cx);
        }
    }

    fn query(&self, cx: &App) -> String {
        self.input.read(cx).text().trim().to_owned()
    }

    fn selected_node(&self) -> Option<usize> {
        match self.rows.get(self.selected?)? {
            PickRow::Shortcut(_, node) => Some(*node),
            PickRow::Node(row) => Some(row.node),
            PickRow::Label(_) => None,
        }
    }

    /// Recomputes the rows; the selection lands on `focus` (a node, and
    /// whether to look for its tree row rather than its first appearance),
    /// else stays on the row it was on, else the first match / first row.
    fn rebuild(&mut self, focus: Option<(usize, bool)>, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let keep = focus.or_else(|| {
            let row = self.rows.get(self.selected?)?;
            let tree = matches!(row, PickRow::Node(_));
            self.selected_node()
                .filter(|_| query.is_empty())
                .map(|n| (n, tree))
        });
        let Some(model) = &mut self.model else {
            cx.notify();
            return;
        };
        let mut rows = Vec::new();
        if let (TreeKind::Iteration { today }, true) = (self.kind, query.is_empty()) {
            let mut group = None;
            for (shortcut, node) in model.shortcuts(today) {
                if group != Some(shortcut) {
                    rows.push(PickRow::Label(shortcut));
                    group = Some(shortcut);
                }
                rows.push(PickRow::Shortcut(shortcut, node));
            }
        }
        rows.extend(model.rows(&query).into_iter().map(PickRow::Node));
        self.rows = rows;
        let selectable = |r: &PickRow| !matches!(r, PickRow::Label(_));
        self.selected = keep
            .and_then(|(node, tree)| {
                self.rows.iter().position(|r| match r {
                    PickRow::Shortcut(_, n) => *n == node && !tree,
                    PickRow::Node(row) => row.node == node,
                    PickRow::Label(_) => false,
                })
            })
            .or_else(|| {
                if query.is_empty() {
                    self.rows.iter().position(selectable)
                } else {
                    self.rows
                        .iter()
                        .position(|r| matches!(r, PickRow::Node(row) if row.matched))
                }
            });
        if let Some(row) = self.selected {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.rows.len() as isize;
        let Some(mut row) = self.selected else { return };
        for _ in 0..len {
            row = (row as isize + delta).rem_euclid(len) as usize;
            if !matches!(self.rows[row], PickRow::Label(_)) {
                self.selected = Some(row);
                self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
                cx.notify();
                return;
            }
        }
    }

    /// `→`: expand, or step into the first child.
    fn right(&mut self, cx: &mut Context<Self>) {
        let filtering = !self.query(cx).is_empty();
        let Some(PickRow::Node(row)) = self.selected.and_then(|r| self.rows.get(r)).copied() else {
            return;
        };
        if !row.has_children {
            return;
        }
        if !row.expanded && !filtering {
            self.set_expanded(row.node, true, cx);
        } else if let Some(PickRow::Node(next)) = self.rows.get(self.selected.unwrap_or(0) + 1)
            && next.depth == row.depth + 1
        {
            self.step(1, cx);
        }
    }

    /// `←`: collapse, or go to the parent.
    fn left(&mut self, cx: &mut Context<Self>) {
        let filtering = !self.query(cx).is_empty();
        let Some(PickRow::Node(row)) = self.selected.and_then(|r| self.rows.get(r)).copied() else {
            return;
        };
        if row.has_children && row.expanded && !filtering {
            self.set_expanded(row.node, false, cx);
            return;
        }
        let parent = self.model.as_ref().and_then(|m| m.parent(row.node));
        if let Some(parent) = parent
            && let Some(ix) = self
                .rows
                .iter()
                .position(|r| matches!(r, PickRow::Node(n) if n.node == parent))
        {
            self.selected = Some(ix);
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.return_focus.restore(window, cx);
        cx.emit(DismissEvent);
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(node), Some(model)) = (self.selected_node(), &self.model) else {
            return;
        };
        let path = model.path(node);
        self.return_focus.restore(window, cx);
        cx.emit(TreeEvent::Chosen(path.into()));
        cx.emit(DismissEvent);
    }

    fn render_row(&self, ix: usize, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let model = self.model.as_ref().expect("rows render with a model");
        let row = self.rows[ix];
        let selected = self.selected == Some(ix);
        let node = match row {
            PickRow::Label(shortcut) => {
                return group_label(shortcut.label(), &theme)
                    .id(("tree-row", ix))
                    .role(Role::Label)
                    .aria_label(shortcut.label())
                    .into_any_element();
            }
            PickRow::Shortcut(_, node) => node,
            PickRow::Node(r) => r.node,
        };
        let current = self.current_node == Some(node);
        let name = model.nodes()[node].name.clone();
        let parent = model.parent(node);
        let frame = row_frame(("tree-row", ix), selected, &theme)
            .role(Role::TreeItem)
            .aria_selected(selected)
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.selected != Some(ix) {
                    this.selected = Some(ix);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = Some(ix);
                this.choose(window, cx);
            }));
        let check = current.then(|| {
            Icon::new(IconName::Check)
                .size(size::ICON_SM)
                .color(colors.text_secondary)
        });
        let count = self.counts.as_ref().and_then(|c| c.get(node)).copied();
        let count_cell = count.map(|n| {
            div()
                .flex_none()
                .text_color(colors.text_tertiary)
                .type_style(typography::SMALL, &theme)
                .child(n.to_string())
        });
        let count_label = |label: &mut String| {
            if let Some(n) = count {
                let noun = if n == 1 { "open item" } else { "open items" };
                label.push_str(&format!(", {n} {noun}"));
            }
        };
        match row {
            PickRow::Shortcut(shortcut, _) => {
                let dates = model.nodes()[node].dates;
                // Several projects: name each row's project; one project:
                // the parent path only when two shown leaves share a name.
                let twin = self.rows.iter().any(|r| {
                    matches!(r, PickRow::Shortcut(_, other)
                        if *other != node && model.nodes()[*other].name == name)
                });
                let context = if self.multi_root {
                    Some(model.nodes()[model.root(node)].name.to_string())
                } else if twin {
                    parent.map(|p| model.path(p))
                } else {
                    None
                };
                let mut label = format!("{name}, {}", shortcut.label().to_lowercase());
                if let Some(project) = context.as_ref().filter(|_| self.multi_root) {
                    label.push_str(&format!(", {project}"));
                }
                if let Some(d) = dates {
                    label.push_str(&format!(", {}", format_day_range(d, true)));
                }
                count_label(&mut label);
                if current {
                    label.push_str(", current value");
                }
                frame
                    .aria_label(label)
                    .aria_level(1)
                    .child(
                        div()
                            .flex_none()
                            .truncate()
                            .text_color(colors.text_primary)
                            .child(name),
                    )
                    .children(dates.map(|d| {
                        div()
                            .flex_none()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(format_day_range(d, false))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .children(context),
                    )
                    .children(count_cell)
                    .children(check)
                    .into_any_element()
            }
            PickRow::Node(r) => {
                let root = parent.is_none();
                let mut label = name.to_string();
                if let Some(p) = parent {
                    label.push_str(&format!(", in {}", model.nodes()[p].name));
                }
                label.push_str(&format!(", level {}", r.depth + 1));
                count_label(&mut label);
                if current {
                    label.push_str(", current");
                }
                let filtering = self.counts.is_some();
                let text: SharedString = match self.kind {
                    TreeKind::Iteration { .. } if root && !filtering => {
                        format!("{name} · No sprint").into()
                    }
                    _ => name.clone(),
                };
                let highlights = if query.is_empty() {
                    Vec::new()
                } else {
                    model.highlights(node, query)
                };
                let disclosure = if r.has_children {
                    let icon = if r.expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    };
                    let expanded = r.expanded;
                    div()
                        .id(("tree-disclosure", ix))
                        .size(px(size::ICON_MD))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_expanded(node, !expanded, cx);
                        }))
                        .child(
                            Icon::new(icon)
                                .size(size::ICON_XS)
                                .color(colors.text_tertiary),
                        )
                        .into_any_element()
                } else {
                    div().size(px(size::ICON_MD)).flex_none().into_any_element()
                };
                let root_icon = match self.kind {
                    TreeKind::Area if root => Some(
                        Icon::new(IconName::FolderTree)
                            .size(size::ICON_SM)
                            .color(colors.text_secondary),
                    ),
                    _ => None,
                };
                frame
                    .aria_label(label)
                    .aria_level(r.depth as usize + 1)
                    .when(r.has_children, |d| d.aria_expanded(r.expanded))
                    .pl(px(space::S8 + space::S16 * f32::from(r.depth)))
                    .gap(px(space::S4))
                    .child(disclosure)
                    .children(root_icon)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if r.matched {
                                colors.text_primary
                            } else {
                                colors.text_secondary
                            })
                            .child(highlighted(text, highlights, &theme)),
                    )
                    .children(count_cell)
                    .children(check)
                    .into_any_element()
            }
            PickRow::Label(_) => unreachable!("handled above"),
        }
    }
}

impl Render for TreePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.query(cx);
        let count = self.rows.len();
        let body = match &self.model {
            None if self.loading.is_shown() => message_row("Loading…", &theme).into_any_element(),
            None => div().into_any_element(),
            Some(_) if count == 0 => {
                let what = match self.kind {
                    TreeKind::Area => "areas",
                    TreeKind::Iteration { .. } => "iterations",
                };
                message_row(format!("No {what} match \u{201c}{query}\u{201d}"), &theme)
                    .into_any_element()
            }
            Some(_) => {
                let list = uniform_list(
                    "tree-rows",
                    count,
                    cx.processor(move |this, range: Range<usize>, _, cx| {
                        range
                            .map(|row| this.render_row(row, &query, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .h(list_height(count, VISIBLE_ROWS));
                div()
                    .id("tree-list")
                    .role(Role::Tree)
                    .aria_label(self.title.clone())
                    .child(list)
                    .into_any_element()
            }
        };
        let search_label = match self.kind {
            TreeKind::Area => "Search areas",
            TreeKind::Iteration { .. } => "Search iterations",
        };
        let card = card("tree-picker", self.placement.width, &theme)
            .role(Role::Dialog)
            .aria_label(self.title.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Choose, window, cx| this.choose(window, cx)))
            .on_action(cx.listener(|this, _: &Close, window, cx| this.dismiss(window, cx)))
            // `→`/`←` drive the tree, not the search field's caret.
            .capture_action(cx.listener(|this, _: &text_input::Right, _, cx| {
                cx.stop_propagation();
                this.right(cx);
            }))
            .capture_action(cx.listener(|this, _: &text_input::Left, _, cx| {
                cx.stop_propagation();
                this.left(cx);
            }))
            .child(
                search_field(self.input.clone().into_any_element(), &theme)
                    .id("tree-search")
                    .role(Role::SearchInput)
                    .aria_label(search_label),
            )
            .child(body)
            .children(self.note.clone().map(|note| {
                div()
                    .id("tree-note")
                    .role(Role::Status)
                    .aria_label(note.clone())
                    .pt(px(space::S4))
                    .px(px(space::S8))
                    .text_color(theme.colors.text_tertiary)
                    .type_style(typography::SMALL, &theme)
                    .child(note)
            }))
            .child(footer_hint(&theme));
        overlay("tree-picker-in", card, self.placement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn areas() -> TreeModel {
        TreeModel::new(nodes_from_paths(&[
            "Storefront",
            "Storefront\\Infra",
            "Storefront\\Infra\\Payments",
            "Storefront\\Infra\\Search",
            "Storefront\\Web",
            "Storefront\\Web\\Checkout",
            "Storefront\\Payments",
        ]))
    }

    fn paths(model: &TreeModel, rows: &[TreeRow]) -> Vec<String> {
        rows.iter().map(|r| model.path(r.node)).collect()
    }

    #[test]
    fn unfiltered_rows_follow_expansion() {
        let mut model = areas();
        let rows = model.rows("");
        assert_eq!(
            paths(&model, &rows),
            [
                "Storefront",
                "Storefront\\Infra",
                "Storefront\\Web",
                "Storefront\\Payments"
            ],
            "roots start expanded"
        );
        let infra = model.find_path("storefront\\INFRA").unwrap();
        model.set_expanded(infra, true);
        let rows = model.rows("");
        assert_eq!(rows[2].depth, 2);
        assert_eq!(model.path(rows[2].node), "Storefront\\Infra\\Payments");
        model.set_expanded(model.find_path("Storefront").unwrap(), false);
        assert_eq!(model.rows("").len(), 1);
    }

    #[test]
    fn filter_keeps_matches_with_their_ancestors() {
        let mut model = areas();
        let rows = model.rows("pay");
        assert_eq!(
            paths(&model, &rows),
            [
                "Storefront",
                "Storefront\\Infra",
                "Storefront\\Infra\\Payments",
                "Storefront\\Payments",
            ]
        );
        let matched: Vec<bool> = rows.iter().map(|r| r.matched).collect();
        assert_eq!(matched, [false, false, true, true]);
        assert!(rows.iter().all(|r| r.expanded), "filtered rows show open");
    }

    #[test]
    fn filter_matches_any_path_segment_and_all_tokens() {
        let mut model = areas();
        // "infra" lists the Infra subtree: their paths contain the segment.
        let rows = model.rows("infra");
        assert_eq!(
            paths(&model, &rows),
            [
                "Storefront",
                "Storefront\\Infra",
                "Storefront\\Infra\\Payments",
                "Storefront\\Infra\\Search",
            ]
        );
        // Tokens may match different segments; all must match.
        let rows = model.rows("infra pay");
        assert_eq!(
            paths(&model, &rows).last().unwrap(),
            "Storefront\\Infra\\Payments"
        );
        assert_eq!(rows.iter().filter(|r| r.matched).count(), 1);
        assert!(model.rows("nothing").is_empty());
    }

    #[test]
    fn highlights_cover_tokens_in_the_own_name() {
        let model = areas();
        let payments = model.find_path("Storefront\\Infra\\Payments").unwrap();
        // "PAY" and "ments" overlap-free but adjacent: one merged run.
        let ranges = model.highlights(payments, "infra PAY ments");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..8);
    }

    #[test]
    fn nodes_from_paths_share_prefixes_and_ignore_empty_segments() {
        let nodes = nodes_from_paths(&["A\\B", "A\\C", "\\A\\B\\D", "E"]);
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_ref()).collect();
        assert_eq!(names, ["A", "B", "C", "D", "E"]);
        assert_eq!(nodes[3].parent, Some(1));
        assert_eq!(nodes[4].parent, None);
    }

    #[test]
    fn a_forward_parent_reference_becomes_a_root() {
        let model = TreeModel::new(vec![TreeNode::new("x", Some(1)), TreeNode::new("y", None)]);
        assert_eq!(model.parent(0), None);
    }

    fn sprints() -> TreeModel {
        // Day numbers: Sprint 43 ended before today (100), 44 includes it,
        // 45 and 46 start later.
        let mut nodes = vec![
            TreeNode::new("Storefront", None),
            TreeNode::new("2026", Some(0)),
        ];
        for (name, start, finish) in [
            ("Sprint 41", 50, 63),
            ("Sprint 42", 64, 77),
            ("Sprint 43", 78, 91),
            ("Sprint 44", 92, 105),
            ("Sprint 46", 120, 133),
            ("Sprint 45", 106, 119),
        ] {
            nodes.push(TreeNode::new(name, Some(1)).dates(DayRange { start, finish }));
        }
        TreeModel::new(nodes)
    }

    #[test]
    fn iteration_shortcuts_are_current_next_and_two_recent() {
        let model = sprints();
        let got: Vec<(Shortcut, &str)> = model
            .shortcuts(100)
            .into_iter()
            .map(|(s, ix)| (s, model.nodes()[ix].name.as_ref()))
            .collect();
        assert_eq!(
            got,
            [
                (Shortcut::Current, "Sprint 44"),
                (Shortcut::Next, "Sprint 45"),
                (Shortcut::Recent, "Sprint 43"),
                (Shortcut::Recent, "Sprint 42"),
            ]
        );
        // The finish day still counts as current.
        assert_eq!(model.shortcuts(105)[0], (Shortcut::Current, 5));
    }

    /// Spec 0008 AC-12: shortcuts per project — both projects' current
    /// sprints, then each project's next.
    #[test]
    fn shortcuts_are_computed_per_root() {
        let mut nodes = vec![TreeNode::new("P1", None)];
        for (name, start, finish) in [
            ("Sprint 43", 78, 91),
            ("Sprint 44", 92, 105),
            ("Sprint 45", 106, 119),
        ] {
            nodes.push(TreeNode::new(name, Some(0)).dates(DayRange { start, finish }));
        }
        let p2 = nodes.len();
        nodes.push(TreeNode::new("P2", None));
        for (name, start, finish) in [("Sprint 7", 95, 101), ("Sprint 8", 102, 108)] {
            nodes.push(TreeNode::new(name, Some(p2)).dates(DayRange { start, finish }));
        }
        let model = TreeModel::new(nodes);
        assert_eq!(model.root_count(), 2);
        let got: Vec<(Shortcut, String)> = model
            .shortcuts(100)
            .into_iter()
            .map(|(s, ix)| (s, model.path(ix)))
            .collect();
        let s = |shortcut, path: &str| (shortcut, path.to_owned());
        assert_eq!(
            got,
            [
                s(Shortcut::Current, "P1\\Sprint 44"),
                s(Shortcut::Current, "P2\\Sprint 7"),
                s(Shortcut::Next, "P1\\Sprint 45"),
                s(Shortcut::Next, "P2\\Sprint 8"),
                s(Shortcut::Recent, "P1\\Sprint 43"),
            ]
        );
        assert_eq!(model.root(model.find_path("P2\\Sprint 8").unwrap()), p2);
    }

    #[test]
    fn a_grown_tree_keeps_the_branches_that_were_open_or_closed() {
        let mut old = areas();
        old.set_expanded(old.find_path("Storefront\\Infra").unwrap(), true);
        old.set_expanded(old.find_path("Storefront").unwrap(), true);
        let mut grown = TreeModel::new(nodes_from_paths(&[
            "Storefront\\Web",
            "Storefront\\Infra\\Payments",
            "Storefront\\Infra\\Queues",
            "Ops",
        ]));
        grown.set_expanded(grown.find_path("Ops").unwrap(), false);
        grown.carry_expansion(&old);
        assert!(grown.is_expanded(grown.find_path("storefront\\infra").unwrap()));
        assert!(
            !grown.is_expanded(grown.find_path("Ops").unwrap()),
            "new nodes keep theirs"
        );
        assert!(!grown.is_expanded(grown.find_path("Storefront\\Web").unwrap()));
    }

    #[test]
    fn day_ranges_format_short_and_spoken() {
        // 2026-09-29 is day 20_725.
        let r = DayRange {
            start: 20_725,
            finish: 20_736,
        };
        assert_eq!(format_day_range(r, false), "Sep 29 \u{2013} Oct 10");
        assert_eq!(format_day_range(r, true), "September 29 to October 10");
    }

    /// AC-12: filtering 10,000 nodes by "infra" keeps matches with their
    /// ancestors in ≤ 5 ms.
    #[test]
    fn filtering_ten_thousand_nodes_fits_the_keystroke_budget() {
        let nodes = fixture_10k();
        let mut model = TreeModel::new(nodes);
        let mut best = std::time::Duration::MAX;
        let mut rows = Vec::new();
        for _ in 0..5 {
            let start = std::time::Instant::now();
            rows = model.rows("infra");
            best = best.min(start.elapsed());
        }
        assert!(rows.iter().any(|r| r.matched));
        for r in &rows {
            if let Some(p) = model.parent(r.node) {
                assert!(rows.iter().any(|a| a.node == p), "ancestor kept");
            }
        }
        assert!(best.as_micros() <= 5_000, "filter took {best:?}");
    }

    /// 10 top areas × 10 teams × 10 components × 10 leaves: 11,111 nodes.
    pub(crate) fn fixture_10k() -> Vec<TreeNode> {
        let mut nodes = vec![TreeNode::new("Storefront", None)];
        let tops = [
            "Infra", "Web", "Mobile", "Data", "Payments", "Search", "Identity", "Growth",
            "Support", "Platform",
        ];
        for top in tops {
            let t = nodes.len();
            nodes.push(TreeNode::new(top, Some(0)));
            for team in 0..10 {
                let m = nodes.len();
                nodes.push(TreeNode::new(format!("Team {top} {team}"), Some(t)));
                for comp in 0..10 {
                    let c = nodes.len();
                    nodes.push(TreeNode::new(format!("Component {comp}"), Some(m)));
                    for leaf in 0..10 {
                        nodes.push(TreeNode::new(format!("Service {leaf}"), Some(c)));
                    }
                }
            }
        }
        nodes
    }
}
