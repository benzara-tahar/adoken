//! Store side of spec 0015: what every work item answer adds to the store
//! (FR-32/FR-34, called from `engine::apply_work_items`), the demo org's
//! local writes (FR-37), and the local link guards (FR-21).

use adoken_model::{
    LinkKind, Millis, PersonId, RichText, TextFormat, WiLink, WorkItem, WorkItemComment,
    WorkItemExtras,
};
use adoken_store::rusqlite::Connection;
use adoken_store::wi::{self, ACCEPTANCE, FullFields, REPRO_STEPS};
use adoken_store::{Store, read, write};
use serde_json::{Map, Value, json};

use super::{WiChange, WiError, WiWrite, distinct_name};
use crate::engine::{Emitter, apply_work_items};
use crate::wire::{WireWorkItem, html_to_text, normalize_mentions, pr_link_id};

/// What a work item answer carries beyond the `WorkItem` row, captured
/// before `WireWorkItem::into_model` consumes it.
pub(crate) struct Extras {
    full: Option<FullFields>,
}

/// The id at the end of a work item URL (`…/_apis/wit/workItems/123`).
fn work_item_target(url: &str) -> Option<i64> {
    let (head, id) = url.rsplit_once('/')?;
    head.to_ascii_lowercase()
        .ends_with("/_apis/wit/workitems")
        .then(|| id.parse().ok())
        .flatten()
}

impl Extras {
    pub(crate) fn of(w: &WireWorkItem) -> Self {
        if w.partial {
            return Extras { full: None };
        }
        let text = |field: &str| {
            let source = w.fields.get(field)?.as_str()?;
            let format = match w.multiline_formats.get(field) {
                Some(f) if f.eq_ignore_ascii_case("markdown") => TextFormat::Markdown,
                _ => TextFormat::Html,
            };
            Some(RichText {
                source: source.to_owned(),
                format,
            })
        };
        let attr = |r: &crate::wire::Relation, k: &str| {
            r.attributes
                .get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let links = w
            .relations
            .iter()
            .flatten()
            .enumerate()
            .map(|(i, r)| WiLink {
                index: i as u32,
                kind: LinkKind::from_rel(&r.rel),
                target_id: work_item_target(&r.url),
                pr_id: (r.rel == "ArtifactLink")
                    .then(|| pr_link_id(&r.url))
                    .flatten(),
                rel: r.rel.clone(),
                url: r.url.clone(),
                name: attr(r, "name"),
                comment: attr(r, "comment"),
            })
            .collect();
        Extras {
            full: Some(FullFields {
                description: text("System.Description"),
                texts: [REPRO_STEPS, ACCEPTANCE]
                    .into_iter()
                    .filter_map(|f| Some((f.to_owned(), text(f)?)))
                    .collect(),
                links,
                reason: w
                    .fields
                    .get("System.Reason")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }),
        }
    }
}

/// FR-32/FR-34: freshness, and for full answers the raw texts and every
/// relation. Runs for every applied answer, changed or not (an unchanged
/// item was still confirmed).
pub(crate) fn store_extras(
    tx: &Connection,
    item: &WorkItem,
    extras: &Extras,
    now: Millis,
) -> adoken_store::Result<()> {
    wi::record_answer(tx, item.id, item.rev, now, extras.full.as_ref())
}

// ── local link guards (FR-21) ────────────────────────────────────────────

/// Why adoken refuses a link change without sending it (FR-21): a link to
/// the item itself, a Parent or Child that makes a cycle among stored items,
/// or a link that already exists. Everything else is Azure DevOps' to
/// refuse. Blocking.
pub fn link_guard(
    c: &Connection,
    item: &WorkItem,
    extras: &WorkItemExtras,
    change: &WiChange,
) -> Result<(), String> {
    let WiChange::AddLink { kind, target, .. } = change else {
        return Ok(());
    };
    let (id, target) = (item.id, *target);
    if target == id {
        return Err(format!("#{id} can't link to itself."));
    }
    let parent = extras.parent().or(item.parent_id);
    let exists = extras.link(*kind, target).is_some()
        || (*kind == LinkKind::Parent && parent == Some(target));
    if exists {
        return Err(format!(
            "#{target} is already linked as {}.",
            kind.label().to_lowercase()
        ));
    }
    let cycle = match kind {
        // `target` would be my parent: it must not descend from me.
        LinkKind::Parent => is_ancestor(c, id, target).map_err(|e| e.to_string())?,
        // `target` would be my child: it must not be one of my ancestors.
        LinkKind::Child => {
            parent == Some(target) || is_ancestor(c, target, id).map_err(|e| e.to_string())?
        }
        _ => false,
    };
    if cycle {
        return Err(format!(
            "#{target} can't be the {} of #{id}: that would make a loop.",
            kind.label().to_lowercase()
        ));
    }
    Ok(())
}

/// Whether `ancestor` is above `of` in the stored hierarchy.
fn is_ancestor(c: &Connection, ancestor: i64, of: i64) -> adoken_store::Result<bool> {
    let mut at = of;
    for _ in 0..64 {
        let parent = match read::work_item(c, at)? {
            Some(wi) => wi.parent_id,
            None => None,
        };
        let parent = match parent {
            Some(p) => Some(p),
            None => wi::extras(c, at)?.and_then(|x| x.parent()),
        };
        match parent {
            Some(p) if p == ancestor => return Ok(true),
            Some(p) => at = p,
            None => return Ok(false),
        }
    }
    Ok(false)
}

// ── demo (FR-37) ─────────────────────────────────────────────────────────

fn my_id(store: &Store) -> Result<PersonId, WiError> {
    store
        .me()?
        .map(|p| p.id)
        .ok_or_else(|| WiError::Local("signed-in identity unknown".into()))
}

/// Run `f` in one write transaction with events on (writes are always
/// mine, so they come out self-caused), keeping its typed error.
pub(crate) fn in_tx<R>(
    store: &Store,
    me: &str,
    now: Millis,
    enabled: bool,
    f: impl FnOnce(&mut Emitter<'_>) -> Result<R, WiError>,
) -> Result<(R, Vec<i64>), WiError> {
    let mut outcome = None;
    let result = store.write(|tx| {
        let mut em = Emitter {
            tx,
            me,
            enabled,
            now,
            new_ids: Vec::new(),
        };
        match f(&mut em) {
            Ok(r) => {
                outcome = Some(Ok((r, em.new_ids)));
                Ok(())
            }
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

fn identity(c: &Connection, id: &str) -> adoken_store::Result<Value> {
    let p = read::person(c, id)?;
    Ok(json!({
        "id": id,
        "displayName": p.as_ref().map(|p| p.display_name.clone()).unwrap_or_default(),
        "uniqueName": p.and_then(|p| p.unique_name),
    }))
}

/// The stored item as Azure DevOps would answer it, so a demo write goes
/// through `apply_work_items` like a real answer (same store effects, same
/// events).
fn as_wire(c: &Connection, item: &WorkItem, extras: &WorkItemExtras) -> Result<Value, WiError> {
    let project = read::projects(c)?
        .into_iter()
        .find(|p| p.id == item.project_id)
        .ok_or_else(|| WiError::Local(format!("#{}'s project isn't stored", item.id)))?;
    let mut fields = Map::new();
    let mut put = |k: &str, v: Value| {
        fields.insert(k.to_owned(), v);
    };
    put("System.TeamProject", json!(project.name));
    put("System.WorkItemType", json!(item.kind.as_str()));
    put("System.Title", json!(item.title));
    put("System.State", json!(item.state));
    if let Some(a) = &item.assigned_to {
        put("System.AssignedTo", identity(c, a)?);
    }
    put("System.CreatedBy", identity(c, &item.created_by)?);
    put("System.ChangedBy", identity(c, &item.changed_by)?);
    put("System.AreaPath", json!(item.area_path));
    put("System.IterationPath", json!(item.iteration_path));
    if let Some(p) = item.priority {
        put("Microsoft.VSTS.Common.Priority", json!(p));
    }
    put("System.Tags", json!(item.tags.join("; ")));
    put("System.CommentCount", json!(item.comment_count));
    put(
        "System.CreatedDate",
        json!(crate::wire::format_time(item.created)),
    );
    put(
        "System.ChangedDate",
        json!(crate::wire::format_time(item.changed)),
    );
    let mut formats = Map::new();
    let mut text = |field: &str, t: Option<&RichText>, fallback: Option<&String>| match t {
        Some(t) => {
            fields.insert(field.to_owned(), json!(t.source));
            if t.format == TextFormat::Markdown {
                formats.insert(field.to_owned(), json!("Markdown"));
            }
        }
        None => {
            if let Some(plain) = fallback {
                fields.insert(field.to_owned(), json!(plain));
                formats.insert(field.to_owned(), json!("Markdown"));
            }
        }
    };
    text(
        "System.Description",
        extras.description.as_ref(),
        item.description.as_ref(),
    );
    text(REPRO_STEPS, extras.repro_steps.as_ref(), None);
    text(ACCEPTANCE, extras.acceptance.as_ref(), None);
    // Demo items carry their parent and PR links only in the row; relations
    // stored since are authoritative.
    let relations: Vec<Value> = if extras.full_synced_ms.is_some() {
        extras
            .links
            .iter()
            .map(|l| json!({"rel": l.rel, "url": l.url, "attributes": {"name": l.name, "comment": l.comment}}))
            .collect()
    } else {
        item.parent_id
            .map(|p| json!({"rel": LinkKind::Parent.rel(), "url": format!("demo:/_apis/wit/workItems/{p}"), "attributes": {}}))
            .into_iter()
            .chain(item.linked_pr_ids.iter().map(|pr| {
                json!({"rel": "ArtifactLink", "url": format!("vstfs:///Git/PullRequestId/demo%2Fdemo%2F{pr}"), "attributes": {"name": "Pull Request"}})
            }))
            .collect()
    };
    Ok(json!({
        "id": item.id,
        "rev": item.rev,
        "fields": fields,
        "relations": relations,
        "multilineFieldsFormat": formats,
    }))
}

fn set_parent_field(wire: &mut Value, parent: Option<i64>) {
    match parent {
        Some(p) => wire["fields"]["System.Parent"] = json!(p),
        None => {
            if let Some(f) = wire["fields"].as_object_mut() {
                f.remove("System.Parent");
            }
        }
    }
}

/// The demo org (FR-37): apply `write` to the store as if Azure DevOps had
/// accepted it: a new revision by me through the same apply path as a real
/// answer, with the same events. Blocking.
pub fn apply_local(store: &Store, write: &WiWrite, now: Millis) -> Result<WorkItem, WiError> {
    write.change.validate()?;
    let me = my_id(store)?;
    let (item, _) = in_tx(store, &me, now, true, |em| {
        let tx = em.tx;
        let item = read::work_item(tx, write.id)?
            .ok_or_else(|| WiError::Local(format!("#{} isn't stored", write.id)))?;
        let extras = wi::extras(tx, write.id)?.unwrap_or_default();
        if let WiChange::AddLink { .. } = &write.change {
            link_guard(tx, &item, &extras, &write.change).map_err(WiError::Invalid)?;
        }
        let mut wire = as_wire(tx, &item, &extras)?;
        wire["rev"] = json!(item.rev + 1);
        wire["fields"]["System.ChangedBy"] = identity(tx, &me)?;
        wire["fields"]["System.ChangedDate"] =
            json!(crate::wire::format_time(now.max(item.changed + 1)));
        let fields = &mut wire["fields"];
        match &write.change {
            WiChange::State { to, .. } => fields["System.State"] = json!(to),
            WiChange::Title { to, .. } => fields["System.Title"] = json!(to.trim()),
            WiChange::Assign { to, .. } => match to {
                Some(p) => {
                    write::upsert_person(tx, p, false)?;
                    fields["System.AssignedTo"] = identity(tx, &p.id)?;
                }
                None => {
                    fields
                        .as_object_mut()
                        .map(|f| f.remove("System.AssignedTo"));
                }
            },
            WiChange::Area { to, .. } => fields["System.AreaPath"] = json!(to),
            WiChange::Iteration { to, .. } => fields["System.IterationPath"] = json!(to),
            WiChange::Description { to, format, .. } => {
                fields["System.Description"] = json!(to);
                let formats = wire["multilineFieldsFormat"]
                    .as_object_mut()
                    .expect("object");
                match format {
                    TextFormat::Markdown => {
                        formats.insert("System.Description".into(), json!("Markdown"));
                    }
                    TextFormat::Html => {
                        formats.remove("System.Description");
                    }
                }
            }
            WiChange::AddLink {
                kind,
                target,
                replace_parent,
            } => {
                let relations = wire["relations"].as_array_mut().expect("array");
                if replace_parent.is_some() || *kind == LinkKind::Parent {
                    relations.retain(|r| r["rel"] != json!(LinkKind::Parent.rel()));
                }
                relations.push(json!({
                    "rel": kind.rel(),
                    "url": format!("demo:/_apis/wit/workItems/{target}"),
                    "attributes": {},
                }));
                if *kind == LinkKind::Parent {
                    set_parent_field(&mut wire, Some(*target));
                }
            }
            WiChange::RemoveLink { kind, target } => {
                let relations = wire["relations"].as_array_mut().expect("array");
                relations.retain(|r| {
                    !(r["rel"] == json!(kind.rel())
                        && r["url"].as_str().and_then(work_item_target) == Some(*target))
                });
                if *kind == LinkKind::Parent {
                    set_parent_field(&mut wire, None);
                }
            }
        }
        if !matches!(
            write.change,
            WiChange::AddLink { .. } | WiChange::RemoveLink { .. }
        ) {
            set_parent_field(&mut wire, item.parent_id);
        }
        for (field, value) in &write.extra {
            wire["fields"][field] = value.clone();
        }
        let wire: WireWorkItem =
            serde_json::from_value(wire).map_err(|e| WiError::Local(e.to_string()))?;
        apply_work_items(em, vec![wire])?;
        read::work_item(em.tx, write.id)?
            .ok_or_else(|| WiError::Local(format!("#{} isn't stored", write.id)))
    })?;
    Ok(item)
}

/// The demo org: post a Markdown comment locally (FR-25, FR-37): the
/// comment is stored with its source, and the item gets a new revision
/// with one more comment, like Azure DevOps does. Blocking.
pub fn comment_local(
    store: &Store,
    id: i64,
    markdown: &str,
    now: Millis,
) -> Result<wi::WiComment, WiError> {
    if markdown.trim().is_empty() {
        return Err(WiError::Invalid("Nothing to post".into()));
    }
    let me = my_id(store)?;
    let (comment, _) = in_tx(store, &me, now, true, |em| {
        let tx = em.tx;
        let item = read::work_item(tx, id)?
            .ok_or_else(|| WiError::Local(format!("#{id} isn't stored")))?;
        let comment = WorkItemComment {
            id: wi::max_comment_id(tx, id)? + 1,
            work_item_id: id,
            author: me.clone(),
            body: normalize_mentions(markdown),
            mentions: Vec::new(),
            created: now,
            updated: now,
            source: Some(markdown.to_owned()),
            format: TextFormat::Markdown,
        };
        write::upsert_wi_comment(tx, &comment)?;
        let extras = wi::extras(tx, id)?.unwrap_or_default();
        let mut wire = as_wire(tx, &item, &extras)?;
        wire["rev"] = json!(item.rev + 1);
        wire["fields"]["System.ChangedBy"] = identity(tx, &me)?;
        wire["fields"]["System.ChangedDate"] =
            json!(crate::wire::format_time(now.max(item.changed + 1)));
        wire["fields"]["System.CommentCount"] = json!(item.comment_count + 1);
        set_parent_field(&mut wire, item.parent_id);
        let wire: WireWorkItem =
            serde_json::from_value(wire).map_err(|e| WiError::Local(e.to_string()))?;
        apply_work_items(em, vec![wire])?;
        // The discussion is already stored: no refetch needed.
        write::set_wi_comments_due(tx, id, false)?;
        stored_comment(tx, id, comment.id)
    })?;
    Ok(comment)
}

pub(crate) fn stored_comment(
    c: &Connection,
    id: i64,
    comment_id: i64,
) -> Result<wi::WiComment, WiError> {
    wi::comments(c, id)?
        .into_iter()
        .find(|cm| cm.id == comment_id)
        .ok_or_else(|| WiError::Local("comment not stored".into()))
}

/// The plain projection of a description for search and previews.
pub fn plain_text(text: &RichText) -> String {
    match text.format {
        TextFormat::Markdown => text.source.clone(),
        TextFormat::Html => html_to_text(&text.source),
    }
}

/// `Name <signin>` of a stored person, else their id.
pub fn distinct_name_of(c: &Connection, id: &str) -> String {
    read::person(c, id)
        .ok()
        .flatten()
        .map_or_else(|| id.to_owned(), |p| distinct_name(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR-32: a title-index row (reporting feed) confirms the item but never
    /// clears its description or relations; a full answer carries them,
    /// links indexed in server order with their targets.
    #[test]
    fn title_index_rows_carry_no_full_fields() {
        let page: crate::wire::RevisionsPage = serde_json::from_value(json!({
            "values": [{"id": 1, "rev": 3, "fields": {"System.Title": "t"}}],
            "isLastBatch": true
        }))
        .unwrap();
        assert!(page.values[0].partial);
        assert!(Extras::of(&page.values[0]).full.is_none());

        let full: WireWorkItem = serde_json::from_value(json!({
            "id": 2, "rev": 4,
            "fields": {"System.Description": "## Hi", "Microsoft.VSTS.TCM.ReproSteps": "<p>1</p>"},
            "multilineFieldsFormat": {"System.Description": "Markdown"},
            "relations": [
                {"rel": "ArtifactLink", "url": "vstfs:///Git/PullRequestId/p%2Fr%2F4821", "attributes": {"name": "Pull Request"}},
                {"rel": "System.LinkTypes.Hierarchy-Reverse", "url": "https://dev.azure.com/o/_apis/wit/workItems/7"}
            ]
        }))
        .unwrap();
        let full = Extras::of(&full).full.unwrap();
        assert_eq!(full.description.unwrap().format, TextFormat::Markdown);
        assert_eq!(full.texts[0].1.format, TextFormat::Html);
        assert_eq!(full.links[0].pr_id, Some(4821));
        assert_eq!(
            (
                full.links[1].index,
                full.links[1].kind,
                full.links[1].target_id
            ),
            (1, Some(LinkKind::Parent), Some(7))
        );
    }
}
