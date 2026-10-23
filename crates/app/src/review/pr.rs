//! One pull request under review (spec 0004): header, changed-files tree,
//! and the virtualized diff with inline threads.
//!
//! Everything renders from [`LoadedPr`], built off the UI thread. Diffs that
//! weren't computed eagerly are computed on the background executor, the
//! selected file first. For a signed-in org, missing blobs are fetched
//! first ([`Blobs`], spec 0004 FR-2), the selection's before the prefetch
//! set's (the first files and the selection's neighbors, FR-3, FR-15).
//! Loading states ([`super::loading`]) show after a grace: a spinner with
//! the file name and, when slow, why; tree skeleton rows while the file
//! list downloads; Retry on failure. Rows are laid out once per (file,
//! layout, folds) and the list paints only the visible ones (FR-4), so
//! frame cost doesn't grow with file size.
//!
//! Review writes (comment, reply, resolve, vote, complete, auto-complete)
//! live in [`compose`]: optimistic where the spec says so, sent through
//! [`super::writes::Access`], reverted with a reason on failure. Read-only
//! access renders no review control, only [`super::writes::READ_ONLY_HINT`].
//! Status changes (publish, mark as draft, abandon, reactivate) and linked
//! work items live in [`status_links`] (FR-16, FR-17).
//!
//! Spec 0019 lives in [`polish`]: the tree's file filter, the Threads view,
//! `n`/`p` across files, copy title / branches, the Publish button; group
//! reviewers follow [`super::groups`].
//!
//! Overlays (vote and status menus, the Complete sheet, confirmations, the
//! work item picker) close on `Esc` and on a click outside them, and hand
//! the keyboard back to the review.

mod compose;
pub mod history;
mod polish;
mod status_links;

pub use compose::{ComposerState, Overlay, SheetState};
pub use polish::{
    CLEAR_FILTER_KEYS, FILTER_KEYS, ReviewMode, ReviewPolishState, THREADS_ONLY_KEYS,
    THREADS_VIEW_KEYS,
};
pub(in crate::review) use status_links::{
    Handler, abandon_text, confirm_dialog, status_keys, status_menu_panel,
};

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use adoken_diff::rows::{SplitRow, UnifiedRow};
use adoken_diff::{FileDiff, LineKind, words};
use adoken_model::{PrStatus, ThreadStatus, Vote, short_branch};
use adoken_store::Store;
use adoken_theme::{radius, size, space, typography};
use adoken_ui::diff::{self, DiffCell, FoldLine, SplitLine, UnifiedLine};
use adoken_ui::file_tree::{FileTreeDir, FileTreeFile, TreeRow};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, Checkbox, Chip, ChipTone, Icon, IconName, Kbd, Skeleton,
    Spinner, TypeStyleExt,
};
use gpui::{
    AnyElement, App, AsyncApp, Context, FocusHandle, Focusable, Hsla, KeyBinding, ListAlignment,
    ListState, Task, UniformListScrollHandle, Window, actions, div, list, prelude::*, px,
    uniform_list,
};

use super::blobs::Blobs;
use super::data::{self, Collapsed, DiffJob, DiffOutcome, Layout, LoadedPr, PaneRow};
use super::loading::{self, FileLoad, ListLoad, Phase};
use super::web::{self, PrLink, WebAction};
use crate::avatar::{Avatar, AvatarSize};

/// `id`'s avatar (initials until the picture is cached, spec 0012).
fn person_avatar(loaded: &LoadedPr, id: &str, size: AvatarSize) -> Avatar {
    match loaded.people.get(id) {
        Some(person) => Avatar::person(person),
        None => Avatar::new(id.to_owned(), id.to_owned()),
    }
    .size(size)
}

/// Of `job`'s blobs, those a signed-in org must download first (stats the
/// cache on the background executor).
async fn missing_blobs(job: &DiffJob, blobs: &Blobs, cx: &mut AsyncApp) -> Vec<String> {
    if !blobs.is_remote() {
        return Vec::new();
    }
    let (ids, reader) = (job.blob_ids(), blobs.clone());
    cx.background_spawn(async move { reader.missing(&ids) })
        .await
}

/// One file's diff once its blobs are local, on the background executor.
/// Never on the UI thread.
async fn compute_diff(
    job: DiffJob,
    blobs: Blobs,
    force: bool,
    cx: &mut AsyncApp,
) -> Result<DiffOutcome, String> {
    let remote = blobs.is_remote();
    let outcome = cx
        .background_spawn(async move { job.run(&|id| blobs.read(id), force) })
        .await;
    match outcome {
        DiffOutcome::Missing if remote => Err("The file's contents aren't available.".into()),
        outcome => Ok(outcome),
    }
}

actions!(
    review,
    [
        /// Select the next file in the tree.
        NextFile,
        /// Select the previous file in the tree.
        PreviousFile,
        /// Mark the file viewed and move to the next one.
        MarkViewed,
        /// Jump to the next comment thread.
        NextThread,
        /// Jump to the previous comment thread.
        PreviousThread,
        /// Switch between unified and side-by-side diff.
        ToggleDiffLayout,
        /// Expand the next collapsed block of unchanged lines.
        ExpandContext,
        /// Move the diff line cursor down.
        NextLine,
        /// Move the diff line cursor up.
        PreviousLine,
        /// Comment on the line under the cursor.
        CommentOnLine,
        /// Comment on the whole pull request.
        CommentOnPullRequest,
        /// Reply to the selected thread.
        ReplyToThread,
        /// Resolve or reactivate the selected thread.
        ToggleResolved,
        /// Post the comment being written.
        PostComment,
        /// Close the comment composer (the draft is kept).
        CloseComposer,
        /// Retry loading the file list or the selected file.
        RetryLoading,
        /// Choose a vote…
        OpenVoteMenu,
        /// Vote: approve.
        Approve,
        /// Vote: approve with suggestions.
        ApproveWithSuggestions,
        /// Vote: wait for author.
        WaitForAuthor,
        /// Vote: reject.
        Reject,
        /// Reset my vote.
        ResetVote,
        /// Complete (merge) the pull request…
        OpenCompleteSheet,
        /// Set auto-complete…
        OpenAutoCompleteSheet,
        /// Cancel the auto-complete I set.
        CancelAutoComplete,
        /// Complete sheet: merge now.
        CompleteNow,
        /// Complete sheet: set auto-complete.
        SetAutoComplete,
        /// Complete sheet: next merge strategy.
        CycleMergeStrategy,
        /// Complete sheet: toggle deleting the source branch.
        ToggleDeleteSourceBranch,
        /// Complete sheet: toggle completing linked work items.
        ToggleTransitionWorkItems,
        /// Close the open menu, sheet, confirmation or picker.
        CloseOverlay,
        MenuNext,
        MenuPrevious,
        MenuConfirm,
        /// Change the pull request's status…
        OpenStatusMenu,
        /// Publish the draft, or mark the pull request as a draft.
        ToggleDraft,
        /// Abandon the pull request (asks first)…
        AbandonPr,
        /// Reactivate the abandoned pull request.
        ReactivatePr,
        /// Link a work item to the pull request…
        LinkWorkItem,
        /// Unlink a work item from the pull request…
        UnlinkWorkItem,
        /// Filter files…
        FocusFileFilter,
        /// Files: clear filter.
        ClearFileFilter,
        /// Only files with threads: turn on or off.
        ToggleThreadsOnly,
        /// Show threads or files.
        ToggleThreadsView,
        /// Threads: show active.
        ShowActiveThreads,
        /// Threads: show resolved.
        ShowResolvedThreads,
        /// Threads: show all.
        ShowAllThreads,
        /// Threads view: first thread.
        FirstThreadCard,
        /// Threads view: last thread.
        LastThreadCard,
        /// Filter field: next listed file.
        FilterNextFile,
        /// Filter field: previous listed file.
        FilterPreviousFile,
        /// Filter field: back to the review.
        FilterAccept,
        /// Filter field: clear, then leave.
        FilterEscape,
    ]
);

pub const KEY_CONTEXT: &str = "PrReview";
/// The review's context while a composer, menu or sheet has the keyboard:
/// single-letter review keys must not steal typed text.
const EDITING_CONTEXT: &str = "PrReviewEditing";
const COMPOSER_CONTEXT: &str = "ReviewComposer";
const MENU_CONTEXT: &str = "ReviewVoteMenu";
const STATUS_MENU_CONTEXT: &str = "ReviewStatusMenu";
const SHEET_CONTEXT: &str = "ReviewSheet";
const CONFIRM_CONTEXT: &str = "ReviewConfirm";
const PICKER_CONTEXT: &str = "ReviewWorkItemPicker";
pub const TOGGLE_LAYOUT_KEYS: &str = "secondary-shift-d";
pub const COMMENT_KEYS: &str = "c";
pub const RETRY_KEYS: &str = "shift-l";
pub const PR_COMMENT_KEYS: &str = "shift-c";
pub const REPLY_KEYS: &str = "r";
pub const RESOLVE_KEYS: &str = "shift-r";
pub const VOTE_KEYS: &str = "shift-v";
pub const COMPLETE_KEYS: &str = "shift-m";
pub const STATUS_KEYS: &str = "shift-s";
pub const DRAFT_KEYS: &str = "shift-p";
pub const ABANDON_KEYS: &str = "shift-a";
pub const LINK_WORK_ITEM_KEYS: &str = "shift-w";
pub const POST_KEYS: &str = "secondary-enter";
const AUTO_COMPLETE_KEYS: &str = "secondary-shift-enter";
const STRATEGY_KEYS: &str = "secondary-m";
const DELETE_BRANCH_KEYS: &str = "secondary-b";
const TRANSITION_KEYS: &str = "secondary-i";
/// Tree pane width.
const TREE_WIDTH: f32 = 300.0;
/// Placeholder tree while the file list downloads: (depth, name width).
const TREE_SKELETON: [(usize, f32); 7] = [
    (0, 96.0),
    (1, 148.0),
    (1, 120.0),
    (0, 72.0),
    (1, 164.0),
    (2, 110.0),
    (1, 136.0),
];

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    let editing = Some(EDITING_CONTEXT);
    let composer = Some(COMPOSER_CONTEXT);
    let menu = Some(MENU_CONTEXT);
    let status = Some(STATUS_MENU_CONTEXT);
    let sheet = Some(SHEET_CONTEXT);
    let confirm = Some(CONFIRM_CONTEXT);
    let picker = Some(PICKER_CONTEXT);
    let filter = Some(polish::FILTER_CONTEXT);
    vec![
        KeyBinding::new("j", NextFile, cx),
        KeyBinding::new("k", PreviousFile, cx),
        KeyBinding::new("v", MarkViewed, cx),
        KeyBinding::new("n", NextThread, cx),
        KeyBinding::new("p", PreviousThread, cx),
        KeyBinding::new(TOGGLE_LAYOUT_KEYS, ToggleDiffLayout, cx),
        KeyBinding::new("enter", ExpandContext, cx),
        KeyBinding::new("down", NextLine, cx),
        KeyBinding::new("up", PreviousLine, cx),
        KeyBinding::new(COMMENT_KEYS, CommentOnLine, cx),
        KeyBinding::new(PR_COMMENT_KEYS, CommentOnPullRequest, cx),
        KeyBinding::new(REPLY_KEYS, ReplyToThread, cx),
        KeyBinding::new(RESOLVE_KEYS, ToggleResolved, cx),
        KeyBinding::new(VOTE_KEYS, OpenVoteMenu, cx),
        KeyBinding::new(COMPLETE_KEYS, OpenCompleteSheet, cx),
        KeyBinding::new(STATUS_KEYS, OpenStatusMenu, cx),
        KeyBinding::new(DRAFT_KEYS, ToggleDraft, cx),
        KeyBinding::new(ABANDON_KEYS, AbandonPr, cx),
        KeyBinding::new(LINK_WORK_ITEM_KEYS, LinkWorkItem, cx),
        KeyBinding::new(RETRY_KEYS, RetryLoading, cx),
        KeyBinding::new(web::COPY_ID_KEYS, super::CopyPrId, cx),
        KeyBinding::new(web::COPY_LINK_KEYS, super::CopyPrLink, cx),
        KeyBinding::new(web::OPEN_KEYS, super::OpenPrInAdo, cx),
        // Spec 0019: file filter, threads only, Threads view.
        KeyBinding::new(polish::FILTER_KEYS, FocusFileFilter, cx),
        KeyBinding::new(polish::CLEAR_FILTER_KEYS, ClearFileFilter, cx),
        KeyBinding::new(polish::THREADS_ONLY_KEYS, ToggleThreadsOnly, cx),
        KeyBinding::new(polish::THREADS_VIEW_KEYS, ToggleThreadsView, cx),
        KeyBinding::new("home", FirstThreadCard, cx),
        KeyBinding::new("end", LastThreadCard, cx),
        KeyBinding::new("down", FilterNextFile, filter),
        KeyBinding::new("up", FilterPreviousFile, filter),
        KeyBinding::new("enter", FilterAccept, filter),
        KeyBinding::new("escape", FilterEscape, filter),
        // An overlay is open but something else has the keyboard (a click
        // outside it moved focus): Esc still closes it.
        KeyBinding::new("escape", CloseOverlay, editing),
        KeyBinding::new(POST_KEYS, PostComment, composer),
        KeyBinding::new("escape", CloseComposer, composer),
        KeyBinding::new("down", MenuNext, menu),
        KeyBinding::new("up", MenuPrevious, menu),
        KeyBinding::new("enter", MenuConfirm, menu),
        KeyBinding::new("escape", CloseOverlay, menu),
        KeyBinding::new("down", MenuNext, status),
        KeyBinding::new("up", MenuPrevious, status),
        KeyBinding::new("enter", MenuConfirm, status),
        KeyBinding::new("escape", CloseOverlay, status),
        KeyBinding::new(POST_KEYS, CompleteNow, sheet),
        KeyBinding::new(AUTO_COMPLETE_KEYS, SetAutoComplete, sheet),
        KeyBinding::new(STRATEGY_KEYS, CycleMergeStrategy, sheet),
        KeyBinding::new(DELETE_BRANCH_KEYS, ToggleDeleteSourceBranch, sheet),
        KeyBinding::new(TRANSITION_KEYS, ToggleTransitionWorkItems, sheet),
        KeyBinding::new("escape", CloseOverlay, sheet),
        KeyBinding::new("enter", MenuConfirm, confirm),
        KeyBinding::new(POST_KEYS, MenuConfirm, confirm),
        KeyBinding::new("escape", CloseOverlay, confirm),
        KeyBinding::new("down", MenuNext, picker),
        KeyBinding::new("up", MenuPrevious, picker),
        KeyBinding::new("enter", MenuConfirm, picker),
        KeyBinding::new("escape", CloseOverlay, picker),
    ]
}

/// Which part a [`PrReview`] paints: the app shows [`Part::All`]; gallery
/// stories render the header, tree or diff alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    All,
    Header,
    Tree,
    Diff,
}

/// What a loading indicator's re-render timer follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watched {
    FileList,
    File(usize),
}

pub struct PrReview {
    store: Option<Store>,
    blobs: Blobs,
    loaded: LoadedPr,
    selected: usize,
    layout: Layout,
    /// The user's explicit choice; `None` = follow window width (Q-3).
    preference: Option<Layout>,
    expanded: HashMap<usize, HashSet<u32>>,
    rows: Vec<PaneRow>,
    diff_list: ListState,
    /// Diff row height at the code font size (virtualization's uniform
    /// height); the list resets when a code font change moves it.
    row_height: f32,
    _code_font: gpui::Subscription,
    tree_scroll: UniformListScrollHandle,
    /// Row index of the thread `n`/`p` last landed on.
    thread_cursor: Option<usize>,
    /// Row index of the diff line `↑`/`↓` last landed on (`c` comments it).
    line_cursor: Option<usize>,
    /// Files whose diff is being computed right now.
    computing: HashSet<usize>,
    /// User-priority loads in progress (selection, prefetch, retry), for
    /// the diff pane's loading state.
    loads: HashMap<usize, FileLoad>,
    /// The file list's refresh at open: skeleton, "Loading files…", Retry.
    files_load: ListLoad,
    focus: FocusHandle,
    part: Part,
    /// Review writes ([`compose`]).
    writes: compose::WriteState,
    /// Status menu, confirmations, linked work items and their picker
    /// ([`status_links`]).
    links: status_links::LinkState,
    _background: Option<Task<()>>,
    /// The file list's refresh at open ([`Self::sync_files`]).
    _files: Option<Task<()>>,
    /// Bumped when the file list is replaced: results computed for the old
    /// list are dropped.
    generation: u64,
    /// Id and web address for Copy PR id / Copy link / Open in Azure
    /// DevOps ([`web`]); set by the list once the org's names are read.
    link: PrLink,
    _pins: Option<gpui::Subscription>,
    /// Spec 0019's filter, Threads view and their state ([`polish`]).
    polish: polish::Polish,
    /// `[review] show_group_reviewers` changes repaint the header.
    _groups: gpui::Subscription,
    /// Re-reads the PR's threads when sync lands (spec 0019 FR-9).
    _threads: Option<Task<()>>,
    /// Back / forward's diff position, waiting for the selected file's
    /// diff (spec 0018 FR-8).
    history_pending: Option<crate::shell::history::Pending<history::DiffRestore>>,
}

/// A link action's toast text ("Copied !4821"), for the page to show.
#[derive(Clone, Debug)]
pub struct Toasted(pub gpui::SharedString);

impl gpui::EventEmitter<Toasted> for PrReview {}

/// The header's back button: the Pull requests view shows its list again
/// (spec 0004 §3.1, one pane at a time).
#[derive(Clone, Debug)]
pub struct Back;

impl gpui::EventEmitter<Back> for PrReview {}

/// The PR changed here (a status change shown, confirmed or reverted): the
/// list's row follows.
#[derive(Clone, Debug)]
pub struct PrChanged(pub adoken_model::PullRequest);

impl gpui::EventEmitter<PrChanged> for PrReview {}

/// A linked work item was opened: the Work items view shows it.
#[derive(Clone, Copy, Debug)]
pub struct OpenWorkItem(pub i64);

impl gpui::EventEmitter<OpenWorkItem> for PrReview {}

impl PrReview {
    /// `store` is `None` in gallery stories: viewed state stays in memory and
    /// every diff must be computed already (`data::build` with all files
    /// eager) or is computed through `blobs`.
    pub fn new(
        store: Option<Store>,
        blobs: Blobs,
        loaded: LoadedPr,
        preference: Option<Layout>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let width = window.viewport_size().width / px(1.0);
        let selected = loaded.order.first().copied().unwrap_or(0);
        let link = PrLink {
            id: loaded.pr().id,
            url: None,
            demo: false,
        };
        let row_height = diff::row_height(cx.theme());
        let code_font = cx.observe_global::<adoken_ui::Theme>(|this: &mut Self, cx| {
            let height = diff::row_height(cx.theme());
            if height != this.row_height {
                this.row_height = height;
                this.refresh_rows();
                cx.notify();
            }
        });
        let diff_list = ListState::new(0, ListAlignment::Top, px(row_height * 20.0));
        // Scrolling the diff is a move: a pending Open in diff stays put.
        let weak = cx.entity().downgrade();
        diff_list.set_scroll_handler(move |_, _, cx| {
            weak.update(cx, |this, _| this.user_moved()).ok();
        });
        let polish = polish::Polish::new(&loaded, cx);
        let mut this = Self {
            store,
            blobs,
            loaded,
            selected,
            layout: Layout::resolve(preference, width),
            preference,
            expanded: HashMap::new(),
            rows: Vec::new(),
            diff_list,
            row_height,
            _code_font: code_font,
            tree_scroll: UniformListScrollHandle::new(),
            thread_cursor: None,
            line_cursor: None,
            computing: HashSet::new(),
            loads: HashMap::new(),
            files_load: ListLoad::Idle,
            focus: cx.focus_handle(),
            part: Part::All,
            writes: compose::WriteState::default(),
            links: status_links::LinkState::default(),
            _background: None,
            _files: None,
            generation: 0,
            link,
            _pins: crate::pin_actions::observe(cx),
            polish,
            _groups: super::groups::observe(cx),
            _threads: None,
            history_pending: None,
        };
        this.rebuild_rows();
        this.load_around_selection(cx);
        this.compute_remaining(cx);
        this.restore_draft(window, cx);
        this.watch_completing(cx);
        this.load_work_items(cx);
        this.watch_threads(cx);
        this
    }

    /// Spec 0019 FR-9: threads that arrive or change by sync while the
    /// review is open show in place (cards keep their screen position).
    fn watch_threads(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self._threads = Some(crate::browse::reload_on_commit(
            &store,
            cx,
            |this: &mut Self, _, cx| this.reread_threads(cx),
        ));
    }

    fn reread_threads(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let id = self.pr_id();
        let read = cx.background_spawn(async move {
            store.with_conn(|c| adoken_store::read::pr_threads(c, id))
        });
        cx.spawn(async move |this, cx| {
            let Ok(fresh) = read.await else {
                return;
            };
            this.update(cx, |this, cx| this.merge_threads(fresh, cx))
                .ok();
        })
        .detach();
    }

    /// Merges a fresh read of the threads (stories and tests call it with
    /// their own).
    pub fn merge_threads(&mut self, fresh: Vec<adoken_model::PrThread>, cx: &mut Context<Self>) {
        let Some(added) = data::merge_threads(&mut self.loaded, fresh) else {
            return;
        };
        // New threads need placing against their file's diff.
        for file in added {
            if self.loaded.files[file].diff.is_some() {
                self.compute(file, false, cx);
            }
        }
        self.threads_changed();
        self.refresh_rows();
        cx.notify();
    }

    pub fn part(mut self, part: Part) -> Self {
        self.part = part;
        self
    }

    /// Where this PR lives on the web ([`web::Directory::link`]).
    pub fn set_link(&mut self, link: PrLink, cx: &mut Context<Self>) {
        self.link = link;
        cx.notify();
    }

    /// Copy PR id / Copy link / Open in Azure DevOps for this PR; the
    /// outcome goes to the page's toast ([`Toasted`]).
    fn web_action(&mut self, action: WebAction, cx: &mut Context<Self>) {
        if matches!(action, WebAction::Open) && self.link.open_blocker().is_none() {
            crate::pin_actions::mark_opened(adoken_model::PinSubject::Pr(self.pr_id()), cx);
        }
        if let Some(text) = self.link.run(action, cx) {
            cx.emit(Toasted(text));
        }
    }

    pub fn pr_id(&self) -> i64 {
        self.loaded.pr().id
    }

    pub fn pr_title(&self) -> &str {
        &self.loaded.pr().title
    }

    pub fn loaded(&self) -> &LoadedPr {
        &self.loaded
    }

    /// The file list's refresh at open (idle, loading, failed).
    pub fn file_list_load(&self) -> &ListLoad {
        &self.files_load
    }

    /// A user-priority load of `file` is underway (the diff pane shows
    /// its loading state).
    pub fn is_loading(&self, file: usize) -> bool {
        self.loads.contains_key(&file)
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The file with the most inline threads (stories open on it).
    pub fn busiest_file(&self) -> usize {
        self.loaded
            .order
            .iter()
            .copied()
            .max_by_key(|&file| {
                let entry = &self.loaded.files[file];
                (entry.anchored.inline.len(), entry.anchored.older.len())
            })
            .unwrap_or(0)
    }

    fn rebuild_rows(&mut self) {
        let empty = HashSet::new();
        let expanded = self.expanded.get(&self.selected).unwrap_or(&empty);
        let composer = self.composer_line(self.selected);
        self.rows = match self.loaded.files.get(self.selected) {
            Some(entry) => data::pane_rows(entry, self.layout, expanded, composer),
            None => Vec::new(),
        };
        self.thread_cursor = None;
        self.line_cursor = None;
        self.diff_list
            .reset_with_uniform_height(self.rows.len(), px(self.row_height));
    }

    /// Rows changed (composer opened/closed, a thread arrived) without the
    /// file changing: keep the scroll position and cursors.
    fn refresh_rows(&mut self) {
        let top = self.diff_list.logical_scroll_top();
        let line = self.line_cursor.and_then(|ix| self.rows.get(ix).copied());
        let thread = self.thread_cursor.and_then(|ix| self.rows.get(ix).copied());
        self.rebuild_rows();
        self.line_cursor = line.and_then(|row| self.rows.iter().position(|r| *r == row));
        self.thread_cursor = thread.and_then(|row| self.rows.iter().position(|r| *r == row));
        self.diff_list.scroll_to(top);
    }

    pub fn select_file(&mut self, file: usize, cx: &mut Context<Self>) {
        if file >= self.loaded.files.len() {
            return;
        }
        self.user_moved();
        self.selected = file;
        self.rebuild_rows();
        if let Some(pos) = self.listed_position(file) {
            self.tree_scroll
                .scroll_to_item(pos, gpui::ScrollStrategy::Center);
        }
        self.load_around_selection(cx);
        cx.notify();
    }

    /// The selected file first, then FR-3's prefetch set ([`data::load_plan`])
    /// once the selection's blobs are in: its downloads never queue behind
    /// the prefetch's. Collapsed files cost nothing until expanded (AC-12).
    fn load_around_selection(&mut self, cx: &mut Context<Self>) {
        let mut plan = data::load_plan(&self.loaded, self.selected).into_iter();
        if let Some(first) = plan.next() {
            self.compute_then(first, false, plan.collect(), cx);
        }
    }

    /// `j`/`k`: the next / previous file the tree lists (spec 0019 FR-4).
    fn step_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.step_listed(delta, cx);
    }

    /// Computes one file's diff at user priority (selection, prefetch,
    /// "Load anyway", retry), fetching its missing blobs first.
    fn compute(&mut self, file: usize, force: bool, cx: &mut Context<Self>) {
        self.compute_then(file, force, Vec::new(), cx);
    }

    /// [`Self::compute`]; the files in `then` start once this file's blobs
    /// are local (or couldn't be fetched).
    fn compute_then(&mut self, file: usize, force: bool, then: Vec<usize>, cx: &mut Context<Self>) {
        self.loads.entry(file).or_insert(FileLoad {
            since: Instant::now(),
            phase: Phase::Computing,
        });
        if !self.computing.insert(file) {
            // Already underway (a background pass may hand it back, see
            // `compute_remaining`).
            for next in then {
                self.compute(next, false, cx);
            }
            return;
        }
        self.tick_while(
            move |this| {
                this.loads
                    .contains_key(&file)
                    .then_some(Watched::File(file))
            },
            cx,
        );
        let job = self.loaded.job(file);
        let blobs = self.blobs.clone();
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let missing = missing_blobs(&job, &blobs, cx).await;
            let fetched = if missing.is_empty() {
                Ok(())
            } else {
                let blobs_n = missing.len();
                this.update(cx, |this, cx| {
                    this.set_phase(file, generation, Phase::Fetching { blobs: blobs_n }, cx)
                })
                .ok();
                let fetched = cx.update(|cx| blobs.fetch(missing, cx)).await;
                this.update(cx, |this, cx| {
                    this.set_phase(file, generation, Phase::Computing, cx)
                })
                .ok();
                fetched
            };
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    for next in then {
                        this.compute(next, false, cx);
                    }
                }
            })
            .ok();
            let result = match fetched {
                Ok(()) => compute_diff(job, blobs, force, cx).await,
                Err(reason) => Err(reason),
            };
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.computing.remove(&file);
                this.loads.remove(&file);
                match result {
                    Ok(outcome) => this.apply(file, outcome, cx),
                    Err(reason) => {
                        this.loaded.files[file].unavailable = Some(reason);
                        if file == this.selected {
                            this.rebuild_rows();
                            this.try_history_restore();
                        }
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn set_phase(&mut self, file: usize, generation: u64, phase: Phase, cx: &mut Context<Self>) {
        if self.generation != generation {
            return;
        }
        if let Some(load) = self.loads.get_mut(&file) {
            load.phase = phase;
            if file == self.selected {
                cx.notify();
            }
        }
    }

    /// Re-render at a load's grace and slow marks (the indicator's steps),
    /// then every second while it lasts, so the wait's reason stays current
    /// without spinner frames (reduce motion). `loading` names what is
    /// still loading, `None` once done.
    fn tick_while(
        &self,
        loading: impl Fn(&Self) -> Option<Watched> + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let mut at = Duration::ZERO;
            let marks = [loading::GRACE, loading::SLOW]
                .into_iter()
                .chain((1..).map(|s| loading::SLOW + Duration::from_secs(s)));
            for mark in marks {
                cx.background_executor().timer(mark - at).await;
                at = mark;
                let going = this.update(cx, |this, cx| {
                    let Some(watched) = loading(this) else {
                        return false;
                    };
                    if watched == Watched::FileList || watched == Watched::File(this.selected) {
                        cx.notify();
                    }
                    true
                });
                if !going.unwrap_or(false) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Remaining files in tree order, one background job at a time, so
    /// counts fill in while the first diff is already on screen. Only
    /// blobs already cached: the rest wait for selection or prefetch.
    fn compute_remaining(&mut self, cx: &mut Context<Self>) {
        let pending: Vec<usize> = self
            .loaded
            .order
            .iter()
            .copied()
            .filter(|&f| {
                let entry = &self.loaded.files[f];
                entry.diff.is_none() && entry.collapsed.is_none()
            })
            .collect();
        if pending.is_empty() {
            return;
        }
        let blobs = self.blobs.clone();
        let generation = self.generation;
        self._background = Some(cx.spawn(async move |this, cx| {
            for file in pending {
                let Ok(Some(job)) = this.update(cx, |this, _| {
                    let entry = &this.loaded.files[file];
                    (this.generation == generation
                        && entry.diff.is_none()
                        && entry.collapsed.is_none()
                        && this.computing.insert(file))
                    .then(|| this.loaded.job(file))
                }) else {
                    continue;
                };
                let blobs = blobs.clone();
                let outcome = cx
                    .background_spawn(async move {
                        blobs
                            .missing(&job.blob_ids())
                            .is_empty()
                            .then(|| job.run(&|id| blobs.read(id), false))
                    })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.generation != generation {
                            return;
                        }
                        this.computing.remove(&file);
                        match outcome {
                            Some(outcome) => {
                                this.loads.remove(&file);
                                this.apply(file, outcome, cx);
                            }
                            // Its blobs aren't local and the selection or
                            // prefetch asked for it meanwhile: fetch now.
                            None if this.loads.contains_key(&file) => {
                                this.compute(file, false, cx);
                            }
                            None => {}
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    fn apply(&mut self, file: usize, outcome: DiffOutcome, cx: &mut Context<Self>) {
        data::apply_diff(&mut self.loaded, file, outcome);
        if file == self.selected {
            self.rebuild_rows();
            self.try_restore_pending(cx);
            self.try_history_restore();
            self.try_land(cx);
        }
        if self.review_mode() == ReviewMode::Threads {
            // Placement decides which cards are on older code (FR-8).
            self.threads_changed();
        }
        cx.notify();
    }

    /// Spec 0004 FR-1 on open: refresh the file list when the stored one is
    /// missing or older than the PR's last push (user priority), then show
    /// the new list in place, keeping the selected path. The tree shows
    /// skeleton rows / "Loading files…" meanwhile, and Retry on failure.
    pub fn sync_files(&mut self, cx: &mut Context<Self>) {
        let (Some(files), Some(store)) = (self.blobs.files(), self.store.clone()) else {
            return;
        };
        let pr = self.loaded.pr().clone();
        let id = pr.id;
        self.files_load.start(Instant::now());
        self.tick_while(
            |this| this.files_load.is_loading().then_some(Watched::FileList),
            cx,
        );
        cx.notify();
        self._files = Some(cx.spawn(async move |this, cx| {
            let synced = cx
                .update(|cx| super::blobs::sync_files(files, pr, cx))
                .await;
            let detail = match synced {
                Ok(true) => cx
                    .background_spawn(async move { store.pr(id) })
                    .await
                    .map_err(|error| error.to_string()),
                Ok(false) => Ok(None),
                Err(error) => Err(error),
            };
            this.update(cx, |this, cx| {
                match detail {
                    Ok(detail) => {
                        this.files_load.finish(Ok(()));
                        if let Some(detail) = detail {
                            this.set_files(detail, cx);
                        }
                    }
                    Err(error) => {
                        log::warn!("review: couldn't sync the files of !{id}: {error}");
                        this.files_load.finish(Err(error));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// `⇧L` / Retry: the file list if its refresh failed, else the selected
    /// file if its blobs couldn't be fetched.
    fn retry_loading(&mut self, _: &RetryLoading, _: &mut Window, cx: &mut Context<Self>) {
        if self.files_load.failure().is_some() {
            self.sync_files(cx);
        } else if self
            .loaded
            .files
            .get(self.selected)
            .is_some_and(|f| f.unavailable.is_some())
        {
            self.retry_file(self.selected, cx);
        }
    }

    fn retry_file(&mut self, file: usize, cx: &mut Context<Self>) {
        self.loaded.files[file].unavailable = None;
        if file == self.selected {
            self.rebuild_rows();
        }
        self.compute(file, false, cx);
        cx.notify();
    }

    /// A new file list (a push): the filter and Threads view re-apply to it
    /// (spec 0019 FR-6).
    pub fn set_files(&mut self, detail: adoken_model::PrDetail, cx: &mut Context<Self>) {
        let path = self.loaded.files.get(self.selected).map(|f| f.path.clone());
        data::set_files(&mut self.loaded, detail);
        self.files_changed();
        self.generation += 1;
        self.computing.clear();
        self.loads.clear();
        self.expanded.clear();
        self.selected = path
            .and_then(|p| self.loaded.files.iter().position(|f| f.path == p))
            .or_else(|| self.loaded.order.first().copied())
            .unwrap_or(0);
        self.rebuild_rows();
        self.load_around_selection(cx);
        self.compute_remaining(cx);
        cx.notify();
    }

    /// Diff pane row count and scrolling, for frame measurements.
    pub fn diff_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn scroll_diff_to(&mut self, row: usize, cx: &mut Context<Self>) {
        self.diff_list.scroll_to(gpui::ListOffset {
            item_ix: row.min(self.rows.len().saturating_sub(1)),
            offset_in_item: px(0.0),
        });
        cx.notify();
    }

    /// Stories only: a PR opened before its file list was synced, the
    /// download past the grace (skeleton rows, "Loading files…"); or, with
    /// `failed`, the download failed (Retry).
    pub fn story_files_loading(&mut self, failed: Option<&str>) {
        let mut detail = self.loaded.detail.clone();
        detail.files.clear();
        data::set_files(&mut self.loaded, detail);
        self.files_changed();
        self.selected = 0;
        self.rebuild_rows();
        self.files_load = match failed {
            Some(error) => ListLoad::Failed(error.to_owned()),
            None => ListLoad::Loading {
                since: Instant::now() - loading::GRACE,
            },
        };
    }

    /// Stories only: the selected file's two blobs downloading for
    /// `waited` (past [`loading::SLOW`] the pane says why).
    pub fn story_diff_loading(&mut self, waited: Duration) {
        let file = self.selected;
        let entry = &mut self.loaded.files[file];
        entry.diff = None;
        entry.anchored = Default::default();
        self.loads.insert(
            file,
            FileLoad {
                since: Instant::now() - waited,
                phase: Phase::Fetching { blobs: 2 },
            },
        );
        self.rebuild_rows();
    }

    /// Stories only: mark a file viewed without a store or notify.
    pub fn story_mark_viewed(&mut self, file: usize) {
        let entry = &self.loaded.files[file];
        let key = (entry.path.clone(), entry.object_id().to_owned());
        self.loaded.viewed.insert(key);
    }

    fn toggle_viewed(&mut self, file: usize, cx: &mut Context<Self>) -> bool {
        let entry = &self.loaded.files[file];
        let key = (entry.path.clone(), entry.object_id().to_owned());
        let viewed = !self.loaded.viewed.contains(&key);
        if viewed {
            self.loaded.viewed.insert(key.clone());
        } else {
            self.loaded.viewed.remove(&key);
        }
        if let Some(store) = self.store.clone() {
            let pr = self.pr_id();
            cx.background_spawn(async move {
                if let Err(error) = data::set_viewed(&store, pr, &key.0, &key.1, viewed) {
                    log::warn!("review: couldn't save viewed state: {error}");
                }
            })
            .detach();
        }
        cx.notify();
        viewed
    }

    fn mark_viewed(&mut self, _: &MarkViewed, _: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_viewed(self.selected, cx) {
            self.step_file(1, cx);
        }
    }

    pub fn toggle_layout(&mut self, cx: &mut Context<Self>) {
        self.layout = self.layout.toggled();
        self.preference = Some(self.layout);
        self.rebuild_rows();
        if let Some(store) = self.store.clone() {
            let layout = self.layout;
            cx.background_spawn(async move {
                if let Err(error) = data::set_layout_preference(&store, layout) {
                    log::warn!("review: couldn't save diff layout: {error}");
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// `n`/`p`: the next / previous thread in the file, then on into the
    /// next / previous listed file with a thread in scope (spec 0019 FR-13).
    fn step_thread(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.user_moved();
        let threads: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, PaneRow::Thread(_) | PaneRow::Older(_)))
            .map(|(ix, _)| ix)
            .collect();
        let next = match (self.thread_cursor, forward) {
            (None, true) => threads.first(),
            (None, false) => threads.last(),
            (Some(at), true) => threads.iter().find(|ix| **ix > at),
            (Some(at), false) => threads.iter().rev().find(|ix| **ix < at),
        };
        match next {
            Some(&ix) => {
                self.thread_cursor = Some(ix);
                self.diff_list.scroll_to_reveal_item(ix);
                cx.notify();
            }
            None => self.step_thread_across(forward, cx),
        }
    }

    fn expand(&mut self, fold_start: u32, cx: &mut Context<Self>) {
        self.expanded
            .entry(self.selected)
            .or_default()
            .insert(fold_start);
        let top = self.diff_list.logical_scroll_top();
        self.rebuild_rows();
        self.diff_list.scroll_to(top);
        cx.notify();
    }

    fn expand_context(&mut self, _: &ExpandContext, _: &mut Window, cx: &mut Context<Self>) {
        let top = self.diff_list.logical_scroll_top().item_ix;
        let fold = self.rows[top.min(self.rows.len())..]
            .iter()
            .find_map(|row| match row {
                PaneRow::Unified(UnifiedRow::Fold(fold)) | PaneRow::Split(SplitRow::Fold(fold)) => {
                    Some(fold.start)
                }
                _ => None,
            });
        if let Some(start) = fold {
            self.expand(start, cx);
        }
    }

    // ── render ───────────────────────────────────────────────────────────

    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let actions = self.render_actions(cx);
        let status_chip = self.render_status_chip(cx);
        let work_items = self.render_work_items(cx);
        let title = self.render_title(cx);
        let branches = self.render_branches(cx);
        // Spec 0019 C1: group reviewers hidden unless the setting shows them;
        // pending required groups leave one chip.
        let show_groups = self.groups_shown(cx);
        let pending_groups = self.render_pending_groups(show_groups, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let pr = self.loaded.pr();
        let (status_icon, status_color, _) = pr_status(pr.status, pr.is_draft, colors);
        let shown = super::groups::visible_reviewers(pr, &self.loaded.people, show_groups);
        let reviewers = shown.map(|reviewer| {
            let (icon, color) = vote_icon(reviewer.vote, colors);
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .child(Icon::new(icon).size(size::ICON_SM).color(color))
                .child(person_avatar(
                    &self.loaded,
                    &reviewer.person,
                    AvatarSize::Sm,
                ))
                .child(
                    div()
                        .text_color(colors.text_primary)
                        .child(self.loaded.name(&reviewer.person)),
                )
                .child(
                    div()
                        .text_color(colors.text_tertiary)
                        .child(reviewer.vote.label()),
                )
                .when(reviewer.is_required, |this| {
                    this.child(Chip::new("Required"))
                })
        });
        let meta = |text: String| div().text_color(colors.text_secondary).child(text);
        // Copy PR id / Copy link / Open in Azure DevOps (user decision
        // 2026-09-30): the `!id` copies itself; the link actions sit at the
        // end of the title row with their keys.
        let kbd = |keys| Kbd::parse(keys).expect("static binding");
        let entity = cx.entity().downgrade();
        let on = move |action: WebAction| {
            let entity = entity.clone();
            move |_: &mut Window, cx: &mut App| {
                entity
                    .update(cx, |this, cx| this.web_action(action, cx))
                    .ok();
            }
        };
        let id_text = self.link.id_text();
        // One pane at a time (spec 0004 §3.1, user decision 2026-09-30):
        // back to the list, same row selected.
        let back = Button::new("review-back", "Pull requests")
            .variant(ButtonVariant::Ghost)
            .icon(IconName::ChevronLeft)
            .shortcut(kbd(super::BACK_KEYS))
            .tooltip(format!(
                "Back to the list · {}",
                kbd(super::BACK_KEYS).label()
            ))
            .on_press({
                let entity = cx.entity().downgrade();
                move |_, cx| {
                    entity.update(cx, |_, cx| cx.emit(Back)).ok();
                }
            });
        let copy_id = div()
            .id("review-copy-id")
            .flex_none()
            .px(px(space::S4))
            .rounded(px(radius::SM))
            .cursor_pointer()
            .hover(|s| s.bg(colors.bg_hover))
            .text_color(colors.text_secondary)
            .type_style(typography::CODE_INLINE, theme)
            .role(gpui::Role::Button)
            .aria_label(format!("Copy pull request id {id_text}"))
            .tooltip(adoken_ui::Tooltip::text(format!(
                "Copy {id_text} · {}",
                kbd(web::COPY_ID_KEYS).label()
            )))
            .on_click({
                let press = on(WebAction::CopyId);
                move |_, window, cx| press(window, cx)
            })
            .child(id_text.clone());
        let copy_blocker = self.link.copy_blocker();
        let open_blocker = self.link.open_blocker();
        let copy_link = Button::new("review-copy-link", "Copy link")
            .variant(ButtonVariant::Ghost)
            .icon(IconName::Copy)
            .shortcut(kbd(web::COPY_LINK_KEYS))
            .disabled(copy_blocker.is_some())
            .tooltip(copy_blocker.unwrap_or("Copy this pull request's web address"))
            .on_press(on(WebAction::CopyLink));
        let open = Button::new("review-open-ado", "Open")
            .variant(ButtonVariant::Ghost)
            .icon(IconName::ExternalLink)
            .shortcut(kbd(web::OPEN_KEYS))
            .disabled(open_blocker.is_some())
            .tooltip(match open_blocker {
                Some(why) => gpui::SharedString::from(why),
                None => format!("Open {id_text} in Azure DevOps").into(),
            })
            .on_press(on(WebAction::Open));
        div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .px(px(space::S16))
            .py(px(space::S12))
            .bg(colors.bg_surface)
            .border_b_1()
            .border_color(colors.border_subtle)
            .type_style(typography::SMALL, theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(back)
                    .child(
                        Icon::new(status_icon)
                            .size(size::ICON_LG)
                            .color(status_color),
                    )
                    .child(copy_id)
                    .child(title)
                    .child(copy_link)
                    .child(crate::pin_actions::button(
                        adoken_model::PinSubject::Pr(pr.id),
                        pr.title.clone().into(),
                        cx,
                    ))
                    .child(open),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(space::S8))
                    .child(status_chip)
                    .when_some(super::writes::merge_problem(pr), |this, why| {
                        this.child(Chip::new(why).tone(ChipTone::Danger))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(space::S4))
                            .child(person_avatar(&self.loaded, &pr.author, AvatarSize::Sm))
                            .child(meta(self.loaded.name(&pr.author))),
                    )
                    .child(branches)
                    .child(meta(format!(
                        "Updated {}",
                        data::age(self.loaded.now, pr.updated)
                    )))
                    .child(meta(format!("Iteration {}", self.loaded.iteration))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(space::S12))
                    .children(reviewers)
                    .children(pending_groups),
            )
            .children(work_items)
            .child(
                div()
                    .self_start()
                    .child(actions)
                    .children(adoken_ui::tour::anchor(super::anchors::ACTIONS, cx)),
            )
            .children(adoken_ui::tour::anchor(super::anchors::HEADER, cx))
    }

    pub(super) fn render_tree(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let failure = self.render_files_failure(cx);
        let files = self.loaded.files.len();
        let indicating = self.files_load.shows_indicator(Instant::now());
        let skeleton = indicating && files == 0;
        // No list yet (loading or failed): no counts to show.
        let unknown = files == 0 && self.files_load != ListLoad::Idle;
        let header = self.render_tree_header(indicating, unknown, window, cx);
        let empty = self.render_tree_empty(cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        // The tree is a tour anchor once its files list (spec 0020).
        let tour_anchor = (!skeleton)
            .then(|| adoken_ui::tour::anchor(super::anchors::FILE_TREE, cx))
            .flatten();
        div()
            .children(tour_anchor)
            .flex()
            .flex_col()
            .h_full()
            .bg(colors.bg_sidebar)
            .child(header)
            .children(failure)
            .child(if skeleton {
                div()
                    .flex_1()
                    .children(
                        TREE_SKELETON
                            .iter()
                            .enumerate()
                            .map(|(ix, &(depth, width))| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(space::S6))
                                    .h(px(size::CONTROL_MD))
                                    .pl(px(space::S12 + depth as f32 * space::S12))
                                    .child(
                                        Skeleton::new(("tree-skeleton-icon", ix))
                                            .width(size::ICON_SM)
                                            .height(size::ICON_SM),
                                    )
                                    .child(
                                        Skeleton::new(("tree-skeleton-name", ix))
                                            .width(width)
                                            .height(typography::SMALL.line_height - space::S4),
                                    )
                            }),
                    )
                    .into_any_element()
            } else if let Some(empty) = empty {
                div().flex_1().child(empty).into_any_element()
            } else {
                uniform_list(
                    "review-tree",
                    self.tree_rows().len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|ix| this.render_tree_row(ix, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.tree_scroll)
                .flex_1()
                .into_any_element()
            })
    }

    /// The file list's refresh failed: why, and Retry (`⇧L`). A stale list
    /// stays below it.
    fn render_files_failure(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let error = self.files_load.failure()?;
        let theme = cx.theme();
        let entity = cx.entity().downgrade();
        Some(
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(space::S8))
                .px(px(space::S12))
                .py(px(space::S8))
                .border_b_1()
                .border_color(theme.colors.border_subtle)
                .text_color(theme.colors.text_secondary)
                .type_style(typography::SMALL, theme)
                .child(format!("Couldn't load the file list. {error}"))
                .child(
                    Button::new("retry-files", "Retry")
                        .shortcut(Kbd::parse(RETRY_KEYS).expect("static binding"))
                        .on_press(move |_, cx| {
                            entity.update(cx, |this, cx| this.sync_files(cx)).ok();
                        }),
                ),
        )
    }

    /// Listed tree row `ix` (spec 0019 FR-3): its own name's matches
    /// highlighted; folders listed only for what they hold are muted.
    fn render_tree_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(&row) = self.tree_rows().get(ix) else {
            return div().into_any_element();
        };
        let filtering = !self.filter_text().trim().is_empty();
        match &self.loaded.tree[row] {
            TreeRow::Dir { name, depth } => {
                let highlights = self.row_highlights(name);
                FileTreeDir::new(name.clone(), *depth)
                    .muted(filtering && highlights.is_empty())
                    .highlights(highlights)
                    .into_any_element()
            }
            TreeRow::File { index, name, depth } => {
                let file = *index;
                let entry = &self.loaded.files[file];
                let mut row = FileTreeFile::new(
                    ("review-file", file),
                    name.clone(),
                    *depth,
                    data::change_kind(entry.change.change_type),
                )
                .comments(entry.thread_count)
                .viewed(self.loaded.is_viewed(file))
                .selected(self.tree_highlighted() == Some(file))
                .highlights(self.row_highlights(name));
                if let Some(diff) = entry.diff.as_deref() {
                    row = row.counts(diff.additions, diff.deletions);
                }
                let select = cx.entity().downgrade();
                let toggle = select.clone();
                row.on_select(move |_, cx| {
                    select
                        .update(cx, |this, cx| match this.review_mode() {
                            ReviewMode::Files => this.select_file(file, cx),
                            ReviewMode::Threads => this.focus_file_group(file, cx),
                        })
                        .ok();
                })
                .on_toggle_viewed(move |_, cx| {
                    toggle
                        .update(cx, |this, cx| {
                            this.toggle_viewed(file, cx);
                        })
                        .ok();
                })
                .into_any_element()
            }
        }
    }

    fn render_file_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let switch = self.render_mode_switch(cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let Some(entry) = self.loaded.files.get(self.selected) else {
            return div();
        };
        let viewed = self.loaded.is_viewed(self.selected);
        let kind = data::change_kind(entry.change.change_type);
        let (layout_label, layout_icon) = match self.layout {
            Layout::Unified => ("Side by side", IconName::Columns2),
            Layout::Split => ("Unified", IconName::Rows2),
        };
        let path = match &entry.change.original_path {
            Some(from) => format!("{} → {}", data::normalize_path(from), entry.path),
            None => entry.path.clone(),
        };
        let entity = cx.entity().downgrade();
        let viewed_entity = entity.clone();
        let file = self.selected;
        div()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .h(px(size::CONTROL_LG))
            .px(px(space::S12))
            .bg(colors.bg_surface)
            .border_b_1()
            .border_color(colors.border_subtle)
            .type_style(typography::SMALL, theme)
            .child(switch)
            .child(Chip::new(kind.label()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(colors.text_primary)
                    .type_style(typography::CODE_INLINE, theme)
                    .child(path),
            )
            .children(entry.diff.as_deref().map(|diff| {
                div()
                    .flex()
                    .flex_none()
                    .gap(px(space::S4))
                    .type_style(typography::CODE_INLINE, theme)
                    .child(
                        div()
                            .text_color(colors.diff_add_gutter)
                            .child(format!("+{}", diff.additions)),
                    )
                    .child(
                        div()
                            .text_color(colors.diff_del_gutter)
                            .child(format!("−{}", diff.deletions)),
                    )
            }))
            .child(
                div()
                    .id("file-viewed")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(space::S4))
                    .cursor_pointer()
                    .text_color(colors.text_secondary)
                    .on_click(move |_, _, cx| {
                        viewed_entity
                            .update(cx, |this, cx| {
                                this.toggle_viewed(file, cx);
                            })
                            .ok();
                    })
                    .child(Checkbox::new(viewed))
                    .child("Viewed")
                    .child(Kbd::parse("v").expect("static binding")),
            )
            .child(
                Button::new("toggle-layout", layout_label)
                    .variant(ButtonVariant::Ghost)
                    .icon(layout_icon)
                    .shortcut(Kbd::parse(TOGGLE_LAYOUT_KEYS).expect("static binding"))
                    .on_press(move |_, cx| {
                        entity.update(cx, |this, cx| this.toggle_layout(cx)).ok();
                    }),
            )
    }

    pub(super) fn render_diff(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.review_mode() == ReviewMode::Threads {
            return self.render_threads(cx);
        }
        let colors = &cx.theme().colors;
        div()
            .children(adoken_ui::tour::anchor(super::anchors::DIFF, cx))
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .bg(colors.bg_code)
            .child(self.render_file_bar(cx))
            .child(
                list(
                    self.diff_list.clone(),
                    cx.processor(|this, ix: usize, window, cx| this.render_row(ix, window, cx)),
                )
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }

    fn render_row(&self, ix: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.loaded.files.get(self.selected) else {
            return div().into_any_element();
        };
        let lang = entry.lang;
        let row = self.rows[ix];
        let diff = entry.diff.as_deref();
        match (row, diff) {
            (PaneRow::Unified(UnifiedRow::Line(line)), Some(diff)) => {
                let l = diff.lines[line as usize];
                let text = diff.text(line as usize);
                let words = intraline(diff, line as usize);
                let element = UnifiedLine::new(
                    ("u", line as usize),
                    l.kind,
                    l.old,
                    l.new,
                    text.to_owned(),
                    lang,
                )
                .words(words)
                .digits(digits(diff))
                .comment(entry.anchored.at(line as usize).next().is_some());
                self.line_row(ix, element, cx)
            }
            (PaneRow::Split(SplitRow::Pair { left, right }), Some(diff)) => {
                let cell = |line: Option<u32>, old_side: bool| {
                    line.map(|line| {
                        let l = diff.lines[line as usize];
                        DiffCell {
                            kind: l.kind,
                            number: if old_side { l.old } else { l.new },
                            text: diff.text(line as usize).to_owned().into(),
                            words: intraline(diff, line as usize),
                        }
                    })
                };
                let marked = |line: Option<u32>| {
                    line.is_some_and(|l| entry.anchored.at(l as usize).next().is_some())
                };
                let right_marked = right != left && marked(right);
                let element = SplitLine::new(("s", ix), cell(left, true), cell(right, false), lang)
                    .digits(digits(diff))
                    .comments(
                        marked(left),
                        right_marked || (right == left && marked(right)),
                    );
                self.line_row(ix, element, cx)
            }
            (
                PaneRow::Unified(UnifiedRow::Fold(fold)) | PaneRow::Split(SplitRow::Fold(fold)),
                _,
            ) => {
                let entity = cx.entity().downgrade();
                FoldLine::new(("fold", fold.start as usize), fold.len)
                    .on_expand(move |_, cx| {
                        entity
                            .update(cx, |this, cx| this.expand(fold.start, cx))
                            .ok();
                    })
                    .into_any_element()
            }
            (PaneRow::Thread(t), _) => self
                .render_thread(t, false, Some(ix) == self.thread_cursor, cx)
                .into_any_element(),
            (PaneRow::Older(t), _) => self
                .render_thread(t, true, Some(ix) == self.thread_cursor, cx)
                .into_any_element(),
            (PaneRow::Composer, _) => self.render_line_composer(cx),
            (PaneRow::OlderHeader(count), _) => {
                let theme = cx.theme();
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .px(px(space::S16))
                    .pt(px(space::S16))
                    .pb(px(space::S4))
                    .text_color(theme.colors.text_secondary)
                    .type_style(typography::LABEL, theme)
                    .child(
                        Icon::new(IconName::MessageSquare)
                            .size(size::ICON_SM)
                            .color(theme.colors.diff_comment),
                    )
                    .child(format!("Comments on older code ({count})"))
                    .into_any_element()
            }
            (PaneRow::Collapsed(reason), _) => self.render_collapsed(reason, cx).into_any_element(),
            (PaneRow::Unavailable, _) => self.render_unavailable(cx).into_any_element(),
            (PaneRow::NoChanges, _) => self
                .render_note("No content changes in this file.", cx)
                .into_any_element(),
            _ => {
                let lines = entry.diff.as_deref().map_or(0, |d| d.lines.len());
                if lines > 0 {
                    return self
                        .render_note(format!("Loading {lines} lines…"), cx)
                        .into_any_element();
                }
                self.render_loading(cx)
            }
        }
    }

    /// The selected file's loading state (spec 0004 States): nothing for
    /// the first [`loading::GRACE`], then a spinner and the file name, and
    /// after [`loading::SLOW`] what the wait is on.
    fn render_loading(&self, cx: &mut Context<Self>) -> AnyElement {
        let indicator = match self.loads.get(&self.selected) {
            Some(load) => {
                loading::indicator(load, Instant::now(), self.is_offline(), self.blobs.hold())
            }
            // Nothing tracks it (a background pass has it): no reason to give.
            None => Some(loading::Indicator { detail: None }),
        };
        let Some(indicator) = indicator else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let colors = &theme.colors;
        let name = self
            .loaded
            .files
            .get(self.selected)
            .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path).to_owned())
            .unwrap_or_default();
        div()
            .id("diff-loading")
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .px(px(space::S16))
            .py(px(space::S12))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY, theme)
                    .child(Spinner::new())
                    .child(format!("Loading {name}…")),
            )
            .children(indicator.detail.map(|detail| {
                div()
                    .pl(px(size::ICON_SM + space::S8))
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .child(detail)
            }))
            .into_any_element()
    }

    fn render_note(&self, text: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
        let theme = cx.theme();
        div()
            .px(px(space::S16))
            .py(px(space::S12))
            .text_color(theme.colors.text_secondary)
            .type_style(typography::BODY, theme)
            .child(text.into())
    }

    fn render_collapsed(
        &self,
        reason: Collapsed,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let text = match reason {
            Collapsed::Generated => "Generated file, collapsed.",
            Collapsed::Binary => "Binary file, not shown.",
            Collapsed::Large => "Large file, collapsed to keep scrolling fast.",
        };
        let entity = cx.entity().downgrade();
        let file = self.selected;
        self.render_note(text, cx)
            .flex()
            .items_center()
            .gap(px(space::S12))
            .child(
                Button::new("load-anyway", "Load anyway").on_press(move |_, cx| {
                    entity
                        .update(cx, |this, cx| this.compute(file, true, cx))
                        .ok();
                }),
            )
    }

    /// The selected file's blobs couldn't be fetched: why, and a retry.
    fn render_unavailable(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let file = self.selected;
        let reason = self.loaded.files[file]
            .unavailable
            .clone()
            .unwrap_or_default();
        let entity = cx.entity().downgrade();
        self.render_note(format!("Couldn't load this file. {reason}"), cx)
            .flex()
            .items_center()
            .gap(px(space::S12))
            .child(
                Button::new("retry-file", "Retry")
                    .shortcut(Kbd::parse(RETRY_KEYS).expect("static binding"))
                    .on_press(move |_, cx| {
                        entity.update(cx, |this, cx| this.retry_file(file, cx)).ok();
                    }),
            )
    }

    fn render_thread(
        &self,
        index: usize,
        older: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let card = self.render_thread_card(index, older, focused, None, false, cx);
        div()
            .pl(px(space::S48))
            .pr(px(space::S16))
            .py(px(space::S6))
            .bg(cx.theme().colors.bg_code)
            .child(card.max_w(px(760.0)))
    }

    /// The inline thread card (0004): status, location, age, comments and
    /// footer; the Threads view adds its excerpt under the top line and
    /// Open in diff (spec 0019 design §2.2).
    fn render_thread_card(
        &self,
        index: usize,
        older: bool,
        focused: bool,
        excerpt: Option<AnyElement>,
        can_open: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let footer = self.render_thread_footer(index, cx);
        let open = can_open.then(|| {
            Button::new(("thread-open-in-diff", index), "Open in diff")
                .variant(ButtonVariant::Ghost)
                .shortcut(Kbd::parse("enter").expect("static binding"))
                .on_press(compose::press(cx, move |this, w, cx| {
                    this.open_thread_in_diff(index, w, cx)
                }))
        });
        let thread = &self.loaded.threads[index];
        let theme = cx.theme();
        let colors = &theme.colors;
        let raw = &theme.raw;
        let (label, tone) = thread_status(thread.status);
        let location = match thread.anchor.as_ref() {
            None => Some("General comment".to_owned()),
            Some(anchor) => anchor.right.or(anchor.left).map(|range| {
                let lines = if range.end > range.start {
                    format!("Lines {}–{}", range.start, range.end)
                } else {
                    format!("Line {}", range.start)
                };
                match thread.iteration {
                    Some(it) if older => format!("{lines} · iteration {it}"),
                    _ => lines,
                }
            }),
        };
        let comments = thread
            .comments
            .iter()
            .filter(|c| !c.is_system && !c.is_deleted)
            .map(|comment| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .child(
                        div()
                            .flex()
                            .gap(px(space::S6))
                            .items_center()
                            .child(person_avatar(&self.loaded, &comment.author, AvatarSize::Md))
                            .child(
                                div()
                                    .text_color(colors.text_primary)
                                    .type_style(typography::BODY_STRONG, theme)
                                    .child(self.loaded.name(&comment.author)),
                            )
                            .child(
                                div()
                                    .text_color(colors.text_tertiary)
                                    .child(data::age(self.loaded.now, comment.created)),
                            ),
                    )
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY, theme)
                            .child(data::resolve_mentions(&comment.body, &self.loaded.people)),
                    )
            });
        let card_bg = theme.flatten(raw.bg_raised, raw.bg_code);
        let border: Hsla = if focused {
            colors.border_focus
        } else {
            colors.border_default
        };
        let footer = match open {
            Some(open) => Some(
                div()
                    .flex()
                    .items_end()
                    .gap(px(space::S8))
                    .child(div().flex_1().min_w_0().children(footer))
                    .child(open)
                    .into_any_element(),
            ),
            None => footer,
        };
        let tour_anchor = adoken_ui::tour::anchor(super::anchors::THREAD, cx);
        div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .p(px(space::S12))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(border)
            .bg(card_bg)
            .type_style(typography::SMALL, theme)
            .children(tour_anchor)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(Chip::new(label).tone(tone))
                    .children(
                        location.map(|text| div().text_color(colors.text_secondary).child(text)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(data::age(self.loaded.now, thread.updated)),
                    ),
            )
            .children(excerpt)
            .children(comments)
            .children(footer)
    }
}

fn digits(diff: &FileDiff) -> usize {
    diff::gutter_digits(diff.old.len().max(diff.new.len()))
}

/// Changed ranges of display line `line` against its partner.
fn intraline(diff: &FileDiff, line: usize) -> Vec<std::ops::Range<usize>> {
    let l = diff.lines[line];
    let Some(partner) = l.partner() else {
        return Vec::new();
    };
    match l.kind {
        LineKind::Del => words::changed_ranges(diff.text(line), diff.text(partner)).0,
        LineKind::Add => words::changed_ranges(diff.text(partner), diff.text(line)).1,
        LineKind::Context => Vec::new(),
    }
}

pub fn pr_status<C: Copy>(
    status: PrStatus,
    draft: bool,
    colors: &adoken_theme::ColorTokens<C>,
) -> (IconName, C, &'static str) {
    match (status, draft) {
        (PrStatus::Active, true) => (IconName::GitPullRequestDraft, colors.pr_draft, "Draft"),
        (PrStatus::Active, false) => (IconName::GitPullRequest, colors.pr_active, "Active"),
        (PrStatus::Completed, _) => (IconName::GitMerge, colors.pr_completed, "Completed"),
        (PrStatus::Abandoned, _) => (
            IconName::GitPullRequestClosed,
            colors.pr_abandoned,
            "Abandoned",
        ),
    }
}

/// tokens.md §2.8 vote icons and colors.
pub fn vote_icon<C: Copy>(vote: Vote, colors: &adoken_theme::ColorTokens<C>) -> (IconName, C) {
    match vote {
        Vote::Approved => (IconName::BadgeCheck, colors.vote_approved),
        Vote::ApprovedWithSuggestions => {
            (IconName::CircleCheckBig, colors.vote_approved_suggestions)
        }
        Vote::NoVote => (IconName::CircleDashed, colors.vote_none),
        Vote::WaitingForAuthor => (IconName::Clock, colors.vote_waiting),
        Vote::Rejected => (IconName::OctagonX, colors.vote_rejected),
    }
}

fn thread_status(status: ThreadStatus) -> (&'static str, ChipTone) {
    match status {
        ThreadStatus::Active => ("Active", ChipTone::Warning),
        ThreadStatus::Pending => ("Pending", ChipTone::Neutral),
        ThreadStatus::Fixed => ("Resolved", ChipTone::Success),
        ThreadStatus::WontFix => ("Won't fix", ChipTone::Neutral),
        ThreadStatus::Closed => ("Closed", ChipTone::Neutral),
        ThreadStatus::ByDesign => ("By design", ChipTone::Neutral),
        ThreadStatus::Unknown => ("Unknown", ChipTone::Neutral),
    }
}

impl Focusable for PrReview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PrReview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Q-3 / AC-16: follow the window width until the user chooses.
        let width = window.viewport_size().width / px(1.0);
        let layout = Layout::resolve(self.preference, width);
        if layout != self.layout {
            self.layout = layout;
            self.rebuild_rows();
        }
        self.track_dwell(cx);
        let context = if self.is_editing(window, cx) || self.filter_focused(window, cx) {
            EDITING_CONTEXT
        } else {
            KEY_CONTEXT
        };
        let threads = self.review_mode() == ReviewMode::Threads;
        let colors = &cx.theme().colors;
        let root = div()
            .id("pr-review")
            .key_context(context)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_app)
            // Threads mode (spec 0019 FR-12): j/k by group, n/p by card,
            // ↩ opens in the diff; v and Enter's expand are Files-only.
            .on_action(cx.listener(move |this, _: &NextFile, _, cx| {
                if threads {
                    this.step_group(1, cx)
                } else {
                    this.step_file(1, cx)
                }
            }))
            .on_action(cx.listener(move |this, _: &PreviousFile, _, cx| {
                if threads {
                    this.step_group(-1, cx)
                } else {
                    this.step_file(-1, cx)
                }
            }))
            .on_action(cx.listener(move |this, a: &MarkViewed, w, cx| {
                if !threads {
                    this.mark_viewed(a, w, cx)
                }
            }))
            .on_action(cx.listener(move |this, _: &NextThread, _, cx| {
                if threads {
                    this.step_card(1, cx)
                } else {
                    this.step_thread(true, cx)
                }
            }))
            .on_action(cx.listener(move |this, _: &PreviousThread, _, cx| {
                if threads {
                    this.step_card(-1, cx)
                } else {
                    this.step_thread(false, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleDiffLayout, _, cx| this.toggle_layout(cx)))
            .on_action(cx.listener(move |this, a: &ExpandContext, w, cx| {
                if threads {
                    this.open_focused_in_diff(w, cx)
                } else {
                    this.expand_context(a, w, cx)
                }
            }))
            .on_action(
                cx.listener(|this, _: &FirstThreadCard, _, cx| this.first_or_last_card(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &LastThreadCard, _, cx| this.first_or_last_card(true, cx)),
            )
            .on_action(cx.listener(|this, _: &FocusFileFilter, w, cx| this.focus_filter(w, cx)))
            .on_action(cx.listener(|this, _: &ClearFileFilter, _, cx| this.clear_filter(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleThreadsOnly, _, cx| this.toggle_threads_only(cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleThreadsView, w, cx| this.toggle_mode(w, cx)))
            .on_action(cx.listener(|this, _: &ShowActiveThreads, _, cx| {
                this.set_scope(crate::review::threads::ThreadScope::Active, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowResolvedThreads, _, cx| {
                this.set_scope(crate::review::threads::ThreadScope::Resolved, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowAllThreads, _, cx| {
                this.set_scope(crate::review::threads::ThreadScope::All, cx)
            }))
            .on_action(cx.listener(|this, _: &super::CopyPrTitle, _, cx| {
                this.copy_target(web::CopyTarget::Title, cx)
            }))
            .on_action(cx.listener(|this, _: &super::CopySourceBranch, _, cx| {
                this.copy_target(web::CopyTarget::SourceBranch, cx)
            }))
            .on_action(cx.listener(|this, _: &super::CopyTargetBranch, _, cx| {
                this.copy_target(web::CopyTarget::TargetBranch, cx)
            }))
            .on_action(cx.listener(Self::retry_loading))
            .on_action(cx.listener(|this, _: &super::CopyPrId, _, cx| {
                this.web_action(WebAction::CopyId, cx)
            }))
            .on_action(cx.listener(|this, _: &super::CopyPrLink, _, cx| {
                this.web_action(WebAction::CopyLink, cx)
            }))
            .on_action(cx.listener(|this, _: &super::OpenPrInAdo, _, cx| {
                this.web_action(WebAction::Open, cx)
            }));
        let root = self.on_write_actions(root, cx);
        let root = self.on_link_actions(root, cx);
        let overlays = self.render_overlays(cx);
        let root = match self.part {
            Part::Header => root.child(self.render_header(cx)),
            Part::Tree => root.child(self.render_tree(window, cx)),
            Part::Diff => root.child(self.render_diff(cx)),
            Part::All => {
                let pr_composer = self.render_pr_composer(cx);
                root.child(self.render_header(cx))
                    .children(pr_composer)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(TREE_WIDTH))
                                    .h_full()
                                    .border_r_1()
                                    .border_color(cx.theme().colors.border_subtle)
                                    .child(self.render_tree(window, cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .child(self.render_diff(cx)),
                            ),
                    )
            }
        };
        root.children(overlays)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_parse() {
        for keys in [
            "j",
            "k",
            "v",
            "n",
            "p",
            "enter",
            TOGGLE_LAYOUT_KEYS,
            COMMENT_KEYS,
            PR_COMMENT_KEYS,
            REPLY_KEYS,
            RESOLVE_KEYS,
            VOTE_KEYS,
            COMPLETE_KEYS,
            STATUS_KEYS,
            DRAFT_KEYS,
            ABANDON_KEYS,
            LINK_WORK_ITEM_KEYS,
            POST_KEYS,
            AUTO_COMPLETE_KEYS,
            STRATEGY_KEYS,
            DELETE_BRANCH_KEYS,
            TRANSITION_KEYS,
            RETRY_KEYS,
            web::COPY_ID_KEYS,
            web::COPY_LINK_KEYS,
            web::OPEN_KEYS,
        ] {
            assert!(Kbd::parse(keys).is_ok(), "{keys}");
        }
    }

    /// Spec 0025 AC-3: digits cast no vote in the open vote menu; the
    /// arrows and ↩ still drive it.
    #[test]
    fn ac3_digits_cast_no_vote_in_the_vote_menu() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(crate::key_bindings());
        let stack: Vec<KeyContext> = ["Workspace", KEY_CONTEXT, MENU_CONTEXT]
            .iter()
            .map(|c| KeyContext::parse(c).expect("context"))
            .collect();
        let resolve = |keys: &str| {
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, &stack);
            bindings.first().map(|binding| binding.action().name())
        };
        let votes = [
            Approve.name(),
            ApproveWithSuggestions.name(),
            WaitForAuthor.name(),
            Reject.name(),
            ResetVote.name(),
        ];
        for digit in 1..=9 {
            let action = resolve(&digit.to_string());
            assert!(
                action.is_none_or(|name| !votes.contains(&name)),
                "{digit} votes: {action:?}"
            );
        }
        assert_eq!(resolve("down"), Some(MenuNext.name()));
        assert_eq!(resolve("up"), Some(MenuPrevious.name()));
        assert_eq!(resolve("enter"), Some(MenuConfirm.name()));
    }
}
