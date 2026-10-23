//! Cmd+P universal search index (spec 0003 FR-3; matcher per ADR 0001).
//!
//! A query runs in two passes:
//! 1. **Retrieval** with frizbee's SIMD Smith-Waterman over folded haystacks
//!    (`id title subtitle`), one pattern per query token, all required. Tokens
//!    of 4+ chars get `max_typos` = FR-3 budget + 1, because frizbee scores an
//!    adjacent transposition as two edits. Corpora of [`PARALLEL_MIN_DOCS`]+
//!    docs are matched on several threads.
//! 2. **Rescoring** of the best [`RESCORE_BATCH`] candidates by frizbee score:
//!    each token is classified against each word (exact > prefix >
//!    word-boundary > substring > subsequence > typo), typos use
//!    optimal-string-alignment distance so a transposition is ONE edit, and
//!    candidates over the FR-3 budget are dropped. Further batches are rescored
//!    only while fewer than `limit` hits survive, up to [`RESCORE_MAX`].
//!
//! Equal scores keep corpus order, so ranking is stable.

mod normalize;
mod rank;
pub mod synthetic;

use frizbee::{CaseMatching, Config, Matcher, Matching, Pattern, PatternConfig};

/// One searchable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchDoc {
    pub id: u64,
    pub kind: DocKind,
    pub title: String,
    pub subtitle: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocKind {
    PullRequest,
    WorkItem,
    Pipeline,
    Repo,
    Project,
    Person,
    Command,
}

/// One ranked result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Index into [`Index::docs`].
    pub doc: u32,
    pub score: u32,
    /// Matched char indices in the doc title, ascending.
    pub positions: Vec<u32>,
}

/// Corpora at least this large are matched on multiple threads.
pub const PARALLEL_MIN_DOCS: usize = 20_000;
/// Candidates rescored per batch (FR-3: top ≤ 500 candidates).
pub const RESCORE_BATCH: usize = 500;
/// Hard cap on rescored candidates when early batches don't fill `limit`.
pub const RESCORE_MAX: usize = 4 * RESCORE_BATCH;

/// Immutable, `Send + Sync` search index. Build it off the main thread and
/// query it from any executor.
pub struct Index {
    docs: Vec<SearchDoc>,
    /// Folded `id title subtitle` per doc, for frizbee.
    haystacks: Vec<String>,
    /// `(id, doc index)` for id-bearing kinds, sorted, for exact-id lookup.
    ids: Vec<(u64, u32)>,
}

impl Index {
    pub fn new(docs: Vec<SearchDoc>) -> Self {
        assert!(
            u32::try_from(docs.len()).is_ok(),
            "search index is limited to u32::MAX docs"
        );
        let mut haystacks = Vec::with_capacity(docs.len());
        let mut ids = Vec::new();
        let mut raw = String::new();
        for (i, doc) in docs.iter().enumerate() {
            raw.clear();
            if rank::has_id(doc.kind) {
                use std::fmt::Write;
                let _ = write!(raw, "{} ", doc.id);
                ids.push((doc.id, i as u32));
            }
            raw.push_str(&doc.title);
            raw.push(' ');
            raw.push_str(&doc.subtitle);
            let mut folded = String::new();
            normalize::fold_into(&raw, &mut folded);
            haystacks.push(folded);
        }
        ids.sort_unstable();
        Self {
            docs,
            haystacks,
            ids,
        }
    }

    pub fn docs(&self) -> &[SearchDoc] {
        &self.docs
    }

    /// Ranked hits for `q`, best first, at most `limit`. A query without any
    /// alphanumeric token returns the first `limit` docs in corpus order.
    pub fn query(&self, q: &str, limit: usize) -> Vec<Hit> {
        if limit == 0 {
            return Vec::new();
        }
        let tokens = rank::tokenize(q);
        if tokens.is_empty() {
            return (0..self.docs.len().min(limit) as u32)
                .map(|doc| Hit {
                    doc,
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
        }

        let patterns: Vec<Pattern> = tokens
            .iter()
            .map(|t| {
                let matching = if t.numeric {
                    Matching::Substring
                } else {
                    Matching::Fuzzy
                };
                Pattern::new(&t.text, PatternConfig::default())
                    .matching(Some(matching))
                    .max_typos(Some(t.retrieval_typos()))
            })
            .collect();
        // Haystacks and tokens are already folded to lowercase.
        let config = Config::default().casing(CaseMatching::Respect);
        let mut matcher = Matcher::from_patterns(&patterns, &config);
        let candidates = if self.haystacks.len() >= PARALLEL_MIN_DOCS {
            matcher.match_list_parallel(&self.haystacks, 0)
        } else {
            matcher.match_list(&self.haystacks)
        };

        let mut scratch = rank::Scratch::default();
        let mut hits = Vec::new();
        let mut rescore = |doc: u32, hits: &mut Vec<Hit>| {
            if let Some((score, positions)) =
                rank::rescore(&self.docs[doc as usize], &tokens, &mut scratch)
            {
                hits.push(Hit {
                    doc,
                    score,
                    positions,
                });
            }
        };

        // Exact id matches rank first (FR-4) even when frizbee ranks them low.
        let mut pinned = Vec::new();
        for t in tokens.iter().filter(|t| t.numeric) {
            if let Ok(id) = t.text.parse::<u64>() {
                let from = self.ids.partition_point(|&(i, _)| i < id);
                pinned.extend(
                    self.ids[from..]
                        .iter()
                        .take_while(|&&(i, _)| i == id)
                        .map(|&(_, doc)| doc),
                );
            }
        }
        for &doc in &pinned {
            rescore(doc, &mut hits);
        }

        for (batch, chunk) in candidates.chunks(RESCORE_BATCH).enumerate() {
            if batch * RESCORE_BATCH >= RESCORE_MAX || (batch > 0 && hits.len() >= limit) {
                break;
            }
            for m in chunk {
                if !pinned.contains(&m.index) {
                    rescore(m.index, &mut hits);
                }
            }
        }

        hits.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.doc.cmp(&b.doc)));
        hits.truncate(limit);
        hits
    }
}

#[cfg(test)]
mod tests;
