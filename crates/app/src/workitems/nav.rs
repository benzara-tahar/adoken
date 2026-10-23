//! Work items' restorable state for back / forward (spec 0018 FR-5): tab
//! and area / iteration path, the selected item by id, the item the detail
//! shows, list and detail scroll, the detail's cursor stop and which pane
//! has the keyboard.

use std::time::Instant;

use gpui::{App, Context, ScrollStrategy, Window, point, px};

use super::detail::model::Stop;
use super::model::{self, ListFilter, Row};
use super::{Detail, Phase, ROW_HEIGHT, WorkItemsView};
use crate::shell::history::{Pending, ScrollAnchor, Subject, truncate_title};

#[derive(Debug, Clone, PartialEq)]
pub struct WorkItemsNavState {
    filter: ListFilter,
    selected: Option<i64>,
    /// The selection's row, for when that item is gone (FR-7).
    position: Option<usize>,
    /// The item the detail shows (it can be outside the tab).
    detail: Option<i64>,
    list_scroll: Option<ScrollAnchor<i64>>,
    detail_ui: DetailRestore,
    /// "#20101 Fix login redirect" when an item is shown.
    title: Option<String>,
}

/// What the detail pane restores once its item has loaded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DetailRestore {
    pub scroll: f32,
    pub cursor: Option<Stop>,
    pub focused: bool,
    pub stacked: bool,
}

impl WorkItemsNavState {
    /// "Work items", or the item shown (§3.3).
    pub fn label(&self) -> String {
        match &self.title {
            Some(title) => truncate_title(title),
            None => "Work items".into(),
        }
    }

    pub fn subject(&self) -> Option<Subject> {
        self.detail.or(self.selected).map(Subject::WorkItem)
    }
}

impl WorkItemsView {
    /// The state to keep in the history entry (no IO, no list-sized work).
    pub fn nav_state(&self, window: &Window, cx: &App) -> WorkItemsNavState {
        let title = match &self.detail {
            Detail::Ready(detail) => Some(format!("#{} {}", detail.item.id, detail.item.title)),
            _ => None,
        };
        WorkItemsNavState {
            filter: self.list_filter(),
            selected: self.selected_id(),
            position: self.selected,
            detail: self.detail_id(),
            list_scroll: self.list_anchor(),
            detail_ui: DetailRestore {
                scroll: -(self.detail_scroll.offset().y / px(1.0)),
                cursor: self.ui.cursor,
                focused: self.detail_focus.contains_focused(window, cx),
                stacked: self.stacked_detail,
            },
            title,
        }
    }

    /// Back / forward landed here. Waits for the list if it's still
    /// loading (FR-8).
    pub fn restore_nav_state(
        &mut self,
        state: WorkItemsNavState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.list_focus, cx);
        if !matches!(self.phase, Phase::Ready) {
            self.pending_restore = Some(Pending::new(state, Instant::now()));
            return;
        }
        self.apply_nav_state(state, Some(window), cx);
    }

    /// Input reached the view first: the user wins (FR-8).
    pub fn cancel_pending_restore(&mut self) {
        self.pending_restore = None;
        self.restore_detail = None;
    }

    /// The list landed: apply a waiting restore unless it's stale.
    pub(super) fn apply_pending_restore(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self
            .pending_restore
            .take()
            .and_then(|pending| pending.take(Instant::now()))
        {
            self.apply_nav_state(state, None, cx);
        }
    }

    /// The detail landed: its scroll, cursor and focus as captured.
    pub(super) fn apply_detail_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(restore) = self
            .restore_detail
            .take()
            .and_then(|pending| pending.take(Instant::now()))
        else {
            return;
        };
        if matches!(self.detail, Detail::Ready(_)) {
            self.set_detail_ui(restore, window, cx);
        }
    }

    /// `window` is `None` when the list landed after the restore was asked:
    /// the detail part then applies when the detail (re)loads.
    fn apply_nav_state(
        &mut self,
        state: WorkItemsNavState,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.restore_list_filter(state.filter, cx);
        let count = self.rows.len();
        self.selected = state
            .selected
            .and_then(|id| model::row_of(&self.rows, &self.list.items, id))
            .or_else(|| {
                // Gone: the row now at its place, else the last row (FR-7).
                let position = state.position?.min(count.checked_sub(1)?);
                model::step(&self.rows, Some(position), 0)
            });
        let anchor = state.list_scroll.and_then(|anchor| {
            let ix = model::row_of(&self.rows, &self.list.items, anchor.row)?;
            Some((ix, anchor.offset))
        });
        match anchor {
            Some((ix, offset)) => {
                ScrollAnchor::<i64>::apply(&self.list_scroll, ROW_HEIGHT, ix, offset)
            }
            None => {
                if let Some(ix) = self.selected {
                    self.list_scroll.scroll_to_item(ix, ScrollStrategy::Top);
                }
            }
        }
        let detail = state.detail.or_else(|| self.selected_id());
        match (detail, window) {
            (Some(id), Some(window))
                if self.detail_id() == Some(id) && matches!(self.detail, Detail::Ready(_)) =>
            {
                self.restore_detail = None;
                self.set_detail_ui(state.detail_ui, window, cx);
            }
            (Some(id), _) => {
                self.load_detail(id, cx);
                self.restore_detail = Some(Pending::new(state.detail_ui, Instant::now()));
                self.stacked_detail = state.detail_ui.stacked;
            }
            (None, _) => {
                self.detail = Detail::Nothing;
                self._detail = None;
            }
        }
        cx.notify();
    }

    fn set_detail_ui(
        &mut self,
        restore: DetailRestore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail_scroll
            .set_offset(point(px(0.0), px(-restore.scroll)));
        self.ui.cursor = restore.cursor;
        self.stacked_detail = restore.stacked;
        if restore.focused {
            window.focus(&self.detail_focus, cx);
        } else {
            window.focus(&self.list_focus, cx);
        }
    }

    /// The first visible item row and how far it's scrolled past; a state
    /// header on top anchors to the item under it.
    fn list_anchor(&self) -> Option<ScrollAnchor<i64>> {
        let ScrollAnchor { row, offset } =
            ScrollAnchor::of_list(&self.list_scroll, ROW_HEIGHT, Some)?;
        self.rows
            .iter()
            .enumerate()
            .skip(row)
            .find_map(|(ix, r)| match r {
                Row::Item(item) => Some((ix, self.list.items.get(*item)?.id)),
                Row::Header { .. } => None,
            })
            .map(|(ix, id)| ScrollAnchor {
                row: id,
                offset: offset - (ix - row) as f32 * ROW_HEIGHT,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workitems::model::Tab;

    #[test]
    fn the_shown_item_is_the_subject_and_names_the_entry() {
        let mut state = WorkItemsNavState {
            filter: ListFilter {
                tab: Tab::MyWork,
                area: None,
                iteration: None,
            },
            selected: Some(1),
            position: Some(0),
            detail: Some(20101),
            list_scroll: None,
            detail_ui: DetailRestore {
                scroll: 0.0,
                cursor: None,
                focused: false,
                stacked: false,
            },
            title: Some("#20101 Fix login redirect".into()),
        };
        assert_eq!(state.subject(), Some(Subject::WorkItem(20101)));
        assert_eq!(state.label(), "#20101 Fix login redirect");
        state.detail = None;
        state.title = None;
        assert_eq!(state.subject(), Some(Subject::WorkItem(1)));
        assert_eq!(state.label(), "Work items");
    }
}
