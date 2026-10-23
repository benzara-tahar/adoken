//! Token-level match classification, OSA typo distance, and document rescoring
//! (spec 0003 FR-3). frizbee only retrieves candidates; everything that decides
//! order lives here.

use crate::normalize::fold;
use crate::{DocKind, SearchDoc};

/// Per-token score tiers. Single-token ranking is exact/prefix > word-boundary
/// > fuzzy > typo; the ranges below never overlap.
pub(crate) const EXACT: u32 = 1000;
const PREFIX: u32 = 800; // + up to 99 for covering more of the word
const BOUNDARY: u32 = 650; // + up to 99
const SUBSTRING: u32 = 500; // + up to 49
const SUBSEQUENCE: u32 = 300; // + up to 60 anchored + 100 contiguity
const TYPO_FULL: u32 = 250; // whole word within the typo budget
const TYPO_PREFIX: u32 = 200; // word prefix within the typo budget (typing in progress)
const TYPO_STEP: u32 = 80; // per extra edit
/// An exact id match always ranks first (FR-4).
pub(crate) const ID_EXACT_BONUS: u32 = 1_000_000;
/// Consecutive query tokens matching consecutive title words, in order.
const ADJACENT_BONUS: u32 = 20;
/// Title words near the start rank slightly higher.
const EARLY_WORD_BONUS: u32 = 2;
const EARLY_WORDS: u32 = 6;

/// Longest token the typo pass handles; longer tokens match without typos.
const MAX_TYPO_TOKEN: usize = 64;

/// One normalized query token.
pub(crate) struct Token {
    pub(crate) text: String,
    pub(crate) chars: Vec<char>,
    pub(crate) numeric: bool,
    /// Allowed OSA edits (FR-3): 0 below 4 chars, 1 for 4-7, 2 for 8+, 0 for numbers.
    pub(crate) budget: u8,
}

impl Token {
    fn new(chars: Vec<char>) -> Self {
        let numeric = chars.iter().all(char::is_ascii_digit);
        let budget = match chars.len() {
            _ if numeric => 0,
            0..=3 => 0,
            4..=7 => 1,
            _ => 2,
        };
        Self {
            text: chars.iter().collect(),
            chars,
            numeric,
            budget,
        }
    }

    /// `max_typos` for frizbee retrieval: FR-3 budget + 1 for tokens of 4+
    /// chars, because frizbee scores an adjacent transposition as 2 edits.
    pub(crate) fn retrieval_typos(&self) -> u16 {
        if self.numeric || self.chars.len() < 4 {
            0
        } else {
            u16::from(self.budget) + 1
        }
    }
}

/// Splits a query into folded alphanumeric tokens, the same way documents are
/// split into words, so `feature/login` and `#48213` behave predictably.
pub(crate) fn tokenize(query: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = Vec::new();
    for c in query.chars().map(fold) {
        if c.is_alphanumeric() {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(Token::new(std::mem::take(&mut current)));
        }
    }
    if !current.is_empty() {
        tokens.push(Token::new(current));
    }
    tokens
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Id,
    Title,
    Subtitle,
}

/// Best match of one token against one word. `mask` marks matched chars of
/// the word (bit i = char i; chars past 64 are not highlighted).
#[derive(Clone, Copy)]
struct WordMatch {
    score: u32,
    mask: u64,
}

fn range_mask(start: usize, len: usize) -> u64 {
    let end = (start + len).min(64);
    if start >= end {
        return 0;
    }
    let width = end - start;
    let ones = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    ones << start
}

/// Classifies how `token` matches `word`. `sub[i]` is true when char `i`
/// starts a sub-word (camelCase hump or letter/digit switch) inside the word.
fn match_word(token: &Token, word: &[char], sub: &[bool]) -> Option<WordMatch> {
    let t = token.chars.as_slice();
    let (m, n) = (t.len(), word.len());
    if word == t {
        return Some(WordMatch {
            score: EXACT,
            mask: range_mask(0, n),
        });
    }
    if n > m && word.starts_with(t) {
        return Some(WordMatch {
            score: PREFIX + (99 * m / n) as u32,
            mask: range_mask(0, m),
        });
    }
    if token.numeric {
        // FR-3: numbers match exactly or by prefix only.
        return None;
    }
    if n > m {
        for s in 1..=n - m {
            if sub[s] && word[s..].starts_with(t) {
                return Some(WordMatch {
                    score: BOUNDARY + (99 * m / (n - s)) as u32,
                    mask: range_mask(s, m),
                });
            }
        }
        if let Some(s) = word.windows(m).position(|w| w == t) {
            return Some(WordMatch {
                score: SUBSTRING + (49 * m / n) as u32,
                mask: range_mask(s, m),
            });
        }
        if m >= 2
            && let Some(found) = subsequence(t, word, sub)
        {
            return Some(found);
        }
    }
    if token.budget > 0
        && m <= MAX_TYPO_TOKEN
        && !word.iter().all(char::is_ascii_digit)
        && let Some((d, len, full)) = osa(t, word, token.budget)
    {
        let base = if full { TYPO_FULL } else { TYPO_PREFIX };
        return Some(WordMatch {
            score: base - TYPO_STEP * u32::from(d - 1),
            mask: range_mask(0, len),
        });
    }
    None
}

/// Best in-word subsequence alignment: tries each start char and keeps the one
/// with the longest contiguous runs; anchored starts (word or sub-word) win.
fn subsequence(t: &[char], word: &[char], sub: &[bool]) -> Option<WordMatch> {
    let m = t.len();
    let mut best: Option<WordMatch> = None;
    for start in 0..word.len() {
        if word[start] != t[0] {
            continue;
        }
        let mut mask = 1u64 << start.min(63);
        let mut last = start;
        let mut adjacent = 0usize;
        let mut ti = 1;
        for (wi, &c) in word.iter().enumerate().skip(start + 1) {
            if ti == m {
                break;
            }
            if c == t[ti] {
                if wi == last + 1 {
                    adjacent += 1;
                }
                if wi < 64 {
                    mask |= 1 << wi;
                }
                last = wi;
                ti += 1;
            }
        }
        if ti < m {
            // Later starts only see a shorter suffix.
            break;
        }
        let anchored = if start == 0 || sub[start] { 60 } else { 0 };
        let score = SUBSEQUENCE + anchored + (100 * adjacent / (m - 1)) as u32;
        if best.is_none_or(|b| score > b.score) {
            best = Some(WordMatch { score, mask });
        }
    }
    best
}

/// Optimal-string-alignment distance (insert, delete, substitute, adjacent
/// transposition = 1 edit each) between `t` and `word` or a prefix of `word`.
/// Returns `(distance, matched word chars, whole word)` when within `budget`,
/// preferring a whole-word match over a prefix at equal distance.
pub(crate) fn osa(t: &[char], word: &[char], budget: u8) -> Option<(u8, usize, bool)> {
    const W: usize = MAX_TYPO_TOKEN + 3;
    let m = t.len();
    let budget = usize::from(budget);
    if m == 0 || m > MAX_TYPO_TOKEN || word.len() + budget < m {
        return None;
    }
    // Prefixes longer than m + budget are over budget by length alone.
    let n = word.len().min(m + budget);
    let (mut prev2, mut prev, mut cur) = ([0u8; W], [0u8; W], [0u8; W]);
    for (j, cell) in prev.iter_mut().enumerate().take(n + 1) {
        *cell = j as u8;
    }
    let mut prev_min = 0u8;
    for i in 1..=m {
        cur[0] = i as u8;
        let mut row_min = cur[0];
        for j in 1..=n {
            let cost = u8::from(t[i - 1] != word[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && t[i - 1] == word[j - 2] && t[i - 2] == word[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
            row_min = row_min.min(v);
        }
        if usize::from(row_min) > budget && usize::from(prev_min) > budget {
            return None;
        }
        prev_min = row_min;
        prev2 = prev;
        prev = cur;
    }
    // `prev` now holds row m. Minimal distance wins; the whole word wins ties.
    let mut best: Option<(u8, usize)> = None;
    for (j, &d) in prev.iter().enumerate().take(n + 1).skip(1) {
        let closer = |b: (u8, usize)| d < b.0 || (d == b.0 && j.abs_diff(m) < b.1.abs_diff(m));
        if usize::from(d) <= budget && best.is_none_or(closer) {
            best = Some((d, j));
        }
    }
    let (d, j) = best?;
    if word.len() == n && prev[n] <= d {
        return Some((prev[n], n, true));
    }
    Some((d, j, false))
}

/// Reusable per-query buffers for splitting fields into words.
#[derive(Default)]
pub(crate) struct Scratch {
    chars: Vec<char>,
    sub: Vec<bool>,
    /// `(start, end, field)` char ranges into `chars`.
    words: Vec<(u32, u32, Field)>,
    id: String,
    /// `(title char offset of the word, matched-char mask)` per highlighted token.
    highlights: Vec<(u32, u64)>,
}

impl Scratch {
    /// Appends the words of `text` (folded) to the scratch buffers.
    fn push_field(&mut self, text: &str, field: Field) {
        let mut prev: Option<char> = None;
        let mut start: Option<usize> = None;
        for c in text.chars() {
            let folded = fold(c);
            let at = self.chars.len();
            self.chars.push(folded);
            if folded.is_alphanumeric() {
                let hump = prev.is_some_and(|p| {
                    (p.is_lowercase() && c.is_uppercase())
                        || (p.is_alphabetic() && c.is_ascii_digit())
                        || (p.is_ascii_digit() && c.is_alphabetic())
                });
                self.sub.push(hump);
                start.get_or_insert(at);
            } else {
                self.sub.push(false);
                if let Some(s) = start.take() {
                    self.words.push((s as u32, at as u32, field));
                }
            }
            prev = Some(c);
        }
        if let Some(s) = start {
            self.words.push((s as u32, self.chars.len() as u32, field));
        }
        // Field separator so words never straddle fields.
        self.chars.push(' ');
        self.sub.push(false);
    }
}

pub(crate) fn has_id(kind: DocKind) -> bool {
    matches!(
        kind,
        DocKind::PullRequest | DocKind::WorkItem | DocKind::Pipeline
    )
}

/// Scores `doc` against every token. `None` when any token has no match
/// within its FR-3 budget (this drops frizbee's widened-retrieval extras).
pub(crate) fn rescore(
    doc: &SearchDoc,
    tokens: &[Token],
    s: &mut Scratch,
) -> Option<(u32, Vec<u32>)> {
    s.chars.clear();
    s.sub.clear();
    s.words.clear();
    s.highlights.clear();
    if has_id(doc.kind) {
        use std::fmt::Write;
        let mut id = std::mem::take(&mut s.id);
        id.clear();
        let _ = write!(id, "{}", doc.id);
        s.push_field(&id, Field::Id);
        s.id = id;
    }
    let title_offset = s.chars.len() as u32;
    let first_title_word = s.words.len();
    s.push_field(&doc.title, Field::Title);
    s.push_field(&doc.subtitle, Field::Subtitle);

    let mut total = 0u32;
    let mut prev_title_word: Option<usize> = None;
    for token in tokens {
        let mut best: Option<(u32, usize)> = None; // (score, word index)
        let mut best_title: Option<(u32, u32, u64)> = None; // (tier score, title offset, mask)
        for (wi, &(start, end, field)) in s.words.iter().enumerate() {
            let (a, b) = (start as usize, end as usize);
            let Some(found) = match_word(token, &s.chars[a..b], &s.sub[a..b]) else {
                continue;
            };
            let score = match field {
                Field::Id if found.score == EXACT => ID_EXACT_BONUS + EXACT,
                Field::Id => found.score,
                Field::Title => {
                    let ordinal = (wi - first_title_word) as u32;
                    found.score + EARLY_WORDS.saturating_sub(ordinal) * EARLY_WORD_BONUS
                }
                Field::Subtitle => found.score / 2,
            };
            if field == Field::Title && best_title.is_none_or(|t| found.score > t.0) {
                best_title = Some((found.score, start - title_offset, found.mask));
            }
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, wi));
            }
        }
        let (score, wi) = best?;
        total += score;
        let in_title = s.words[wi].2 == Field::Title;
        if in_title && prev_title_word.is_some_and(|p| p + 1 == wi) {
            total += ADJACENT_BONUS;
        }
        prev_title_word = in_title.then_some(wi);
        if let Some((_, start, mask)) = best_title {
            s.highlights.push((start, mask));
        }
    }

    let mut positions = Vec::new();
    for &(start, mut mask) in &s.highlights {
        while mask != 0 {
            positions.push(start + mask.trailing_zeros());
            mask &= mask - 1;
        }
    }
    positions.sort_unstable();
    positions.dedup();
    Some((total, positions))
}
