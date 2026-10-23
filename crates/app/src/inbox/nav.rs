//! The Inbox's restorable state for back / forward (spec 0018 FR-5, FR-6,
//! FR-7): the filter chips, the selected notification by id, and the
//! list's scroll anchor. A notification captured as it was opened stays
//! listed when the Inbox comes back, even though opening read it and it no
//! longer matches Unread, until the selection leaves it or a filter
//! changes.

use std::time::Instant;

use adoken_notify::{Engine, Filter, NotificationId, Priority, Source, StateFilter};
use gpui::{Context, ScrollStrategy, Window};

use super::{InboxView, ROW_HEIGHT, rows};
use crate::shell::history::{Pending, ScrollAnchor};

#[derive(Debug, Clone, PartialEq)]
pub struct InboxNavState {
    pub filter: Filter,
    pub selected: Option<NotificationId>,
    /// The selection's row, for when that notification is gone (FR-7).
    pub position: usize,
    /// Captured as `selected` was opened (FR-6).
    pub opened: bool,
    pub scroll: Option<ScrollAnchor<NotificationId>>,
}

impl InboxNavState {
    /// "Inbox", or "Inbox · Unread · Pull requests" with filters set (§3.3).
    pub fn label(&self) -> String {
        let filter = &self.filter;
        let mut parts = vec!["Inbox"];
        if filter.state != StateFilter::default() {
            parts.push(filter.state.label());
        }
        parts.extend(
            Source::ALL
                .into_iter()
                .filter(|s| filter.has_source(*s))
                .map(Source::label),
        );
        parts.extend(
            Priority::ALL
                .into_iter()
                .filter(|p| filter.has_priority(*p))
                .map(Priority::label),
        );
        parts.join(" · ")
    }
}

/// The rows for the engine's filter, plus `sticky` (an opened row that no
/// longer matches, FR-6) in its sort place while it's still in the inbox.
pub(super) fn listed(engine: &Engine, sticky: Option<NotificationId>) -> Vec<NotificationId> {
    let filter = engine.filter();
    let list = engine.list(&filter);
    let mut rows: Vec<NotificationId> = list.iter().map(|n| n.id).collect();
    if let Some(n) = sticky.and_then(|id| engine.get(id))
        && n.in_inbox()
        && !rows.contains(&n.id)
    {
        // `list` sorts newest activity first, then the higher id.
        let at = list
            .iter()
            .position(|row| (row.updated_at, row.id) < (n.updated_at, n.id))
            .unwrap_or(rows.len());
        rows.insert(at, n.id);
    }
    rows
}

/// The row to select: `selected` where it's listed, else the row now at
/// its old `position`, else the last row (FR-7).
pub(super) fn restored_row(
    rows: &[NotificationId],
    selected: Option<NotificationId>,
    position: usize,
) -> usize {
    selected
        .and_then(|id| rows.iter().position(|row| *row == id))
        .unwrap_or_else(|| rows::clamp_selection(position, rows.len()))
}

impl InboxView {
    /// The selected notification (smoke runs).
    pub fn selected_notification(&self) -> Option<NotificationId> {
        self.selected_id()
    }

    /// The listed notifications in row order (smoke runs).
    pub fn row_ids(&self) -> &[NotificationId] {
        &self.rows
    }

    /// The list's scroll offset (px from the top) as last painted.
    pub fn scroll_top(&self) -> f32 {
        -(self.scroll.0.borrow().base_handle.offset().y / gpui::px(1.0))
    }

    /// The state to keep in the history entry (no IO, no list-sized work).
    pub fn nav_state(&self) -> InboxNavState {
        InboxNavState {
            filter: self.engine.filter(),
            selected: self.selected_id(),
            position: self.selected,
            opened: false,
            scroll: ScrollAnchor::of_list(&self.scroll, ROW_HEIGHT, |ix| {
                self.rows.get(ix).copied()
            }),
        }
    }

    /// Back / forward landed here: filter, selection and scroll as they
    /// were; the list has the keyboard. Waits for the first load if it
    /// hasn't landed (FR-8).
    pub fn restore_nav_state(
        &mut self,
        state: InboxNavState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if !self.loaded {
            self.pending_restore = Some(Pending::new(state, Instant::now()));
            return;
        }
        self.apply_nav_state(state, cx);
    }

    /// Input reached the view before a deferred restore could apply: the
    /// user wins (FR-8).
    pub fn cancel_pending_restore(&mut self) {
        self.pending_restore = None;
    }

    /// The first load landed: apply a waiting restore unless it's stale.
    pub(super) fn apply_pending_restore(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self
            .pending_restore
            .take()
            .and_then(|pending| pending.take(Instant::now()))
        {
            self.apply_nav_state(state, cx);
        }
    }

    fn apply_nav_state(&mut self, state: InboxNavState, cx: &mut Context<Self>) {
        self.menu = None;
        self.status = None;
        self.engine.set_filter(state.filter);
        self.sticky = state.selected.filter(|_| state.opened);
        self.refresh(cx);
        self.selected = restored_row(&self.rows, state.selected, state.position);
        let anchor = state.scroll.and_then(|anchor| {
            let ix = self.rows.iter().position(|row| *row == anchor.row)?;
            Some((ix, anchor.offset))
        });
        match anchor {
            Some((ix, offset)) => {
                ScrollAnchor::<NotificationId>::apply(&self.scroll, ROW_HEIGHT, ix, offset)
            }
            None => self
                .scroll
                .scroll_to_item(self.selected, ScrollStrategy::Top),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use adoken_notify::{Engine, Filter, Source, StateFilter};

    use super::*;
    use crate::inbox::demo;

    fn unread_prs() -> Engine {
        let mut engine = Engine::from_state(Arc::new(demo::clock()), demo::state());
        let mut filter = Filter {
            state: StateFilter::Unread,
            ..Filter::default()
        };
        filter.toggle_source(Source::PullRequests);
        engine.set_filter(filter);
        engine
    }

    #[test]
    fn labels_name_the_filters_set() {
        let mut state = InboxNavState {
            filter: Filter::default(),
            selected: None,
            position: 0,
            opened: false,
            scroll: None,
        };
        assert_eq!(state.label(), "Inbox");
        state.filter.state = StateFilter::Unread;
        state.filter.toggle_source(Source::PullRequests);
        assert_eq!(state.label(), "Inbox · Unread · Pull requests");
    }

    /// Owner scenario B on the view model: the row opened under Unread is
    /// listed and selected again after back, at its place, and the next
    /// row is the one that followed it.
    #[test]
    fn an_opened_row_stays_listed_and_selected_under_unread() {
        let mut engine = unread_prs();
        let before = listed(&engine, None);
        assert!(before.len() >= 3, "the demo has unread PR rows");
        let (opened, next) = (before[1], before[2]);
        engine.open(opened);
        assert!(!listed(&engine, None).contains(&opened), "read rows leave");
        let rows = listed(&engine, Some(opened));
        assert_eq!(rows, before, "listed again, in place");
        let ix = restored_row(&rows, Some(opened), 1);
        assert_eq!(rows[ix], opened);
        assert_eq!(rows[ix + 1], next, "↓ goes to the row that followed");
    }

    /// FR-7: a selection that's gone (marked done elsewhere) selects the
    /// row now at its place, else the last row, and isn't kept listed.
    #[test]
    fn a_row_gone_selects_its_old_position() {
        let mut engine = unread_prs();
        let before = listed(&engine, None);
        let gone = before[1];
        engine.mark_done(gone);
        let rows = listed(&engine, Some(gone));
        assert!(!rows.contains(&gone), "done rows don't stick");
        assert_eq!(rows[restored_row(&rows, Some(gone), 1)], before[2]);
        assert_eq!(restored_row(&rows, Some(gone), 99), rows.len() - 1);
        assert_eq!(restored_row(&[], Some(gone), 1), 0, "empty state");
    }
}
