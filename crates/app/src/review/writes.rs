//! Review writes in the app (spec 0004 FR-6..FR-12, spec 0002 FR-14): who
//! may write ([`Access`]), where a write goes (the org's [`Writer`] at user
//! priority, or the local store for the demo org), durable composer drafts
//! (FR-8, AC-13), which review controls exist (AC-11), and the optimistic
//! edits of a [`LoadedPr`] with their reverts.
//!
//! Everything here except [`Access::send`] is pure or blocking store IO (run
//! on a background executor).

use adoken_model::{
    MergeStatus, Millis, PrStatus, PrThread, PullRequest, ThreadStatus, Vote, WorkItem,
};
use adoken_store::Store;
use adoken_store::rusqlite::params;
use adoken_sync::Control as SyncControl;
use adoken_sync::writes::{
    Applied, LineAnchor, PrRef, ReviewWrite, Side, WriteError, Writer, apply_local,
};
use gpui::{App, Global, Task};
use gpui_tokio::Tokio;
use tokio::sync::{mpsc, watch};

use super::data::LoadedPr;

/// AC-11: the single explanation shown instead of every review control.
pub const READ_ONLY_HINT: &str = "Review actions need Code (Read & write)";

/// Who may change pull requests from this window.
#[derive(Clone)]
pub enum Access {
    /// The demo org: writes apply to the local store, never the network.
    Local,
    /// Signed in with Review access.
    Remote(Remote),
    /// A Read-only token (or no account): no review control exists.
    ReadOnly,
}

/// The signed-in org's write path.
#[derive(Clone)]
pub struct Remote {
    /// Appears once sync has read the token and built its engine.
    pub writer: watch::Receiver<Option<Writer>>,
    /// The sync scheduler: a landed write re-reads the PR in the hot tier
    /// within 5 s (spec 0002 FR-15).
    pub control: mpsc::UnboundedSender<SyncControl>,
    /// The token may link work items (`vso.work_write`, or its scopes are
    /// unknown: a refusal then explains itself, spec 0004 FR-17).
    pub link_work_items: bool,
}

impl Access {
    pub fn can_write(&self) -> bool {
        !matches!(self, Access::ReadOnly)
    }

    /// Linking work items to PRs is possible (FR-17): the demo org, or a
    /// token with `vso.work_write`.
    pub fn can_link_work_items(&self) -> bool {
        match self {
            Access::Local => true,
            Access::Remote(remote) => remote.link_work_items,
            Access::ReadOnly => false,
        }
    }

    /// The PR's linked work item ids per Azure DevOps, fetched and stored
    /// (FR-17); `None` for the demo org and read-only access (the store's
    /// links are all there is).
    pub fn linked_work_items(
        &self,
        pr: PrRef,
        cx: &App,
    ) -> Option<Task<Result<Vec<i64>, WriteError>>> {
        let Access::Remote(remote) = self else {
            return None;
        };
        let writer = remote.writer.borrow().clone()?;
        let job = Tokio::spawn(cx, async move { writer.linked_work_items(&pr).await });
        Some(cx.background_executor().spawn(async move {
            job.await
                .unwrap_or_else(|e| Err(WriteError::Local(e.to_string())))
        }))
    }

    /// Send `write` now: through the org's governor and ledger (remote), or
    /// applied to `store` (demo). Resolves off the UI thread; a remote
    /// answer is recorded against the write's scope (spec 0001 FR-17).
    pub fn send(
        &self,
        store: Option<Store>,
        write: ReviewWrite,
        now: Millis,
        cx: &App,
    ) -> Task<Result<Applied, WriteError>> {
        match self {
            Access::ReadOnly => Task::ready(Err(WriteError::MissingScope)),
            Access::Local => match store {
                Some(store) => cx
                    .background_executor()
                    .spawn(async move { apply_local(&store, &write, now) }),
                None => Task::ready(Err(WriteError::Local("no local store".into()))),
            },
            Access::Remote(remote) => {
                let Some(writer) = remote.writer.borrow().clone() else {
                    return Task::ready(Err(WriteError::NotConnected));
                };
                let control = remote.control.clone();
                let complete = matches!(write, ReviewWrite::Complete { .. });
                let scope = write_scope(&write);
                let job = Tokio::spawn(cx, async move {
                    let result = writer.send(write).await;
                    if let Ok(applied) = &result {
                        control.send(SyncControl::AfterWrite).ok();
                        // Spec 0004 FR-10: the merge is queued; re-read the
                        // PR shortly so the review flips to Completed.
                        if complete
                            && let Applied::Pr(pr) = applied
                            && pr.status == PrStatus::Active
                            && !pr.merge_status.is_failure()
                        {
                            control.send(SyncControl::Completing(pr.id)).ok();
                        }
                    }
                    result
                });
                cx.spawn(async move |cx| {
                    let result = job
                        .await
                        .unwrap_or_else(|e| Err(WriteError::Local(e.to_string())));
                    if let Some((scope, refused)) = scope_answer(scope, &result) {
                        cx.update(|cx| crate::account::record_write(scope, refused, cx));
                    }
                    result
                })
            }
        }
    }
}

/// The token scope `write` needs: linking edits the work item (FR-17),
/// everything else writes the pull request.
fn write_scope(write: &ReviewWrite) -> &'static str {
    match write {
        ReviewWrite::LinkWorkItem { .. } | ReviewWrite::UnlinkWorkItem { .. } => {
            adoken_sync::writes::WORK_WRITE_SCOPE
        }
        _ => adoken_sync::writes::WRITE_SCOPE,
    }
}

/// What a remote answer proves about the token (spec 0001 FR-17): an
/// accepted write proves `scope`; a scope refusal marks its scope missing;
/// other failures (offline, conflicts, permissions) say nothing.
fn scope_answer(
    scope: &'static str,
    result: &Result<Applied, WriteError>,
) -> Option<(&'static str, bool)> {
    match result {
        Ok(_) => Some((scope, false)),
        Err(error) => error.missing_scope().map(|scope| (scope, true)),
    }
}

/// The window's [`Access`], set by the shell when the store opens.
pub struct ReviewAccess(pub Access);

impl Global for ReviewAccess {}

/// The current access; `ReadOnly` until the shell sets one (stories that
/// want writable controls set [`Access::Local`]).
pub fn access(cx: &App) -> Access {
    cx.try_global::<ReviewAccess>()
        .map_or(Access::ReadOnly, |a| a.0.clone())
}

pub fn set_access(access: Access, cx: &mut App) {
    cx.set_global(ReviewAccess(access));
}

// ── controls (AC-11) ─────────────────────────────────────────────────────

/// A review control the header / thread cards may show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Vote,
    Comment,
    Reply,
    Resolve,
    Complete,
    CancelAutoComplete,
}

/// Why Complete is shown but unavailable. `completing`: Azure DevOps is
/// still merging my earlier Complete (spec 0004 FR-10).
pub fn complete_blocker(pr: &PullRequest, completing: bool) -> Option<&'static str> {
    if pr.status != PrStatus::Active {
        Some("This pull request is closed")
    } else if completing {
        Some("Azure DevOps is completing it")
    } else if pr.is_draft {
        Some("Publish the draft before completing")
    } else if pr.merge_status == MergeStatus::Conflicts {
        Some("Resolve the merge conflicts first")
    } else if pr.last_source_commit.is_none() {
        Some("The source commit isn't synced yet")
    } else {
        None
    }
}

/// Why the merge can't go through as is, from Azure DevOps' `mergeStatus`:
/// shown in the header; after a queued Complete failed, Complete is offered
/// again (conflicts still block it, [`complete_blocker`]).
pub fn merge_problem(pr: &PullRequest) -> Option<&'static str> {
    if pr.status != PrStatus::Active {
        return None;
    }
    match pr.merge_status {
        MergeStatus::Conflicts => Some("Merge conflicts"),
        MergeStatus::RejectedByPolicy => Some("Merge rejected by policy"),
        MergeStatus::Failure => Some("Merge failed"),
        MergeStatus::NotSet | MergeStatus::Queued | MergeStatus::Succeeded => None,
    }
}

/// The review controls that exist for this PR. Read-only access has none:
/// nothing to focus, one hint instead (AC-11).
pub fn controls(read_only: bool, pr: &PullRequest, auto_complete_mine: bool) -> Vec<Control> {
    if read_only {
        return Vec::new();
    }
    let mut out = vec![Control::Comment, Control::Reply, Control::Resolve];
    if pr.status == PrStatus::Active {
        out.insert(0, Control::Vote);
        out.push(Control::Complete);
        if auto_complete_mine {
            out.push(Control::CancelAutoComplete);
        }
    }
    out
}

// ── status (FR-16) ───────────────────────────────────────────────────────

/// A status change the header's status menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusAction {
    Publish,
    MarkDraft,
    /// Asks for confirmation first.
    Abandon,
    Reactivate,
}

impl StatusAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Publish => "Publish",
            Self::MarkDraft => "Mark as draft",
            Self::Abandon => "Abandon…",
            Self::Reactivate => "Reactivate",
        }
    }

    /// What the menu row explains.
    pub fn detail(self) -> &'static str {
        match self {
            Self::Publish => "Reviewers are notified and policies run",
            Self::MarkDraft => "Back to work in progress; can't be completed",
            Self::Abandon => "Close without merging; it can be reactivated",
            Self::Reactivate => "Open it again for review",
        }
    }

    pub fn write(self, pr: PrRef) -> ReviewWrite {
        match self {
            Self::Publish => ReviewWrite::SetDraft { pr, draft: false },
            Self::MarkDraft => ReviewWrite::SetDraft { pr, draft: true },
            Self::Abandon => ReviewWrite::Abandon { pr },
            Self::Reactivate => ReviewWrite::Reactivate { pr },
        }
    }

    /// The header's outcome line once Azure DevOps accepted it.
    pub fn done(self, id: i64) -> String {
        match self {
            Self::Publish => format!("Published !{id}"),
            Self::MarkDraft => format!("!{id} is a draft again"),
            Self::Abandon => format!("Abandoned !{id}"),
            Self::Reactivate => format!("Reactivated !{id}"),
        }
    }
}

/// The status changes valid for `pr` now, in menu order. Anyone with write
/// access is offered them (Azure DevOps lets contributors, not only the
/// author, publish or abandon); a refusal explains itself. None while
/// Azure DevOps is merging my Complete, or once completed.
pub fn status_actions(read_only: bool, pr: &PullRequest, completing: bool) -> Vec<StatusAction> {
    if read_only || completing {
        return Vec::new();
    }
    match (pr.status, pr.is_draft) {
        (PrStatus::Active, true) => vec![StatusAction::Publish, StatusAction::Abandon],
        (PrStatus::Active, false) => vec![StatusAction::MarkDraft, StatusAction::Abandon],
        (PrStatus::Abandoned, _) => vec![StatusAction::Reactivate],
        _ => Vec::new(),
    }
}

/// `⇧P`: publish a draft, else mark the PR as a draft.
pub fn draft_toggle(pr: &PullRequest) -> StatusAction {
    if pr.is_draft {
        StatusAction::Publish
    } else {
        StatusAction::MarkDraft
    }
}

/// Why `action` isn't offered for `pr` now (the header says so instead of
/// sending it); `None` when it is.
pub fn status_refusal(
    read_only: bool,
    pr: &PullRequest,
    completing: bool,
    action: StatusAction,
) -> Option<String> {
    if status_actions(read_only, pr, completing).contains(&action) {
        return None;
    }
    let id = pr.id;
    Some(if read_only {
        READ_ONLY_HINT.to_owned()
    } else if completing {
        format!("Azure DevOps is completing !{id}")
    } else if pr.status == PrStatus::Completed {
        format!("!{id} is completed")
    } else {
        match action {
            StatusAction::Publish | StatusAction::MarkDraft if pr.status == PrStatus::Abandoned => {
                format!("!{id} is abandoned")
            }
            StatusAction::Publish => format!("!{id} isn't a draft"),
            StatusAction::MarkDraft => format!("!{id} is already a draft"),
            StatusAction::Abandon => format!("!{id} is already abandoned"),
            StatusAction::Reactivate => format!("!{id} isn't abandoned"),
        }
    })
}

/// What a status write restores on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusBefore {
    status: PrStatus,
    draft: bool,
    closed: Option<Millis>,
}

/// Show `action`'s outcome now (the review header and the list row repaint
/// on the next frame); returns what [`revert_status`] restores. The one
/// optimistic step of every status change, from the review or the list
/// (spec 0019 FR-21).
pub fn apply_status_action(
    pr: &mut PullRequest,
    action: StatusAction,
    now: Millis,
) -> StatusBefore {
    let before = StatusBefore {
        status: pr.status,
        draft: pr.is_draft,
        closed: pr.closed,
    };
    match action {
        StatusAction::Publish => pr.is_draft = false,
        StatusAction::MarkDraft => pr.is_draft = true,
        StatusAction::Abandon => {
            pr.status = PrStatus::Abandoned;
            pr.closed = Some(now);
        }
        StatusAction::Reactivate => {
            pr.status = PrStatus::Active;
            pr.closed = None;
        }
    }
    before
}

pub fn revert_status(pr: &mut PullRequest, before: StatusBefore) {
    pr.status = before.status;
    pr.is_draft = before.draft;
    pr.closed = before.closed;
}

/// The list's toast when Azure DevOps refuses a status change (spec 0019
/// FR-21).
pub fn status_refused(id: i64, reason: &str) -> String {
    format!("Status of !{id} not changed. {reason}")
}

// ── linked work items (FR-17) ────────────────────────────────────────────

/// One row of the header's Work items section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedItem {
    pub id: i64,
    /// `None`: linked in Azure DevOps, but not synced (another project).
    pub item: Option<WorkItem>,
}

/// The section's rows: the store's linked items plus ids Azure DevOps
/// reports that the store doesn't have, by id. `server`: the PR's links per
/// Azure DevOps when fetched (it wins: a stored link it no longer lists is
/// dropped).
pub fn linked_items(stored: Vec<WorkItem>, server: Option<&[i64]>) -> Vec<LinkedItem> {
    let mut rows: Vec<LinkedItem> = stored
        .into_iter()
        .filter(|w| server.is_none_or(|ids| ids.contains(&w.id)))
        .map(|w| LinkedItem {
            id: w.id,
            item: Some(w),
        })
        .collect();
    for id in server.unwrap_or_default() {
        if !rows.iter().any(|r| r.id == *id) {
            rows.push(LinkedItem {
                id: *id,
                item: None,
            });
        }
    }
    rows.sort_by_key(|r| r.id);
    rows
}

/// Why Link work item… can't be used: one passive explanation naming the
/// permission, no token action (spec 0001 FR-9, FR-14).
pub const LINK_SCOPE_HINT: &str = "Linking work items needs Work Items (Read & write).";

// ── drafts (FR-8, AC-13) ─────────────────────────────────────────────────

/// What a composer comments on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftTarget {
    /// A line of the PR's diff: repo path, side, 1-based line on that side.
    Line {
        path: String,
        side: Side,
        line: u32,
    },
    Reply {
        thread: i64,
    },
    /// A PR-level comment.
    Pr,
}

impl DraftTarget {
    fn key(&self) -> String {
        match self {
            DraftTarget::Line { path, side, line } => {
                let side = match side {
                    Side::Left => "L",
                    Side::Right => "R",
                };
                format!("line:{side}:{line}:{path}")
            }
            DraftTarget::Reply { thread } => format!("reply:{thread}"),
            DraftTarget::Pr => "pr".to_owned(),
        }
    }

    fn parse(key: &str) -> Option<Self> {
        if key == "pr" {
            return Some(DraftTarget::Pr);
        }
        if let Some(thread) = key.strip_prefix("reply:") {
            return thread
                .parse()
                .ok()
                .map(|thread| DraftTarget::Reply { thread });
        }
        let mut parts = key.strip_prefix("line:")?.splitn(3, ':');
        let side = match parts.next()? {
            "L" => Side::Left,
            "R" => Side::Right,
            _ => return None,
        };
        let line = parts.next()?.parse().ok()?;
        let path = parts.next()?.to_owned();
        Some(DraftTarget::Line { path, side, line })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub target: DraftTarget,
    pub body: String,
    pub updated: Millis,
}

/// `review` namespace step 2 (step 1 is viewed + layout, `data.rs`).
pub const DRAFTS_MIGRATION: &str = "
CREATE TABLE review_draft (
  pr_id INTEGER NOT NULL, target TEXT NOT NULL, body TEXT NOT NULL, updated_ms INTEGER NOT NULL,
  PRIMARY KEY (pr_id, target)
) STRICT, WITHOUT ROWID;
";

/// Unsent composer text for `pr_id`, newest first.
pub fn drafts(store: &Store, pr_id: i64) -> adoken_store::Result<Vec<Draft>> {
    store.with_conn(|c| {
        let mut stmt = c.prepare_cached(
            "SELECT target, body, updated_ms FROM review_draft WHERE pr_id = ?1
             ORDER BY updated_ms DESC",
        )?;
        let rows = stmt.query_map([pr_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (key, body, updated) = row?;
            if let Some(target) = DraftTarget::parse(&key) {
                out.push(Draft {
                    target,
                    body,
                    updated,
                });
            }
        }
        Ok(out)
    })
}

/// Save (or, when `body` is blank, forget) the draft for `target`.
pub fn save_draft(
    store: &Store,
    pr_id: i64,
    target: &DraftTarget,
    body: &str,
    now: Millis,
) -> adoken_store::Result<()> {
    store.write(|tx| {
        if body.trim().is_empty() {
            tx.execute(
                "DELETE FROM review_draft WHERE pr_id = ?1 AND target = ?2",
                params![pr_id, target.key()],
            )?;
        } else {
            tx.execute(
                "INSERT INTO review_draft(pr_id, target, body, updated_ms) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(pr_id, target) DO UPDATE SET body = excluded.body,
                   updated_ms = excluded.updated_ms",
                params![pr_id, target.key(), body, now],
            )?;
        }
        Ok(())
    })
}

pub fn delete_draft(store: &Store, pr_id: i64, target: &DraftTarget) -> adoken_store::Result<()> {
    save_draft(store, pr_id, target, "", 0)
}

/// What ⌘↵ in a composer does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    /// Nothing to post.
    Empty,
    /// Offline: keep the draft, send nothing (AC-13).
    KeepDraft,
    Send,
}

/// The only path to posting a comment: the user's ⌘↵. Offline it keeps the
/// draft; reconnecting never sends on its own.
pub fn submit(offline: bool, body: &str) -> Submit {
    if body.trim().is_empty() {
        Submit::Empty
    } else if offline {
        Submit::KeepDraft
    } else {
        Submit::Send
    }
}

// ── optimistic edits ─────────────────────────────────────────────────────

/// My vote shown now (FR-9, AC-9: the header repaints on the next frame).
/// Returns the vote to restore if the write fails.
pub fn apply_vote(loaded: &mut LoadedPr, vote: Vote, now: Millis) -> Option<Vote> {
    let me = loaded.me.clone();
    let pr = &mut loaded.detail.pr;
    match pr.reviewers.iter_mut().find(|r| r.person == me) {
        Some(r) => {
            let before = r.vote;
            r.vote = vote;
            r.voted_at = (vote != Vote::NoVote).then_some(now);
            Some(before)
        }
        None => {
            pr.reviewers.push(adoken_model::Reviewer {
                person: me,
                vote,
                is_required: false,
                has_declined: false,
                added_at: Some(now),
                voted_at: (vote != Vote::NoVote).then_some(now),
            });
            None
        }
    }
}

/// Undo [`apply_vote`]: `before` is its return value (`None` = I wasn't a
/// reviewer).
pub fn revert_vote(loaded: &mut LoadedPr, before: Option<Vote>) {
    let me = loaded.me.clone();
    let reviewers = &mut loaded.detail.pr.reviewers;
    match before {
        Some(vote) => {
            if let Some(r) = reviewers.iter_mut().find(|r| r.person == me) {
                r.vote = vote;
            }
        }
        None => reviewers.retain(|r| r.person != me),
    }
}

/// Index into [`LoadedPr::threads`] of thread `id`.
pub fn thread_index(loaded: &LoadedPr, id: i64) -> Option<usize> {
    loaded.threads.iter().position(|t| t.id == id)
}

/// Set a thread's status now; returns the status to restore on failure.
pub fn apply_status(loaded: &mut LoadedPr, thread: usize, status: ThreadStatus) -> ThreadStatus {
    let threads = std::sync::Arc::make_mut(&mut loaded.threads);
    std::mem::replace(&mut threads[thread].status, status)
}

/// Replace a thread with the server's copy (server wins).
pub fn replace_thread(loaded: &mut LoadedPr, thread: usize, server: PrThread) {
    std::sync::Arc::make_mut(&mut loaded.threads)[thread] = server;
}

/// A thread the server created on display line `line` of `file`: rendered
/// right after that line.
pub fn insert_thread(loaded: &mut LoadedPr, file: usize, line: usize, thread: PrThread) -> usize {
    let threads = std::sync::Arc::make_mut(&mut loaded.threads);
    threads.push(thread);
    let ix = threads.len() - 1;
    let entry = &mut loaded.files[file];
    entry.thread_count += 1;
    let at = entry.anchored.inline.partition_point(|(l, _)| *l <= line);
    entry.anchored.inline.insert(at, (line, ix));
    ix
}

/// The resolve toggle's next status: resolved threads reactivate, the rest
/// resolve as `fixed` ("Resolved" in the web UI).
pub fn toggled_status(status: ThreadStatus) -> ThreadStatus {
    if status.is_resolved() {
        ThreadStatus::Active
    } else {
        ThreadStatus::Fixed
    }
}

/// FR-6 / AC-7: the anchor of a line comment on the displayed diff, with
/// that file's `changeTrackingId` when known (omitted otherwise) so Azure
/// DevOps tracks the thread across later pushes.
pub fn line_anchor(loaded: &LoadedPr, path: &str, side: Side, line: u32) -> LineAnchor {
    LineAnchor {
        path: path.to_owned(),
        side,
        line,
        change_tracking_id: loaded
            .file_at(path)
            .and_then(|f| f.change.change_tracking_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::data;
    use adoken_model::{MergeStatus, PrDetail, Reviewer};
    use std::collections::HashSet;

    fn pr() -> PullRequest {
        PullRequest {
            id: 912,
            repo_id: "r".into(),
            project_id: "p".into(),
            title: "Fix".into(),
            description: String::new(),
            author: "bo".into(),
            reviewers: vec![Reviewer {
                person: "me".into(),
                vote: Vote::NoVote,
                is_required: true,
                has_declined: false,
                added_at: None,
                voted_at: None,
            }],
            status: PrStatus::Active,
            is_draft: false,
            merge_status: MergeStatus::Succeeded,
            source_branch: "refs/heads/f".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: Some("aaaa".into()),
            created: 0,
            updated: 0,
            closed: None,
            iterations: 1,
            last_push: None,
        }
    }

    fn loaded(pr: PullRequest) -> LoadedPr {
        let blob = |_: &str| None;
        data::build(
            data::Source {
                detail: PrDetail {
                    pr,
                    iterations: Vec::new(),
                    files: Vec::new(),
                    threads: Vec::new(),
                },
                people: Vec::new(),
                me: "me".into(),
                now: 0,
                viewed: HashSet::new(),
                read_only: false,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            0,
        )
    }

    #[test]
    fn ac11_read_only_has_no_review_controls() {
        assert!(controls(true, &pr(), true).is_empty());
        assert_eq!(
            controls(false, &pr(), false),
            [
                Control::Vote,
                Control::Comment,
                Control::Reply,
                Control::Resolve,
                Control::Complete
            ]
        );
        let mut done = pr();
        done.status = PrStatus::Completed;
        assert!(!controls(false, &done, true).contains(&Control::Vote));
        assert!(controls(false, &pr(), true).contains(&Control::CancelAutoComplete));
    }

    #[test]
    fn complete_explains_why_it_is_unavailable() {
        assert_eq!(complete_blocker(&pr(), false), None);
        // Spec 0004 FR-10: my queued Complete blocks another one.
        assert_eq!(
            complete_blocker(&pr(), true),
            Some("Azure DevOps is completing it")
        );
        let mut conflicts = pr();
        conflicts.merge_status = MergeStatus::Conflicts;
        assert_eq!(
            complete_blocker(&conflicts, false),
            Some("Resolve the merge conflicts first")
        );
        let mut draft = pr();
        draft.is_draft = true;
        assert!(complete_blocker(&draft, false).is_some());
    }

    #[test]
    fn completing_ends_when_the_merge_lands_or_fails() {
        let mut l = loaded(pr());
        l.detail.pr.merge_status = MergeStatus::Queued;
        assert!(!l.is_completing(), "queued by someone else");
        l.completing = Some(5);
        assert!(l.is_completing());
        assert!(controls(false, l.pr(), false).contains(&Control::Complete));
        assert!(complete_blocker(l.pr(), l.is_completing()).is_some());
        assert_eq!(merge_problem(l.pr()), None);

        // The queued merge failed: say why, offer Complete again.
        l.detail.pr.merge_status = MergeStatus::RejectedByPolicy;
        assert!(!l.is_completing());
        assert_eq!(merge_problem(l.pr()), Some("Merge rejected by policy"));
        assert_eq!(complete_blocker(l.pr(), l.is_completing()), None);
        l.detail.pr.merge_status = MergeStatus::Failure;
        assert_eq!(merge_problem(l.pr()), Some("Merge failed"));

        // Landed.
        l.detail.pr.status = PrStatus::Completed;
        l.detail.pr.merge_status = MergeStatus::Succeeded;
        assert!(!l.is_completing());
        assert_eq!(merge_problem(l.pr()), None);
    }

    #[test]
    fn ac7_line_comment_carries_the_files_change_tracking_id() {
        use adoken_model::{ChangeType, PrFileChange};
        let blob = |_: &str| None;
        let file = |path: &str, tracking| PrFileChange {
            path: path.into(),
            original_path: None,
            change_type: ChangeType::Edit,
            old_content: None,
            new_content: None,
            change_tracking_id: tracking,
        };
        let l = data::build(
            data::Source {
                detail: PrDetail {
                    pr: pr(),
                    iterations: Vec::new(),
                    files: vec![file("/src/a.rs", Some(4)), file("/src/b.rs", None)],
                    threads: Vec::new(),
                },
                people: Vec::new(),
                me: "me".into(),
                now: 0,
                viewed: HashSet::new(),
                read_only: false,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            0,
        );
        // Draft targets hold the tree's normalized path.
        let anchor = line_anchor(&l, "src/a.rs", Side::Right, 12);
        assert_eq!(
            anchor,
            LineAnchor {
                path: "src/a.rs".into(),
                side: Side::Right,
                line: 12,
                change_tracking_id: Some(4),
            }
        );
        assert_eq!(
            line_anchor(&l, "/src/b.rs", Side::Left, 3).change_tracking_id,
            None
        );
        assert_eq!(
            line_anchor(&l, "src/gone.rs", Side::Right, 1).change_tracking_id,
            None
        );
    }

    #[test]
    fn ac9_vote_applies_at_once_and_reverts_exactly() {
        let mut l = loaded(pr());
        let before = apply_vote(&mut l, Vote::Approved, 5);
        assert_eq!(l.pr().vote_of("me"), Some(Vote::Approved));
        revert_vote(&mut l, before);
        assert_eq!(l.pr().vote_of("me"), Some(Vote::NoVote));

        // Not a reviewer yet: voting adds me; reverting removes me again.
        let mut other = pr();
        other.reviewers.clear();
        let mut l = loaded(other);
        let before = apply_vote(&mut l, Vote::WaitingForAuthor, 5);
        assert_eq!(l.pr().vote_of("me"), Some(Vote::WaitingForAuthor));
        revert_vote(&mut l, before);
        assert_eq!(l.pr().vote_of("me"), None);
    }

    #[test]
    fn fr16_status_menu_offers_only_valid_transitions() {
        use StatusAction::*;
        let mut p = pr();
        assert_eq!(status_actions(false, &p, false), [MarkDraft, Abandon]);
        p.is_draft = true;
        assert_eq!(status_actions(false, &p, false), [Publish, Abandon]);
        assert_eq!(draft_toggle(&p), Publish);
        p.status = PrStatus::Abandoned;
        assert_eq!(status_actions(false, &p, false), [Reactivate]);
        p.status = PrStatus::Completed;
        assert!(status_actions(false, &p, false).is_empty());
        // Read-only, or while Azure DevOps merges my Complete: none.
        assert!(status_actions(true, &pr(), false).is_empty());
        assert!(status_actions(false, &pr(), true).is_empty());
    }

    #[test]
    fn fr16_refused_status_changes_say_why() {
        use StatusAction::*;
        let active = pr();
        assert_eq!(status_refusal(false, &active, false, MarkDraft), None);
        let why = |pr: &PullRequest, action| status_refusal(false, pr, false, action);
        assert_eq!(why(&active, Publish).as_deref(), Some("!912 isn't a draft"));
        assert_eq!(
            why(&active, Reactivate).as_deref(),
            Some("!912 isn't abandoned")
        );
        let mut draft = pr();
        draft.is_draft = true;
        assert_eq!(
            why(&draft, MarkDraft).as_deref(),
            Some("!912 is already a draft")
        );
        let mut abandoned = pr();
        abandoned.status = PrStatus::Abandoned;
        assert_eq!(
            why(&abandoned, Publish).as_deref(),
            Some("!912 is abandoned")
        );
        assert_eq!(
            why(&abandoned, Abandon).as_deref(),
            Some("!912 is already abandoned")
        );
        let mut completed = pr();
        completed.status = PrStatus::Completed;
        assert_eq!(
            why(&completed, Reactivate).as_deref(),
            Some("!912 is completed")
        );
        assert_eq!(
            status_refusal(true, &active, false, Abandon).as_deref(),
            Some(READ_ONLY_HINT)
        );
        assert_eq!(
            status_refusal(false, &active, true, Abandon).as_deref(),
            Some("Azure DevOps is completing !912")
        );
    }

    #[test]
    fn fr16_status_applies_at_once_and_reverts_exactly() {
        let mut p = pr();
        let before = apply_status_action(&mut p, StatusAction::Abandon, 7);
        assert_eq!((p.status, p.closed), (PrStatus::Abandoned, Some(7)));
        revert_status(&mut p, before);
        assert_eq!((p.status, p.closed), (PrStatus::Active, None));

        let before = apply_status_action(&mut p, StatusAction::MarkDraft, 7);
        assert!(p.is_draft);
        revert_status(&mut p, before);
        assert!(!p.is_draft);

        // Reactivating keeps a draft a draft (Azure DevOps does too).
        let mut abandoned = pr();
        abandoned.status = PrStatus::Abandoned;
        abandoned.is_draft = true;
        abandoned.closed = Some(3);
        apply_status_action(&mut abandoned, StatusAction::Reactivate, 7);
        assert_eq!(
            (abandoned.status, abandoned.is_draft, abandoned.closed),
            (PrStatus::Active, true, None)
        );
    }

    #[test]
    fn fr17_linked_items_follow_azure_devops_when_known() {
        let item = |id: i64| WorkItem {
            id,
            project_id: "p".into(),
            rev: 1,
            kind: adoken_model::WorkItemType::Bug,
            title: format!("Item {id}"),
            state: "Active".into(),
            state_category: adoken_model::StateCategory::InProgress,
            assigned_to: None,
            created_by: "bo".into(),
            changed_by: "bo".into(),
            area_path: String::new(),
            iteration_path: String::new(),
            priority: None,
            tags: Vec::new(),
            parent_id: None,
            linked_pr_ids: vec![912],
            description: None,
            comment_count: 0,
            created: 0,
            changed: 0,
        };
        let ids = |rows: Vec<LinkedItem>| {
            rows.iter()
                .map(|r| (r.id, r.item.is_some()))
                .collect::<Vec<_>>()
        };
        // Not fetched yet: the store's links, by id.
        assert_eq!(
            ids(linked_items(vec![item(30), item(7)], None)),
            [(7, true), (30, true)]
        );
        // Azure DevOps wins: 30 was unlinked there, 55 is in a project
        // adoken doesn't sync.
        assert_eq!(
            ids(linked_items(vec![item(30), item(7)], Some(&[7, 55]))),
            [(7, true), (55, false)]
        );
    }

    #[test]
    fn draft_targets_round_trip_through_their_keys() {
        for target in [
            DraftTarget::Pr,
            DraftTarget::Reply { thread: 42 },
            DraftTarget::Line {
                path: "src/a:b.rs".into(),
                side: Side::Left,
                line: 7,
            },
        ] {
            assert_eq!(DraftTarget::parse(&target.key()), Some(target));
        }
        assert_eq!(DraftTarget::parse("line:X:1:a"), None);
    }

    #[test]
    fn resolve_toggle() {
        assert_eq!(toggled_status(ThreadStatus::Active), ThreadStatus::Fixed);
        assert_eq!(toggled_status(ThreadStatus::Pending), ThreadStatus::Fixed);
        assert_eq!(toggled_status(ThreadStatus::WontFix), ThreadStatus::Active);
    }

    /// AC-13 against a mock transport: text typed offline survives a
    /// restart, ⌘↵ offline sends nothing, and after reconnecting exactly one
    /// request goes out, only on ⌘↵.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ac13_offline_draft_survives_restart_and_posts_only_on_command() {
        use std::sync::Arc;

        use adoken_model::{Person, Project, Repo};
        use adoken_store::write;
        use adoken_sync::transport::{ApiResponse, RateHeaders, Transport, TransportError};
        use adoken_sync::writes::PrRef;
        use adoken_sync::{ApiRequest, Governor, ManualClock, Method};
        use futures::future::BoxFuture;
        use parking_lot::Mutex;

        #[derive(Default)]
        struct Mock(Mutex<Vec<ApiRequest>>);
        impl Transport for Mock {
            fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
                self.0.lock().push(req);
                let body = serde_json::json!({
                    "id": 55, "status": "active",
                    "threadContext": { "filePath": "/src/a.rs",
                        "rightFileStart": { "line": 12, "offset": 1 },
                        "rightFileEnd": { "line": 12, "offset": 1 } },
                    "comments": [{ "id": 1, "parentCommentId": 0,
                        "author": { "id": "me", "displayName": "Ada Me" },
                        "content": "Off by one?", "commentType": "text",
                        "publishedDate": "2026-09-30T12:00:00Z",
                        "lastUpdatedDate": "2026-09-30T12:00:00Z" }],
                    "lastUpdatedDate": "2026-09-30T12:00:00Z",
                });
                Box::pin(async move {
                    Ok(ApiResponse {
                        status: 200,
                        rate: RateHeaders::default(),
                        body: serde_json::to_vec(&body).unwrap(),
                        content_type: None,
                        validators: Default::default(),
                    })
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("adoken.sqlite");
        let target = DraftTarget::Line {
            path: "src/a.rs".into(),
            side: Side::Right,
            line: 12,
        };
        {
            let store = Store::open(&path).unwrap();
            data::migrate(&store).unwrap();
            store
                .write(|tx| {
                    let person = |id: &str| Person {
                        id: id.into(),
                        display_name: id.into(),
                        unique_name: None,
                        is_group: false,
                        image_url: None,
                    };
                    write::upsert_person(tx, &person("me"), true)?;
                    write::upsert_person(tx, &person("bo"), false)?;
                    write::upsert_project(
                        tx,
                        &Project {
                            id: "p".into(),
                            name: "P".into(),
                        },
                    )?;
                    write::upsert_repo(
                        tx,
                        &Repo {
                            id: "r".into(),
                            project_id: "p".into(),
                            name: "r".into(),
                            default_branch: None,
                        },
                    )?;
                    write::upsert_pr(tx, &pr())?;
                    Ok(())
                })
                .unwrap();
            // Typed while offline; ⌘↵ keeps it.
            let body = "Off by one?";
            save_draft(&store, 912, &target, body, 10).unwrap();
            assert_eq!(submit(true, body), Submit::KeepDraft);
        }

        // Restart.
        let store = Store::open(&path).unwrap();
        data::migrate(&store).unwrap();
        let restored = drafts(&store, 912).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].target, target);
        assert_eq!(restored[0].body, "Off by one?");

        let mock = Arc::new(Mock::default());
        let clock = Arc::new(ManualClock::new(1_000));
        let writer = Writer::new(
            store.clone(),
            mock.clone(),
            Arc::new(Governor::new(clock.clone())),
            clock,
            url::Url::parse("https://dev.azure.com/contoso/").unwrap(),
        );
        // Back online: nothing was sent on its own.
        assert!(mock.0.lock().is_empty());
        // ⌘↵ again.
        assert_eq!(submit(false, &restored[0].body), Submit::Send);
        writer
            .send(ReviewWrite::CreateThread {
                pr: PrRef::of(&pr()),
                anchor: Some(LineAnchor {
                    path: "src/a.rs".into(),
                    side: Side::Right,
                    line: 12,
                    change_tracking_id: None,
                }),
                iteration: 1,
                body: restored[0].body.clone(),
            })
            .await
            .unwrap();
        let sent = mock.0.lock().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].method(), &Method::Post);
        assert_eq!(
            sent[0].body().unwrap()["comments"][0]["content"],
            "Off by one?"
        );
        // Posted: the draft is gone.
        delete_draft(&store, 912, &target).unwrap();
        assert!(drafts(&store, 912).unwrap().is_empty());
    }

    #[test]
    fn drafts_are_per_target_and_blank_text_forgets() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        data::migrate(&store).unwrap();
        save_draft(&store, 1, &DraftTarget::Pr, "first", 1).unwrap();
        save_draft(&store, 1, &DraftTarget::Reply { thread: 3 }, "reply", 2).unwrap();
        save_draft(&store, 2, &DraftTarget::Pr, "other PR", 3).unwrap();
        let got = drafts(&store, 1).unwrap();
        assert_eq!(
            got.iter().map(|d| d.body.as_str()).collect::<Vec<_>>(),
            ["reply", "first"],
            "newest first"
        );
        save_draft(&store, 1, &DraftTarget::Pr, "  ", 4).unwrap();
        assert_eq!(drafts(&store, 1).unwrap().len(), 1);
        assert_eq!(submit(false, " \n"), Submit::Empty);
    }

    /// Spec 0001 FR-17 on the review side: accepted writes prove their own
    /// scope (links prove `vso.work_write`), scope refusals mark the refused
    /// scope, and a permission refusal (FR-21's Contribute) or a conflict
    /// says nothing about the token.
    #[test]
    fn write_answers_prove_or_refuse_the_right_scope() {
        use adoken_sync::writes::{WORK_WRITE_SCOPE, WRITE_SCOPE};
        let pr = PrRef {
            project_id: "p".into(),
            repo_id: "r".into(),
            pr_id: 1,
        };
        let link = write_scope(&ReviewWrite::LinkWorkItem {
            pr: pr.clone(),
            work_item: 7,
        });
        let draft = write_scope(&ReviewWrite::SetDraft {
            pr: pr.clone(),
            draft: true,
        });
        assert_eq!(link, WORK_WRITE_SCOPE);
        assert_eq!(draft, WRITE_SCOPE);
        let ok = Ok(Applied::WorkItem(None));
        assert_eq!(scope_answer(link, &ok), Some((WORK_WRITE_SCOPE, false)));
        assert_eq!(
            scope_answer(draft, &Err(WriteError::MissingScope)),
            Some((WRITE_SCOPE, true))
        );
        let work_refused = Err(WriteError::MissingWorkScope { message: None });
        assert_eq!(
            scope_answer(link, &work_refused),
            Some((WORK_WRITE_SCOPE, true))
        );
        for said_nothing in [
            WriteError::NotAllowed { message: None },
            WriteError::NewCommits,
            WriteError::NotConnected,
        ] {
            assert_eq!(scope_answer(draft, &Err(said_nothing)), None);
        }
    }
}
