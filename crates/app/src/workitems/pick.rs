//! The shared work item picker engine (spec 0004 FR-17, reused by spec
//! 0015 FR-21): the `Cmd+P` engine and query language scoped to work
//! items, so `#1234`, bare ids, filters and pasted work item links behave
//! as in the palette. The PR review's Link picker and the work item
//! detail's Add link picker render their own frames over these rows.

use adoken_model::WorkItem;
use adoken_search::DocKind;
use adoken_store::{Store, WorkItemFilter};
use gpui::SharedString;

use crate::palette::catalog::{Catalog, CatalogData, Item, Row, Target};
use crate::palette::query::{Jump, LinkTarget, Query};

/// Results the picker keeps per query.
pub const PICKER_LIMIT: usize = 50;

/// One row of the work item picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickRow {
    /// A synced work item.
    Item(Box<WorkItem>),
    /// `#1234` or a pasted link to a work item adoken hasn't synced: it
    /// links all the same.
    NotSynced(i64),
    /// Why the query can't link anything (a link to a PR, another org).
    Note(SharedString),
}

impl PickRow {
    pub fn id(&self) -> Option<i64> {
        match self {
            PickRow::Item(item) => Some(item.id),
            PickRow::NotSynced(id) => Some(*id),
            PickRow::Note(_) => None,
        }
    }
}

/// The picker's rows for `text` over `catalog` (synced work items).
pub fn picker_rows(catalog: &Catalog, text: &str, now: i64) -> Vec<PickRow> {
    let mut query = Query::parse(text);
    if let Some(Jump::Link(link)) = &query.jump
        && !matches!(link.target, LinkTarget::WorkItem(_))
    {
        return vec![PickRow::Note("That link isn't a work item".into())];
    }
    if query.scope != Some(DocKind::WorkItem) {
        query.scope = Some(DocKind::WorkItem);
        query.jump = match query.jump {
            Some(Jump::Id(id)) => Some(Jump::WorkItem(id)),
            Some(Jump::PullRequest(_)) => None,
            jump => jump,
        };
    }
    catalog
        .run(&query, PICKER_LIMIT, now)
        .into_iter()
        .filter_map(|row| match row {
            Row::Pin(_) => None,
            Row::Item { item, .. } => match catalog.item(item) {
                Item::WorkItem(wi) => Some(PickRow::Item(Box::new(wi.clone()))),
                _ => None,
            },
            Row::NotSynced(Target::WorkItem(id)) => Some(PickRow::NotSynced(id)),
            Row::NotSynced(_) => None,
            Row::OtherOrg { org } => Some(PickRow::Note(
                format!("That link is for the {org} organization").into(),
            )),
        })
        .collect()
}

/// Blocking store reads for the picker: the synced work items (no
/// descriptions), the people they name, and the org (pasted links).
pub fn load_catalog(store: &Store) -> adoken_store::Result<Catalog> {
    let mut work_items = store.work_items(&WorkItemFilter::default())?;
    for wi in &mut work_items {
        wi.description = None;
    }
    let data = CatalogData {
        org: store.org()?.map(|org| org.name),
        me: store.me()?.map(|me| me.id),
        people: store.people()?,
        work_items,
        ..CatalogData::default()
    };
    Ok(Catalog::new(data, Vec::new()))
}
