//! The Threads view's pure parts (spec 0019 group B): status scopes and
//! their counts (FR-9), the fixed list order (FR-8), and each card's code
//! excerpt (FR-10). No gpui; [`super::pr`] renders from these.

use std::collections::HashSet;
use std::ops::Range;

use adoken_diff::{FileDiff, LineKind};
use adoken_model::{PrThread, ThreadStatus};

use super::data::{FileEntry, LoadedPr, normalize_path};
use super::filter::{FileQuery, fold};

/// FR-9 status scope. Active counts pending and unknown threads too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ThreadScope {
    #[default]
    Active,
    Resolved,
    All,
}

impl ThreadScope {
    pub const ALL: [Self; 3] = [Self::Active, Self::Resolved, Self::All];

    pub fn includes(self, status: ThreadStatus) -> bool {
        match self {
            Self::Active => !status.is_resolved(),
            Self::Resolved => status.is_resolved(),
            Self::All => true,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Resolved => "Resolved",
            Self::All => "All",
        }
    }

    /// The empty-state word ("No active threads").
    pub fn adjective(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Resolved => "resolved",
            Self::All => "",
        }
    }
}

/// Threads per scope, for the scope control's labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScopeCounts {
    pub active: usize,
    pub resolved: usize,
}

impl ScopeCounts {
    pub fn of(threads: &[PrThread]) -> Self {
        let resolved = threads.iter().filter(|t| t.status.is_resolved()).count();
        Self {
            active: threads.len() - resolved,
            resolved,
        }
    }

    pub fn get(self, scope: ThreadScope) -> usize {
        match scope {
            ThreadScope::Active => self.active,
            ThreadScope::Resolved => self.resolved,
            ThreadScope::All => self.active + self.resolved,
        }
    }
}

/// One group of the list (FR-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// Threads without a file anchor.
    Pr,
    /// A file of the pull request (index into [`LoadedPr::files`]).
    File(usize),
    /// Anchored to paths no longer in the file list.
    Gone,
}

/// One row of the virtualized Threads list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Header {
        group: Group,
        count: usize,
    },
    /// Index into [`LoadedPr::threads`].
    Card {
        thread: usize,
        group: Group,
    },
}

impl Item {
    pub fn thread(self) -> Option<usize> {
        match self {
            Item::Card { thread, .. } => Some(thread),
            Item::Header { .. } => None,
        }
    }

    pub fn group(self) -> Group {
        match self {
            Item::Header { group, .. } | Item::Card { group, .. } => group,
        }
    }

    /// The same list row: a header whose count changed still is.
    pub fn same_row(&self, other: &Item) -> bool {
        match (self, other) {
            (Item::Header { group: a, .. }, Item::Header { group: b, .. }) => a == b,
            _ => self == other,
        }
    }
}

/// The anchor's line range on the side that numbers it (right side first),
/// `None` for a PR-level thread. File-level threads count as line 1.
pub fn anchor_range(thread: &PrThread) -> Option<(adoken_diff::Side, u32, u32)> {
    let anchor = thread.anchor.as_ref()?;
    Some(match (anchor.right, anchor.left) {
        (Some(r), _) => (adoken_diff::Side::Right, r.start, r.end.max(r.start)),
        (None, Some(l)) => (adoken_diff::Side::Left, l.start, l.end.max(l.start)),
        (None, None) => (adoken_diff::Side::Right, 1, 1),
    })
}

/// "Comments on older code" (0004 FR-5): placed as older once the diff is
/// computed; before that, a thread from another iteration.
pub fn is_older(loaded: &LoadedPr, entry: &FileEntry, thread: usize) -> bool {
    if entry.diff.is_some() {
        entry.anchored.older.contains(&thread)
    } else {
        loaded.threads[thread]
            .iteration
            .is_some_and(|it| it != loaded.iteration)
    }
}

/// FR-8: the list for `scope`, files passing `query` in tree order. Cards
/// in `sticky` stay whatever their status (a thread resolved here keeps its
/// place until the scope changes, FR-9).
pub fn items(
    loaded: &LoadedPr,
    scope: ThreadScope,
    query: &FileQuery,
    sticky: &HashSet<i64>,
) -> Vec<Item> {
    let threads = &*loaded.threads;
    let shown = |ix: usize| {
        let t = &threads[ix];
        scope.includes(t.status) || sticky.contains(&t.id)
    };
    let mut per_file: Vec<Vec<usize>> = vec![Vec::new(); loaded.files.len()];
    let mut pr_level = Vec::new();
    let mut gone = Vec::new();
    for (ix, thread) in threads.iter().enumerate() {
        if !shown(ix) {
            continue;
        }
        let Some(anchor) = &thread.anchor else {
            pr_level.push(ix);
            continue;
        };
        let path = normalize_path(&anchor.file_path);
        match loaded.files.iter().position(|f| f.path == path) {
            Some(file) => per_file[file].push(ix),
            None => gone.push(ix),
        }
    }
    let line = |ix: usize| anchor_range(&threads[ix]).map_or(0, |(_, start, _)| start);
    let mut out = Vec::new();
    let push = |group: Group, cards: &[usize], out: &mut Vec<Item>| {
        if cards.is_empty() {
            return;
        }
        out.push(Item::Header {
            group,
            count: cards.len(),
        });
        out.extend(cards.iter().map(|&thread| Item::Card { thread, group }));
    };
    // Newest activity first (Q-6).
    pr_level.sort_by_key(|ix| std::cmp::Reverse(threads[*ix].updated));
    push(Group::Pr, &pr_level, &mut out);
    for &file in &loaded.order {
        let entry = &loaded.files[file];
        if !query.matches(&fold(&entry.path)) {
            continue;
        }
        let cards = &mut per_file[file];
        cards.sort_by_key(|&ix| (is_older(loaded, entry, ix), line(ix), ix));
        push(Group::File(file), cards, &mut out);
    }
    gone.retain(|&ix| {
        let path = thread_path(&threads[ix]).unwrap_or_default();
        query.matches(&fold(path))
    });
    gone.sort_by(|a, b| {
        thread_path(&threads[*a])
            .cmp(&thread_path(&threads[*b]))
            .then(line(*a).cmp(&line(*b)))
    });
    push(Group::Gone, &gone, &mut out);
    out
}

pub fn thread_path(thread: &PrThread) -> Option<&str> {
    thread.anchor.as_ref().map(|a| normalize_path(&a.file_path))
}

/// Anchored lines shown at most; three lines of context come before them.
const MAX_ANCHOR_ROWS: usize = 9;
const CONTEXT_ROWS: usize = 3;

/// FR-10: an excerpt's row count, known before the diff is computed.
pub fn excerpt_rows(thread: &PrThread) -> usize {
    let lines = anchor_range(thread).map_or(1, |(_, start, end)| (end - start + 1) as usize);
    lines.min(MAX_ANCHOR_ROWS) + CONTEXT_ROWS
}

/// A card's code: unified display lines of the computed diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excerpt {
    /// Display lines (indices into [`FileDiff::lines`]), ending at the
    /// anchor's last line.
    pub lines: Range<usize>,
    /// Blank rows before them at the top of a file, so every excerpt has
    /// [`excerpt_rows`] rows.
    pub pad: usize,
    /// Display lines marked as anchored.
    pub marked: Range<usize>,
}

/// The excerpt of a thread placed inline after display line `end` (0004
/// placement: the anchor's last line), `rows` long (see [`excerpt_rows`]).
/// `side` and `len` are the anchor's side and line count: rows numbered on
/// that side within the last `len` numbers up to `end`'s are marked.
pub fn excerpt(
    diff: &FileDiff,
    end: usize,
    side: adoken_diff::Side,
    len: u32,
    rows: usize,
) -> Excerpt {
    let end = end.min(diff.lines.len().saturating_sub(1));
    let start = (end + 1).saturating_sub(rows);
    let number = |ix: usize| {
        let line = diff.lines[ix];
        match side {
            adoken_diff::Side::Right => line.new,
            // A left anchor numbers deleted and context lines.
            adoken_diff::Side::Left => line.old.filter(|_| line.kind != LineKind::Add),
        }
    };
    let last = number(end).unwrap_or(0);
    let first = (last + 1).saturating_sub(len.max(1));
    let marked_from = (start..=end)
        .find(|&ix| number(ix).is_some_and(|n| n >= first && n <= last))
        .unwrap_or(end);
    Excerpt {
        lines: start..end + 1,
        pad: rows - (end + 1 - start),
        marked: marked_from..end + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_diff::{Side, Text};
    use adoken_model::{Comment, LineRange, ThreadAnchor};

    /// `(path, right lines, left lines)` of a test thread's anchor.
    type Anchor<'a> = (&'a str, Option<(u32, u32)>, Option<(u32, u32)>);

    fn thread(id: i64, anchor: Option<Anchor>) -> PrThread {
        PrThread {
            id,
            pr_id: 1,
            status: ThreadStatus::Active,
            anchor: anchor.map(|(path, right, left)| ThreadAnchor {
                file_path: format!("/{path}"),
                right: right.map(|(start, end)| LineRange { start, end }),
                left: left.map(|(start, end)| LineRange { start, end }),
                iteration_blob: None,
            }),
            iteration: Some(1),
            comments: vec![Comment {
                id: 1,
                parent_id: None,
                author: "a".into(),
                body: "x".into(),
                created: 0,
                updated: 0,
                is_system: false,
                is_deleted: false,
            }],
            updated: id,
        }
    }

    fn numbered(lines: std::ops::RangeInclusive<u32>) -> String {
        lines.map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn fr9_scopes_count_pending_and_unknown_as_active() {
        let mut ts: Vec<PrThread> = (0..4).map(|i| thread(i, None)).collect();
        ts[1].status = ThreadStatus::Pending;
        ts[2].status = ThreadStatus::Unknown;
        ts[3].status = ThreadStatus::WontFix;
        let counts = ScopeCounts::of(&ts);
        assert_eq!(
            ThreadScope::ALL.map(|s| counts.get(s)),
            [3, 1, 4],
            "Active · Resolved · All"
        );
        assert!(!ThreadScope::Active.includes(ThreadStatus::Closed));
        assert!(ThreadScope::Resolved.includes(ThreadStatus::ByDesign));
    }

    #[test]
    fn ac_b3_excerpt_ends_at_the_anchor_and_marks_it() {
        // New file 1..=60; line 41 changed: rows ... 40, -41, +41, 42 ...
        let old = numbered(1..=60);
        let new = old.replace("line 41\n", "changed 41\n");
        let diff = FileDiff::compute(Text::new(old), Text::new(new));
        let end = diff.index_of(Side::Right, 42).unwrap();
        let t = thread(1, Some(("a.rs", Some((40, 42)), None)));
        let rows = excerpt_rows(&t);
        assert_eq!(rows, 6);
        let e = excerpt(&diff, end, Side::Right, 3, rows);
        assert_eq!(e.lines.len(), 6);
        assert_eq!(e.pad, 0);
        assert_eq!(diff.lines[e.lines.end - 1].new, Some(42));
        // 40, +41, 42 are marked; the deleted 41 between them too.
        let marked: Vec<_> = e.marked.clone().map(|ix| diff.lines[ix].new).collect();
        assert_eq!(marked, [Some(40), None, Some(41), Some(42)]);

        let long = thread(2, Some(("a.rs", Some((1, 20)), None)));
        assert_eq!(excerpt_rows(&long), 12);
        // Near the top of the file the excerpt pads to its fixed height.
        let top = excerpt(
            &diff,
            diff.index_of(Side::Right, 2).unwrap(),
            Side::Right,
            1,
            4,
        );
        assert_eq!((top.lines.len(), top.pad), (2, 2));
    }

    #[test]
    fn ac_b3_left_only_anchor_ends_at_the_old_line() {
        let old = numbered(1..=12);
        let new = old.replace("line 7\n", "");
        let diff = FileDiff::compute(Text::new(old), Text::new(new));
        let end = diff.index_of(Side::Left, 7).unwrap();
        let t = thread(1, Some(("a.rs", None, Some((7, 7)))));
        let e = excerpt(&diff, end, Side::Left, 1, excerpt_rows(&t));
        assert_eq!(diff.lines[e.lines.end - 1].old, Some(7));
        assert_eq!(diff.lines[e.lines.end - 1].kind, LineKind::Del);
        assert_eq!(e.marked.len(), 1);
    }
}
