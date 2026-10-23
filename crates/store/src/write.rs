//! Idempotent upserts over any `&Connection`, normally the `&Transaction`
//! handed out by [`crate::Store::write`] (one sync page = one transaction).
//!
//! Replays are no-ops: rows are keyed on natural ADO ids and guarded by
//! monotonic fields (`rev` for work items, `updated_ms` for threads and
//! comments, the list fingerprint for PRs). `search_doc` (and through its
//! triggers `search_fts`) and `mention` rows are maintained in the same
//! statements' transaction.

use adoken_model::{
    Comment, MAX_PIN_LABEL_CHARS, MAX_PINS, Millis, Org, Person, Pin, PinSubject, PinUnavailable,
    Pipeline, PipelineRun, PrFileChange, PrIteration, PrThread, Project, PullRequest, Repo,
    RunResult, RunStage, RunStatus, StateCategory, SyncEvent, WorkItem, WorkItemComment,
    short_branch,
};
use adoken_search::DocKind;
use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::read::{self, Cursor, KIND_COMMENT, kind_code, mention_ids};

/// Outcome of an upsert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upsert {
    Inserted,
    Updated,
    Unchanged,
}

impl Upsert {
    pub fn changed(self) -> bool {
        self != Upsert::Unchanged
    }
}

struct Doc<'a> {
    kind: i64,
    key: &'a str,
    doc_id: i64,
    project_id: Option<&'a str>,
    title: &'a str,
    subtitle: &'a str,
    body: Option<&'a str>,
    updated_ms: Millis,
}

fn put_doc(c: &Connection, d: Doc<'_>) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO search_doc(kind, entity_key, doc_id, project_id, title, subtitle, body, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(entity_key) DO UPDATE SET
           doc_id = excluded.doc_id, project_id = excluded.project_id, title = excluded.title,
           subtitle = excluded.subtitle, body = excluded.body, updated_ms = excluded.updated_ms
         WHERE search_doc.title IS NOT excluded.title OR search_doc.subtitle IS NOT excluded.subtitle
            OR search_doc.body IS NOT excluded.body OR search_doc.doc_id IS NOT excluded.doc_id",
    )?
    .execute(params![
        d.kind,
        d.key,
        d.doc_id,
        d.project_id,
        d.title,
        d.subtitle,
        d.body,
        d.updated_ms
    ])?;
    Ok(())
}

fn name_of(c: &Connection, table: &str, id: &str) -> Result<String> {
    let sql = match table {
        "identity" => "SELECT display_name FROM identity WHERE id = ?1",
        "repo" => "SELECT name FROM repo WHERE id = ?1",
        _ => "SELECT name FROM project WHERE id = ?1",
    };
    Ok(c.prepare_cached(sql)?
        .query_row([id], |r| r.get(0))
        .optional()?
        .unwrap_or_default())
}

fn rowid_of(c: &Connection, table: &str, id: &str) -> Result<i64> {
    let sql = match table {
        "identity" => "SELECT rowid FROM identity WHERE id = ?1",
        "repo" => "SELECT rowid FROM repo WHERE id = ?1",
        _ => "SELECT rowid FROM project WHERE id = ?1",
    };
    Ok(c.prepare_cached(sql)?.query_row([id], |r| r.get(0))?)
}

// ── org structure ─────────────────────────────────────────────────────────

pub fn put_org(c: &Connection, org: &Org) -> Result<()> {
    c.execute("DELETE FROM org WHERE id <> ?1", [&org.id])?;
    c.prepare_cached(
        "INSERT INTO org(id, name, url) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, url = excluded.url",
    )?
    .execute(params![org.id, org.name, org.url])?;
    Ok(())
}

/// Upsert an identity. `is_me` only ever turns on; a payload without an
/// image URL keeps the stored one.
pub fn upsert_person(c: &Connection, p: &Person, is_me: bool) -> Result<Upsert> {
    let existed = read::person(c, &p.id)?;
    let same = |old: &Person| {
        old.id == p.id
            && old.display_name == p.display_name
            && old.unique_name == p.unique_name
            && old.is_group == p.is_group
            && (p.image_url.is_none() || old.image_url == p.image_url)
    };
    if existed.as_ref().is_some_and(same) && !is_me {
        return Ok(Upsert::Unchanged);
    }
    c.prepare_cached(
        "INSERT INTO identity(id, display_name, unique_name, is_me, is_group, image_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name,
           unique_name = coalesce(excluded.unique_name, identity.unique_name),
           is_me = max(identity.is_me, excluded.is_me), is_group = excluded.is_group,
           image_url = coalesce(excluded.image_url, identity.image_url)",
    )?
    .execute(params![
        p.id,
        p.display_name,
        p.unique_name,
        is_me,
        p.is_group,
        p.image_url
    ])?;
    if !p.is_group {
        let rowid = rowid_of(c, "identity", &p.id)?;
        put_doc(
            c,
            Doc {
                kind: kind_code(DocKind::Person),
                key: &format!("person:{}", p.id),
                doc_id: rowid,
                project_id: None,
                title: &p.display_name,
                subtitle: p.unique_name.as_deref().unwrap_or(""),
                body: None,
                updated_ms: 0,
            },
        )?;
    }
    Ok(match existed {
        None => Upsert::Inserted,
        Some(old) if same(&old) => Upsert::Unchanged,
        Some(_) => Upsert::Updated,
    })
}

pub fn upsert_project(c: &Connection, p: &Project) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO project(id, name, synced) VALUES (?1, ?2, 1)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, synced = 1
         WHERE project.name IS NOT excluded.name OR project.synced = 0",
    )?
    .execute(params![p.id, p.name])?;
    put_doc(
        c,
        Doc {
            kind: kind_code(DocKind::Project),
            key: &format!("project:{}", p.id),
            doc_id: rowid_of(c, "project", &p.id)?,
            project_id: Some(&p.id),
            title: &p.name,
            subtitle: "Project",
            body: None,
            updated_ms: 0,
        },
    )
}

/// A project the user didn't select, known only because a pinned item lives
/// in it (spec 0014 FR-11): stored (`synced = 0`) so that item's rows can
/// reference it, and never synced as a whole. A selected project stays
/// selected.
pub fn upsert_project_ref(c: &Connection, p: &Project) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO project(id, name, synced) VALUES (?1, ?2, 0)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name WHERE project.name IS NOT excluded.name",
    )?
    .execute(params![p.id, p.name])?;
    Ok(())
}

pub fn upsert_repo(c: &Connection, r: &Repo) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO repo(id, project_id, name, default_branch) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, name = excluded.name,
           default_branch = excluded.default_branch",
    )?
    .execute(params![r.id, r.project_id, r.name, r.default_branch])?;
    let project = name_of(c, "project", &r.project_id)?;
    put_doc(
        c,
        Doc {
            kind: kind_code(DocKind::Repo),
            key: &format!("repo:{}", r.id),
            doc_id: rowid_of(c, "repo", &r.id)?,
            project_id: Some(&r.project_id),
            title: &r.name,
            subtitle: &project,
            body: None,
            updated_ms: 0,
        },
    )
}

// ── pull requests ─────────────────────────────────────────────────────────

/// FNV-1a over the list-level fields sync uses for change detection
/// (spec 0002 FR-6): status, draft, merge status, source commit, title,
/// target, reviewers and votes.
pub fn pr_fingerprint(pr: &PullRequest) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes.iter().chain(std::iter::once(&0xff)) {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    eat(pr.status.as_str().as_bytes());
    eat(&[pr.is_draft as u8]);
    eat(pr.merge_status.as_str().as_bytes());
    eat(pr.last_source_commit.as_deref().unwrap_or("").as_bytes());
    eat(pr.title.as_bytes());
    eat(pr.target_branch.as_bytes());
    let mut reviewers: Vec<_> = pr
        .reviewers
        .iter()
        .map(|r| {
            (
                r.person.as_str(),
                r.vote.as_i8(),
                r.is_required,
                r.has_declined,
            )
        })
        .collect();
    reviewers.sort_unstable();
    for (id, vote, req, declined) in reviewers {
        eat(id.as_bytes());
        eat(&[vote as u8, req as u8, declined as u8]);
    }
    h as i64
}

/// Upsert a PR from a list payload. `Unchanged` when the fingerprint and the
/// derived timestamps match the stored row. A stored description longer than
/// the list API's 400-char truncation survives list refreshes.
pub fn upsert_pr(c: &Connection, pr: &PullRequest) -> Result<Upsert> {
    let fingerprint = pr_fingerprint(pr);
    let old: Option<(i64, Millis, Option<Millis>)> = c
        .prepare_cached(
            "SELECT fingerprint, updated_ms, closed_ms FROM pull_request WHERE id = ?1",
        )?
        .query_row([pr.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    if let Some((fp, updated, closed)) = old
        && fp == fingerprint
        && updated >= pr.updated
        && closed == pr.closed
    {
        return Ok(Upsert::Unchanged);
    }
    c.prepare_cached(
        "INSERT INTO pull_request(id, repo_id, project_id, title, description, status, is_draft,
           author_id, source_ref, target_ref, merge_status, last_source_commit, iterations,
           last_push_ms, created_ms, updated_ms, closed_ms, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         ON CONFLICT(id) DO UPDATE SET repo_id = excluded.repo_id, project_id = excluded.project_id,
           title = excluded.title,
           description = CASE WHEN length(excluded.description) >= 400
                               AND substr(pull_request.description, 1, length(excluded.description)) = excluded.description
                              THEN pull_request.description ELSE excluded.description END,
           status = excluded.status, is_draft = excluded.is_draft, author_id = excluded.author_id,
           source_ref = excluded.source_ref, target_ref = excluded.target_ref,
           merge_status = excluded.merge_status, last_source_commit = excluded.last_source_commit,
           iterations = max(pull_request.iterations, excluded.iterations),
           last_push_ms = coalesce(excluded.last_push_ms, pull_request.last_push_ms),
           created_ms = excluded.created_ms,
           updated_ms = max(pull_request.updated_ms, excluded.updated_ms),
           closed_ms = excluded.closed_ms, fingerprint = excluded.fingerprint",
    )?
    .execute(params![
        pr.id,
        pr.repo_id,
        pr.project_id,
        pr.title,
        pr.description,
        pr.status.as_str(),
        pr.is_draft,
        pr.author,
        pr.source_branch,
        pr.target_branch,
        pr.merge_status.as_str(),
        pr.last_source_commit,
        pr.iterations,
        pr.last_push,
        pr.created,
        pr.updated,
        pr.closed,
        fingerprint,
    ])?;
    c.prepare_cached("DELETE FROM pr_reviewer WHERE pr_id = ?1")?
        .execute([pr.id])?;
    let mut ins = c.prepare_cached(
        "INSERT INTO pr_reviewer(pr_id, identity_id, vote, is_required, has_declined, added_ms,
           voted_ms, ord) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(pr_id, identity_id) DO NOTHING",
    )?;
    for (ord, r) in pr.reviewers.iter().enumerate() {
        ins.execute(params![
            pr.id,
            r.person,
            r.vote.as_i8(),
            r.is_required,
            r.has_declined,
            r.added_at,
            r.voted_at,
            ord as i64
        ])?;
    }
    let repo = name_of(c, "repo", &pr.repo_id)?;
    let author = name_of(c, "identity", &pr.author)?;
    let subtitle = format!(
        "{repo} · {author} · {} · {}",
        if pr.is_draft && pr.status == adoken_model::PrStatus::Active {
            "draft"
        } else {
            pr.status.as_str()
        },
        short_branch(&pr.source_branch)
    );
    let description = c
        .prepare_cached("SELECT description FROM pull_request WHERE id = ?1")?
        .query_row([pr.id], |r| r.get::<_, String>(0))?;
    put_doc(
        c,
        Doc {
            kind: kind_code(DocKind::PullRequest),
            key: &format!("pr:{}", pr.id),
            doc_id: pr.id,
            project_id: Some(&pr.project_id),
            title: &pr.title,
            subtitle: &subtitle,
            body: Some(&description),
            updated_ms: pr.updated,
        },
    )?;
    Ok(if old.is_some() {
        Upsert::Updated
    } else {
        Upsert::Inserted
    })
}

/// Full description from the single-PR endpoint (hydrate).
pub fn set_pr_description(c: &Connection, pr_id: i64, description: &str) -> Result<()> {
    c.prepare_cached("UPDATE pull_request SET description = ?2 WHERE id = ?1")?
        .execute(params![pr_id, description])?;
    c.prepare_cached("UPDATE search_doc SET body = ?2 WHERE entity_key = ?1 AND body IS NOT ?2")?
        .execute(params![format!("pr:{pr_id}"), description])?;
    Ok(())
}

pub fn mark_pr_hydrated(c: &Connection, pr_id: i64, at: Millis) -> Result<()> {
    c.prepare_cached("UPDATE pull_request SET hydrated_ms = ?2 WHERE id = ?1")?
        .execute(params![pr_id, at])?;
    Ok(())
}

/// Raise `updated_ms` (e.g. a new comment is PR activity).
pub fn touch_pr(c: &Connection, pr_id: i64, at: Millis) -> Result<()> {
    c.prepare_cached("UPDATE pull_request SET updated_ms = ?2 WHERE id = ?1 AND updated_ms < ?2")?
        .execute(params![pr_id, at])?;
    Ok(())
}

pub fn put_pr_iterations(c: &Connection, pr_id: i64, its: &[PrIteration]) -> Result<()> {
    let mut st = c.prepare_cached(
        "INSERT INTO pr_iteration(pr_id, id, source_commit, created_ms, description,
           commit_count, commits_truncated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(pr_id, id) DO UPDATE SET source_commit = excluded.source_commit,
           created_ms = excluded.created_ms, description = excluded.description,
           commits_truncated = CASE WHEN excluded.commit_count IS NULL
             THEN pr_iteration.commits_truncated ELSE excluded.commits_truncated END,
           commit_count = coalesce(excluded.commit_count, pr_iteration.commit_count)",
    )?;
    for it in its {
        st.execute(params![
            pr_id,
            it.id,
            it.source_commit,
            it.created,
            it.description,
            it.commit_count,
            it.commits_truncated
        ])?;
    }
    Ok(())
}

/// Replace the changed-file list of a PR.
pub fn put_pr_files(c: &Connection, pr_id: i64, files: &[PrFileChange]) -> Result<()> {
    c.prepare_cached("DELETE FROM pr_file WHERE pr_id = ?1")?
        .execute([pr_id])?;
    let mut st = c.prepare_cached(
        "INSERT INTO pr_file(pr_id, ord, path, original_path, change_type, old_blob, new_blob,
           change_tracking_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (ord, f) in files.iter().enumerate() {
        st.execute(params![
            pr_id,
            ord as i64,
            f.path,
            f.original_path,
            f.change_type.as_str(),
            f.old_content,
            f.new_content,
            f.change_tracking_id
        ])?;
    }
    Ok(())
}

/// Record which iteration `pr_file` now describes (spec 0004 FR-1). Call in
/// the same transaction as [`put_pr_files`].
pub fn put_pr_file_sync(c: &Connection, pr_id: i64, sync: &read::FileSync) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO pr_file_sync(pr_id, iteration, source_commit, base_commit, synced_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(pr_id) DO UPDATE SET iteration = excluded.iteration,
           source_commit = excluded.source_commit, base_commit = excluded.base_commit,
           synced_ms = excluded.synced_ms",
    )?
    .execute(params![
        pr_id,
        sync.iteration,
        sync.source_commit,
        sync.base_commit,
        sync.synced_ms
    ])?;
    Ok(())
}

/// Upsert a thread and its comments. Returns the ADO ids of comments that
/// were not stored before (callers derive `PrCommentAdded` etc. from them).
pub fn upsert_thread(c: &Connection, t: &PrThread) -> Result<Vec<i64>> {
    let a = t.anchor.as_ref();
    c.prepare_cached(
        "INSERT INTO pr_thread(pr_id, id, status, file_path, right_start, right_end, left_start,
           left_end, iteration, iteration_blob, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(pr_id, id) DO UPDATE SET status = excluded.status,
           file_path = excluded.file_path, right_start = excluded.right_start,
           right_end = excluded.right_end, left_start = excluded.left_start,
           left_end = excluded.left_end, iteration = excluded.iteration,
           iteration_blob = excluded.iteration_blob, updated_ms = excluded.updated_ms
         WHERE excluded.updated_ms > pr_thread.updated_ms",
    )?
    .execute(params![
        t.pr_id,
        t.id,
        t.status.as_str(),
        a.map(|a| &a.file_path),
        a.and_then(|a| a.right).map(|r| r.start),
        a.and_then(|a| a.right).map(|r| r.end),
        a.and_then(|a| a.left).map(|r| r.start),
        a.and_then(|a| a.left).map(|r| r.end),
        t.iteration,
        a.and_then(|a| a.iteration_blob.as_deref()),
        t.updated,
    ])?;
    let mut new = Vec::new();
    for cm in &t.comments {
        if upsert_comment(c, "pr", t.pr_id, t.id, cm, &[])? == Upsert::Inserted {
            new.push(cm.id);
        }
    }
    Ok(new)
}

/// `mentions`: people mentioned beyond the body's `@<guid>` tokens.
fn upsert_comment(
    c: &Connection,
    source: &str,
    parent: i64,
    thread: i64,
    cm: &Comment,
    mentions: &[String],
) -> Result<Upsert> {
    let old: Option<(i64, Millis)> = c
        .prepare_cached(
            "SELECT id, updated_ms FROM comment
             WHERE source = ?1 AND parent_id = ?2 AND thread_id = ?3 AND ado_comment_id = ?4",
        )?
        .query_row(params![source, parent, thread, cm.id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let rowid = match old {
        Some((_, updated)) if updated >= cm.updated => return Ok(Upsert::Unchanged),
        Some((rowid, _)) => {
            c.prepare_cached(
                "UPDATE comment SET body = ?2, updated_ms = ?3, is_deleted = ?4, reply_to = ?5
                 WHERE id = ?1",
            )?
            .execute(params![
                rowid,
                cm.body,
                cm.updated,
                cm.is_deleted,
                cm.parent_id
            ])?;
            rowid
        }
        None => {
            c.prepare_cached(
                "INSERT INTO comment(source, parent_id, thread_id, ado_comment_id, reply_to,
                   author_id, body, is_system, created_ms, updated_ms, is_deleted)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?
            .execute(params![
                source,
                parent,
                thread,
                cm.id,
                cm.parent_id,
                cm.author,
                cm.body,
                cm.is_system,
                cm.created,
                cm.updated,
                cm.is_deleted
            ])?;
            c.last_insert_rowid()
        }
    };
    c.prepare_cached("DELETE FROM mention WHERE comment_id = ?1")?
        .execute([rowid])?;
    if !cm.is_deleted {
        let mut ins = c.prepare_cached(
            "INSERT INTO mention(comment_id, identity_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
        )?;
        for id in mention_ids(&cm.body)
            .into_iter()
            .chain(mentions.iter().map(String::as_str))
        {
            ins.execute(params![rowid, id])?;
        }
    }
    if !cm.is_system {
        let (parent_key, title_sql) = if source == "pr" {
            (
                format!("pr:{parent}"),
                "SELECT title FROM pull_request WHERE id = ?1",
            )
        } else {
            (
                format!("wi:{parent}"),
                "SELECT title FROM work_item WHERE id = ?1",
            )
        };
        let title: String = c
            .prepare_cached(title_sql)?
            .query_row([parent], |r| r.get(0))
            .optional()?
            .unwrap_or_default();
        let key = format!("{parent_key}/c:{rowid}");
        if cm.is_deleted {
            c.prepare_cached("DELETE FROM search_doc WHERE entity_key = ?1")?
                .execute([&key])?;
        } else {
            put_doc(
                c,
                Doc {
                    kind: KIND_COMMENT,
                    key: &key,
                    doc_id: rowid,
                    project_id: None,
                    title: &title,
                    subtitle: "",
                    body: Some(&cm.body),
                    updated_ms: cm.updated,
                },
            )?;
        }
    }
    Ok(if old.is_some() {
        Upsert::Updated
    } else {
        Upsert::Inserted
    })
}

pub fn upsert_wi_comment(c: &Connection, cm: &WorkItemComment) -> Result<Upsert> {
    let comment = Comment {
        id: cm.id,
        parent_id: None,
        author: cm.author.clone(),
        body: cm.body.clone(),
        created: cm.created,
        updated: cm.updated,
        is_system: false,
        is_deleted: false,
    };
    let upsert = upsert_comment(c, "wi", cm.work_item_id, 0, &comment, &cm.mentions)?;
    // Spec 0015 FR-34: the raw text, also for rows stored before 0015.
    if let Some(source) = &cm.source {
        c.prepare_cached(
            "UPDATE comment SET body_source = ?3, format = ?4
             WHERE source = 'wi' AND parent_id = ?1 AND thread_id = 0 AND ado_comment_id = ?2
               AND (body_source IS NOT ?3 OR format IS NOT ?4)",
        )?
        .execute(params![cm.work_item_id, cm.id, source, cm.format.as_str()])?;
    }
    Ok(upsert)
}

/// Mark a work item's stored comments that aren't in `keep` (ADO ids)
/// deleted: the discussion list omits deleted comments.
pub fn retain_wi_comments(c: &Connection, work_item_id: i64, keep: &[i64]) -> Result<()> {
    for id in read::wi_comment_ids(c, work_item_id)? {
        if keep.contains(&id) {
            continue;
        }
        let rowid: i64 = c
            .prepare_cached(
                "UPDATE comment SET is_deleted = 1
                 WHERE source = 'wi' AND parent_id = ?1 AND thread_id = 0 AND ado_comment_id = ?2
                 RETURNING id",
            )?
            .query_row(params![work_item_id, id], |r| r.get(0))?;
        c.prepare_cached("DELETE FROM mention WHERE comment_id = ?1")?
            .execute([rowid])?;
        c.prepare_cached("DELETE FROM search_doc WHERE entity_key = ?1")?
            .execute([format!("wi:{work_item_id}/c:{rowid}")])?;
    }
    Ok(())
}

/// Flag (or clear) a work item's discussion for fetching.
pub fn set_wi_comments_due(c: &Connection, work_item_id: i64, due: bool) -> Result<()> {
    c.prepare_cached("UPDATE work_item SET comments_due = ?2 WHERE id = ?1")?
        .execute(params![work_item_id, due])?;
    Ok(())
}

// ── work items ────────────────────────────────────────────────────────────

/// Rev-guarded upsert: only a strictly newer `rev` changes the row.
pub fn upsert_work_item(c: &Connection, wi: &WorkItem) -> Result<Upsert> {
    upsert_work_item_with_transitions(c, wi, Transitions::default())
}

/// Transition dates the wire carries beside a work item's fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transitions {
    /// `ClosedDate`, else `StateChangeDate`: refines the first observed
    /// closure.
    pub closed_at: Option<Millis>,
    /// `StateChangeDate`: when the item entered its current state (spec
    /// 0017 "Open Nd").
    pub state_changed: Option<Millis>,
}

/// Wire transition dates refine the first observed closure, without moving
/// an already recorded terminal transition when another field changes.
/// `state_since_ms` takes the wire's `StateChangeDate`; without one, a state
/// change observed here starts at the item's `changed` time and an
/// unchanged state keeps what was recorded.
pub fn upsert_work_item_with_transitions(
    c: &Connection,
    wi: &WorkItem,
    transitions: Transitions,
) -> Result<Upsert> {
    let old_rev = read::work_item_rev(c, wi.id)?;
    if old_rev.is_some_and(|r| r >= wi.rev) {
        return Ok(Upsert::Unchanged);
    }
    // The type's fetched states know the real category of custom states.
    let category = crate::wi::state_category(c, &wi.project_id, wi.kind.as_str(), &wi.state)?
        .unwrap_or(wi.state_category);
    c.prepare_cached(
        "INSERT INTO work_item(id, project_id, rev, type, title, state, state_category,
           assigned_to_id, created_by_id, changed_by_id, area_path, iteration_path, priority, tags,
           parent_id, description, comment_count, created_ms, changed_ms, terminal_since_ms,
           state_since_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
         ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, rev = excluded.rev,
           type = excluded.type, title = excluded.title, state = excluded.state,
           state_category = excluded.state_category, assigned_to_id = excluded.assigned_to_id,
           created_by_id = excluded.created_by_id, changed_by_id = excluded.changed_by_id,
           area_path = excluded.area_path, iteration_path = excluded.iteration_path,
           priority = excluded.priority, tags = excluded.tags, parent_id = excluded.parent_id,
           description = coalesce(excluded.description, work_item.description),
           comment_count = excluded.comment_count, created_ms = excluded.created_ms,
           terminal_since_ms = CASE
             WHEN excluded.state_category NOT IN ('Completed', 'Removed') THEN NULL
             WHEN work_item.state_category IN ('Completed', 'Removed')
               THEN coalesce(work_item.terminal_since_ms, excluded.terminal_since_ms)
             ELSE excluded.terminal_since_ms END,
           state_since_ms = CASE
             WHEN excluded.state_since_ms IS NOT NULL THEN excluded.state_since_ms
             WHEN excluded.state IS NOT work_item.state THEN excluded.changed_ms
             ELSE work_item.state_since_ms END,
           changed_ms = excluded.changed_ms, is_deleted = 0
         WHERE excluded.rev > work_item.rev",
    )?
    .execute(params![
        wi.id,
        wi.project_id,
        wi.rev,
        wi.kind.as_str(),
        wi.title,
        wi.state,
        category.as_str(),
        wi.assigned_to,
        wi.created_by,
        wi.changed_by,
        wi.area_path,
        wi.iteration_path,
        wi.priority,
        wi.tags.join("; "),
        wi.parent_id,
        wi.description,
        wi.comment_count,
        wi.created,
        wi.changed,
        matches!(category, StateCategory::Completed | StateCategory::Removed)
            .then_some(transitions.closed_at.unwrap_or(wi.changed)),
        transitions.state_changed,
    ])?;
    c.prepare_cached("DELETE FROM wi_pr_link WHERE work_item_id = ?1")?
        .execute([wi.id])?;
    let mut link = c.prepare_cached(
        "INSERT INTO wi_pr_link(work_item_id, pr_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
    )?;
    for pr in &wi.linked_pr_ids {
        link.execute(params![wi.id, pr])?;
    }
    let project = name_of(c, "project", &wi.project_id)?;
    let assignee = match &wi.assigned_to {
        Some(id) => name_of(c, "identity", id)?,
        None => "Unassigned".to_owned(),
    };
    let subtitle = format!(
        "{} · {} · {project} · {assignee}",
        wi.kind.as_str(),
        wi.state
    );
    put_doc(
        c,
        Doc {
            kind: kind_code(DocKind::WorkItem),
            key: &format!("wi:{}", wi.id),
            doc_id: wi.id,
            project_id: Some(&wi.project_id),
            title: &wi.title,
            subtitle: &subtitle,
            body: wi.description.as_deref(),
            updated_ms: wi.changed,
        },
    )?;
    Ok(if old_rev.is_some() {
        Upsert::Updated
    } else {
        Upsert::Inserted
    })
}

/// Soft-delete (reporting feed `includeDeleted`).
pub fn delete_work_item(c: &Connection, id: i64, rev: i64) -> Result<()> {
    c.prepare_cached("UPDATE work_item SET is_deleted = 1, rev = ?2 WHERE id = ?1 AND rev < ?2")?
        .execute(params![id, rev])?;
    c.prepare_cached("DELETE FROM search_doc WHERE entity_key = ?1")?
        .execute([format!("wi:{id}")])?;
    Ok(())
}

// ── pipelines ─────────────────────────────────────────────────────────────

pub fn upsert_pipeline(c: &Connection, p: &Pipeline) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO pipeline(project_id, id, name, folder) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(project_id, id) DO UPDATE SET name = excluded.name, folder = excluded.folder",
    )?
    .execute(params![p.project_id, p.id, p.name, p.folder])?;
    let project = name_of(c, "project", &p.project_id)?;
    let subtitle = if p.folder == "\\" || p.folder.is_empty() {
        format!("Pipeline · {project}")
    } else {
        format!(
            "Pipeline · {project} · {}",
            p.folder.trim_start_matches('\\')
        )
    };
    put_doc(
        c,
        Doc {
            kind: kind_code(DocKind::Pipeline),
            key: &format!("pipe:{}:{}", p.project_id, p.id),
            doc_id: p.id,
            project_id: Some(&p.project_id),
            title: &p.name,
            subtitle: &subtitle,
            body: None,
            updated_ms: 0,
        },
    )
}

/// Upsert a run; `Unchanged` when status/result/timestamps are the same.
pub fn upsert_run(c: &Connection, r: &PipelineRun) -> Result<Upsert> {
    let old = read::run(c, r.id)?;
    if old.as_ref() == Some(r) {
        return Ok(Upsert::Unchanged);
    }
    c.prepare_cached(
        "INSERT INTO pipeline_run(id, project_id, definition_id, definition_name, run_number,
           status, result, reason, source_branch, source_version, requested_for_id, pr_id,
           queued_ms, started_ms, finished_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET status = excluded.status, result = excluded.result,
           definition_name = excluded.definition_name, run_number = excluded.run_number,
           source_version = excluded.source_version, started_ms = excluded.started_ms,
           finished_ms = excluded.finished_ms",
    )?
    .execute(params![
        r.id,
        r.project_id,
        r.pipeline_id,
        r.pipeline_name,
        r.run_number,
        r.status.as_str(),
        r.result.map(|x| x.as_str()),
        r.reason.as_str(),
        r.branch,
        r.source_version,
        r.requested_for,
        r.pr_id,
        r.queued,
        r.started,
        r.finish,
    ])?;
    Ok(if old.is_some() {
        Upsert::Updated
    } else {
        Upsert::Inserted
    })
}

/// Replace a run's stages with the timeline fetched while the run looked
/// like `seen`. The stored stages count as current only while the row still
/// matches `seen` and `seen` is completed; otherwise the run stays due
/// ([`read::runs_timeline_due`]).
pub fn put_run_stages(c: &Connection, seen: &PipelineRun, stages: &[RunStage]) -> Result<()> {
    c.prepare_cached("DELETE FROM pipelines_stage WHERE run_id = ?1")?
        .execute([seen.id])?;
    let mut ins = c.prepare_cached(
        "INSERT INTO pipelines_stage(run_id, ord, name, outcome, started_ms, finished_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (ord, s) in stages.iter().enumerate() {
        ins.execute(params![
            seen.id,
            ord as i64,
            s.name,
            s.outcome.as_str(),
            s.started,
            s.finished
        ])?;
    }
    let fp = if seen.status == RunStatus::Completed {
        timeline_fp(seen)
    } else {
        "live".to_owned()
    };
    c.prepare_cached("UPDATE pipeline_run SET timeline_fp = ?2 WHERE id = ?1")?
        .execute(params![seen.id, fp])?;
    Ok(())
}

/// Same value as the `run_timeline_due` index expression.
fn timeline_fp(r: &PipelineRun) -> String {
    format!(
        "{}:{}:{}",
        r.status.as_str(),
        r.result.map(|x| x.as_str()).unwrap_or(""),
        r.finish.map(|f| f.to_string()).unwrap_or_default()
    )
}

// ── events, cursors, meta ─────────────────────────────────────────────────

/// Persist an event; `None` when `(kind, subject, dedupe_key)` already exists.
pub fn insert_event(c: &Connection, e: &SyncEvent) -> Result<Option<i64>> {
    let n = c
        .prepare_cached(
            "INSERT INTO sync_event(kind, subject_key, dedupe_key, actor_id, self_caused, at_ms, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(kind, subject_key, dedupe_key) DO NOTHING",
        )?
        .execute(params![
            e.kind.as_str(),
            e.subject.key(),
            e.dedupe_key,
            e.actor,
            e.self_caused,
            e.at,
            e.summary
        ])?;
    Ok((n == 1).then(|| c.last_insert_rowid()))
}

pub fn put_cursor(c: &Connection, cur: &Cursor) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO sync_cursor(scope, project_id, token, high_water, backfill_to, baseline_done,
           last_ok_ms, last_error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(scope, project_id) DO UPDATE SET token = excluded.token,
           high_water = excluded.high_water, backfill_to = excluded.backfill_to,
           baseline_done = excluded.baseline_done, last_ok_ms = excluded.last_ok_ms,
           last_error = excluded.last_error",
    )?
    .execute(params![
        cur.scope,
        cur.project_id,
        cur.token,
        cur.high_water,
        cur.backfill_to,
        cur.baseline_done,
        cur.last_ok_ms,
        cur.last_error
    ])?;
    Ok(())
}

/// Record `lane`'s last successful pass (spec 0016 FR-10); never moves it
/// back in time.
pub fn put_lane_sync(c: &Connection, lane: &str, last_ok_ms: Millis) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO lane_sync(lane, last_ok_ms) VALUES (?1, ?2)
         ON CONFLICT(lane) DO UPDATE SET last_ok_ms = max(last_ok_ms, excluded.last_ok_ms)",
    )?
    .execute(params![lane, last_ok_ms])?;
    Ok(())
}

pub fn set_meta(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )?
    .execute(params![key, value])?;
    Ok(())
}

/// Local view history (hot-set input: "PRs I opened in adoken").
pub fn mark_viewed(c: &Connection, entity_key: &str, at: Millis) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO view_state(entity_key, last_viewed_ms) VALUES (?1, ?2)
         ON CONFLICT(entity_key) DO UPDATE SET last_viewed_ms = max(last_viewed_ms, excluded.last_viewed_ms)",
    )?
    .execute(params![entity_key, at])?;
    Ok(())
}

/// I set auto-complete on `pr_id` in adoken, having reviewed `source_commit`
/// (spec 0004 FR-12). Setting it again re-arms the guard at the new commit.
pub fn set_auto_complete(
    c: &Connection,
    pr_id: i64,
    source_commit: &str,
    at: Millis,
) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO pr_auto_complete(pr_id, source_commit, set_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(pr_id) DO UPDATE SET source_commit = excluded.source_commit,
           set_ms = excluded.set_ms",
    )?
    .execute(params![pr_id, source_commit, at])?;
    Ok(())
}

/// Auto-complete cancelled, or the PR completed / was abandoned.
pub fn clear_auto_complete(c: &Connection, pr_id: i64) -> Result<()> {
    c.prepare_cached("DELETE FROM pr_auto_complete WHERE pr_id = ?1")?
        .execute([pr_id])?;
    Ok(())
}

/// Azure DevOps accepted my Complete of `pr_id` at `source_commit` and is
/// merging it asynchronously (spec 0004 FR-10). Completing again re-arms it.
pub fn set_completing(c: &Connection, pr_id: i64, source_commit: &str, at: Millis) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO pr_completing(pr_id, source_commit, set_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(pr_id) DO UPDATE SET source_commit = excluded.source_commit,
           set_ms = excluded.set_ms",
    )?
    .execute(params![pr_id, source_commit, at])?;
    Ok(())
}

/// The queued merge finished: the PR closed, or the merge failed.
pub fn clear_completing(c: &Connection, pr_id: i64) -> Result<()> {
    c.prepare_cached("DELETE FROM pr_completing WHERE pr_id = ?1")?
        .execute([pr_id])?;
    Ok(())
}

// ── pins (spec 0014, local) ──────────────────────────────────────────────

/// Outcome of adding a pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinOutcome {
    Added,
    AlreadyPinned,
    /// [`MAX_PINS`] reached; nothing changed (FR-3).
    Full,
}

fn pin_count(c: &Connection) -> Result<u32> {
    Ok(c.prepare_cached("SELECT count(*) FROM pin")?
        .query_row([], |r| r.get(0))?)
}

/// Positions stay dense `0..n` in pin order.
fn renumber_pins(c: &Connection) -> Result<()> {
    let keys: Vec<String> = c
        .prepare_cached("SELECT subject_key FROM pin ORDER BY position, pinned_ms")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut st = c.prepare_cached("UPDATE pin SET position = ?2 WHERE subject_key = ?1")?;
    for (ix, key) in keys.iter().enumerate() {
        st.execute(params![key, ix as i64])?;
    }
    Ok(())
}

fn insert_pin(c: &Connection, pin: &Pin, position: u32) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO pin(subject_key, position, label, title, pinned_ms, opened_ms,
           seen_result, unavailable)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?
    .execute(params![
        pin.subject.key(),
        position,
        pin.label,
        pin.title,
        pin.pinned_at,
        pin.opened_at,
        pin.seen_result.map(|r| r.as_str()),
        pin.unavailable.map(|u| u.as_str()),
    ])?;
    Ok(())
}

/// Appends `pin` (its `position` is ignored), unless it's pinned already or
/// [`MAX_PINS`] are (FR-3).
pub fn add_pin(c: &Connection, pin: &Pin) -> Result<PinOutcome> {
    if read::pin(c, &pin.subject)?.is_some() {
        return Ok(PinOutcome::AlreadyPinned);
    }
    let count = pin_count(c)?;
    if count as usize >= MAX_PINS {
        return Ok(PinOutcome::Full);
    }
    insert_pin(c, pin, count)?;
    Ok(PinOutcome::Added)
}

/// Undo of an unpin: puts `pin` back at its old position with its label
/// (FR-2), moving later pins down.
pub fn restore_pin(c: &Connection, pin: &Pin) -> Result<PinOutcome> {
    if read::pin(c, &pin.subject)?.is_some() {
        return Ok(PinOutcome::AlreadyPinned);
    }
    let count = pin_count(c)?;
    if count as usize >= MAX_PINS {
        return Ok(PinOutcome::Full);
    }
    let position = pin.position.min(count);
    c.prepare_cached("UPDATE pin SET position = position + 1 WHERE position >= ?1")?
        .execute([position])?;
    insert_pin(c, pin, position)?;
    Ok(PinOutcome::Added)
}

/// Removes the pin, returning it as it was (for Undo).
pub fn unpin(c: &Connection, subject: &PinSubject) -> Result<Option<Pin>> {
    let Some(pin) = read::pin(c, subject)? else {
        return Ok(None);
    };
    c.prepare_cached("DELETE FROM pin WHERE subject_key = ?1")?
        .execute([subject.key()])?;
    renumber_pins(c)?;
    Ok(Some(pin))
}

/// Moves the pin to position `to` (clamped), shifting the pins between;
/// returns its new position.
pub fn move_pin(c: &Connection, subject: &PinSubject, to: u32) -> Result<Option<u32>> {
    let Some(pin) = read::pin(c, subject)? else {
        return Ok(None);
    };
    let last = pin_count(c)?.saturating_sub(1);
    let to = to.min(last);
    let from = pin.position;
    if to < from {
        c.prepare_cached(
            "UPDATE pin SET position = position + 1 WHERE position >= ?1 AND position < ?2",
        )?
        .execute([to, from])?;
    } else if to > from {
        c.prepare_cached(
            "UPDATE pin SET position = position - 1 WHERE position > ?1 AND position <= ?2",
        )?
        .execute([from, to])?;
    }
    c.prepare_cached("UPDATE pin SET position = ?2 WHERE subject_key = ?1")?
        .execute(params![subject.key(), to])?;
    Ok(Some(to))
}

/// Sets the local label (FR-14): trimmed, at most [`MAX_PIN_LABEL_CHARS`]
/// characters; empty clears it (the Azure DevOps title shows again).
pub fn rename_pin(c: &Connection, subject: &PinSubject, label: &str) -> Result<bool> {
    let label: String = label.trim().chars().take(MAX_PIN_LABEL_CHARS).collect();
    let label = (!label.is_empty()).then_some(label);
    Ok(
        c.prepare_cached("UPDATE pin SET label = ?2 WHERE subject_key = ?1")?
            .execute(params![subject.key(), label])?
            > 0,
    )
}

/// Establish a definition's first result baseline without recording an open.
pub fn initialize_pin_result(
    c: &Connection,
    subject: &PinSubject,
    result: RunResult,
) -> Result<bool> {
    Ok(c.prepare_cached(
        "UPDATE pin SET seen_result = ?2 WHERE subject_key = ?1 AND seen_result IS NULL",
    )?
    .execute(params![subject.key(), result.as_str()])?
        > 0)
}

/// Opening clears the change dot (FR-7, FR-10): records the time and, for
/// definitions, the result now shown. `from_pins` counts the open for the
/// `⌘J` footer stat (a 7-day window; older rows are pruned here).
pub fn mark_pin_opened(
    c: &Connection,
    subject: &PinSubject,
    at: Millis,
    seen_result: Option<RunResult>,
    from_pins: bool,
) -> Result<bool> {
    let changed = c
        .prepare_cached(
            "UPDATE pin SET opened_ms = ?2, seen_result = COALESCE(?3, seen_result)
             WHERE subject_key = ?1",
        )?
        .execute(params![subject.key(), at, seen_result.map(|r| r.as_str())])?
        > 0;
    if changed && from_pins {
        c.prepare_cached("INSERT INTO pin_open(at_ms) VALUES (?1)")?
            .execute([at])?;
        c.prepare_cached("DELETE FROM pin_open WHERE at_ms < ?1")?
            .execute([at - 7 * adoken_model::time::DAY])?;
    }
    Ok(changed)
}

/// Sync's view of a pinned item: readable again (`None`) or 404 / 403
/// (FR-13). Never removes the pin.
pub fn set_pin_unavailable(
    c: &Connection,
    subject: &PinSubject,
    unavailable: Option<PinUnavailable>,
) -> Result<()> {
    c.prepare_cached("UPDATE pin SET unavailable = ?2 WHERE subject_key = ?1")?
        .execute(params![subject.key(), unavailable.map(|u| u.as_str())])?;
    Ok(())
}

/// The latest Azure DevOps title of a pinned item (kept for "Not available").
pub fn set_pin_title(c: &Connection, subject: &PinSubject, title: &str) -> Result<()> {
    c.prepare_cached("UPDATE pin SET title = ?2 WHERE subject_key = ?1 AND title != ?2")?
        .execute(params![subject.key(), title])?;
    Ok(())
}
