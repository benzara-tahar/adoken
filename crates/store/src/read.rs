//! Typed reads over any `&Connection` (pooled reader or the writer's open
//! transaction). Every query is served by an index or a primary key.

use std::collections::{HashMap, HashSet};

use adoken_model::{
    ChangeType, Comment, LineRange, Mention, MentionSource, MergeStatus, Millis, Org, Person,
    PersonId, Pin, PinSubject, PinUnavailable, Pipeline, PipelineRun, PrDetail, PrFileChange,
    PrIteration, PrStatus, PrThread, Project, PullRequest, Repo, Reviewer, RunReason, RunResult,
    RunStage, RunStatus, StageOutcome, StateCategory, Subject, SyncEvent, SyncEventKind,
    ThreadAnchor, ThreadStatus, Vote, WorkItem, WorkItemType,
};
use adoken_search::{DocKind, SearchDoc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::{Error, PrFilter, Result, RunFilter, WorkItemFilter};

/// `search_doc.kind` for comment docs (tier 2 only; not in the Cmd+P corpus).
pub const KIND_COMMENT: i64 = 100;

pub fn kind_code(kind: DocKind) -> i64 {
    match kind {
        DocKind::PullRequest => 1,
        DocKind::WorkItem => 2,
        DocKind::Pipeline => 3,
        DocKind::Repo => 4,
        DocKind::Project => 5,
        DocKind::Person => 6,
        DocKind::Command => 7,
    }
}

fn kind_from_code(code: i64) -> Option<DocKind> {
    Some(match code {
        1 => DocKind::PullRequest,
        2 => DocKind::WorkItem,
        3 => DocKind::Pipeline,
        4 => DocKind::Repo,
        5 => DocKind::Project,
        6 => DocKind::Person,
        7 => DocKind::Command,
        _ => return None,
    })
}

fn corrupt(what: &str, value: &str) -> Error {
    Error::Corrupt(format!("{what}: {value:?}"))
}

fn parse<T>(what: &str, value: &str, f: impl Fn(&str) -> Option<T>) -> Result<T> {
    f(value).ok_or_else(|| corrupt(what, value))
}

// ── org structure ─────────────────────────────────────────────────────────

pub fn org(c: &Connection) -> Result<Option<Org>> {
    Ok(
        c.query_row("SELECT id, name, url FROM org LIMIT 1", [], |r| {
            Ok(Org {
                id: r.get(0)?,
                name: r.get(1)?,
                url: r.get(2)?,
            })
        })
        .optional()?,
    )
}

fn person_row(r: &Row) -> rusqlite::Result<Person> {
    Ok(Person {
        id: r.get(0)?,
        display_name: r.get(1)?,
        unique_name: r.get(2)?,
        is_group: r.get(3)?,
        image_url: r.get(4)?,
    })
}

pub fn me(c: &Connection) -> Result<Option<Person>> {
    Ok(c.query_row(
        "SELECT id, display_name, unique_name, is_group, image_url FROM identity
         WHERE is_me = 1 LIMIT 1",
        [],
        person_row,
    )
    .optional()?)
}

pub fn people(c: &Connection) -> Result<Vec<Person>> {
    let mut st = c.prepare_cached(
        "SELECT id, display_name, unique_name, is_group, image_url FROM identity
         ORDER BY display_name",
    )?;
    let rows = st.query_map([], person_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn person(c: &Connection, id: &str) -> Result<Option<Person>> {
    Ok(c.prepare_cached(
        "SELECT id, display_name, unique_name, is_group, image_url FROM identity WHERE id = ?1",
    )?
    .query_row([id], person_row)
    .optional()?)
}

pub fn projects(c: &Connection) -> Result<Vec<Project>> {
    let mut st = c.prepare_cached("SELECT id, name FROM project ORDER BY name")?;
    let rows = st.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The projects sync covers (onboarding / Settings › Projects); [`projects`]
/// also lists projects known only through a pinned item (spec 0014).
pub fn synced_projects(c: &Connection) -> Result<Vec<Project>> {
    let mut st = c.prepare_cached("SELECT id, name FROM project WHERE synced = 1 ORDER BY name")?;
    let rows = st.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn repos(c: &Connection) -> Result<Vec<Repo>> {
    let mut st =
        c.prepare_cached("SELECT id, project_id, name, default_branch FROM repo ORDER BY name")?;
    let rows = st.query_map([], |r| {
        Ok(Repo {
            id: r.get(0)?,
            project_id: r.get(1)?,
            name: r.get(2)?,
            default_branch: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ── pull requests ─────────────────────────────────────────────────────────

const PR_COLS: &str = "id, repo_id, project_id, title, description, author_id, status, is_draft, \
     merge_status, source_ref, target_ref, last_source_commit, created_ms, updated_ms, closed_ms, \
     iterations, last_push_ms";

struct PrRow {
    pr: PullRequest,
    status: String,
    merge: String,
}

fn pr_row(r: &Row) -> rusqlite::Result<PrRow> {
    Ok(PrRow {
        pr: PullRequest {
            id: r.get(0)?,
            repo_id: r.get(1)?,
            project_id: r.get(2)?,
            title: r.get(3)?,
            description: r.get(4)?,
            author: r.get(5)?,
            reviewers: Vec::new(),
            status: PrStatus::Active,
            is_draft: r.get(7)?,
            merge_status: MergeStatus::NotSet,
            source_branch: r.get(9)?,
            target_branch: r.get(10)?,
            last_source_commit: r.get(11)?,
            created: r.get(12)?,
            updated: r.get(13)?,
            closed: r.get(14)?,
            iterations: r.get(15)?,
            last_push: r.get(16)?,
        },
        status: r.get(6)?,
        merge: r.get(8)?,
    })
}

fn finish_pr(c: &Connection, row: PrRow) -> Result<PullRequest> {
    let mut pr = row.pr;
    pr.status = parse("pr.status", &row.status, PrStatus::parse)?;
    pr.merge_status = parse("pr.merge_status", &row.merge, MergeStatus::parse)?;
    pr.reviewers = reviewers(c, pr.id)?;
    Ok(pr)
}

pub fn reviewers(c: &Connection, pr_id: i64) -> Result<Vec<Reviewer>> {
    let mut st = c.prepare_cached(
        "SELECT identity_id, vote, is_required, has_declined, added_ms, voted_ms
         FROM pr_reviewer WHERE pr_id = ?1 ORDER BY ord",
    )?;
    let rows = st.query_map([pr_id], |r| {
        Ok(Reviewer {
            person: r.get(0)?,
            vote: Vote::from_i8(r.get(1)?),
            is_required: r.get(2)?,
            has_declined: r.get(3)?,
            added_at: r.get(4)?,
            voted_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn pr(c: &Connection, id: i64) -> Result<Option<PullRequest>> {
    let row = c
        .prepare_cached(&format!("SELECT {PR_COLS} FROM pull_request WHERE id = ?1"))?
        .query_row([id], pr_row)
        .optional()?;
    row.map(|row| finish_pr(c, row)).transpose()
}

pub fn prs(c: &Connection, f: &PrFilter) -> Result<Vec<PullRequest>> {
    let mut sql = format!("SELECT {PR_COLS} FROM pull_request p WHERE 1");
    let mut args: Vec<Value> = Vec::new();
    if let Some(s) = f.status {
        sql.push_str(" AND status = ?");
        args.push(s.as_str().to_owned().into());
    }
    if let Some(a) = &f.author {
        sql.push_str(" AND author_id = ?");
        args.push(a.clone().into());
    }
    if let Some(r) = &f.reviewer {
        sql.push_str(" AND id IN (SELECT pr_id FROM pr_reviewer WHERE identity_id = ?)");
        args.push(r.clone().into());
    }
    if let Some(who) = &f.involving {
        sql.push_str(
            " AND (author_id = ? OR id IN (SELECT pr_id FROM pr_reviewer WHERE identity_id = ?))",
        );
        args.push(who.clone().into());
        args.push(who.clone().into());
    }
    if let Some(p) = &f.project_id {
        sql.push_str(" AND project_id = ?");
        args.push(p.clone().into());
    }
    if let Some(r) = &f.repo_id {
        sql.push_str(" AND repo_id = ?");
        args.push(r.clone().into());
    }
    if let Some(t) = f.updated_since {
        sql.push_str(" AND updated_ms >= ?");
        args.push(t.into());
    }
    sql.push_str(" ORDER BY updated_ms DESC, id DESC");
    if let Some(n) = f.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut st = c.prepare_cached(&sql)?;
    let rows = st
        .query_map(params_from_iter(args), pr_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|row| finish_pr(c, row)).collect()
}

pub fn pr_iterations(c: &Connection, pr_id: i64) -> Result<Vec<PrIteration>> {
    let mut st = c.prepare_cached(
        "SELECT id, source_commit, created_ms, description, commit_count, commits_truncated
         FROM pr_iteration WHERE pr_id = ?1 ORDER BY id",
    )?;
    let rows = st.query_map([pr_id], iteration_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Spec 0017 FR-4 for `pr_id`: its pushes at or after `since` (today's
/// commits), oldest first, and the source commit of its latest stored push
/// (when that isn't the PR's head, a push hasn't reached the iteration list
/// yet: "New commits today"). One primary-key range scan.
pub fn pr_iterations_since(
    c: &Connection,
    pr_id: i64,
    since: Millis,
) -> Result<(Vec<PrIteration>, Option<String>)> {
    let mut st = c.prepare_cached(
        "SELECT id, source_commit, created_ms, description, commit_count, commits_truncated
         FROM pr_iteration
         WHERE pr_id = ?1
           AND (created_ms >= ?2 OR id = (SELECT max(id) FROM pr_iteration WHERE pr_id = ?1))
         ORDER BY id",
    )?;
    let mut rows = st
        .query_map(params![pr_id, since], iteration_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let latest = rows.last().map(|it| it.source_commit.clone());
    rows.retain(|it| it.created >= since);
    Ok((rows, latest))
}

fn iteration_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PrIteration> {
    Ok(PrIteration {
        id: r.get(0)?,
        source_commit: r.get(1)?,
        created: r.get(2)?,
        description: r.get(3)?,
        commit_count: r.get(4)?,
        commits_truncated: r.get(5)?,
    })
}

pub fn pr_files(c: &Connection, pr_id: i64) -> Result<Vec<PrFileChange>> {
    let mut st = c.prepare_cached(
        "SELECT path, original_path, change_type, old_blob, new_blob, change_tracking_id
         FROM pr_file WHERE pr_id = ?1 ORDER BY ord",
    )?;
    let rows = st
        .query_map([pr_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<u32>>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(path, original_path, ct, old, new, tracking)| {
            Ok(PrFileChange {
                path,
                original_path,
                change_type: parse("pr_file.change_type", &ct, ChangeType::parse)?,
                old_content: old,
                new_content: new,
                change_tracking_id: tracking,
            })
        })
        .collect()
}

pub fn pr_threads(c: &Connection, pr_id: i64) -> Result<Vec<PrThread>> {
    let mut st = c.prepare_cached(
        "SELECT id, status, file_path, right_start, right_end, left_start, left_end, iteration,
                iteration_blob, updated_ms
         FROM pr_thread WHERE pr_id = ?1 ORDER BY id",
    )?;
    let raw = st
        .query_map([pr_id], |r| {
            let range = |s: Option<u32>, e: Option<u32>| {
                s.map(|start| LineRange {
                    start,
                    end: e.unwrap_or(start),
                })
            };
            let file_path: Option<String> = r.get(2)?;
            let anchor = match file_path {
                Some(file_path) => Some(ThreadAnchor {
                    file_path,
                    right: range(r.get(3)?, r.get(4)?),
                    left: range(r.get(5)?, r.get(6)?),
                    iteration_blob: r.get(8)?,
                }),
                None => None,
            };
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                anchor,
                r.get::<_, Option<u32>>(7)?,
                r.get::<_, i64>(9)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut comments = pr_comments(c, pr_id)?;
    raw.into_iter()
        .map(|(id, status, anchor, iteration, updated)| {
            Ok(PrThread {
                id,
                pr_id,
                status: ThreadStatus::parse(&status).unwrap_or(ThreadStatus::Unknown),
                anchor,
                iteration,
                comments: comments.remove(&id).unwrap_or_default(),
                updated,
            })
        })
        .collect()
}

/// Comments of one PR grouped by thread id, each in ADO comment-id order.
fn pr_comments(c: &Connection, pr_id: i64) -> Result<HashMap<i64, Vec<Comment>>> {
    let mut st = c.prepare_cached(
        "SELECT thread_id, ado_comment_id, reply_to, author_id, body, created_ms, updated_ms,
                is_system, is_deleted
         FROM comment WHERE source = 'pr' AND parent_id = ?1
         ORDER BY thread_id, ado_comment_id",
    )?;
    let mut out: HashMap<i64, Vec<Comment>> = HashMap::new();
    let rows = st.query_map([pr_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Comment {
                id: r.get(1)?,
                parent_id: r.get(2)?,
                author: r.get(3)?,
                body: r.get(4)?,
                created: r.get(5)?,
                updated: r.get(6)?,
                is_system: r.get(7)?,
                is_deleted: r.get(8)?,
            },
        ))
    })?;
    for row in rows {
        let (thread, comment) = row?;
        out.entry(thread).or_default().push(comment);
    }
    Ok(out)
}

pub fn pr_detail(c: &Connection, id: i64) -> Result<Option<PrDetail>> {
    let Some(pr) = pr(c, id)? else {
        return Ok(None);
    };
    Ok(Some(PrDetail {
        iterations: pr_iterations(c, id)?,
        files: pr_files(c, id)?,
        threads: pr_threads(c, id)?,
        pr,
    }))
}

pub fn pr_threads_batch(c: &Connection, ids: &[i64]) -> Result<Vec<(i64, Vec<PrThread>)>> {
    ids.iter().map(|&id| Ok((id, pr_threads(c, id)?))).collect()
}

/// List-level fingerprint stored with the PR (change detection, spec 0002 FR-6).
pub fn pr_fingerprint(c: &Connection, id: i64) -> Result<Option<i64>> {
    Ok(
        c.prepare_cached("SELECT fingerprint FROM pull_request WHERE id = ?1")?
            .query_row([id], |r| r.get(0))
            .optional()?,
    )
}

pub fn pr_hydrated(c: &Connection, id: i64) -> Result<Option<Millis>> {
    Ok(
        c.prepare_cached("SELECT hydrated_ms FROM pull_request WHERE id = ?1")?
            .query_row([id], |r| r.get::<_, Option<Millis>>(0))
            .optional()?
            .flatten(),
    )
}

/// Ids of PRs with the given status.
pub fn pr_ids_with_status(c: &Connection, status: PrStatus) -> Result<Vec<i64>> {
    let mut st = c.prepare_cached("SELECT id FROM pull_request WHERE status = ?1")?;
    let rows = st.query_map([status.as_str()], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Which iteration a PR's `pr_file` rows describe (spec 0004 FR-1): its
/// latest iteration's changes against the merge base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSync {
    pub iteration: u32,
    pub source_commit: String,
    /// The iteration's `commonRefCommit` (merge base).
    pub base_commit: Option<String>,
    pub synced_ms: Millis,
}

pub fn pr_file_sync(c: &Connection, pr_id: i64) -> Result<Option<FileSync>> {
    Ok(c.prepare_cached(
        "SELECT iteration, source_commit, base_commit, synced_ms FROM pr_file_sync
         WHERE pr_id = ?1",
    )?
    .query_row([pr_id], |r| {
        Ok(FileSync {
            iteration: r.get(0)?,
            source_commit: r.get(1)?,
            base_commit: r.get(2)?,
            synced_ms: r.get(3)?,
        })
    })
    .optional()?)
}

// ── work items ────────────────────────────────────────────────────────────

const WI_COLS: &str = "id, project_id, rev, type, title, state, state_category, assigned_to_id, \
     created_by_id, changed_by_id, area_path, iteration_path, priority, tags, parent_id, \
     description, comment_count, created_ms, changed_ms";

struct WiRow {
    wi: WorkItem,
    kind: String,
    category: String,
    tags: String,
}

fn wi_row(r: &Row) -> rusqlite::Result<WiRow> {
    Ok(WiRow {
        wi: WorkItem {
            id: r.get(0)?,
            project_id: r.get(1)?,
            rev: r.get(2)?,
            kind: WorkItemType::Task,
            title: r.get(4)?,
            state: r.get(5)?,
            state_category: StateCategory::Proposed,
            assigned_to: r.get(7)?,
            created_by: r.get(8)?,
            changed_by: r.get(9)?,
            area_path: r.get(10)?,
            iteration_path: r.get(11)?,
            priority: r.get(12)?,
            tags: Vec::new(),
            parent_id: r.get(14)?,
            linked_pr_ids: Vec::new(),
            description: r.get(15)?,
            comment_count: r.get(16)?,
            created: r.get(17)?,
            changed: r.get(18)?,
        },
        kind: r.get(3)?,
        category: r.get(6)?,
        tags: r.get(13)?,
    })
}

fn finish_wi(c: &Connection, row: WiRow) -> Result<WorkItem> {
    let mut wi = row.wi;
    wi.kind = parse("work_item.type", &row.kind, WorkItemType::parse)?;
    wi.state_category = parse(
        "work_item.state_category",
        &row.category,
        StateCategory::parse,
    )?;
    wi.tags = row
        .tags
        .split(';')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect();
    let mut st =
        c.prepare_cached("SELECT pr_id FROM wi_pr_link WHERE work_item_id = ?1 ORDER BY pr_id")?;
    wi.linked_pr_ids = st
        .query_map([wi.id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(wi)
}

pub fn work_item(c: &Connection, id: i64) -> Result<Option<WorkItem>> {
    let row = c
        .prepare_cached(&format!(
            "SELECT {WI_COLS} FROM work_item WHERE id = ?1 AND is_deleted = 0"
        ))?
        .query_row([id], wi_row)
        .optional()?;
    row.map(|row| finish_wi(c, row)).transpose()
}

/// The latest transition into Completed/Removed; ordinary edits don't move it.
pub fn work_item_closed_at(c: &Connection, id: i64) -> Result<Option<Millis>> {
    Ok(c.prepare_cached(
        "SELECT terminal_since_ms FROM work_item WHERE id = ?1 AND is_deleted = 0",
    )?
    .query_row([id], |r| r.get::<_, Option<Millis>>(0))
    .optional()?
    .flatten())
}

/// Work items assigned to `assignee` whose transition into Completed is at
/// or after `since` (spec 0017 FR-3 "done"): ids, by id. Removed items and
/// items completed earlier but edited since don't count. Served by
/// `wi_by_assignee`.
pub fn work_items_completed_since(
    c: &Connection,
    assignee: &str,
    since: Millis,
) -> Result<Vec<i64>> {
    let mut st = c.prepare_cached(
        "SELECT id FROM work_item
         WHERE assigned_to_id = ?1 AND state_category = 'Completed' AND is_deleted = 0
           AND terminal_since_ms >= ?2
         ORDER BY id",
    )?;
    let rows = st.query_map(params![assignee, since], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// When each work item assigned to `assignee` in `category` entered its
/// current state (`System.StateChangeDate`), falling back to its last
/// change when unknown (owner decision on 0017 Q-2): `(id, since)` by id.
/// One range of `wi_by_assignee`.
pub fn work_items_state_since(
    c: &Connection,
    assignee: &str,
    category: StateCategory,
) -> Result<Vec<(i64, Millis)>> {
    let mut st = c.prepare_cached(
        "SELECT id, coalesce(state_since_ms, changed_ms) FROM work_item
         WHERE assigned_to_id = ?1 AND state_category = ?2 AND is_deleted = 0
         ORDER BY id",
    )?;
    let rows = st.query_map(params![assignee, category.as_str()], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Work items linked to pull request `pr_id` (their `ArtifactLink`
/// relations, `wi_pr_link`), by id. Indexed by `wi_pr_link_by_pr`.
pub fn pr_work_items(c: &Connection, pr_id: i64) -> Result<Vec<WorkItem>> {
    let rows = c
        .prepare_cached(&format!(
            "SELECT {WI_COLS} FROM work_item WHERE is_deleted = 0
               AND id IN (SELECT work_item_id FROM wi_pr_link WHERE pr_id = ?1) ORDER BY id"
        ))?
        .query_map([pr_id], wi_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|row| finish_wi(c, row)).collect()
}

pub fn work_item_rev(c: &Connection, id: i64) -> Result<Option<i64>> {
    Ok(c.prepare_cached("SELECT rev FROM work_item WHERE id = ?1")?
        .query_row([id], |r| r.get(0))
        .optional()?)
}

pub fn work_items(c: &Connection, f: &WorkItemFilter) -> Result<Vec<WorkItem>> {
    let mut sql = format!("SELECT {WI_COLS} FROM work_item WHERE is_deleted = 0");
    let mut args: Vec<Value> = Vec::new();
    if let Some(a) = &f.assigned_to {
        sql.push_str(" AND assigned_to_id = ?");
        args.push(a.clone().into());
    }
    if let Some(p) = &f.project_id {
        sql.push_str(" AND project_id = ?");
        args.push(p.clone().into());
    }
    if let Some(k) = &f.kind {
        sql.push_str(" AND type = ?");
        args.push(k.as_str().to_owned().into());
    }
    if let Some(s) = f.state_category {
        sql.push_str(" AND state_category = ?");
        args.push(s.as_str().to_owned().into());
    }
    if f.open_only {
        sql.push_str(" AND state_category IN ('Proposed', 'InProgress')");
    }
    if let Some(t) = f.changed_since {
        sql.push_str(" AND changed_ms >= ?");
        args.push(t.into());
    }
    sql.push_str(" ORDER BY changed_ms DESC, id DESC");
    if let Some(n) = f.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut st = c.prepare_cached(&sql)?;
    let rows = st
        .query_map(params_from_iter(args), wi_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|row| finish_wi(c, row)).collect()
}

// ── pipelines ─────────────────────────────────────────────────────────────

pub fn pipelines(c: &Connection) -> Result<Vec<Pipeline>> {
    let mut st =
        c.prepare_cached("SELECT id, project_id, name, folder FROM pipeline ORDER BY name")?;
    let rows = st.query_map([], |r| {
        Ok(Pipeline {
            id: r.get(0)?,
            project_id: r.get(1)?,
            name: r.get(2)?,
            folder: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

const RUN_COLS: &str = "id, project_id, definition_id, definition_name, run_number, status, result, \
     reason, source_branch, source_version, requested_for_id, pr_id, queued_ms, started_ms, \
     finished_ms";

fn run_row(r: &Row) -> rusqlite::Result<(PipelineRun, String, Option<String>, String)> {
    Ok((
        PipelineRun {
            id: r.get(0)?,
            project_id: r.get(1)?,
            pipeline_id: r.get(2)?,
            pipeline_name: r.get(3)?,
            run_number: r.get(4)?,
            status: RunStatus::NotStarted,
            result: None,
            reason: RunReason::Other,
            branch: r.get(8)?,
            source_version: r.get(9)?,
            requested_for: r.get(10)?,
            pr_id: r.get(11)?,
            queued: r.get(12)?,
            started: r.get(13)?,
            finish: r.get(14)?,
        },
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
    ))
}

fn finish_run(
    (mut run, status, result, reason): (PipelineRun, String, Option<String>, String),
) -> Result<PipelineRun> {
    run.status = parse("run.status", &status, RunStatus::parse)?;
    run.result = result
        .map(|r| parse("run.result", &r, RunResult::parse))
        .transpose()?;
    run.reason = RunReason::parse(&reason).unwrap_or(RunReason::Other);
    Ok(run)
}

pub fn run(c: &Connection, id: i64) -> Result<Option<PipelineRun>> {
    c.prepare_cached(&format!(
        "SELECT {RUN_COLS} FROM pipeline_run WHERE id = ?1"
    ))?
    .query_row([id], run_row)
    .optional()?
    .map(finish_run)
    .transpose()
}

pub fn runs(c: &Connection, f: &RunFilter) -> Result<Vec<PipelineRun>> {
    let mut sql = format!("SELECT {RUN_COLS} FROM pipeline_run WHERE 1");
    let mut args: Vec<Value> = Vec::new();
    if let Some(p) = &f.requested_for {
        sql.push_str(" AND requested_for_id = ?");
        args.push(p.clone().into());
    }
    if let Some(p) = f.pr_id {
        sql.push_str(" AND pr_id = ?");
        args.push(p.into());
    }
    if let Some(b) = &f.branch {
        sql.push_str(" AND source_branch = ?");
        args.push(b.clone().into());
    }
    if let Some(p) = &f.project_id {
        sql.push_str(" AND project_id = ?");
        args.push(p.clone().into());
    }
    if let Some(p) = f.pipeline_id {
        sql.push_str(" AND definition_id = ?");
        args.push(p.into());
    }
    if let Some(s) = f.status {
        sql.push_str(" AND status = ?");
        args.push(s.as_str().to_owned().into());
    }
    sql.push_str(" ORDER BY queued_ms DESC, id DESC");
    if let Some(n) = f.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut st = c.prepare_cached(&sql)?;
    let rows = st
        .query_map(params_from_iter(args), run_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(finish_run).collect()
}

/// Finish time of the oldest finished run `who` requested in `project`
/// (`run_by_user` index).
pub fn oldest_run_finish(c: &Connection, project: &str, who: &str) -> Result<Option<Millis>> {
    Ok(c.prepare_cached(
        "SELECT min(finished_ms) FROM pipeline_run
         WHERE requested_for_id = ?1 AND project_id = ?2 AND finished_ms IS NOT NULL",
    )?
    .query_row(params![who, project], |r| r.get(0))?)
}

/// Stages of one run in pipeline order (primary-key range scan).
pub fn run_stages(c: &Connection, run_id: i64) -> Result<Vec<RunStage>> {
    let mut st = c.prepare_cached(
        "SELECT name, outcome, started_ms, finished_ms FROM pipelines_stage
         WHERE run_id = ?1 ORDER BY ord",
    )?;
    let rows = st
        .query_map([run_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(name, outcome, started, finished)| {
            Ok(RunStage {
                name,
                outcome: parse("pipelines_stage.outcome", &outcome, StageOutcome::parse)?,
                started,
                finished,
            })
        })
        .collect()
}

/// Runs whose stored stages don't match their current state (never fetched,
/// fetched while running, or the run changed since), newest queued first.
/// Served by the `run_timeline_due` partial index.
pub fn runs_timeline_due(c: &Connection, limit: usize) -> Result<Vec<PipelineRun>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {RUN_COLS} FROM pipeline_run
         WHERE timeline_fp IS NOT (status || ':' || coalesce(result, '') || ':' || coalesce(finished_ms, ''))
         ORDER BY queued_ms DESC LIMIT ?1"
    ))?;
    let rows = st
        .query_map([limit as i64], run_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(finish_run).collect()
}

/// `(id, project_id)` of work items whose discussion must be fetched,
/// most recently changed first (`wi_comments_due` partial index).
pub fn work_items_comments_due(c: &Connection, limit: usize) -> Result<Vec<(i64, String)>> {
    let mut st = c.prepare_cached(
        "SELECT id, project_id FROM work_item WHERE comments_due = 1
         ORDER BY changed_ms DESC LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// ADO comment ids of a work item's stored (not deleted) discussion.
pub fn wi_comment_ids(c: &Connection, work_item_id: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare_cached(
        "SELECT ado_comment_id FROM comment
         WHERE source = 'wi' AND parent_id = ?1 AND thread_id = 0 AND is_deleted = 0",
    )?;
    let rows = st
        .query_map([work_item_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// People indexed as mentioned in a stored comment.
pub fn comment_mentions(
    c: &Connection,
    source: &str,
    parent: i64,
    thread: i64,
    ado_comment_id: i64,
) -> Result<Vec<PersonId>> {
    let mut st = c.prepare_cached(
        "SELECT m.identity_id FROM comment c JOIN mention m ON m.comment_id = c.id
         WHERE c.source = ?1 AND c.parent_id = ?2 AND c.thread_id = ?3 AND c.ado_comment_id = ?4",
    )?;
    let rows = st
        .query_map(params![source, parent, thread, ado_comment_id], |r| {
            r.get(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ── mentions ──────────────────────────────────────────────────────────────

/// `@<guid>` mention tokens in a comment body, in order, deduplicated.
pub fn mention_ids(body: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("@<") {
        rest = &rest[i + 2..];
        let Some(end) = rest.find('>') else { break };
        let id = &rest[..end];
        if !id.is_empty()
            && id.len() <= 64
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !out.contains(&id)
        {
            out.push(id);
        }
        rest = &rest[end + 1..];
    }
    out
}

/// Comment text with `@<guid>` resolved to `@Name`, whitespace collapsed,
/// cut to ~160 chars around the first mention of `who`.
pub fn snippet(c: &Connection, body: &str, who: &str) -> Result<String> {
    let mut text = String::with_capacity(body.len());
    let mut rest = body;
    let mut focus = None;
    while let Some(i) = rest.find("@<") {
        text.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find('>') {
            Some(end) => {
                let id = &after[..end];
                if id == who && focus.is_none() {
                    focus = Some(text.len());
                }
                let name = person(c, id)?
                    .map(|p| p.display_name)
                    .unwrap_or_else(|| "someone".to_owned());
                text.push('@');
                text.push_str(&name);
                rest = &after[end + 1..];
            }
            None => {
                text.push_str("@<");
                rest = after;
            }
        }
    }
    text.push_str(rest);
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 160;
    let chars: Vec<char> = collapsed.chars().collect();
    if chars.len() <= MAX {
        return Ok(collapsed);
    }
    // Map the byte focus in `text` to roughly the same char offset.
    let focus_chars = focus
        .map(|b| {
            text[..b]
                .split_whitespace()
                .map(|w| w.chars().count() + 1)
                .sum::<usize>()
        })
        .unwrap_or(0);
    let start = focus_chars.saturating_sub(40).min(chars.len() - MAX);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..start + MAX]);
    if start + MAX < chars.len() {
        out.push('…');
    }
    Ok(out)
}

pub fn mentions(c: &Connection, who: &PersonId, limit: usize) -> Result<Vec<Mention>> {
    let mut st = c.prepare_cached(
        "SELECT c.source, c.parent_id, c.thread_id, c.ado_comment_id, c.author_id, c.body,
                c.created_ms
         FROM mention m JOIN comment c ON c.id = m.comment_id
         WHERE m.identity_id = ?1 AND c.is_deleted = 0
         ORDER BY c.created_ms DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![who, limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(source, parent, thread, comment, author, body, when)| {
            let source = match source.as_str() {
                "pr" => MentionSource::Pr {
                    pr_id: parent,
                    thread_id: thread,
                    comment_id: comment,
                },
                _ => MentionSource::WorkItem {
                    work_item_id: parent,
                    comment_id: comment,
                },
            };
            Ok(Mention {
                source,
                who: author,
                mentioned: who.clone(),
                when,
                snippet: snippet(c, &body, who)?,
            })
        })
        .collect()
}

// ── events ────────────────────────────────────────────────────────────────

pub fn sync_events_since(c: &Connection, after_id: i64, limit: usize) -> Result<Vec<SyncEvent>> {
    let mut st = c.prepare_cached(
        "SELECT id, kind, subject_key, dedupe_key, actor_id, self_caused, at_ms, summary
         FROM sync_event WHERE id > ?1 ORDER BY id LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![after_id, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(
            |(id, kind, subject, dedupe_key, actor, self_caused, at, summary)| {
                Ok(SyncEvent {
                    id,
                    kind: parse("sync_event.kind", &kind, SyncEventKind::parse)?,
                    subject: parse("sync_event.subject", &subject, Subject::parse_key)?,
                    dedupe_key,
                    actor,
                    self_caused,
                    at,
                    summary,
                })
            },
        )
        .collect()
}

pub fn sync_event_count(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT count(*) FROM sync_event", [], |r| r.get(0))?)
}

// ── search ────────────────────────────────────────────────────────────────

pub fn search_docs(c: &Connection) -> Result<Vec<SearchDoc>> {
    let mut st = c.prepare_cached(
        "SELECT kind, doc_id, title, subtitle FROM search_doc WHERE kind < ?1 ORDER BY id",
    )?;
    let rows = st.query_map([KIND_COMMENT], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, id, title, subtitle) = row?;
        let Some(kind) = kind_from_code(kind) else {
            continue;
        };
        out.push(SearchDoc {
            id: id as u64,
            kind,
            title,
            subtitle,
        });
    }
    Ok(out)
}

pub fn entity_key(c: &Connection, kind: DocKind, id: u64) -> Result<Option<String>> {
    Ok(c.prepare_cached(
        "SELECT entity_key FROM search_doc WHERE kind = ?1 AND doc_id = ?2 LIMIT 1",
    )?
    .query_row(params![kind_code(kind), id as i64], |r| r.get(0))
    .optional()?)
}

/// One tier-2 (FTS5) hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextHit {
    /// `pr:4312`, `wi:88121`, or for comments the commented entity's key.
    pub entity_key: String,
    pub title: String,
    /// Matched text with `[` `]` around hits.
    pub snippet: String,
}

/// FTS5 prefix-AND query: each alphanumeric token becomes `"tok"*`.
pub fn search_text(c: &Connection, query: &str, limit: usize) -> Result<Vec<TextHit>> {
    let terms: Vec<String> = query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut st = c.prepare_cached(
        "SELECT d.entity_key, d.title, snippet(search_fts, -1, '[', ']', '…', 12)
         FROM search_fts JOIN search_doc d ON d.id = search_fts.rowid
         WHERE search_fts MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = st.query_map(params![terms.join(" AND "), limit as i64], |r| {
        let key: String = r.get(0)?;
        Ok(TextHit {
            entity_key: key.split('/').next().unwrap_or_default().to_owned(),
            title: r.get(1)?,
            snippet: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ── cursors & meta ────────────────────────────────────────────────────────

/// One `sync_cursor` row (ADR 0001 §5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub scope: String,
    pub project_id: String,
    pub token: Option<String>,
    pub high_water: Option<String>,
    pub backfill_to: Option<String>,
    pub baseline_done: bool,
    pub last_ok_ms: Option<Millis>,
    pub last_error: Option<String>,
}

pub fn cursor(c: &Connection, scope: &str, project_id: &str) -> Result<Option<Cursor>> {
    Ok(c.prepare_cached(
        "SELECT token, high_water, backfill_to, baseline_done, last_ok_ms, last_error
         FROM sync_cursor WHERE scope = ?1 AND project_id = ?2",
    )?
    .query_row(params![scope, project_id], |r| {
        Ok(Cursor {
            scope: scope.to_owned(),
            project_id: project_id.to_owned(),
            token: r.get(0)?,
            high_water: r.get(1)?,
            backfill_to: r.get(2)?,
            baseline_done: r.get(3)?,
            last_ok_ms: r.get(4)?,
            last_error: r.get(5)?,
        })
    })
    .optional()?)
}

pub fn last_sync(c: &Connection) -> Result<Option<Millis>> {
    Ok(c.query_row("SELECT max(last_ok_ms) FROM sync_cursor", [], |r| r.get(0))?)
}

/// Last successful pass per scheduler lane (spec 0016 FR-10):
/// `(lane key, last ok)`, in no particular order.
pub fn lane_sync(c: &Connection) -> Result<Vec<(String, Millis)>> {
    let mut stmt = c.prepare_cached("SELECT lane, last_ok_ms FROM lane_sync")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn meta(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.prepare_cached("SELECT value FROM meta WHERE key = ?1")?
        .query_row([key], |r| r.get(0))
        .optional()?)
}

pub fn last_viewed(c: &Connection, entity_key: &str) -> Result<Option<Millis>> {
    Ok(
        c.prepare_cached("SELECT last_viewed_ms FROM view_state WHERE entity_key = ?1")?
            .query_row([entity_key], |r| r.get(0))
            .optional()?,
    )
}

/// Auto-complete I set in adoken on `pr_id` (spec 0004 FR-12): the source
/// commit I had reviewed, and when.
pub fn auto_complete(c: &Connection, pr_id: i64) -> Result<Option<(String, Millis)>> {
    Ok(
        c.prepare_cached("SELECT source_commit, set_ms FROM pr_auto_complete WHERE pr_id = ?1")?
            .query_row([pr_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?,
    )
}

/// My Complete of `pr_id` that Azure DevOps is still merging (spec 0004
/// FR-10): the source commit sent, and when it was accepted.
pub fn completing(c: &Connection, pr_id: i64) -> Result<Option<(String, Millis)>> {
    Ok(
        c.prepare_cached("SELECT source_commit, set_ms FROM pr_completing WHERE pr_id = ?1")?
            .query_row([pr_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?,
    )
}

/// Human discussion still open, per PR (spec 0004 §3.1 list rows): threads
/// with status `active` and at least one comment that is neither a system
/// message nor deleted. PRs with none are absent.
pub fn active_thread_counts(c: &Connection) -> Result<HashMap<i64, u32>> {
    let mut st = c.prepare_cached(
        "SELECT t.pr_id, COUNT(*) FROM pr_thread t
         WHERE t.status = 'active' AND EXISTS (
             SELECT 1 FROM comment m
             WHERE m.source = 'pr' AND m.parent_id = t.pr_id AND m.thread_id = t.id
               AND m.is_system = 0 AND m.is_deleted = 0)
         GROUP BY t.pr_id",
    )?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The latest run of each pipeline built for each PR (`run_by_pr`), for the
/// PR list's CI status. Ties on queue time go to the higher run id.
pub fn latest_pr_runs(c: &Connection) -> Result<Vec<PipelineRun>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {RUN_COLS} FROM pipeline_run r
         WHERE r.pr_id IS NOT NULL AND NOT EXISTS (
             SELECT 1 FROM pipeline_run n
             WHERE n.pr_id = r.pr_id AND n.definition_id = r.definition_id
               AND (n.queued_ms > r.queued_ms OR (n.queued_ms = r.queued_ms AND n.id > r.id)))"
    ))?;
    let rows = st
        .query_map([], run_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(finish_run).collect()
}

/// PRs with auto-complete I set in adoken ([`auto_complete`]).
pub fn auto_complete_ids(c: &Connection) -> Result<HashSet<i64>> {
    let mut st = c.prepare_cached("SELECT pr_id FROM pr_auto_complete")?;
    let rows = st.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// PRs Azure DevOps is still merging after my Complete ([`completing`]).
pub fn completing_ids(c: &Connection) -> Result<HashSet<i64>> {
    let mut st = c.prepare_cached("SELECT pr_id FROM pr_completing")?;
    let rows = st.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Files changed in the stored iteration of `pr_id` (`pr_file` primary-key
/// range); 0 until its files have synced.
pub fn pr_file_count(c: &Connection, pr_id: i64) -> Result<u32> {
    Ok(
        c.prepare_cached("SELECT count(*) FROM pr_file WHERE pr_id = ?1")?
            .query_row([pr_id], |r| r.get(0))?,
    )
}

// ── pins (spec 0014, local) ──────────────────────────────────────────────

const PIN_COLS: &str =
    "subject_key, position, label, title, pinned_ms, opened_ms, seen_result, unavailable";

fn pin_row(r: &Row) -> rusqlite::Result<(String, Pin, Option<String>, Option<String>)> {
    Ok((
        r.get(0)?,
        Pin {
            subject: PinSubject::Pr(0),
            position: r.get(1)?,
            label: r.get(2)?,
            title: r.get(3)?,
            pinned_at: r.get(4)?,
            opened_at: r.get(5)?,
            seen_result: None,
            unavailable: None,
        },
        r.get(6)?,
        r.get(7)?,
    ))
}

fn finish_pin(
    (key, mut pin, result, unavailable): (String, Pin, Option<String>, Option<String>),
) -> Result<Pin> {
    pin.subject = parse("pin.subject_key", &key, PinSubject::parse_key)?;
    pin.seen_result = result
        .map(|r| parse("pin.seen_result", &r, RunResult::parse))
        .transpose()?;
    pin.unavailable = unavailable
        .map(|u| parse("pin.unavailable", &u, PinUnavailable::parse))
        .transpose()?;
    Ok(pin)
}

/// Every pin in pin order (FR-3).
pub fn pins(c: &Connection) -> Result<Vec<Pin>> {
    let mut st = c.prepare_cached(&format!("SELECT {PIN_COLS} FROM pin ORDER BY position"))?;
    let rows = st
        .query_map([], pin_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(finish_pin).collect()
}

pub fn pin(c: &Connection, subject: &PinSubject) -> Result<Option<Pin>> {
    let row = c
        .prepare_cached(&format!(
            "SELECT {PIN_COLS} FROM pin WHERE subject_key = ?1"
        ))?
        .query_row([subject.key()], pin_row)
        .optional()?;
    row.map(finish_pin).transpose()
}

/// Opens from the pins at or after `since` (the `⌘J` footer stat).
pub fn pin_opens_since(c: &Connection, since: Millis) -> Result<u32> {
    Ok(
        c.prepare_cached("SELECT count(*) FROM pin_open WHERE at_ms >= ?1")?
            .query_row([since], |r| r.get(0))?,
    )
}

/// Whether someone else caused an event on `subject` after `after`
/// (`sync_event_by_subject`): the change dot of a pinned PR or work item
/// (FR-10).
pub fn changed_since(c: &Connection, subject: &Subject, after: Millis) -> Result<bool> {
    Ok(c.prepare_cached(
        "SELECT 1 FROM sync_event
             WHERE subject_key = ?1 AND at_ms > ?2 AND self_caused = 0 LIMIT 1",
    )?
    .exists(params![subject.key(), after])?)
}

/// The newest `limit` runs of one definition, newest queued first
/// (`run_by_definition`).
pub fn definition_runs(
    c: &Connection,
    project_id: &str,
    definition_id: i64,
    limit: usize,
) -> Result<Vec<PipelineRun>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {RUN_COLS} FROM pipeline_run
         WHERE project_id = ?1 AND definition_id = ?2
         ORDER BY queued_ms DESC, id DESC LIMIT ?3"
    ))?;
    let rows = st
        .query_map(params![project_id, definition_id, limit as i64], run_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(finish_run).collect()
}

/// One definition (its name and folder).
pub fn pipeline(c: &Connection, project_id: &str, id: i64) -> Result<Option<Pipeline>> {
    Ok(c.prepare_cached(
        "SELECT id, project_id, name, folder FROM pipeline WHERE project_id = ?1 AND id = ?2",
    )?
    .query_row(params![project_id, id], |r| {
        Ok(Pipeline {
            id: r.get(0)?,
            project_id: r.get(1)?,
            name: r.get(2)?,
            folder: r.get(3)?,
        })
    })
    .optional()?)
}
