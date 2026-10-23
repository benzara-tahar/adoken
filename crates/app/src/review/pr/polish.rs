//! PR review polish (spec 0019) inside the review: the tree's file filter
//! and "Only files with threads" (group A), the Threads view (group B),
//! `n`/`p` across files and Open in diff (FR-13), the header's copy
//! targets (group E) and Publish button (FR-20).
//!
//! Everything the user set here lives in one [`ReviewPolishState`] so the
//! review can be captured and restored (spec 0018 FR-5). Filtering runs on
//! the UI thread over the in-memory file list (FR-2, under a millisecond at
//! 300 files); the Threads list is virtualized and only computes the diffs
//! of cards that stayed on screen for [`DWELL`] (FR-11), through the same
//! path Files mode uses, so nothing is fetched twice.

use std::ops::Range;
use std::rc::Rc;

use adoken_diff::Side;
use adoken_model::ThreadStatus;
use adoken_ui::{TextInput, TextInputEvent, Tooltip};
use gpui::{Entity, ListOffset, Pixels, SharedString, Subscription, canvas};

use super::*;
use crate::review::filter::{self, FileQuery, FoldedPaths, Listing};
use crate::review::threads::{self, Group, Item, ScopeCounts, ThreadScope};
use crate::review::web::CopyTarget;

/// The filter field's key context (`↑`/`↓`/`↩`/`Esc` while typing).
pub const FILTER_CONTEXT: &str = "ReviewTreeFilter";
pub const FILTER_KEYS: &str = "/";
pub const CLEAR_FILTER_KEYS: &str = "secondary-backspace";
pub const THREADS_ONLY_KEYS: &str = "shift-t";
pub const THREADS_VIEW_KEYS: &str = "t";
/// FR-11: a card on screen this long queues its file's diff.
pub const DWELL: Duration = Duration::from_millis(150);
/// The Threads list lays out about one screen beyond the viewport, so the
/// cards "within one screen below" are known (FR-11).
const LOOKAHEAD: f32 = 900.0;
/// Design §2.2: the list's content width.
const THREADS_MAX_WIDTH: f32 = 880.0;

/// Files or Threads (FR-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewMode {
    #[default]
    Files,
    Threads,
}

/// What the user set in this review (FR-6): kept while it's open, across a
/// new push, gone when it closes; restorable by back/forward (0018 FR-5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewPolishState {
    pub mode: ReviewMode,
    /// The tree filter's text.
    pub filter: String,
    /// "Only files with threads" (FR-5), as set in Files mode.
    pub threads_only: bool,
    pub scope: ThreadScope,
    /// The Threads view's focused card (a thread id).
    pub focused_thread: Option<i64>,
}

/// Where a pending Open in diff / `n` / `p` lands once its file's diff is
/// computed (FR-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landing {
    Thread(usize),
    First,
    Last,
}

/// Where the focused card sat after the last layout (AC-B8). While the
/// list's scroll position is unchanged (nobody scrolled), a card above it
/// that grew, shrank, arrived or left moves it; the next layout scrolls by
/// the difference so it stays put.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FocusAnchor {
    /// The focused row.
    focused: usize,
    /// Its top below the viewport's top.
    top: Pixels,
    /// The list's scroll position: (top row, offset in it).
    scroll: (usize, Pixels),
}

fn scroll_of(list: &ListState) -> (usize, Pixels) {
    let top = list.logical_scroll_top();
    (top.item_ix, top.offset_in_item)
}

pub(in crate::review) struct Polish {
    state: ReviewPolishState,
    input: Entity<TextInput>,
    query: FileQuery,
    paths: FoldedPaths,
    listing: Listing,
    /// Threads in the current scope, per file.
    in_scope: Vec<usize>,
    counts: ScopeCounts,
    /// The Threads view's rows, in FR-8 order.
    items: Vec<Item>,
    list: ListState,
    /// Index into `items` of the focused card.
    focused: Option<usize>,
    /// Threads resolved or reactivated here: their cards keep their place
    /// until the scope changes or the view is left (FR-9).
    sticky: HashSet<i64>,
    /// Items the last layout drew (FR-11's dwell).
    laid_out: Vec<usize>,
    dwell_seen: Vec<usize>,
    _dwell: Option<Task<()>>,
    /// A pending landing for `(file, where)`; any move by the user drops it.
    landing: Option<(usize, Landing)>,
    /// Stories: group reviewers shown or hidden regardless of the setting.
    show_groups: Option<bool>,
    /// Holds the focused card in place across layouts (AC-B8).
    anchor: Rc<std::cell::Cell<Option<FocusAnchor>>>,
    _input: Subscription,
}

impl Polish {
    pub(super) fn new(loaded: &LoadedPr, cx: &mut Context<PrReview>) -> Self {
        let input = cx.new(|cx| TextInput::new("Filter files", cx));
        let events = cx.subscribe(&input, |this: &mut PrReview, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let text = input.read(cx).text().to_owned();
                this.set_filter_text(text, cx);
            }
        });
        let mut polish = Self {
            state: ReviewPolishState::default(),
            input,
            query: FileQuery::default(),
            paths: FoldedPaths::new(loaded.files.iter().map(|f| f.path.as_str())),
            listing: Listing::default(),
            in_scope: Vec::new(),
            counts: ScopeCounts::default(),
            items: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(LOOKAHEAD)),
            focused: None,
            sticky: HashSet::new(),
            laid_out: Vec::new(),
            dwell_seen: Vec::new(),
            _dwell: None,
            landing: None,
            show_groups: None,
            anchor: Rc::default(),
            _input: events,
        };
        polish.relist(loaded);
        polish
    }

    /// "Only files with threads" in effect: forced on in Threads (FR-5).
    fn threads_only(&self) -> bool {
        self.state.threads_only || self.state.mode == ReviewMode::Threads
    }

    /// Counts, per-file scope counts and the tree listing.
    fn relist(&mut self, loaded: &LoadedPr) {
        let scope = self.state.scope;
        self.counts = ScopeCounts::of(&loaded.threads);
        self.in_scope = vec![0; loaded.files.len()];
        let index: HashMap<&str, usize> = loaded
            .files
            .iter()
            .enumerate()
            .map(|(ix, f)| (f.path.as_str(), ix))
            .collect();
        for thread in loaded.threads.iter() {
            if !scope.includes(thread.status) {
                continue;
            }
            if let Some(file) = threads::thread_path(thread).and_then(|p| index.get(p)) {
                self.in_scope[*file] += 1;
            }
        }
        let in_scope = &self.in_scope;
        let keep = |file: usize| in_scope[file] > 0;
        let keep: Option<&dyn Fn(usize) -> bool> = self.threads_only().then_some(&keep);
        self.listing = filter::list(&loaded.tree, &self.paths, &self.query, keep);
    }
}

fn kbd(keys: &str) -> Kbd {
    Kbd::parse(keys).expect("static binding")
}

impl PrReview {
    // ── state (FR-6) ─────────────────────────────────────────────────────

    pub fn polish_state(&self) -> ReviewPolishState {
        let mut state = self.polish.state.clone();
        state.focused_thread = self.focused_card().map(|t| self.loaded.threads[t].id);
        state
    }

    /// Back/forward (0018): the filter, toggle, mode, scope and focused
    /// card as they were.
    pub fn restore_polish_state(
        &mut self,
        state: ReviewPolishState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focused = state.focused_thread;
        self.polish.state.threads_only = state.threads_only;
        self.polish.state.scope = state.scope;
        self.polish.sticky.clear();
        // Emits `Changed`: the listing follows.
        self.polish
            .input
            .update(cx, |input, cx| input.set_text(&state.filter, cx));
        self.set_filter_text(state.filter, cx);
        self.set_mode(state.mode, window, cx);
        if let Some(id) = focused
            && let Some(item) = self.item_of_thread(id)
        {
            self.focus_item(item, cx);
        }
        cx.notify();
    }

    pub fn review_mode(&self) -> ReviewMode {
        self.polish.state.mode
    }

    /// Listed files in tree order (tests, stories).
    pub fn listed_files(&self) -> &[usize] {
        &self.polish.listing.files
    }

    /// The Threads view's rows (tests).
    pub fn thread_items(&self) -> &[Item] {
        &self.polish.items
    }

    /// The focused card's thread (an index into `LoadedPr::threads`).
    pub fn focused_card(&self) -> Option<usize> {
        self.polish
            .focused
            .and_then(|ix| self.polish.items.get(ix))
            .and_then(|item| item.thread())
    }

    /// The thread `n`/`p` landed on in Files mode.
    pub fn thread_cursor_thread(&self) -> Option<usize> {
        match self.rows.get(self.thread_cursor?)? {
            PaneRow::Thread(t) | PaneRow::Older(t) => Some(*t),
            _ => None,
        }
    }

    /// The file whose diff Files mode shows (tests).
    pub fn selected_file(&self) -> usize {
        self.selected
    }

    /// The file selected in the tree: highlighted and listed (FR-4: a
    /// selection the filter hides selects no row).
    pub fn tree_selected_file(&self) -> Option<usize> {
        self.tree_highlighted()
            .filter(|file| self.polish.listing.files.contains(file))
    }

    /// "Only files with threads" as shown: forced on in Threads (FR-5).
    pub fn threads_only_shown(&self) -> bool {
        self.polish.threads_only()
    }

    pub fn scope_counts(&self) -> ScopeCounts {
        self.polish.counts
    }

    /// The thread cursor's row is laid out inside the diff's viewport
    /// (FR-13's landing; tests).
    pub fn thread_cursor_on_screen(&self) -> bool {
        let Some(row) = self.thread_cursor else {
            return false;
        };
        let viewport = self.diff_list.viewport_bounds();
        self.diff_list
            .bounds_for_item(row)
            .is_some_and(|b| b.top() < viewport.bottom() && b.bottom() > viewport.top())
    }

    /// The focused card's top below the Threads list's top, once laid out
    /// (AC-B8; tests).
    pub fn focused_card_top(&self) -> Option<Pixels> {
        let card = self.polish.list.bounds_for_item(self.polish.focused?)?;
        Some(card.top() - self.polish.list.viewport_bounds().top())
    }

    /// The Threads list's top row, and whether its last row is in view, as
    /// last laid out (tests, frame runs).
    pub fn threads_scroll(&self) -> (usize, bool) {
        let list = &self.polish.list;
        let end = self
            .polish
            .items
            .len()
            .checked_sub(1)
            .and_then(|last| list.bounds_for_item(last))
            .is_some_and(|row| row.bottom() <= list.viewport_bounds().bottom() + px(1.0));
        (list.logical_scroll_top().item_ix, end)
    }

    /// The filter field has the keyboard.
    pub fn filter_focused(&self, window: &Window, cx: &App) -> bool {
        self.polish.input.focus_handle(cx).is_focused(window)
    }

    pub fn filter_text(&self) -> &str {
        &self.polish.state.filter
    }

    /// The tree header's counts, as shown ("Files 3 of 5", "1/3 viewed").
    pub fn tree_counts(&self) -> (String, String) {
        let total = self.loaded.files.len();
        let listing = &self.polish.listing;
        let files = if listing.filtered {
            format!("Files {} of {total}", listing.files.len())
        } else {
            format!("Files {total}")
        };
        let viewed = listing
            .files
            .iter()
            .filter(|f| self.loaded.is_viewed(**f))
            .count();
        (files, format!("{viewed}/{} viewed", listing.files.len()))
    }

    pub(super) fn files_changed(&mut self) {
        self.polish.paths = FoldedPaths::new(self.loaded.files.iter().map(|f| f.path.as_str()));
        self.polish.landing = None;
        self.relist();
    }

    /// Threads changed (a write's answer, a sync): counts, the listing and
    /// the Threads view follow, cards keep their place on screen.
    pub(super) fn threads_changed(&mut self) {
        self.relist();
    }

    fn relist(&mut self) {
        self.polish.relist(&self.loaded);
        self.rebuild_items();
    }

    fn set_filter_text(&mut self, text: String, cx: &mut Context<Self>) {
        if self.polish.state.filter == text {
            return;
        }
        self.polish.query = FileQuery::parse(&text);
        self.polish.state.filter = text;
        self.relist();
        cx.notify();
    }

    // ── group A ──────────────────────────────────────────────────────────

    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_overlays();
        window.focus(&self.polish.input.focus_handle(cx), cx);
        cx.notify();
    }

    fn focus_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// `⌘⌫`: no text, "Only files with threads" off (FR-4).
    pub fn clear_filter(&mut self, cx: &mut Context<Self>) {
        self.polish.state.threads_only = false;
        self.polish
            .input
            .update(cx, |input, cx| input.set_text("", cx));
        self.set_filter_text(String::new(), cx);
        self.relist();
        cx.notify();
    }

    /// `⇧T` (FR-5); always on in Threads mode.
    pub fn toggle_threads_only(&mut self, cx: &mut Context<Self>) {
        if self.polish.state.mode == ReviewMode::Threads {
            return;
        }
        self.polish.state.threads_only = !self.polish.state.threads_only;
        self.relist();
        cx.notify();
    }

    /// `↓`/`↑` in the field: the next / previous listed file (its diff
    /// loads as on `j`/`k`).
    fn filter_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.step_file(delta, cx);
    }

    /// `↩` in the field: the first listed file when the selection isn't
    /// listed, then the keyboard goes back to the review.
    fn filter_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let files = &self.polish.listing.files;
        if !files.contains(&self.selected)
            && let Some(&first) = files.first()
        {
            self.select_file(first, cx);
        }
        self.focus_review(window, cx);
    }

    /// `Esc` in the field: clear its text, or leave it when empty.
    fn filter_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.polish.state.filter.is_empty() {
            self.focus_review(window, cx);
        } else {
            self.polish
                .input
                .update(cx, |input, cx| input.set_text("", cx));
            self.set_filter_text(String::new(), cx);
        }
    }

    /// `j`/`k` (and `v`'s advance) over the listed files (FR-4). From a
    /// selection the filter hides, the first listed file after / before
    /// its tree position.
    pub(super) fn step_listed(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.polish.landing = None;
        let next = {
            let files = &self.polish.listing.files;
            match files.iter().position(|f| *f == self.selected) {
                Some(pos) => {
                    let next = pos as isize + delta;
                    (0..files.len() as isize)
                        .contains(&next)
                        .then(|| files[next as usize])
                }
                None => {
                    let order = &self.loaded.order;
                    let at = |file: usize| order.iter().position(|f| *f == file);
                    let here = at(self.selected).unwrap_or(0);
                    if delta > 0 {
                        files.iter().copied().find(|f| at(*f) > Some(here))
                    } else {
                        files.iter().rev().copied().find(|f| at(*f) < Some(here))
                    }
                }
            }
        };
        if let Some(file) = next {
            self.select_file(file, cx);
        }
    }

    /// The tree row showing `file`, if listed.
    pub(super) fn listed_position(&self, file: usize) -> Option<usize> {
        self.polish.listing.rows.iter().position(
            |&row| matches!(&self.loaded.tree[row], TreeRow::File { index, .. } if *index == file),
        )
    }

    // ── FR-13: n / p across files ────────────────────────────────────────

    /// `n` past the selected file's last thread (or `p` before its first):
    /// the next / previous listed file with a thread in scope.
    pub(super) fn step_thread_across(&mut self, forward: bool, cx: &mut Context<Self>) {
        let files = &self.polish.listing.files;
        let in_scope = &self.polish.in_scope;
        let order = &self.loaded.order;
        let at = |file: usize| order.iter().position(|f| *f == file);
        let here = at(self.selected);
        let next = if forward {
            files
                .iter()
                .copied()
                .find(|f| in_scope[*f] > 0 && at(*f) > here)
        } else {
            files
                .iter()
                .rev()
                .copied()
                .find(|f| in_scope[*f] > 0 && at(*f) < here)
        };
        let Some(file) = next else {
            return;
        };
        self.select_file(file, cx);
        let landing = if forward {
            Landing::First
        } else {
            Landing::Last
        };
        self.land_or_wait(file, landing, cx);
    }

    fn land_or_wait(&mut self, file: usize, landing: Landing, cx: &mut Context<Self>) {
        if self.loaded.files[file].diff.is_some() {
            self.land(landing, cx);
        } else {
            self.polish.landing = Some((file, landing));
        }
    }

    /// The selected file's diff arrived: a pending landing happens now.
    pub(super) fn try_land(&mut self, cx: &mut Context<Self>) {
        if let Some((file, landing)) = self.polish.landing
            && file == self.selected
        {
            self.polish.landing = None;
            self.land(landing, cx);
        }
    }

    /// Any move by the user drops a pending landing (FR-13).
    pub(super) fn user_moved(&mut self) {
        self.polish.landing = None;
    }

    /// Put the thread cursor on `landing` and scroll so the card and its
    /// anchored line are on screen.
    fn land(&mut self, landing: Landing, cx: &mut Context<Self>) {
        let threads: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, PaneRow::Thread(_) | PaneRow::Older(_)))
            .map(|(ix, _)| ix)
            .collect();
        let row = match landing {
            Landing::First => threads.first().copied(),
            Landing::Last => threads.last().copied(),
            Landing::Thread(t) => self
                .rows
                .iter()
                .position(|r| matches!(r, PaneRow::Thread(x) | PaneRow::Older(x) if *x == t)),
        };
        let Some(row) = row else {
            return;
        };
        self.thread_cursor = Some(row);
        // The anchored line sits above the thread's cards at that line.
        let line = (0..row)
            .rev()
            .find(|ix| {
                !matches!(
                    self.rows[*ix],
                    PaneRow::Thread(_) | PaneRow::Older(_) | PaneRow::OlderHeader(_)
                )
            })
            .unwrap_or(row);
        self.diff_list.scroll_to(ListOffset {
            item_ix: line.saturating_sub(2),
            offset_in_item: px(0.0),
        });
        cx.notify();
    }

    // ── group B: mode, scope ─────────────────────────────────────────────

    /// `t`: Files ↔ Threads (FR-7, FR-12).
    pub fn toggle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = match self.polish.state.mode {
            ReviewMode::Files => ReviewMode::Threads,
            ReviewMode::Threads => ReviewMode::Files,
        };
        self.set_mode(mode, window, cx);
    }

    pub fn set_mode(&mut self, mode: ReviewMode, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_mode(mode, cx);
        self.focus_review(window, cx);
    }

    /// [`Self::set_mode`] without moving focus.
    pub fn apply_mode(&mut self, mode: ReviewMode, cx: &mut Context<Self>) {
        if self.polish.state.mode == mode {
            return;
        }
        let back_to = self.focused_card().and_then(|t| self.file_of_thread(t));
        self.polish.state.mode = mode;
        self.polish.sticky.clear();
        self.polish.landing = None;
        self.relist();
        match mode {
            ReviewMode::Threads => {
                // Open on the selected file's first card, else the first.
                let start = self
                    .polish
                    .items
                    .iter()
                    .position(|i| matches!(i, Item::Card { group: Group::File(f), .. } if *f == self.selected))
                    .or_else(|| self.polish.items.iter().position(|i| i.thread().is_some()));
                self.polish.focused = None;
                if let Some(item) = start {
                    self.focus_item(item, cx);
                }
            }
            ReviewMode::Files => {
                self.polish.focused = None;
                if let Some(file) = back_to
                    && file != self.selected
                {
                    self.select_file(file, cx);
                }
            }
        }
        cx.notify();
    }

    pub fn set_scope(&mut self, scope: ThreadScope, cx: &mut Context<Self>) {
        self.polish.state.scope = scope;
        self.polish.sticky.clear();
        self.relist();
        cx.notify();
    }

    /// The Threads rows for the current scope and filter, keeping the
    /// focused card and its screen position (FR-9, AC-B8): only the rows
    /// that changed are spliced, so the rest keep their measured heights;
    /// the top row stays where it was; [`FocusAnchor`] then holds the
    /// focused card in place while cards above it change height.
    fn rebuild_items(&mut self) {
        let polish = &mut self.polish;
        if polish.state.mode != ReviewMode::Threads {
            polish.items.clear();
            polish.focused = None;
            polish.anchor.set(None);
            polish.list.reset(0);
            return;
        }
        let items = threads::items(
            &self.loaded,
            polish.state.scope,
            &polish.query,
            &polish.sticky,
        );
        if items == polish.items {
            return;
        }
        let focused = polish.focused.and_then(|ix| polish.items.get(ix).copied());
        let top = polish.list.logical_scroll_top();
        let top_item = polish.items.get(top.item_ix).copied();
        let old = &polish.items;
        // A header whose count changed is the same row.
        let same = |a: &Item, b: &Item| a.same_row(b);
        let prefix = old
            .iter()
            .zip(&items)
            .take_while(|(a, b)| same(a, b))
            .count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(items[prefix..].iter().rev())
            .take_while(|(a, b)| same(a, b))
            .count();
        polish
            .list
            .splice(prefix..old.len() - suffix, items.len() - prefix - suffix);
        if let Some(item_ix) = top_item.and_then(|t| items.iter().position(|i| i.same_row(&t))) {
            polish.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: top.offset_in_item,
            });
        }
        polish.focused = focused.and_then(|item| items.iter().position(|i| *i == item));
        // The anchor follows the rows' new indices.
        polish.anchor.set(polish.anchor.get().and_then(|anchor| {
            Some(FocusAnchor {
                focused: polish.focused?,
                scroll: scroll_of(&polish.list),
                ..anchor
            })
        }));
        polish.items = items;
    }

    fn item_of_thread(&self, id: i64) -> Option<usize> {
        self.polish.items.iter().position(|item| {
            item.thread()
                .is_some_and(|t| self.loaded.threads[t].id == id)
        })
    }

    pub(super) fn file_of_thread(&self, thread: usize) -> Option<usize> {
        let path = threads::thread_path(&self.loaded.threads[thread])?;
        self.loaded.files.iter().position(|f| f.path == path)
    }

    fn focus_item(&mut self, item: usize, cx: &mut Context<Self>) {
        self.polish.focused = Some(item);
        let list = &self.polish.list;
        if list.bounds_for_item(item).is_some() {
            // Laid out: the heights up to it are known.
            list.scroll_to_reveal_item(item);
        } else {
            // Not laid out yet (above the top, or beyond the look-ahead):
            // its height is unknown, so it goes to the top, with its
            // group's header when it's the group's first card.
            let item_ix = match item.checked_sub(1) {
                Some(prev) if matches!(self.polish.items[prev], Item::Header { .. }) => prev,
                _ => item,
            };
            list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
        if let Some(Group::File(file)) = self.polish.items.get(item).map(|i| i.group())
            && let Some(pos) = self.listed_position(file)
        {
            self.tree_scroll
                .scroll_to_item(pos, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// `n`/`↓` and `p`/`↑` (FR-12): the next / previous card, no wrap.
    pub(super) fn step_card(&mut self, delta: isize, cx: &mut Context<Self>) {
        let items = &self.polish.items;
        let from = self.polish.focused;
        let next = if delta > 0 {
            let start = from.map_or(0, |f| f + 1);
            (start..items.len()).find(|ix| items[*ix].thread().is_some())
        } else {
            from.and_then(|f| (0..f).rev().find(|ix| items[*ix].thread().is_some()))
        };
        if let Some(ix) = next {
            self.focus_item(ix, cx);
        }
    }

    /// `j`/`k`: the first card of the next / previous group.
    pub(super) fn step_group(&mut self, delta: isize, cx: &mut Context<Self>) {
        let items = &self.polish.items;
        let headers: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i, Item::Header { .. }))
            .map(|(ix, _)| ix)
            .collect();
        let current = self
            .polish
            .focused
            .and_then(|f| headers.iter().rposition(|h| *h < f));
        let target = match (current, delta > 0) {
            (None, true) => headers.first().copied(),
            (None, false) => None,
            (Some(at), true) => headers.get(at + 1).copied(),
            (Some(at), false) => at.checked_sub(1).map(|p| headers[p]),
        };
        if let Some(header) = target {
            self.focus_item(header + 1, cx);
        }
    }

    pub(super) fn first_or_last_card(&mut self, last: bool, cx: &mut Context<Self>) {
        let items = &self.polish.items;
        let card = if last {
            items.iter().rposition(|i| i.thread().is_some())
        } else {
            items.iter().position(|i| i.thread().is_some())
        };
        if let Some(ix) = card {
            self.focus_item(ix, cx);
        }
    }

    /// A thread resolved or reactivated from the Threads view keeps its
    /// card until the scope changes (FR-9).
    pub(super) fn keep_card(&mut self, thread: usize) {
        if self.polish.state.mode == ReviewMode::Threads {
            self.polish.sticky.insert(self.loaded.threads[thread].id);
        }
    }

    /// A click on a tree file in Threads mode: its group's first card.
    pub(super) fn focus_file_group(&mut self, file: usize, cx: &mut Context<Self>) {
        let item = self
            .polish
            .items
            .iter()
            .position(|i| matches!(i, Item::Card { group: Group::File(f), .. } if *f == file));
        if let Some(item) = item {
            self.focus_item(item, cx);
        }
    }

    /// `↩` / Open in diff (FR-13): Files with the thread's file selected,
    /// its card in view and the thread cursor on it. Not for PR-level
    /// threads or files no longer in the PR.
    pub fn open_thread_in_diff(
        &mut self,
        thread: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.file_of_thread(thread) else {
            return;
        };
        // An open for back / forward (spec 0018 FR-3.4).
        let before = self.history_state();
        if !self
            .polish
            .query
            .matches(&filter::fold(&self.loaded.files[file].path))
        {
            self.polish
                .input
                .update(cx, |input, cx| input.set_text("", cx));
            self.polish.query = FileQuery::default();
            self.polish.state.filter.clear();
        }
        self.polish.state.mode = ReviewMode::Files;
        self.polish.sticky.clear();
        self.relist();
        if file != self.selected {
            self.select_file(file, cx);
        }
        self.land_or_wait(file, Landing::Thread(thread), cx);
        self.focus_review(window, cx);
        cx.emit(super::history::ThreadOpened(Box::new(before)));
    }

    pub(super) fn open_focused_in_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(thread) = self.focused_card() {
            self.open_thread_in_diff(thread, window, cx);
        }
    }

    // ── FR-11: excerpts of dwelled cards ────────────────────────────────

    /// Called before each paint: the last layout's cards, once unchanged
    /// for [`DWELL`], queue their files' diffs.
    pub(super) fn track_dwell(&mut self, cx: &mut Context<Self>) {
        if self.polish.state.mode != ReviewMode::Threads {
            return;
        }
        let mut seen = std::mem::take(&mut self.polish.laid_out);
        seen.sort_unstable();
        seen.dedup();
        if seen.is_empty() || seen == self.polish.dwell_seen {
            return;
        }
        self.polish.dwell_seen = seen;
        self.restart_dwell(cx);
    }

    fn restart_dwell(&mut self, cx: &mut Context<Self>) {
        self.polish._dwell = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DWELL).await;
            this.update(cx, |this, cx| this.dwell_elapsed(cx)).ok();
        }));
    }

    fn dwell_elapsed(&mut self, cx: &mut Context<Self>) {
        let mut now = self.polish.laid_out.clone();
        now.sort_unstable();
        now.dedup();
        if !now.is_empty() && now != self.polish.dwell_seen {
            // Still moving: wait for it to rest.
            self.polish.dwell_seen = now;
            self.restart_dwell(cx);
            return;
        }
        let mut files: Vec<usize> = Vec::new();
        for &ix in &self.polish.dwell_seen {
            if self.polish.list.item_is_above_viewport(ix) == Some(true) {
                continue;
            }
            if let Some(Item::Card {
                group: Group::File(file),
                ..
            }) = self.polish.items.get(ix)
                && !files.contains(file)
            {
                files.push(*file);
            }
        }
        for file in files {
            let entry = &self.loaded.files[file];
            let needs = entry.diff.is_none()
                && entry.collapsed.is_none()
                && entry.unavailable.is_none()
                && !self.computing.contains(&file);
            if needs && !(self.is_offline() && self.blobs.is_remote()) {
                self.compute(file, false, cx);
            }
        }
    }

    // ── group E, FR-20 ───────────────────────────────────────────────────

    pub(super) fn copy_target(&mut self, target: CopyTarget, cx: &mut Context<Self>) {
        let toast = web::copy_pr_text(self.loaded.pr(), target, cx);
        cx.emit(Toasted(toast));
    }

    // ── render: tree (A) ─────────────────────────────────────────────────

    /// Counts, filter field and the threads toggle (design §1.1).
    pub(super) fn render_tree_header(
        &self,
        indicating: bool,
        unknown: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let (files, viewed) = self.tree_counts();
        let filtered = self.polish.listing.filtered;
        let total_viewed = (0..self.loaded.files.len())
            .filter(|f| self.loaded.is_viewed(*f))
            .count();
        let counts = div()
            .id("review-tree-counts")
            .flex()
            .items_center()
            .justify_between()
            .gap(px(space::S8))
            .h(px(size::CONTROL_LG))
            .px(px(space::S12))
            .text_color(colors.text_secondary)
            .type_style(typography::LABEL, &theme)
            .role(gpui::Role::Status)
            .aria_label(if filtered {
                format!(
                    "{} of {} files",
                    self.polish.listing.files.len(),
                    self.loaded.files.len()
                )
            } else {
                files.clone()
            })
            .child(if unknown { "Files".to_owned() } else { files })
            .child(if indicating {
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .child(Spinner::new().size(size::ICON_XS))
                    .child("Loading files…")
                    .into_any_element()
            } else if unknown {
                div().into_any_element()
            } else {
                div()
                    .id("review-tree-viewed")
                    .when(filtered, |d| {
                        d.tooltip(Tooltip::text(format!(
                            "{total_viewed} of {} viewed in this pull request",
                            self.loaded.files.len()
                        )))
                    })
                    .child(viewed)
                    .into_any_element()
            });
        let focused = self.filter_focused(window, cx);
        let empty = self.polish.state.filter.is_empty();
        let entity = cx.entity().downgrade();
        let field = div()
            .id("review-tree-filter")
            .key_context(FILTER_CONTEXT)
            .mx(px(space::S8))
            .h(px(size::CONTROL_MD))
            .flex()
            .items_center()
            .gap(px(space::S6))
            .px(px(space::S8))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(if focused {
                colors.border_focus
            } else {
                colors.border_default
            })
            .bg(colors.bg_input)
            .role(gpui::Role::SearchInput)
            .aria_label("Filter files")
            .capture_action(
                cx.listener(|this, _: &adoken_ui::text_input::DeleteToStart, _, cx| {
                    cx.stop_propagation();
                    this.clear_filter(cx);
                }),
            )
            .on_action(cx.listener(|this, _: &FilterNextFile, _, cx| this.filter_step(1, cx)))
            .on_action(cx.listener(|this, _: &FilterPreviousFile, _, cx| this.filter_step(-1, cx)))
            .on_action(cx.listener(|this, _: &FilterAccept, w, cx| this.filter_accept(w, cx)))
            .on_action(cx.listener(|this, _: &FilterEscape, w, cx| this.filter_escape(w, cx)))
            .child(
                Icon::new(IconName::Search)
                    .size(size::ICON_SM)
                    .color(colors.text_tertiary),
            )
            .child(div().flex_1().min_w_0().child(self.polish.input.clone()))
            .when(empty && !focused, |d| d.child(kbd(FILTER_KEYS)))
            .when(!empty, |d| {
                d.child(
                    div()
                        .id("review-tree-filter-clear")
                        .flex_none()
                        .p(px(space::S2))
                        .rounded(px(radius::XS))
                        .cursor_pointer()
                        .hover(|s| s.bg(colors.bg_hover))
                        .role(gpui::Role::Button)
                        .aria_label("Clear filter")
                        .tooltip(Tooltip::text(format!(
                            "Clear filter · {}",
                            kbd(CLEAR_FILTER_KEYS).label()
                        )))
                        .child(
                            Icon::new(IconName::X)
                                .size(size::ICON_SM)
                                .color(colors.text_tertiary),
                        )
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |this, cx| this.clear_filter(cx)).ok();
                        }),
                )
            });
        let threads_mode = self.polish.state.mode == ReviewMode::Threads;
        let pressed = self.polish.threads_only();
        let toggle = div()
            .id("review-threads-only")
            .mx(px(space::S8))
            .h(px(size::CONTROL_SM))
            .flex()
            .items_center()
            .gap(px(space::S6))
            .px(px(space::S6))
            .rounded(px(radius::SM))
            .type_style(typography::SMALL, &theme)
            .role(gpui::Role::Button)
            .aria_label("Only files with threads")
            .aria_toggled(if pressed {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .when(pressed, |d| d.bg(colors.bg_selected))
            .when(threads_mode, |d| {
                d.opacity(adoken_theme::opacity::DIMMED)
                    .tooltip(Tooltip::text("Always on in Threads"))
            })
            .when(!threads_mode, |d| {
                d.cursor_pointer()
                    .when(!pressed, |d| d.hover(|s| s.bg(colors.bg_hover)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_threads_only(cx)))
            })
            .child(
                Icon::new(IconName::MessageSquare)
                    .size(size::ICON_SM)
                    .color(if pressed {
                        colors.accent_fg
                    } else {
                        colors.text_tertiary
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .text_color(colors.text_secondary)
                    .child("Only files with threads"),
            )
            .child(kbd(THREADS_ONLY_KEYS));
        div()
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .pb(px(space::S6))
            .child(counts)
            .child(field)
            .child(toggle)
    }

    /// The tree filtered to nothing (design §1.1).
    pub(super) fn render_tree_empty(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let listing = &self.polish.listing;
        if !listing.filtered || !listing.rows.is_empty() || self.loaded.files.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let text = &self.polish.state.filter;
        let (message, button) = if text.trim().is_empty() {
            (
                format!(
                    "No files with {} threads",
                    self.polish.state.scope.adjective()
                )
                .replace("  ", " "),
                Button::new("tree-show-all", "Show all files")
                    .variant(ButtonVariant::Ghost)
                    .shortcut(kbd(THREADS_ONLY_KEYS))
                    .disabled(self.polish.state.mode == ReviewMode::Threads)
                    .on_press(compose::press(cx, |this, _, cx| {
                        this.toggle_threads_only(cx)
                    })),
            )
        } else {
            (
                format!("No files match \u{201c}{}\u{201d}", text.trim()),
                Button::new("tree-clear-filter", "Clear filter")
                    .variant(ButtonVariant::Ghost)
                    .shortcut(kbd(CLEAR_FILTER_KEYS))
                    .on_press(compose::press(cx, |this, _, cx| this.clear_filter(cx))),
            )
        };
        Some(
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(space::S8))
                .px(px(space::S12))
                .py(px(space::S12))
                .text_color(theme.colors.text_secondary)
                .type_style(typography::SMALL, &theme)
                .child(message)
                .child(button)
                .into_any_element(),
        )
    }

    /// Highlight ranges of tree row `row`'s own name (FR-2).
    pub(super) fn row_highlights(&self, name: &str) -> Vec<Range<usize>> {
        filter::highlights(name, &self.polish.query)
    }

    /// Tree row ids in display order (the listed rows).
    pub(super) fn tree_rows(&self) -> &[usize] {
        &self.polish.listing.rows
    }

    /// The file the tree highlights: the selection, or in Threads mode the
    /// focused card's file (FR-12).
    pub(super) fn tree_highlighted(&self) -> Option<usize> {
        match self.polish.state.mode {
            ReviewMode::Files => Some(self.selected),
            ReviewMode::Threads => self.focused_card().and_then(|t| self.file_of_thread(t)),
        }
    }

    // ── render: mode bar (B) ─────────────────────────────────────────────

    /// `[Files | Threads n]  t` (design §1.2, `review.mode`).
    pub(super) fn render_mode_switch(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scope = self.polish.state.scope;
        let mode = self.polish.state.mode;
        let entity = cx.entity().downgrade();
        let labels = [
            (ReviewMode::Files, "Files".to_owned()),
            (
                ReviewMode::Threads,
                format!("Threads {}", self.polish.counts.get(scope)),
            ),
        ];
        let options: Vec<(String, bool)> = labels
            .iter()
            .map(|(m, label)| (label.clone(), *m == mode))
            .collect();
        let segments = segments(
            "review-mode",
            "Files or threads",
            options.into_iter(),
            Rc::new(move |ix, window, cx| {
                let mode = labels[ix].0;
                entity
                    .update(cx, |this, cx| this.set_mode(mode, window, cx))
                    .ok();
            }),
            cx,
        );
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(space::S6))
            .child(segments)
            .child(kbd(THREADS_VIEW_KEYS))
    }

    fn render_scope(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let counts = self.polish.counts;
        let current = self.polish.state.scope;
        let entity = cx.entity().downgrade();
        segments(
            "review-thread-scope",
            "Thread status",
            ThreadScope::ALL
                .iter()
                .map(move |s| (format!("{} {}", s.label(), counts.get(*s)), *s == current)),
            Rc::new(move |ix, _, cx| {
                entity
                    .update(cx, |this, cx| this.set_scope(ThreadScope::ALL[ix], cx))
                    .ok();
            }),
            cx,
        )
    }

    // ── render: Threads view (B) ─────────────────────────────────────────

    pub(super) fn render_threads(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let bar = div()
            .flex()
            .items_center()
            .gap(px(space::S12))
            .h(px(size::CONTROL_LG))
            .px(px(space::S12))
            .bg(colors.bg_surface)
            .border_b_1()
            .border_color(colors.border_subtle)
            .type_style(typography::SMALL, &theme)
            .child(self.render_mode_switch(cx))
            .child(self.render_scope(cx));
        let body = if self.polish.items.is_empty() {
            self.render_threads_empty(cx)
        } else {
            let state = self.polish.list.clone();
            let anchor = self.polish.anchor.clone();
            let focused = self.polish.focused;
            div()
                .id("review-threads-list")
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .role(gpui::Role::List)
                .aria_label("Threads")
                .child(
                    list(
                        self.polish.list.clone(),
                        cx.processor(|this, ix: usize, _, cx| {
                            this.polish.laid_out.push(ix);
                            this.render_threads_item(ix, cx)
                        }),
                    )
                    .flex_1()
                    .min_h_0(),
                )
                // After the list's layout: hold the focused card in place
                // (AC-B8); a correction draws next frame.
                .child(
                    canvas(
                        move |_, window, _| {
                            let viewport = state.viewport_bounds();
                            let now = focused.and_then(|ix| {
                                let card = state.bounds_for_item(ix)?;
                                (card.top() < viewport.bottom() && card.bottom() > viewport.top())
                                    .then(|| (ix, card.top() - viewport.top()))
                            });
                            let scroll = scroll_of(&state);
                            let Some((ix, top)) = now else {
                                anchor.set(None);
                                return;
                            };
                            match anchor.get() {
                                Some(was)
                                    if was.focused == ix
                                        && was.scroll == scroll
                                        && (top - was.top).abs() >= px(0.5) =>
                                {
                                    state.scroll_by(top - was.top);
                                    window.refresh();
                                    anchor.set(Some(FocusAnchor {
                                        scroll: scroll_of(&state),
                                        ..was
                                    }));
                                }
                                _ => anchor.set(Some(FocusAnchor {
                                    focused: ix,
                                    top,
                                    scroll,
                                })),
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_0(),
                )
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .bg(colors.bg_code)
            .child(bar)
            .child(body)
            .into_any_element()
    }

    fn render_threads_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let counts = self.polish.counts;
        let scope = self.polish.state.scope;
        let text = self.polish.state.filter.trim().to_owned();
        let (title, action): (String, Option<Button>) = if counts.get(ThreadScope::All) == 0 {
            ("No comments on this pull request yet".into(), None)
        } else if !text.is_empty() {
            (
                format!("No threads match \u{201c}{text}\u{201d}"),
                Some(
                    Button::new("threads-clear-filter", "Clear filter")
                        .variant(ButtonVariant::Ghost)
                        .shortcut(kbd(CLEAR_FILTER_KEYS))
                        .on_press(compose::press(cx, |this, _, cx| this.clear_filter(cx))),
                ),
            )
        } else {
            let all = counts.get(ThreadScope::All);
            (
                format!("No {} threads", scope.adjective()),
                Some(
                    Button::new("threads-show-all", format!("Show all {all}"))
                        .variant(ButtonVariant::Ghost)
                        .on_press(compose::press(cx, |this, _, cx| {
                            this.set_scope(ThreadScope::All, cx)
                        })),
                ),
            )
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(space::S8))
            .px(px(space::S16))
            .py(px(space::S16))
            .child(
                div()
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY_STRONG, &theme)
                    .child(title),
            )
            .children(action)
            .into_any_element()
    }

    fn render_threads_item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.polish.items[ix] {
            Item::Header { group, count } => self.render_group_header(group, count, cx),
            Item::Card { thread, group } => {
                let focused = self.polish.focused == Some(ix);
                self.render_card(thread, group, focused, cx)
            }
        }
    }

    fn render_group_header(
        &self,
        group: Group,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let noun = if count == 1 { "thread" } else { "threads" };
        let (dir, name): (String, String) = match group {
            Group::Pr => (String::new(), "Pull request".into()),
            Group::Gone => (String::new(), "Files no longer in this pull request".into()),
            Group::File(file) => {
                let path = &self.loaded.files[file].path;
                match path.rsplit_once('/') {
                    Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
                    None => (String::new(), path.clone()),
                }
            }
        };
        let open = match group {
            Group::File(file) => {
                let first = self.polish.items.iter().find_map(|i| match i {
                    Item::Card {
                        thread,
                        group: Group::File(f),
                    } if *f == file => Some(*thread),
                    _ => None,
                });
                first.map(|thread| {
                    Button::new(("group-open-in-diff", file), "Open in diff")
                        .variant(ButtonVariant::Ghost)
                        .shortcut(kbd("enter"))
                        .on_press(compose::press(cx, move |this, w, cx| {
                            this.open_thread_in_diff(thread, w, cx)
                        }))
                })
            }
            _ => None,
        };
        let code = matches!(group, Group::File(_));
        div()
            .id(("thread-group", ix_of(group)))
            .pt(px(space::S16))
            .px(px(space::S16))
            .child(
                div()
                    .id("thread-group-heading")
                    .max_w(px(THREADS_MAX_WIDTH))
                    .h(px(size::CONTROL_MD))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .role(gpui::Role::Heading)
                    .aria_label(format!("{dir}{name} · {count} {noun}"))
                    .type_style(
                        if code {
                            typography::CODE_INLINE
                        } else {
                            typography::BODY_STRONG
                        },
                        &theme,
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(div().text_color(colors.text_secondary).child(dir))
                            .child(div().text_color(colors.text_primary).child(name)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(format!("· {count} {noun}")),
                    )
                    .child(div().flex_1())
                    .children(open),
            )
            .into_any_element()
    }

    fn render_card(
        &self,
        thread: usize,
        group: Group,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let older = match group {
            Group::File(file) => threads::is_older(&self.loaded, &self.loaded.files[file], thread),
            _ => false,
        };
        let excerpt =
            match group {
                Group::Pr => None,
                Group::Gone => Some(self.render_excerpt_note(
                    "This file is no longer changed in this pull request.",
                    cx,
                )),
                Group::File(file) => Some(self.render_excerpt(thread, file, older, cx)),
            };
        let can_open = matches!(group, Group::File(_));
        let card = self.render_thread_card(thread, older, focused, excerpt, can_open, cx);
        let label = card_label(&self.loaded, thread, group);
        div()
            .id(("thread-card", thread))
            .px(px(space::S16))
            .py(px(space::S6))
            .role(gpui::Role::ListItem)
            .aria_label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(item) = this
                    .polish
                    .items
                    .iter()
                    .position(|i| i.thread() == Some(thread))
                {
                    this.polish.focused = Some(item);
                    cx.notify();
                }
            }))
            .child(card.max_w(px(THREADS_MAX_WIDTH)))
            .into_any_element()
    }

    fn render_excerpt_note(
        &self,
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        div()
            .py(px(space::S4))
            .border_t_1()
            .border_b_1()
            .border_color(theme.colors.border_subtle)
            .text_color(theme.colors.text_tertiary)
            .type_style(typography::SMALL, &theme)
            .child(text.into())
            .into_any_element()
    }

    /// FR-10: the thread's code as read-only unified rows, its fixed height
    /// known before the diff is (placeholders until then).
    fn render_excerpt(
        &self,
        thread: usize,
        file: usize,
        older: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = &self.loaded.files[file];
        let t = &self.loaded.threads[thread];
        if older {
            let it = t.iteration.unwrap_or(self.loaded.iteration);
            return self
                .render_excerpt_note(format!("Code from iteration {it} isn't shown here."), cx);
        }
        if let Some(reason) = entry.collapsed {
            let text = match reason {
                Collapsed::Generated => "Generated file: code not loaded.",
                Collapsed::Binary => "Binary file.",
                Collapsed::Large => "Large file: code not loaded.",
            };
            return self.render_excerpt_note(text, cx);
        }
        if entry.unavailable.is_some() {
            let note = self.render_excerpt_note("Couldn't load this file's code.", cx);
            return div()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(div().flex_1().child(note))
                .child(
                    Button::new(("excerpt-retry", thread), "Retry")
                        .variant(ButtonVariant::Ghost)
                        .shortcut(kbd(RETRY_KEYS))
                        .on_press(compose::press(cx, move |this, _, cx| {
                            this.retry_file(file, cx)
                        })),
                )
                .into_any_element();
        }
        let rows = threads::excerpt_rows(t);
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let row_h = px(self.row_height);
        let frame = |content: Vec<AnyElement>| {
            div()
                .flex()
                .flex_col()
                .h(row_h * rows as f32)
                .overflow_hidden()
                .border_t_1()
                .border_b_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_code)
                .children(content)
                .into_any_element()
        };
        let Some(diff) = entry.diff.as_deref() else {
            if self.is_offline() && self.blobs.is_remote() {
                return self.render_excerpt_note("Offline", cx);
            }
            const WIDTHS: [f32; 6] = [0.62, 0.48, 0.71, 0.36, 0.55, 0.44];
            let bars = (0..rows)
                .map(|ix| {
                    div()
                        .h(row_h)
                        .flex()
                        .items_center()
                        .px(px(space::S8))
                        .child(
                            Skeleton::new(("excerpt-skeleton", thread * 16 + ix))
                                .width(THREADS_MAX_WIDTH * WIDTHS[ix % WIDTHS.len()])
                                .height(self.row_height * 0.5),
                        )
                        .into_any_element()
                })
                .collect();
            return frame(bars);
        };
        let line = entry
            .anchored
            .inline
            .iter()
            .find(|(_, t)| *t == thread)
            .map(|(line, _)| *line);
        let Some(end) = line else {
            return self.render_excerpt_note("This code isn't in the displayed diff.", cx);
        };
        let (side, start, last) = threads::anchor_range(t).unwrap_or((Side::Right, 1, 1));
        let excerpt = threads::excerpt(diff, end, side, last - start + 1, rows);
        let digits = digits(diff);
        let lang = entry.lang;
        let marker = colors.diff_comment;
        let mut content: Vec<AnyElement> = (0..excerpt.pad)
            .map(|_| div().h(row_h).into_any_element())
            .collect();
        content.extend(excerpt.lines.clone().map(|ix| {
            let l = diff.lines[ix];
            let marked = excerpt.marked.contains(&ix);
            div()
                .relative()
                .child(
                    UnifiedLine::new(
                        ("excerpt", thread * 100_000 + ix),
                        l.kind,
                        l.old,
                        l.new,
                        diff.text(ix).to_owned(),
                        lang,
                    )
                    .words(intraline(diff, ix))
                    .digits(digits),
                )
                .when(marked, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .w(px(2.0))
                            .bg(marker),
                    )
                })
                .into_any_element()
        }));
        frame(content)
    }

    // ── render: header (C, D, E) ─────────────────────────────────────────

    /// The header title: click copies it (FR-22).
    pub(super) fn render_title(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let title = self.loaded.pr().title.clone();
        div()
            .id("review-copy-title")
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .px(px(space::S4))
            .rounded(px(radius::SM))
            .cursor_pointer()
            .hover(|s| s.bg(colors.bg_hover))
            .text_color(colors.text_primary)
            .type_style(typography::HEADING, &theme)
            .role(gpui::Role::Button)
            .aria_label(format!("Copy title: {}", title.trim()))
            .tooltip(Tooltip::text("Copy title"))
            .on_click(cx.listener(|this, _, _, cx| this.copy_target(CopyTarget::Title, cx)))
            .child(title)
    }

    /// Source → target, each a copy target (FR-23).
    pub(super) fn render_branches(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let pr = self.loaded.pr();
        let branch = |id: &'static str, which: &str, target: CopyTarget, name: String| {
            div()
                .id(id)
                .px(px(space::S2))
                .rounded(px(radius::SM))
                .cursor_pointer()
                .hover(|s| s.bg(colors.bg_hover))
                .role(gpui::Role::Button)
                .aria_label(format!("Copy {which} branch {name}"))
                .tooltip(Tooltip::text(format!("Copy {name}")))
                .on_click(cx.listener(move |this, _, _, cx| this.copy_target(target, cx)))
                .child(name)
        };
        div()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .type_style(typography::CODE_INLINE, &theme)
            .text_color(colors.text_primary)
            .child(branch(
                "review-copy-source",
                "source",
                CopyTarget::SourceBranch,
                short_branch(&pr.source_branch).to_owned(),
            ))
            .child(
                Icon::new(IconName::ArrowRight)
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary),
            )
            .child(branch(
                "review-copy-target",
                "target",
                CopyTarget::TargetBranch,
                short_branch(&pr.target_branch).to_owned(),
            ))
    }

    /// FR-16: "Waiting on N required group(s)" at the end of the
    /// reviewers row while groups are hidden.
    pub(super) fn render_pending_groups(
        &self,
        show_groups: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pr = self.loaded.pr();
        let pending =
            crate::review::groups::pending_required_groups(pr, &self.loaded.people, show_groups);
        if pending.is_empty() {
            return None;
        }
        let colors = &cx.theme().colors;
        let tooltip = crate::review::groups::waiting_tooltip(&pending, &self.loaded.people);
        Some(
            div()
                .id("review-pending-groups")
                .flex()
                .items_center()
                .gap(px(space::S4))
                .aria_label(tooltip.clone())
                .tooltip(Tooltip::text(tooltip))
                .child(
                    Icon::new(IconName::Users)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(Chip::new(crate::review::groups::waiting_label(
                    pending.len(),
                )))
                .into_any_element(),
        )
    }

    /// FR-20: a draft with write access starts its actions row with
    /// Publish (`⇧P`).
    pub(super) fn render_publish(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pr = self.loaded.pr();
        let valid = self
            .status_actions()
            .contains(&crate::review::writes::StatusAction::Publish);
        if !pr.is_draft || !valid {
            return None;
        }
        let mine = pr.author == self.loaded.me;
        Some(
            Button::new("review-publish", "Publish")
                .variant(if mine {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                })
                .icon(IconName::GitPullRequest)
                .shortcut(kbd(DRAFT_KEYS))
                .loading(self.status_sending())
                .on_press(compose::press(cx, |this, w, cx| {
                    this.request_status(crate::review::writes::StatusAction::Publish, w, cx)
                }))
                .into_any_element(),
        )
    }

    // ── stories ──────────────────────────────────────────────────────────

    /// `[review] show_group_reviewers`, or the story's override (C1).
    pub(super) fn groups_shown(&self, cx: &App) -> bool {
        self.polish
            .show_groups
            .unwrap_or_else(|| crate::review::groups::shown(cx))
    }

    /// Stories: group reviewers shown or hidden whatever the setting.
    pub fn story_group_reviewers(&mut self, show: bool, cx: &mut Context<Self>) {
        self.polish.show_groups = Some(show);
        cx.notify();
    }

    /// Stories: `text` typed in the filter (and the toggle as given).
    pub fn story_filter(&mut self, text: &str, threads_only: bool, cx: &mut Context<Self>) {
        self.polish.state.threads_only = threads_only;
        self.polish
            .input
            .update(cx, |input, cx| input.set_text(text, cx));
        self.set_filter_text(text.to_owned(), cx);
        self.relist();
        cx.notify();
    }

    /// Stories: the Threads view in `scope`, its first (or last) card
    /// focused.
    pub fn story_threads(&mut self, scope: ThreadScope, last: bool, cx: &mut Context<Self>) {
        self.polish.state.scope = scope;
        self.apply_mode(ReviewMode::Threads, cx);
        self.first_or_last_card(last, cx);
    }

    /// Stories: drop the computed diff of every file but the selected one
    /// (excerpts show their placeholders, as before a dwell).
    pub fn story_uncomputed(&mut self) {
        for (ix, entry) in self.loaded.files.iter_mut().enumerate() {
            if ix != self.selected {
                entry.diff = None;
                entry.anchored = Default::default();
            }
        }
    }
}

/// A stable id per group for element ids.
fn ix_of(group: Group) -> usize {
    match group {
        Group::Pr => 0,
        Group::Gone => 1,
        Group::File(file) => file + 2,
    }
}

/// "Thread on src/pay/retry.rs lines 40 to 42, active, 2 comments, last
/// by Dana Ruiz 1h ago" (design §7).
fn card_label(loaded: &LoadedPr, thread: usize, group: Group) -> String {
    let t = &loaded.threads[thread];
    let place = match (group, threads::anchor_range(t)) {
        (Group::Pr, _) | (_, None) => "Thread on the pull request".to_owned(),
        (_, Some((_, start, end))) => {
            let path = threads::thread_path(t).unwrap_or_default();
            if end > start {
                format!("Thread on {path} lines {start} to {end}")
            } else {
                format!("Thread on {path} line {start}")
            }
        }
    };
    let comments: Vec<_> = t
        .comments
        .iter()
        .filter(|c| !c.is_system && !c.is_deleted)
        .collect();
    let status = match t.status {
        ThreadStatus::Fixed => "resolved",
        ThreadStatus::Pending => "pending",
        ThreadStatus::WontFix => "won't fix",
        ThreadStatus::Closed => "closed",
        ThreadStatus::ByDesign => "by design",
        ThreadStatus::Active | ThreadStatus::Unknown => "active",
    };
    let last = comments
        .last()
        .map(|c| {
            format!(
                ", last by {} {} ago",
                loaded.name(&c.author),
                data::age(loaded.now, c.created)
            )
        })
        .unwrap_or_default();
    format!(
        "{place}, {status}, {} comment{}{last}",
        comments.len(),
        if comments.len() == 1 { "" } else { "s" }
    )
}

/// A segment's click handler: the picked segment's index.
type PickSegment = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A small segmented control (design §1.2): `Role::RadioGroup` of
/// `Role::RadioButton` segments.
fn segments(
    id: &'static str,
    label: &'static str,
    options: impl Iterator<Item = (String, bool)>,
    pick: PickSegment,
    cx: &mut Context<PrReview>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme().clone();
    let colors = theme.colors;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(space::S2))
        .h(px(size::CONTROL_SM))
        .p(px(space::S2))
        .rounded(px(radius::SM))
        .border_1()
        .border_color(colors.border_default)
        .role(gpui::Role::RadioGroup)
        .aria_label(label)
        .type_style(typography::SMALL, &theme)
        .children(options.enumerate().map(move |(ix, (text, selected))| {
            let pick = pick.clone();
            div()
                .id(ix)
                .h_full()
                .px(px(space::S8))
                .flex()
                .items_center()
                .rounded(px(radius::XS))
                .cursor_pointer()
                .role(gpui::Role::RadioButton)
                .aria_label(text.clone())
                .aria_selected(selected)
                .when(selected, |d| {
                    d.bg(colors.bg_selected).text_color(colors.text_primary)
                })
                .when(!selected, |d| {
                    d.text_color(colors.text_secondary)
                        .hover(|s| s.bg(colors.bg_hover))
                })
                .on_click(move |_, window, cx| pick(ix, window, cx))
                .child(text)
        }))
}
