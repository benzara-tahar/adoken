//! Today's restorable state for back / forward (spec 0018 FR-5, spec 0017
//! FR-27): the selected row by what it shows, and the scroll position by
//! row identity, the first visible row and how many px of it are scrolled
//! past. Rows that appeared above it while the user was away don't move
//! their place. Row bounds are only known once the restored rows have
//! painted, so the scroll part applies from the prepaint after them
//! ([`TodayView::render_scroll_restore`]); before the first day lands both
//! wait (FR-8).

use std::time::Instant;

use gpui::{Bounds, Context, Pixels, Point, Window, canvas, point, prelude::*, px};

use super::model::{self, Row, RowKey};
use super::{State, TodayView};
use crate::shell::history::ScrollAnchor;

#[derive(Clone, Debug, PartialEq)]
pub struct TodayNavState {
    pub selected: Option<RowKey>,
    /// `None` at the top.
    pub scroll: Option<ScrollAnchor<RowKey>>,
}

impl TodayView {
    /// The state to keep in the history entry (no IO, no list-sized work
    /// beyond the painted rows).
    pub fn nav_state(&self) -> TodayNavState {
        TodayNavState {
            selected: self.selected,
            scroll: self.scroll_anchor(),
        }
    }

    /// The list's scroll offset (smoke runs).
    pub fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll.offset()
    }

    /// Back / forward landed here: the row selected and the scroll as it
    /// was. Before the first day lands the restore waits (dropped on any
    /// input or after 5 s). The keyboard returns to the list (FR-16).
    pub fn restore_nav_state(
        &mut self,
        state: TodayNavState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if matches!(self.state, State::Ready(_)) {
            self.apply_restore(state, cx);
        } else {
            self.pending_restore = Some((state, Instant::now()));
        }
    }

    /// Input reached the view first: the user wins (FR-8).
    pub fn cancel_pending_restore(&mut self) {
        self.pending_restore = None;
        self.scroll_restore.take();
    }

    pub(super) fn apply_restore(&mut self, state: TodayNavState, cx: &mut Context<Self>) {
        self.selected = state.selected;
        self.clamp_selection();
        match state.scroll {
            Some(anchor) => self.scroll_restore.set(Some(anchor)),
            None => {
                self.scroll_restore.take();
                self.scroll.set_offset(point(px(0.0), px(0.0)));
            }
        }
        cx.notify();
    }

    /// The first painted row crossing the viewport's top, unless at the top.
    fn scroll_anchor(&self) -> Option<ScrollAnchor<RowKey>> {
        // Left before a restore applied: keep what it would show.
        let waiting = self.scroll_restore.take();
        self.scroll_restore.set(waiting.clone());
        if waiting.is_some() {
            return waiting;
        }
        if self.scroll.offset().y >= px(0.0) {
            return None;
        }
        let loaded = self.loaded()?;
        let rows = self.rows.borrow();
        let (row, offset) = first_visible(
            rows.iter().map(|(row, bounds)| (*row, *bounds)),
            self.scroll.bounds().top(),
        )?;
        Some(ScrollAnchor {
            row: model::key(loaded, row)?,
            offset,
        })
    }

    /// Paints nothing; once the rows of this frame recorded their bounds,
    /// scrolls a waiting anchor's row back to its offset and asks for the
    /// next frame. A row that no longer shows drops the anchor (the
    /// selection already fell back, FR-7).
    pub(super) fn render_scroll_restore(&self) -> impl IntoElement {
        let anchor = self.scroll_restore.take();
        self.scroll_restore.set(anchor.clone());
        let target = anchor.and_then(|anchor| {
            let loaded = self.loaded()?;
            let row = model::find(loaded, &self.layout(self.wide()), anchor.row);
            Some((row, anchor.offset))
        });
        let pending = self.scroll_restore.clone();
        let rows = self.rows.clone();
        let scroll = self.scroll.clone();
        canvas(
            move |_, window, _| {
                let Some((row, offset)) = target else {
                    return;
                };
                pending.take();
                let Some(bounds) = row.and_then(|row: Row| rows.borrow().get(&row).copied()) else {
                    return;
                };
                let current = scroll.offset();
                let y = restored_offset(offset, bounds.top(), scroll.bounds().top(), current.y);
                scroll.set_offset(point(current.x, y));
                window.request_animation_frame();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }
}

/// Of painted rows, the first one crossing `top` (the topmost of those
/// still visible), with how many px of it are above `top`.
fn first_visible<K>(
    rows: impl Iterator<Item = (K, Bounds<Pixels>)>,
    top: Pixels,
) -> Option<(K, f32)> {
    let (key, bounds) = rows
        .filter(|(_, bounds)| bounds.bottom() > top)
        .min_by(|(_, a), (_, b)| f32::from(a.top()).total_cmp(&f32::from(b.top())))?;
    Some((key, (top - bounds.top()) / px(1.0)))
}

/// The scroll offset that puts a row now painted at `row_top` (under scroll
/// offset `current`) back at `offset` px above the viewport's `top`.
fn restored_offset(offset: f32, row_top: Pixels, top: Pixels, current: Pixels) -> Pixels {
    current + (top - px(offset) - row_top)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn row(y: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(y)), size(px(100.0), px(height)))
    }

    #[test]
    fn the_anchor_is_the_row_crossing_the_top_and_how_far_past_it() {
        // Viewport top at 100: row b (90..130) crosses it, 10 px past.
        let rows = [
            ("a", row(40.0, 40.0)),
            ("b", row(90.0, 40.0)),
            ("c", row(140.0, 40.0)),
        ];
        assert_eq!(
            first_visible(rows.into_iter(), px(100.0)),
            Some(("b", 10.0))
        );
        // A gap above the first visible row: it's below the top.
        let rows = [("a", row(40.0, 40.0)), ("c", row(110.0, 40.0))];
        assert_eq!(
            first_visible(rows.into_iter(), px(100.0)),
            Some(("c", -10.0))
        );
        // Two columns: the topmost visible row of either.
        let rows = [("left", row(95.0, 60.0)), ("right", row(70.0, 50.0))];
        assert_eq!(
            first_visible(rows.into_iter(), px(100.0)),
            Some(("right", 30.0))
        );
        assert_eq!(
            first_visible([("a", row(0.0, 50.0))].into_iter(), px(100.0)),
            None
        );
    }

    #[test]
    fn rows_inserted_above_scroll_the_anchor_back_to_its_place() {
        // Captured: viewport top 100, row 10 px past it under offset -300.
        // Two 40 px rows arrived above it: it now paints 80 px lower.
        let y = restored_offset(10.0, px(170.0), px(100.0), px(-300.0));
        assert_eq!(y, px(-380.0));
        // Under that offset it paints at 90 again: 10 px past the top.
        assert_eq!(px(170.0) + (y - px(-300.0)), px(90.0));
    }
}
