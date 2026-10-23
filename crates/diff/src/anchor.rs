//! FR-5 thread anchoring. A thread names lines on one side of the diff as of
//! the iteration it was left on. When that differs from the displayed
//! iteration, its right-side lines are mapped through a diff of the file at
//! the thread's iteration against the displayed file: lines that survived
//! unchanged move with the code, lines that were edited or deleted can't be
//! placed and go to "Comments on older code" (AC-6).

use crate::{FileDiff, LineKind, Side, Text};

/// Maps one-based line numbers of an older version of a file to the current
/// version. Only unchanged lines map.
#[derive(Debug, Clone)]
pub struct LineMap {
    to: Vec<Option<u32>>,
}

impl LineMap {
    pub fn new(from: &str, to: &str) -> Self {
        Self::from_diff(&FileDiff::compute(Text::new(from), Text::new(to)))
    }

    pub fn from_diff(diff: &FileDiff) -> Self {
        let mut to = vec![None; diff.old.len()];
        for line in &diff.lines {
            if let (LineKind::Context, Some(old), Some(new)) = (line.kind, line.old, line.new) {
                to[old as usize - 1] = Some(new);
            }
        }
        Self { to }
    }

    pub fn map(&self, line: u32) -> Option<u32> {
        let slot = (line as usize).checked_sub(1)?;
        self.to.get(slot).copied().flatten()
    }
}

/// A thread's position as stored: side plus inclusive one-based range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub side: Side,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Show after display line `line` (the range's last line) with the range
    /// resolved to the displayed file's numbering.
    Inline { line: usize, start: u32, end: u32 },
    /// Lines no longer exist in the displayed diff.
    Older,
}

/// Places one thread. `map` is `Some` when the thread's iteration differs
/// from the displayed one and the file at its iteration is known; `stale`
/// says the iteration differs but no content is available to map with, so
/// right-side positions can't be trusted.
pub fn place(diff: &FileDiff, pos: Position, map: Option<&LineMap>, stale: bool) -> Placement {
    let (start, end) = match (pos.side, map) {
        (Side::Right, Some(map)) => match (map.map(pos.start), map.map(pos.end.max(pos.start))) {
            (Some(start), Some(end)) if end >= start => (start, end),
            _ => return Placement::Older,
        },
        (Side::Right, None) if stale => return Placement::Older,
        _ => (pos.start, pos.end.max(pos.start)),
    };
    match (diff.index_of(pos.side, start), diff.index_of(pos.side, end)) {
        (Some(_), Some(line)) => Placement::Inline { line, start, end },
        _ => Placement::Older,
    }
}

/// Threads split into inline (sorted by display line, stable for equal lines)
/// and older-code lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchored<K> {
    pub inline: Vec<(usize, K)>,
    pub older: Vec<K>,
}

impl<K> Default for Anchored<K> {
    fn default() -> Self {
        Self {
            inline: Vec::new(),
            older: Vec::new(),
        }
    }
}

impl<K> Anchored<K> {
    /// Threads anchored after display line `line`.
    pub fn at(&self, line: usize) -> impl Iterator<Item = &K> {
        let from = self.inline.partition_point(|(l, _)| *l < line);
        self.inline[from..]
            .iter()
            .take_while(move |(l, _)| *l == line)
            .map(|(_, key)| key)
    }
}

pub fn anchor_all<'a, K>(
    diff: &FileDiff,
    threads: impl IntoIterator<Item = (K, Position, Option<&'a LineMap>, bool)>,
) -> Anchored<K> {
    let mut out = Anchored::default();
    for (key, pos, map, stale) in threads {
        match place(diff, pos, map, stale) {
            Placement::Inline { line, .. } => out.inline.push((line, key)),
            Placement::Older => out.older.push(key),
        }
    }
    out.inline.sort_by_key(|(line, _)| *line);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(range: std::ops::RangeInclusive<u32>, prefix: &str) -> String {
        range.map(|i| format!("{prefix}{i}\n")).collect()
    }

    fn right(start: u32, end: u32) -> Position {
        Position {
            side: Side::Right,
            start,
            end,
        }
    }

    /// AC-6: iteration 2 → 3 inserted 15 lines above lines 40–42 and deleted
    /// lines 60–62.
    fn iterations() -> (String, String) {
        let iter2 = numbered(1..=80, "line ");
        let iter3 = format!(
            "{}{}{}{}",
            numbered(1..=10, "line "),
            numbered(1..=15, "inserted "),
            numbered(11..=59, "line "),
            numbered(63..=80, "line "),
        );
        (iter2, iter3)
    }

    #[test]
    fn thread_follows_moved_lines_across_iterations() {
        let (iter2, iter3) = iterations();
        let base = numbered(1..=80, "line ");
        let diff = FileDiff::from_strs(&base, &iter3);
        let map = LineMap::new(&iter2, &iter3);
        match place(&diff, right(40, 42), Some(&map), true) {
            Placement::Inline { line, start, end } => {
                assert_eq!((start, end), (55, 57));
                assert_eq!(diff.lines[line].new, Some(57));
            }
            Placement::Older => panic!("moved lines must map"),
        }
    }

    #[test]
    fn thread_on_deleted_lines_goes_to_older_code() {
        let (iter2, iter3) = iterations();
        let diff = FileDiff::from_strs(&numbered(1..=80, "line "), &iter3);
        let map = LineMap::new(&iter2, &iter3);
        assert_eq!(
            place(&diff, right(60, 61), Some(&map), true),
            Placement::Older
        );
        // Range whose end was deleted can't be placed either.
        assert_eq!(
            place(&diff, right(58, 60), Some(&map), true),
            Placement::Older
        );
    }

    #[test]
    fn same_iteration_places_directly_and_rejects_out_of_range() {
        let diff = FileDiff::from_strs("a\nb\n", "a\nB\nc\n");
        let placed = place(&diff, right(3, 3), None, false);
        assert_eq!(
            placed,
            Placement::Inline {
                line: 3,
                start: 3,
                end: 3
            }
        );
        assert_eq!(place(&diff, right(9, 9), None, false), Placement::Older);
        let left = Position {
            side: Side::Left,
            start: 2,
            end: 2,
        };
        assert_eq!(
            place(&diff, left, None, false),
            Placement::Inline {
                line: 1,
                start: 2,
                end: 2
            }
        );
    }

    #[test]
    fn stale_right_thread_without_content_is_older_but_left_side_stays() {
        let diff = FileDiff::from_strs("a\nb\n", "a\nB\n");
        assert_eq!(place(&diff, right(1, 1), None, true), Placement::Older);
        let left = Position {
            side: Side::Left,
            start: 1,
            end: 1,
        };
        assert!(matches!(
            place(&diff, left, None, true),
            Placement::Inline { .. }
        ));
    }

    #[test]
    fn anchor_all_sorts_and_groups_by_line() {
        let diff = FileDiff::from_strs("a\nb\nc\n", "a\nb\nc\n");
        let anchored = anchor_all(
            &diff,
            [
                ("late", right(3, 3), None, false),
                ("gone", right(7, 7), None, false),
                ("early", right(1, 1), None, false),
                ("late-2", right(2, 3), None, false),
            ],
        );
        assert_eq!(anchored.older, ["gone"]);
        assert_eq!(anchored.at(0).copied().collect::<Vec<_>>(), ["early"]);
        assert_eq!(
            anchored.at(2).copied().collect::<Vec<_>>(),
            ["late", "late-2"]
        );
        assert_eq!(anchored.at(1).count(), 0);
    }
}
