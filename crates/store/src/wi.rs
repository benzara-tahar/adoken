//! Spec 0015 storage: rich text sources, relations and freshness of work
//! items (cache, FR-32/FR-34), the metadata cache (FR-33), and the local
//! drafts (FR-20) and recent assignees (FR-13). Reads and writes take a plain
//! `&Connection` like [`crate::read`] / [`crate::write`].

use std::collections::HashMap;

use adoken_model::{
    ClassNode, ClassTree, FieldKind, FieldMeta, LinkKind, Millis, NodeKind, Person, PersonId,
    RichText, StateCategory, TextFormat, TypeMeta, WiLink, WiState, WorkItemExtras,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// `Microsoft.VSTS.TCM.ReproSteps`, shown read-only (spec 0015 Q-10).
pub const REPRO_STEPS: &str = "Microsoft.VSTS.TCM.ReproSteps";
/// `Microsoft.VSTS.Common.AcceptanceCriteria`, shown read-only (Q-10).
pub const ACCEPTANCE: &str = "Microsoft.VSTS.Common.AcceptanceCriteria";

fn format_of(s: Option<String>) -> TextFormat {
    s.as_deref().and_then(TextFormat::parse).unwrap_or_default()
}

// ── work item extras ─────────────────────────────────────────────────────

/// Description source, other long-text fields, relations and freshness of
/// work item `id`; `None` when it isn't stored.
pub fn extras(c: &Connection, id: i64) -> Result<Option<WorkItemExtras>> {
    let row = c
        .prepare_cached(
            "SELECT description_source, description_format, synced_ms, full_synced_ms, reason
             FROM work_item WHERE id = ?1 AND is_deleted = 0",
        )?
        .query_row([id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<Millis>>(2)?,
                r.get::<_, Option<Millis>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .optional()?;
    let Some((source, format, synced_ms, full_synced_ms, reason)) = row else {
        return Ok(None);
    };
    let mut out = WorkItemExtras {
        description: source.map(|source| RichText {
            source,
            format: format_of(format),
        }),
        synced_ms,
        full_synced_ms,
        reason,
        ..WorkItemExtras::default()
    };
    let mut st = c.prepare_cached(
        "SELECT field, source, format FROM wi_text_field WHERE work_item_id = ?1",
    )?;
    let rows = st.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            RichText {
                source: r.get(1)?,
                format: format_of(r.get(2)?),
            },
        ))
    })?;
    for row in rows {
        let (field, text) = row?;
        match field.as_str() {
            REPRO_STEPS => out.repro_steps = Some(text),
            ACCEPTANCE => out.acceptance = Some(text),
            _ => {}
        }
    }
    let (links, rev) = links(c, id)?;
    out.links = links;
    out.links_rev = rev;
    Ok(Some(out))
}

/// Every stored relation of `id` in server order, and the revision they
/// were read at (`None` when none are stored).
pub fn links(c: &Connection, id: i64) -> Result<(Vec<WiLink>, Option<i64>)> {
    let mut st = c.prepare_cached(
        "SELECT ord, rel, url, target_id, pr_id, name, comment, rev FROM wi_link
         WHERE work_item_id = ?1 ORDER BY ord",
    )?;
    let mut rev = None;
    let rows = st.query_map([id], |r| {
        let rel: String = r.get(1)?;
        Ok((
            WiLink {
                index: r.get(0)?,
                kind: LinkKind::from_rel(&rel),
                rel,
                url: r.get(2)?,
                target_id: r.get(3)?,
                pr_id: r.get(4)?,
                name: r.get(5)?,
                comment: r.get(6)?,
            },
            r.get::<_, i64>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (link, at) = row?;
        rev = Some(at);
        out.push(link);
    }
    Ok((out, rev))
}

/// Work items whose stored relations point at `target` with `kind`
/// (e.g. the children of an item that has no relations stored itself).
pub fn linked_from(c: &Connection, target: i64, kind: LinkKind) -> Result<Vec<i64>> {
    let mut st = c.prepare_cached(
        "SELECT work_item_id FROM wi_link WHERE target_id = ?1 AND rel = ?2 ORDER BY work_item_id",
    )?;
    let rows = st.query_map(params![target, kind.rel()], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What a full Azure DevOps answer carried beyond the `WorkItem` row.
#[derive(Clone, Debug, Default)]
pub struct FullFields {
    pub description: Option<RichText>,
    /// Other long-text fields by reference name ([`REPRO_STEPS`], [`ACCEPTANCE`]).
    pub texts: Vec<(String, RichText)>,
    /// Relations in server order (`index` = position).
    pub links: Vec<WiLink>,
    /// `System.Reason`.
    pub reason: Option<String>,
}

/// Record that work item `id` at `rev` was just read from Azure DevOps
/// (FR-32): `synced_ms`, and with `full` the raw texts, relations and
/// `full_synced_ms`. Nothing happens unless the stored row is at `rev` (an
/// older answer confirms nothing).
pub fn record_answer(
    c: &Connection,
    id: i64,
    rev: i64,
    now: Millis,
    full: Option<&FullFields>,
) -> Result<()> {
    let Some(full) = full else {
        c.prepare_cached("UPDATE work_item SET synced_ms = ?3 WHERE id = ?1 AND rev = ?2")?
            .execute(params![id, rev, now])?;
        return Ok(());
    };
    let changed = c
        .prepare_cached(
            "UPDATE work_item SET synced_ms = ?3, full_synced_ms = ?3, description_source = ?4,
               description_format = ?5, reason = ?6
             WHERE id = ?1 AND rev = ?2",
        )?
        .execute(params![
            id,
            rev,
            now,
            full.description.as_ref().map(|d| d.source.as_str()),
            full.description.as_ref().map(|d| d.format.as_str()),
            full.reason,
        ])?;
    if changed == 0 {
        return Ok(());
    }
    c.prepare_cached("DELETE FROM wi_text_field WHERE work_item_id = ?1")?
        .execute([id])?;
    let mut text = c.prepare_cached(
        "INSERT INTO wi_text_field(work_item_id, field, source, format) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (field, t) in &full.texts {
        text.execute(params![id, field, t.source, t.format.as_str()])?;
    }
    put_links(c, id, rev, &full.links)
}

/// Replace the stored relations of `id` (read at `rev`).
pub fn put_links(c: &Connection, id: i64, rev: i64, links: &[WiLink]) -> Result<()> {
    c.prepare_cached("DELETE FROM wi_link WHERE work_item_id = ?1")?
        .execute([id])?;
    let mut ins = c.prepare_cached(
        "INSERT INTO wi_link(work_item_id, ord, rel, url, target_id, pr_id, name, comment, rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (ord, l) in links.iter().enumerate() {
        ins.execute(params![
            id,
            ord as i64,
            l.rel,
            l.url,
            l.target_id,
            l.pr_id,
            l.name,
            l.comment,
            rev
        ])?;
    }
    Ok(())
}

// ── comments ─────────────────────────────────────────────────────────────

/// One work item comment as the discussion renders it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WiComment {
    /// Azure DevOps' comment id.
    pub id: i64,
    pub author: PersonId,
    /// Plain projection, mentions as `@<guid>`.
    pub body: String,
    /// Raw text (`None` for rows synced before 0015).
    pub source: Option<String>,
    pub format: TextFormat,
    pub created: Millis,
    pub updated: Millis,
}

impl WiComment {
    /// Edited after posting.
    pub fn edited(&self) -> bool {
        self.updated > self.created
    }
}

/// A work item's discussion, oldest first.
pub fn comments(c: &Connection, id: i64) -> Result<Vec<WiComment>> {
    let mut st = c.prepare_cached(
        "SELECT ado_comment_id, author_id, body, body_source, format, created_ms, updated_ms
         FROM comment
         WHERE source = 'wi' AND parent_id = ?1 AND is_deleted = 0 AND is_system = 0
         ORDER BY created_ms, id",
    )?;
    let rows = st.query_map([id], |r| {
        Ok(WiComment {
            id: r.get(0)?,
            author: r.get(1)?,
            body: r.get(2)?,
            source: r.get(3)?,
            format: format_of(r.get(4)?),
            created: r.get(5)?,
            updated: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The highest stored comment id of `id` (demo comments count on from it).
pub fn max_comment_id(c: &Connection, id: i64) -> Result<i64> {
    Ok(c.prepare_cached(
        "SELECT coalesce(max(ado_comment_id), 0) FROM comment WHERE source = 'wi' AND parent_id = ?1",
    )?
    .query_row([id], |r| r.get(0))?)
}

// ── drafts (FR-20, local) ────────────────────────────────────────────────

/// What a draft is the unsent text of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DraftKind {
    Description,
    Comment,
}

impl DraftKind {
    fn as_str(self) -> &'static str {
        match self {
            DraftKind::Description => "description",
            DraftKind::Comment => "comment",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "description" => Some(DraftKind::Description),
            "comment" => Some(DraftKind::Comment),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WiDraft {
    pub work_item_id: i64,
    pub kind: DraftKind,
    pub body: String,
    /// The revision the edit started from.
    pub base_rev: i64,
    pub updated: Millis,
}

/// The drafts of work item `id`.
pub fn drafts(c: &Connection, id: i64) -> Result<Vec<WiDraft>> {
    let mut st = c.prepare_cached(
        "SELECT kind, body, base_rev, updated_ms FROM wi_draft WHERE work_item_id = ?1
         ORDER BY kind",
    )?;
    let rows = st.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Millis>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, body, base_rev, updated) = row?;
        if let Some(kind) = DraftKind::parse(&kind) {
            out.push(WiDraft {
                work_item_id: id,
                kind,
                body,
                base_rev,
                updated,
            });
        }
    }
    Ok(out)
}

/// Save `draft`; a blank body deletes it.
pub fn save_draft(c: &Connection, draft: &WiDraft) -> Result<()> {
    if draft.body.trim().is_empty() {
        return delete_draft(c, draft.work_item_id, draft.kind);
    }
    c.prepare_cached(
        "INSERT INTO wi_draft(work_item_id, kind, body, base_rev, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(work_item_id, kind) DO UPDATE SET body = excluded.body,
           base_rev = excluded.base_rev, updated_ms = excluded.updated_ms",
    )?
    .execute(params![
        draft.work_item_id,
        draft.kind.as_str(),
        draft.body,
        draft.base_rev,
        draft.updated
    ])?;
    Ok(())
}

pub fn delete_draft(c: &Connection, id: i64, kind: DraftKind) -> Result<()> {
    c.prepare_cached("DELETE FROM wi_draft WHERE work_item_id = ?1 AND kind = ?2")?
        .execute(params![id, kind.as_str()])?;
    Ok(())
}

// ── recent assignees (FR-13, local) ──────────────────────────────────────

/// How many recent assignees the picker lists.
pub const RECENT_ASSIGNEES: usize = 5;

/// People I assigned in adoken, most recent first.
pub fn recent_assignees(c: &Connection, limit: usize) -> Result<Vec<Person>> {
    let mut st = c.prepare_cached(
        "SELECT identity_id, display_name, unique_name FROM wi_recent_assignee
         ORDER BY used_ms DESC LIMIT ?1",
    )?;
    let rows = st.query_map([limit as i64], |r| {
        Ok(Person {
            id: r.get(0)?,
            display_name: r.get(1)?,
            unique_name: r.get(2)?,
            is_group: false,
            image_url: None,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Remember that I assigned `person` now; keeps the newest
/// [`RECENT_ASSIGNEES`].
pub fn touch_recent_assignee(c: &Connection, person: &Person, now: Millis) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO wi_recent_assignee(identity_id, display_name, unique_name, used_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(identity_id) DO UPDATE SET display_name = excluded.display_name,
           unique_name = excluded.unique_name, used_ms = excluded.used_ms",
    )?
    .execute(params![
        person.id,
        person.display_name,
        person.unique_name,
        now
    ])?;
    c.prepare_cached(
        "DELETE FROM wi_recent_assignee WHERE identity_id NOT IN (
           SELECT identity_id FROM wi_recent_assignee ORDER BY used_ms DESC LIMIT ?1)",
    )?
    .execute([RECENT_ASSIGNEES as i64])?;
    Ok(())
}

// ── metadata cache (FR-33) ───────────────────────────────────────────────

/// States and transitions of `type_name` in `project_id`, if fetched.
pub fn type_meta(c: &Connection, project_id: &str, type_name: &str) -> Result<Option<TypeMeta>> {
    let fetched: Option<Option<Millis>> = c
        .prepare_cached("SELECT states_ms FROM wi_type_meta WHERE project_id = ?1 AND type = ?2")?
        .query_row(params![project_id, type_name], |r| r.get(0))
        .optional()?;
    let Some(Some(fetched_ms)) = fetched else {
        return Ok(None);
    };
    let mut st = c.prepare_cached(
        "SELECT name, category, color FROM wi_type_state WHERE project_id = ?1 AND type = ?2
         ORDER BY ord",
    )?;
    let states = st
        .query_map(params![project_id, type_name], |r| {
            let name: String = r.get(0)?;
            let category: String = r.get(1)?;
            Ok(WiState {
                category: StateCategory::parse(&category)
                    .unwrap_or_else(|| StateCategory::of_state(&name)),
                name,
                color: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut st = c.prepare_cached(
        "SELECT from_state, to_state FROM wi_type_transition WHERE project_id = ?1 AND type = ?2
         ORDER BY from_state, ord",
    )?;
    let mut transitions: HashMap<String, Vec<String>> = HashMap::new();
    for row in st.query_map(params![project_id, type_name], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (from, to) = row?;
        transitions.entry(from).or_default().push(to);
    }
    Ok(Some(TypeMeta {
        states,
        transitions,
        fetched_ms,
    }))
}

/// The category Azure DevOps gives `state` of `type_name` in `project_id`
/// (from the cached `workitemtypes/{type}` states), if fetched. Process
/// templates name their states freely ("Doing", "Ready", "Shipped"), so
/// this, not the stock-name fallback, decides open vs. closed.
pub fn state_category(
    c: &Connection,
    project_id: &str,
    type_name: &str,
    state: &str,
) -> Result<Option<StateCategory>> {
    let category: Option<String> = c
        .prepare_cached(
            "SELECT category FROM wi_type_state
             WHERE project_id = ?1 AND type = ?2 AND name = ?3 COLLATE NOCASE
             ORDER BY ord LIMIT 1",
        )?
        .query_row(params![project_id, type_name, state], |r| r.get(0))
        .optional()?;
    Ok(category.as_deref().and_then(StateCategory::parse))
}

/// Work item types present in the store (not deleted) whose states were
/// never fetched or were fetched before `stale_before`: `(project, type)`.
pub fn types_needing_states(c: &Connection, stale_before: Millis) -> Result<Vec<(String, String)>> {
    let mut st = c.prepare_cached(
        "SELECT DISTINCT w.project_id, w.type FROM work_item w
         LEFT JOIN wi_type_meta m ON m.project_id = w.project_id AND m.type = w.type
         WHERE w.is_deleted = 0 AND (m.states_ms IS NULL OR m.states_ms < ?1)
         ORDER BY w.project_id, w.type",
    )?;
    let rows = st
        .query_map([stale_before], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Replace the cached states and transitions of a type. Stored items of
/// that type take the fetched categories (their category was a guess from
/// the stock state names until now).
pub fn put_type_meta(
    c: &Connection,
    project_id: &str,
    type_name: &str,
    meta: &TypeMeta,
) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO wi_type_meta(project_id, type, states_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(project_id, type) DO UPDATE SET states_ms = excluded.states_ms",
    )?
    .execute(params![project_id, type_name, meta.fetched_ms])?;
    c.prepare_cached("DELETE FROM wi_type_state WHERE project_id = ?1 AND type = ?2")?
        .execute(params![project_id, type_name])?;
    c.prepare_cached("DELETE FROM wi_type_transition WHERE project_id = ?1 AND type = ?2")?
        .execute(params![project_id, type_name])?;
    let mut state = c.prepare_cached(
        "INSERT INTO wi_type_state(project_id, type, ord, name, category, color)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (ord, s) in meta.states.iter().enumerate() {
        state.execute(params![
            project_id,
            type_name,
            ord as i64,
            s.name,
            s.category.as_str(),
            s.color
        ])?;
    }
    let mut tr = c.prepare_cached(
        "INSERT OR IGNORE INTO wi_type_transition(project_id, type, from_state, ord, to_state)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (from, to) in &meta.transitions {
        for (ord, t) in to.iter().enumerate() {
            tr.execute(params![project_id, type_name, from, ord as i64, t])?;
        }
    }
    recategorize(c, project_id, type_name)
}

/// Align the stored items of a type with its cached state categories.
/// `terminal_since_ms` follows: set (to the last change) on entering
/// Completed/Removed, cleared on leaving them.
fn recategorize(c: &Connection, project_id: &str, type_name: &str) -> Result<()> {
    c.prepare_cached(
        "UPDATE work_item SET
           terminal_since_ms = CASE
             WHEN s.category NOT IN ('Completed', 'Removed') THEN NULL
             WHEN work_item.state_category IN ('Completed', 'Removed')
               THEN work_item.terminal_since_ms
             ELSE work_item.changed_ms END,
           state_category = s.category
         FROM (SELECT name, category FROM wi_type_state
               WHERE project_id = ?1 AND type = ?2) AS s
         WHERE work_item.project_id = ?1 AND work_item.type = ?2
           AND s.name = work_item.state COLLATE NOCASE
           AND work_item.state_category <> s.category",
    )?
    .execute(params![project_id, type_name])?;
    Ok(())
}

/// The fields of `type_name`, and when they were fetched.
pub fn type_fields(
    c: &Connection,
    project_id: &str,
    type_name: &str,
) -> Result<Option<(Vec<FieldMeta>, Millis)>> {
    let fetched: Option<Option<Millis>> = c
        .prepare_cached("SELECT fields_ms FROM wi_type_meta WHERE project_id = ?1 AND type = ?2")?
        .query_row(params![project_id, type_name], |r| r.get(0))
        .optional()?;
    let Some(Some(at)) = fetched else {
        return Ok(None);
    };
    let mut st = c.prepare_cached(
        "SELECT reference, name, kind, allowed FROM wi_type_field
         WHERE project_id = ?1 AND type = ?2 ORDER BY name",
    )?;
    let rows = st.query_map(params![project_id, type_name], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (reference, name, kind, allowed) = row?;
        out.push(FieldMeta {
            reference,
            name,
            kind: field_kind(kind.as_deref(), allowed),
        });
    }
    Ok(Some((out, at)))
}

/// `None` kind (not resolved yet) reads as [`FieldKind::Unknown`]; see
/// [`field_resolved`].
fn field_kind(kind: Option<&str>, allowed: Option<String>) -> FieldKind {
    match kind {
        Some("list") => FieldKind::List(
            allowed
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect(),
        ),
        Some("text") => FieldKind::Text,
        Some("person") => FieldKind::Person,
        _ => FieldKind::Unknown,
    }
}

/// Whether the kind of field `reference` is known (a list, or resolved by
/// its definition).
pub fn field_resolved(
    c: &Connection,
    project_id: &str,
    type_name: &str,
    reference: &str,
) -> Result<bool> {
    Ok(c.prepare_cached(
        "SELECT kind IS NOT NULL FROM wi_type_field
         WHERE project_id = ?1 AND type = ?2 AND reference = ?3",
    )?
    .query_row(params![project_id, type_name, reference], |r| r.get(0))
    .optional()?
    .unwrap_or(false))
}

/// One field of a type as listed by Azure DevOps: its allowed values make
/// it a list; otherwise its kind is resolved later ([`set_field_kind`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedField {
    pub reference: String,
    pub name: String,
    pub allowed: Vec<String>,
}

/// Replace the cached fields of a type.
pub fn put_type_fields(
    c: &Connection,
    project_id: &str,
    type_name: &str,
    fields: &[ListedField],
    now: Millis,
) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO wi_type_meta(project_id, type, fields_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(project_id, type) DO UPDATE SET fields_ms = excluded.fields_ms",
    )?
    .execute(params![project_id, type_name, now])?;
    c.prepare_cached("DELETE FROM wi_type_field WHERE project_id = ?1 AND type = ?2")?
        .execute(params![project_id, type_name])?;
    let mut ins = c.prepare_cached(
        "INSERT OR REPLACE INTO wi_type_field(project_id, type, reference, name, kind, allowed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for f in fields {
        let list = !f.allowed.is_empty();
        ins.execute(params![
            project_id,
            type_name,
            f.reference,
            f.name,
            list.then_some("list"),
            list.then(|| f.allowed.join("\n")),
        ])?;
    }
    Ok(())
}

/// Record a field's kind from its definition (FR-9).
pub fn set_field_kind(
    c: &Connection,
    project_id: &str,
    type_name: &str,
    reference: &str,
    kind: &FieldKind,
) -> Result<()> {
    let (code, allowed) = match kind {
        FieldKind::List(values) => ("list", Some(values.join("\n"))),
        FieldKind::Text => ("text", None),
        FieldKind::Person => ("person", None),
        FieldKind::Unknown => ("unknown", None),
    };
    c.prepare_cached(
        "UPDATE wi_type_field SET kind = ?4, allowed = ?5
         WHERE project_id = ?1 AND type = ?2 AND reference = ?3",
    )?
    .execute(params![project_id, type_name, reference, code, allowed])?;
    Ok(())
}

/// A project's area and iteration trees, if fetched.
pub fn class_tree(c: &Connection, project_id: &str) -> Result<Option<ClassTree>> {
    let fetched: Option<Millis> = c
        .prepare_cached("SELECT fetched_ms FROM wi_class_tree WHERE project_id = ?1")?
        .query_row([project_id], |r| r.get(0))
        .optional()?;
    let Some(fetched_ms) = fetched else {
        return Ok(None);
    };
    let mut tree = ClassTree {
        fetched_ms,
        ..ClassTree::default()
    };
    let mut st = c.prepare_cached(
        "SELECT kind, path, name, depth, start_ms, finish_ms FROM wi_class_node
         WHERE project_id = ?1 ORDER BY kind, ord",
    )?;
    let rows = st.query_map([project_id], |r| {
        let kind: String = r.get(0)?;
        Ok(ClassNode {
            kind: if kind == "area" {
                NodeKind::Area
            } else {
                NodeKind::Iteration
            },
            path: r.get(1)?,
            name: r.get(2)?,
            depth: r.get(3)?,
            start_ms: r.get(4)?,
            finish_ms: r.get(5)?,
        })
    })?;
    for row in rows {
        let node = row?;
        match node.kind {
            NodeKind::Area => tree.areas.push(node),
            NodeKind::Iteration => tree.iterations.push(node),
        }
    }
    Ok(Some(tree))
}

/// Replace a project's cached trees.
pub fn put_class_tree(c: &Connection, project_id: &str, tree: &ClassTree) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO wi_class_tree(project_id, fetched_ms) VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET fetched_ms = excluded.fetched_ms",
    )?
    .execute(params![project_id, tree.fetched_ms])?;
    c.prepare_cached("DELETE FROM wi_class_node WHERE project_id = ?1")?
        .execute([project_id])?;
    let mut ins = c.prepare_cached(
        "INSERT INTO wi_class_node(project_id, kind, ord, path, name, depth, start_ms, finish_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (kind, nodes) in [("area", &tree.areas), ("iteration", &tree.iterations)] {
        for (ord, n) in nodes.iter().enumerate() {
            ins.execute(params![
                project_id,
                kind,
                ord as i64,
                n.path,
                n.name,
                n.depth,
                n.start_ms,
                n.finish_ms
            ])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
