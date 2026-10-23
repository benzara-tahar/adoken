//! Pull requests view (spec 0004): one pane at a time (§3.1, user decision
//! 2026-09-30). The full-width PR list with Mine / To review / All tabs and a
//! repository filter (`repo_filter`), or the full-width review of one PR
//! ([`PrReview`]); `↩` / a click opens the review, `Esc` / the header's back
//! button returns to the list with the same row selected and the scroll kept
//! (`⌘[` / `⌘]` are the shell's history across views, spec 0018, [`nav`]).
//!
//! Store reads run on the background executor; the list paints from memory
//! once loaded, and opening a PR builds its header, tree and first diff off
//! the UI thread. For a signed-in org, the file list and blobs come through
//! [`blobs`] (the sync governor at user priority, into the store's blob
//! cache); the demo org's store has them all. Review writes go through
//! [`writes::Access`] (the org's governor at user priority, or the local
//! store for the demo org); local state (viewed files, diff layout, drafts)
//! lives in this crate's own `review_*` tables. Copy PR id / Copy link /
//! Open in Azure DevOps ([`web`]) act on the open review or, in the list,
//! its selection; the page shows their toast.
//!
//! Keyboard focus: the view's root tracks the list's focus handle and
//! carries the list key context while the list shows, so `↑`/`↓` work as
//! soon as the view is shown and after returning; the open review takes
//! focus itself ([`Focusable`] hands the shell whichever pane is shown).

pub mod blobs;
pub mod data;
pub mod filter;
pub mod fixtures;
pub mod groups;
mod list_status;
pub mod loading;
pub mod nav;
pub mod pr;
mod repo_filter;
#[cfg(test)]
mod tests;
pub mod threads;
pub mod web;
pub mod writes;

/// Guided tour anchors (spec 0020 FR-10).
pub mod anchors {
    pub const HEADER: &str = "review.header";
    pub const FILE_TREE: &str = "review.file-tree";
    pub const DIFF: &str = "review.diff";
    pub const THREAD: &str = "review.thread";
    pub const ACTIONS: &str = "review.actions";
}

use std::collections::{HashMap, HashSet};

use adoken_model::{Person, PersonId, PullRequest, Repo, Vote, short_branch};
use adoken_store::{PrFilter, Store};
use adoken_theme::{opacity, radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, Chip, ChipTone, EmptyState, Icon, IconName, Segmented,
    Tooltip, TypeStyleExt,
};
use futures::FutureExt as _;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyBinding, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, actions, div, prelude::*,
    px, uniform_list,
};

use crate::avatar::{Avatar, AvatarSize};
use crate::palette::PaletteCommand;
use crate::pipelines::outcome_style;
use data::{Layout, ListTab, Move, RowStatus};
pub use pr::{Part, PrReview};

actions!(
    pull_requests,
    [
        /// Move the PR list selection down.
        SelectNextPr,
        /// Move the PR list selection up.
        SelectPreviousPr,
        /// Select the first pull request in the list.
        SelectFirstPr,
        /// Select the last pull request in the list.
        SelectLastPr,
        /// Move the PR list selection down one page.
        PageDownPrs,
        /// Move the PR list selection up one page.
        PageUpPrs,
        /// Open the selected pull request.
        OpenSelectedPr,
        /// Show my open pull requests.
        ShowMine,
        /// Show pull requests waiting on my review.
        ShowToReview,
        /// Show all pull requests.
        ShowAll,
        /// Show the next pull request tab.
        NextPrTab,
        /// Show the previous pull request tab.
        PreviousPrTab,
        /// Leave the review for the pull request list.
        BackToPrList,
        /// Copy the pull request's id (`!4821`).
        CopyPrId,
        /// Copy the pull request's web address.
        CopyPrLink,
        /// Open the pull request in Azure DevOps.
        OpenPrInAdo,
        /// Show or hide group reviewers (teams and policy groups).
        ToggleGroupReviewers,
        /// Copy the pull request's title.
        CopyPrTitle,
        /// Copy the pull request's source branch.
        CopySourceBranch,
        /// Copy the pull request's target branch.
        CopyTargetBranch,
    ]
);

/// The list's key context (on the view's root while the list shows).
const LIST_CONTEXT: &str = "PrList";
/// The review pane before a review is built (opening, not synced, failed):
/// only Back applies.
const PANE_CONTEXT: &str = "PullRequest";
/// Back to the list from the review (and its header button's keycap).
pub const BACK_KEYS: &str = "escape";
/// PRs loaded into the list; the store returns newest activity first.
const LIST_LIMIT: usize = 2_000;
/// Reviewer avatars drawn per vote group before a "+n".
const GROUP_AVATARS: usize = 3;
/// A list row: up to two title lines over the meta line (id, repository,
/// author, branches; badges and reviewers at its end), `space.8` above and
/// below.
const ROW_HEIGHT: f32 =
    space::S8 * 2.0 + typography::BODY_STRONG.line_height * 2.0 + space::S4 + size::AVATAR_MD;

pub fn bindings() -> Vec<KeyBinding> {
    let list = Some(LIST_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("down", SelectNextPr, list),
        KeyBinding::new("j", SelectNextPr, list),
        KeyBinding::new("up", SelectPreviousPr, list),
        KeyBinding::new("k", SelectPreviousPr, list),
        KeyBinding::new("home", SelectFirstPr, list),
        KeyBinding::new("end", SelectLastPr, list),
        KeyBinding::new("pagedown", PageDownPrs, list),
        KeyBinding::new("pageup", PageUpPrs, list),
        KeyBinding::new("enter", OpenSelectedPr, list),
        KeyBinding::new("right", NextPrTab, list),
        KeyBinding::new("left", PreviousPrTab, list),
        KeyBinding::new(web::COPY_ID_KEYS, CopyPrId, list),
        KeyBinding::new(web::COPY_LINK_KEYS, CopyPrLink, list),
        KeyBinding::new(web::OPEN_KEYS, OpenPrInAdo, list),
        // Spec 0019 FR-18: the selected row's status, as in the review.
        KeyBinding::new(pr::STATUS_KEYS, pr::OpenStatusMenu, list),
        KeyBinding::new(pr::DRAFT_KEYS, pr::ToggleDraft, list),
        KeyBinding::new(pr::ABANDON_KEYS, pr::AbandonPr, list),
    ];
    // Back: from the review (not while a composer, menu or sheet has the
    // keyboard: their own Esc closes them) and from the pane's other states.
    // `⌘[` is the shell's history back (spec 0018 FR-12), never bound here.
    for context in [pr::KEY_CONTEXT, PANE_CONTEXT] {
        bindings.push(KeyBinding::new(BACK_KEYS, BackToPrList, Some(context)));
    }
    bindings.extend(pr::bindings());
    bindings.extend(repo_filter::bindings());
    bindings
}

/// Every review action, for the Cmd+P command list (keyboard-first).
pub fn palette_commands() -> Vec<PaletteCommand> {
    let command = |title, keys, action: Box<dyn gpui::Action>| PaletteCommand {
        title,
        keys,
        action,
    };
    vec![
        command("Pull requests: mine", None, Box::new(ShowMine)),
        command("Pull requests: to review", None, Box::new(ShowToReview)),
        command("Pull requests: all", None, Box::new(ShowAll)),
        command(
            "Pull requests: next tab",
            Some("right"),
            Box::new(NextPrTab),
        ),
        command(
            "Pull requests: previous tab",
            Some("left"),
            Box::new(PreviousPrTab),
        ),
        command(
            "Back to pull request list",
            Some(BACK_KEYS),
            Box::new(BackToPrList),
        ),
        command(
            "Filter pull requests by repository…",
            Some(repo_filter::OPEN_KEYS),
            Box::new(repo_filter::FilterPrsByRepo),
        ),
        command(
            "Pull requests: all repositories",
            Some(repo_filter::CLEAR_KEYS),
            Box::new(repo_filter::ClearRepoFilter),
        ),
        command(
            "Toggle diff layout",
            Some(pr::TOGGLE_LAYOUT_KEYS),
            Box::new(pr::ToggleDiffLayout),
        ),
        command("Next file", Some("j"), Box::new(pr::NextFile)),
        command("Previous file", Some("k"), Box::new(pr::PreviousFile)),
        command("Mark file viewed", Some("v"), Box::new(pr::MarkViewed)),
        command("Next comment thread", Some("n"), Box::new(pr::NextThread)),
        command(
            "Previous comment thread",
            Some("p"),
            Box::new(pr::PreviousThread),
        ),
        command(
            "Expand unchanged lines",
            Some("enter"),
            Box::new(pr::ExpandContext),
        ),
        command(
            "Retry loading files",
            Some(pr::RETRY_KEYS),
            Box::new(pr::RetryLoading),
        ),
        command("Next diff line", Some("down"), Box::new(pr::NextLine)),
        command("Previous diff line", Some("up"), Box::new(pr::PreviousLine)),
        command("Approve", None, Box::new(pr::Approve)),
        command(
            "Approve with suggestions",
            None,
            Box::new(pr::ApproveWithSuggestions),
        ),
        command("Wait for author", None, Box::new(pr::WaitForAuthor)),
        command("Reject", None, Box::new(pr::Reject)),
        command("Reset vote", None, Box::new(pr::ResetVote)),
        command("Vote…", Some(pr::VOTE_KEYS), Box::new(pr::OpenVoteMenu)),
        command(
            "Comment on line",
            Some(pr::COMMENT_KEYS),
            Box::new(pr::CommentOnLine),
        ),
        command(
            "Comment on pull request",
            Some(pr::PR_COMMENT_KEYS),
            Box::new(pr::CommentOnPullRequest),
        ),
        command(
            "Reply to thread",
            Some(pr::REPLY_KEYS),
            Box::new(pr::ReplyToThread),
        ),
        command(
            "Resolve or reactivate thread",
            Some(pr::RESOLVE_KEYS),
            Box::new(pr::ToggleResolved),
        ),
        command(
            "Complete pull request…",
            Some(pr::COMPLETE_KEYS),
            Box::new(pr::OpenCompleteSheet),
        ),
        command(
            "Set auto-complete…",
            None,
            Box::new(pr::OpenAutoCompleteSheet),
        ),
        command(
            "Cancel auto-complete",
            None,
            Box::new(pr::CancelAutoComplete),
        ),
        command(
            "Change pull request status…",
            Some(pr::STATUS_KEYS),
            Box::new(pr::OpenStatusMenu),
        ),
        command(
            "Publish or mark as draft",
            Some(pr::DRAFT_KEYS),
            Box::new(pr::ToggleDraft),
        ),
        command(
            "Abandon pull request…",
            Some(pr::ABANDON_KEYS),
            Box::new(pr::AbandonPr),
        ),
        command("Reactivate pull request", None, Box::new(pr::ReactivatePr)),
        command(
            "Link work item…",
            Some(pr::LINK_WORK_ITEM_KEYS),
            Box::new(pr::LinkWorkItem),
        ),
        command("Unlink work item…", None, Box::new(pr::UnlinkWorkItem)),
        command(
            "Copy pull request id",
            Some(web::COPY_ID_KEYS),
            Box::new(CopyPrId),
        ),
        command(
            "Copy pull request link",
            Some(web::COPY_LINK_KEYS),
            Box::new(CopyPrLink),
        ),
        command(
            "Open pull request in Azure DevOps",
            Some(web::OPEN_KEYS),
            Box::new(OpenPrInAdo),
        ),
        // Spec 0019.
        command(
            "Filter files…",
            Some(pr::FILTER_KEYS),
            Box::new(pr::FocusFileFilter),
        ),
        command(
            "Files: clear filter",
            Some(pr::CLEAR_FILTER_KEYS),
            Box::new(pr::ClearFileFilter),
        ),
        command(
            "Only files with threads: turn on or off",
            Some(pr::THREADS_ONLY_KEYS),
            Box::new(pr::ToggleThreadsOnly),
        ),
        command(
            "Threads view: turn on or off",
            Some(pr::THREADS_VIEW_KEYS),
            Box::new(pr::ToggleThreadsView),
        ),
        command(
            "Threads: show active",
            None,
            Box::new(pr::ShowActiveThreads),
        ),
        command(
            "Threads: show resolved",
            None,
            Box::new(pr::ShowResolvedThreads),
        ),
        command("Threads: show all", None, Box::new(pr::ShowAllThreads)),
        command("Copy pull request title", None, Box::new(CopyPrTitle)),
        command("Copy source branch", None, Box::new(CopySourceBranch)),
        command("Copy target branch", None, Box::new(CopyTargetBranch)),
        command(
            "Group reviewers: show or hide",
            None,
            Box::new(ToggleGroupReviewers),
        ),
    ]
}

pub fn new(store: Store, window: &mut Window, cx: &mut App) -> Entity<ReviewView> {
    cx.new(|cx| ReviewView::new(store, window, cx))
}

/// What the list needs from the store, read in one background job.
struct ListData {
    me: PersonId,
    people: Vec<Person>,
    repos: HashMap<String, String>,
    prs: Vec<PullRequest>,
    /// Comment counts, CI, auto-complete and completing per PR.
    facts: data::RowFacts,
    preference: Option<Layout>,
    /// The saved repository filter, if that repository still exists.
    repo: Option<String>,
    /// Org URL and names for the rows' links ([`web`]).
    web: web::Directory,
}

impl ListData {
    fn read(store: &Store) -> adoken_store::Result<Self> {
        data::migrate(store)?;
        let repos: HashMap<String, String> = store
            .repos()?
            .into_iter()
            .map(|Repo { id, name, .. }| (id, name))
            .collect();
        let saved = data::repo_filter_preference(store)?;
        let repo = data::resolve_repo(saved.clone(), &repos);
        if saved.is_some() && repo.is_none() {
            // Gone: back to all repositories, and don't revive it later.
            data::set_repo_filter_preference(store, None)?;
        }
        Ok(Self {
            me: store.me()?.map(|p| p.id).unwrap_or_default(),
            people: store.people()?,
            repos,
            prs: store.prs(&PrFilter {
                limit: Some(LIST_LIMIT),
                ..PrFilter::default()
            })?,
            facts: data::RowFacts::read(store)?,
            preference: data::layout_preference(store)?,
            repo,
            web: web::Directory::read(store)?,
        })
    }
}

/// The review pane. Anything but `Nothing` shows it instead of the list.
enum Detail {
    Nothing,
    Opening(i64),
    Missing(i64),
    Failed(String),
    Open(Entity<PrReview>),
}

/// What the Pull requests view asks the shell to do.
#[derive(Clone, Debug)]
pub enum ReviewEvent {
    /// A linked work item was opened from the review header (spec 0004
    /// FR-17): show it in the Work items view.
    OpenWorkItem(i64),
    /// A review opened (a list row, the palette, a link): the view as it
    /// was before (spec 0018 FR-3).
    Navigated(Box<nav::PullRequestsNavState>),
    /// `Esc` or the back button left a review for the list (spec 0018
    /// FR-12): the review as it was.
    BackToList(Box<nav::ReviewNavState>),
    /// `↩` on a Threads card: an open within the review (FR-3.4).
    ThreadOpened(Box<nav::ReviewNavState>),
}

impl gpui::EventEmitter<ReviewEvent> for ReviewView {}

type DraftSave = futures::future::Shared<Task<()>>;

pub struct ReviewView {
    store: Store,
    list: Option<ListData>,
    people: HashMap<PersonId, Person>,
    /// Segmented labels with counts under the repository filter.
    tab_labels: Vec<String>,
    tab: ListTab,
    /// Repository filter (a repo id); `None` = all repositories.
    repo: Option<String>,
    picker: Option<repo_filter::RepoPicker>,
    /// Indices into `list.prs` for the current tab and repository.
    visible: Vec<usize>,
    /// Index into `visible`.
    selected: Option<usize>,
    /// Kept while the review shows, so the list comes back where it was.
    list_scroll: UniformListScrollHandle,
    detail: Detail,
    /// Fixed "now" for stories; `None` = wall clock.
    clock: Option<adoken_model::Millis>,
    /// Reachability from the shell's sync state (composers keep drafts).
    offline: bool,
    /// The list's focus (tracked by the view's root).
    focus: FocusHandle,
    _load: Task<()>,
    _open: Option<Task<()>>,
    /// Survives canceled navigation so a queued draft write cannot be canceled.
    draft_save: Option<DraftSave>,
    /// "Copied !4821" ([`web::toast`]) and its dismissal.
    toast: Option<SharedString>,
    toast_kind: web::ToastKind,
    _toast: Option<Task<()>>,
    /// The open review's link toasts and back button.
    _review_events: Vec<Subscription>,
    row_menu: adoken_ui::action_menu::MenuSlot,
    row_bounds: adoken_ui::action_menu::BoundsSlot,
    _pins: Option<gpui::Subscription>,
    /// Status changes from the list (spec 0019 group D).
    list_status: list_status::ListStatus,
    /// `[review] show_group_reviewers` repaints the rows (spec 0019 C1).
    _groups: Subscription,
    /// Stories: group reviewers shown or hidden regardless of the setting
    /// (the gallery shares one app across stories).
    show_groups: Option<bool>,
    /// Back / forward to a review that's reopening: its state, applied
    /// once the review is built (spec 0018 FR-8).
    pending_review: Option<(
        i64,
        crate::shell::history::Pending<pr::history::ReviewState>,
    )>,
    /// Back / forward before the list loaded: its state, and whether its
    /// scroll applies.
    pending_restore_list: Option<crate::shell::history::Pending<(nav::PrListNavState, bool)>>,
    /// The PR whose review couldn't be read (the location stays on it).
    failed_pr: Option<i64>,
}

impl ReviewView {
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reader = store.clone();
        let load = cx.spawn_in(window, async move |this, cx| {
            let data = cx
                .background_spawn(async move { ListData::read(&reader) })
                .await;
            this.update(cx, |this, cx| match data {
                Ok(data) => this.set_list(data, cx),
                Err(error) => {
                    log::warn!("review: couldn't read pull requests: {error}");
                    this.detail = Detail::Failed(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        });
        list_status::install(cx);
        Self {
            store,
            list: None,
            people: HashMap::new(),
            tab_labels: ListTab::ALL.iter().map(|t| t.label().to_owned()).collect(),
            tab: ListTab::ToReview,
            repo: None,
            picker: None,
            visible: Vec::new(),
            selected: None,
            list_scroll: UniformListScrollHandle::new(),
            detail: Detail::Nothing,
            clock: None,
            offline: false,
            focus: cx.focus_handle(),
            _load: load,
            _open: None,
            draft_save: None,
            toast: None,
            toast_kind: web::ToastKind::Done,
            _toast: None,
            _review_events: Vec::new(),
            row_menu: Default::default(),
            row_bounds: Default::default(),
            _pins: crate::pin_actions::observe(cx),
            list_status: Default::default(),
            _groups: groups::observe(cx),
            show_groups: None,
            pending_review: None,
            pending_restore_list: None,
            failed_pr: None,
        }
    }

    /// Pin "now" (relative ages) for deterministic stories.
    pub fn set_clock(&mut self, now: adoken_model::Millis, cx: &mut Context<Self>) {
        self.clock = Some(now);
        cx.notify();
    }

    fn now(&self) -> adoken_model::Millis {
        self.clock.unwrap_or_else(now_ms)
    }

    /// Offline composers keep their text and never send (spec 0004
    /// States: Offline, AC-13).
    pub fn set_offline(&mut self, offline: bool, cx: &mut Context<Self>) {
        self.offline = offline;
        if let Detail::Open(review) = &self.detail {
            review.update(cx, |review, cx| review.set_offline(offline, cx));
        }
    }

    fn set_list(&mut self, data: ListData, cx: &mut Context<Self>) {
        self.people = data
            .people
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        self.repo = data.repo.clone();
        self.list = Some(data);
        self.refilter(cx);
        self.list_loaded(cx);
        cx.notify();
    }

    /// The open review changed its PR (a status change shown, confirmed or
    /// reverted): its row, tabs and counts follow.
    fn update_listed_pr(&mut self, pr: PullRequest, cx: &mut Context<Self>) {
        let Some(list) = self.list.as_mut() else {
            return;
        };
        if let Some(row) = list.prs.iter_mut().find(|row| row.id == pr.id) {
            // Closing a PR drops the auto-complete I set (the store does too).
            if pr.status != adoken_model::PrStatus::Active {
                list.facts.auto_complete.remove(&pr.id);
            }
            *row = pr;
            self.refilter(cx);
            cx.notify();
        }
    }

    /// Recomputes the rows and tab counts for the tab and repository. The
    /// selection stays on the same PR when it's still listed, else on the
    /// open PR, else the first row.
    fn refilter(&mut self, cx: &App) {
        let Some(list) = &self.list else {
            return;
        };
        let selected = self.selected_pr().map(|pr| pr.id);
        let repo = self.repo.as_deref();
        self.visible = data::filter(&list.prs, self.tab, &list.me, repo);
        self.tab_labels = data::tab_labels(&list.prs, &list.me, repo);
        let open = match &self.detail {
            Detail::Opening(id) | Detail::Missing(id) => Some(*id),
            Detail::Open(review) => review_id(review, cx),
            _ => None,
        };
        let position = |id: i64| self.visible.iter().position(|ix| list.prs[*ix].id == id);
        self.selected = selected
            .and_then(position)
            .or_else(|| open.and_then(position))
            .or((!self.visible.is_empty()).then_some(0));
        self.refresh_repo_picker(cx);
    }

    pub fn set_tab(&mut self, tab: ListTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.refilter(cx);
        self.list_scroll
            .scroll_to_item(self.selected.unwrap_or(0), ScrollStrategy::Top);
        cx.notify();
    }

    /// A tab from the palette or `←`/`→`: the list shows it.
    fn show_tab(&mut self, tab: ListTab, window: &mut Window, cx: &mut Context<Self>) {
        self.back(window, cx);
        self.set_tab(tab, cx);
    }

    /// `→` / `←`: the next or previous tab, wrapping.
    fn cycle_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = ListTab::ALL.len() as isize;
        let current = ListTab::ALL
            .iter()
            .position(|t| *t == self.tab)
            .unwrap_or(0) as isize;
        let next = ListTab::ALL[(current + delta).rem_euclid(count) as usize];
        self.show_tab(next, window, cx);
    }

    fn step(&mut self, step: Move, cx: &mut Context<Self>) {
        let Some(next) = data::move_selection(self.selected, self.visible.len(), step) else {
            return;
        };
        self.selected = Some(next);
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// PageUp / PageDown: as many rows as the list shows.
    fn page(&mut self, direction: isize, cx: &mut Context<Self>) {
        let viewport = self.list_scroll.0.borrow().base_handle.bounds().size.height / px(1.0);
        let rows = data::page_rows(viewport, ROW_HEIGHT) as isize;
        self.step(Move::By(direction * rows), cx);
    }

    fn selected_pr(&self) -> Option<&PullRequest> {
        let list = self.list.as_ref()?;
        let ix = *self.visible.get(self.selected?)?;
        list.prs.get(ix)
    }

    /// The list selection's PR id (kept while the review shows).
    pub fn selected_pr_id(&self) -> Option<i64> {
        self.selected_pr().map(|pr| pr.id)
    }

    pub fn pin_subject(&self, cx: &App) -> Option<adoken_model::PinSubject> {
        self.open_review_id(cx)
            .or_else(|| self.selected_pr_id())
            .map(adoken_model::PinSubject::Pr)
    }

    pub fn pin_title(&self, cx: &App) -> SharedString {
        match &self.detail {
            Detail::Open(review) => review.read(cx).pr_title().to_owned().into(),
            _ => self
                .selected_pr()
                .map(|pr| pr.title.clone())
                .unwrap_or_default()
                .into(),
        }
    }

    fn open_row_menu(
        &mut self,
        position: adoken_ui::action_menu::MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Design §4 order: open, web, copy, pin, then the valid status
        // changes (spec 0019 FR-18; invalid ones are absent).
        let mut items = vec![
            crate::pin_actions::action("Open", OpenSelectedPr).keys("enter"),
            crate::pin_actions::action("Open in browser", OpenPrInAdo).keys(web::OPEN_KEYS),
            crate::pin_actions::action("Copy link", CopyPrLink).keys(web::COPY_LINK_KEYS),
            crate::pin_actions::action("Copy PR id", CopyPrId).keys(web::COPY_ID_KEYS),
            crate::pin_actions::action("Copy title", CopyPrTitle),
            crate::pin_actions::action("Copy source branch", CopySourceBranch),
            crate::pin_actions::menu_item(self.pin_subject(cx), self.pin_title(cx), cx),
        ];
        items.extend(self.status_menu_items(cx));
        self.row_menu.open(
            "Pull request",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this| &mut this.row_menu,
            window,
            cx,
        );
    }

    /// The listed PRs in row order (current tab and repository).
    pub fn row_ids(&self) -> Vec<i64> {
        let Some(list) = &self.list else {
            return Vec::new();
        };
        self.visible.iter().map(|ix| list.prs[*ix].id).collect()
    }

    /// The review pane is shown instead of the list.
    pub fn is_showing_review(&self) -> bool {
        !matches!(self.detail, Detail::Nothing)
    }

    /// The PR whose review is open, once built.
    pub fn open_review_id(&self, cx: &App) -> Option<i64> {
        match &self.detail {
            Detail::Open(review) => review_id(review, cx),
            _ => None,
        }
    }

    /// The list's scroll offset (px from the top), for navigation tests.
    pub fn list_scroll_top(&self) -> f32 {
        -(self.list_scroll.0.borrow().base_handle.offset().y / px(1.0))
    }

    /// Back to the list: the review closes, the row it came from stays
    /// selected and in view, the list has the keyboard. No-op on the list.
    /// The shell records it (spec 0018 FR-12).
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_showing_review() {
            return;
        }
        let before = self.nav_state(cx);
        self.close_review(cx);
        if let Some(selected) = self.selected {
            self.list_scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
        window.focus(&self.focus, cx);
        if let nav::PullRequestsNavState::Review(before) = before {
            cx.emit(ReviewEvent::BackToList(before));
        }
        cx.notify();
    }

    /// The review pane closes (its draft is kept); the list shows.
    fn close_review(&mut self, cx: &mut Context<Self>) {
        self.preserve_draft(cx);
        self.detail = Detail::Nothing;
        self._open = None;
        self._review_events.clear();
        self.failed_pr = None;
    }

    fn preserve_draft(&mut self, cx: &mut Context<Self>) {
        if let Detail::Open(review) = &self.detail
            && let Some(save) = review.update(cx, |review, cx| review.flush_draft(cx))
        {
            self.draft_save = Some(save.shared());
        }
    }

    /// Opens the review for PR `id` (list, palette, inbox, Today,
    /// notifications) full-width. Shows "not synced" when the store doesn't
    /// have it. The shell records the open (spec 0018 FR-3).
    pub fn open_pr(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.nav_state(cx);
        self.pending_review = None;
        self.show_review(id, window, cx);
        cx.emit(ReviewEvent::Navigated(Box::new(before)));
    }

    /// [`Self::open_pr`] without recording it (back / forward).
    fn show_review(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        crate::pin_actions::mark_opened(adoken_model::PinSubject::Pr(id), cx);
        self.preserve_draft(cx);
        if let Some(list) = &self.list
            && let Some(pos) = self.visible.iter().position(|ix| list.prs[*ix].id == id)
        {
            self.selected = Some(pos);
            self.list_scroll
                .scroll_to_item(pos, ScrollStrategy::Nearest);
        }
        self.detail = Detail::Opening(id);
        self.failed_pr = None;
        self._review_events.clear();
        // The pane has the keyboard (Esc goes back) until the review does.
        window.focus(&self.focus, cx);
        let store = self.store.clone();
        let now = self.now();
        let (me, people, known_preference) = match &self.list {
            Some(list) => (
                list.me.clone(),
                Some(list.people.clone()),
                Some(list.preference),
            ),
            None => (PersonId::new(), None, None),
        };
        let read_only = !writes::access(cx).can_write();
        let offline = self.offline;
        let files = blobs::files(cx);
        let due_check = files.clone();
        let draft_save = self.draft_save.clone();
        self._open = Some(cx.spawn_in(window, async move |this, cx| {
            if let Some(save) = draft_save {
                let _ = save.await;
            }
            let reader = store.clone();
            #[allow(clippy::type_complexity)]
            let built: adoken_store::Result<
                Option<(data::LoadedPr, Option<Layout>, bool, web::PrLink)>,
            > = cx
                .background_spawn(async move {
                    let Some(detail) = reader.pr(id)? else {
                        return Ok(None);
                    };
                    let people = match people {
                        Some(people) => people,
                        None => reader.people()?,
                    };
                    let me = if me.is_empty() {
                        reader.me()?.map(|p| p.id).unwrap_or_default()
                    } else {
                        me
                    };
                    data::migrate(&reader)?;
                    let preference = match known_preference {
                        Some(preference) => preference,
                        None => data::layout_preference(&reader)?,
                    };
                    let viewed = data::viewed(&reader, id)?;
                    let drafts = writes::drafts(&reader, id)?;
                    let auto_complete = reader.auto_complete(id)?.map(|(commit, _)| commit);
                    let completing = reader.completing(id)?.map(|(_, at)| at);
                    let link = web::Directory::read(&reader)?.link(&detail.pr);
                    // Local view history: "PRs I opened in adoken" (0002
                    // FR-2); Ken counts reviews done here as engaged.
                    reader.write(|tx| {
                        adoken_store::write::mark_viewed(tx, &format!("pr:{id}"), now)
                    })?;
                    // The file list is missing or older than the last push:
                    // refetch it at user priority once the review is open.
                    let due = match &due_check {
                        Some(files) => files.is_due(&detail.pr)?,
                        None => false,
                    };
                    let blob = |blob_id: &str| reader.blob(blob_id).ok().flatten();
                    let mut loaded = data::build(
                        data::Source {
                            detail,
                            people,
                            me,
                            now,
                            viewed,
                            read_only,
                            auto_complete,
                            drafts,
                            blob: &blob,
                        },
                        1,
                    );
                    loaded.completing = completing;
                    Ok(Some((loaded, preference, due, link)))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                if !matches!(this.detail, Detail::Opening(opening) if opening == id) {
                    return;
                }
                this.detail = match built {
                    Ok(Some((loaded, preference, due, link))) => {
                        let repo = adoken_sync::RepoRef::of(loaded.pr());
                        let blobs = blobs::Blobs::store(&store, files.map(|f| (f, repo)));
                        let review = cx.new(|cx| {
                            let mut review =
                                PrReview::new(Some(store), blobs, loaded, preference, window, cx);
                            review.set_offline(offline, cx);
                            review.set_link(link, cx);
                            if due {
                                review.sync_files(cx);
                            }
                            review
                        });
                        window.focus(&review.focus_handle(cx), cx);
                        this._review_events = vec![
                            cx.subscribe(&review, |this, _, pr::Toasted(text), cx| {
                                this.show_toast(text.clone(), cx)
                            }),
                            cx.subscribe_in(
                                &review,
                                window,
                                |this, _, _: &pr::Back, window, cx| this.back(window, cx),
                            ),
                            // Status changes (spec 0004 FR-16): the row's
                            // chip follows the review.
                            cx.subscribe(&review, |this, _, pr::PrChanged(pr), cx| {
                                this.update_listed_pr(pr.clone(), cx)
                            }),
                            cx.subscribe(&review, |_, _, pr::OpenWorkItem(id), cx| {
                                cx.emit(ReviewEvent::OpenWorkItem(*id))
                            }),
                            cx.subscribe(&review, |this, review, opened, cx| {
                                let pr::history::ThreadOpened(before) = opened;
                                let pr = review.read(cx).pr_id();
                                let state = this.review_nav_state(pr, (**before).clone(), cx);
                                cx.emit(ReviewEvent::ThreadOpened(Box::new(state)));
                            }),
                        ];
                        Detail::Open(review)
                    }
                    Ok(None) => Detail::Missing(id),
                    Err(error) => {
                        this.failed_pr = Some(id);
                        Detail::Failed(error.to_string())
                    }
                };
                this.review_built(id, window, cx);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn open_selected(&mut self, _: &OpenSelectedPr, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_pr().map(|pr| pr.id) {
            self.open_pr(id, window, cx);
        }
    }

    /// Copy PR id / Copy link / Open in Azure DevOps for the list selection.
    fn list_web_action(&mut self, action: web::WebAction, cx: &mut Context<Self>) {
        let Some(list) = &self.list else {
            return;
        };
        let Some(link) = self.selected_pr().map(|pr| list.web.link(pr)) else {
            return;
        };
        if matches!(action, web::WebAction::Open)
            && link.open_blocker().is_none()
            && let Some(id) = self.selected_pr_id()
        {
            crate::pin_actions::mark_opened(adoken_model::PinSubject::Pr(id), cx);
        }
        if let Some(text) = link.run(action, cx) {
            self.show_toast(text, cx);
        }
    }

    /// Shows `text` in the page's toast for [`web::TOAST`].
    fn show_toast(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.show_toast_kind(text, web::ToastKind::Done, cx);
    }

    /// [`Self::show_toast`] with its meaning (a refusal shows a cross).
    fn show_toast_kind(
        &mut self,
        text: SharedString,
        kind: web::ToastKind,
        cx: &mut Context<Self>,
    ) {
        self.toast = Some(text);
        self.toast_kind = kind;
        self._toast = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(web::TOAST).await;
            this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The toast on screen, if any.
    pub fn toast(&self) -> Option<&SharedString> {
        self.toast.as_ref()
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let title = self.render_list_title(cx);
        let repo_chip = self.render_repo_chip(cx);
        let repo_empty = if self.list.is_some() && self.visible.is_empty() {
            self.render_repo_empty(cx)
        } else {
            None
        };
        let counts = self.tab_labels.clone();
        let selected_tab = ListTab::ALL
            .iter()
            .position(|t| *t == self.tab)
            .unwrap_or(0);
        let entity = cx.entity().downgrade();
        let body = if self.list.is_none() {
            div().flex_1().into_any_element()
        } else if let Some(empty) = repo_empty {
            div().flex_1().child(empty).into_any_element()
        } else if self.visible.is_empty() {
            let (icon, headline, body) = match self.tab {
                ListTab::ToReview => (
                    IconName::CircleCheck,
                    "No reviews waiting on you",
                    "You're all caught up.",
                ),
                ListTab::Mine => (
                    IconName::GitPullRequest,
                    "No open pull requests of yours",
                    "Pull requests you create show up here.",
                ),
                ListTab::All => (
                    IconName::GitPullRequest,
                    "No pull requests synced yet",
                    "They appear here after the first sync.",
                ),
            };
            div()
                .flex_1()
                .child(EmptyState::new(icon, headline).body(body))
                .into_any_element()
        } else {
            uniform_list(
                "pr-list",
                self.visible.len(),
                cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                    let focused = this.focus.is_focused(window);
                    range
                        .map(|ix| this.render_pr_row(ix, focused, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.list_scroll)
            .flex_1()
            .into_any_element()
        };
        let colors = &cx.theme().colors;
        div()
            .id("pr-list-pane")
            .flex()
            .flex_col()
            .size_full()
            .bg(colors.bg_surface)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::S8))
                    .px(px(space::S16))
                    .py(px(space::S12))
                    .child(title)
                    .child(div().flex().child(
                        Segmented::new("pr-tabs", counts, selected_tab).on_select(
                            move |ix, _, cx| {
                                entity
                                    .update(cx, |this, cx| this.set_tab(ListTab::ALL[ix], cx))
                                    .ok();
                            },
                        ),
                    ))
                    .children(repo_chip),
            )
            .child(body)
    }

    fn render_pr_row(&self, ix: usize, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let list = self.list.as_ref().expect("rows render only once loaded");
        let pr = &list.prs[self.visible[ix]];
        let theme = cx.theme();
        let colors = &theme.colors;
        let (icon, color, _) = pr::pr_status(pr.status, pr.is_draft, colors);
        let author = self.person(&pr.author);
        let repo = list.repos.get(&pr.repo_id).map_or("", String::as_str);
        let selected = self.selected == Some(ix);
        let id = pr.id;
        let dot = || {
            div()
                .flex_none()
                .text_color(colors.text_tertiary)
                .child("·")
        };

        let title = div()
            .flex_1()
            .min_w_0()
            .line_clamp(2)
            .text_ellipsis()
            .text_color(colors.text_primary)
            .type_style(typography::BODY_STRONG, theme)
            .child(pr.title.clone());
        let age = div()
            .flex_none()
            .text_color(colors.text_tertiary)
            .type_style(typography::SMALL, theme)
            .child(data::age(self.now(), pr.updated));
        let meta = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(space::S6))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_color(colors.text_secondary)
            .type_style(typography::SMALL, theme)
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::CODE_INLINE, theme)
                    .child(format!("!{id}")),
            )
            .child(dot())
            .child(div().flex_none().child(repo.to_owned()))
            .child(dot())
            .child(author.size(AvatarSize::Sm))
            .child(div().flex_none().child(self.name(&pr.author)))
            .child(dot())
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .overflow_hidden()
                    .type_style(typography::CODE_INLINE, theme)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(short_branch(&pr.source_branch).to_owned()),
                    )
                    .child(
                        Icon::new(IconName::ArrowRight)
                            .size(size::ICON_XS)
                            .color(colors.text_tertiary),
                    )
                    .child(
                        div()
                            .flex_none()
                            .child(short_branch(&pr.target_branch).to_owned()),
                    ),
            );

        let sending = self.is_sending_status(id);
        let statuses = data::row_status(pr, &list.facts)
            .into_iter()
            .map(|status| Chip::new(status.label()).tone(status_tone(status)));
        // Spec 0019 design §4: the new status with "Sending…" until Azure
        // DevOps confirms it.
        let sending = sending.then(|| list_status::sending_badge(theme));
        let comments = list.facts.comments.get(&id).copied().filter(|n| *n > 0);
        let comments = comments.map(|count| {
            div()
                .id(("pr-row-comments", ix))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .text_color(colors.text_secondary)
                .type_style(typography::SMALL, theme)
                .tooltip(Tooltip::text(match count {
                    1 => "1 active comment thread".to_owned(),
                    n => format!("{n} active comment threads"),
                }))
                .child(
                    Icon::new(IconName::MessageSquare)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(count.to_string())
        });
        let checks = list.facts.checks.get(&id).map(|outcome| {
            let (icon, color) = outcome_style(*outcome, colors);
            // Labelled: a bare status glyph reads like a vote icon.
            div()
                .id(("pr-row-checks", ix))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .text_color(colors.text_secondary)
                .type_style(typography::SMALL, theme)
                .tooltip(Tooltip::text(format!("Checks: {}", outcome.label())))
                .child(Icon::new(icon).size(size::ICON_SM).color(color))
                .child("CI")
        });
        let badges = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(space::S12))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .children(statuses)
                    .children(sending)
                    .children(comments)
                    .children(checks),
            )
            .child(self.render_reviewers(pr, cx));

        let entity = cx.entity().downgrade();
        let colors = &cx.theme().colors;
        div()
            .id(("pr-row", ix))
            .relative()
            .children(
                selected.then(|| adoken_ui::action_menu::track_bounds(self.row_bounds.clone())),
            )
            .h(px(ROW_HEIGHT))
            .w_full()
            .px(px(space::S6))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.selected = Some(ix);
                    this.open_row_menu(
                        adoken_ui::action_menu::MenuPosition::at_pointer(event.position),
                        window,
                        cx,
                    );
                }),
            )
            .on_click(move |event: &gpui::ClickEvent, window, cx| {
                entity
                    .update(cx, |this, cx| {
                        this.selected = Some(ix);
                        if event.modifiers().platform {
                            this.list_web_action(web::WebAction::Open, cx);
                        } else {
                            this.open_pr(id, window, cx);
                        }
                    })
                    .ok();
            })
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .px(px(space::S12))
                    .rounded(px(radius::SM))
                    .overflow_hidden()
                    .cursor_pointer()
                    .when(selected, |row| {
                        row.bg(if focused {
                            colors.bg_selected
                        } else {
                            colors.bg_selected_muted
                        })
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(space::S2))
                                .bg(colors.accent_solid),
                        )
                    })
                    .when(!selected, |row| row.hover(|s| s.bg(colors.bg_hover)))
                    .child(
                        div()
                            .flex_none()
                            .w(px(size::CONTROL_SM))
                            .flex()
                            .justify_center()
                            .child(Icon::new(icon).size(size::ICON_MD).color(color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(space::S4))
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(space::S12))
                                    .child(title)
                                    .child(age),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(space::S12))
                                    .h(px(size::AVATAR_MD))
                                    .child(meta)
                                    .child(badges),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Reviewer avatars grouped by vote (spec 0004 §3.1): each group leads
    /// with its vote icon and rings its avatars in the vote color; required
    /// reviewers who haven't voted come last, dimmed.
    fn render_reviewers(
        &self,
        pr: &PullRequest,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = &cx.theme().colors;
        let theme = cx.theme();
        let show = self.show_groups.unwrap_or_else(|| groups::shown(cx));
        let pending = groups::pending_required_groups(pr, &self.people, show);
        // Spec 0019 FR-16: hidden required groups without a vote leave a
        // dimmed count; nothing else of them shows.
        let indicator = (!pending.is_empty()).then(|| {
            let label = groups::waiting_tooltip(&pending, &self.people);
            div()
                .id("pr-row-pending-groups")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S2))
                .opacity(opacity::DIMMED)
                .text_color(colors.text_tertiary)
                .type_style(typography::SMALL, theme)
                .aria_label(label.clone())
                .tooltip(Tooltip::text(label))
                .child(
                    Icon::new(IconName::Users)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(pending.len().to_string())
        });
        let groups = groups::visible_groups(pr, &self.people, show)
            .into_iter()
            .enumerate()
            .map(|(group_ix, (vote, reviewers))| {
                let (icon, color) = pr::vote_icon(vote, colors);
                let hidden = reviewers.len().saturating_sub(GROUP_AVATARS);
                let avatars =
                    reviewers
                        .iter()
                        .take(GROUP_AVATARS)
                        .enumerate()
                        .map(|(ix, reviewer)| {
                            let name = self.name(&reviewer.person);
                            let label = if vote == Vote::NoVote {
                                format!("{name} · Required, no vote yet")
                            } else {
                                format!("{name} · {}", vote.label())
                            };
                            div()
                                // Scoped by the row's own id.
                                .id(("pr-row-reviewer", group_ix * GROUP_AVATARS + ix))
                                .flex_none()
                                .size(px(size::AVATAR_MD))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(radius::FULL))
                                .border_2()
                                .border_color(color)
                                .tooltip(Tooltip::text(label))
                                .child(self.person(&reviewer.person).size(AvatarSize::Sm))
                        });
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(space::S2))
                    .when(vote == Vote::NoVote, |g| g.opacity(opacity::DIMMED))
                    .child(Icon::new(icon).size(size::ICON_SM).color(color))
                    .children(avatars)
                    .when(hidden > 0, |g| {
                        g.child(
                            div()
                                .text_color(colors.text_tertiary)
                                .type_style(typography::SMALL, theme)
                                .child(format!("+{hidden}")),
                        )
                    })
            })
            .collect::<Vec<_>>();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .children(groups)
            .children(indicator)
    }

    fn name(&self, id: &str) -> String {
        self.people
            .get(id)
            .map_or_else(|| id.to_owned(), |p| p.display_name.clone())
    }

    fn person(&self, id: &str) -> Avatar {
        match self.people.get(id) {
            Some(person) => Avatar::person(person),
            None => Avatar::new(id.to_owned(), id.to_owned()),
        }
    }

    /// The review pane's own back bar, while no review header is there to
    /// carry it (opening, not synced, failed).
    fn render_back_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let colors = &cx.theme().colors;
        let entity = cx.entity().downgrade();
        div()
            .flex_none()
            .flex()
            .items_center()
            .px(px(space::S16))
            .py(px(space::S12))
            .bg(colors.bg_surface)
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                Button::new("review-pane-back", "Pull requests")
                    .variant(ButtonVariant::Ghost)
                    .icon(IconName::ChevronLeft)
                    .shortcut(adoken_ui::Kbd::parse(BACK_KEYS).expect("static binding"))
                    .on_press(move |window, cx| {
                        entity.update(cx, |this, cx| this.back(window, cx)).ok();
                    }),
            )
    }

    fn render_detail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let container = div().size_full().flex().flex_col();
        let empty = |state: EmptyState| div().flex_1().child(state);
        match &self.detail {
            Detail::Open(review) => container.child(review.clone()),
            Detail::Nothing => container,
            // principles §3.2: under 150 ms shows nothing; opening a synced PR
            // is well under that.
            Detail::Opening(_) => container.child(self.render_back_bar(cx)),
            Detail::Missing(id) => container.child(self.render_back_bar(cx)).child(empty(
                EmptyState::new(IconName::CircleAlert, format!("!{id} isn't synced yet"))
                    .body("It appears here after the next sync."),
            )),
            Detail::Failed(error) => container.child(self.render_back_bar(cx)).child(empty(
                EmptyState::new(IconName::CircleX, "Couldn't read this pull request")
                    .body(format!("Your local data is safe. {error}")),
            )),
        }
        .text_color(cx.theme().colors.text_primary)
    }
}

fn status_tone(status: RowStatus) -> ChipTone {
    match status {
        RowStatus::Draft | RowStatus::Abandoned => ChipTone::Neutral,
        RowStatus::Active | RowStatus::Completing | RowStatus::AutoComplete => ChipTone::Accent,
        RowStatus::Completed => ChipTone::Success,
        RowStatus::MergeConflict => ChipTone::Danger,
    }
}

fn review_id(review: &Entity<PrReview>, cx: &App) -> Option<i64> {
    Some(review.read(cx).pr_id())
}

pub(crate) fn now_ms() -> adoken_model::Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as adoken_model::Millis)
}

impl Focusable for ReviewView {
    /// Whichever pane shows: the open review, else the list (the shell
    /// focuses this when the view is shown).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.detail {
            Detail::Open(review) => review.focus_handle(cx),
            _ => self.focus.clone(),
        }
    }
}

impl Render for ReviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let review = self.is_showing_review();
        let context = match (&self.detail, &self.picker) {
            (Detail::Open(_), _) => None,
            _ if review => Some(PANE_CONTEXT),
            // The picker's query field takes typed letters: list keys
            // (j, k, r…) stay off while it's open; so does the status
            // menu or confirmation over the list.
            (_, Some(_)) => None,
            _ if self.list_overlay_open() => None,
            (_, None) => Some(LIST_CONTEXT),
        };
        div()
            .id("pull-requests")
            .when_some(context, |d, context| d.key_context(context))
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(colors.bg_app)
            .on_action(cx.listener(|this, _: &crate::pins::TogglePin, window, cx| {
                crate::pin_actions::toggle(this.pin_subject(cx), this.pin_title(cx), window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &crate::pin_actions::OpenRowMenu, window, cx| {
                    this.open_row_menu(
                        adoken_ui::action_menu::keyboard_position(&this.row_bounds, window),
                        window,
                        cx,
                    );
                }),
            )
            .on_action(cx.listener(|this, _: &ShowMine, w, cx| this.show_tab(ListTab::Mine, w, cx)))
            .on_action(
                cx.listener(|this, _: &ShowToReview, w, cx| {
                    this.show_tab(ListTab::ToReview, w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ShowAll, w, cx| this.show_tab(ListTab::All, w, cx)))
            .on_action(cx.listener(|this, _: &NextPrTab, w, cx| this.cycle_tab(1, w, cx)))
            .on_action(cx.listener(|this, _: &PreviousPrTab, w, cx| this.cycle_tab(-1, w, cx)))
            .on_action(
                cx.listener(|this, _: &repo_filter::FilterPrsByRepo, window, cx| {
                    this.open_repo_picker(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &repo_filter::ClearRepoFilter, _, cx| {
                    this.set_repo(None, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &BackToPrList, window, cx| this.back(window, cx)))
            .when(!review, |d| {
                d.on_action(cx.listener(|this, _: &SelectNextPr, _, cx| this.step(Move::By(1), cx)))
                    .on_action(
                        cx.listener(|this, _: &SelectPreviousPr, _, cx| {
                            this.step(Move::By(-1), cx)
                        }),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectFirstPr, _, cx| this.step(Move::First, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectLastPr, _, cx| this.step(Move::Last, cx)),
                    )
                    .on_action(cx.listener(|this, _: &PageDownPrs, _, cx| this.page(1, cx)))
                    .on_action(cx.listener(|this, _: &PageUpPrs, _, cx| this.page(-1, cx)))
                    .on_action(cx.listener(Self::open_selected))
                    .on_action(cx.listener(|this, _: &CopyPrId, _, cx| {
                        this.list_web_action(web::WebAction::CopyId, cx)
                    }))
                    .on_action(cx.listener(|this, _: &CopyPrLink, _, cx| {
                        this.list_web_action(web::WebAction::CopyLink, cx)
                    }))
                    .on_action(cx.listener(|this, _: &OpenPrInAdo, _, cx| {
                        this.list_web_action(web::WebAction::Open, cx)
                    }))
                    // Spec 0019 FR-18, FR-19: the selected row's status and
                    // copies, from keys, the row menu and `⌘P`.
                    .on_action(cx.listener(|this, _: &pr::OpenStatusMenu, w, cx| {
                        let bounds = this.row_bounds.clone();
                        this.open_list_status_menu(&bounds, w, cx)
                    }))
                    .on_action(cx.listener(|this, _: &pr::ToggleDraft, w, cx| {
                        this.request_list_status(None, w, cx)
                    }))
                    .on_action(cx.listener(|this, _: &pr::AbandonPr, w, cx| {
                        this.request_list_status(Some(writes::StatusAction::Abandon), w, cx)
                    }))
                    .on_action(cx.listener(|this, _: &pr::ReactivatePr, w, cx| {
                        this.request_list_status(Some(writes::StatusAction::Reactivate), w, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &pr::MenuNext, _, cx| this.step_list_status(1, cx)),
                    )
                    .on_action(cx.listener(|this, _: &pr::MenuPrevious, _, cx| {
                        this.step_list_status(-1, cx)
                    }))
                    .on_action(cx.listener(|this, _: &pr::MenuConfirm, w, cx| {
                        this.confirm_list_status(w, cx)
                    }))
                    .on_action(cx.listener(|this, _: &pr::CloseOverlay, w, cx| {
                        this.close_list_status(w, cx)
                    }))
                    .on_action(cx.listener(|this, _: &CopyPrTitle, _, cx| {
                        this.list_copy(web::CopyTarget::Title, cx)
                    }))
                    .on_action(cx.listener(|this, _: &CopySourceBranch, _, cx| {
                        this.list_copy(web::CopyTarget::SourceBranch, cx)
                    }))
                    .on_action(cx.listener(|this, _: &CopyTargetBranch, _, cx| {
                        this.list_copy(web::CopyTarget::TargetBranch, cx)
                    }))
                    .child(self.render_list(cx))
            })
            .when(review, |d| d.child(self.render_detail(cx)))
            .children(
                self.toast
                    .clone()
                    .map(|text| web::toast(text, self.toast_kind, cx.theme())),
            )
            .children(self.row_menu.element())
            .children(self.render_list_status(cx))
    }
}

/// Gallery stories: a [`PrReview`] over the demo dataset's richest PR, with
/// every diff computed up front (stories render synchronously).
pub mod story {
    use super::*;

    /// A title long enough to wrap to two lines and end in an ellipsis in
    /// a default-size window.
    pub const LONG_TITLE: &str = "Checkout: retry the payment provider call with capped exponential backoff, keep the idempotency key across attempts, surface the final decline reason to the order timeline, stop double-charging customers whose first attempt timed out after the provider had already accepted it, and backfill the refunds for every order charged twice since the September release";

    /// Stories: rewrites the store's five newest active PRs so the list
    /// shows every row badge (spec 0004 §3.1): a long title with a
    /// reviewer in every vote group (plus a required one who hasn't voted)
    /// and failing CI; a draft with CI running; a merge conflict;
    /// auto-complete; Completing….
    pub fn seed_row_badges(store: &Store, now: adoken_model::Millis) {
        use adoken_model::{
            MergeStatus, PipelineRun, PrStatus, Reviewer, RunReason, RunResult, RunStatus,
        };
        use adoken_store::write;
        let mut prs = store
            .prs(&PrFilter {
                status: Some(PrStatus::Active),
                ..PrFilter::default()
            })
            .expect("read PRs");
        assert!(prs.len() >= 5, "the demo org has active PRs");
        let author = prs[0].author.clone();
        let people: Vec<Person> = store
            .people()
            .expect("read people")
            .into_iter()
            .filter(|p| !p.is_group && p.id != author)
            .collect();
        let reviewer = |ix: usize, vote: Vote, is_required: bool| Reviewer {
            person: people[ix % people.len()].id.clone(),
            vote,
            is_required,
            has_declined: false,
            added_at: Some(now),
            voted_at: (vote != Vote::NoVote).then_some(now),
        };
        prs[0].title = LONG_TITLE.to_owned();
        prs[0].reviewers = vec![
            reviewer(0, Vote::Approved, true),
            reviewer(1, Vote::Approved, false),
            reviewer(2, Vote::ApprovedWithSuggestions, false),
            reviewer(3, Vote::WaitingForAuthor, false),
            reviewer(4, Vote::Rejected, false),
            reviewer(5, Vote::NoVote, true),
            reviewer(6, Vote::NoVote, false),
        ];
        prs[1].is_draft = true;
        prs[2].merge_status = MergeStatus::Conflicts;
        let run = |id: i64, pr: &PullRequest, result: Option<RunResult>| PipelineRun {
            id,
            project_id: pr.project_id.clone(),
            pipeline_id: id,
            pipeline_name: "PR build".into(),
            run_number: id.to_string(),
            status: if result.is_some() {
                RunStatus::Completed
            } else {
                RunStatus::InProgress
            },
            result,
            reason: RunReason::PullRequest,
            branch: format!("refs/pull/{}/merge", pr.id),
            source_version: pr.last_source_commit.clone(),
            requested_for: Some(pr.author.clone()),
            pr_id: Some(pr.id),
            queued: now,
            started: Some(now),
            finish: result.map(|_| now),
        };
        store
            .write(|tx| {
                for pr in &prs[..5] {
                    write::upsert_pr(tx, pr)?;
                }
                write::upsert_run(tx, &run(990_001, &prs[0], Some(RunResult::Failed)))?;
                write::upsert_run(tx, &run(990_002, &prs[1], None))?;
                let commit = |pr: &PullRequest| pr.last_source_commit.clone().unwrap_or_default();
                write::set_auto_complete(tx, prs[3].id, &commit(&prs[3]), now)?;
                write::set_completing(tx, prs[4].id, &commit(&prs[4]), now)
            })
            .expect("seed the row badges");
    }

    /// Stories: select list row `ix` (clamped) as the keyboard would.
    pub fn select_row(view: &mut ReviewView, ix: usize, cx: &mut Context<ReviewView>) {
        view.step(Move::First, cx);
        view.step(Move::By(ix as isize), cx);
    }

    /// Stories: list row of PR `id` selected; `false` when it isn't listed.
    pub fn select_pr(view: &mut ReviewView, id: i64, cx: &mut Context<ReviewView>) -> bool {
        let Some(ix) = view.row_ids().iter().position(|&row| row == id) else {
            return false;
        };
        select_row(view, ix, cx);
        true
    }

    /// Stories (spec 0019 C1): every active PR gets the demo org's policy
    /// groups as reviewers: the first required without a vote (FR-16's
    /// indicator), the second optional and approved (no trace when hidden).
    pub fn seed_group_reviewers(store: &Store, now: adoken_model::Millis) {
        use adoken_model::{PrStatus, Reviewer};
        use adoken_store::write;
        let groups: Vec<Person> = store
            .people()
            .expect("read people")
            .into_iter()
            .filter(|p| p.is_group)
            .collect();
        assert!(!groups.is_empty(), "the demo org has policy groups");
        let mut prs = store
            .prs(&PrFilter {
                status: Some(PrStatus::Active),
                ..PrFilter::default()
            })
            .expect("read PRs");
        let group = |ix: usize, vote: Vote, is_required: bool| Reviewer {
            person: groups[ix % groups.len()].id.clone(),
            vote,
            is_required,
            has_declined: false,
            added_at: Some(now),
            voted_at: (vote != Vote::NoVote).then_some(now),
        };
        for pr in &mut prs {
            pr.reviewers
                .retain(|r| !groups.iter().any(|g| g.id == r.person));
            pr.reviewers.push(group(0, Vote::NoVote, true));
            if groups.len() > 1 {
                pr.reviewers.push(group(1, Vote::Approved, false));
            }
        }
        store
            .write(|tx| {
                for pr in &prs {
                    write::upsert_pr(tx, pr)?;
                }
                Ok(())
            })
            .expect("seed the group reviewers");
    }

    /// The Pull requests view over `store` with its list read on this
    /// thread, so a story can put it into the state under test right away.
    pub fn list(
        store: Store,
        now: adoken_model::Millis,
        window: &mut Window,
        cx: &mut Context<ReviewView>,
    ) -> ReviewView {
        let data = ListData::read(&store).expect("read the story store");
        let mut view = ReviewView::new(store, window, cx);
        // Replaces (cancels) the background load.
        view._load = Task::ready(());
        view.clock = Some(now);
        view.set_list(data, cx);
        view
    }

    /// Stories: the toast a copy shows, without writing the clipboard or
    /// dismissing it.
    pub fn show_copied(view: &mut ReviewView, text: &str, cx: &mut Context<ReviewView>) {
        view.toast = Some(text.to_owned().into());
        cx.notify();
    }

    /// Builds the review for demo PR `id` at `DEMO_NOW`, on its busiest file.
    pub fn review(
        id: i64,
        layout: Layout,
        part: Part,
        window: &mut Window,
        cx: &mut Context<PrReview>,
    ) -> PrReview {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let pr = ds.pr(id).cloned().expect("demo PR exists");
        let extras = ds.pr_extras.get(&id).cloned().unwrap_or_default();
        let detail = adoken_model::PrDetail {
            pr,
            iterations: extras.iterations,
            files: extras.files,
            threads: extras.threads,
        };
        let blobs = ds.blobs.iter().cloned().collect();
        // The demo org: the fake contoso link copies, nothing opens.
        let link =
            web::Directory::new(Some(ds.org.url), ds.projects, ds.repos, true).link(&detail.pr);
        let mut review = review_from(
            detail, blobs, ds.people, ds.me, ds.now, layout, part, window, cx,
        );
        review.set_link(link, cx);
        review
    }

    /// A review over any detail + blobs, every diff computed up front, the
    /// busiest file selected (stories, frame measurements).
    #[allow(clippy::too_many_arguments)]
    pub fn review_from(
        detail: adoken_model::PrDetail,
        blobs: HashMap<String, String>,
        people: Vec<Person>,
        me: PersonId,
        now: adoken_model::Millis,
        layout: Layout,
        part: Part,
        window: &mut Window,
        cx: &mut Context<PrReview>,
    ) -> PrReview {
        let fixture = fixtures::Fixture {
            detail,
            blobs,
            people,
            me,
            now,
        };
        fixture_review(fixture, None, layout, part, window, cx)
    }

    /// [`review_from`] over a spec 0019 fixture; writes land in `store`
    /// when given (view tests), else nowhere.
    pub fn fixture_review(
        fixture: fixtures::Fixture,
        store: Option<Store>,
        layout: Layout,
        part: Part,
        window: &mut Window,
        cx: &mut Context<PrReview>,
    ) -> PrReview {
        let blobs = blobs::Blobs::memory(fixture.blobs);
        let blob = |blob_id: &str| blobs.read(blob_id);
        let files = fixture.detail.files.len();
        let loaded = data::build(
            data::Source {
                detail: fixture.detail,
                people: fixture.people,
                me: fixture.me,
                now: fixture.now,
                viewed: HashSet::new(),
                // The demo org is writable (writes apply locally);
                // `PrReview::story_read_only` shows the AC-11 state.
                read_only: false,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            files,
        );
        let mut review = PrReview::new(store, blobs, loaded, Some(layout), window, cx).part(part);
        let busiest = review.busiest_file();
        review.select_file(busiest, cx);
        // One file viewed so the tree shows both checkbox states.
        if let Some(&first) = review.loaded().order.first()
            && first != busiest
        {
            review.story_mark_viewed(first);
        }
        review
    }
}
