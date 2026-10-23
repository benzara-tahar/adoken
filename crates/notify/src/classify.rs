//! Source and default-priority classification (spec 0005 FR-3).

use std::collections::BTreeMap;
use std::fmt;

use adoken_model::{RunResult, SyncEventKind, Vote};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// P0 is the most urgent. No default rule produces P0 (FR-3): it's for user
/// rules. `Ord` sorts most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl Priority {
    pub const ALL: [Self; 4] = [Self::P0, Self::P1, Self::P2, Self::P3];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        ["P0", "P1", "P2", "P3"][self.index()]
    }

    /// Accessible / tooltip wording (tokens.md `priority.*` rows).
    pub fn description(self) -> &'static str {
        ["Urgent", "High", "Normal", "Low"][self.index()]
    }
}

/// Inbox filter sources (flow step 2, keys `1`–`5`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Source {
    PullRequests,
    WorkItems,
    Pipelines,
    Mentions,
    Nudges,
}

impl Source {
    pub const ALL: [Self; 5] = [
        Self::PullRequests,
        Self::WorkItems,
        Self::Pipelines,
        Self::Mentions,
        Self::Nudges,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PullRequests => "Pull requests",
            Self::WorkItems => "Work items",
            Self::Pipelines => "Pipelines",
            Self::Mentions => "Mentions",
            Self::Nudges => "Nudges",
        }
    }

    /// Singular noun for summary banners ("4 pipeline updates").
    pub fn noun(self) -> &'static str {
        match self {
            Self::PullRequests => "pull request",
            Self::WorkItems => "work item",
            Self::Pipelines => "pipeline",
            Self::Mentions => "mention",
            Self::Nudges => "nudge",
        }
    }
}

/// What happened: a sync event kind (0002 FR-11) or a nudge (0006).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Sync(SyncEventKind),
    Nudge,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync(kind) => kind.as_str(),
            Self::Nudge => "Nudge",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text.eq_ignore_ascii_case("Nudge") {
            Some(Self::Nudge)
        } else {
            SyncEventKind::parse(text).map(Self::Sync)
        }
    }

    /// Token expiry is an account chore, filed with nudges: the five filter
    /// chips are the only sources.
    pub fn source(self) -> Source {
        use SyncEventKind::*;
        match self {
            Self::Nudge | Self::Sync(TokenRejected | TokenReplaced | TokenNotReplaced) => {
                Source::Nudges
            }
            Self::Sync(MentionedInPr | MentionedInWorkItem) => Source::Mentions,
            Self::Sync(WorkItemAssigned | WorkItemChanged) => Source::WorkItems,
            Self::Sync(BuildCompleted) => Source::Pipelines,
            Self::Sync(
                ReviewRequested | PrCommentAdded | PrThreadResolved | PrVoteChanged | PrPushed
                | PrStatusChanged | PrMergeConflict,
            ) => Source::PullRequests,
        }
    }

    /// "3 new comments", "4 pipeline updates".
    pub fn count_label(self, count: u32) -> String {
        use SyncEventKind::*;
        let (one, many) = match self {
            Self::Sync(ReviewRequested) => ("review request", "review requests"),
            Self::Sync(PrCommentAdded) => ("new comment", "new comments"),
            Self::Sync(PrThreadResolved) => ("resolved thread", "resolved threads"),
            Self::Sync(MentionedInPr | MentionedInWorkItem) => ("mention", "mentions"),
            Self::Sync(PrVoteChanged) => ("vote", "votes"),
            Self::Sync(PrPushed) => ("push", "pushes"),
            Self::Sync(PrStatusChanged) => ("status change", "status changes"),
            Self::Sync(PrMergeConflict) => ("merge conflict", "merge conflicts"),
            Self::Sync(WorkItemAssigned) => ("assignment", "assignments"),
            Self::Sync(WorkItemChanged) => ("update", "updates"),
            Self::Sync(BuildCompleted) => ("pipeline update", "pipeline updates"),
            Self::Sync(TokenRejected | TokenReplaced | TokenNotReplaced) => {
                ("token notice", "token notices")
            }
            Self::Nudge => ("nudge", "nudges"),
        };
        format!("{count} {}", if count == 1 { one } else { many })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Kind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("unknown kind `{text}`")))
    }
}

/// Facts about an event's subject that classification, rules, and mutes
/// match on. Built from store reads by the caller (the engine does no IO).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventContext {
    /// Row title: "!4821 Retry idempotency keys".
    pub title: String,
    pub project: Option<String>,
    pub repo_id: Option<String>,
    pub repo_name: Option<String>,
    pub pipeline_id: Option<i64>,
    pub pipeline_name: Option<String>,
    /// Short branch name (`main`).
    pub branch: Option<String>,
    pub work_item_type: Option<String>,
    /// Display name of the event's actor, for rows and banners.
    pub actor_name: Option<String>,
    /// My PR, or a run I requested / for my PR.
    pub mine: bool,
    /// A run of a pipeline definition I pinned (spec 0014 FR-16).
    pub pinned: bool,
    /// The subject is a pinned PR or work item and the pin is my only
    /// relation to this event: sync emits it for the pin's change dot
    /// (spec 0014 FR-10), and it never becomes a notification (FR-16).
    pub pin_only: bool,
    /// I'm a required reviewer on this PR.
    pub required_reviewer: bool,
    /// I set auto-complete on this PR (0004 FR-12 push guard).
    pub auto_complete_mine: bool,
    pub vote: Option<Vote>,
    pub build_result: Option<RunResult>,
}

/// Rows of the editable priority table (Settings → Notifications).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RuleKey {
    Mention,
    ReviewRequested,
    BuildFailedMine,
    BuildFailedOther,
    MergeConflict,
    /// Spec 0001 Q-6: Azure DevOps rejected the token (or it expired).
    /// Accepts `TokenExpiring`, the row it replaced, from older settings.
    #[serde(alias = "TokenExpiring")]
    TokenRejected,
    /// The background replacement worked (owner decision 2026-10-01):
    /// fixed P3, not in the editable table.
    TokenReplaced,
    /// The background replacement still fails 2 days before expiry: fixed
    /// P2, not in the editable table.
    TokenNotReplaced,
    PrComment,
    VoteBlocking,
    VoteApproval,
    WorkItemAssigned,
    PrStatus,
    Nudge,
    PushAutoComplete,
    Push,
    ThreadResolved,
    WorkItemChanged,
    BuildFinished,
}

impl RuleKey {
    pub const ALL: [Self; 17] = [
        Self::Mention,
        Self::ReviewRequested,
        Self::BuildFailedMine,
        Self::BuildFailedOther,
        Self::MergeConflict,
        Self::TokenRejected,
        Self::PrComment,
        Self::VoteBlocking,
        Self::VoteApproval,
        Self::WorkItemAssigned,
        Self::PrStatus,
        Self::Nudge,
        Self::PushAutoComplete,
        Self::Push,
        Self::ThreadResolved,
        Self::WorkItemChanged,
        Self::BuildFinished,
    ];

    /// FR-3 defaults. Nothing defaults to P0.
    pub fn default_priority(self) -> Priority {
        use Priority::*;
        match self {
            Self::Mention
            | Self::ReviewRequested
            | Self::BuildFailedMine
            | Self::MergeConflict
            | Self::TokenRejected
            | Self::VoteBlocking
            | Self::PushAutoComplete => P1,
            Self::BuildFailedOther
            | Self::PrComment
            | Self::VoteApproval
            | Self::WorkItemAssigned
            | Self::PrStatus
            | Self::Nudge
            | Self::TokenNotReplaced => P2,
            Self::Push
            | Self::ThreadResolved
            | Self::WorkItemChanged
            | Self::BuildFinished
            | Self::TokenReplaced => P3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mention => "Mentioned in a PR or work item",
            Self::ReviewRequested => "Review requested",
            Self::BuildFailedMine => "Build failed (mine or my PR)",
            Self::BuildFailedOther => "Build failed (pinned pipeline)",
            Self::MergeConflict => "Merge conflict on my PR",
            Self::TokenRejected => "Token rejected or expired",
            Self::TokenReplaced => "Token replaced automatically",
            Self::TokenNotReplaced => "Token couldn't be replaced",
            Self::PrComment => "Comment on my PR or thread",
            Self::VoteBlocking => "Rejected or waiting for author",
            Self::VoteApproval => "Approved or vote reset",
            Self::WorkItemAssigned => "Work item assigned",
            Self::PrStatus => "My PR completed or abandoned",
            Self::Nudge => "Nudges",
            Self::PushAutoComplete => "Push on my auto-complete PR",
            Self::Push => "Push on a PR I review",
            Self::ThreadResolved => "Thread resolved",
            Self::WorkItemChanged => "Work item changed",
            Self::BuildFinished => "Build finished",
        }
    }

    pub fn of(kind: Kind, ctx: &EventContext) -> Self {
        use SyncEventKind::*;
        match kind {
            Kind::Nudge => Self::Nudge,
            Kind::Sync(kind) => match kind {
                MentionedInPr | MentionedInWorkItem => Self::Mention,
                ReviewRequested => Self::ReviewRequested,
                BuildCompleted => match ctx.build_result {
                    Some(RunResult::Failed) if ctx.mine => Self::BuildFailedMine,
                    // Spec 0014 FR-16 / 0005 FR-3: only pinned definitions
                    // raise other people's failures at P2.
                    Some(RunResult::Failed) if ctx.pinned => Self::BuildFailedOther,
                    _ => Self::BuildFinished,
                },
                PrMergeConflict => Self::MergeConflict,
                TokenRejected => Self::TokenRejected,
                TokenReplaced => Self::TokenReplaced,
                TokenNotReplaced => Self::TokenNotReplaced,
                PrCommentAdded => Self::PrComment,
                PrThreadResolved => Self::ThreadResolved,
                PrVoteChanged => match ctx.vote {
                    Some(Vote::WaitingForAuthor | Vote::Rejected) => Self::VoteBlocking,
                    _ => Self::VoteApproval,
                },
                WorkItemAssigned => Self::WorkItemAssigned,
                PrStatusChanged => Self::PrStatus,
                PrPushed if ctx.auto_complete_mine => Self::PushAutoComplete,
                PrPushed => Self::Push,
                WorkItemChanged => Self::WorkItemChanged,
            },
        }
    }
}

/// Event kind → priority, editable. Missing rows fall back to the FR-3
/// default, so new kinds need no settings migration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PriorityTable(pub BTreeMap<RuleKey, Priority>);

impl PriorityTable {
    pub fn get(&self, key: RuleKey) -> Priority {
        self.0
            .get(&key)
            .copied()
            .unwrap_or_else(|| key.default_priority())
    }

    /// Stores only overrides; setting a row back to its default removes it.
    pub fn set(&mut self, key: RuleKey, priority: Priority) {
        if priority == key.default_priority() {
            self.0.remove(&key);
        } else {
            self.0.insert(key, priority);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EventContext {
        EventContext::default()
    }

    #[test]
    fn fr3_default_priorities() {
        use SyncEventKind::*;
        let p = |kind, ctx: &EventContext| PriorityTable::default().get(RuleKey::of(kind, ctx));
        assert_eq!(p(Kind::Sync(MentionedInPr), &ctx()), Priority::P1);
        assert_eq!(p(Kind::Sync(MentionedInWorkItem), &ctx()), Priority::P1);
        assert_eq!(p(Kind::Sync(ReviewRequested), &ctx()), Priority::P1);
        assert_eq!(p(Kind::Sync(PrPushed), &ctx()), Priority::P3);
        let auto = EventContext {
            auto_complete_mine: true,
            ..ctx()
        };
        assert_eq!(p(Kind::Sync(PrPushed), &auto), Priority::P1);
        let failed_mine = EventContext {
            mine: true,
            build_result: Some(RunResult::Failed),
            ..ctx()
        };
        assert_eq!(p(Kind::Sync(BuildCompleted), &failed_mine), Priority::P1);
        let succeeded = EventContext {
            mine: true,
            build_result: Some(RunResult::Succeeded),
            ..ctx()
        };
        assert_eq!(p(Kind::Sync(BuildCompleted), &succeeded), Priority::P3);
        // 0005 FR-3 as amended by 0014: someone else's failure is P2 only on
        // a pinned definition.
        let failed_pinned = EventContext {
            pinned: true,
            build_result: Some(RunResult::Failed),
            ..ctx()
        };
        assert_eq!(p(Kind::Sync(BuildCompleted), &failed_pinned), Priority::P2);
        assert_eq!(
            RuleKey::of(Kind::Sync(BuildCompleted), &failed_pinned),
            RuleKey::BuildFailedOther
        );
        let failed_elsewhere = EventContext {
            build_result: Some(RunResult::Failed),
            ..ctx()
        };
        assert_eq!(
            p(Kind::Sync(BuildCompleted), &failed_elsewhere),
            Priority::P3
        );
        assert_eq!(
            RuleKey::of(Kind::Sync(BuildCompleted), &failed_elsewhere),
            RuleKey::BuildFinished
        );
        let reject = EventContext {
            vote: Some(Vote::Rejected),
            ..ctx()
        };
        assert_eq!(p(Kind::Sync(PrVoteChanged), &reject), Priority::P1);
        let approve = EventContext {
            vote: Some(Vote::Approved),
            ..ctx()
        };
        assert_eq!(p(Kind::Sync(PrVoteChanged), &approve), Priority::P2);
        assert_eq!(p(Kind::Nudge, &ctx()), Priority::P2);
        assert_eq!(p(Kind::Sync(WorkItemChanged), &ctx()), Priority::P3);
    }

    #[test]
    fn no_default_is_p0() {
        assert!(
            RuleKey::ALL
                .iter()
                .all(|key| key.default_priority() != Priority::P0)
        );
    }

    /// Spec 0001 Q-6 and the owner's 2026-10-01 decision: rejection is P1,
    /// a background replacement P3, a failing one P2; a settings file that
    /// still names the old `TokenExpiring` row keeps its override.
    #[test]
    fn token_notices_classify_and_old_settings_still_load() {
        use SyncEventKind::*;
        let p = |kind| PriorityTable::default().get(RuleKey::of(Kind::Sync(kind), &ctx()));
        assert_eq!(p(TokenRejected), Priority::P1);
        assert_eq!(p(TokenReplaced), Priority::P3);
        assert_eq!(p(TokenNotReplaced), Priority::P2);
        let table: PriorityTable = toml::from_str(r#"TokenExpiring = "P2""#).unwrap();
        assert_eq!(table.get(RuleKey::TokenRejected), Priority::P2);
    }

    #[test]
    fn table_stores_only_overrides() {
        let mut table = PriorityTable::default();
        table.set(RuleKey::Push, Priority::P2);
        assert_eq!(table.get(RuleKey::Push), Priority::P2);
        table.set(RuleKey::Push, Priority::P3);
        assert!(table.0.is_empty());
    }

    #[test]
    fn kinds_round_trip() {
        for kind in SyncEventKind::ALL {
            let kind = Kind::Sync(*kind);
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("nudge"), Some(Kind::Nudge));
    }
}
