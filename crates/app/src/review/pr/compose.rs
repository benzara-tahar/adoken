//! Review writes in the PR view (spec 0004 flow steps 4–8): the line /
//! reply / PR-level composer with durable drafts, resolve toggle, vote menu,
//! the Complete / auto-complete sheet, and the header's review controls.
//!
//! - Votes and thread status apply optimistically (the header or card
//!   repaints on the next frame) and revert with the reason on failure.
//! - Comments show as the composer in its posting state; the server's
//!   thread replaces it on success, the composer returns with the reason on
//!   failure. Drafts persist in the store as you type (FR-8) and are only
//!   ever sent by ⌘↵ (AC-13).
//! - Complete pins the displayed source commit; a newer one shows "New
//!   commits since you reviewed" and nothing is retried (AC-10). Azure
//!   DevOps merges asynchronously: while my accepted Complete is queued the
//!   header shows "Completing…" and re-reads the PR as sync lands, until it
//!   is Completed or the merge failed (the reason shows, Complete is back).
//! - Read-only access renders no review control (AC-11).

use std::time::Duration;

use adoken_model::{Vote, short_branch};
use adoken_sync::writes::{
    Applied, CompletionOptions, MergeStrategy, PrRef, ReviewWrite, Side, WriteError,
};
use adoken_theme::elevation;
use adoken_ui::{Notice, Segmented, TextInput, TextInputEvent};
use gpui::{ClickEvent, Div, Entity, SharedString, Stateful, Subscription};

use super::*;
use crate::review::writes::{self as review_writes, Control, Draft, DraftTarget, READ_ONLY_HINT};

/// Typing pauses this long before the draft is saved.
const DRAFT_SAVE_DELAY: Duration = Duration::from_millis(250);
const COMPOSER_WIDTH: f32 = 760.0;
const SHEET_WIDTH: f32 = 540.0;
const MENU_WIDTH: f32 = 280.0;

/// Menu order (flow step 6), keys 1–5.
pub(super) const VOTE_MENU: [Vote; 5] = [
    Vote::Approved,
    Vote::ApprovedWithSuggestions,
    Vote::WaitingForAuthor,
    Vote::Rejected,
    Vote::NoVote,
];

fn vote_menu_label(vote: Vote) -> &'static str {
    match vote {
        Vote::Approved => "Approve",
        Vote::ApprovedWithSuggestions => "Approve with suggestions",
        Vote::WaitingForAuthor => "Wait for author",
        Vote::Rejected => "Reject",
        Vote::NoVote => "Reset vote",
    }
}

#[derive(Default)]
pub(in crate::review) struct WriteState {
    composer: Option<Composer>,
    vote_menu: Option<VoteMenu>,
    sheet: Option<Sheet>,
    /// The header's last write outcome.
    notice: Option<(ChipTone, SharedString)>,
    /// Thread write failures, shown on the card.
    thread_errors: HashMap<i64, SharedString>,
    /// Threads with a status write in flight.
    resolving: HashSet<i64>,
    voting: bool,
    offline: bool,
    /// A line draft waiting for its file's diff (restored after a restart).
    pending_draft: Option<Draft>,
    /// Re-reads the PR on store commits while my Complete is merging.
    completing_watch: Option<Task<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposerState {
    Editing,
    Posting,
    Failed(SharedString),
}

struct Composer {
    target: DraftTarget,
    /// Line composers: `(file, display line)` in the diff pane.
    at: Option<(usize, u32)>,
    input: Entity<TextInput>,
    state: ComposerState,
    save: Option<Task<()>>,
    _changes: Subscription,
}

struct VoteMenu {
    selected: usize,
    focus: FocusHandle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetState {
    Idle,
    Sending {
        auto: bool,
    },
    /// AC-10: the server has a newer source commit; nothing merged.
    NewCommits,
    Failed(SharedString),
}

struct Sheet {
    /// Opened for auto-complete (its button is the default).
    auto: bool,
    options: CompletionOptions,
    message: Entity<TextInput>,
    state: SheetState,
    focus: FocusHandle,
}

/// Which overlay has the review's keyboard, for tests and the key context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    VoteMenu,
    StatusMenu,
    Sheet,
    Confirm,
    WorkItemPicker,
}

/// A button handler that runs on the review.
pub(super) fn press(
    cx: &Context<PrReview>,
    f: impl Fn(&mut PrReview, &mut Window, &mut Context<PrReview>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = cx.entity().downgrade();
    move |window, cx| {
        this.update(cx, |this, cx| f(this, window, cx)).ok();
    }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

impl PrReview {
    // ── state ────────────────────────────────────────────────────────────

    /// Reachability (the shell's sync state): offline composers keep their
    /// draft and never send (AC-13).
    pub fn set_offline(&mut self, offline: bool, cx: &mut Context<Self>) {
        if self.writes.offline != offline {
            self.writes.offline = offline;
            cx.notify();
        }
    }

    /// Sync's view of reachability (the diff's loading copy).
    pub(super) fn is_offline(&self) -> bool {
        self.writes.offline
    }

    /// The open composer's state; `None` once it closed (posted or discarded).
    pub fn composer_state(&self) -> Option<&ComposerState> {
        self.writes.composer.as_ref().map(|c| &c.state)
    }

    /// The open Complete / auto-complete sheet's state; `None` when closed.
    pub fn sheet_state(&self) -> Option<&SheetState> {
        self.writes.sheet.as_ref().map(|s| &s.state)
    }

    /// The open menu, sheet, confirmation or picker, if any.
    pub fn overlay(&self) -> Option<Overlay> {
        if self.writes.vote_menu.is_some() {
            Some(Overlay::VoteMenu)
        } else if self.writes.sheet.is_some() {
            Some(Overlay::Sheet)
        } else {
            self.link_overlay()
        }
    }

    pub(super) fn composer_line(&self, file: usize) -> Option<u32> {
        let (at_file, line) = self.writes.composer.as_ref()?.at?;
        (at_file == file).then_some(line)
    }

    pub(super) fn is_editing(&self, window: &Window, cx: &App) -> bool {
        self.overlay().is_some()
            || self
                .writes
                .composer
                .as_ref()
                .is_some_and(|c| c.input.focus_handle(cx).is_focused(window))
    }

    /// The header's outcome line (a write's result or why it can't run).
    pub(super) fn set_notice(
        &mut self,
        tone: ChipTone,
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.writes.notice = Some((tone, text.into()));
        cx.notify();
    }

    pub(super) fn clear_notice(&mut self) {
        self.writes.notice = None;
    }

    pub(super) fn now(&self) -> adoken_model::Millis {
        crate::review::now_ms()
    }

    pub(super) fn pr_ref(&self) -> PrRef {
        PrRef::of(self.loaded.pr())
    }

    /// The commit the diff shows: the displayed iteration's source commit.
    pub(super) fn displayed_commit(&self) -> Option<String> {
        self.loaded
            .detail
            .iterations
            .iter()
            .find(|it| it.id == self.loaded.iteration)
            .map(|it| it.source_commit.clone())
            .or_else(|| self.loaded.pr().last_source_commit.clone())
    }

    fn controls(&self) -> Vec<Control> {
        review_writes::controls(
            self.loaded.read_only,
            self.loaded.pr(),
            self.loaded.auto_complete.is_some(),
        )
    }

    /// Gate for every write action; read-only explains itself instead.
    fn allowed(&mut self, control: Control, cx: &mut Context<Self>) -> bool {
        if self.controls().contains(&control) {
            return true;
        }
        let why: SharedString = if self.loaded.read_only {
            READ_ONLY_HINT.into()
        } else {
            "This pull request is closed".into()
        };
        self.writes.notice = Some((ChipTone::Neutral, why));
        cx.notify();
        false
    }

    /// Send `write`; `done` runs on the UI thread with the outcome. The write
    /// finishes even if the view closes (the store still gets it).
    pub(super) fn send(
        &mut self,
        write: ReviewWrite,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Applied, WriteError>, &mut Context<Self>) + 'static,
    ) {
        let task = review_writes::access(cx).send(self.store.clone(), write, self.now(), cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ── line cursor ──────────────────────────────────────────────────────

    fn is_line_row(row: &PaneRow) -> bool {
        matches!(
            row,
            PaneRow::Unified(UnifiedRow::Line(_)) | PaneRow::Split(SplitRow::Pair { .. })
        )
    }

    fn step_line(&mut self, forward: bool, cx: &mut Context<Self>) {
        // Threads view (spec 0019 FR-12): ↓/↑ move card to card.
        if self.review_mode() == super::ReviewMode::Threads {
            self.step_card(if forward { 1 } else { -1 }, cx);
            return;
        }
        self.user_moved();
        let start = self
            .line_cursor
            .unwrap_or_else(|| self.diff_list.logical_scroll_top().item_ix);
        let next = if self.line_cursor.is_none() {
            (start..self.rows.len()).find(|ix| Self::is_line_row(&self.rows[*ix]))
        } else if forward {
            (start + 1..self.rows.len()).find(|ix| Self::is_line_row(&self.rows[*ix]))
        } else {
            (0..start)
                .rev()
                .find(|ix| Self::is_line_row(&self.rows[*ix]))
        };
        if let Some(ix) = next {
            self.line_cursor = Some(ix);
            self.diff_list.scroll_to_reveal_item(ix);
            cx.notify();
        }
    }

    /// `(display line, side, 1-based number)` a comment on row `ix` targets:
    /// the PR's side unless the line only exists in the base.
    fn line_target(&self, ix: usize) -> Option<(u32, Side, u32)> {
        let diff = self.loaded.files.get(self.selected)?.diff.as_deref()?;
        let pick = |line: u32| {
            let l = diff.lines[line as usize];
            match (l.new, l.old) {
                (Some(n), _) => Some((line, Side::Right, n)),
                (None, Some(o)) => Some((line, Side::Left, o)),
                (None, None) => None,
            }
        };
        match self.rows.get(ix)? {
            PaneRow::Unified(UnifiedRow::Line(line)) => pick(*line),
            PaneRow::Split(SplitRow::Pair { left, right }) => {
                right.and_then(pick).or_else(|| left.and_then(pick))
            }
            _ => None,
        }
    }

    pub(super) fn line_row(
        &self,
        ix: usize,
        element: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cursor = self.line_cursor == Some(ix);
        let focus = cx.theme().colors.border_focus;
        div()
            .id(("diff-row", ix))
            .relative()
            .child(element)
            .when(cursor, |d| {
                d.child(div().absolute().inset_0().border_1().border_color(focus))
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.line_cursor = Some(ix);
                // Double-click comments on the line (like the gutter "+").
                if event.click_count() > 1 {
                    this.comment_on_line(&CommentOnLine, window, cx);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    // ── composer ─────────────────────────────────────────────────────────

    fn open_composer(
        &mut self,
        target: DraftTarget,
        at: Option<(usize, u32)>,
        text: &str,
        focus: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(composer) = &self.writes.composer
            && composer.target == target
        {
            if focus && let Some(window) = window {
                window.focus(&composer.input.focus_handle(cx), cx);
            }
            return;
        }
        if let Some(save) = self.flush_draft(cx) {
            save.detach();
        }
        let placeholder = match &target {
            DraftTarget::Reply { .. } => "Reply…",
            _ => "Leave a comment (Markdown)…",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        if !text.is_empty() {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        let changes = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| {
            if matches!(event, TextInputEvent::Changed) {
                this.schedule_draft_save(cx);
            }
        });
        if focus && let Some(window) = window {
            window.focus(&input.focus_handle(cx), cx);
        }
        self.writes.composer = Some(Composer {
            target,
            at,
            input,
            state: ComposerState::Editing,
            save: None,
            _changes: changes,
        });
        self.refresh_rows();
        cx.notify();
    }

    fn schedule_draft_save(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let pr = self.pr_id();
        let now = self.now();
        let Some(composer) = self.writes.composer.as_mut() else {
            return;
        };
        if matches!(composer.state, ComposerState::Failed(_)) {
            composer.state = ComposerState::Editing;
        }
        let target = composer.target.clone();
        let text = composer.input.read(cx).text().to_owned();
        composer.save = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(DRAFT_SAVE_DELAY).await;
            cx.background_spawn(async move {
                if let Err(error) = review_writes::save_draft(&store, pr, &target, &text, now) {
                    log::warn!("review: couldn't save the draft: {error}");
                }
            })
            .await;
        }));
    }

    /// Save the open composer's text now (closing it, switching target).
    pub(in crate::review) fn flush_draft(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Task<()>> {
        let store = self.store.clone()?;
        let pr = self.pr_id();
        let now = self.now();
        let composer = self.writes.composer.as_mut()?;
        drop(composer.save.take()?);
        let target = composer.target.clone();
        let text = composer.input.read(cx).text().to_owned();
        Some(cx.background_spawn(async move {
            if let Err(error) = review_writes::save_draft(&store, pr, &target, &text, now) {
                log::warn!("review: couldn't save the draft: {error}");
            }
        }))
    }

    fn close_composer_now(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(save) = self.flush_draft(cx) {
            save.detach();
        }
        if self.writes.composer.take().is_some() {
            self.refresh_rows();
            if let Some(window) = window {
                window.focus(&self.focus, cx);
            }
            cx.notify();
        }
    }

    fn forget_draft(&self, target: DraftTarget, cx: &mut Context<Self>) {
        if let Some(store) = self.store.clone() {
            let pr = self.pr_id();
            cx.background_spawn(async move {
                if let Err(error) = review_writes::delete_draft(&store, pr, &target) {
                    log::warn!("review: couldn't delete the draft: {error}");
                }
            })
            .detach();
        }
    }

    /// Reopen the newest draft after a restart (AC-13): shown, never sent.
    pub(super) fn restore_draft(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.loaded.drafts.first().cloned() else {
            return;
        };
        match &draft.target {
            DraftTarget::Line { path, .. } => {
                let Some(file) = self
                    .loaded
                    .files
                    .iter()
                    .position(|f| f.path == data::normalize_path(path))
                else {
                    return;
                };
                self.writes.pending_draft = Some(draft);
                if file != self.selected {
                    self.select_file(file, cx);
                }
                self.try_restore_pending(cx);
            }
            DraftTarget::Reply { thread } => {
                if review_writes::thread_index(&self.loaded, *thread).is_some() {
                    self.open_composer(draft.target.clone(), None, &draft.body, false, None, cx);
                }
            }
            DraftTarget::Pr => {
                self.open_composer(DraftTarget::Pr, None, &draft.body, false, None, cx);
            }
        }
    }

    /// A line draft reopens once its file's diff exists.
    pub(super) fn try_restore_pending(&mut self, cx: &mut Context<Self>) {
        let Some(Draft {
            target: DraftTarget::Line { path, side, line },
            ..
        }) = self.writes.pending_draft.as_ref()
        else {
            return;
        };
        let Some(entry) = self.loaded.files.get(self.selected) else {
            return;
        };
        if entry.path != data::normalize_path(path) {
            return;
        }
        let Some(diff) = entry.diff.as_deref() else {
            return;
        };
        let display = diff.lines.iter().position(|l| match side {
            Side::Right => l.new == Some(*line),
            Side::Left => l.old == Some(*line) && l.new.is_none(),
        });
        let draft = self.writes.pending_draft.take().expect("checked above");
        if let Some(display) = display {
            let at = Some((self.selected, display as u32));
            self.open_composer(draft.target, at, &draft.body, false, None, cx);
        }
    }

    fn comment_on_line(&mut self, _: &CommentOnLine, window: &mut Window, cx: &mut Context<Self>) {
        // `c` is Files-only (spec 0019 FR-12).
        if self.review_mode() == super::ReviewMode::Threads || !self.allowed(Control::Comment, cx) {
            return;
        }
        let cursor = self.line_cursor.or_else(|| {
            let top = self.diff_list.logical_scroll_top().item_ix;
            (top..self.rows.len()).find(|ix| Self::is_line_row(&self.rows[*ix]))
        });
        let Some((display, side, line)) = cursor.and_then(|ix| self.line_target(ix)) else {
            return;
        };
        self.line_cursor = cursor;
        let path = self.loaded.files[self.selected].path.clone();
        let target = DraftTarget::Line { path, side, line };
        let text = self.draft_text(&target);
        self.open_composer(
            target,
            Some((self.selected, display)),
            &text,
            true,
            Some(window),
            cx,
        );
    }

    fn comment_on_pr(
        &mut self,
        _: &CommentOnPullRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.allowed(Control::Comment, cx) {
            return;
        }
        let text = self.draft_text(&DraftTarget::Pr);
        self.open_composer(DraftTarget::Pr, None, &text, true, Some(window), cx);
    }

    /// The thread `n`/`p` selected (the focused card in the Threads view),
    /// else the one whose reply is open.
    fn selected_thread(&self) -> Option<usize> {
        let cursor = if self.review_mode() == super::ReviewMode::Threads {
            self.focused_card()
        } else {
            self.thread_cursor.and_then(|ix| match self.rows.get(ix) {
                Some(PaneRow::Thread(t) | PaneRow::Older(t)) => Some(*t),
                _ => None,
            })
        };
        cursor.or_else(|| match &self.writes.composer.as_ref()?.target {
            DraftTarget::Reply { thread } => review_writes::thread_index(&self.loaded, *thread),
            _ => None,
        })
    }

    fn reply_to(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.allowed(Control::Reply, cx) {
            return;
        }
        let target = DraftTarget::Reply {
            thread: self.loaded.threads[index].id,
        };
        let text = self.draft_text(&target);
        self.open_composer(target, None, &text, true, Some(window), cx);
    }

    fn reply_to_thread(&mut self, _: &ReplyToThread, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_thread() {
            Some(index) => self.reply_to(index, window, cx),
            None => {
                self.writes.notice = Some((ChipTone::Neutral, "Pick a thread with n / p".into()));
                cx.notify();
            }
        }
    }

    fn draft_text(&self, target: &DraftTarget) -> String {
        self.loaded
            .drafts
            .iter()
            .find(|d| &d.target == target)
            .map(|d| d.body.clone())
            .unwrap_or_default()
    }

    fn write_for(&self, target: &DraftTarget, body: String) -> Option<ReviewWrite> {
        let pr = self.pr_ref();
        Some(match target {
            DraftTarget::Line { path, side, line } => ReviewWrite::CreateThread {
                pr,
                anchor: Some(review_writes::line_anchor(&self.loaded, path, *side, *line)),
                iteration: self.loaded.iteration,
                body,
            },
            DraftTarget::Pr => ReviewWrite::CreateThread {
                pr,
                anchor: None,
                iteration: self.loaded.iteration,
                body,
            },
            DraftTarget::Reply { thread } => {
                let index = review_writes::thread_index(&self.loaded, *thread)?;
                let parent = self.loaded.threads[index]
                    .comments
                    .iter()
                    .find(|c| !c.is_system)
                    .map_or(1, |c| c.id);
                ReviewWrite::Reply {
                    pr,
                    thread_id: *thread,
                    parent_comment_id: parent,
                    body,
                }
            }
        })
    }

    fn post_comment(&mut self, _: &PostComment, _: &mut Window, cx: &mut Context<Self>) {
        let offline = self.writes.offline;
        let Some(composer) = self.writes.composer.as_mut() else {
            return;
        };
        if composer.state == ComposerState::Posting {
            return;
        }
        let body = composer.input.read(cx).text().trim().to_owned();
        match review_writes::submit(offline, &body) {
            review_writes::Submit::Empty => return,
            review_writes::Submit::KeepDraft => {
                // AC-13: the draft is saved; only a later ⌘↵ sends it.
                composer.state = ComposerState::Failed(
                    "You're offline. Your draft is saved; press ⌘↵ again once you're back online."
                        .into(),
                );
                cx.notify();
                return;
            }
            review_writes::Submit::Send => {}
        }
        let target = composer.target.clone();
        let at = composer.at;
        let Some(write) = self.write_for(&target, body) else {
            return;
        };
        if let Some(composer) = self.writes.composer.as_mut() {
            composer.state = ComposerState::Posting;
        }
        cx.notify();
        self.send(write, cx, move |this, result, cx| {
            let mine = this
                .writes
                .composer
                .as_ref()
                .is_some_and(|c| c.target == target);
            match result {
                Ok(applied) => {
                    this.forget_draft(target.clone(), cx);
                    this.loaded.drafts.retain(|d| d.target != target);
                    if let Applied::Thread(thread) = applied {
                        match (&target, at) {
                            (DraftTarget::Line { .. }, Some((file, line))) => {
                                review_writes::insert_thread(
                                    &mut this.loaded,
                                    file,
                                    line as usize,
                                    thread,
                                );
                            }
                            (DraftTarget::Reply { thread: id }, _) => {
                                if let Some(ix) = review_writes::thread_index(&this.loaded, *id) {
                                    review_writes::replace_thread(&mut this.loaded, ix, thread);
                                }
                            }
                            _ => {
                                // A PR-level thread: listed in the Threads
                                // view's Pull request group (spec 0019 FR-8).
                                std::sync::Arc::make_mut(&mut this.loaded.threads).push(thread);
                                this.writes.notice = Some((
                                    ChipTone::Success,
                                    format!("Comment posted on !{}", this.pr_id()).into(),
                                ));
                            }
                        }
                        this.threads_changed();
                    }
                    if mine {
                        if let Some(composer) = this.writes.composer.as_mut() {
                            composer.save = None;
                        }
                        this.writes.composer = None;
                        // The focused input is gone: keep the review keys
                        // (n, r, ⇧R, …) working without a click.
                        cx.focus_later(this.focus.clone());
                    }
                    this.refresh_rows();
                }
                Err(error) => {
                    if mine && let Some(composer) = this.writes.composer.as_mut() {
                        composer.state = ComposerState::Failed(error.to_string().into());
                    } else {
                        this.writes.notice = Some((
                            ChipTone::Danger,
                            format!("Your comment wasn't posted: {error}").into(),
                        ));
                    }
                }
            }
        });
    }

    fn close_composer(&mut self, _: &CloseComposer, window: &mut Window, cx: &mut Context<Self>) {
        self.close_composer_now(Some(window), cx);
    }

    fn discard_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(composer) = self.writes.composer.take() {
            self.loaded.drafts.retain(|d| d.target != composer.target);
            self.forget_draft(composer.target, cx);
            self.refresh_rows();
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    // ── resolve ──────────────────────────────────────────────────────────

    fn toggle_thread(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.allowed(Control::Resolve, cx) {
            return;
        }
        let id = self.loaded.threads[index].id;
        if !self.writes.resolving.insert(id) {
            return;
        }
        self.writes.thread_errors.remove(&id);
        let status = review_writes::toggled_status(self.loaded.threads[index].status);
        let before = review_writes::apply_status(&mut self.loaded, index, status);
        // FR-9: the card stays in place in the Threads view; counts update.
        self.keep_card(index);
        self.threads_changed();
        cx.notify();
        let write = ReviewWrite::SetThreadStatus {
            pr: self.pr_ref(),
            thread_id: id,
            status,
        };
        self.send(write, cx, move |this, result, _| {
            this.writes.resolving.remove(&id);
            let Some(index) = review_writes::thread_index(&this.loaded, id) else {
                return;
            };
            match result {
                Ok(Applied::Thread(thread)) => {
                    review_writes::replace_thread(&mut this.loaded, index, thread);
                    this.threads_changed();
                }
                Ok(_) => {}
                Err(error) => {
                    review_writes::apply_status(&mut this.loaded, index, before);
                    this.writes
                        .thread_errors
                        .insert(id, format!("Status not changed: {error}").into());
                    this.threads_changed();
                    this.refresh_rows();
                }
            }
        });
    }

    fn toggle_resolved(&mut self, _: &ToggleResolved, _: &mut Window, cx: &mut Context<Self>) {
        match self.selected_thread() {
            Some(index) => self.toggle_thread(index, cx),
            None => {
                self.writes.notice = Some((ChipTone::Neutral, "Pick a thread with n / p".into()));
                cx.notify();
            }
        }
    }

    // ── vote ─────────────────────────────────────────────────────────────

    pub fn vote(&mut self, vote: Vote, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        if !self.allowed(Control::Vote, cx) || self.writes.voting {
            return;
        }
        if self.loaded.pr().vote_of(&self.loaded.me) == Some(vote) {
            return;
        }
        // AC-9: the header shows the new vote on the next frame.
        let now = self.now();
        let before = review_writes::apply_vote(&mut self.loaded, vote, now);
        self.writes.voting = true;
        self.writes.notice = None;
        cx.notify();
        let write = ReviewWrite::Vote {
            pr: self.pr_ref(),
            reviewer: self.loaded.me.clone(),
            vote,
        };
        self.send(write, cx, move |this, result, _| {
            this.writes.voting = false;
            match result {
                Ok(Applied::Pr(pr)) => this.loaded.detail.pr.reviewers = pr.reviewers,
                Ok(_) => {}
                Err(error) => {
                    review_writes::revert_vote(&mut this.loaded, before);
                    this.writes.notice =
                        Some((ChipTone::Danger, format!("Vote not saved. {error}").into()));
                }
            }
        });
    }

    fn open_vote_menu(&mut self, _: &OpenVoteMenu, window: &mut Window, cx: &mut Context<Self>) {
        if !self.allowed(Control::Vote, cx) {
            return;
        }
        let current = self.loaded.pr().vote_of(&self.loaded.me);
        let selected = VOTE_MENU
            .iter()
            .position(|v| Some(*v) == current)
            .unwrap_or(0);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.clear_overlays();
        self.writes.vote_menu = Some(VoteMenu { selected, focus });
        cx.notify();
    }

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writes.vote_menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Drop every overlay without moving focus (another one is opening).
    pub(super) fn clear_overlays(&mut self) {
        self.writes.vote_menu = None;
        self.writes.sheet = None;
        self.clear_link_overlays();
    }

    /// `↑`/`↓` in whichever menu or picker is open.
    fn step_menu(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(menu) = self.writes.vote_menu.as_mut() {
            menu.selected =
                (menu.selected as isize + delta).clamp(0, VOTE_MENU.len() as isize - 1) as usize;
            cx.notify();
        } else {
            self.step_link_overlay(delta, cx);
        }
    }

    /// `↩` in whichever menu, confirmation or picker is open.
    fn confirm_menu(&mut self, _: &MenuConfirm, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &self.writes.vote_menu {
            let vote = VOTE_MENU[menu.selected];
            self.vote(vote, window, cx);
        } else {
            self.confirm_link_overlay(window, cx);
        }
    }

    // ── complete / auto-complete ─────────────────────────────────────────

    fn open_sheet(&mut self, auto: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.allowed(Control::Complete, cx) {
            return;
        }
        let message = cx.new(|cx| TextInput::new("Merge commit message (optional)", cx));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.clear_overlays();
        self.writes.sheet = Some(Sheet {
            auto,
            options: CompletionOptions::default(),
            message,
            state: SheetState::Idle,
            focus,
        });
        cx.notify();
    }

    fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writes.sheet.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// `Esc` or a click outside: close whatever overlay is open and give the
    /// keyboard back to the review.
    pub(super) fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        self.close_sheet(window, cx);
        self.close_link_overlays(window, cx);
    }

    fn close_overlay(&mut self, _: &CloseOverlay, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlays(window, cx);
    }

    fn edit_sheet(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut CompletionOptions)) {
        if let Some(sheet) = self.writes.sheet.as_mut()
            && !matches!(sheet.state, SheetState::Sending { .. })
        {
            f(&mut sheet.options);
            cx.notify();
        }
    }

    fn submit_sheet(&mut self, auto: bool, cx: &mut Context<Self>) {
        let blocker =
            review_writes::complete_blocker(self.loaded.pr(), self.loaded.is_completing());
        let commit = self.displayed_commit();
        let me = self.loaded.me.clone();
        let pr = self.pr_ref();
        let Some(sheet) = self.writes.sheet.as_mut() else {
            return;
        };
        if matches!(sheet.state, SheetState::Sending { .. }) {
            return;
        }
        if let Some(why) = blocker {
            sheet.state = SheetState::Failed(why.into());
            cx.notify();
            return;
        }
        let Some(commit) = commit else {
            return;
        };
        let mut options = sheet.options.clone();
        let message = sheet.message.read(cx).text().trim().to_owned();
        options.merge_commit_message = (!message.is_empty()).then_some(message);
        sheet.state = SheetState::Sending { auto };
        cx.notify();
        let write = if auto {
            ReviewWrite::SetAutoComplete {
                pr,
                me,
                source_commit: commit.clone(),
                options,
            }
        } else {
            ReviewWrite::Complete {
                pr,
                source_commit: commit.clone(),
                options,
            }
        };
        self.send(write, cx, move |this, result, cx| {
            let state = match result {
                Ok(Applied::Pr(pr)) => {
                    let id = pr.id;
                    let merging = !auto && pr.status == adoken_model::PrStatus::Active;
                    this.loaded.detail.pr = pr;
                    this.writes.sheet = None;
                    this.writes.notice = if auto {
                        this.loaded.auto_complete = Some(commit);
                        Some((
                            ChipTone::Success,
                            format!(
                                "Auto-complete set on !{id}. adoken alerts you if new commits land first."
                            )
                            .into(),
                        ))
                    } else if merging {
                        // Spec 0004 FR-10: accepted, merge queued. The
                        // header says "Completing…" until sync confirms.
                        this.loaded.completing = Some(this.now());
                        this.watch_completing(cx);
                        // Spec 0022: celebrate when the merge is confirmed.
                        crate::celebrate::complete_accepted(id, cx);
                        None
                    } else {
                        // The answer is the confirmed merge (spec 0022 FR-5).
                        crate::celebrate::pr_completed(this.loaded.pr(), &this.loaded.me, cx);
                        Some((ChipTone::Success, format!("Completed !{id}").into()))
                    };
                    cx.focus_later(this.focus.clone());
                    return;
                }
                Ok(_) => return,
                // AC-10: nothing merged, nothing retried.
                Err(WriteError::NewCommits) => SheetState::NewCommits,
                Err(error) => SheetState::Failed(error.to_string().into()),
            };
            if let Some(sheet) = this.writes.sheet.as_mut() {
                sheet.state = state;
            }
        });
    }

    fn cancel_auto_complete(
        &mut self,
        _: &CancelAutoComplete,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.allowed(Control::CancelAutoComplete, cx) {
            return;
        }
        let write = ReviewWrite::CancelAutoComplete { pr: self.pr_ref() };
        self.send(write, cx, |this, result, _| {
            this.writes.notice = Some(match result {
                Ok(_) => {
                    this.loaded.auto_complete = None;
                    (ChipTone::Success, "Auto-complete cancelled".into())
                }
                Err(error) => (
                    ChipTone::Danger,
                    format!("Auto-complete is still set. {error}").into(),
                ),
            });
        });
    }

    /// While my accepted Complete is merging, re-read the PR on every store
    /// commit (sync's follow-up re-read lands through the store).
    pub(super) fn watch_completing(&mut self, cx: &mut Context<Self>) {
        if !self.loaded.is_completing() || self.writes.completing_watch.is_some() {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        self.writes.completing_watch = Some(crate::browse::reload_on_commit(
            &store,
            cx,
            |this: &mut Self, _, cx| this.reread_completing(cx),
        ));
    }

    fn reread_completing(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone().filter(|_| self.loaded.is_completing()) else {
            return;
        };
        let id = self.pr_id();
        let job = cx.background_spawn(async move {
            store.with_conn(|c| {
                Ok((
                    adoken_store::read::pr(c, id)?,
                    adoken_store::read::completing(c, id)?,
                    adoken_store::read::auto_complete(c, id)?,
                ))
            })
        });
        cx.spawn(async move |this, cx| {
            let Ok((Some(pr), completing, auto_complete)) = job.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.loaded.detail.pr = pr;
                this.loaded.completing = completing.map(|(_, at)| at);
                this.loaded.auto_complete = auto_complete.map(|(commit, _)| commit);
                if !this.loaded.is_completing() {
                    this.writes.notice = completing_outcome(this.loaded.pr());
                    // Spec 0022: my pending Complete confirmed (or not).
                    crate::celebrate::pr_seen(this.loaded.pr(), &this.loaded.me, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ── actions ──────────────────────────────────────────────────────────

    pub(super) fn on_write_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &NextLine, _, cx| this.step_line(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousLine, _, cx| this.step_line(false, cx)))
            .on_action(cx.listener(Self::comment_on_line))
            .on_action(cx.listener(Self::comment_on_pr))
            .on_action(cx.listener(Self::reply_to_thread))
            .on_action(cx.listener(Self::toggle_resolved))
            .on_action(cx.listener(Self::post_comment))
            .on_action(cx.listener(Self::close_composer))
            .on_action(cx.listener(Self::open_vote_menu))
            .on_action(cx.listener(|this, _: &Approve, w, cx| this.vote(Vote::Approved, w, cx)))
            .on_action(cx.listener(|this, _: &ApproveWithSuggestions, w, cx| {
                this.vote(Vote::ApprovedWithSuggestions, w, cx)
            }))
            .on_action(cx.listener(|this, _: &WaitForAuthor, w, cx| {
                this.vote(Vote::WaitingForAuthor, w, cx)
            }))
            .on_action(cx.listener(|this, _: &Reject, w, cx| this.vote(Vote::Rejected, w, cx)))
            .on_action(cx.listener(|this, _: &ResetVote, w, cx| this.vote(Vote::NoVote, w, cx)))
            .on_action(cx.listener(|this, _: &MenuNext, _, cx| this.step_menu(1, cx)))
            .on_action(cx.listener(|this, _: &MenuPrevious, _, cx| this.step_menu(-1, cx)))
            .on_action(cx.listener(Self::confirm_menu))
            .on_action(
                cx.listener(|this, _: &OpenCompleteSheet, w, cx| this.open_sheet(false, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenAutoCompleteSheet, w, cx| this.open_sheet(true, w, cx)),
            )
            .on_action(cx.listener(Self::cancel_auto_complete))
            .on_action(cx.listener(|this, _: &CompleteNow, _, cx| this.submit_sheet(false, cx)))
            .on_action(cx.listener(|this, _: &SetAutoComplete, _, cx| this.submit_sheet(true, cx)))
            .on_action(cx.listener(|this, _: &CycleMergeStrategy, _, cx| {
                this.edit_sheet(cx, |o| {
                    let at = MergeStrategy::ALL
                        .iter()
                        .position(|s| *s == o.merge_strategy)
                        .unwrap_or(0);
                    o.merge_strategy = MergeStrategy::ALL[(at + 1) % MergeStrategy::ALL.len()];
                })
            }))
            .on_action(cx.listener(|this, _: &ToggleDeleteSourceBranch, _, cx| {
                this.edit_sheet(cx, |o| o.delete_source_branch = !o.delete_source_branch)
            }))
            .on_action(cx.listener(|this, _: &ToggleTransitionWorkItems, _, cx| {
                this.edit_sheet(cx, |o| o.transition_work_items = !o.transition_work_items)
            }))
            .on_action(cx.listener(Self::close_overlay))
    }

    // ── render ───────────────────────────────────────────────────────────

    /// The header's review controls, or the read-only hint (AC-11).
    pub(super) fn render_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let controls = self.controls();
        // Spec 0019 FR-20: a draft starts its actions row with Publish.
        let publish = self.render_publish(cx);
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let row = div().flex().flex_wrap().items_center().gap(px(space::S8));
        let notice = self.writes.notice.clone().map(|(tone, text)| {
            let (fg, _) = tone.colors(colors);
            div()
                .id("review-notice")
                .role(gpui::Role::Status)
                .text_color(if tone == ChipTone::Neutral {
                    colors.text_secondary
                } else {
                    fg
                })
                .child(text)
        });
        if controls.is_empty() {
            let hint = if self.loaded.read_only {
                READ_ONLY_HINT
            } else {
                "This pull request is closed"
            };
            return row
                .child(
                    div()
                        .id("review-read-only")
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .text_color(colors.text_secondary)
                        .aria_label(hint)
                        .child(
                            Icon::new(IconName::Lock)
                                .size(size::ICON_SM)
                                .color(colors.text_tertiary),
                        )
                        .child(hint),
                )
                .children(notice)
                .into_any_element();
        }
        let kbd = |keys| Kbd::parse(keys).expect("static binding");
        let mine = self.loaded.pr().vote_of(&self.loaded.me);
        let blocker =
            review_writes::complete_blocker(self.loaded.pr(), self.loaded.is_completing());
        let mut row = row.children(publish);
        if controls.contains(&Control::Vote) {
            let approved = mine == Some(Vote::Approved);
            row = row
                .child(
                    Button::new(
                        "review-approve",
                        if approved { "Approved" } else { "Approve" },
                    )
                    .variant(ButtonVariant::Primary)
                    .icon(IconName::BadgeCheck)
                    .disabled(approved)
                    .loading(self.writes.voting)
                    .on_press(press(cx, |this, w, cx| this.vote(Vote::Approved, w, cx))),
                )
                .child(
                    Button::new(
                        "review-vote",
                        match mine {
                            Some(v) if v != Vote::NoVote => v.label(),
                            _ => "Vote…",
                        },
                    )
                    .shortcut(kbd(VOTE_KEYS))
                    .on_press(press(cx, |this, w, cx| {
                        this.open_vote_menu(&OpenVoteMenu, w, cx)
                    })),
                );
        }
        if controls.contains(&Control::Comment) {
            row = row.child(
                Button::new("review-comment", "Comment")
                    .icon(IconName::MessageSquare)
                    .shortcut(kbd(PR_COMMENT_KEYS))
                    .on_press(press(cx, |this, w, cx| {
                        this.comment_on_pr(&CommentOnPullRequest, w, cx)
                    })),
            );
        }
        if controls.contains(&Control::Complete) {
            row = row.child(
                Button::new("review-complete", "Complete…")
                    .icon(IconName::GitMerge)
                    .shortcut(kbd(COMPLETE_KEYS))
                    .disabled(blocker.is_some())
                    .when_some(blocker, |b, why| b.tooltip(why))
                    .on_press(press(cx, |this, w, cx| this.open_sheet(false, w, cx))),
            );
            if let Some(why) = blocker {
                row = row.child(div().text_color(colors.text_tertiary).child(why));
            }
        }
        if controls.contains(&Control::CancelAutoComplete) {
            let commit = self.loaded.auto_complete.as_deref().unwrap_or_default();
            row = row
                .child(
                    Chip::new(format!("Auto-complete set at {}", short(commit)))
                        .tone(ChipTone::Accent),
                )
                .child(
                    Button::new("review-cancel-auto", "Cancel auto-complete")
                        .variant(ButtonVariant::Ghost)
                        .on_press(press(cx, |this, w, cx| {
                            this.cancel_auto_complete(&CancelAutoComplete, w, cx)
                        })),
                );
        }
        row.children(notice).into_any_element()
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let composer = self.writes.composer.as_ref()?;
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let title = match &composer.target {
            DraftTarget::Line {
                side: Side::Right,
                line,
                ..
            } => format!("Comment on line {line}"),
            DraftTarget::Line {
                side: Side::Left,
                line,
                ..
            } => format!("Comment on removed line {line}"),
            DraftTarget::Reply { .. } => "Reply".to_owned(),
            DraftTarget::Pr => format!("Comment on !{}", self.pr_id()),
        };
        let posting = composer.state == ComposerState::Posting;
        let status = match (&composer.state, self.writes.offline) {
            (ComposerState::Posting, _) => Some(Chip::new("Posting…").tone(ChipTone::Accent)),
            (_, true) => Some(Chip::new("Offline · draft saved").tone(ChipTone::Warning)),
            _ => None,
        };
        let failed = match &composer.state {
            ComposerState::Failed(reason) => Some(reason.clone()),
            _ => None,
        };
        let kbd = |keys| Kbd::parse(keys).expect("static binding");
        Some(
            div()
                .id("review-composer")
                .key_context(COMPOSER_CONTEXT)
                .flex()
                .flex_col()
                .gap(px(space::S8))
                // Fills a narrow diff pane instead of overflowing it.
                .w_full()
                .min_w_0()
                .max_w(px(COMPOSER_WIDTH))
                .p(px(space::S12))
                .rounded(px(radius::MD))
                .border_1()
                .border_color(if failed.is_some() {
                    colors.status_danger_fg
                } else {
                    colors.border_focus
                })
                .bg(colors.bg_raised)
                .type_style(typography::SMALL, &theme)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        // Room for the status chip (LABEL + 1 px padding
                        // each side): "Posting…" doesn't nudge the input.
                        .min_h(px(typography::LABEL.line_height + 2.0))
                        .child(
                            div()
                                .text_color(colors.text_secondary)
                                .type_style(typography::LABEL, &theme)
                                .child(title),
                        )
                        .children(status),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(size::CONTROL_LG))
                        .px(px(space::S8))
                        .rounded(px(radius::SM))
                        .border_1()
                        .border_color(colors.border_control)
                        .bg(colors.bg_input)
                        .text_color(if posting {
                            colors.text_secondary
                        } else {
                            colors.text_primary
                        })
                        .type_style(typography::BODY, &theme)
                        .child(composer.input.clone()),
                )
                .children(
                    failed.map(|reason| Notice::new(ChipTone::Danger, "Not posted").detail(reason)),
                )
                .child(
                    div()
                        .flex()
                        // A pane too narrow for the hint beside the buttons
                        // moves it to its own line (still right-aligned),
                        // where it ellipsizes, instead of cutting the key.
                        .flex_wrap()
                        .items_center()
                        .gap(px(space::S8))
                        .child(
                            Button::new("composer-post", "Post")
                                .variant(ButtonVariant::Primary)
                                .shortcut(kbd(POST_KEYS))
                                .loading(posting)
                                .on_press(press(cx, |this, w, cx| {
                                    this.post_comment(&PostComment, w, cx)
                                })),
                        )
                        .child(
                            Button::new("composer-discard", "Discard")
                                .variant(ButtonVariant::Ghost)
                                .disabled(posting)
                                .on_press(press(cx, |this, w, cx| this.discard_composer(w, cx))),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .flex()
                                .items_center()
                                .min_w_0()
                                .overflow_hidden()
                                .gap(px(space::S4))
                                .text_color(colors.text_tertiary)
                                .child(div().flex_none().child(kbd("escape")))
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child("closes, the draft is kept"),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_line_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let bg = cx.theme().colors.bg_code;
        div()
            .pl(px(space::S48))
            .pr(px(space::S16))
            .py(px(space::S6))
            .bg(bg)
            .children(self.render_composer(cx))
            .into_any_element()
    }

    /// The PR-level composer, under the header (Part::All).
    pub(super) fn render_pr_composer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let composer = self.writes.composer.as_ref()?;
        if composer.target != DraftTarget::Pr {
            return None;
        }
        let colors = &cx.theme().colors;
        let (bg, border) = (colors.bg_surface, colors.border_subtle);
        Some(
            div()
                .px(px(space::S16))
                .py(px(space::S8))
                .bg(bg)
                .border_b_1()
                .border_color(border)
                .children(self.render_composer(cx))
                .into_any_element(),
        )
    }

    /// Reply / resolve under a thread's comments, the reply composer, and
    /// the last failure for this thread.
    pub(super) fn render_thread_footer(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let controls = self.controls();
        let thread = &self.loaded.threads[index];
        let id = thread.id;
        let replying = self
            .writes
            .composer
            .as_ref()
            .is_some_and(|c| c.target == DraftTarget::Reply { thread: id });
        let error = self.writes.thread_errors.get(&id).cloned();
        if controls.is_empty() && error.is_none() {
            return None;
        }
        let kbd = |keys| Kbd::parse(keys).expect("static binding");
        let resolving = self.writes.resolving.contains(&id);
        let resolved = thread.status.is_resolved();
        let composer = replying.then(|| self.render_composer(cx)).flatten();
        let buttons = (!replying && !controls.is_empty()).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .when(controls.contains(&Control::Reply), |d| {
                    d.child(
                        Button::new(("thread-reply", index), "Reply")
                            .variant(ButtonVariant::Ghost)
                            .shortcut(kbd(REPLY_KEYS))
                            .on_press(press(cx, move |this, w, cx| this.reply_to(index, w, cx))),
                    )
                })
                .when(controls.contains(&Control::Resolve), |d| {
                    d.child(
                        Button::new(
                            ("thread-resolve", index),
                            if resolved { "Reactivate" } else { "Resolve" },
                        )
                        .variant(ButtonVariant::Ghost)
                        .shortcut(kbd(RESOLVE_KEYS))
                        .loading(resolving)
                        .on_press(press(cx, move |this, _, cx| this.toggle_thread(index, cx))),
                    )
                })
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .children(error.map(|e| Notice::new(ChipTone::Danger, e)))
                .children(composer)
                .children(buttons)
                .into_any_element(),
        )
    }

    pub(super) fn render_overlays(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.render_vote_menu(cx));
        out.extend(self.render_sheet(cx));
        out.extend(self.render_link_overlays(cx));
        out
    }

    fn render_vote_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.writes.vote_menu.as_ref()?;
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let current = self.loaded.pr().vote_of(&self.loaded.me);
        let rows = VOTE_MENU.iter().enumerate().map(|(i, vote)| {
            let vote = *vote;
            let (icon, color) = vote_icon(vote, colors);
            div()
                .id(("vote-menu", i))
                .h(px(size::PALETTE_ROW_COMPACT))
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .rounded(px(radius::SM))
                .cursor_pointer()
                .text_color(colors.text_primary)
                .type_style(typography::BODY, &theme)
                .when(i == menu.selected, |d| d.bg(colors.bg_selected))
                .when(i != menu.selected, |d| d.hover(|s| s.bg(colors.bg_hover)))
                .role(gpui::Role::MenuItem)
                .aria_label(vote_menu_label(vote))
                .child(Icon::new(icon).size(size::ICON_SM).color(color))
                .child(div().flex_1().child(vote_menu_label(vote)))
                .when(Some(vote) == current && vote != Vote::NoVote, |d| {
                    d.child(Chip::new("Current"))
                })
                .on_click(cx.listener(move |this, _, w, cx| this.vote(vote, w, cx)))
        });
        Some(
            div()
                .id("vote-menu")
                .key_context(MENU_CONTEXT)
                .track_focus(&menu.focus)
                .absolute()
                .top(px(space::S64))
                .right(px(space::S16))
                .w(px(MENU_WIDTH))
                .p(px(space::S4))
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_default)
                .bg(colors.bg_overlay)
                .shadow(theme.shadows(elevation::LEVEL_2))
                .role(gpui::Role::Menu)
                .aria_label("Vote")
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_menu(window, cx)))
                .child(
                    div()
                        .px(px(space::S8))
                        .py(px(space::S4))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, &theme)
                        .child("Vote"),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_sheet(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.writes.sheet.as_ref()?;
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let pr = self.loaded.pr();
        let commit = self.displayed_commit().unwrap_or_default();
        let branch = short_branch(&pr.source_branch).to_owned();
        let blocker = review_writes::complete_blocker(pr, self.loaded.is_completing());
        let sending = matches!(sheet.state, SheetState::Sending { .. });
        let kbd = |keys| Kbd::parse(keys).expect("static binding");
        let strategy_ix = MergeStrategy::ALL
            .iter()
            .position(|s| *s == sheet.options.merge_strategy)
            .unwrap_or(0);
        let this = cx.entity().downgrade();
        let toggle = |id: &'static str,
                      label: String,
                      keys: &'static str,
                      on: bool,
                      f: fn(&mut CompletionOptions)| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(space::S8))
                .cursor_pointer()
                .text_color(colors.text_primary)
                .role(gpui::Role::CheckBox)
                .aria_label(label.clone())
                .child(Checkbox::new(on))
                .child(div().flex_1().child(label))
                .child(kbd(keys))
                .on_click(cx.listener(move |this, _, _, cx| this.edit_sheet(cx, f)))
        };
        let delete = toggle(
            "sheet-delete-branch",
            format!("Delete {branch} after merging"),
            DELETE_BRANCH_KEYS,
            sheet.options.delete_source_branch,
            |o| o.delete_source_branch = !o.delete_source_branch,
        );
        let transition = toggle(
            "sheet-transition",
            "Complete linked work items".to_owned(),
            TRANSITION_KEYS,
            sheet.options.transition_work_items,
            |o| o.transition_work_items = !o.transition_work_items,
        );
        let state = match &sheet.state {
            SheetState::NewCommits => Some(
                Notice::new(ChipTone::Danger, "New commits since you reviewed")
                    .detail(format!(
                        "Azure DevOps has commits on {branch} newer than {}. Nothing was merged. Reload the diff and review them first.",
                        short(&commit)
                    ))
                    .into_any_element(),
            ),
            SheetState::Failed(reason) => Some(
                Notice::new(ChipTone::Danger, "Not completed")
                    .detail(reason.clone())
                    .into_any_element(),
            ),
            _ => None,
        };
        let guard = if sheet.auto {
            format!(
                "Auto-complete merges whatever is on {branch} once policies pass, including commits pushed after you set it. adoken remembers {} (the commit you reviewed) and raises a P1 alert with one-key Cancel auto-complete if new commits land first.",
                short(&commit)
            )
        } else {
            format!(
                "Merges exactly {}, the commit you're looking at. If anyone pushed to {branch} since, Azure DevOps refuses and nothing is merged.",
                short(&commit)
            )
        };
        let title = if sheet.auto {
            format!("Set auto-complete on !{}", pr.id)
        } else {
            format!("Complete !{}", pr.id)
        };
        let (primary_auto, secondary_auto) = (sheet.auto, !sheet.auto);
        let label = |auto: bool| {
            if auto {
                "Set auto-complete"
            } else {
                "Complete"
            }
        };
        let keys = |auto: bool| if auto { AUTO_COMPLETE_KEYS } else { POST_KEYS };
        let sending_auto = matches!(sheet.state, SheetState::Sending { auto: true });
        let sending_now = matches!(sheet.state, SheetState::Sending { auto: false });
        let loading = |auto: bool| if auto { sending_auto } else { sending_now };
        Some(
            div()
                .id("complete-sheet-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.bg_scrim)
                .occlude()
                .child(
                    div()
                        .id("complete-sheet")
                        .key_context(SHEET_CONTEXT)
                        .track_focus(&sheet.focus)
                        .role(gpui::Role::Dialog)
                        .aria_label(title.clone())
                        .on_mouse_down_out(
                            cx.listener(|this, _, window, cx| this.close_sheet(window, cx)),
                        )
                        .w(px(SHEET_WIDTH))
                        .flex()
                        .flex_col()
                        .gap(px(space::S12))
                        .p(px(space::S20))
                        .rounded(px(radius::LG))
                        .border_1()
                        .border_color(colors.border_default)
                        .bg(colors.bg_overlay)
                        .shadow(theme.shadows(elevation::LEVEL_3))
                        .type_style(typography::BODY, &theme)
                        .child(
                            div()
                                .text_color(colors.text_primary)
                                .type_style(typography::HEADING, &theme)
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(space::S8))
                                .text_color(colors.text_secondary)
                                .child("Merge type")
                                .child(
                                    Segmented::new(
                                        "sheet-strategy",
                                        MergeStrategy::ALL.map(MergeStrategy::label),
                                        strategy_ix,
                                    )
                                    .on_select(
                                        move |ix, _, cx| {
                                            this.update(cx, |this, cx| {
                                                this.edit_sheet(cx, |o| {
                                                    o.merge_strategy = MergeStrategy::ALL[ix]
                                                })
                                            })
                                            .ok();
                                        },
                                    ),
                                )
                                .child(kbd(STRATEGY_KEYS)),
                        )
                        .child(delete)
                        .child(transition)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(size::CONTROL_LG))
                                .px(px(space::S8))
                                .rounded(px(radius::SM))
                                .border_1()
                                .border_color(colors.border_control)
                                .bg(colors.bg_input)
                                .text_color(colors.text_primary)
                                .child(sheet.message.clone()),
                        )
                        .child(
                            div()
                                .id("sheet-guard")
                                .flex()
                                .gap(px(space::S8))
                                .text_color(colors.text_secondary)
                                .type_style(typography::SMALL, &theme)
                                .child(
                                    Icon::new(IconName::Shield)
                                        .size(size::ICON_MD)
                                        .color(colors.accent_fg),
                                )
                                .child(div().flex_1().min_w_0().child(guard)),
                        )
                        .children(blocker.map(|why| Notice::new(ChipTone::Warning, why)))
                        .children(state)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(space::S8))
                                .child(
                                    Button::new("sheet-primary", label(primary_auto))
                                        .variant(ButtonVariant::Primary)
                                        .shortcut(kbd(keys(primary_auto)))
                                        .disabled(
                                            blocker.is_some()
                                                || (sending && !loading(primary_auto)),
                                        )
                                        .loading(loading(primary_auto))
                                        .on_press(press(cx, move |this, _, cx| {
                                            this.submit_sheet(primary_auto, cx)
                                        })),
                                )
                                .child(
                                    Button::new("sheet-secondary", label(secondary_auto))
                                        .shortcut(kbd(keys(secondary_auto)))
                                        .disabled(
                                            blocker.is_some()
                                                || (sending && !loading(secondary_auto)),
                                        )
                                        .loading(loading(secondary_auto))
                                        .on_press(press(cx, move |this, _, cx| {
                                            this.submit_sheet(secondary_auto, cx)
                                        })),
                                )
                                .child(div().flex_1())
                                .child(
                                    Button::new("sheet-cancel", "Cancel")
                                        .variant(ButtonVariant::Ghost)
                                        .shortcut(kbd("escape"))
                                        .on_press(press(cx, |this, w, cx| this.close_sheet(w, cx))),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    // ── stories ──────────────────────────────────────────────────────────

    /// Stories: a line composer on the busiest file's first commented line
    /// with `text`, in `state`.
    pub fn story_composer(&mut self, text: &str, state: ComposerState, cx: &mut Context<Self>) {
        let Some(ix) = self
            .rows
            .iter()
            .position(|r| matches!(r, PaneRow::Thread(_)))
            .and_then(|t| (0..t).rev().find(|ix| Self::is_line_row(&self.rows[*ix])))
        else {
            return;
        };
        let Some((display, side, line)) = self.line_target(ix) else {
            return;
        };
        let path = self.loaded.files[self.selected].path.clone();
        self.open_composer(
            DraftTarget::Line { path, side, line },
            Some((self.selected, display)),
            text,
            false,
            None,
            cx,
        );
        if let Some(composer) = self.writes.composer.as_mut() {
            composer.state = state;
            composer.save = None;
        }
        self.line_cursor = Some(ix);
        self.scroll_diff_to(ix.saturating_sub(4), cx);
    }

    /// Stories: the vote menu open (focus stays where it is).
    pub fn story_vote_menu(&mut self, cx: &mut Context<Self>) {
        self.writes.vote_menu = Some(VoteMenu {
            selected: 0,
            focus: cx.focus_handle(),
        });
        cx.notify();
    }

    /// Stories: the Complete / auto-complete sheet in `state`.
    pub fn story_sheet(&mut self, auto: bool, state: SheetState, cx: &mut Context<Self>) {
        let message = cx.new(|cx| TextInput::new("Merge commit message (optional)", cx));
        self.writes.sheet = Some(Sheet {
            auto,
            options: CompletionOptions {
                merge_strategy: MergeStrategy::Squash,
                ..CompletionOptions::default()
            },
            message,
            state,
            focus: cx.focus_handle(),
        });
        cx.notify();
    }

    /// Stories: a header outcome line (e.g. a reverted vote).
    pub fn story_notice(&mut self, tone: ChipTone, text: &str, cx: &mut Context<Self>) {
        self.writes.notice = Some((tone, text.to_owned().into()));
        cx.notify();
    }

    /// Stories: the Read-only access state (AC-11).
    pub fn story_read_only(&mut self, cx: &mut Context<Self>) {
        self.loaded.read_only = true;
        cx.notify();
    }

    /// Stories: my Complete accepted, Azure DevOps still merging (spec 0004
    /// FR-10): the header's "Completing…" state.
    pub fn story_completing(&mut self, cx: &mut Context<Self>) {
        self.loaded.detail.pr.status = adoken_model::PrStatus::Active;
        self.loaded.detail.pr.merge_status = adoken_model::MergeStatus::Queued;
        self.loaded.completing = Some(self.loaded.now);
        cx.notify();
    }

    /// Stories: my queued Complete failed in Azure DevOps (the reason
    /// shows; Complete is offered again).
    pub fn story_complete_failed(&mut self, cx: &mut Context<Self>) {
        self.loaded.detail.pr.status = adoken_model::PrStatus::Active;
        self.loaded.detail.pr.merge_status = adoken_model::MergeStatus::RejectedByPolicy;
        self.loaded.completing = None;
        self.writes.notice = completing_outcome(self.loaded.pr());
        cx.notify();
    }
}

/// The header line once my queued Complete stopped merging.
fn completing_outcome(pr: &adoken_model::PullRequest) -> Option<(ChipTone, SharedString)> {
    if pr.status == adoken_model::PrStatus::Completed {
        return Some((ChipTone::Success, format!("Completed !{}", pr.id).into()));
    }
    review_writes::merge_problem(pr).map(|why| {
        (
            ChipTone::Danger,
            format!("Azure DevOps couldn't complete !{}: {why}", pr.id).into(),
        )
    })
}

trait FocusLater {
    fn focus_later(&mut self, focus: FocusHandle);
}

impl FocusLater for Context<'_, PrReview> {
    /// After a sheet or composer closes from an async result there is no
    /// window at hand: move focus back to the review on the next frame.
    /// Callers pass the handle from the `&mut PrReview` they already hold:
    /// reading the entity here would re-lease it mid-update and panic.
    fn focus_later(&mut self, focus: FocusHandle) {
        self.defer(move |cx| {
            if let Some(window) = cx.active_window() {
                window
                    .update(cx, |_, window, cx| window.focus(&focus, cx))
                    .ok();
            }
        });
    }
}
