//! Back / forward across views (spec 0018): the pure model the shell keeps
//! one of per main window, plus the small helpers every view uses to
//! capture and restore its own state by identity (scroll anchors, deferred
//! restores, destination labels).
//!
//! The history is an ordered list of entries and a cursor, in memory only
//! (FR-1, Q-1). A push drops every entry after the cursor and, past
//! [`CAPACITY`], the oldest one (FR-9). The shell captures the current
//! view's state into the entry under the cursor before every push, back
//! and forward (FR-5), so forward returns to the exact state too.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui::{UniformListScrollHandle, point, px};

/// At most this many entries (FR-9).
pub const CAPACITY: usize = 50;
/// A restore still waiting for its view's data after this long is dropped
/// (FR-8).
pub const RESTORE_GIVE_UP: Duration = Duration::from_secs(5);
/// Destination titles ellipsize past this many characters (§3.3).
pub const TITLE_MAX: usize = 48;

/// Entries and a cursor; `E` is the shell's entry (location + view state),
/// or a fake in tests.
#[derive(Debug, Clone)]
pub struct History<E> {
    entries: VecDeque<E>,
    cursor: usize,
}

impl<E> History<E> {
    /// The launch entry (FR-1).
    pub fn new(first: E) -> Self {
        Self {
            entries: VecDeque::from([first]),
            cursor: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Index of the current entry, oldest first.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn current(&self) -> &E {
        &self.entries[self.cursor]
    }

    /// Where captured view state goes before the cursor leaves (FR-5).
    pub fn current_mut(&mut self) -> &mut E {
        &mut self.entries[self.cursor]
    }

    /// The entry back would show.
    pub fn peek_back(&self) -> Option<&E> {
        self.cursor.checked_sub(1).map(|ix| &self.entries[ix])
    }

    /// The entry forward would show.
    pub fn peek_forward(&self) -> Option<&E> {
        self.entries.get(self.cursor + 1)
    }

    /// An open: `entry` becomes current, forward entries are dropped, and
    /// the oldest entry goes past [`CAPACITY`].
    pub fn push(&mut self, entry: E) {
        self.entries.truncate(self.cursor + 1);
        self.entries.push_back(entry);
        if self.entries.len() > CAPACITY {
            self.entries.pop_front();
        }
        self.cursor = self.entries.len() - 1;
    }

    /// Moves the cursor back; `None` (and no move) at the oldest entry.
    pub fn back(&mut self) -> Option<&E> {
        self.cursor = self.cursor.checked_sub(1)?;
        Some(&self.entries[self.cursor])
    }

    /// Moves the cursor forward; `None` (and no move) at the newest entry.
    pub fn forward(&mut self) -> Option<&E> {
        if self.cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        Some(&self.entries[self.cursor])
    }

    /// Every entry, oldest first (size checks, tests).
    pub fn entries(&self) -> impl Iterator<Item = &E> {
        self.entries.iter()
    }
}

/// What an entry is about within its location, for "an open of the place
/// already shown pushes nothing" (FR-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    WorkItem(i64),
    Run(i64),
    Definition(i64),
}

/// A list's scroll position by identity (FR-5): the first visible row and
/// how many pixels of it are scrolled past, so rows inserted above it or a
/// changed sort don't move what the user was looking at.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollAnchor<K> {
    pub row: K,
    pub offset: f32,
}

impl<K> ScrollAnchor<K> {
    /// The anchor of a uniform list whose rows are `row_height` tall;
    /// `key_at` names the row at an index. `None` before the list scrolled
    /// or when the row isn't listed.
    pub fn of_list(
        scroll: &UniformListScrollHandle,
        row_height: f32,
        key_at: impl FnOnce(usize) -> Option<K>,
    ) -> Option<Self> {
        let top = -(scroll.0.borrow().base_handle.offset().y / px(1.0));
        let (ix, offset) = split_offset(top, row_height);
        Some(Self {
            row: key_at(ix)?,
            offset,
        })
    }

    /// Scrolls a uniform list so row `ix` is first visible, `offset` px of
    /// it scrolled past (the list clamps at its end when it paints).
    pub fn apply(scroll: &UniformListScrollHandle, row_height: f32, ix: usize, offset: f32) {
        let mut state = scroll.0.borrow_mut();
        state.deferred_scroll_to_item = None;
        state
            .base_handle
            .set_offset(point(px(0.0), px(-(ix as f32 * row_height + offset))));
    }
}

/// `top` px scrolled into rows of `row_height`: the first visible row and
/// how far into it.
pub fn split_offset(top: f32, row_height: f32) -> (usize, f32) {
    if top <= 0.0 || row_height <= 0.0 {
        return (0, 0.0);
    }
    let ix = (top / row_height).floor();
    (ix as usize, top - ix * row_height)
}

/// A restore waiting for its view's data (FR-8): user input cancels it
/// (the view drops it), and it gives up after [`RESTORE_GIVE_UP`].
#[derive(Debug, Clone)]
pub struct Pending<S> {
    state: S,
    since: Instant,
}

impl<S> Pending<S> {
    pub fn new(state: S, now: Instant) -> Self {
        Self { state, since: now }
    }

    /// The state to apply now that the data landed, unless it waited too
    /// long.
    pub fn take(self, now: Instant) -> Option<S> {
        (now.saturating_duration_since(self.since) <= RESTORE_GIVE_UP).then_some(self.state)
    }

    /// The waiting state (captured again if the user leaves before it
    /// applied).
    pub fn peek(&self) -> &S {
        &self.state
    }
}

/// `title` cut to [`TITLE_MAX`] characters with an ellipsis (§3.3).
pub fn truncate_title(title: &str) -> String {
    let title = title.trim();
    if title.chars().count() <= TITLE_MAX {
        return title.to_owned();
    }
    let mut cut: String = title.chars().take(TITLE_MAX - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake entry: a place and the row selected there.
    #[derive(Debug, Clone, PartialEq)]
    struct Fake {
        place: &'static str,
        row: usize,
    }

    fn at(place: &'static str) -> Fake {
        Fake { place, row: 0 }
    }

    fn places(history: &History<Fake>) -> Vec<&'static str> {
        history.entries().map(|e| e.place).collect()
    }

    #[test]
    fn launch_holds_one_entry_with_nothing_either_way() {
        let mut history = History::new(at("today"));
        assert_eq!(history.len(), 1);
        assert!(history.peek_back().is_none() && history.peek_forward().is_none());
        assert!(history.back().is_none());
        assert!(history.forward().is_none());
        assert_eq!(history.current().place, "today");
    }

    #[test]
    fn a_push_after_going_back_drops_the_forward_entries() {
        let mut history = History::new(at("a"));
        history.push(at("b"));
        history.push(at("c"));
        assert_eq!(history.back().map(|e| e.place), Some("b"));
        history.push(at("d"));
        assert_eq!(places(&history), ["a", "b", "d"]);
        assert!(history.peek_forward().is_none(), "forward disabled");
        assert_eq!(history.peek_back().map(|e| e.place), Some("b"));
    }

    #[test]
    fn keeps_the_newest_fifty_and_backs_stop_at_the_oldest() {
        let mut history = History::new(Fake {
            place: "launch",
            row: 0,
        });
        for row in 1..=60 {
            history.push(Fake { place: "p", row });
        }
        assert_eq!(history.len(), CAPACITY);
        let rows: Vec<usize> = history.entries().map(|e| e.row).collect();
        assert_eq!(rows, (11..=60).collect::<Vec<_>>());
        for _ in 0..CAPACITY - 1 {
            assert!(history.back().is_some());
        }
        assert_eq!(history.current().row, 11, "the oldest kept entry");
        assert!(history.back().is_none(), "a 50th back does nothing");
        assert_eq!(history.cursor(), 0);
    }

    #[test]
    fn forward_returns_the_state_captured_when_leaving() {
        let mut history = History::new(at("a"));
        history.push(Fake { place: "b", row: 2 });
        // On B the selection moves to row 5; the shell captures it into
        // the current entry before the cursor leaves.
        history.current_mut().row = 5;
        history.back();
        assert_eq!(history.forward().map(|e| e.row), Some(5));
    }

    #[test]
    fn scroll_offsets_split_into_first_row_and_remainder() {
        assert_eq!(split_offset(0.0, 40.0), (0, 0.0));
        assert_eq!(split_offset(-3.0, 40.0), (0, 0.0));
        assert_eq!(split_offset(130.0, 40.0), (3, 10.0));
        assert_eq!(split_offset(120.0, 40.0), (3, 0.0));
    }

    #[test]
    fn anchors_survive_rows_inserted_above() {
        let scroll = UniformListScrollHandle::new();
        let rows = ["n1", "n2", "n3", "n4", "n5", "n6"];
        ScrollAnchor::<&str>::apply(&scroll, 40.0, 3, 12.0);
        let anchor =
            ScrollAnchor::of_list(&scroll, 40.0, |ix| rows.get(ix).copied()).expect("an anchor");
        assert_eq!(
            anchor,
            ScrollAnchor {
                row: "n4",
                offset: 12.0
            }
        );
        // Two newer rows arrive on top: the anchor row is now 6th.
        let rows = ["x", "y", "n1", "n2", "n3", "n4", "n5", "n6"];
        let ix = rows.iter().position(|r| *r == anchor.row).expect("listed");
        ScrollAnchor::<&str>::apply(&scroll, 40.0, ix, anchor.offset);
        let again = ScrollAnchor::of_list(&scroll, 40.0, |ix| rows.get(ix).copied());
        assert_eq!(again, Some(anchor));
    }

    #[test]
    fn pending_restores_give_up_after_five_seconds() {
        let start = Instant::now();
        let pending = Pending::new("state", start);
        assert_eq!(
            pending.clone().take(start + Duration::from_millis(300)),
            Some("state")
        );
        assert_eq!(pending.clone().take(start + RESTORE_GIVE_UP), Some("state"));
        assert_eq!(
            pending.take(start + RESTORE_GIVE_UP + Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn titles_truncate_at_48_characters() {
        assert_eq!(
            truncate_title("Retry idempotency keys"),
            "Retry idempotency keys"
        );
        let long = "a".repeat(60);
        let cut = truncate_title(&long);
        assert_eq!(cut.chars().count(), TITLE_MAX);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate_title(&"b".repeat(48)), "b".repeat(48));
    }
}
