//! The review's file filter (spec 0019 group A): path terms typed above the
//! tree and "Only files with threads". Pure and allocation-light: paths
//! are folded once per file list ([`FoldedPaths`]); each keystroke matches
//! the in-memory list on the UI thread (FR-2: never a store query) and lists
//! the tree rows to paint ([`list`]).
//!
//! Matching (FR-2, Q-1): the query splits on whitespace; a file is listed
//! when its display path contains every plain term and none of the
//! `-`-prefixed ones (a lone `-` is ignored), case-insensitively under
//! Unicode simple case folding. Folders are listed when they hold a listed
//! file (FR-3). Matched ranges of plain terms are highlighted in file and
//! folder names ([`highlights`]).

use std::ops::Range;

use adoken_ui::file_tree::TreeRow;

/// Lower-cases `text` char by char (Unicode simple case folding, as
/// `char::to_lowercase`). ASCII stays byte-for-byte aligned.
pub fn fold(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.chars().flat_map(char::to_lowercase).collect()
}

/// A parsed filter query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileQuery {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl FileQuery {
    pub fn parse(text: &str) -> Self {
        let mut query = Self::default();
        for term in text.split_whitespace() {
            match term.strip_prefix('-') {
                Some("") => {}
                Some(excluded) => query.exclude.push(fold(excluded)),
                None => query.include.push(fold(term)),
            }
        }
        query
    }

    /// Lists every file.
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// `folded` is a path already passed through [`fold`].
    pub fn matches(&self, folded: &str) -> bool {
        self.include.iter().all(|t| folded.contains(t.as_str()))
            && !self.exclude.iter().any(|t| folded.contains(t.as_str()))
    }

    pub fn include(&self) -> &[String] {
        &self.include
    }
}

/// Every file's display path, folded once per file list.
#[derive(Debug, Clone, Default)]
pub struct FoldedPaths(Vec<String>);

impl FoldedPaths {
    pub fn new<S: AsRef<str>>(paths: impl IntoIterator<Item = S>) -> Self {
        Self(paths.into_iter().map(|p| fold(p.as_ref())).collect())
    }

    pub fn get(&self, file: usize) -> &str {
        self.0.get(file).map_or("", String::as_str)
    }
}

/// What the tree shows for a query: tree rows in order, and the listed
/// files in tree order (the `j`/`k` order while filtered).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Indices into the tree's rows.
    pub rows: Vec<usize>,
    /// File indices, tree order.
    pub files: Vec<usize>,
    /// Whether anything narrows the list (text or a predicate).
    pub filtered: bool,
}

/// The rows listed for `query` and `keep` (FR-5's "only files with
/// threads"; `None` keeps every file). `tree` is [`adoken_ui::file_tree::build`]'s
/// pre-order rows: a folder at depth `d` is followed by its contents
/// (folders at `d + 1`, files at `d + 1`).
pub fn list(
    tree: &[TreeRow],
    paths: &FoldedPaths,
    query: &FileQuery,
    keep: Option<&dyn Fn(usize) -> bool>,
) -> Listing {
    let filtered = !query.is_empty() || keep.is_some();
    if !filtered {
        return Listing {
            rows: (0..tree.len()).collect(),
            files: tree
                .iter()
                .filter_map(|row| match row {
                    TreeRow::File { index, .. } => Some(*index),
                    TreeRow::Dir { .. } => None,
                })
                .collect(),
            filtered,
        };
    }
    let mut rows = Vec::new();
    let mut files = Vec::new();
    // Open folders: (row, depth, already listed).
    let mut open: Vec<(usize, usize, bool)> = Vec::new();
    for (ix, row) in tree.iter().enumerate() {
        match row {
            TreeRow::Dir { depth, .. } => {
                while open.last().is_some_and(|(_, d, _)| *d >= *depth) {
                    open.pop();
                }
                open.push((ix, *depth, false));
            }
            TreeRow::File { index, depth, .. } => {
                // A file at depth `d` lives in the open folder at `d - 1`.
                while open.last().is_some_and(|(_, d, _)| *d >= *depth) {
                    open.pop();
                }
                let listed =
                    query.matches(paths.get(*index)) && keep.is_none_or(|keep| keep(*index));
                if !listed {
                    continue;
                }
                for (dir, _, shown) in open.iter_mut().filter(|(_, _, shown)| !*shown) {
                    rows.push(*dir);
                    *shown = true;
                }
                rows.push(ix);
                files.push(*index);
            }
        }
    }
    Listing {
        rows,
        files,
        filtered,
    }
}

/// Byte ranges of `name` (a file or folder row's own text) that match the
/// query's plain terms, sorted and merged. Folding is per char, so ranges
/// land on `name`'s own char boundaries.
pub fn highlights(name: &str, query: &FileQuery) -> Vec<Range<usize>> {
    if query.include.is_empty() {
        return Vec::new();
    }
    // Folded text plus, per folded byte, the original char's byte range.
    let (folded, origin): (String, Vec<Range<usize>>) = if name.is_ascii() {
        (
            name.to_ascii_lowercase(),
            (0..name.len()).map(|i| i..i + 1).collect(),
        )
    } else {
        let mut folded = String::with_capacity(name.len());
        let mut origin = Vec::with_capacity(name.len());
        for (at, ch) in name.char_indices() {
            let span = at..at + ch.len_utf8();
            for lower in ch.to_lowercase() {
                folded.push(lower);
                origin.extend(std::iter::repeat_n(span.clone(), lower.len_utf8()));
            }
        }
        (folded, origin)
    };
    let mut out: Vec<Range<usize>> = Vec::new();
    for term in &query.include {
        if term.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(found) = folded[from..].find(term.as_str()) {
            let start = from + found;
            let end = start + term.len();
            out.push(origin[start].start..origin[end - 1].end);
            from = end;
        }
    }
    out.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(out.len());
    for range in out {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_ui::file_tree::build;

    /// Fixture S (spec 0019 §6).
    const S: [&str; 5] = [
        "docs/AUTH.md",
        "src/auth/token.rs",
        "src/auth/token_test.rs",
        "src/pay/ledger.rs",
        "src/pay/retry.rs",
    ];

    fn names(tree: &[TreeRow], listing: &Listing) -> Vec<String> {
        listing
            .rows
            .iter()
            .map(|&ix| match &tree[ix] {
                TreeRow::Dir { name, .. } => format!("{name}/"),
                TreeRow::File { name, .. } => name.clone(),
            })
            .collect()
    }

    fn run(text: &str) -> (Vec<TreeRow>, Listing) {
        let tree = build(&S);
        let listing = list(&tree, &FoldedPaths::new(S), &FileQuery::parse(text), None);
        (tree, listing)
    }

    #[test]
    fn ac_a1_terms_list_matching_files_and_their_folders() {
        let (tree, auth) = run("auth");
        assert_eq!(
            names(&tree, &auth),
            [
                "docs/",
                "AUTH.md",
                "src/",
                "auth/",
                "token.rs",
                "token_test.rs"
            ]
        );
        assert_eq!(auth.files, [0, 1, 2]);
        let (tree, no_test) = run("auth -test");
        assert_eq!(
            names(&tree, &no_test),
            ["docs/", "AUTH.md", "src/", "auth/", "token.rs"]
        );
        assert_eq!(run("AUTH").1.files, auth.files);
        for everything in ["", "   ", "-"] {
            let (_, listing) = run(everything);
            assert_eq!(listing.files.len(), 5, "{everything:?}");
            assert!(!listing.filtered);
        }
        let (_, none) = run("zzz");
        assert!(none.rows.is_empty() && none.files.is_empty() && none.filtered);
    }

    /// `highlights` as `(start, end)` pairs, for terse expectations.
    fn spans(name: &str, query: &FileQuery) -> Vec<(usize, usize)> {
        highlights(name, query)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn ac_a1_highlights_cover_matches_in_names() {
        let query = FileQuery::parse("auth");
        assert_eq!(spans("auth", &query), [(0, 4)]);
        assert_eq!(spans("AUTH.md", &query), [(0, 4)]);
        // The folder path matched, not the name: no highlight.
        assert!(highlights("token.rs", &query).is_empty());
        // Excluded terms never highlight; overlapping terms merge.
        assert!(highlights("token_test.rs", &FileQuery::parse("-test")).is_empty());
        assert_eq!(
            spans("token_test.rs", &FileQuery::parse("ken_ to")),
            [(0, 6)]
        );
    }

    #[test]
    fn unicode_folding_keeps_original_char_boundaries() {
        let query = FileQuery::parse("über");
        assert!(query.matches(&fold("src/ÜBER/x.rs")));
        assert_eq!(spans("ÜBER.md", &query), [(0, "ÜBER".len())]);
        // A char whose lowercase is longer ('İ' → "i̇") still maps back.
        let dotted = FileQuery::parse("i̇x");
        assert_eq!(spans("aİx", &dotted), [(1, 1 + 'İ'.len_utf8() + 1)]);
    }

    #[test]
    fn ac_a4_threads_only_combines_with_text() {
        let tree = build(&S);
        let paths = FoldedPaths::new(S);
        // Active threads: AUTH.md, token.rs, ledger.rs.
        let active = |file: usize| matches!(file, 0 | 1 | 3);
        let only = list(&tree, &paths, &FileQuery::default(), Some(&active));
        assert_eq!(only.files, [0, 1, 3]);
        assert_eq!(
            names(&tree, &only),
            [
                "docs/",
                "AUTH.md",
                "src/",
                "auth/",
                "token.rs",
                "pay/",
                "ledger.rs"
            ]
        );
        let both = list(&tree, &paths, &FileQuery::parse("auth"), Some(&active));
        assert_eq!(both.files, [0, 1]);
    }

    #[test]
    fn compressed_folder_chains_and_root_files() {
        let paths = [
            "web/app/components/Retry.tsx",
            "README.md",
            "src/a/b.rs",
            "src/c.rs",
        ];
        let tree = build(&paths);
        let listing = list(
            &tree,
            &FoldedPaths::new(paths),
            &FileQuery::parse("-b.rs"),
            None,
        );
        // `src/` holds both `a/b.rs` (excluded) and `c.rs`; `a/` stays out.
        // The compressed chain lists as one row; a root file needs no folder.
        assert_eq!(
            names(&tree, &listing),
            [
                "src/",
                "c.rs",
                "web/app/components/",
                "Retry.tsx",
                "README.md"
            ]
        );
        // FR-2 matches the full path: `src/a/b.rs` holds "c" in `src`.
        let c = list(
            &tree,
            &FoldedPaths::new(paths),
            &FileQuery::parse("c"),
            None,
        );
        assert_eq!(c.files.len(), 3);
    }
}
