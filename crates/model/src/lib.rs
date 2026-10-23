//! Plain Azure DevOps domain types (spec 0002 FR-1, ADR 0001 schema).
//!
//! No IO here: `adoken-store` persists these, `adoken-sync` produces them from
//! ADO payloads, `adoken-demo` fabricates them, and views render them.
//!
//! Conventions:
//! - Timestamps are Unix milliseconds UTC ([`Millis`]), like the store.
//! - People are referenced by ADO identity id ([`PersonId`]); resolve names via
//!   the store's `people()`.
//! - Enums that mirror ADO strings round-trip through `as_str` / `parse` so the
//!   store keeps ADO's own spelling.

use serde::{Deserialize, Serialize};

/// Unix milliseconds, UTC.
pub type Millis = i64;
/// ADO identity id (GUID string).
pub type PersonId = String;

pub mod time {
    use super::Millis;
    pub const SECOND: Millis = 1_000;
    pub const MINUTE: Millis = 60 * SECOND;
    pub const HOUR: Millis = 60 * MINUTE;
    pub const DAY: Millis = 24 * HOUR;
}

macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            /// ADO's wire spelling.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $s),+ }
            }
            /// Case-insensitive parse of ADO's wire spelling.
            pub fn parse(s: &str) -> Option<Self> {
                $(if s.eq_ignore_ascii_case($s) { return Some($name::$variant); })+
                None
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

// ── org structure ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Org {
    pub id: String,
    /// Organization name as in `dev.azure.com/{name}`.
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// Full ref, e.g. `refs/heads/main`.
    pub default_branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: PersonId,
    pub display_name: String,
    /// `alice@contoso.com` for users, `[Project]\Team` for groups.
    pub unique_name: Option<String>,
    pub is_group: bool,
    /// The payload's `imageUrl` when it is Azure DevOps' identity-image
    /// route (spec 0012 FR-1). Informational: avatars are always fetched
    /// from the org host by id (`adoken_sync::avatars::avatar_url`).
    #[serde(default)]
    pub image_url: Option<String>,
}

// ── pull requests ─────────────────────────────────────────────────────────

str_enum!(
    /// `pullRequest.status`.
    PrStatus { Active => "active", Completed => "completed", Abandoned => "abandoned" }
);

str_enum!(
    /// `pullRequest.mergeStatus`.
    MergeStatus {
        NotSet => "notSet",
        Queued => "queued",
        Conflicts => "conflicts",
        Succeeded => "succeeded",
        RejectedByPolicy => "rejectedByPolicy",
        Failure => "failure",
    }
);

impl MergeStatus {
    /// The merge can't go through as is: a queued Complete failed.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            MergeStatus::Conflicts | MergeStatus::RejectedByPolicy | MergeStatus::Failure
        )
    }
}

/// Reviewer vote, ADO's integer scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Vote {
    Approved,
    ApprovedWithSuggestions,
    NoVote,
    WaitingForAuthor,
    Rejected,
}

impl Vote {
    pub const ALL: &'static [Vote] = &[
        Vote::Approved,
        Vote::ApprovedWithSuggestions,
        Vote::NoVote,
        Vote::WaitingForAuthor,
        Vote::Rejected,
    ];

    /// ADO's wire value: 10, 5, 0, -5, -10.
    pub fn as_i8(self) -> i8 {
        match self {
            Vote::Approved => 10,
            Vote::ApprovedWithSuggestions => 5,
            Vote::NoVote => 0,
            Vote::WaitingForAuthor => -5,
            Vote::Rejected => -10,
        }
    }

    /// Unknown values collapse to the nearest meaning by sign.
    pub fn from_i8(v: i8) -> Self {
        match v {
            10.. => Vote::Approved,
            1..=9 => Vote::ApprovedWithSuggestions,
            0 => Vote::NoVote,
            -9..=-1 => Vote::WaitingForAuthor,
            _ => Vote::Rejected,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Vote::Approved => "Approved",
            Vote::ApprovedWithSuggestions => "Approved with suggestions",
            Vote::NoVote => "No vote",
            Vote::WaitingForAuthor => "Waiting for author",
            Vote::Rejected => "Rejected",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewer {
    pub person: PersonId,
    pub vote: Vote,
    pub is_required: bool,
    pub has_declined: bool,
    /// When the reviewer was added (from the reviewer-added system thread).
    pub added_at: Option<Millis>,
    /// When the current vote was cast; `None` while `NoVote`.
    pub voted_at: Option<Millis>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    /// `pullRequestId`, unique per org.
    pub id: i64,
    pub repo_id: String,
    pub project_id: String,
    pub title: String,
    /// Markdown. The list API truncates to 400 chars until hydrated.
    pub description: String,
    pub author: PersonId,
    pub reviewers: Vec<Reviewer>,
    pub status: PrStatus,
    pub is_draft: bool,
    pub merge_status: MergeStatus,
    /// Full ref, e.g. `refs/heads/feature/x`.
    pub source_branch: String,
    pub target_branch: String,
    pub last_source_commit: Option<String>,
    pub created: Millis,
    /// Latest activity seen (push, vote, comment, status). ADO has no field
    /// for this; sync derives it.
    pub updated: Millis,
    pub closed: Option<Millis>,
    /// Number of pushes (iterations) so far.
    pub iterations: u32,
    /// Time of the latest push (latest iteration's `createdDate`).
    pub last_push: Option<Millis>,
}

impl PullRequest {
    pub fn is_reviewer(&self, who: &str) -> bool {
        self.reviewers.iter().any(|r| r.person == who)
    }
    pub fn vote_of(&self, who: &str) -> Option<Vote> {
        self.reviewers
            .iter()
            .find(|r| r.person == who)
            .map(|r| r.vote)
    }
}

/// Strip `refs/heads/` for display.
pub fn short_branch(r: &str) -> &str {
    r.strip_prefix("refs/heads/").unwrap_or(r)
}

str_enum!(
    /// PR thread status.
    ThreadStatus {
        Active => "active",
        Fixed => "fixed",
        WontFix => "wontFix",
        Closed => "closed",
        ByDesign => "byDesign",
        Pending => "pending",
        Unknown => "unknown",
    }
);

impl ThreadStatus {
    pub fn is_resolved(self) -> bool {
        matches!(
            self,
            ThreadStatus::Fixed
                | ThreadStatus::WontFix
                | ThreadStatus::Closed
                | ThreadStatus::ByDesign
        )
    }
}

/// Inclusive 1-based line range in one side of a diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

/// Where a thread is anchored. `None` on the thread = PR-level discussion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAnchor {
    pub file_path: String,
    /// Lines in the new file (`rightFileStart/End`).
    pub right: Option<LineRange>,
    /// Lines in the old file (`leftFileStart/End`).
    pub left: Option<LineRange>,
    /// Right-side blob of `file_path` at the thread's iteration, so the
    /// anchor can be mapped onto later iterations. `None` = unknown.
    pub iteration_blob: Option<BlobId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    /// ADO comment id, unique within its thread.
    pub id: i64,
    pub parent_id: Option<i64>,
    pub author: PersonId,
    /// Markdown; mentions encoded as `@<identity-guid>`.
    pub body: String,
    pub created: Millis,
    pub updated: Millis,
    pub is_system: bool,
    pub is_deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrThread {
    /// ADO thread id, unique within its PR.
    pub id: i64,
    pub pr_id: i64,
    pub status: ThreadStatus,
    pub anchor: Option<ThreadAnchor>,
    /// Iteration the anchor refers to.
    pub iteration: Option<u32>,
    pub comments: Vec<Comment>,
    pub updated: Millis,
}

str_enum!(
    ChangeType { Add => "add", Edit => "edit", Delete => "delete", Rename => "rename" }
);

/// Content-addressed blob reference (git object id). Resolve with
/// `Store::blob`.
pub type BlobId = String;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrFileChange {
    pub path: String,
    /// Set for renames.
    pub original_path: Option<String>,
    pub change_type: ChangeType,
    /// `None` for adds.
    pub old_content: Option<BlobId>,
    /// `None` for deletes.
    pub new_content: Option<BlobId>,
    /// `changeEntries[].changeTrackingId` of the displayed iteration: sent
    /// with new line comments so Azure DevOps tracks them across later
    /// pushes (spec 0004 FR-6). `None` = unknown (never sent as 0).
    #[serde(default)]
    pub change_tracking_id: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrIteration {
    pub id: u32,
    pub source_commit: String,
    pub created: Millis,
    pub description: Option<String>,
    /// Commits this push added (its commits minus the previous iteration's,
    /// spec 0017 FR-4). `None` = unknown (synced before commit counts).
    #[serde(default)]
    pub commit_count: Option<u32>,
    /// Azure DevOps cut the commit list short (`hasMoreCommits`): the count
    /// is a lower bound.
    #[serde(default)]
    pub commits_truncated: bool,
}

/// A PR with everything the review view needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetail {
    pub pr: PullRequest,
    pub iterations: Vec<PrIteration>,
    /// Files changed in the latest iteration vs target.
    pub files: Vec<PrFileChange>,
    pub threads: Vec<PrThread>,
}

// ── work items ────────────────────────────────────────────────────────────

/// A work item type, by its Azure DevOps name. Open-ended: every process
/// template (Agile, Scrum, CMMI, Basic, inherited and custom ones) names its
/// own types ("Product Backlog Item", "Requirement", "Impediment",
/// "Test Case", "Incident"...), so the name is kept verbatim. Views pick an
/// icon and color through [`Self::family`].
///
/// The associated constants name the stock types, for tests, fixtures and
/// the demo org: `WorkItemType::Bug`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkItemType(std::borrow::Cow<'static, str>);

#[allow(non_upper_case_globals)]
impl WorkItemType {
    pub const Bug: Self = Self::stock("Bug");
    pub const UserStory: Self = Self::stock("User Story");
    pub const ProductBacklogItem: Self = Self::stock("Product Backlog Item");
    pub const Requirement: Self = Self::stock("Requirement");
    pub const Task: Self = Self::stock("Task");
    pub const Feature: Self = Self::stock("Feature");
    pub const Epic: Self = Self::stock("Epic");
    pub const Issue: Self = Self::stock("Issue");

    /// The stock types of the four system processes, plus the Basic `Issue`.
    const STOCK: &'static [WorkItemType] = &[
        Self::Bug,
        Self::UserStory,
        Self::ProductBacklogItem,
        Self::Requirement,
        Self::Task,
        Self::Feature,
        Self::Epic,
        Self::Issue,
    ];

    const fn stock(name: &'static str) -> Self {
        Self(std::borrow::Cow::Borrowed(name))
    }

    /// A type named `name` (any process, custom types included).
    pub fn new(name: impl Into<String>) -> Self {
        Self(std::borrow::Cow::Owned(name.into()))
    }

    /// ADO's spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Any non-empty name. Stock names match case-insensitively and keep
    /// ADO's spelling (`"user story"` → `User Story`).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        Some(
            Self::STOCK
                .iter()
                .find(|t| t.as_str().eq_ignore_ascii_case(s))
                .cloned()
                .unwrap_or_else(|| Self::new(s)),
        )
    }

    /// The process-independent family, for icons and colors. Stock names
    /// of every system process map to their backlog level; anything else
    /// (custom types) is [`TypeFamily::Other`].
    pub fn family(&self) -> TypeFamily {
        let name = self.as_str();
        let is = |s: &str| name.eq_ignore_ascii_case(s);
        if is("Bug") || is("Defect") {
            TypeFamily::Bug
        } else if is("User Story") || is("Product Backlog Item") || is("Requirement") {
            TypeFamily::Requirement
        } else if is("Task") {
            TypeFamily::Task
        } else if is("Feature") {
            TypeFamily::Feature
        } else if is("Epic") {
            TypeFamily::Epic
        } else if is("Issue") || is("Impediment") || is("Risk") || is("Change Request") {
            TypeFamily::Issue
        } else {
            TypeFamily::Other
        }
    }
}

impl std::fmt::Display for WorkItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for WorkItemType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WorkItemType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        // Before types were open-ended, serde wrote the enum variant
        // (`UserStory`); every other stock variant matched its ADO name.
        let s = if s == "UserStory" {
            "User Story".to_owned()
        } else {
            s
        };
        Self::parse(&s).ok_or_else(|| serde::de::Error::custom("empty work item type"))
    }
}

/// Backlog level of a work item type, whatever its process calls it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeFamily {
    Bug,
    /// User Story (Agile), Product Backlog Item (Scrum), Requirement (CMMI).
    Requirement,
    Task,
    Feature,
    Epic,
    /// Issue (Agile, Basic), Impediment (Scrum), Risk and Change Request (CMMI).
    Issue,
    /// Custom and non-backlog types (Test Case, Incident...).
    Other,
}

str_enum!(
    /// Process-independent state category (ADO `stateCategory`): custom
    /// states such as "Doing" map to `InProgress`.
    StateCategory {
        Proposed => "Proposed",
        InProgress => "InProgress",
        Resolved => "Resolved",
        Completed => "Completed",
        Removed => "Removed",
    }
);

impl StateCategory {
    /// Fallback category of the stock Agile/Scrum/Basic/CMMI state names,
    /// for when a type's states haven't been fetched yet. The real category
    /// comes from `workitemtypes/{type}` (each state's `category`), cached
    /// per project and type in the store; sync and the store prefer it.
    pub fn of_state(state: &str) -> Self {
        let is = |s: &str| state.eq_ignore_ascii_case(s);
        if is("New") || is("Proposed") || is("To Do") || is("Approved") {
            StateCategory::Proposed
        } else if is("Resolved") {
            StateCategory::Resolved
        } else if is("Closed") || is("Done") || is("Completed") {
            StateCategory::Completed
        } else if is("Removed") {
            StateCategory::Removed
        } else {
            StateCategory::InProgress
        }
    }
    pub fn is_open(self) -> bool {
        matches!(self, StateCategory::Proposed | StateCategory::InProgress)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: i64,
    pub project_id: String,
    /// Revision; upserts only move forward.
    pub rev: i64,
    pub kind: WorkItemType,
    pub title: String,
    /// Free-form per process template: New, Active, Resolved, Closed, ...
    pub state: String,
    pub state_category: StateCategory,
    pub assigned_to: Option<PersonId>,
    pub created_by: PersonId,
    pub changed_by: PersonId,
    pub area_path: String,
    pub iteration_path: String,
    /// 1 (highest) ..= 4.
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub parent_id: Option<i64>,
    /// PRs linked via ArtifactLink relations.
    pub linked_pr_ids: Vec<i64>,
    /// Plain text; hot set only.
    pub description: Option<String>,
    pub comment_count: u32,
    pub created: Millis,
    pub changed: Millis,
}

impl WorkItem {
    pub fn is_open(&self) -> bool {
        self.state_category.is_open()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItemComment {
    pub id: i64,
    pub work_item_id: i64,
    pub author: PersonId,
    /// HTML converted to text; mentions encoded as `@<identity-guid>`.
    pub body: String,
    /// People mentioned per the API's `mentions[]`, beyond the body's
    /// `@<guid>` tokens (the store indexes both).
    pub mentions: Vec<PersonId>,
    pub created: Millis,
    pub updated: Millis,
    /// Raw text as Azure DevOps stores it (spec 0015 FR-34); `None` for
    /// rows synced before 0015.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub format: TextFormat,
}

mod wi;
pub use wi::*;

// ── pipelines ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pipeline {
    /// Build definition id, unique per project.
    pub id: i64,
    pub project_id: String,
    pub name: String,
    pub folder: String,
}

str_enum!(
    RunStatus {
        NotStarted => "notStarted",
        InProgress => "inProgress",
        Completed => "completed",
        Cancelling => "cancelling",
        Postponed => "postponed",
    }
);

str_enum!(
    RunResult {
        Succeeded => "succeeded",
        PartiallySucceeded => "partiallySucceeded",
        Failed => "failed",
        Canceled => "canceled",
    }
);

str_enum!(
    RunReason {
        Manual => "manual",
        IndividualCi => "individualCI",
        BatchedCi => "batchedCI",
        PullRequest => "pullRequest",
        Schedule => "schedule",
        Other => "other",
    }
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    /// Build id, unique per org.
    pub id: i64,
    pub project_id: String,
    pub pipeline_id: i64,
    pub pipeline_name: String,
    /// e.g. `20260925.3`.
    pub run_number: String,
    pub status: RunStatus,
    /// Set once completed.
    pub result: Option<RunResult>,
    pub reason: RunReason,
    /// Full ref.
    pub branch: String,
    pub source_version: Option<String>,
    pub requested_for: Option<PersonId>,
    /// Set when `reason == PullRequest`.
    pub pr_id: Option<i64>,
    pub queued: Millis,
    pub started: Option<Millis>,
    pub finish: Option<Millis>,
}

str_enum!(
    /// A stage's state in the build timeline (spec 0008 FR-5); the store
    /// keeps these spellings (`Partial` is ADO's `succeededWithIssues`).
    StageOutcome {
        Succeeded => "succeeded",
        Partial => "succeededWithIssues",
        Failed => "failed",
        Canceled => "canceled",
        Skipped => "skipped",
        Running => "inProgress",
        Pending => "pending",
    }
);

impl StageOutcome {
    pub fn label(self) -> &'static str {
        match self {
            StageOutcome::Succeeded => "Succeeded",
            StageOutcome::Partial => "Succeeded with issues",
            StageOutcome::Failed => "Failed",
            StageOutcome::Canceled => "Canceled",
            StageOutcome::Skipped => "Skipped",
            StageOutcome::Running => "Running",
            StageOutcome::Pending => "Waiting",
        }
    }
}

/// One stage of a run, in pipeline order (from the build timeline).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStage {
    pub name: String,
    pub outcome: StageOutcome,
    pub started: Option<Millis>,
    pub finished: Option<Millis>,
}

// ── mentions ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MentionSource {
    Pr {
        pr_id: i64,
        thread_id: i64,
        comment_id: i64,
    },
    WorkItem {
        work_item_id: i64,
        comment_id: i64,
    },
}

impl MentionSource {
    /// Stable entity key of the subject (`pr:4312`, `wi:88121`).
    pub fn subject(&self) -> Subject {
        match *self {
            MentionSource::Pr { pr_id, .. } => Subject::Pr(pr_id),
            MentionSource::WorkItem { work_item_id, .. } => Subject::WorkItem(work_item_id),
        }
    }
}

/// Someone mentioned `mentioned` in a comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    pub source: MentionSource,
    /// Comment author.
    pub who: PersonId,
    pub mentioned: PersonId,
    pub when: Millis,
    /// Comment text around the mention, mention tokens resolved to names.
    pub snippet: String,
}

// ── sync events (spec 0002 FR-11) ─────────────────────────────────────────

/// What an event is about.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Subject {
    Pr(i64),
    WorkItem(i64),
    Build(i64),
    Account,
}

impl Subject {
    /// Stable key: `pr:4312`, `wi:88121`, `run:5521`, `account`.
    pub fn key(&self) -> String {
        match self {
            Subject::Pr(id) => format!("pr:{id}"),
            Subject::WorkItem(id) => format!("wi:{id}"),
            Subject::Build(id) => format!("run:{id}"),
            Subject::Account => "account".to_owned(),
        }
    }

    pub fn parse_key(key: &str) -> Option<Self> {
        if key == "account" {
            return Some(Subject::Account);
        }
        let (kind, id) = key.split_once(':')?;
        let id = id.parse().ok()?;
        match kind {
            "pr" => Some(Subject::Pr(id)),
            "wi" => Some(Subject::WorkItem(id)),
            "run" => Some(Subject::Build(id)),
            _ => None,
        }
    }
}

str_enum!(
    /// Event kinds from spec 0002 FR-11.
    SyncEventKind {
        ReviewRequested => "ReviewRequested",
        PrCommentAdded => "PrCommentAdded",
        PrThreadResolved => "PrThreadResolved",
        MentionedInPr => "MentionedInPr",
        MentionedInWorkItem => "MentionedInWorkItem",
        PrVoteChanged => "PrVoteChanged",
        PrPushed => "PrPushed",
        PrStatusChanged => "PrStatusChanged",
        PrMergeConflict => "PrMergeConflict",
        WorkItemAssigned => "WorkItemAssigned",
        WorkItemChanged => "WorkItemChanged",
        BuildCompleted => "BuildCompleted",
        // Account notices adoken posts itself (spec 0001 FR-15 Q-6, owner
        // decision 2026-10-01); never emitted by sync.
        TokenRejected => "TokenRejected",
        TokenReplaced => "TokenReplaced",
        TokenNotReplaced => "TokenNotReplaced",
    }
);

/// One normalized change, persisted and deduplicated by
/// `(kind, subject, dedupe_key)`. Consumed by notify (0005) and nudges (0006).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncEvent {
    pub id: i64,
    pub kind: SyncEventKind,
    pub subject: Subject,
    /// Per-kind key from the FR-11 table (comment id, commit id, ...).
    pub dedupe_key: String,
    /// Who caused it, when known.
    pub actor: Option<PersonId>,
    /// FR-12: actor is me. Consumers hide these by default.
    pub self_caused: bool,
    /// When the change happened on the server (or was observed).
    pub at: Millis,
    /// One-line human summary, e.g. "Priya Shah approved #4312 Fix flaky checkout".
    pub summary: String,
}

impl SyncEvent {
    /// Globally unique dedupe string: `kind|subject|dedupe_key`.
    pub fn unique_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.kind.as_str(),
            self.subject.key(),
            self.dedupe_key
        )
    }
}

// ── pins (spec 0014) ──────────────────────────────────────────────────────

/// At most this many pins (FR-3).
pub const MAX_PINS: usize = 20;
/// Local labels are at most this many characters (FR-14).
pub const MAX_PIN_LABEL_CHARS: usize = 60;

/// What a pin points at (FR-1). A run pins its definition.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinSubject {
    Pr(i64),
    WorkItem(i64),
    /// Build definitions are unique per project.
    Definition {
        project_id: String,
        id: i64,
    },
}

impl PinSubject {
    /// Stable key, the palette's entity keys: `pr:4821`, `wi:48213`,
    /// `pipe:<project id>:<definition id>`.
    pub fn key(&self) -> String {
        match self {
            PinSubject::Pr(id) => format!("pr:{id}"),
            PinSubject::WorkItem(id) => format!("wi:{id}"),
            PinSubject::Definition { project_id, id } => format!("pipe:{project_id}:{id}"),
        }
    }

    pub fn parse_key(key: &str) -> Option<Self> {
        let (kind, rest) = key.split_once(':')?;
        match kind {
            "pr" => rest.parse().ok().map(PinSubject::Pr),
            "wi" => rest.parse().ok().map(PinSubject::WorkItem),
            "pipe" => {
                let (project_id, id) = rest.rsplit_once(':')?;
                Some(PinSubject::Definition {
                    project_id: project_id.to_owned(),
                    id: id.parse().ok()?,
                })
            }
            _ => None,
        }
    }

    /// The pin row's reference: `!4821`, `#48213`; definitions have none.
    pub fn reference(&self) -> Option<String> {
        match self {
            PinSubject::Pr(id) => Some(format!("!{id}")),
            PinSubject::WorkItem(id) => Some(format!("#{id}")),
            PinSubject::Definition { .. } => None,
        }
    }

    /// The sync event subject whose events light the change dot (FR-10);
    /// definitions compare run results instead.
    pub fn event_subject(&self) -> Option<Subject> {
        match self {
            PinSubject::Pr(id) => Some(Subject::Pr(*id)),
            PinSubject::WorkItem(id) => Some(Subject::WorkItem(*id)),
            PinSubject::Definition { .. } => None,
        }
    }
}

str_enum!(
    /// Why a pinned item can't be read any more (FR-13): `Gone` = 404, or
    /// omitted by `workitemsbatch` (deleted or moved); `Forbidden` = 403.
    PinUnavailable {
        Gone => "gone",
        Forbidden => "forbidden",
    }
);

/// One pin: user-authored local state (FR-15).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub subject: PinSubject,
    /// 0-based order.
    pub position: u32,
    /// Local label (FR-14); never written to ADO.
    pub label: Option<String>,
    /// Last known Azure DevOps title, kept for "Not available" rows.
    pub title: String,
    pub pinned_at: Millis,
    pub opened_at: Option<Millis>,
    /// Definitions: the latest completed run's result when last opened (or
    /// pinned); the change dot lights when it differs (FR-10).
    pub seen_result: Option<RunResult>,
    /// Set by sync on 404 / 403 (FR-13); cleared when the item reads again.
    pub unavailable: Option<PinUnavailable>,
}

impl Pin {
    /// When the change dot compares against: last opened, else pinned.
    pub fn seen_at(&self) -> Millis {
        self.opened_at.unwrap_or(self.pinned_at)
    }

    /// The label if set, else the Azure DevOps title.
    pub fn display_title(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_wire_spelling() {
        for s in PrStatus::ALL {
            assert_eq!(PrStatus::parse(s.as_str()), Some(*s));
        }
        for s in SyncEventKind::ALL {
            assert_eq!(SyncEventKind::parse(s.as_str()), Some(*s));
        }
        assert_eq!(
            MergeStatus::parse("CONFLICTS"),
            Some(MergeStatus::Conflicts)
        );
        assert_eq!(
            WorkItemType::parse("user story"),
            Some(WorkItemType::UserStory)
        );
        assert_eq!(RunResult::parse("nope"), None);
    }

    #[test]
    fn work_item_types_are_open_ended() {
        // Scrum, CMMI and custom types survive; stock names map to a family.
        let pbi = WorkItemType::parse("Product Backlog Item").unwrap();
        assert_eq!(pbi, WorkItemType::ProductBacklogItem);
        assert_eq!(pbi.family(), TypeFamily::Requirement);
        assert_eq!(
            WorkItemType::parse("Requirement").unwrap().family(),
            TypeFamily::Requirement
        );
        assert_eq!(
            WorkItemType::parse("Impediment").unwrap().family(),
            TypeFamily::Issue
        );
        let custom = WorkItemType::parse("Incident").unwrap();
        assert_eq!(custom.as_str(), "Incident");
        assert_eq!(custom.family(), TypeFamily::Other);
        assert_eq!(WorkItemType::parse("  "), None);
        // Serialized as the ADO name; the old enum spelling still reads.
        let json = serde_json::to_string(&WorkItemType::UserStory).unwrap();
        assert_eq!(json, "\"User Story\"");
        let old: WorkItemType = serde_json::from_str("\"UserStory\"").unwrap();
        assert_eq!(old, WorkItemType::UserStory);
    }

    #[test]
    fn vote_scale_round_trips_and_clamps() {
        for v in Vote::ALL {
            assert_eq!(Vote::from_i8(v.as_i8()), *v);
        }
        assert_eq!(Vote::from_i8(7), Vote::ApprovedWithSuggestions);
        assert_eq!(Vote::from_i8(-128), Vote::Rejected);
    }

    #[test]
    fn subject_key_round_trips() {
        for s in [
            Subject::Pr(4312),
            Subject::WorkItem(88121),
            Subject::Build(5521),
            Subject::Account,
        ] {
            assert_eq!(Subject::parse_key(&s.key()), Some(s));
        }
        assert_eq!(Subject::parse_key("pr:x"), None);
        assert_eq!(Subject::parse_key("zz:1"), None);
    }

    #[test]
    fn pin_keys_round_trip_including_project_ids_with_colons() {
        for s in [
            PinSubject::Pr(4821),
            PinSubject::WorkItem(48213),
            PinSubject::Definition {
                project_id: "a1b2-c3".into(),
                id: 7,
            },
            PinSubject::Definition {
                project_id: "odd:id".into(),
                id: 12,
            },
        ] {
            assert_eq!(PinSubject::parse_key(&s.key()), Some(s));
        }
        assert_eq!(PinSubject::parse_key("pipe:p1"), None);
        assert_eq!(PinSubject::parse_key("run:5"), None);
    }

    #[test]
    fn resolved_statuses() {
        assert!(ThreadStatus::Fixed.is_resolved());
        assert!(!ThreadStatus::Active.is_resolved());
        assert!(!ThreadStatus::Pending.is_resolved());
    }
}
