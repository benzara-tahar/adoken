//! Store reads for the Work items view (spec 0008 FR-1, FR-3; spec 0015
//! FR-2, FR-3, FR-23). Every function blocks: call it on a background
//! executor. Queries are served by primary keys or indexes
//! (`wi_by_changed`, `wi_by_assignee`, the `comment` and `sync_event`
//! unique keys, `wi_link`'s primary key). Rich text is parsed here, off the
//! UI thread, and cached per `(id, rev)` (FR-2).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use adoken_model::{
    LinkKind, Millis, Person, PersonId, PullRequest, RichText, SyncEventKind, TextFormat, WiLink,
    WorkItem, WorkItemExtras,
};
use adoken_richtext::{Block, Doc, DocCache, Format, Inline, Span};
use adoken_store::rusqlite::{Connection, params};
use adoken_store::wi::{DraftKind, WiDraft};
use adoken_store::{Store, WorkItemFilter, read};

use super::detail::model::LinkGroup;

/// Newest-changed items loaded into the list (plus every item assigned to
/// me, whatever its age).
pub const LIST_LIMIT: usize = 5_000;
/// Sync events shown in an item's history.
const HISTORY_LIMIT: usize = 50;

/// What the list needs, read in one background job.
#[derive(Debug, Clone, Default)]
pub struct ListData {
    pub me: PersonId,
    pub names: HashMap<PersonId, String>,
    /// Everyone in the store (pickers, mentions).
    pub people: Vec<Person>,
    pub items: Vec<WorkItem>,
    /// Project id → name.
    pub projects: HashMap<String, String>,
    pub org_url: Option<String>,
    /// Demo org: "Open in Azure DevOps" stays disabled.
    pub demo: bool,
    pub last_sync: Option<Millis>,
}

impl ListData {
    pub fn read(store: &Store) -> adoken_store::Result<Self> {
        let me = store.me()?.map(|p| p.id).unwrap_or_default();
        let mut items = store.work_items(&WorkItemFilter {
            limit: Some(LIST_LIMIT),
            ..WorkItemFilter::default()
        })?;
        if !me.is_empty() {
            let loaded: HashSet<i64> = items.iter().map(|item| item.id).collect();
            let mine = store.work_items(&WorkItemFilter {
                assigned_to: Some(me.clone()),
                ..WorkItemFilter::default()
            })?;
            items.extend(mine.into_iter().filter(|item| !loaded.contains(&item.id)));
        }
        let people = store.people()?;
        Ok(Self {
            names: people
                .iter()
                .map(|p| (p.id.clone(), p.display_name.clone()))
                .collect(),
            people,
            projects: store
                .projects()?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect(),
            org_url: store.org()?.map(|org| org.url),
            demo: store.meta("demo")?.is_some(),
            last_sync: store.last_sync()?,
            me,
            items,
        })
    }

    pub fn name(&self, id: &str) -> String {
        name(&self.names, id)
    }
}

/// Display name, or the raw id when the person isn't synced.
pub fn name(names: &HashMap<PersonId, String>, id: &str) -> String {
    names.get(id).cloned().unwrap_or_else(|| id.to_owned())
}

/// `https://dev.azure.com/contoso/Payments/_workitems/edit/20101`.
pub fn web_url(org_url: &str, project: &str, id: i64) -> String {
    format!(
        "{}/{}/_workitems/edit/{id}",
        org_url.trim_end_matches('/'),
        project.replace(' ', "%20")
    )
}

/// One comment, rendered off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Azure DevOps' comment id.
    pub id: i64,
    pub author: PersonId,
    pub name: String,
    pub doc: Arc<Doc>,
    /// Plain text with mentions resolved (accessible label).
    pub plain: String,
    pub at: Millis,
    pub edited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    Created,
    Changed,
    Assigned,
    Mentioned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub kind: HistoryKind,
    pub text: String,
    pub at: Millis,
}

/// What a link row points to (spec FR-23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A work item; `item` when it's in the store.
    WorkItem {
        id: i64,
        item: Option<Box<WorkItem>>,
    },
    PullRequest {
        id: i64,
        pr: Option<Box<PullRequest>>,
    },
    Commit {
        sha: String,
        web: Option<String>,
    },
    /// Hyperlinks, branches, builds, attachments: what it is and where it
    /// opens (`None` when there's no browser target).
    Other {
        label: String,
        web: Option<String>,
    },
}

/// One row of the Links section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    pub group: LinkGroup,
    /// Editable work item relation and its stored index (FR-22).
    pub kind: Option<LinkKind>,
    pub index: Option<u32>,
    pub target: LinkTarget,
}

impl LinkRow {
    pub fn work_item_id(&self) -> Option<i64> {
        match self.target {
            LinkTarget::WorkItem { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Rendered long text and its size for the 480 px clip (design §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichDoc {
    pub doc: Arc<Doc>,
    /// Estimated rendered lines at prose width.
    pub lines: usize,
}

/// Whether the description editor opens, and on what (spec FR-17, FR-18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editable {
    /// Markdown field: its source, byte for byte (AC-13).
    Markdown(String),
    /// HTML field that round-trips: the Markdown to edit.
    Html(String),
    /// HTML that can't round-trip: what was found ("a table and 2
    /// images").
    Blocked(String),
    /// Only the plain projection is stored (synced before 0015): the
    /// source must be fetched first.
    NotFetched,
}

/// Everything the detail pane shows for one item.
#[derive(Debug, Clone)]
pub struct DetailData {
    pub item: WorkItem,
    pub project: String,
    pub extras: Option<WorkItemExtras>,
    pub description: Option<RichDoc>,
    pub repro_steps: Option<RichDoc>,
    pub acceptance: Option<RichDoc>,
    pub editable: Editable,
    /// The description source as stored (conflict Compare, FR-19).
    pub description_source: Option<RichText>,
    pub links: Vec<LinkRow>,
    pub comments: Vec<Comment>,
    /// Newest first.
    pub history: Vec<HistoryEntry>,
    pub drafts: Vec<WiDraft>,
    pub freshness: crate::workitems::writes::FreshnessInput,
}

impl DetailData {
    pub fn draft(&self, kind: DraftKind) -> Option<&WiDraft> {
        self.drafts.iter().find(|d| d.kind == kind)
    }

    /// `(group, target id)` of the work item links (picker refusals).
    pub fn linked(&self) -> Vec<(LinkGroup, i64)> {
        self.links
            .iter()
            .filter_map(|row| Some((row.group, row.work_item_id()?)))
            .collect()
    }

    pub fn parent(&self) -> Option<&LinkRow> {
        self.links.iter().find(|row| row.group == LinkGroup::Parent)
    }

    /// Description, repro steps and acceptance criteria were never fetched
    /// (a title-index row, FR-29).
    pub fn partly_synced(&self) -> bool {
        self.extras
            .as_ref()
            .is_none_or(|extras| extras.full_synced_ms.is_none())
    }
}

/// One `sync_event` row about a work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemEvent {
    pub kind: SyncEventKind,
    pub at: Millis,
    pub summary: String,
}

/// Characters per rendered line at the 720 px prose width (`type.body`),
/// for the "Show all · N lines" estimate.
const CHARS_PER_LINE: usize = 96;

/// Parsed documents by `(item id, rev or comment updated, slot)`.
static DOCS: LazyLock<Mutex<DocCache<(i64, i64, i64)>>> =
    LazyLock::new(|| Mutex::new(DocCache::new(512)));

/// Slot keys in [`DOCS`]: negative for fields, comment ids otherwise.
const DESCRIPTION: i64 = -1;
const REPRO: i64 = -2;
const ACCEPTANCE: i64 = -3;

fn format(format: TextFormat) -> Format {
    match format {
        TextFormat::Html => Format::Html,
        TextFormat::Markdown => Format::Markdown,
    }
}

/// `text` parsed once per key (FR-2).
fn parse_cached(key: (i64, i64, i64), text: &RichText) -> Arc<Doc> {
    if let Some(doc) = DOCS.lock().ok().and_then(|mut cache| cache.get(&key)) {
        return doc;
    }
    let doc = Arc::new(adoken_richtext::parse(&text.source, format(text.format)));
    if let Ok(mut cache) = DOCS.lock() {
        cache.insert(key, doc.clone());
    }
    doc
}

/// Plain text as paragraphs (fields synced before 0015, design §18).
pub fn plain_doc(text: &str) -> Doc {
    Doc::from_blocks(
        paragraphs(text)
            .into_iter()
            .map(|p| Block::Paragraph(vec![Inline::Text(Span::plain(p))]))
            .collect(),
    )
}

fn rich(doc: Arc<Doc>) -> RichDoc {
    let lines = doc.line_estimate(CHARS_PER_LINE);
    RichDoc { doc, lines }
}

/// FR-17/18: what the description editor opens on.
pub fn editable(source: Option<&RichText>) -> Editable {
    match source {
        None => Editable::NotFetched,
        Some(text) if text.format == TextFormat::Markdown => {
            Editable::Markdown(text.source.clone())
        }
        Some(text) => match adoken_richtext::editable_html(&text.source) {
            Ok(markdown) => Editable::Html(markdown),
            Err(blocked) => Editable::Blocked(blocked.describe()),
        },
    }
}

/// Blocking detail read; `Ok(None)` when `id` isn't in the store.
pub fn load_detail(
    store: &Store,
    id: i64,
    names: &HashMap<PersonId, String>,
    projects: &HashMap<String, String>,
) -> adoken_store::Result<Option<DetailData>> {
    let freshness = match crate::workitems::writes::load_freshness_input(store, id) {
        Ok(input) => input,
        Err(error) => {
            log::warn!("work items: couldn't read #{id}'s freshness: {error}");
            Default::default()
        }
    };
    store.with_conn(|c| {
        let Some(item) = read::work_item(c, id)? else {
            return Ok(None);
        };
        let extras = adoken_store::wi::extras(c, id)?;
        let rev = item.rev;
        let field = |slot: i64, text: Option<&RichText>| {
            text.filter(|t| !t.source.trim().is_empty())
                .map(|t| rich(parse_cached((id, rev, slot), t)))
        };
        let description_source = extras.as_ref().and_then(|e| e.description.clone());
        let description = match &extras {
            Some(extras) if extras.description.is_some() => {
                field(DESCRIPTION, extras.description.as_ref())
            }
            _ => item
                .description
                .as_deref()
                .filter(|d| !d.trim().is_empty())
                .map(|d| rich(Arc::new(plain_doc(d)))),
        };
        let repro_steps = field(REPRO, extras.as_ref().and_then(|e| e.repro_steps.as_ref()));
        let acceptance = field(
            ACCEPTANCE,
            extras.as_ref().and_then(|e| e.acceptance.as_ref()),
        );
        let links = link_rows(c, &item, extras.as_ref())?;
        let comments = adoken_store::wi::comments(c, id)?
            .into_iter()
            .map(|comment| {
                let doc = match &comment.source {
                    Some(source) => parse_cached(
                        (id, comment.updated, comment.id),
                        &RichText {
                            source: source.clone(),
                            format: comment.format,
                        },
                    ),
                    None => Arc::new(plain_doc(&comment.body)),
                };
                Comment {
                    id: comment.id,
                    name: name(names, &comment.author),
                    plain: resolve_mentions(&comment.body, names),
                    edited: comment.edited(),
                    author: comment.author,
                    doc,
                    at: comment.created,
                }
            })
            .collect();
        let history = history(&item, &events(c, id)?, names);
        let drafts = adoken_store::wi::drafts(c, id)?;
        Ok(Some(DetailData {
            project: projects
                .get(&item.project_id)
                .cloned()
                .unwrap_or_else(|| item.project_id.clone()),
            editable: editable(description_source.as_ref()),
            description_source,
            description,
            repro_steps,
            acceptance,
            extras,
            item,
            links,
            comments,
            history,
            drafts,
            freshness,
        }))
    })
}

/// Link rows in FR-23 order: work item relations by group, then pull
/// requests, commits and other artifacts. Items synced before 0015 have
/// no stored relations: their parent and linked PRs stand in.
fn link_rows(
    c: &Connection,
    item: &WorkItem,
    extras: Option<&WorkItemExtras>,
) -> adoken_store::Result<Vec<LinkRow>> {
    let stored: Vec<WiLink> = extras.map(|e| e.links.clone()).unwrap_or_default();
    let mut rows = Vec::with_capacity(stored.len().max(1 + item.linked_pr_ids.len()));
    if stored.is_empty() {
        if let Some(parent) = item.parent_id {
            rows.push(LinkRow {
                group: LinkGroup::Parent,
                kind: Some(LinkKind::Parent),
                index: None,
                target: work_item_target(c, parent)?,
            });
        }
        for &pr in &item.linked_pr_ids {
            rows.push(LinkRow {
                group: LinkGroup::PullRequests,
                kind: None,
                index: None,
                target: LinkTarget::PullRequest {
                    id: pr,
                    pr: read::pr(c, pr)?.map(Box::new),
                },
            });
        }
        return Ok(rows);
    }
    for link in stored {
        let row = if let (Some(kind), Some(target)) = (link.kind, link.target_id) {
            LinkRow {
                group: LinkGroup::of_kind(kind),
                kind: Some(kind),
                index: Some(link.index),
                target: work_item_target(c, target)?,
            }
        } else if let Some(pr) = link.pr_id {
            LinkRow {
                group: LinkGroup::PullRequests,
                kind: None,
                index: Some(link.index),
                target: LinkTarget::PullRequest {
                    id: pr,
                    pr: read::pr(c, pr)?.map(Box::new),
                },
            }
        } else {
            let group = LinkGroup::of_artifact(false, &link.url);
            let target = match group {
                LinkGroup::Commits => LinkTarget::Commit {
                    sha: commit_sha(&link.url).to_owned(),
                    web: None,
                },
                _ => LinkTarget::Other {
                    label: link
                        .comment
                        .clone()
                        .filter(|c| !c.trim().is_empty())
                        .or_else(|| web_host(&link.url))
                        .or_else(|| link.name.clone())
                        .unwrap_or_else(|| link.rel.clone()),
                    web: link.url.starts_with("http").then(|| link.url.clone()),
                },
            };
            LinkRow {
                group,
                kind: None,
                index: Some(link.index),
                target,
            }
        };
        rows.push(row);
    }
    // Stable: server order within each group.
    rows.sort_by_key(|row| row.group);
    Ok(rows)
}

fn work_item_target(c: &Connection, id: i64) -> adoken_store::Result<LinkTarget> {
    Ok(LinkTarget::WorkItem {
        id,
        item: read::work_item(c, id)?.map(|mut wi| {
            wi.description = None;
            Box::new(wi)
        }),
    })
}

/// `vstfs:///Git/Commit/{project}%2F{repo}%2F{sha}` → the SHA.
pub fn commit_sha(url: &str) -> &str {
    url.rsplit("%2F")
        .next()
        .and_then(|tail| tail.rsplit('/').next())
        .unwrap_or(url)
}

/// `https://example.com/path` → `example.com`.
fn web_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    Some(rest.split('/').next().unwrap_or(rest).to_owned())
}

/// The item's sync events, newest first. The `IN` on `kind` lets SQLite
/// probe the `(kind, subject_key, dedupe_key)` unique index.
fn events(c: &Connection, id: i64) -> adoken_store::Result<Vec<ItemEvent>> {
    let mut st = c.prepare_cached(
        "SELECT kind, at_ms, summary FROM sync_event
         WHERE kind IN ('WorkItemChanged', 'WorkItemAssigned', 'MentionedInWorkItem')
           AND subject_key = ?1
         ORDER BY at_ms DESC, id DESC LIMIT ?2",
    )?;
    let key = adoken_model::Subject::WorkItem(id).key();
    let rows = st.query_map(params![key, HISTORY_LIMIT as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, at, summary) = row?;
        if let Some(kind) = SyncEventKind::parse(&kind) {
            out.push(ItemEvent { kind, at, summary });
        }
    }
    Ok(out)
}

/// History newest first: synced events, the last change (unless an event
/// already covers that moment) and creation.
pub fn history(
    item: &WorkItem,
    events: &[ItemEvent],
    names: &HashMap<PersonId, String>,
) -> Vec<HistoryEntry> {
    let mut out: Vec<HistoryEntry> = events
        .iter()
        .map(|e| HistoryEntry {
            kind: match e.kind {
                SyncEventKind::WorkItemAssigned => HistoryKind::Assigned,
                SyncEventKind::MentionedInWorkItem => HistoryKind::Mentioned,
                _ => HistoryKind::Changed,
            },
            text: e.summary.clone(),
            at: e.at,
        })
        .collect();
    if item.changed > item.created && !events.iter().any(|e| e.at == item.changed) {
        out.push(HistoryEntry {
            kind: HistoryKind::Changed,
            text: format!(
                "{} changed it (revision {}), state {}",
                name(names, &item.changed_by),
                item.rev,
                item.state
            ),
            at: item.changed,
        });
    }
    out.push(HistoryEntry {
        kind: HistoryKind::Created,
        text: format!("{} created it", name(names, &item.created_by)),
        at: item.created,
    });
    out.sort_by_key(|h| std::cmp::Reverse(h.at));
    out
}

/// `@<guid>` → `@Name` (`@someone` when the person isn't synced).
pub fn resolve_mentions(body: &str, names: &HashMap<PersonId, String>) -> String {
    let mut out = body.to_owned();
    for id in read::mention_ids(body) {
        let display = names.get(id).map_or("someone", String::as_str);
        out = out.replace(&format!("@<{id}>"), &format!("@{display}"));
    }
    out
}

/// Plain-text paragraphs (blank-line separated), trimmed, empties dropped.
pub fn paragraphs(text: &str) -> Vec<&str> {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::time::DAY;
    use adoken_model::{StateCategory, WorkItemType};

    fn item() -> WorkItem {
        WorkItem {
            id: 7,
            project_id: "p".into(),
            rev: 4,
            kind: WorkItemType::Bug,
            title: "t".into(),
            state: "Active".into(),
            state_category: StateCategory::InProgress,
            assigned_to: None,
            created_by: "a".into(),
            changed_by: "b".into(),
            area_path: String::new(),
            iteration_path: String::new(),
            priority: None,
            tags: Vec::new(),
            parent_id: None,
            linked_pr_ids: Vec::new(),
            description: None,
            comment_count: 0,
            created: 10 * DAY,
            changed: 12 * DAY,
        }
    }

    fn names() -> HashMap<PersonId, String> {
        [("a".into(), "Ana".into()), ("b".into(), "Bo".into())].into()
    }

    #[test]
    fn history_is_newest_first_with_creation_and_last_change() {
        let events = [ItemEvent {
            kind: SyncEventKind::WorkItemAssigned,
            at: 11 * DAY,
            summary: "Ana assigned Bug 7 to you: t".into(),
        }];
        let history = history(&item(), &events, &names());
        let kinds: Vec<_> = history.iter().map(|h| (h.kind, h.at)).collect();
        assert_eq!(
            kinds,
            [
                (HistoryKind::Changed, 12 * DAY),
                (HistoryKind::Assigned, 11 * DAY),
                (HistoryKind::Created, 10 * DAY)
            ]
        );
        assert_eq!(history[0].text, "Bo changed it (revision 4), state Active");
        assert_eq!(history[2].text, "Ana created it");
    }

    #[test]
    fn an_event_at_the_last_change_replaces_the_synthesized_entry() {
        let events = [ItemEvent {
            kind: SyncEventKind::WorkItemChanged,
            at: 12 * DAY,
            summary: "Bo changed Bug 7: t".into(),
        }];
        let entries = history(&item(), &events, &names());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Bo changed Bug 7: t");
        // Never-changed items only show creation.
        let mut fresh = item();
        fresh.changed = fresh.created;
        assert_eq!(history(&fresh, &[], &names()).len(), 1);
    }

    #[test]
    fn mentions_resolve_to_names_and_unknown_people_stay_anonymous() {
        let body = "@<a> can you check with @<zz>? cc @<a>";
        assert_eq!(
            resolve_mentions(body, &names()),
            "@Ana can you check with @someone? cc @Ana"
        );
    }

    #[test]
    fn paragraphs_split_on_blank_lines() {
        assert_eq!(
            paragraphs("One.\n\n\n  Two\nlines.  \n\n"),
            ["One.", "Two\nlines."]
        );
        assert!(paragraphs("  ").is_empty());
    }

    #[test]
    fn web_urls_escape_project_spaces() {
        assert_eq!(
            web_url("https://dev.azure.com/contoso/", "Mobile App", 5),
            "https://dev.azure.com/contoso/Mobile%20App/_workitems/edit/5"
        );
    }

    #[test]
    fn demo_store_details_load_links_comments_and_history() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::DEMO_NOW).expect("seed");
        let list = ListData::read(&store).expect("list");
        assert!(list.demo);
        let mine: Vec<&WorkItem> = list
            .items
            .iter()
            .filter(|i| i.assigned_to.as_deref() == Some(list.me.as_str()))
            .collect();
        assert!(!mine.is_empty(), "every item assigned to me is loaded");
        let linked = mine
            .iter()
            .find(|i| i.linked_pr_ids == [4295])
            .expect("scenario item");
        let detail = load_detail(&store, linked.id, &list.names, &list.projects)
            .expect("detail")
            .expect("present");
        let pr = detail
            .links
            .iter()
            .find_map(|row| match &row.target {
                LinkTarget::PullRequest { id: 4295, pr } => Some(pr),
                _ => None,
            })
            .expect("the linked PR is a Pull requests row");
        assert!(pr.is_some(), "linked PR resolves from the store");
        assert_eq!(
            detail.history.last().map(|h| h.kind),
            Some(HistoryKind::Created)
        );
        let commented = mine
            .iter()
            .find(|i| i.comment_count > 0)
            .expect("an item of mine with comments");
        let detail = load_detail(&store, commented.id, &list.names, &list.projects)
            .expect("detail")
            .expect("present");
        assert_eq!(detail.comments.len(), commented.comment_count as usize);
        assert!(
            detail.comments.iter().all(|c| !c.plain.contains("@<")),
            "mentions resolve to names"
        );
        assert!(
            load_detail(&store, 1, &list.names, &list.projects)
                .unwrap()
                .is_none()
        );
    }

    fn demo_detail(index: usize) -> (DetailData, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::DEMO_NOW).expect("seed");
        let list = ListData::read(&store).expect("list");
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);
        let detail = load_detail(&store, ds.work_items[index].id, &list.names, &list.projects)
            .expect("detail")
            .expect("present");
        (detail, dir)
    }

    /// AC-1 (view side): the stored HTML description renders as a heading,
    /// a two-level list, a link, a mention and an image chip, parsed off
    /// the UI thread from its source; images block editing (AC-15).
    #[test]
    fn ac1_ac15_html_description_renders_rich_blocks_and_images_block_editing() {
        use adoken_richtext::{Block, Inline};
        let (detail, _dir) = demo_detail(adoken_demo::rich::SCENARIO);
        let doc = &detail.description.as_ref().expect("description").doc;
        let mut inlines = Vec::new();
        doc.visit_inlines(&mut |inline| inlines.push(inline.clone()));
        assert!(
            doc.blocks
                .iter()
                .any(|b| matches!(b, Block::Heading { .. })),
            "a heading"
        );
        let nested = doc.blocks.iter().any(|b| match b {
            Block::List(list) => list
                .items
                .iter()
                .any(|item| item.blocks.iter().any(|b| matches!(b, Block::List(_)))),
            _ => false,
        });
        assert!(nested, "list items at two levels");
        assert!(
            inlines.iter().any(|i| matches!(i, Inline::Mention(_))),
            "a mention"
        );
        assert!(
            inlines.iter().any(|i| matches!(i, Inline::Image { .. })),
            "an image chip"
        );
        assert!(
            inlines
                .iter()
                .any(|i| matches!(i, Inline::Text(span) if span.link.is_some())),
            "a link"
        );
        assert_eq!(detail.editable, Editable::Blocked("an image".into()));
        assert!(!detail.partly_synced());
        assert!(detail.repro_steps.is_some(), "Repro steps render (Q-10)");
    }

    /// AC-15 (view side): tables, images and colors block the editor and
    /// name what was found.
    #[test]
    fn ac15_table_images_and_colors_keep_the_editor_closed() {
        let (detail, _dir) = demo_detail(adoken_demo::rich::BLOCKED);
        let Editable::Blocked(what) = &detail.editable else {
            panic!("blocked: {:?}", detail.editable);
        };
        assert!(what.contains("a table"), "{what}");
        assert!(what.contains("2 images"), "{what}");
    }

    /// AC-13 (view side): a Markdown field opens on its source byte for
    /// byte; an HTML field that round-trips opens as Markdown.
    #[test]
    fn ac13_markdown_fields_edit_their_source_and_html_converts() {
        let (markdown, _a) = demo_detail(adoken_demo::rich::MARKDOWN);
        let source = &markdown.description_source.as_ref().expect("source").source;
        assert_eq!(markdown.editable, Editable::Markdown(source.clone()));
        assert!(
            markdown.acceptance.is_some(),
            "Acceptance criteria render (Q-10)"
        );
        let (html, _b) = demo_detail(adoken_demo::rich::HTML);
        let Editable::Html(text) = &html.editable else {
            panic!("HTML round-trips: {:?}", html.editable);
        };
        assert!(text.contains("refund rules"), "{text}");
    }
}
