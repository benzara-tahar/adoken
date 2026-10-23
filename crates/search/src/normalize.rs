//! Case and diacritic folding (spec 0003 FR-3).
//!
//! Every source char folds to exactly one char, so a char index into folded
//! text is also a char index into the source. `Hit::positions` relies on it.

use unicode_normalization::char::decompose_canonical;

/// Lowercases `c` and strips diacritics from Latin, Greek and Cyrillic letters.
#[inline]
pub(crate) fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        // No canonical decomposition, but users type them without the stroke.
        'ø' => 'o',
        'ł' => 'l',
        'đ' | 'ð' => 'd',
        'ħ' => 'h',
        'ı' => 'i',
        'ŧ' | 'þ' => 't',
        'æ' => 'a',
        'œ' => 'o',
        'ß' => 's',
        // Scripts where the first decomposed char is the base letter. Hangul
        // and friends decompose into jamo, so they must not be folded.
        '\u{00C0}'..='\u{058F}' | '\u{1E00}'..='\u{1FFF}' => {
            let mut base = None;
            decompose_canonical(lower, |d| {
                base.get_or_insert(d);
            });
            base.unwrap_or(lower)
        }
        _ => lower,
    }
}

/// Folds every char of `s` into `out` (cleared first).
pub(crate) fn fold_into(s: &str, out: &mut String) {
    out.clear();
    out.reserve(s.len());
    out.extend(s.chars().map(fold));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_diacritics_one_to_one() {
        let src = "Zoë Łukasz José SØREN Ünïcödé";
        let mut out = String::new();
        fold_into(src, &mut out);
        assert_eq!(out, "zoe lukasz jose soren unicode");
        assert_eq!(out.chars().count(), src.chars().count());
    }

    #[test]
    fn leaves_hangul_and_cjk_intact() {
        assert_eq!(fold('한'), '한');
        assert_eq!(fold('検'), '検');
    }
}
