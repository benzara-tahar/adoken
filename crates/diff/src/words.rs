//! Intraline word diff for a paired `-`/`+` line (`diff.*.word` tokens).
//! Computed per visible row: lines are short and this is a few µs.

use std::ops::Range;

use similar::{Algorithm, DiffOp, capture_diff_slices};

/// Longer lines skip intraline highlighting (minified code, data blobs).
const MAX_LINE: usize = 1_000;
/// Below this share of unchanged bytes the line was rewritten, not edited:
/// highlighting nearly all of it adds noise, so show the line wash only.
const MIN_SHARED: f32 = 0.3;

/// Changed byte ranges in `old` and `new`, or two empty lists when the pair
/// is too long or too different to be worth highlighting.
pub fn changed_ranges(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if old == new || old.len() > MAX_LINE || new.len() > MAX_LINE {
        return (Vec::new(), Vec::new());
    }
    let old_tokens = tokens(old);
    let new_tokens = tokens(new);
    let old_words: Vec<&str> = old_tokens.iter().map(|r| &old[r.clone()]).collect();
    let new_words: Vec<&str> = new_tokens.iter().map(|r| &new[r.clone()]).collect();
    let ops = capture_diff_slices(Algorithm::Myers, &old_words, &new_words);

    let mut shared = 0usize;
    let mut old_out: Vec<Range<usize>> = Vec::new();
    let mut new_out: Vec<Range<usize>> = Vec::new();
    let span = |tokens: &[Range<usize>], start: usize, len: usize| {
        tokens[start].start..tokens[start + len - 1].end
    };
    let push = |out: &mut Vec<Range<usize>>, range: Range<usize>| match out.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => out.push(range),
    };
    for op in ops {
        match op {
            DiffOp::Equal { old_index, len, .. } => {
                shared += span(&old_tokens, old_index, len).len();
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => push(&mut old_out, span(&old_tokens, old_index, old_len)),
            DiffOp::Insert {
                new_index, new_len, ..
            } => push(&mut new_out, span(&new_tokens, new_index, new_len)),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                push(&mut old_out, span(&old_tokens, old_index, old_len));
                push(&mut new_out, span(&new_tokens, new_index, new_len));
            }
        }
    }
    let longest = old.len().max(new.len()).max(1);
    if (shared as f32 / longest as f32) < MIN_SHARED {
        return (Vec::new(), Vec::new());
    }
    (old_out, new_out)
}

/// Words (`[A-Za-z0-9_]+` and other alphanumerics), whitespace runs, and
/// single other characters.
fn tokens(line: &str) -> Vec<Range<usize>> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut prev: Option<Class> = None;
    for (ix, c) in line.char_indices() {
        let current = class(c);
        match (prev, out.last_mut()) {
            (Some(p), Some(last)) if p == current && current != Class::Other => {
                last.end = ix + c.len_utf8();
            }
            _ => out.push(ix..ix + c.len_utf8()),
        }
        prev = Some(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_only_the_changed_word() {
        let (old, new) = changed_ranges(
            "let retries = config.max_retries;",
            "let retries = config.retry_budget;",
        );
        assert_eq!(old, vec![21..32]);
        assert_eq!(new, vec![21..33]);
    }

    #[test]
    fn pure_insertion_marks_new_side_only() {
        let (old, new) = changed_ranges("foo(a, b)", "foo(a, b, c)");
        assert!(old.is_empty());
        assert_eq!(new, vec![8..11]);
    }

    #[test]
    fn rewritten_lines_get_no_word_highlight() {
        let (old, new) = changed_ranges("fn alpha() {}", "let x = [1, 2, 3];");
        assert!(old.is_empty() && new.is_empty());
    }

    #[test]
    fn multibyte_text_keeps_char_boundaries() {
        let (old, new) = changed_ranges("naïve café", "naïve cafés");
        assert_eq!(&"naïve café"[old[0].clone()], "café");
        assert_eq!(&"naïve cafés"[new[0].clone()], "cafés");
    }
}
