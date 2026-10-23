//! Inbox filter bar (flow step 2): source chips `1`–`5`, priority chips
//! `Shift+1..4`, state tabs. Persisted with the local notify state.

use serde::{Deserialize, Serialize};

use crate::classify::{Priority, Source};
use crate::engine::Notification;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StateFilter {
    Unread,
    /// Everything not done, snoozed, or muted.
    #[default]
    All,
    Snoozed,
    Done,
}

impl StateFilter {
    pub const ALL: [Self; 4] = [Self::Unread, Self::All, Self::Snoozed, Self::Done];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unread => "Unread",
            Self::All => "All",
            Self::Snoozed => "Snoozed",
            Self::Done => "Done",
        }
    }

    pub fn matches(self, n: &Notification) -> bool {
        match self {
            Self::Unread => n.in_inbox() && !n.read,
            Self::All => n.in_inbox(),
            Self::Snoozed => !n.muted && n.done_at.is_none() && n.snooze.is_some(),
            Self::Done => !n.muted && n.done_at.is_some(),
        }
    }
}

/// Empty source/priority sets mean "any".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    /// Bit per [`Source::index`].
    pub sources: u8,
    /// Bit per [`Priority::index`].
    pub priorities: u8,
    pub state: StateFilter,
}

impl Filter {
    pub fn has_source(&self, source: Source) -> bool {
        self.sources & (1 << source.index()) != 0
    }

    pub fn has_priority(&self, priority: Priority) -> bool {
        self.priorities & (1 << priority.index()) != 0
    }

    pub fn toggle_source(&mut self, source: Source) {
        self.sources ^= 1 << source.index();
    }

    pub fn toggle_priority(&mut self, priority: Priority) {
        self.priorities ^= 1 << priority.index();
    }

    /// Source and priority chips cleared (the state tab is a view, not a
    /// filter the empty state offers to clear).
    pub fn is_narrowed(&self) -> bool {
        self.sources != 0 || self.priorities != 0
    }

    pub fn clear(&mut self) {
        self.sources = 0;
        self.priorities = 0;
    }

    pub fn matches(&self, n: &Notification) -> bool {
        (self.sources == 0 || self.has_source(n.source))
            && (self.priorities == 0 || self.has_priority(n.priority))
            && self.state.matches(n)
    }

    /// "Pipelines, Mentions · P0, P1" for the filtered-to-nothing state.
    pub fn describe(&self) -> String {
        let sources: Vec<&str> = Source::ALL
            .into_iter()
            .filter(|s| self.has_source(*s))
            .map(Source::label)
            .collect();
        let priorities: Vec<&str> = Priority::ALL
            .into_iter()
            .filter(|p| self.has_priority(*p))
            .map(Priority::label)
            .collect();
        [sources.join(", "), priorities.join(", ")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_and_describe() {
        let mut filter = Filter::default();
        assert!(!filter.is_narrowed());
        filter.toggle_source(Source::Pipelines);
        filter.toggle_source(Source::Mentions);
        filter.toggle_priority(Priority::P1);
        assert!(filter.has_source(Source::Pipelines));
        assert!(!filter.has_source(Source::PullRequests));
        assert_eq!(filter.describe(), "Pipelines, Mentions · P1");
        filter.toggle_source(Source::Pipelines);
        assert_eq!(filter.describe(), "Mentions · P1");
        filter.clear();
        assert!(!filter.is_narrowed());
        assert_eq!(filter.state, StateFilter::All);
    }
}
