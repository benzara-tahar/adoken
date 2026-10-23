//! Hit kinds, their move names (flavor text only, ken.md §3.1) and combo
//! ranks (§3.2). Every name here is original (ken.md §8).

use serde::{Deserialize, Serialize};

/// How captions are worded (`[ken] naming`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Naming {
    English,
    #[default]
    EnglishKanji,
    RomajiKanji,
}

impl Naming {
    pub const ALL: [Naming; 3] = [Naming::English, Naming::EnglishKanji, Naming::RomajiKanji];

    pub fn label(self) -> &'static str {
        match self {
            Naming::English => "English",
            Naming::EnglishKanji => "English + kanji",
            Naming::RomajiKanji => "Romaji + kanji",
        }
    }
}

/// One name in the three spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    pub english: &'static str,
    pub kanji: &'static str,
    pub romaji: &'static str,
}

impl Name {
    /// Primary caption and the optional small kanji beside it.
    pub fn caption(self, naming: Naming) -> (&'static str, Option<&'static str>) {
        match naming {
            Naming::English => (self.english, None),
            Naming::EnglishKanji => (self.english, Some(self.kanji)),
            Naming::RomajiKanji => (self.romaji, Some(self.kanji)),
        }
    }

    /// Caption as one string: "Gale 疾風", "shippū 疾風", "Gale".
    pub fn text(self, naming: Naming) -> String {
        match self.caption(naming) {
            (primary, Some(kanji)) => format!("{primary} {kanji}"),
            (primary, None) => primary.to_owned(),
        }
    }
}

/// A qualifying review action (ken.md §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    /// Vote Approve (10).
    Approve,
    /// Vote Approve with suggestions (5).
    ApproveWithSuggestions,
    /// Vote Wait for author (−5).
    WaitForAuthor,
    /// Vote Reject (−10).
    Reject,
    /// A review comment or reply.
    Comment,
    /// A thread resolved (fixed / won't fix / closed).
    Resolve,
    /// You completed a PR.
    Complete,
    /// A changed file of the PR's latest iteration marked Viewed in adoken.
    Viewed,
}

impl HitKind {
    pub const ALL: [HitKind; 8] = [
        HitKind::Approve,
        HitKind::ApproveWithSuggestions,
        HitKind::WaitForAuthor,
        HitKind::Reject,
        HitKind::Comment,
        HitKind::Resolve,
        HitKind::Complete,
        HitKind::Viewed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HitKind::Approve => "approve",
            HitKind::ApproveWithSuggestions => "approve_with_suggestions",
            HitKind::WaitForAuthor => "wait_for_author",
            HitKind::Reject => "reject",
            HitKind::Comment => "comment",
            HitKind::Resolve => "resolve",
            HitKind::Complete => "complete",
            HitKind::Viewed => "viewed",
        }
    }

    pub fn parse(s: &str) -> Option<HitKind> {
        HitKind::ALL.into_iter().find(|kind| kind.as_str() == s)
    }

    pub fn is_vote(self) -> bool {
        matches!(
            self,
            HitKind::Approve
                | HitKind::ApproveWithSuggestions
                | HitKind::WaitForAuthor
                | HitKind::Reject
        )
    }

    /// The move name: HUD caption and toast flavor only, never a button label.
    pub fn move_name(self) -> Name {
        match self {
            HitKind::Approve => Name {
                english: "Hanko Slam",
                kanji: "判子",
                romaji: "hanko",
            },
            HitKind::ApproveWithSuggestions => Name {
                english: "Guiding Palm",
                kanji: "導",
                romaji: "michibiki",
            },
            HitKind::WaitForAuthor => Name {
                english: "Ready Stance",
                kanji: "構え",
                romaji: "kamae",
            },
            HitKind::Reject => Name {
                english: "Iron Wall",
                kanji: "鉄壁",
                romaji: "teppeki",
            },
            HitKind::Comment => Name {
                english: "Quick Jab",
                kanji: "突き",
                romaji: "tsuki",
            },
            HitKind::Resolve => Name {
                english: "Clean Cut",
                kanji: "断",
                romaji: "dan",
            },
            HitKind::Complete => Name {
                english: "Merge Wave",
                kanji: "波",
                romaji: "nami",
            },
            HitKind::Viewed => Name {
                english: "Keen Eye",
                kanji: "目利き",
                romaji: "mekiki",
            },
        }
    }
}

/// Combo rank caption (ken.md §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rank {
    Flurry,
    Gale,
    Surge,
    Storm,
}

impl Rank {
    pub const ALL: [Rank; 4] = [Rank::Flurry, Rank::Gale, Rank::Surge, Rank::Storm];

    /// ×2–×4 Flurry, ×5–×7 Gale, ×8–×11 Surge, ×12+ Storm; below ×2 there
    /// is no combo.
    pub fn for_hits(hits: u32) -> Option<Rank> {
        match hits {
            0 | 1 => None,
            2..=4 => Some(Rank::Flurry),
            5..=7 => Some(Rank::Gale),
            8..=11 => Some(Rank::Surge),
            _ => Some(Rank::Storm),
        }
    }

    /// Fewest hits that reach this rank.
    pub fn threshold(self) -> u32 {
        match self {
            Rank::Flurry => 2,
            Rank::Gale => 5,
            Rank::Surge => 8,
            Rank::Storm => 12,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Rank::Flurry => "flurry",
            Rank::Gale => "gale",
            Rank::Surge => "surge",
            Rank::Storm => "storm",
        }
    }

    pub fn parse(s: &str) -> Option<Rank> {
        Rank::ALL.into_iter().find(|rank| rank.as_str() == s)
    }

    pub fn name(self) -> Name {
        match self {
            Rank::Flurry => Name {
                english: "Flurry",
                kanji: "連打",
                romaji: "renda",
            },
            Rank::Gale => Name {
                english: "Gale",
                kanji: "疾風",
                romaji: "shippū",
            },
            Rank::Surge => Name {
                english: "Surge",
                kanji: "怒涛",
                romaji: "dotō",
            },
            Rank::Storm => Name {
                english: "Storm",
                kanji: "嵐",
                romaji: "arashi",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_thresholds_match_the_table() {
        let expected = [
            (0, None),
            (1, None),
            (2, Some(Rank::Flurry)),
            (4, Some(Rank::Flurry)),
            (5, Some(Rank::Gale)),
            (7, Some(Rank::Gale)),
            (8, Some(Rank::Surge)),
            (11, Some(Rank::Surge)),
            (12, Some(Rank::Storm)),
            (400, Some(Rank::Storm)),
        ];
        for (hits, rank) in expected {
            assert_eq!(Rank::for_hits(hits), rank, "×{hits}");
        }
        for rank in Rank::ALL {
            assert_eq!(Rank::for_hits(rank.threshold()), Some(rank));
            assert!(Rank::for_hits(rank.threshold() - 1) < Some(rank));
        }
    }

    #[test]
    fn captions_follow_naming() {
        let gale = Rank::Gale.name();
        assert_eq!(gale.text(Naming::English), "Gale");
        assert_eq!(gale.text(Naming::EnglishKanji), "Gale 疾風");
        assert_eq!(gale.text(Naming::RomajiKanji), "shippū 疾風");
    }

    #[test]
    fn kinds_and_ranks_round_trip() {
        for kind in HitKind::ALL {
            assert_eq!(HitKind::parse(kind.as_str()), Some(kind));
        }
        for rank in Rank::ALL {
            assert_eq!(Rank::parse(rank.as_str()), Some(rank));
        }
    }

    /// ken.md §8: no fighting-game names anywhere in our flavor text.
    #[test]
    fn names_avoid_the_banned_list() {
        let banned = crate::BANNED_NAMES;
        let names = HitKind::ALL
            .iter()
            .map(|k| k.move_name())
            .chain(Rank::ALL.iter().map(|r| r.name()));
        for name in names {
            for text in [name.english, name.romaji] {
                let lower = text.to_lowercase();
                for word in banned {
                    assert!(!lower.contains(word), "{text} contains {word}");
                }
            }
        }
    }
}
