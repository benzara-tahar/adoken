use std::collections::HashMap;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};

use crate::{Side, Text};

/// Marks a line with no intraline partner.
pub const NO_PAIR: u32 = u32::MAX;

/// Past this the diff falls back to a coarser (still correct) script rather
/// than keep a background executor busy; see [`FileDiff::approximate`].
const DEADLINE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
}

/// One line of the full-file diff. Line numbers are one-based, matching ADO
/// `threadContext` positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// Index (into [`FileDiff::lines`]) of the `+`/`-` line this one replaced,
    /// for intraline highlighting; [`NO_PAIR`] otherwise.
    pub pair: u32,
}

impl DiffLine {
    pub fn partner(&self) -> Option<usize> {
        (self.pair != NO_PAIR).then_some(self.pair as usize)
    }
}

/// Full-file line diff of two blob texts (FR-1). Every line of both files
/// appears exactly once in [`lines`](Self::lines), in display order; folding
/// unchanged regions is a layout concern ([`crate::rows`]).
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub old: Text,
    pub new: Text,
    pub lines: Vec<DiffLine>,
    pub additions: u32,
    pub deletions: u32,
    /// The deadline was hit: the script is valid but may not be minimal.
    pub approximate: bool,
    /// `old line - 1 → index in lines`, same for `new`.
    old_ix: Vec<u32>,
    new_ix: Vec<u32>,
}

impl FileDiff {
    /// Diffs `old` against `new`. Lines are interned to `u32` ids first so the
    /// algorithm compares integers, not strings. Meant for a background
    /// executor; a 5k-line file takes a few ms (see `benches/diff.rs`).
    pub fn compute(old: Text, new: Text) -> Self {
        let mut ids: HashMap<&str, u32> = HashMap::with_capacity(old.len() + new.len());
        let mut intern = |line| {
            let next = ids.len() as u32;
            *ids.entry(line).or_insert(next)
        };
        let old_ids: Vec<u32> = old.lines().map(&mut intern).collect();
        let new_ids: Vec<u32> = new.lines().map(&mut intern).collect();

        let started = Instant::now();
        let ops = capture_diff_slices_deadline(
            Algorithm::Myers,
            &old_ids,
            &new_ids,
            Some(started + DEADLINE),
        );
        let approximate = started.elapsed() >= DEADLINE;

        let mut lines = Vec::with_capacity(old.len().max(new.len()) + 16);
        let (mut additions, mut deletions) = (0, 0);
        let mut push_change = |lines: &mut Vec<DiffLine>,
                               old_start: usize,
                               old_len: usize,
                               new_start: usize,
                               new_len: usize| {
            let first_del = lines.len();
            let first_add = first_del + old_len;
            let paired = old_len.min(new_len);
            for i in 0..old_len {
                lines.push(DiffLine {
                    kind: LineKind::Del,
                    old: Some((old_start + i + 1) as u32),
                    new: None,
                    pair: if i < paired {
                        (first_add + i) as u32
                    } else {
                        NO_PAIR
                    },
                });
            }
            for i in 0..new_len {
                lines.push(DiffLine {
                    kind: LineKind::Add,
                    old: None,
                    new: Some((new_start + i + 1) as u32),
                    pair: if i < paired {
                        (first_del + i) as u32
                    } else {
                        NO_PAIR
                    },
                });
            }
            deletions += old_len as u32;
            additions += new_len as u32;
        };
        for op in ops {
            match op {
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => lines.extend((0..len).map(|i| DiffLine {
                    kind: LineKind::Context,
                    old: Some((old_index + i + 1) as u32),
                    new: Some((new_index + i + 1) as u32),
                    pair: NO_PAIR,
                })),
                DiffOp::Delete {
                    old_index, old_len, ..
                } => push_change(&mut lines, old_index, old_len, 0, 0),
                DiffOp::Insert {
                    new_index, new_len, ..
                } => push_change(&mut lines, 0, 0, new_index, new_len),
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => push_change(&mut lines, old_index, old_len, new_index, new_len),
            }
        }

        let mut old_ix = vec![0; old.len()];
        let mut new_ix = vec![0; new.len()];
        for (ix, line) in lines.iter().enumerate() {
            if let Some(no) = line.old {
                old_ix[no as usize - 1] = ix as u32;
            }
            if let Some(no) = line.new {
                new_ix[no as usize - 1] = ix as u32;
            }
        }
        Self {
            old,
            new,
            lines,
            additions,
            deletions,
            approximate,
            old_ix,
            new_ix,
        }
    }

    pub fn from_strs(old: &str, new: &str) -> Self {
        Self::compute(Text::new(old), Text::new(new))
    }

    /// Text of display line `ix`. Deleted lines read from the old file,
    /// everything else from the new one.
    pub fn text(&self, ix: usize) -> &str {
        let line = &self.lines[ix];
        match (line.new, line.old) {
            (Some(no), _) => self.new.line(no as usize - 1),
            (None, Some(no)) => self.old.line(no as usize - 1),
            (None, None) => "",
        }
    }

    /// Display index of one-based `line` on `side`, if the file has it.
    pub fn index_of(&self, side: Side, line: u32) -> Option<usize> {
        let table = match side {
            Side::Left => &self.old_ix,
            Side::Right => &self.new_ix,
        };
        let slot = (line as usize).checked_sub(1)?;
        table.get(slot).map(|ix| *ix as usize)
    }

    pub fn is_unchanged(&self) -> bool {
        self.additions == 0 && self.deletions == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(diff: &FileDiff) -> Vec<String> {
        (0..diff.lines.len())
            .map(|ix| {
                let sign = match diff.lines[ix].kind {
                    LineKind::Context => ' ',
                    LineKind::Add => '+',
                    LineKind::Del => '-',
                };
                format!("{sign}{}", diff.text(ix))
            })
            .collect()
    }

    #[test]
    fn identical_texts_are_all_context() {
        let diff = FileDiff::from_strs("a\nb\n", "a\nb\n");
        assert!(diff.is_unchanged());
        assert_eq!(render(&diff), [" a", " b"]);
    }

    #[test]
    fn replace_emits_deletes_then_adds_and_pairs_them() {
        let diff = FileDiff::from_strs("a\nb\nc\nd\n", "a\nB\nx\nd\n");
        assert_eq!(render(&diff), [" a", "-b", "-c", "+B", "+x", " d"]);
        assert_eq!((diff.additions, diff.deletions), (2, 2));
        assert_eq!(diff.lines[1].partner(), Some(3));
        assert_eq!(diff.lines[4].partner(), Some(2));
    }

    #[test]
    fn unequal_blocks_pair_only_the_overlap() {
        let diff = FileDiff::from_strs("a\nb\nz\n", "a\nB\nC\nD\nz\n");
        assert_eq!(render(&diff), [" a", "-b", "+B", "+C", "+D", " z"]);
        assert_eq!(diff.lines[2].partner(), Some(1));
        assert_eq!(diff.lines[3].partner(), None);
    }

    #[test]
    fn line_numbers_are_one_based_and_indexable() {
        let diff = FileDiff::from_strs("keep\ndrop\ntail\n", "new\nkeep\ntail\n");
        assert_eq!(render(&diff), ["+new", " keep", "-drop", " tail"]);
        assert_eq!(diff.lines[1].old, Some(1));
        assert_eq!(diff.lines[1].new, Some(2));
        assert_eq!(diff.index_of(Side::Left, 2), Some(2));
        assert_eq!(diff.index_of(Side::Right, 1), Some(0));
        assert_eq!(diff.index_of(Side::Right, 3), Some(3));
        assert_eq!(diff.index_of(Side::Right, 4), None);
        assert_eq!(diff.index_of(Side::Right, 0), None);
    }

    #[test]
    fn added_and_deleted_files() {
        let added = FileDiff::from_strs("", "x\ny\n");
        assert_eq!(render(&added), ["+x", "+y"]);
        let deleted = FileDiff::from_strs("x\n", "");
        assert_eq!(render(&deleted), ["-x"]);
        assert_eq!(deleted.lines[0].partner(), None);
    }

    #[test]
    fn every_line_of_both_files_appears_once() {
        let old: String = (0..400).map(|i| format!("line {}\n", i * 7 % 50)).collect();
        let new: String = (0..380).map(|i| format!("line {}\n", i * 3 % 60)).collect();
        let diff = FileDiff::from_strs(&old, &new);
        let olds: Vec<u32> = diff.lines.iter().filter_map(|l| l.old).collect();
        let news: Vec<u32> = diff.lines.iter().filter_map(|l| l.new).collect();
        assert_eq!(olds, (1..=400).collect::<Vec<_>>());
        assert_eq!(news, (1..=380).collect::<Vec<_>>());
        for (ix, line) in diff.lines.iter().enumerate() {
            if line.kind == LineKind::Context {
                assert_eq!(diff.old.line(line.old.unwrap() as usize - 1), diff.text(ix));
            }
        }
    }
}
