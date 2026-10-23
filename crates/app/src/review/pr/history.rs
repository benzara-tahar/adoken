//! A review's restorable state for back / forward (spec 0018 FR-5):
//! spec 0019's Files / Threads state, the selected file by path, the tree's
//! and the diff's scroll anchors, and the line and thread cursors. Rows are
//! named by value ([`PaneRow`]), so the same file's diff lands on the same
//! line. A file a new iteration dropped selects the file now at its tree
//! position with the diff from the top (FR-7). When the selected file's
//! diff isn't computed yet, the diff part applies once it lands (FR-8).

use std::time::Instant;

use adoken_theme::size;
use gpui::{Context, ListOffset, Window, px};

use super::super::data::PaneRow;
use super::{PrReview, ReviewPolishState};
use crate::shell::history::{Pending, ScrollAnchor};

/// The first visible diff row and how far it's scrolled past.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DiffAnchor {
    row: PaneRow,
    /// Its index, for when that row is gone.
    ix: usize,
    offset: f32,
}

/// What the diff pane restores once the selected file's diff exists.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DiffRestore {
    file: usize,
    top: Option<DiffAnchor>,
    line: Option<(PaneRow, usize)>,
    thread: Option<(PaneRow, usize)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewState {
    polish: ReviewPolishState,
    file: Option<String>,
    /// The selected file's position among the listed files (FR-7).
    position: Option<usize>,
    /// Tree scroll: a row of `LoadedPr::tree` and px past it.
    tree: Option<ScrollAnchor<usize>>,
    top: Option<DiffAnchor>,
    line: Option<(PaneRow, usize)>,
    thread: Option<(PaneRow, usize)>,
}

/// `↩` on a Threads card switched to Files on its thread (spec 0019
/// FR-13): an open, with the review as it was before (spec 0018 FR-3.4).
#[derive(Debug, Clone)]
pub struct ThreadOpened(pub Box<ReviewState>);

impl gpui::EventEmitter<ThreadOpened> for PrReview {}

/// Where a review is, as the user sees it (smoke runs): the selected
/// file, the first visible diff row and px past it, the line cursor's row
/// and the tree's first visible row and px past it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewPosition {
    pub file: Option<String>,
    pub diff_top: (usize, f32),
    pub line_cursor: Option<usize>,
    pub tree_top: Option<ScrollAnchor<usize>>,
}

impl PrReview {
    /// What the review shows now (smoke runs).
    pub fn position(&self) -> ReviewPosition {
        let top = self.diff_list.logical_scroll_top();
        ReviewPosition {
            file: self.loaded.files.get(self.selected).map(|f| f.path.clone()),
            diff_top: (top.item_ix, top.offset_in_item / px(1.0)),
            line_cursor: self.line_cursor,
            tree_top: self.tree_anchor(),
        }
    }

    /// A restore is waiting for the selected file's diff (FR-8).
    pub fn restore_pending(&self) -> bool {
        self.history_pending.is_some()
    }

    /// The state to keep in the history entry (no IO, no list-sized work).
    pub fn history_state(&self) -> ReviewState {
        if let Some(pending) = &self.history_pending {
            // Left before a restore applied: keep what it would show.
            let restore = pending.peek();
            return ReviewState {
                polish: self.polish_state(),
                file: self.loaded.files.get(restore.file).map(|f| f.path.clone()),
                position: self.listed_files().iter().position(|f| *f == restore.file),
                tree: self.tree_anchor(),
                top: restore.top,
                line: restore.line,
                thread: restore.thread,
            };
        }
        let top = self.diff_list.logical_scroll_top();
        let at = |ix: Option<usize>| ix.and_then(|ix| Some((*self.rows.get(ix)?, ix)));
        ReviewState {
            polish: self.polish_state(),
            file: self.loaded.files.get(self.selected).map(|f| f.path.clone()),
            position: self.listed_files().iter().position(|f| *f == self.selected),
            tree: self.tree_anchor(),
            top: self.rows.get(top.item_ix).map(|row| DiffAnchor {
                row: *row,
                ix: top.item_ix,
                offset: top.offset_in_item / px(1.0),
            }),
            line: at(self.line_cursor),
            thread: at(self.thread_cursor),
        }
    }

    /// Back / forward landed on this review: everything as it was, the
    /// keyboard in the review.
    pub fn restore_history_state(
        &mut self,
        state: ReviewState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.history_pending = None;
        self.restore_polish_state(state.polish, window, cx);
        let by_path = state
            .file
            .as_deref()
            .and_then(|path| self.loaded.files.iter().position(|f| f.path == path));
        let listed = self.listed_files();
        let file = by_path.or_else(|| {
            let position = state.position?.min(listed.len().checked_sub(1)?);
            Some(listed[position])
        });
        if let Some(file) = file
            && file != self.selected
        {
            self.select_file(file, cx);
        }
        if let Some(tree) = state.tree {
            let ix = self.tree_rows().iter().position(|row| *row == tree.row);
            if let Some(ix) = ix {
                ScrollAnchor::<usize>::apply(&self.tree_scroll, size::CONTROL_MD, ix, tree.offset);
            }
        }
        // A dropped file shows its replacement from the top (FR-7).
        let restore = DiffRestore {
            file: self.selected,
            top: state.top.filter(|_| by_path.is_some()),
            line: state.line.filter(|_| by_path.is_some()),
            thread: state.thread.filter(|_| by_path.is_some()),
        };
        if self.diff_ready() {
            self.apply_diff_restore(restore);
        } else {
            self.history_pending = Some(Pending::new(restore, Instant::now()));
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Input reached the review before its diff landed: the user wins.
    pub fn cancel_pending_restore(&mut self) {
        self.history_pending = None;
    }

    /// The selected file's diff arrived: a waiting restore applies now.
    pub(super) fn try_history_restore(&mut self) {
        let Some(pending) = self.history_pending.take() else {
            return;
        };
        if let Some(restore) = pending.take(Instant::now())
            && restore.file == self.selected
        {
            self.apply_diff_restore(restore);
        }
    }

    fn diff_ready(&self) -> bool {
        self.loaded
            .files
            .get(self.selected)
            .is_some_and(|f| f.diff.is_some() || f.unavailable.is_some())
    }

    fn apply_diff_restore(&mut self, restore: DiffRestore) {
        let last = self.rows.len().saturating_sub(1);
        // The same row, else the row now at its index (the nearest earlier
        // line when lines were dropped above it).
        let find = |row: PaneRow, ix: usize| {
            self.rows
                .iter()
                .position(|r| *r == row)
                .unwrap_or(ix.min(last))
        };
        match restore.top {
            Some(top) => {
                let item_ix = find(top.row, top.ix);
                self.diff_list.scroll_to(ListOffset {
                    item_ix,
                    offset_in_item: px(top.offset),
                });
            }
            None => self.diff_list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            }),
        }
        let found = |cursor: Option<(PaneRow, usize)>| {
            let (row, _) = cursor?;
            self.rows.iter().position(|r| *r == row)
        };
        self.line_cursor = found(restore.line);
        self.thread_cursor = found(restore.thread);
    }

    fn tree_anchor(&self) -> Option<ScrollAnchor<usize>> {
        ScrollAnchor::of_list(&self.tree_scroll, size::CONTROL_MD, |ix| {
            self.tree_rows().get(ix).copied()
        })
    }
}
