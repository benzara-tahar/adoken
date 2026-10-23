//! The five tours as static data (spec 0020 FR-1), mirroring
//! `docs/design/tours.md` §5, the single source of their copy.
//!
//! Bodies are templates: `[action|keys]` is a key reference, rendered from
//! the live keymap (FR-12); `keys` is the documented binding, preferred
//! when it is still bound. `[a|x]–[b|y]` is a range (one reference for the
//! copy limits). `` `text` `` is literal query text (`type.code_inline`).

use adoken_ui::tour::Side;

/// A tour's stable id (`[tours] seen` entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TourId {
    Review,
    Palette,
    Inbox,
    WorkItem,
    Today,
}

impl TourId {
    pub const ALL: [TourId; 5] = [
        TourId::Today,
        TourId::Inbox,
        TourId::Review,
        TourId::WorkItem,
        TourId::Palette,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TourId::Review => "review",
            TourId::Palette => "palette",
            TourId::Inbox => "inbox",
            TourId::WorkItem => "work-item",
            TourId::Today => "today",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tour| tour.id() == id)
    }

    /// Used in the a11y label and "Tour: <name>".
    pub fn name(self) -> &'static str {
        match self {
            TourId::Review => "Pull request review",
            TourId::Palette => "Command palette",
            TourId::Inbox => "Inbox",
            TourId::WorkItem => "Work item",
            TourId::Today => "Today",
        }
    }

    pub fn tour(self) -> &'static Tour {
        match self {
            TourId::Review => &REVIEW,
            TourId::Palette => &PALETTE,
            TourId::Inbox => &INBOX,
            TourId::WorkItem => &WORK_ITEM,
            TourId::Today => &TODAY,
        }
    }
}

/// One coach mark.
#[derive(Debug)]
pub struct Step {
    pub anchor: &'static str,
    pub side: Side,
    pub title: &'static str,
    pub body: &'static str,
    /// FR-13: the body when the token can't write (the view's read-only
    /// hint); `None` when the step names no write.
    pub read_only: Option<&'static str>,
}

#[derive(Debug)]
pub struct Tour {
    pub id: TourId,
    /// The gallery story AC-7 renders this tour's anchors in.
    pub story: &'static str,
    /// Key contexts the tour's view puts on the focus path (outermost
    /// first); key references resolve against bindings that apply there.
    pub contexts: &'static [&'static str],
    pub steps: &'static [Step],
}

const REVIEW_READ_ONLY: &str = "Voting, commenting and completing need Code (Read & write).";
const WORK_ITEM_READ_ONLY: &str = "Editing needs Work items (Read & write). [work_items::OpenWorkItemInAdo|o] opens it in Azure DevOps.";

pub static REVIEW: Tour = Tour {
    id: TourId::Review,
    story: "review-open",
    contexts: &["Workspace", "PullRequest", "PrReview"],
    steps: &[
        Step {
            anchor: crate::review::anchors::HEADER,
            side: Side::Below,
            title: "This pull request",
            body: "Status, reviewers and branches at a glance. [pull_requests::CopyPrId|shift-y] copies the \
                   id, [pull_requests::CopyPrLink|y] the link. Click the title or a branch to copy it.",
            read_only: None,
        },
        Step {
            anchor: crate::review::anchors::FILE_TREE,
            side: Side::Right,
            title: "Files",
            body: "[review::NextFile|j] and [review::PreviousFile|k] move between files. \
                   [review::MarkViewed|v] marks one viewed and moves on. \
                   [review::FocusFileFilter|/] filters by path, \
                   [review::ToggleThreadsOnly|shift-t] keeps files with threads.",
            read_only: None,
        },
        Step {
            anchor: crate::review::anchors::DIFF,
            side: Side::Inside,
            title: "The diff",
            body: "[review::CommentOnLine|c] comments on the cursor line. \
                   [review::ToggleDiffLayout|secondary-shift-d] switches between unified and side \
                   by side. [review::ToggleThreadsView|t] shows only the threads.",
            read_only: None,
        },
        Step {
            anchor: crate::review::anchors::THREAD,
            side: Side::Left,
            title: "Threads",
            body: "[review::NextThread|n] and [review::PreviousThread|p] jump between threads. \
                   [review::ReplyToThread|r] replies, [review::ToggleResolved|shift-r] resolves.",
            read_only: None,
        },
        Step {
            anchor: crate::review::anchors::ACTIONS,
            side: Side::Below,
            title: "Vote and finish",
            body: "[review::OpenVoteMenu|shift-v] votes, [review::CommentOnPullRequest|shift-c] \
                   comments on the whole pull request, [review::OpenCompleteSheet|shift-m] \
                   completes. [review::OpenStatusMenu|shift-s] publishes, marks as draft or \
                   abandons.",
            read_only: Some(REVIEW_READ_ONLY),
        },
    ],
};

pub static PALETTE: Tour = Tour {
    id: TourId::Palette,
    story: "palette-my-work",
    contexts: &["Workspace", "Palette"],
    steps: &[
        Step {
            anchor: crate::palette::anchors::INPUT,
            side: Side::Below,
            title: "Find anything",
            body: "Type part of a title, an id like `!4821` or `#1234`, or paste an Azure DevOps \
                   link.",
            read_only: None,
        },
        Step {
            anchor: crate::palette::anchors::INPUT,
            side: Side::Below,
            title: "Narrow it down",
            body: "Start with `!` for pull requests, `#` work items, `@` people, `>` commands. Add \
                   filters like `author:@me` or `is:draft`.",
            read_only: None,
        },
        Step {
            anchor: crate::palette::anchors::RESULTS,
            side: Side::Right,
            title: "Act on a result",
            body: "[palette::Confirm|enter] opens it, [palette::ConfirmBrowser|secondary-enter] \
                   opens it in Azure DevOps, [pin_actions::OpenRowMenu|secondary-k] shows its \
                   actions: vote, copy, assign.",
            read_only: None,
        },
        Step {
            anchor: crate::palette::anchors::FOOTER,
            side: Side::Below,
            title: "Every action is here",
            body: "Type `>` to run anything by name, these tours included. Start with `?` to see \
                   every scope and filter.",
            read_only: None,
        },
    ],
};

pub static INBOX: Tour = Tour {
    id: TourId::Inbox,
    story: "inbox",
    contexts: &["Workspace", "Inbox"],
    steps: &[
        Step {
            anchor: crate::inbox::anchors::LIST,
            side: Side::Inside,
            title: "Only what's addressed to you",
            body: "Your pull requests, work items, pipelines and mentions, one row per item. \
                   [inbox::Open|enter] opens it. [shell::GoBack|secondary-[] brings you back to \
                   this row.",
            read_only: None,
        },
        Step {
            anchor: crate::inbox::anchors::FILTERS,
            side: Side::Below,
            title: "Filter with one key",
            body: "[inbox::ToggleSourcePullRequests|1]–[inbox::ToggleSourceNudges|5] show one \
                   source, [inbox::TogglePriority0|!]–[inbox::TogglePriority3|$] one \
                   priority. [inbox::ClearFilters|secondary-backspace] shows everything again.",
            read_only: None,
        },
        Step {
            anchor: crate::inbox::anchors::FOOTER,
            side: Side::Above,
            title: "Triage without opening",
            body: "[inbox::MarkDone|e] marks done, [inbox::Snooze|s] snoozes, \
                   [inbox::MuteSubject|m] mutes this item. [inbox::Undo|z] undoes.",
            read_only: None,
        },
        Step {
            anchor: crate::inbox::anchors::SETTINGS,
            side: Side::Below,
            title: "Make it quieter",
            body: "Priority rules, quiet hours and mutes live here. \
                   [inbox::ToggleRules|secondary-shift-,]",
            read_only: None,
        },
    ],
};

pub static WORK_ITEM: Tour = Tour {
    id: TourId::WorkItem,
    story: "work-item-detail",
    contexts: &["Workspace", "WorkItems", "WorkItemDetail"],
    steps: &[
        Step {
            anchor: crate::workitems::detail::anchors::STATE,
            side: Side::Below,
            title: "State in one key",
            body: "[work_items::ChangeWorkItemState|s] changes the state, \
                   [work_items::EditWorkItemTitle|t] edits the title. Changes go to Azure DevOps \
                   as you make them.",
            read_only: Some(WORK_ITEM_READ_ONLY),
        },
        Step {
            anchor: crate::workitems::detail::anchors::FIELDS,
            side: Side::Below,
            title: "Fields",
            body: "[work_items::AssignWorkItem|a] assigns, [work_items::AssignWorkItemToMe|i] \
                   assigns it to you, [work_items::ChangeWorkItemArea|shift-a] sets the area, \
                   [work_items::ChangeWorkItemIteration|shift-i] the iteration.",
            read_only: Some(WORK_ITEM_READ_ONLY),
        },
        Step {
            anchor: crate::workitems::detail::anchors::DESCRIPTION,
            side: Side::Left,
            title: "Description and links",
            body: "[work_items::EditWorkItemDescription|d] edits the description. \
                   [work_items::AddWorkItemLink|l] links another work item.",
            read_only: Some(WORK_ITEM_READ_ONLY),
        },
        Step {
            anchor: crate::workitems::detail::anchors::DISCUSSION,
            side: Side::Above,
            title: "Discussion",
            body: "[work_items::CommentOnWorkItem|c] writes a comment. Type `@` to mention someone.",
            read_only: Some(WORK_ITEM_READ_ONLY),
        },
        Step {
            anchor: crate::workitems::detail::anchors::FRESHNESS,
            side: Side::Below,
            title: "Is this current?",
            body: "When adoken last confirmed this item. \
                   [work_items::SyncWorkItem|secondary-shift-r] syncs just this one. \
                   [shell::GoBack|secondary-[] goes back to where you came from.",
            read_only: None,
        },
    ],
};

pub static TODAY: Tour = Tour {
    id: TourId::Today,
    story: "today-busy-day",
    contexts: &["Workspace", "Today"],
    steps: &[
        Step {
            anchor: crate::today::anchors::SUMMARY,
            side: Side::Below,
            title: "Your day at a glance",
            body: "What moved today and what needs you. The picture follows the day: shipped, \
                   waiting, blocked or clear.",
            read_only: None,
        },
        Step {
            anchor: crate::today::anchors::PRS,
            side: Side::Right,
            title: "Your pull requests",
            body: "Most recently updated first; the bar shows how close each is to merged. \
                   [today::SelectDown|j] and [today::SelectUp|k] move, [today::Activate|enter] \
                   opens.",
            read_only: None,
        },
        Step {
            anchor: crate::today::anchors::UP_NEXT,
            side: Side::Left,
            title: "Up next",
            body: "The one thing worth doing now. [today::DismissFocus|e] hides it until \
                   tomorrow. [today::SelectPrev|h] and [today::SelectNext|l] switch columns.",
            read_only: None,
        },
        Step {
            anchor: crate::today::anchors::WAITING,
            side: Side::Left,
            title: "Waiting on you",
            body: "Reviews and replies others need from you. [today::SnoozeNudge|s] snoozes, \
                   [today::MarkNudgeDone|d] marks done, [today::NotForThisItem|x] stops it for \
                   this item.",
            read_only: None,
        },
        Step {
            anchor: crate::today::anchors::LOOSE_ENDS,
            side: Side::Left,
            title: "Loose ends",
            body: "Things you started that went quiet, with how long they've been open.",
            read_only: None,
        },
    ],
};

/// One parsed piece of a body template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part<'a> {
    Text(&'a str),
    /// A key reference: the action's name and its documented keys.
    Key {
        action: &'a str,
        keys: &'a str,
    },
    /// `[a]–[b]`: two references read as one ("1–5").
    Range {
        from: (&'a str, &'a str),
        to: (&'a str, &'a str),
    },
    Code(&'a str),
}

/// Parse a body template. Malformed markup is kept as text (the copy
/// lint in `tour_anchors.rs` catches it).
pub fn parse(body: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let next = rest.find(['[', '`']).unwrap_or(rest.len());
        if next > 0 {
            parts.push(Part::Text(&rest[..next]));
            rest = &rest[next..];
            continue;
        }
        if let Some(code) = rest.strip_prefix('`')
            && let Some(end) = code.find('`')
        {
            parts.push(Part::Code(&code[..end]));
            rest = &code[end + 1..];
            continue;
        }
        match key_ref(rest) {
            Some((first, after)) => match after.strip_prefix('–').and_then(key_ref) {
                Some((second, after)) => {
                    parts.push(Part::Range {
                        from: first,
                        to: second,
                    });
                    rest = after;
                }
                None => {
                    parts.push(Part::Key {
                        action: first.0,
                        keys: first.1,
                    });
                    rest = after;
                }
            },
            None => {
                let ch = rest.chars().next().map_or(1, char::len_utf8);
                parts.push(Part::Text(&rest[..ch]));
                rest = &rest[ch..];
            }
        }
    }
    parts
}

/// `[action|keys]` at the start of `text`: `((action, keys), rest)`. The
/// keys may contain `[` (`secondary-[`); the reference ends at the first
/// `]` after the `|`.
fn key_ref(text: &str) -> Option<((&str, &str), &str)> {
    let inner = text.strip_prefix('[')?;
    let bar = inner.find('|')?;
    let action = &inner[..bar];
    if action.is_empty() || !action.contains("::") || action.contains(['[', ']', ' ']) {
        return None;
    }
    let after_bar = &inner[bar + 1..];
    // The first `]` that isn't the key itself (`secondary-]`).
    let close = after_bar
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == ']')
        .map(|(i, _)| i)?;
    Some(((action, &after_bar[..close]), &after_bar[close + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_ranges_code_and_bracket_keys() {
        assert_eq!(
            parse("[a::B|j] and `!` [c::D|1]–[c::E|5]. [s::Back|secondary-[] back"),
            vec![
                Part::Key {
                    action: "a::B",
                    keys: "j"
                },
                Part::Text(" and "),
                Part::Code("!"),
                Part::Text(" "),
                Part::Range {
                    from: ("c::D", "1"),
                    to: ("c::E", "5")
                },
                Part::Text(". "),
                Part::Key {
                    action: "s::Back",
                    keys: "secondary-["
                },
                Part::Text(" back"),
            ]
        );
    }

    #[test]
    fn stray_brackets_stay_text() {
        assert_eq!(
            parse("a [b] c"),
            vec![Part::Text("a "), Part::Text("["), Part::Text("b] c")]
        );
    }

    #[test]
    fn ids_round_trip_and_unknown_ids_are_none() {
        for tour in TourId::ALL {
            assert_eq!(TourId::parse(tour.id()), Some(tour));
            assert_eq!(tour.tour().id, tour);
        }
        assert_eq!(TourId::parse("future-tour"), None);
    }
}
