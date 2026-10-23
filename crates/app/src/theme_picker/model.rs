//! Picker logic without gpui: the entry list, search, and grid keyboard
//! movement. Unit tested.

use adoken_theme::{Appearance, ThemeRegistry};

/// Grid columns (cards are `flex_1` inside fixed rows).
pub const COLUMNS: usize = 3;

/// One card: a variant plus what the grid shows about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub variant_id: String,
    pub name: String,
    pub family_id: String,
    pub family_name: String,
    pub author: Option<String>,
    pub appearance: Appearance,
    /// Loaded from the user's themes folder.
    pub user: bool,
    /// Lowercased name, family, author, id and appearance, for search.
    haystack: String,
}

/// Every variant in registry order (built-ins first, then user files).
pub fn entries(registry: &ThemeRegistry) -> Vec<Entry> {
    registry
        .families()
        .iter()
        .flat_map(|family| {
            family.variants.iter().map(move |variant| {
                let haystack = [
                    variant.name.as_str(),
                    family.name.as_str(),
                    family.author.as_deref().unwrap_or_default(),
                    variant.id.as_str(),
                    variant.appearance.as_str(),
                ]
                .join(" ")
                .to_lowercase();
                Entry {
                    variant_id: variant.id.clone(),
                    name: variant.name.clone(),
                    family_id: family.id.clone(),
                    family_name: family.name.clone(),
                    author: family.author.clone(),
                    appearance: variant.appearance,
                    user: !ThemeRegistry::is_builtin_family(&family.id),
                    haystack,
                }
            })
        })
        .collect()
}

/// Indices of `entries` containing every whitespace-separated query word
/// (case-insensitive). "dark" or "light" narrows by appearance.
pub fn visible(entries: &[Entry], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| words.iter().all(|word| entry.haystack.contains(word)))
        .map(|(index, _)| index)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

/// The highlighted position after `step` in a `len`-card grid of
/// [`COLUMNS`]. From nothing, any move lands on the first card. Left/right
/// run through rows; up/down keep the column and stop at the edges.
pub fn step(current: Option<usize>, len: usize, step: Move) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let Some(current) = current.map(|c| c.min(len - 1)) else {
        return Some(0);
    };
    Some(match step {
        Move::Left => current.saturating_sub(1),
        Move::Right => (current + 1).min(len - 1),
        Move::Up => current.checked_sub(COLUMNS).unwrap_or(current),
        Move::Down if current + COLUMNS < len => current + COLUMNS,
        // Down into a short last row: land on its last card.
        Move::Down if current / COLUMNS < (len - 1) / COLUMNS => len - 1,
        Move::Down => current,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_every_query_word() {
        let entries = entries(ThemeRegistry::builtin());
        let all = visible(&entries, "");
        assert_eq!(all.len(), entries.len());

        let hits: Vec<&str> = visible(&entries, "CATPPUCCIN  latte")
            .into_iter()
            .map(|i| entries[i].variant_id.as_str())
            .collect();
        assert_eq!(hits, ["catppuccin-latte"]);
        // Family and author are searchable too.
        let by_author = visible(&entries, "primer light");
        assert_eq!(entries[by_author[0]].variant_id, "github-light");
        // The appearance word narrows without a separate filter.
        let dark = visible(&entries, "dark");
        assert!(
            dark.iter()
                .all(|&i| entries[i].appearance == Appearance::Dark)
        );
        assert!(!dark.is_empty() && dark.len() < all.len());
        assert!(visible(&entries, "latte dark").is_empty());
    }

    #[test]
    fn builtins_are_not_marked_as_user_themes() {
        assert!(entries(ThemeRegistry::builtin()).iter().all(|e| !e.user));
    }

    #[test]
    fn grid_movement_keeps_columns_and_clamps() {
        // 7 cards: rows [0 1 2] [3 4 5] [6].
        assert_eq!(step(None, 7, Move::Down), Some(0));
        assert_eq!(step(None, 0, Move::Down), None);
        assert_eq!(step(Some(1), 7, Move::Down), Some(4));
        assert_eq!(step(Some(4), 7, Move::Down), Some(6));
        assert_eq!(step(Some(6), 7, Move::Down), Some(6));
        assert_eq!(step(Some(1), 7, Move::Up), Some(1));
        assert_eq!(step(Some(5), 7, Move::Up), Some(2));
        assert_eq!(step(Some(2), 7, Move::Right), Some(3));
        assert_eq!(step(Some(6), 7, Move::Right), Some(6));
        assert_eq!(step(Some(0), 7, Move::Left), Some(0));
        // A stale index after the list shrank is clamped first.
        assert_eq!(step(Some(20), 7, Move::Left), Some(5));
    }
}
