//! Pull requests' restorable state for back / forward (spec 0018 FR-5):
//! the list (tab, repository filter, selected PR by id, scroll anchor) and
//! an open review (the list state for `Esc`, plus the review's own state,
//! [`pr::history::ReviewState`]). A review whose entity was replaced
//! reopens through the usual open path, from the local store, and its
//! state applies when the file list and diff land (FR-8, FR-13).

use std::time::Instant;

use gpui::{App, Context, Focusable as _, ScrollStrategy, Window};

use super::data::ListTab;
use super::pr::history::ReviewState;
use super::{Detail, ROW_HEIGHT, ReviewView};
use crate::shell::history::{Pending, ScrollAnchor, truncate_title};

/// The PR list (and, inside a review, where `Esc` returns).
#[derive(Debug, Clone, PartialEq)]
pub struct PrListNavState {
    tab: ListTab,
    repo: Option<String>,
    selected: Option<i64>,
    /// The selection's row, for when that PR is gone (FR-7).
    position: Option<usize>,
    scroll: Option<ScrollAnchor<i64>>,
}

/// An open review of PR `pr`.
#[derive(Debug, Clone)]
pub struct ReviewNavState {
    pr: i64,
    title: Option<String>,
    list: PrListNavState,
    /// `None` while the review is still opening, isn't synced or failed.
    review: Option<ReviewState>,
}

impl ReviewNavState {
    pub fn pr(&self) -> i64 {
        self.pr
    }

    /// "!4821 Retry idempotency keys" (§3.3).
    pub fn label(&self) -> String {
        match &self.title {
            Some(title) => format!("!{} {}", self.pr, truncate_title(title)),
            None => format!("!{}", self.pr),
        }
    }

    /// The list state the review keeps for `Esc` (FR-12).
    pub fn list(&self) -> &PrListNavState {
        &self.list
    }
}

/// What the Pull requests view shows: its list or a review.
#[derive(Debug, Clone)]
pub enum PullRequestsNavState {
    List(PrListNavState),
    Review(Box<ReviewNavState>),
}

impl ReviewView {
    /// The PR whose review pane shows (opening, open, not synced).
    pub fn shown_review_id(&self, cx: &App) -> Option<i64> {
        match &self.detail {
            Detail::Opening(id) | Detail::Missing(id) => Some(*id),
            Detail::Open(review) => Some(review.read(cx).pr_id()),
            Detail::Failed(_) => self.failed_pr,
            Detail::Nothing => None,
        }
    }

    /// The state to keep in the history entry (no IO, no list-sized work).
    pub fn nav_state(&self, cx: &App) -> PullRequestsNavState {
        let list = self.list_nav_state();
        let Some(pr) = self.shown_review_id(cx) else {
            return PullRequestsNavState::List(list);
        };
        let (title, review) = match &self.detail {
            Detail::Open(review) => {
                let review = review.read(cx);
                (
                    Some(review.pr_title().to_owned()),
                    Some(review.history_state()),
                )
            }
            _ => (
                self.list
                    .as_ref()
                    .and_then(|l| l.prs.iter().find(|p| p.id == pr))
                    .map(|p| p.title.clone()),
                // Still opening: keep what a pending restore would apply.
                self.pending_review
                    .as_ref()
                    .filter(|(id, _)| *id == pr)
                    .map(|(_, pending)| pending.peek().clone()),
            ),
        };
        PullRequestsNavState::Review(Box::new(ReviewNavState {
            pr,
            title,
            list,
            review,
        }))
    }

    /// A review entry for `pr` holding `review`.
    pub(super) fn review_nav_state(
        &self,
        pr: i64,
        review: ReviewState,
        cx: &App,
    ) -> ReviewNavState {
        let title = match &self.detail {
            Detail::Open(open) => Some(open.read(cx).pr_title().to_owned()),
            _ => None,
        };
        ReviewNavState {
            pr,
            title,
            list: self.list_nav_state(),
            review: Some(review),
        }
    }

    fn list_nav_state(&self) -> PrListNavState {
        PrListNavState {
            tab: self.tab,
            repo: self.repo.clone(),
            selected: self.selected_pr_id(),
            position: self.selected,
            scroll: ScrollAnchor::of_list(&self.list_scroll, ROW_HEIGHT, |ix| {
                let list = self.list.as_ref()?;
                Some(list.prs.get(*self.visible.get(ix)?)?.id)
            }),
        }
    }

    /// Back / forward to the list: the review closes (its draft is kept,
    /// 0004 FR-8), the list shows as it was and has the keyboard.
    pub fn restore_list(
        &mut self,
        state: PrListNavState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_review = None;
        self.close_review(cx);
        self.apply_list(state, true, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Back / forward to a review: shown from memory when it's the open
    /// one, else reopened from the local store with its state applied once
    /// the review lands (FR-8, FR-13). A PR gone from the store shows the
    /// review's "isn't synced" bar (FR-7).
    pub fn restore_review(
        &mut self,
        state: ReviewNavState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_list(state.list.clone(), false, cx);
        let live = match &self.detail {
            Detail::Open(review) if review.read(cx).pr_id() == state.pr => Some(review.clone()),
            _ => None,
        };
        match live {
            Some(review) => {
                self.pending_review = None;
                if let Some(review_state) = state.review {
                    review.update(cx, |review, cx| {
                        review.restore_history_state(review_state, window, cx)
                    });
                }
                window.focus(&review.focus_handle(cx), cx);
            }
            None => {
                self.pending_review = state
                    .review
                    .map(|review| (state.pr, Pending::new(review, Instant::now())));
                self.show_review(state.pr, window, cx);
            }
        }
        cx.notify();
    }

    /// Input reached the view first: the user wins (FR-8).
    pub fn cancel_pending_restore(&mut self, cx: &mut Context<Self>) {
        self.pending_restore_list = None;
        self.pending_review = None;
        if let Detail::Open(review) = &self.detail {
            review.update(cx, |review, _| review.cancel_pending_restore());
        }
    }

    /// The review of `pr` was built: hand it a waiting restore.
    pub(super) fn review_built(&mut self, pr: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, pending)) = self.pending_review.take() else {
            return;
        };
        let Detail::Open(review) = &self.detail else {
            return;
        };
        if id != pr {
            return;
        }
        if let Some(state) = pending.take(Instant::now()) {
            review.update(cx, |review, cx| {
                review.restore_history_state(state, window, cx)
            });
        }
    }

    /// The list landed: apply a list state that waited for it.
    pub(super) fn list_loaded(&mut self, cx: &mut Context<Self>) {
        if let Some((state, scroll)) = self
            .pending_restore_list
            .take()
            .and_then(|pending| pending.take(Instant::now()))
        {
            self.apply_list(state, scroll, cx);
        }
    }

    /// Tab, repository, selection by id (else the row at its place) and,
    /// when `scroll`, the scroll anchor.
    fn apply_list(&mut self, state: PrListNavState, scroll: bool, cx: &mut Context<Self>) {
        if self.list.is_none() {
            self.pending_restore_list = Some(Pending::new((state, scroll), Instant::now()));
            return;
        }
        self.tab = state.tab;
        self.repo = state.repo;
        self.refilter(cx);
        let count = self.visible.len();
        let row_of = |this: &Self, id: i64| {
            let list = this.list.as_ref()?;
            this.visible.iter().position(|ix| list.prs[*ix].id == id)
        };
        self.selected = state
            .selected
            .and_then(|id| row_of(self, id))
            .or_else(|| Some(state.position?.min(count.checked_sub(1)?)));
        if !scroll {
            return;
        }
        match state
            .scroll
            .and_then(|anchor| Some((row_of(self, anchor.row)?, anchor.offset)))
        {
            Some((ix, offset)) => {
                ScrollAnchor::<i64>::apply(&self.list_scroll, ROW_HEIGHT, ix, offset)
            }
            None => {
                if let Some(ix) = self.selected {
                    self.list_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_labels_lead_with_the_id_and_truncate_the_title() {
        let list = PrListNavState {
            tab: ListTab::ToReview,
            repo: None,
            selected: None,
            position: None,
            scroll: None,
        };
        let state = ReviewNavState {
            pr: 4821,
            title: Some("Retry idempotency keys".into()),
            list,
            review: None,
        };
        assert_eq!(state.label(), "!4821 Retry idempotency keys");
        let long = ReviewNavState {
            title: Some("x".repeat(80)),
            ..state.clone()
        };
        assert!(long.label().starts_with("!4821 "));
        assert_eq!(long.label().chars().count(), "!4821 ".len() + 48);
        let opening = ReviewNavState {
            title: None,
            ..state
        };
        assert_eq!(opening.label(), "!4821");
    }
}
