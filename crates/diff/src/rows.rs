//! Row layouts over a [`FileDiff`]: every visible row is an index, so a view
//! virtualizes by row count and renders only what's on screen (FR-4).
//!
//! Unchanged runs longer than `2 × CONTEXT` collapse into a [`Fold`] row
//! (spec 0004 §3: "unchanged regions collapsed with expanders"). A fold is
//! identified by the display index of its first hidden line, so the same
//! expanded set works for both layouts.

use std::collections::HashSet;

use crate::{FileDiff, LineKind};

/// Unchanged lines kept visible around each change.
pub const CONTEXT: usize = 3;

/// A collapsed run of unchanged lines: display indices `start..start + len`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fold {
    pub start: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedRow {
    Line(u32),
    Fold(Fold),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitRow {
    /// Left = old-side line (context or `-`), right = new-side line (context
    /// or `+`). A context line sits on both sides.
    Pair {
        left: Option<u32>,
        right: Option<u32>,
    },
    Fold(Fold),
}

impl UnifiedRow {
    /// First display line this row covers; rows are sorted by it.
    pub fn first_line(self) -> u32 {
        match self {
            Self::Line(ix) => ix,
            Self::Fold(fold) => fold.start,
        }
    }
}

impl SplitRow {
    pub fn first_line(self) -> u32 {
        match self {
            Self::Pair { left, right } => match (left, right) {
                (Some(l), Some(r)) => l.min(r),
                (Some(ix), None) | (None, Some(ix)) => ix,
                (None, None) => 0,
            },
            Self::Fold(fold) => fold.start,
        }
    }

    pub fn contains(self, line: u32) -> bool {
        match self {
            Self::Pair { left, right } => left == Some(line) || right == Some(line),
            Self::Fold(fold) => (fold.start..fold.start + fold.len).contains(&line),
        }
    }
}

/// Every fold the diff can have, expanded or not, in display order.
pub fn folds(diff: &FileDiff) -> Vec<Fold> {
    if diff.is_unchanged() {
        return Vec::new();
    }
    let total = diff.lines.len();
    let mut out = Vec::new();
    let mut ix = 0;
    while ix < total {
        if diff.lines[ix].kind != LineKind::Context {
            ix += 1;
            continue;
        }
        let run_start = ix;
        while ix < total && diff.lines[ix].kind == LineKind::Context {
            ix += 1;
        }
        let lead = if run_start == 0 { 0 } else { CONTEXT };
        let trail = if ix == total { 0 } else { CONTEXT };
        let hidden_start = run_start + lead;
        let hidden_end = ix.saturating_sub(trail);
        // Hiding a single line behind an expander costs more than showing it.
        if hidden_end > hidden_start + 1 {
            out.push(Fold {
                start: hidden_start as u32,
                len: (hidden_end - hidden_start) as u32,
            });
        }
    }
    out
}

/// One row per visible line, collapsed folds as one row each.
pub fn unified(diff: &FileDiff, expanded: &HashSet<u32>) -> Vec<UnifiedRow> {
    let mut rows = Vec::with_capacity(diff.lines.len());
    let mut next = 0u32;
    for fold in folds(diff) {
        if expanded.contains(&fold.start) {
            continue;
        }
        rows.extend((next..fold.start).map(UnifiedRow::Line));
        rows.push(UnifiedRow::Fold(fold));
        next = fold.start + fold.len;
    }
    rows.extend((next..diff.lines.len() as u32).map(UnifiedRow::Line));
    rows
}

/// Side-by-side rows: each change block's deletions and additions pair up by
/// position; the longer side gets one-sided rows.
pub fn split(diff: &FileDiff, expanded: &HashSet<u32>) -> Vec<SplitRow> {
    let unified = unified(diff, expanded);
    let mut rows = Vec::with_capacity(unified.len());
    let mut dels: Vec<u32> = Vec::new();
    let mut adds: Vec<u32> = Vec::new();
    let flush = |rows: &mut Vec<SplitRow>, dels: &mut Vec<u32>, adds: &mut Vec<u32>| {
        for i in 0..dels.len().max(adds.len()) {
            rows.push(SplitRow::Pair {
                left: dels.get(i).copied(),
                right: adds.get(i).copied(),
            });
        }
        dels.clear();
        adds.clear();
    };
    for row in unified {
        match row {
            UnifiedRow::Fold(fold) => {
                flush(&mut rows, &mut dels, &mut adds);
                rows.push(SplitRow::Fold(fold));
            }
            UnifiedRow::Line(ix) => match diff.lines[ix as usize].kind {
                LineKind::Del => {
                    // A deletion after additions starts a new block.
                    if !adds.is_empty() {
                        flush(&mut rows, &mut dels, &mut adds);
                    }
                    dels.push(ix);
                }
                LineKind::Add => adds.push(ix),
                LineKind::Context => {
                    flush(&mut rows, &mut dels, &mut adds);
                    rows.push(SplitRow::Pair {
                        left: Some(ix),
                        right: Some(ix),
                    });
                }
            },
        }
    }
    flush(&mut rows, &mut dels, &mut adds);
    rows
}

/// Row index showing display line `line` in unified rows (a fold if hidden).
pub fn unified_row_of(rows: &[UnifiedRow], line: u32) -> Option<usize> {
    let ix = rows.partition_point(|row| row.first_line() <= line);
    let candidate = ix.checked_sub(1)?;
    match rows[candidate] {
        UnifiedRow::Line(l) if l == line => Some(candidate),
        UnifiedRow::Fold(fold) if (fold.start..fold.start + fold.len).contains(&line) => {
            Some(candidate)
        }
        _ => None,
    }
}

/// Row index showing display line `line` in split rows. Rows in a change
/// block aren't strictly sorted by their first line, so this scans a small
/// window back from the sorted position.
pub fn split_row_of(rows: &[SplitRow], line: u32) -> Option<usize> {
    let upper = rows.partition_point(|row| row.first_line() <= line);
    (0..upper)
        .rev()
        .find(|&ix| rows[ix].contains(line))
        .or_else(|| {
            rows[upper..]
                .iter()
                .position(|row| row.contains(line))
                .map(|ix| ix + upper)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("{i}\n")).collect()
    }

    fn with_changes(n: usize, changed: &[usize]) -> String {
        (1..=n)
            .map(|i| {
                if changed.contains(&i) {
                    format!("{i} changed\n")
                } else {
                    format!("{i}\n")
                }
            })
            .collect()
    }

    #[test]
    fn long_unchanged_runs_fold_with_context() {
        let diff = FileDiff::from_strs(&numbered(40), &with_changes(40, &[20]));
        let folds = folds(&diff);
        // Head run: lines 1..19 (idx 0..19) keeps 3 trailing context → hide 0..16.
        // Tail run: idx 21..41 (after -20 +20) keeps 3 leading → hide from 24.
        assert_eq!(
            folds,
            [Fold { start: 0, len: 16 }, Fold { start: 24, len: 17 }]
        );
        let rows = unified(&diff, &HashSet::new());
        assert_eq!(rows.len(), 1 + 3 + 2 + 3 + 1);
        assert_eq!(rows[0], UnifiedRow::Fold(folds[0]));
        assert_eq!(rows[1], UnifiedRow::Line(16));
    }

    #[test]
    fn expanding_a_fold_shows_its_lines() {
        let diff = FileDiff::from_strs(&numbered(40), &with_changes(40, &[20]));
        let expanded = HashSet::from([0]);
        let rows = unified(&diff, &expanded);
        assert_eq!(rows[0], UnifiedRow::Line(0));
        assert_eq!(rows.len(), 16 + 3 + 2 + 3 + 1);
    }

    #[test]
    fn unchanged_file_and_short_gaps_do_not_fold() {
        let same = FileDiff::from_strs(&numbered(50), &numbered(50));
        assert!(folds(&same).is_empty());
        // Changes 7 lines apart leave a 6-line gap = 2 × CONTEXT: nothing to hide.
        let near = FileDiff::from_strs(&numbered(12), &with_changes(12, &[2, 9]));
        assert!(folds(&near).iter().all(|f| f.start > 10));
    }

    #[test]
    fn split_pairs_blocks_by_position() {
        let diff = FileDiff::from_strs("a\nb\nc\nd\n", "a\nB\nd\ne\n");
        // display: " a", "-b", "-c", "+B", " d", "+e"
        let rows = split(&diff, &HashSet::new());
        assert_eq!(
            rows,
            [
                SplitRow::Pair {
                    left: Some(0),
                    right: Some(0)
                },
                SplitRow::Pair {
                    left: Some(1),
                    right: Some(3)
                },
                SplitRow::Pair {
                    left: Some(2),
                    right: None
                },
                SplitRow::Pair {
                    left: Some(4),
                    right: Some(4)
                },
                SplitRow::Pair {
                    left: None,
                    right: Some(5)
                },
            ]
        );
        assert_eq!(split_row_of(&rows, 3), Some(1));
        assert_eq!(split_row_of(&rows, 2), Some(2));
        assert_eq!(split_row_of(&rows, 5), Some(4));
    }

    #[test]
    fn row_lookup_lands_on_folds_for_hidden_lines() {
        let diff = FileDiff::from_strs(&numbered(40), &with_changes(40, &[20]));
        let rows = unified(&diff, &HashSet::new());
        assert_eq!(unified_row_of(&rows, 5), Some(0));
        assert_eq!(unified_row_of(&rows, 16), Some(1));
        let split_rows = split(&diff, &HashSet::new());
        assert_eq!(split_row_of(&split_rows, 30), Some(split_rows.len() - 1));
    }
}
