//! User-initiated pull request writes (spec 0004 FR-6, FR-8..FR-10, FR-12,
//! FR-16, FR-17; spec 0002 FR-14/FR-15): line and PR comments, replies,
//! thread status, votes, complete, auto-complete with its unseen-commit
//! guard, the PR's status (publish, draft, abandon, reactivate), and links
//! between the PR and work items.
//!
//! - [`ReviewWrite::request`] is the exact REST call (method, URL with
//!   `api-version`, JSON body); pure, so tests assert it byte for byte.
//! - [`Writer::send`] sends it at [`Tier::User`] through the engine's
//!   governor and transport (so the ledger records it), maps failures to a
//!   [`WriteError`] the UI can show as-is, and applies the server's answer to
//!   the store in one transaction (server wins, FR-15) together with
//!   self-caused events, the same ones a later sync pass would derive, so
//!   notify hides them and Ken scores them.
//! - [`apply_local`] applies a write to the store without any network (the
//!   demo org): same store effects, same events.
//!
//! Nothing here retries on its own: a failed write reverts in the UI and
//! waits for the user.

use std::sync::Arc;

use adoken_model::{
    Comment, MergeStatus, Millis, PersonId, PrStatus, PrThread, PullRequest, Reviewer, Subject,
    SyncEventKind, ThreadAnchor, ThreadStatus, Vote, WorkItem,
};
use adoken_store::rusqlite::Connection;
use adoken_store::{Store, read, write};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;

use crate::Tier;
use crate::activity::Tracker;
use crate::attribution::{self, Origin, UserAction};
use crate::clock::Clock;
use crate::engine::{API_VERSION, Emitter, apply_pr, apply_work_items, name, status_key};
use crate::governor::{GovError, Governor};
use crate::transport::{ApiRequest, ApiResponse, Transport};
use crate::wire::{
    self, GitPullRequest, List, Reviewer as WireReviewer, WireComment, WireThread, WireWorkItem,
};

/// `autoCompleteSetBy.id` that clears auto-complete.
pub const NO_IDENTITY: &str = "00000000-0000-0000-0000-000000000000";

/// The scope every write needs (spec 0004 §5); named in 403 errors.
pub const WRITE_SCOPE: &str = "vso.code_write";

/// Linking work items to a PR edits the work items (spec 0004 FR-17,
/// spec 0001 §5.2).
pub const WORK_WRITE_SCOPE: &str = "vso.work_write";

/// Where a pull request lives: every PR endpoint is project + repo scoped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    pub project_id: String,
    pub repo_id: String,
    pub pr_id: i64,
}

impl PrRef {
    pub fn of(pr: &PullRequest) -> Self {
        PrRef {
            project_id: pr.project_id.clone(),
            repo_id: pr.repo_id.clone(),
            pr_id: pr.id,
        }
    }

    fn path(&self, rest: &str) -> String {
        format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}{rest}",
            self.project_id, self.repo_id, self.pr_id
        )
    }

    /// The PR's artifact URL, as a work item's `ArtifactLink` relation
    /// names it (the web UI's own form, verified live 2026-09-30).
    pub fn artifact_url(&self) -> String {
        format!(
            "vstfs:///Git/PullRequestId/{}%2F{}%2F{}",
            self.project_id, self.repo_id, self.pr_id
        )
    }
}

/// Diff side of a line comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The base (`leftFileStart`), e.g. a deleted line.
    Left,
    /// The PR's version (`rightFileStart`).
    Right,
}

/// A one-line comment anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineAnchor {
    /// Repo path; a leading `/` is added if missing (ADO's form).
    pub path: String,
    pub side: Side,
    /// 1-based line number on `side`.
    pub line: u32,
    /// The file's `changeTrackingId` in the displayed iteration, so Azure
    /// DevOps tracks the comment across later pushes (spec 0004 FR-6).
    /// `None` = unknown: the field is omitted, never sent as 0.
    pub change_tracking_id: Option<u32>,
}

impl LineAnchor {
    pub fn file_path(&self) -> String {
        if self.path.starts_with('/') {
            self.path.clone()
        } else {
            format!("/{}", self.path)
        }
    }
}

/// `completionOptions.mergeStrategy`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Merge commit (ADO's default).
    #[default]
    NoFastForward,
    Squash,
    Rebase,
    RebaseMerge,
}

impl MergeStrategy {
    pub const ALL: [Self; 4] = [
        Self::Squash,
        Self::NoFastForward,
        Self::Rebase,
        Self::RebaseMerge,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoFastForward => "noFastForward",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::RebaseMerge => "rebaseMerge",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NoFastForward => "Merge",
            Self::Squash => "Squash",
            Self::Rebase => "Rebase",
            Self::RebaseMerge => "Semi-linear",
        }
    }
}

/// `completionOptions` (spec 0004 flow step 7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionOptions {
    pub merge_strategy: MergeStrategy,
    pub delete_source_branch: bool,
    pub transition_work_items: bool,
    /// `None` = Azure DevOps' default message.
    pub merge_commit_message: Option<String>,
}

impl Default for CompletionOptions {
    fn default() -> Self {
        CompletionOptions {
            merge_strategy: MergeStrategy::default(),
            delete_source_branch: true,
            transition_work_items: true,
            merge_commit_message: None,
        }
    }
}

impl CompletionOptions {
    fn json(&self) -> Value {
        let mut options = json!({
            "mergeStrategy": self.merge_strategy.as_str(),
            "deleteSourceBranch": self.delete_source_branch,
            "transitionWorkItems": self.transition_work_items,
        });
        if let Some(message) = self.merge_commit_message.as_deref().map(str::trim)
            && !message.is_empty()
        {
            options["mergeCommitMessage"] = json!(message);
        }
        options
    }
}

/// One user action on a pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewWrite {
    /// A new thread: on a line (FR-6) or PR-level (`anchor: None`).
    /// `iteration` is the displayed iteration.
    CreateThread {
        pr: PrRef,
        anchor: Option<LineAnchor>,
        iteration: u32,
        body: String,
    },
    Reply {
        pr: PrRef,
        thread_id: i64,
        /// The thread's first comment (ADO threads replies under it).
        parent_comment_id: i64,
        body: String,
    },
    /// Resolve (`fixed`, …) or reactivate (`active`).
    SetThreadStatus {
        pr: PrRef,
        thread_id: i64,
        status: ThreadStatus,
    },
    /// FR-9: `PUT …/reviewers/{me}` with ADO's vote value.
    Vote {
        pr: PrRef,
        reviewer: PersonId,
        vote: Vote,
    },
    /// FR-10: merge exactly the displayed source commit.
    Complete {
        pr: PrRef,
        source_commit: String,
        options: CompletionOptions,
    },
    /// FR-10/FR-12: auto-complete set by me; `source_commit` is what I
    /// reviewed (the guard).
    SetAutoComplete {
        pr: PrRef,
        me: PersonId,
        source_commit: String,
        options: CompletionOptions,
    },
    CancelAutoComplete {
        pr: PrRef,
    },
    /// FR-16: publish (`draft: false`) or mark as draft (`true`).
    SetDraft {
        pr: PrRef,
        draft: bool,
    },
    /// FR-16: `status: abandoned`, after the user confirmed.
    Abandon {
        pr: PrRef,
    },
    /// FR-16: an abandoned PR becomes active again.
    Reactivate {
        pr: PrRef,
    },
    /// FR-17: add an `ArtifactLink` relation from the work item to the PR.
    LinkWorkItem {
        pr: PrRef,
        work_item: i64,
    },
    /// FR-17: remove that relation. Two calls: [`Self::request`] reads the
    /// work item's relations, [`unlink_request`] removes the PR's by index,
    /// guarded by the revision just read.
    UnlinkWorkItem {
        pr: PrRef,
        work_item: i64,
    },
}

impl ReviewWrite {
    pub fn pr(&self) -> &PrRef {
        match self {
            Self::CreateThread { pr, .. }
            | Self::Reply { pr, .. }
            | Self::SetThreadStatus { pr, .. }
            | Self::Vote { pr, .. }
            | Self::Complete { pr, .. }
            | Self::SetAutoComplete { pr, .. }
            | Self::CancelAutoComplete { pr }
            | Self::SetDraft { pr, .. }
            | Self::Abandon { pr }
            | Self::Reactivate { pr }
            | Self::LinkWorkItem { pr, .. }
            | Self::UnlinkWorkItem { pr, .. } => pr,
        }
    }

    /// Changes the PR's status or draft flag (FR-16).
    fn is_status(&self) -> bool {
        matches!(
            self,
            Self::SetDraft { .. } | Self::Abandon { .. } | Self::Reactivate { .. }
        )
    }

    /// Edits a work item, not the PR (FR-17).
    fn is_work_item(&self) -> bool {
        matches!(
            self,
            Self::LinkWorkItem { .. } | Self::UnlinkWorkItem { .. }
        )
    }

    /// The user action this write is, for the network log's origin
    /// (spec 0016 FR-18).
    pub fn action(&self) -> UserAction {
        match self {
            Self::CreateThread { .. } => UserAction::Comment,
            Self::Reply { .. } => UserAction::Reply,
            Self::SetThreadStatus { .. } => UserAction::Resolve,
            Self::Vote { .. } => UserAction::Vote,
            Self::Complete { .. } => UserAction::Complete,
            Self::SetAutoComplete { .. } | Self::CancelAutoComplete { .. } => {
                UserAction::AutoComplete
            }
            Self::SetDraft { .. } | Self::Abandon { .. } | Self::Reactivate { .. } => {
                UserAction::StatusChange
            }
            Self::LinkWorkItem { .. } => UserAction::LinkWorkItem,
            Self::UnlinkWorkItem { .. } => UserAction::UnlinkWorkItem,
        }
    }

    /// The REST call, `api-version=7.1` (spec 0004 §5).
    pub fn request(&self, org_url: &Url) -> ApiRequest {
        let url = |path: String| {
            let mut url = org_url.join(&path).expect("relative ADO path");
            url.query_pairs_mut()
                .append_pair("api-version", API_VERSION);
            url
        };
        match self {
            Self::CreateThread {
                pr,
                anchor,
                iteration,
                body,
            } => {
                let mut thread = json!({
                    "comments": [{
                        "parentCommentId": 0,
                        "content": body,
                        "commentType": "text",
                    }],
                    "status": "active",
                });
                if let Some(anchor) = anchor {
                    let pos = json!({ "line": anchor.line, "offset": 1 });
                    let (start, end) = match anchor.side {
                        Side::Right => ("rightFileStart", "rightFileEnd"),
                        Side::Left => ("leftFileStart", "leftFileEnd"),
                    };
                    thread["threadContext"] = json!({
                        "filePath": anchor.file_path(),
                        start: pos,
                        end: pos,
                    });
                    // The displayed diff is the latest iteration against the
                    // merge base; the web UI anchors such comments the same
                    // way (`secondComparingIteration` = the iteration seen)
                    // and adds the file's `changeTrackingId`: without it
                    // Azure DevOps never tracks the thread across pushes.
                    let mut context = json!({
                        "iterationContext": {
                            "firstComparingIteration": 1,
                            "secondComparingIteration": iteration,
                        },
                    });
                    if let Some(id) = anchor.change_tracking_id {
                        context["changeTrackingId"] = json!(id);
                    }
                    thread["pullRequestThreadContext"] = context;
                }
                ApiRequest::post(url(pr.path("/threads")), thread)
            }
            Self::Reply {
                pr,
                thread_id,
                parent_comment_id,
                body,
            } => ApiRequest::post(
                url(pr.path(&format!("/threads/{thread_id}/comments"))),
                json!({
                    "parentCommentId": parent_comment_id,
                    "content": body,
                    "commentType": "text",
                }),
            ),
            Self::SetThreadStatus {
                pr,
                thread_id,
                status,
            } => ApiRequest::patch(
                url(pr.path(&format!("/threads/{thread_id}"))),
                json!({ "status": status.as_str() }),
            ),
            Self::Vote { pr, reviewer, vote } => ApiRequest::put(
                url(pr.path(&format!("/reviewers/{reviewer}"))),
                json!({ "vote": vote.as_i8() }),
            ),
            Self::Complete {
                pr,
                source_commit,
                options,
            } => ApiRequest::patch(
                url(pr_update_path(pr)),
                json!({
                    "status": "completed",
                    "lastMergeSourceCommit": { "commitId": source_commit },
                    "completionOptions": options.json(),
                }),
            ),
            Self::SetAutoComplete {
                pr, me, options, ..
            } => ApiRequest::patch(
                url(pr_update_path(pr)),
                json!({
                    "autoCompleteSetBy": { "id": me },
                    "completionOptions": options.json(),
                }),
            ),
            Self::CancelAutoComplete { pr } => ApiRequest::patch(
                url(pr_update_path(pr)),
                json!({ "autoCompleteSetBy": { "id": NO_IDENTITY } }),
            ),
            Self::SetDraft { pr, draft } => {
                ApiRequest::patch(url(pr_update_path(pr)), json!({ "isDraft": draft }))
            }
            Self::Abandon { pr } => {
                ApiRequest::patch(url(pr_update_path(pr)), json!({ "status": "abandoned" }))
            }
            Self::Reactivate { pr } => {
                ApiRequest::patch(url(pr_update_path(pr)), json!({ "status": "active" }))
            }
            Self::LinkWorkItem { pr, work_item } => ApiRequest::json_patch(
                url(work_item_path(*work_item)),
                json!([{
                    "op": "add",
                    "path": "/relations/-",
                    "value": {
                        "rel": "ArtifactLink",
                        "url": pr.artifact_url(),
                        "attributes": { "name": "Pull Request" },
                    },
                }]),
            ),
            Self::UnlinkWorkItem { work_item, .. } => {
                let mut url = url(work_item_path(*work_item));
                url.query_pairs_mut().append_pair("$expand", "relations");
                ApiRequest::get(url)
            }
        }
    }
}

/// Unlink's second call: remove relation `index` of `work_item`, only if the
/// work item is still at `rev` (the revision the index was read from), so a
/// concurrent edit can't shift the index onto another link.
pub fn unlink_request(org_url: &Url, work_item: i64, index: usize, rev: i64) -> ApiRequest {
    let mut url = org_url
        .join(&work_item_path(work_item))
        .expect("relative ADO path");
    url.query_pairs_mut()
        .append_pair("api-version", API_VERSION);
    ApiRequest::json_patch(
        url,
        json!([
            { "op": "test", "path": "/rev", "value": rev },
            { "op": "remove", "path": format!("/relations/{index}") },
        ]),
    )
}

/// Index of `pr`'s `ArtifactLink` among `item`'s relations.
pub fn pr_relation_index(item: &WireWorkItem, pr: &PrRef) -> Option<usize> {
    item.relations
        .iter()
        .flatten()
        .position(|r| r.rel == "ArtifactLink" && wire::pr_link_id(&r.url) == Some(pr.pr_id))
}

/// `PATCH …/pullrequests/{id}` (the update endpoint's own casing).
fn pr_update_path(pr: &PrRef) -> String {
    format!(
        "{}/_apis/git/repositories/{}/pullrequests/{}",
        pr.project_id, pr.repo_id, pr.pr_id
    )
}

/// Work items are org-wide: no project segment needed.
fn work_item_path(id: i64) -> String {
    format!("_apis/wit/workitems/{id}")
}

/// Why a write didn't happen; `Display` is the reason the UI shows.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    /// 403: the token can read but not write pull requests.
    #[error("This token can't vote or comment. Review actions need Code (Read & write).")]
    MissingScope,
    /// 403 on a status change (FR-16): the token writes, but this account
    /// may not change this PR's status (Azure DevOps' reason attached).
    #[error(
        "Azure DevOps won't let you change this pull request's status. It takes the Contribute to pull requests permission on the repository.{}",
        said(message)
    )]
    NotAllowed { message: Option<String> },
    /// The token lacks `vso.work_write` (known from its scopes, or a 403 on
    /// a link, FR-17).
    #[error("Linking work items needs Work Items (Read & write).{}", said(message))]
    MissingWorkScope { message: Option<String> },
    /// 409 on Complete: the source branch moved past the displayed commit
    /// (spec 0004 AC-10; Azure DevOps answers `TF401192`,
    /// `GitPullRequestStaleException`). Never retried automatically.
    #[error("New commits since you reviewed")]
    NewCommits,
    #[error("You're offline. Nothing was sent.")]
    Offline,
    /// Signed in, but sync hasn't built its connection yet (token still
    /// being read).
    #[error("Not connected to Azure DevOps yet. Nothing was sent.")]
    NotConnected,
    #[error("Azure DevOps is throttling requests. Try again in a moment.")]
    Throttled { until: Millis },
    /// The main transport latched the token as rejected (spec 0001 FR-15).
    #[error("Azure DevOps rejected adoken's token. Replace it in Settings › Account.")]
    Unauthorized,
    #[error("Azure DevOps refused (HTTP {status}){}", message.as_deref().map(|m| format!(": {m}")).unwrap_or_default())]
    Rejected {
        status: u16,
        message: Option<String>,
    },
    /// Local store or response decoding failure.
    #[error("Couldn't save the change locally: {0}")]
    Local(String),
}

fn said(message: &Option<String>) -> String {
    message
        .as_deref()
        .map(|m| format!(" Azure DevOps said: {m}"))
        .unwrap_or_default()
}

impl WriteError {
    fn from_gov(error: GovError, write: &ReviewWrite) -> Self {
        match error {
            // A PAT without `vso.work_write` gets 401 (through the optional
            // transport, so the token stays usable); 403 is a permission.
            GovError::Status {
                status: 401 | 403,
                message,
                ..
            } if write.is_work_item() => Self::MissingWorkScope { message },
            GovError::Status {
                status: 403,
                message,
                ..
            } if write.is_status() => Self::NotAllowed { message },
            GovError::Status { status: 403, .. } => Self::MissingScope,
            GovError::Status { status: 409, .. }
                if matches!(write, ReviewWrite::Complete { .. }) =>
            {
                Self::NewCommits
            }
            other => Self::from_read(other),
        }
    }

    /// A read's failure: any refusal carries Azure DevOps' own message.
    fn from_read(error: GovError) -> Self {
        match error {
            GovError::Unauthorized => Self::Unauthorized,
            GovError::Offline(_) => Self::Offline,
            GovError::Throttled { until } => Self::Throttled { until },
            // User-tier requests are never deferred; treat it as throttling.
            GovError::Deferred => Self::Throttled { until: 0 },
            GovError::Status {
                status, message, ..
            } => Self::Rejected { status, message },
        }
    }

    /// The §5.2 scope Azure DevOps refused this write for (spec 0001
    /// FR-17), if that's why it failed.
    pub fn missing_scope(&self) -> Option<&'static str> {
        match self {
            Self::MissingScope => Some(WRITE_SCOPE),
            Self::MissingWorkScope { .. } => Some(WORK_WRITE_SCOPE),
            _ => None,
        }
    }
}

impl From<adoken_store::Error> for WriteError {
    fn from(error: adoken_store::Error) -> Self {
        Self::Local(error.to_string())
    }
}

/// The store's view of what a successful write changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    /// A new thread, a reply, or a status change: the thread as stored.
    Thread(PrThread),
    /// Vote, complete, auto-complete, PR status: the PR as stored.
    Pr(PullRequest),
    /// A link or unlink: the work item as stored (`None` when its project
    /// isn't synced, so the store doesn't keep it).
    WorkItem(Option<WorkItem>),
}

/// `…/pullRequests/{id}/workitems` entries.
#[derive(Deserialize)]
struct ResourceRef {
    id: String,
}

/// Sends [`ReviewWrite`]s for one org. Cheap to clone; `send` needs a Tokio
/// runtime (store work runs on its blocking pool).
#[derive(Clone)]
pub struct Writer {
    store: Store,
    transport: Arc<dyn Transport>,
    /// Work item calls (FR-17). Azure DevOps answers **401**, not 403, when
    /// a PAT lacks `vso.work_write` (seen live 2026-09-30); through the
    /// main transport that would mark the whole token rejected. This one
    /// ([`crate::transport::OptionalAdoTransport`] in the app) hands the
    /// 401 back as a status, reported as [`WriteError::MissingWorkScope`].
    work_items: Arc<dyn Transport>,
    gov: Arc<Governor>,
    clock: Arc<dyn Clock>,
    org_url: Url,
    /// Sync activity: a write's re-read is labeled with the write
    /// (spec 0016 FR-18), per-item syncs are passes (FR-11).
    tracker: Arc<Tracker>,
}

impl Writer {
    pub fn new(
        store: Store,
        transport: Arc<dyn Transport>,
        gov: Arc<Governor>,
        clock: Arc<dyn Clock>,
        org_url: Url,
    ) -> Self {
        let mut org_url = org_url;
        if !org_url.path().ends_with('/') {
            org_url.set_path(&format!("{}/", org_url.path()));
        }
        Writer {
            store,
            work_items: transport.clone(),
            transport,
            gov,
            tracker: Arc::new(Tracker::new(clock.clone())),
            clock,
            org_url,
        }
    }

    /// Report into the session's sync activity (the engine's tracker).
    pub fn with_tracker(mut self, tracker: Arc<Tracker>) -> Self {
        self.tracker = tracker;
        self
    }

    pub fn tracker(&self) -> &Arc<Tracker> {
        &self.tracker
    }

    /// Send work item calls through `transport`, one whose 401 never
    /// rejects the credential (see [`Writer`]'s `work_items`).
    pub fn with_work_item_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.work_items = transport;
        self
    }

    /// Spec 0015's work item writes, per-item sync and metadata, through the
    /// same store, governor and work item transport.
    pub fn work_items(&self) -> crate::wi::WiWriter {
        crate::wi::WiWriter::new(
            self.store.clone(),
            self.work_items.clone(),
            self.gov.clone(),
            self.clock.clone(),
            self.org_url.clone(),
        )
        .with_tracker(self.tracker.clone())
    }

    /// Send now (bypassing tier scheduling, not the governor) and apply the
    /// response. Exactly one request (unlink: read, then remove); no
    /// automatic retry beyond the governor's handling of 429 /
    /// `Retry-After` (the request wasn't processed then).
    pub async fn send(&self, write: ReviewWrite) -> Result<Applied, WriteError> {
        let action = write.action();
        let result = attribution::or_origin(Origin::You(action), self.send_inner(write)).await;
        if result.is_ok() {
            self.tracker.note_write(action);
        }
        result
    }

    async fn send_inner(&self, write: ReviewWrite) -> Result<Applied, WriteError> {
        let request = match &write {
            ReviewWrite::UnlinkWorkItem { pr, work_item } => {
                let located = self.call(&write, write.request(&self.org_url)).await?;
                let item: WireWorkItem = decode(&located.body)?;
                match pr_relation_index(&item, pr) {
                    Some(index) => unlink_request(&self.org_url, *work_item, index, item.rev),
                    // Already unlinked (elsewhere): store what the server has.
                    None => return self.apply(write, located.body).await,
                }
            }
            _ => write.request(&self.org_url),
        };
        let response = self.call(&write, request).await?;
        self.apply(write, response.body).await
    }

    async fn call(
        &self,
        write: &ReviewWrite,
        request: ApiRequest,
    ) -> Result<ApiResponse, WriteError> {
        let sent = if write.is_work_item() {
            self.gov
                .send_optional(self.work_items.as_ref(), Tier::User, request)
                .await
        } else {
            self.gov
                .send(self.transport.as_ref(), Tier::User, request)
                .await
        };
        sent.map_err(|e| WriteError::from_gov(e, write))
    }

    async fn apply(&self, write: ReviewWrite, body: Vec<u8>) -> Result<Applied, WriteError> {
        let now = self.clock.now_ms();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || apply_response(&store, &write, &body, now))
            .await
            .map_err(|e| WriteError::Local(e.to_string()))?
    }

    /// The work items linked to `pr` per Azure DevOps (`…/workitems`), in
    /// its order; those in synced projects are fetched with their relations
    /// and stored, so the store's links (and the review header) match the
    /// server (spec 0004 FR-17). Two reads at user priority (one when none
    /// are linked). No events: nothing here is news.
    pub async fn linked_work_items(&self, pr: &PrRef) -> Result<Vec<i64>, WriteError> {
        attribution::or_origin(
            Origin::You(UserAction::OpenPr),
            self.linked_work_items_inner(pr),
        )
        .await
    }

    async fn linked_work_items_inner(&self, pr: &PrRef) -> Result<Vec<i64>, WriteError> {
        let url = |path: &str, extra: &[(&str, &str)]| {
            let mut url = self.org_url.join(path).expect("relative ADO path");
            url.query_pairs_mut()
                .extend_pairs(extra)
                .append_pair("api-version", API_VERSION);
            url
        };
        // Optional: `vso.work` is an optional scope, and a 401 here must not
        // reject the token (see [`Writer`]'s `work_items`).
        let read = |request| async move {
            self.gov
                .send_optional(self.work_items.as_ref(), Tier::User, request)
                .await
                .map_err(WriteError::from_read)
        };
        let refs = read(ApiRequest::get(url(&pr.path("/workitems"), &[]))).await?;
        let refs: List<ResourceRef> = decode(&refs.body)?;
        let ids: Vec<i64> = refs
            .value
            .iter()
            .filter_map(|r| r.id.parse().ok())
            .collect();
        if ids.is_empty() {
            return Ok(ids);
        }
        let joined = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let items = read(ApiRequest::get(url(
            "_apis/wit/workitems",
            &[
                ("ids", &joined),
                ("$expand", "relations"),
                ("errorPolicy", "omit"),
            ],
        )))
        .await?;
        // `errorPolicy=omit` answers `null` for items this token can't read.
        let items: List<Option<WireWorkItem>> = decode(&items.body)?;
        let items: Vec<WireWorkItem> = items.value.into_iter().flatten().collect();
        let now = self.clock.now_ms();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let me = my_id(&store)?;
            store
                .write(|tx| {
                    let mut em = Emitter {
                        tx,
                        me: &me,
                        enabled: false,
                        now,
                        new_ids: Vec::new(),
                    };
                    apply_work_items(&mut em, items).map(|_| ())
                })
                .map_err(WriteError::from)
        })
        .await
        .map_err(|e| WriteError::Local(e.to_string()))??;
        Ok(ids)
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, WriteError> {
    serde_json::from_slice(body).map_err(|e| WriteError::Local(format!("response: {e}")))
}

/// Server answer → store, with self-caused events, in one transaction.
fn apply_response(
    store: &Store,
    write: &ReviewWrite,
    body: &[u8],
    now: Millis,
) -> Result<Applied, WriteError> {
    let me = my_id(store)?;
    let pr_id = write.pr().pr_id;
    match write {
        ReviewWrite::CreateThread { .. } => {
            let wire: WireThread = decode(body)?;
            let people = wire.people();
            let thread = wire.into_model(pr_id);
            in_tx(store, &me, now, |em| {
                for p in &people {
                    write::upsert_person(em.tx, p, false)?;
                }
                record_thread(em, thread)
            })
        }
        ReviewWrite::Reply { thread_id, .. } => {
            let wire: WireComment = decode(body)?;
            let author = wire.author.person();
            let comment = wire.into_model();
            in_tx(store, &me, now, |em| {
                write::upsert_person(em.tx, &author, false)?;
                let mut thread = stored_thread(em.tx, pr_id, *thread_id)?;
                thread.updated = thread.updated.max(comment.updated);
                thread.comments.push(comment);
                record_thread(em, thread)
            })
        }
        ReviewWrite::SetThreadStatus {
            thread_id, status, ..
        } => {
            // The PATCH answer may omit comments: only the status is taken.
            let wire: WireThread = decode(body)?;
            let status = wire
                .status
                .as_deref()
                .and_then(ThreadStatus::parse)
                .unwrap_or(*status);
            in_tx(store, &me, now, |em| {
                let mut thread = stored_thread(em.tx, pr_id, *thread_id)?;
                thread.status = status;
                thread.updated = (thread.updated + 1).max(now);
                record_thread(em, thread)
            })
        }
        ReviewWrite::Vote { .. } => {
            let wire: WireReviewer = decode(body)?;
            let vote = Vote::from_i8(wire.vote);
            in_tx(store, &me, now, |em| {
                write::upsert_person(em.tx, &wire.identity.person(), false)?;
                record_vote(em, pr_id, vote)
            })
        }
        ReviewWrite::Complete { source_commit, .. } => {
            // Azure DevOps merges asynchronously: a 2xx usually answers
            // `status: active, mergeStatus: queued`. Stored as is (server
            // truth); the PR is "completing" until sync reads it closed or
            // the queued merge failed (spec 0004 FR-10).
            let wire: GitPullRequest = decode(body)?;
            in_tx(store, &me, now, |em| {
                let status = wire.status();
                if status == PrStatus::Completed {
                    emit_completed(em, pr_id)?;
                }
                apply_pr(em, &wire)?;
                if status == PrStatus::Active && !wire.merge_status().is_failure() {
                    write::set_completing(em.tx, pr_id, source_commit, now)?;
                }
                stored_pr(em.tx, pr_id).map(Applied::Pr)
            })
        }
        ReviewWrite::SetAutoComplete { source_commit, .. } => {
            let wire: GitPullRequest = decode(body)?;
            in_tx(store, &me, now, |em| {
                apply_pr(em, &wire)?;
                write::set_auto_complete(em.tx, pr_id, source_commit, now)?;
                stored_pr(em.tx, pr_id).map(Applied::Pr)
            })
        }
        ReviewWrite::CancelAutoComplete { .. } => {
            let wire: GitPullRequest = decode(body)?;
            in_tx(store, &me, now, |em| {
                apply_pr(em, &wire)?;
                write::clear_auto_complete(em.tx, pr_id)?;
                stored_pr(em.tx, pr_id).map(Applied::Pr)
            })
        }
        ReviewWrite::SetDraft { .. }
        | ReviewWrite::Abandon { .. }
        | ReviewWrite::Reactivate { .. } => {
            let wire: GitPullRequest = decode(body)?;
            in_tx(store, &me, now, |em| {
                emit_status(em, pr_id, wire.status(), wire.is_draft)?;
                apply_pr(em, &wire)?;
                stored_pr(em.tx, pr_id).map(Applied::Pr)
            })
        }
        ReviewWrite::LinkWorkItem { .. } | ReviewWrite::UnlinkWorkItem { .. } => {
            // Only relations changed: sync derives no event from that, so
            // none is emitted here either.
            let wire: WireWorkItem = decode(body)?;
            let id = wire.id;
            in_tx(store, &me, now, |em| {
                apply_work_items(em, vec![wire])?;
                Ok(Applied::WorkItem(read::work_item(em.tx, id)?))
            })
        }
    }
}

/// The demo org (and anything without a network): apply `write` to the
/// store as if Azure DevOps had accepted it. Blocking.
pub fn apply_local(store: &Store, write: &ReviewWrite, now: Millis) -> Result<Applied, WriteError> {
    let me = my_id(store)?;
    let pr_id = write.pr().pr_id;
    in_tx(store, &me, now, |em| match write {
        ReviewWrite::CreateThread {
            anchor,
            iteration,
            body,
            ..
        } => {
            let id = read::pr_threads(em.tx, pr_id)?
                .iter()
                .map(|t| t.id)
                .max()
                .unwrap_or(0)
                + 1;
            let thread = PrThread {
                id,
                pr_id,
                status: ThreadStatus::Active,
                anchor: anchor.as_ref().map(|a| ThreadAnchor {
                    file_path: a.file_path(),
                    right: (a.side == Side::Right).then_some(adoken_model::LineRange {
                        start: a.line,
                        end: a.line,
                    }),
                    left: (a.side == Side::Left).then_some(adoken_model::LineRange {
                        start: a.line,
                        end: a.line,
                    }),
                    iteration_blob: None,
                }),
                iteration: anchor.as_ref().map(|_| *iteration),
                comments: vec![local_comment(1, None, &me, body, now)],
                updated: now,
            };
            record_thread(em, thread)
        }
        ReviewWrite::Reply {
            thread_id,
            parent_comment_id,
            body,
            ..
        } => {
            let mut thread = stored_thread(em.tx, pr_id, *thread_id)?;
            let id = thread.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            thread
                .comments
                .push(local_comment(id, Some(*parent_comment_id), &me, body, now));
            thread.updated = thread.updated.max(now);
            record_thread(em, thread)
        }
        ReviewWrite::SetThreadStatus {
            thread_id, status, ..
        } => {
            let mut thread = stored_thread(em.tx, pr_id, *thread_id)?;
            thread.status = *status;
            thread.updated = (thread.updated + 1).max(now);
            record_thread(em, thread)
        }
        ReviewWrite::Vote { vote, .. } => record_vote(em, pr_id, *vote),
        ReviewWrite::Complete { .. } => {
            let mut pr = stored_pr(em.tx, pr_id)?;
            emit_completed(em, pr_id)?;
            pr.status = PrStatus::Completed;
            pr.merge_status = MergeStatus::Succeeded;
            pr.closed = Some(now);
            pr.updated = pr.updated.max(now);
            write::upsert_pr(em.tx, &pr)?;
            write::clear_auto_complete(em.tx, pr_id)?;
            stored_pr(em.tx, pr_id).map(Applied::Pr)
        }
        ReviewWrite::SetAutoComplete { source_commit, .. } => {
            write::set_auto_complete(em.tx, pr_id, source_commit, now)?;
            stored_pr(em.tx, pr_id).map(Applied::Pr)
        }
        ReviewWrite::CancelAutoComplete { .. } => {
            write::clear_auto_complete(em.tx, pr_id)?;
            stored_pr(em.tx, pr_id).map(Applied::Pr)
        }
        ReviewWrite::SetDraft { draft, .. } => {
            local_status(em, pr_id, PrStatus::Active, Some(*draft))
        }
        ReviewWrite::Abandon { .. } => local_status(em, pr_id, PrStatus::Abandoned, None),
        ReviewWrite::Reactivate { .. } => local_status(em, pr_id, PrStatus::Active, None),
        ReviewWrite::LinkWorkItem { work_item, .. } => local_link(em, *work_item, |links| {
            if !links.contains(&pr_id) {
                links.push(pr_id);
            }
        }),
        ReviewWrite::UnlinkWorkItem { work_item, .. } => {
            local_link(em, *work_item, |links| links.retain(|id| *id != pr_id))
        }
    })
}

/// Demo status change: `draft: None` keeps the flag.
fn local_status(
    em: &mut Emitter<'_>,
    pr_id: i64,
    status: PrStatus,
    draft: Option<bool>,
) -> Result<Applied, WriteError> {
    let mut pr = stored_pr(em.tx, pr_id)?;
    let draft = draft.unwrap_or(pr.is_draft);
    emit_status(em, pr_id, status, draft)?;
    let now = em.now;
    pr.status = status;
    pr.is_draft = draft;
    pr.closed = (status != PrStatus::Active).then_some(now);
    pr.updated = pr.updated.max(now);
    write::upsert_pr(em.tx, &pr)?;
    if status != PrStatus::Active {
        write::clear_auto_complete(em.tx, pr_id)?;
    }
    stored_pr(em.tx, pr_id).map(Applied::Pr)
}

/// Demo link change: a new revision of the stored work item by me.
fn local_link(
    em: &mut Emitter<'_>,
    id: i64,
    edit: impl FnOnce(&mut Vec<i64>),
) -> Result<Applied, WriteError> {
    let mut item = read::work_item(em.tx, id)?
        .ok_or_else(|| WriteError::Local(format!("#{id} isn't synced")))?;
    edit(&mut item.linked_pr_ids);
    item.rev += 1;
    item.changed = item.changed.max(em.now);
    item.changed_by = em.me.to_owned();
    write::upsert_work_item(em.tx, &item)?;
    Ok(Applied::WorkItem(read::work_item(em.tx, id)?))
}

fn my_id(store: &Store) -> Result<PersonId, WriteError> {
    store
        .me()?
        .map(|p| p.id)
        .ok_or_else(|| WriteError::Local("signed-in identity unknown".into()))
}

fn in_tx<R>(
    store: &Store,
    me: &str,
    now: Millis,
    f: impl FnOnce(&mut Emitter<'_>) -> Result<R, WriteError>,
) -> Result<R, WriteError> {
    let mut outcome = None;
    let result = store.write(|tx| {
        let mut em = Emitter {
            tx,
            me,
            enabled: true,
            now,
            new_ids: Vec::new(),
        };
        match f(&mut em) {
            Ok(r) => {
                outcome = Some(Ok(r));
                Ok(())
            }
            // Roll the transaction back, keep the typed error.
            Err(e) => {
                outcome = Some(Err(e));
                Err(adoken_store::Error::Corrupt("write rolled back".into()))
            }
        }
    });
    match (result, outcome) {
        (_, Some(Err(e))) => Err(e),
        (Ok(()), Some(Ok(r))) => Ok(r),
        (Err(e), _) => Err(e.into()),
        (Ok(()), None) => unreachable!("closure ran"),
    }
}

fn local_comment(id: i64, parent: Option<i64>, me: &str, body: &str, now: Millis) -> Comment {
    Comment {
        id,
        parent_id: parent,
        author: me.to_owned(),
        body: body.to_owned(),
        created: now,
        updated: now,
        is_system: false,
        is_deleted: false,
    }
}

fn stored_pr(tx: &Connection, pr_id: i64) -> Result<PullRequest, WriteError> {
    read::pr(tx, pr_id)?.ok_or_else(|| WriteError::Local(format!("!{pr_id} isn't synced")))
}

fn stored_thread(tx: &Connection, pr_id: i64, thread_id: i64) -> Result<PrThread, WriteError> {
    read::pr_threads(tx, pr_id)?
        .into_iter()
        .find(|t| t.id == thread_id)
        .ok_or_else(|| WriteError::Local(format!("thread {thread_id} isn't synced")))
}

/// Upsert `thread`; my new comments become `PrCommentAdded` and a status
/// change to resolved `PrThreadResolved`, both actor = me (the keys sync
/// would use, so its later pass dedupes against them).
fn record_thread(em: &mut Emitter<'_>, thread: PrThread) -> Result<Applied, WriteError> {
    let pr = stored_pr(em.tx, thread.pr_id)?;
    let before = read::pr_threads(em.tx, thread.pr_id)?
        .into_iter()
        .find(|t| t.id == thread.id);
    let new_ids = write::upsert_thread(em.tx, &thread)?;
    let me = em.me.to_owned();
    let subject = Subject::Pr(pr.id);
    let label = format!("!{} {}", pr.id, pr.title);
    for c in thread
        .comments
        .iter()
        .filter(|c| new_ids.contains(&c.id) && c.author == me)
    {
        em.emit(
            SyncEventKind::PrCommentAdded,
            subject.clone(),
            format!("{}:{}", thread.id, c.id),
            Some(&me),
            c.created,
            format!("You commented on {label}"),
        )?;
    }
    let was_resolved = before.as_ref().is_some_and(|t| t.status.is_resolved());
    if thread.status.is_resolved() && !was_resolved {
        em.emit(
            SyncEventKind::PrThreadResolved,
            subject,
            format!("{}:{}", thread.id, thread.status.as_str()),
            Some(&me),
            thread.updated,
            format!(
                "You marked a thread on !{} {}",
                pr.id,
                thread.status.as_str()
            ),
        )?;
    }
    if !thread.comments.is_empty() {
        write::touch_pr(em.tx, pr.id, thread.updated)?;
    }
    Ok(Applied::Thread(stored_thread(
        em.tx,
        thread.pr_id,
        thread.id,
    )?))
}

/// My vote on `pr_id` (added as a reviewer if I wasn't one), plus the
/// `PrVoteChanged` event sync keys as `person:vote:commit`.
fn record_vote(em: &mut Emitter<'_>, pr_id: i64, vote: Vote) -> Result<Applied, WriteError> {
    let mut pr = stored_pr(em.tx, pr_id)?;
    let me = em.me.to_owned();
    let now = em.now;
    match pr.reviewers.iter_mut().find(|r| r.person == me) {
        Some(r) if r.vote == vote => return Ok(Applied::Pr(pr)),
        Some(r) => {
            r.vote = vote;
            r.voted_at = (vote != Vote::NoVote).then_some(now);
        }
        None => pr.reviewers.push(Reviewer {
            person: me.clone(),
            vote,
            is_required: false,
            has_declined: false,
            added_at: Some(now),
            voted_at: (vote != Vote::NoVote).then_some(now),
        }),
    }
    pr.updated = pr.updated.max(now);
    write::upsert_pr(em.tx, &pr)?;
    if vote != Vote::NoVote {
        em.emit(
            SyncEventKind::PrVoteChanged,
            Subject::Pr(pr.id),
            format!(
                "{me}:{}:{}",
                vote.as_i8(),
                pr.last_source_commit.as_deref().unwrap_or("")
            ),
            Some(&me),
            now,
            format!("You voted “{}” on !{} {}", vote.label(), pr.id, pr.title),
        )?;
    }
    stored_pr(em.tx, pr_id).map(Applied::Pr)
}

/// `PrStatusChanged` "completed" by me, before the PR row changes, so a
/// later sync pass dedupes against it (Ken's merge moment).
fn emit_completed(em: &mut Emitter<'_>, pr_id: i64) -> Result<(), WriteError> {
    let pr = stored_pr(em.tx, pr_id)?;
    let me = em.me.to_owned();
    let who = name(em.tx, &me);
    em.emit(
        SyncEventKind::PrStatusChanged,
        Subject::Pr(pr_id),
        PrStatus::Completed.as_str().to_owned(),
        Some(&me),
        em.now,
        format!("{who} completed !{} {}", pr.id, pr.title),
    )?;
    Ok(())
}

/// `PrStatusChanged` by me when my write changes the PR's status or draft
/// flag, emitted before the row changes with the key sync derives
/// ([`status_key`]), so its later pass (and `apply_pr` right after) dedupe
/// against it. Nothing when the stored PR already reads that way.
fn emit_status(
    em: &mut Emitter<'_>,
    pr_id: i64,
    status: PrStatus,
    draft: bool,
) -> Result<(), WriteError> {
    let pr = stored_pr(em.tx, pr_id)?;
    if pr.status == status && pr.is_draft == draft {
        return Ok(());
    }
    let verb = match (pr.status, status, draft) {
        (PrStatus::Active, PrStatus::Active, true) => "marked as draft",
        (PrStatus::Active, PrStatus::Active, false) => "published",
        (_, PrStatus::Active, _) => "reactivated",
        (_, PrStatus::Abandoned, _) => "abandoned",
        (_, PrStatus::Completed, _) => "completed",
    };
    let me = em.me.to_owned();
    let who = name(em.tx, &me);
    em.emit(
        SyncEventKind::PrStatusChanged,
        Subject::Pr(pr_id),
        status_key(status, draft).to_owned(),
        Some(&me),
        em.now,
        format!("{who} {verb} !{} {}", pr.id, pr.title),
    )?;
    Ok(())
}

impl Applied {
    pub fn into_pr(self) -> Option<PullRequest> {
        match self {
            Applied::Pr(pr) => Some(pr),
            _ => None,
        }
    }

    pub fn into_thread(self) -> Option<PrThread> {
        match self {
            Applied::Thread(t) => Some(t),
            _ => None,
        }
    }

    /// A link write's work item (`None` inside: not kept by the store).
    pub fn into_work_item(self) -> Option<Option<WorkItem>> {
        match self {
            Applied::WorkItem(item) => Some(item),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr() -> PrRef {
        PrRef {
            project_id: "p1".into(),
            repo_id: "r1".into(),
            pr_id: 912,
        }
    }

    #[test]
    fn complete_conflicts_map_to_new_commits_only_for_complete() {
        let status = |status| GovError::Status {
            status,
            url: String::new(),
            message: Some("TF401192".into()),
        };
        let complete = ReviewWrite::Complete {
            pr: pr(),
            source_commit: "a".into(),
            options: CompletionOptions::default(),
        };
        let vote = ReviewWrite::Vote {
            pr: pr(),
            reviewer: "me".into(),
            vote: Vote::Approved,
        };
        assert_eq!(
            WriteError::from_gov(status(409), &complete),
            WriteError::NewCommits
        );
        // Only 409 means "stale"; any other 400 keeps its own message.
        assert_eq!(
            WriteError::from_gov(status(400), &complete),
            WriteError::Rejected {
                status: 400,
                message: Some("TF401192".into())
            }
        );
        assert_eq!(
            WriteError::from_gov(status(409), &vote),
            WriteError::Rejected {
                status: 409,
                message: Some("TF401192".into())
            }
        );
        assert_eq!(
            WriteError::from_gov(status(403), &vote),
            WriteError::MissingScope
        );
    }

    #[test]
    fn blank_merge_message_is_omitted() {
        let mut options = CompletionOptions {
            merge_commit_message: Some("  ".into()),
            ..CompletionOptions::default()
        };
        assert!(options.json().get("mergeCommitMessage").is_none());
        options.merge_commit_message = Some(" Merged !912 ".into());
        assert_eq!(options.json()["mergeCommitMessage"], "Merged !912");
    }
}
