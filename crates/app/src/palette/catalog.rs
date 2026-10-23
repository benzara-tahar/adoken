//! What the palette searches: the store's PRs, work items, pipeline runs,
//! people, repos, and projects plus the app's commands, indexed for Cmd+P
//! and kept as domain rows so filters (§3.3) and row templates (§5) read
//! real fields.
//!
//! Built off the UI thread ([`CatalogData::load`] + [`Catalog::new`]);
//! immutable afterwards, queried from background executors.

use std::collections::HashMap;

use adoken_model::{
    Person, PersonId, PipelineRun, PrStatus, Project, PullRequest, Repo, RunResult, RunStatus,
    Vote, WorkItem, WorkItemType, short_branch,
};
use adoken_search::{DocKind, Index, SearchDoc};
use adoken_store::{PrFilter, RunFilter, Store, WorkItemFilter};

use super::query::{DateRange, Filter, FilterKey, Jump, LinkTarget, Query};
use crate::pipelines::model::format_duration as duration;

/// Scoped / filtered queries over-fetch from the matcher, then narrow.
const CANDIDATES: usize = 5_000;
/// Rows in the empty-query "My work" list.
const MY_WORK_ROWS: usize = 30;

/// A command row: title, aliases for matching, and its keybinding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub title: &'static str,
    pub keys: Option<&'static str>,
}

/// Raw rows read from the store.
#[derive(Debug, Default, Clone)]
pub struct CatalogData {
    pub org: Option<String>,
    pub org_url: Option<String>,
    pub me: Option<PersonId>,
    pub people: Vec<Person>,
    pub projects: Vec<Project>,
    pub repos: Vec<Repo>,
    pub prs: Vec<PullRequest>,
    pub work_items: Vec<WorkItem>,
    pub runs: Vec<PipelineRun>,
}

impl CatalogData {
    /// Blocking store reads; call on a background executor.
    pub fn load(store: &Store) -> adoken_store::Result<Self> {
        let mut prs = store.prs(&PrFilter::default())?;
        let mut work_items = store.work_items(&WorkItemFilter::default())?;
        // Descriptions aren't searched or shown in rows; don't keep them.
        for pr in &mut prs {
            pr.description = String::new();
        }
        for wi in &mut work_items {
            wi.description = None;
        }
        Ok(Self {
            org: store.org()?.map(|org| org.name),
            org_url: if store.meta("demo")?.is_some() {
                None
            } else {
                store.org()?.map(|org| org.url)
            },
            me: store.me()?.map(|me| me.id),
            people: store.people()?,
            projects: store.projects()?,
            repos: store.repos()?,
            prs,
            work_items,
            runs: store.runs(&RunFilter::default())?,
        })
    }
}

impl CatalogData {
    /// The demo dataset in store order (newest first), without IO; for
    /// gallery stories and tests.
    pub fn from_dataset(ds: &adoken_demo::Dataset) -> Self {
        let mut prs = ds.prs.clone();
        prs.sort_by_key(|pr| std::cmp::Reverse(pr.updated));
        let mut work_items = ds.work_items.clone();
        work_items.sort_by_key(|wi| std::cmp::Reverse(wi.changed));
        let mut runs = ds.runs.clone();
        runs.sort_by_key(|run| std::cmp::Reverse(run.queued));
        Self {
            org: Some(ds.org.name.clone()),
            me: Some(ds.me.clone()),
            org_url: None,
            people: ds.people.clone(),
            projects: ds.projects.clone(),
            repos: ds.repos.clone(),
            prs,
            work_items,
            runs,
        }
    }
}

/// One searchable thing, parallel to the index docs.
#[derive(Debug, Clone)]
pub enum Item {
    PullRequest(PullRequest),
    WorkItem(WorkItem),
    Run(PipelineRun),
    Person(Person),
    Repo(Repo),
    Project(Project),
    Command(usize),
}

impl Item {
    pub fn kind(&self) -> DocKind {
        match self {
            Item::PullRequest(_) => DocKind::PullRequest,
            Item::WorkItem(_) => DocKind::WorkItem,
            Item::Run(_) => DocKind::Pipeline,
            Item::Person(_) => DocKind::Person,
            Item::Repo(_) => DocKind::Repo,
            Item::Project(_) => DocKind::Project,
            Item::Command(_) => DocKind::Command,
        }
    }

    pub fn pin_subject(&self) -> Option<adoken_model::PinSubject> {
        use adoken_model::PinSubject;
        match self {
            Item::PullRequest(pr) => Some(PinSubject::Pr(pr.id)),
            Item::WorkItem(wi) => Some(PinSubject::WorkItem(wi.id)),
            Item::Run(run) => Some(PinSubject::Definition {
                project_id: run.project_id.clone(),
                id: run.pipeline_id,
            }),
            _ => None,
        }
    }
}

/// Where opening a row goes (the shell routes it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    PullRequest(i64),
    WorkItem(i64),
    Run(i64),
}

/// One result row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Pin(adoken_model::PinSubject),
    /// A catalog item; `positions` are matched title chars.
    Item {
        item: u32,
        positions: Vec<u32>,
    },
    /// A direct jump to an id that isn't in the store (§3.2).
    NotSynced(Target),
    /// A pasted link for another organization.
    OtherOrg {
        org: String,
    },
}

pub struct Catalog {
    index: Index,
    items: Vec<Item>,
    commands: Vec<CommandEntry>,
    org: Option<String>,
    org_url: Option<String>,
    me: Option<PersonId>,
    people: HashMap<PersonId, Person>,
    repos: HashMap<String, Repo>,
    projects: HashMap<String, Project>,
    by_target: HashMap<(DocKind, i64), u32>,
    by_pin: HashMap<adoken_model::PinSubject, u32>,
    open_prs_by_author: HashMap<PersonId, usize>,
    open_prs_by_repo: HashMap<String, usize>,
    open_prs_by_project: HashMap<String, usize>,
    /// Whether store data is in (commands alone before that).
    loaded: bool,
}

impl Catalog {
    /// Commands only: available on the first frame, before the store loads.
    pub fn commands_only(commands: Vec<CommandEntry>) -> Self {
        let mut catalog = Self::new(CatalogData::default(), commands);
        catalog.loaded = false;
        catalog
    }

    pub fn new(data: CatalogData, commands: Vec<CommandEntry>) -> Self {
        let people: HashMap<_, _> = data
            .people
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        let repos: HashMap<_, _> = data
            .repos
            .iter()
            .map(|r| (r.id.clone(), r.clone()))
            .collect();
        let projects: HashMap<_, _> = data
            .projects
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        let name_of = |id: &str| people.get(id).map_or("", |p| p.display_name.as_str());
        let repo_name = |id: &str| repos.get(id).map_or("", |r| r.name.as_str());
        let project_name = |id: &str| projects.get(id).map_or("", |p| p.name.as_str());

        let mut open_prs_by_author: HashMap<PersonId, usize> = HashMap::new();
        let mut open_prs_by_repo: HashMap<String, usize> = HashMap::new();
        let mut open_prs_by_project: HashMap<String, usize> = HashMap::new();
        for pr in data.prs.iter().filter(|pr| pr.status == PrStatus::Active) {
            *open_prs_by_author.entry(pr.author.clone()).or_default() += 1;
            *open_prs_by_repo.entry(pr.repo_id.clone()).or_default() += 1;
            *open_prs_by_project
                .entry(pr.project_id.clone())
                .or_default() += 1;
        }

        let capacity = data.prs.len()
            + data.work_items.len()
            + data.runs.len()
            + data.people.len()
            + data.repos.len()
            + data.projects.len()
            + commands.len();
        let mut docs = Vec::with_capacity(capacity);
        let mut items = Vec::with_capacity(capacity);
        let mut by_target = HashMap::new();
        let mut push = |doc: SearchDoc, item: Item| {
            let ix = items.len() as u32;
            match &item {
                Item::PullRequest(pr) => {
                    by_target.insert((DocKind::PullRequest, pr.id), ix);
                }
                Item::WorkItem(wi) => {
                    by_target.insert((DocKind::WorkItem, wi.id), ix);
                }
                Item::Run(run) => {
                    by_target.insert((DocKind::Pipeline, run.id), ix);
                }
                _ => {}
            }
            docs.push(doc);
            items.push(item);
        };
        // FR-2 indexed text per type.
        for pr in data.prs {
            let subtitle = format!(
                "{} {} {} {}",
                repo_name(&pr.repo_id),
                short_branch(&pr.source_branch),
                short_branch(&pr.target_branch),
                name_of(&pr.author)
            );
            push(
                SearchDoc {
                    id: pr.id as u64,
                    kind: DocKind::PullRequest,
                    title: pr.title.clone(),
                    subtitle,
                },
                Item::PullRequest(pr),
            );
        }
        for wi in data.work_items {
            let subtitle = format!(
                "{} {} {} {}",
                wi.kind.clone(),
                wi.state,
                wi.assigned_to.as_deref().map_or("", name_of),
                wi.tags.join(" ")
            );
            push(
                SearchDoc {
                    id: wi.id as u64,
                    kind: DocKind::WorkItem,
                    title: wi.title.clone(),
                    subtitle,
                },
                Item::WorkItem(wi),
            );
        }
        for run in data.runs {
            let subtitle = format!("{} {}", run.run_number, short_branch(&run.branch));
            push(
                SearchDoc {
                    id: run.id as u64,
                    kind: DocKind::Pipeline,
                    title: run.pipeline_name.clone(),
                    subtitle,
                },
                Item::Run(run),
            );
        }
        for (ix, person) in data.people.into_iter().enumerate() {
            push(
                SearchDoc {
                    id: ix as u64,
                    kind: DocKind::Person,
                    title: person.display_name.clone(),
                    subtitle: person.unique_name.clone().unwrap_or_default(),
                },
                Item::Person(person),
            );
        }
        for (ix, repo) in data.repos.into_iter().enumerate() {
            push(
                SearchDoc {
                    id: ix as u64,
                    kind: DocKind::Repo,
                    title: repo.name.clone(),
                    subtitle: project_name(&repo.project_id).to_owned(),
                },
                Item::Repo(repo),
            );
        }
        for (ix, project) in data.projects.into_iter().enumerate() {
            push(
                SearchDoc {
                    id: ix as u64,
                    kind: DocKind::Project,
                    title: project.name.clone(),
                    subtitle: String::new(),
                },
                Item::Project(project),
            );
        }
        for (ix, command) in commands.iter().enumerate() {
            push(
                SearchDoc {
                    id: ix as u64,
                    kind: DocKind::Command,
                    title: command.title.to_owned(),
                    subtitle: String::new(),
                },
                Item::Command(ix),
            );
        }
        let mut by_pin = HashMap::new();
        for (ix, item) in items.iter().enumerate() {
            if let Some(subject) = item.pin_subject() {
                by_pin.entry(subject).or_insert(ix as u32);
            }
        }

        Self {
            index: Index::new(docs),
            items,
            commands,
            org: data.org,
            org_url: data.org_url,
            me: data.me,
            people,
            repos,
            projects,
            by_target,
            by_pin,
            open_prs_by_author,
            open_prs_by_repo,
            open_prs_by_project,
            loaded: true,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn item(&self, ix: u32) -> &Item {
        &self.items[ix as usize]
    }

    pub fn title(&self, ix: u32) -> &str {
        &self.index.docs()[ix as usize].title
    }

    pub fn command(&self, ix: usize) -> &CommandEntry {
        &self.commands[ix]
    }

    pub fn org(&self) -> Option<&str> {
        self.org.as_deref()
    }

    pub fn me(&self) -> Option<&str> {
        self.me.as_deref()
    }

    pub fn person_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.people.get(id).map_or(id, |p| p.display_name.as_str())
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.get(id)
    }

    pub fn repo_name(&self, id: &str) -> &str {
        self.repos.get(id).map_or("", |r| r.name.as_str())
    }

    pub fn project_name(&self, id: &str) -> &str {
        self.projects.get(id).map_or("", |p| p.name.as_str())
    }

    pub fn open_prs_by_author(&self, id: &str) -> usize {
        self.open_prs_by_author.get(id).copied().unwrap_or(0)
    }

    pub fn open_prs_in_repo(&self, id: &str) -> usize {
        self.open_prs_by_repo.get(id).copied().unwrap_or(0)
    }

    pub fn open_prs_in_project(&self, id: &str) -> usize {
        self.open_prs_by_project.get(id).copied().unwrap_or(0)
    }

    fn find(&self, target: &Target) -> Option<u32> {
        let key = match *target {
            Target::PullRequest(id) => (DocKind::PullRequest, id),
            Target::WorkItem(id) => (DocKind::WorkItem, id),
            Target::Run(id) => (DocKind::Pipeline, id),
        };
        self.by_target.get(&key).copied()
    }

    /// Where opening `item` goes, for content rows the shell routes.
    pub fn target(&self, ix: u32) -> Option<Target> {
        match self.item(ix) {
            Item::PullRequest(pr) => Some(Target::PullRequest(pr.id)),
            Item::WorkItem(wi) => Some(Target::WorkItem(wi.id)),
            Item::Run(run) => Some(Target::Run(run.id)),
            _ => None,
        }
    }

    pub fn pin_subject(&self, ix: u32) -> Option<adoken_model::PinSubject> {
        self.item(ix).pin_subject()
    }

    pub fn web_url(&self, ix: u32) -> Option<String> {
        let org = self.org_url.as_deref()?;
        match self.item(ix) {
            Item::PullRequest(pr) => crate::review::web::web_url(
                org,
                self.project_name(&pr.project_id),
                self.repo_name(&pr.repo_id),
                pr.id,
            ),
            Item::WorkItem(wi) => Some(crate::workitems::data::web_url(
                org,
                self.project_name(&wi.project_id),
                wi.id,
            )),
            Item::Run(run) => {
                let mut url = url::Url::parse(org).ok()?;
                url.path_segments_mut().ok()?.pop_if_empty().extend([
                    self.project_name(&run.project_id),
                    "_build",
                    "results",
                ]);
                url.query_pairs_mut()
                    .append_pair("buildId", &run.id.to_string());
                Some(url.into())
            }
            _ => None,
        }
    }

    pub fn pin_matches(&self, subject: &adoken_model::PinSubject, query: &Query, now: i64) -> bool {
        if query.filters.is_empty() {
            return true;
        }
        let filters = Prepared::new(self, &query.filters, now);
        if let Some(ix) = self.by_pin.get(subject) {
            let item = self.item(*ix);
            return query.admits(item.kind()) && filters.matches(self, item);
        }
        if let adoken_model::PinSubject::Definition { project_id, .. } = subject {
            return query.filters.iter().all(|filter| {
                if filter.key != FilterKey::Project {
                    return false;
                }
                let matched = filter.values.iter().any(|value| {
                    value.eq_ignore_ascii_case(project_id)
                        || value.eq_ignore_ascii_case(self.project_name(project_id))
                });
                matched != filter.negated
            });
        }
        false
    }

    /// The query a person / repo / project row refines to (§4.4 ↩ column).
    pub fn refine_query(&self, ix: u32) -> Option<String> {
        let quote = |s: &str| {
            if s.contains(char::is_whitespace) {
                format!("\"{s}\"")
            } else {
                s.to_owned()
            }
        };
        match self.item(ix) {
            Item::Person(p) => {
                let handle = p
                    .unique_name
                    .as_deref()
                    .and_then(|u| u.split('@').next())
                    .unwrap_or(&p.display_name);
                Some(format!("! author:@{} is:open", quote(handle)))
            }
            Item::Repo(r) => Some(format!("! repo:{} is:open", quote(&r.name))),
            Item::Project(p) => Some(format!("! project:{} is:open", quote(&p.name))),
            _ => None,
        }
    }

    /// Runs one query; called on a background executor.
    pub fn run(&self, query: &Query, limit: usize, now: i64) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(Jump::Link(link)) = &query.jump {
            let target = match link.target {
                LinkTarget::PullRequest(id) => Target::PullRequest(id),
                LinkTarget::WorkItem(id) => Target::WorkItem(id),
                LinkTarget::Build(id) => Target::Run(id),
            };
            let same_org = self
                .org
                .as_deref()
                .is_none_or(|org| org.eq_ignore_ascii_case(&link.org));
            rows.push(match (same_org, self.find(&target)) {
                (false, _) => Row::OtherOrg {
                    org: link.org.clone(),
                },
                (true, Some(item)) => Row::Item {
                    item,
                    positions: Vec::new(),
                },
                (true, None) => Row::NotSynced(target),
            });
            return rows;
        }
        let missing = match query.jump {
            Some(Jump::PullRequest(id)) => Some(Target::PullRequest(id)),
            Some(Jump::WorkItem(id)) => Some(Target::WorkItem(id)),
            _ => None,
        };
        if let Some(target) = missing.filter(|t| self.loaded && self.find(t).is_none()) {
            rows.push(Row::NotSynced(target));
        }
        if query.is_blank() {
            rows.extend(self.my_work(limit));
            return rows;
        }

        let filters = Prepared::new(self, &query.filters, now);
        let keep = |ix: u32| {
            let item = self.item(ix);
            query.admits(item.kind()) && filters.matches(self, item)
        };
        if query.text.is_empty() {
            // A bare scope and/or filters list everything that passes, in the
            // store's order (newest first).
            rows.extend(
                (0..self.items.len() as u32)
                    .filter(|&ix| keep(ix))
                    .take(limit)
                    .map(|item| Row::Item {
                        item,
                        positions: Vec::new(),
                    }),
            );
            return rows;
        }
        let narrowed = query.scope.is_some() || !query.filters.is_empty();
        let fetch = if narrowed {
            CANDIDATES.max(limit)
        } else {
            limit
        };
        rows.extend(
            self.index
                .query(&query.text, fetch)
                .into_iter()
                .filter(|hit| keep(hit.doc))
                .take(limit)
                .map(|hit| Row::Item {
                    item: hit.doc,
                    positions: hit.positions,
                }),
        );
        rows
    }

    /// Empty query (§4.2): reviews waiting on me, then my open PRs, then my
    /// active work items.
    fn my_work(&self, limit: usize) -> Vec<Row> {
        let Some(me) = self.me.as_deref() else {
            return Vec::new();
        };
        let row = |item| Row::Item {
            item,
            positions: Vec::new(),
        };
        let pr = |ix: u32| match self.item(ix) {
            Item::PullRequest(pr) if pr.status == PrStatus::Active => Some(pr),
            _ => None,
        };
        let awaiting = (0..self.items.len() as u32).filter(|&ix| {
            pr(ix).is_some_and(|pr| pr.author != me && pr.vote_of(me) == Some(Vote::NoVote))
        });
        let mine =
            (0..self.items.len() as u32).filter(|&ix| pr(ix).is_some_and(|pr| pr.author == me));
        let work = (0..self.items.len() as u32).filter(|&ix| {
            matches!(self.item(ix), Item::WorkItem(wi) if wi.is_open() && wi.assigned_to.as_deref() == Some(me))
        });
        awaiting
            .chain(mine)
            .chain(work)
            .take(limit.min(MY_WORK_ROWS))
            .map(row)
            .collect()
    }
}

/// Filters with person and date values resolved once per query.
struct Prepared<'a> {
    filters: Vec<(&'a Filter, Resolved)>,
}

enum Resolved {
    None,
    People(Vec<PersonId>),
    Dates(Vec<DateRange>),
}

impl<'a> Prepared<'a> {
    fn new(catalog: &Catalog, filters: &'a [Filter], now: i64) -> Self {
        let filters = filters
            .iter()
            .map(|filter| {
                let resolved = match filter.key {
                    FilterKey::Author
                    | FilterKey::Reviewer
                    | FilterKey::Assigned
                    | FilterKey::By => Resolved::People(
                        filter
                            .values
                            .iter()
                            .flat_map(|v| resolve_people(catalog, v))
                            .collect(),
                    ),
                    FilterKey::Updated | FilterKey::Created => Resolved::Dates(
                        filter
                            .values
                            .iter()
                            .filter_map(|v| DateRange::parse(v, now))
                            .collect(),
                    ),
                    _ => Resolved::None,
                };
                (filter, resolved)
            })
            .collect();
        Self { filters }
    }

    /// People named by a `reviewer:` filter (scopes `vote:`).
    fn reviewers(&self) -> Option<&[PersonId]> {
        self.filters
            .iter()
            .find_map(|(filter, resolved)| match resolved {
                Resolved::People(ids) if filter.key == FilterKey::Reviewer && !filter.negated => {
                    Some(ids.as_slice())
                }
                _ => None,
            })
    }

    fn matches(&self, catalog: &Catalog, item: &Item) -> bool {
        self.filters.iter().all(|(filter, resolved)| {
            let any = filter.values.iter().any(|value| {
                value_matches(catalog, item, filter.key, value, resolved, self.reviewers())
            });
            any != filter.negated
        })
    }
}

/// `@me`, or people whose name or email contains the value.
fn resolve_people(catalog: &Catalog, value: &str) -> Vec<PersonId> {
    if value.eq_ignore_ascii_case("@me") {
        return catalog.me.iter().cloned().collect();
    }
    let needle = value.trim_start_matches('@').to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    catalog
        .people
        .values()
        .filter(|p| {
            p.display_name.to_lowercase().contains(&needle)
                || p.unique_name
                    .as_deref()
                    .is_some_and(|u| u.to_lowercase().contains(&needle))
        })
        .map(|p| p.id.clone())
        .collect()
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn value_matches(
    catalog: &Catalog,
    item: &Item,
    key: FilterKey,
    value: &str,
    resolved: &Resolved,
    reviewers: Option<&[PersonId]>,
) -> bool {
    let people = |id: Option<&str>| match (resolved, id) {
        (Resolved::People(ids), Some(id)) => ids.iter().any(|p| p == id),
        _ => false,
    };
    let dates = |at: i64| match resolved {
        Resolved::Dates(ranges) => ranges.iter().any(|r| r.contains(at)),
        _ => false,
    };
    let me = catalog.me.as_deref();
    let value_lc = value.to_ascii_lowercase();
    match (key, item) {
        (FilterKey::Is, Item::PullRequest(pr)) => match value_lc.as_str() {
            "open" | "active" => pr.status == PrStatus::Active,
            "draft" => pr.is_draft,
            "merged" | "completed" => pr.status == PrStatus::Completed,
            "abandoned" => pr.status == PrStatus::Abandoned,
            "mine" => me.is_some_and(|me| pr.author == me),
            "review-requested" => me.is_some_and(|me| pr.is_reviewer(me)),
            _ => false,
        },
        (FilterKey::Is, Item::WorkItem(wi)) => match value_lc.as_str() {
            "mine" => me.is_some() && wi.assigned_to.as_deref() == me,
            "open" => wi.is_open(),
            "closed" => !wi.is_open(),
            _ => false,
        },
        (FilterKey::Author, Item::PullRequest(pr)) => people(Some(&pr.author)),
        (FilterKey::Reviewer, Item::PullRequest(pr)) => {
            pr.reviewers.iter().any(|r| people(Some(&r.person)))
        }
        (FilterKey::Vote, Item::PullRequest(pr)) => {
            let Some(wanted) = parse_vote(&value_lc) else {
                return false;
            };
            // Combined with `reviewer:`, votes of those reviewers; else mine.
            match reviewers {
                Some(ids) => pr
                    .reviewers
                    .iter()
                    .any(|r| r.vote == wanted && ids.contains(&r.person)),
                None => me.and_then(|me| pr.vote_of(me)) == Some(wanted),
            }
        }
        (FilterKey::Repo, Item::PullRequest(pr)) => {
            contains_ci(catalog.repo_name(&pr.repo_id), value)
        }
        (FilterKey::Branch, Item::PullRequest(pr)) => {
            contains_ci(short_branch(&pr.source_branch), value)
        }
        (FilterKey::Branch, Item::Run(run)) => contains_ci(short_branch(&run.branch), value),
        (FilterKey::Target, Item::PullRequest(pr)) => {
            contains_ci(short_branch(&pr.target_branch), value)
        }
        (FilterKey::Type, Item::WorkItem(wi)) => parse_type(&value_lc).as_ref() == Some(&wi.kind),
        (FilterKey::State, Item::WorkItem(wi)) => wi.state.eq_ignore_ascii_case(value),
        (FilterKey::Assigned, Item::WorkItem(wi)) => people(wi.assigned_to.as_deref()),
        (FilterKey::Priority, Item::WorkItem(wi)) => {
            let wanted = match value_lc.as_str() {
                "urgent" => Some(1),
                "high" => Some(2),
                "normal" => Some(3),
                "low" => Some(4),
                n => n.parse::<u8>().ok(),
            };
            wanted.is_some() && wi.priority == wanted
        }
        (FilterKey::Area, Item::WorkItem(wi)) => contains_ci(&wi.area_path, value),
        (FilterKey::Iteration, Item::WorkItem(wi)) => contains_ci(&wi.iteration_path, value),
        (FilterKey::Tag, Item::WorkItem(wi)) => {
            wi.tags.iter().any(|t| t.eq_ignore_ascii_case(value))
        }
        (FilterKey::Status, Item::Run(run)) => match value_lc.as_str() {
            "failed" => run.result == Some(RunResult::Failed),
            "succeeded" => run.result == Some(RunResult::Succeeded),
            "partial" => run.result == Some(RunResult::PartiallySucceeded),
            "canceled" | "cancelled" => run.result == Some(RunResult::Canceled),
            "running" => run.status == RunStatus::InProgress,
            "queued" => run.status == RunStatus::NotStarted,
            _ => false,
        },
        (FilterKey::By, Item::Run(run)) => people(run.requested_for.as_deref()),
        (FilterKey::Project, item) => {
            let project_id = match item {
                Item::PullRequest(pr) => &pr.project_id,
                Item::WorkItem(wi) => &wi.project_id,
                Item::Run(run) => &run.project_id,
                Item::Repo(repo) => &repo.project_id,
                Item::Project(project) => return contains_ci(&project.name, value),
                _ => return false,
            };
            contains_ci(catalog.project_name(project_id), value)
        }
        (FilterKey::Updated, Item::PullRequest(pr)) => dates(pr.updated),
        (FilterKey::Updated, Item::WorkItem(wi)) => dates(wi.changed),
        (FilterKey::Created, Item::PullRequest(pr)) => dates(pr.created),
        (FilterKey::Created, Item::WorkItem(wi)) => dates(wi.created),
        _ => false,
    }
}

fn parse_vote(value: &str) -> Option<Vote> {
    Some(match value {
        "approved" => Vote::Approved,
        "suggestions" => Vote::ApprovedWithSuggestions,
        "waiting" => Vote::WaitingForAuthor,
        "rejected" => Vote::Rejected,
        "none" => Vote::NoVote,
        _ => return None,
    })
}

fn parse_type(value: &str) -> Option<WorkItemType> {
    match value {
        "story" | "userstory" | "user-story" => Some(WorkItemType::UserStory),
        "pbi" => Some(WorkItemType::ProductBacklogItem),
        other => WorkItemType::parse(other),
    }
}

impl Catalog {
    /// A row-template field's display value (§5.3); `""` when it doesn't
    /// apply. Formatting is fixed per field (relative times, sigil ids).
    pub fn field(&self, ix: u32, name: &str, now: i64) -> String {
        let age = |at: i64| relative_time(at, now);
        match self.item(ix) {
            Item::PullRequest(pr) => match name {
                "id" => format!("!{}", pr.id),
                "title" => pr.title.clone(),
                "repo" => self.repo_name(&pr.repo_id).to_owned(),
                "project" => self.project_name(&pr.project_id).to_owned(),
                "author" => self.person_name(&pr.author).to_owned(),
                "source_branch" => short_branch(&pr.source_branch).to_owned(),
                "target_branch" => short_branch(&pr.target_branch).to_owned(),
                "status" => pr_status_label(pr).to_owned(),
                "draft" => {
                    if pr.is_draft {
                        "Draft".into()
                    } else {
                        String::new()
                    }
                }
                "my_vote" => self
                    .me
                    .as_deref()
                    .and_then(|me| pr.vote_of(me))
                    .filter(|vote| *vote != Vote::NoVote)
                    .map(|vote| vote_short(vote).to_owned())
                    .unwrap_or_default(),
                "reviewers_summary" => reviewers_summary(pr),
                "reviewers" => pr
                    .reviewers
                    .iter()
                    .map(|r| self.person_name(&r.person))
                    .collect::<Vec<_>>()
                    .join(", "),
                "merge_status" => match pr.merge_status {
                    adoken_model::MergeStatus::Conflicts => "Conflicts".into(),
                    adoken_model::MergeStatus::RejectedByPolicy => "Blocked by policy".into(),
                    adoken_model::MergeStatus::Failure => "Merge failed".into(),
                    _ => String::new(),
                },
                "iterations" => format!("{} pushes", pr.iterations),
                "created" => age(pr.created),
                "updated" => age(pr.updated),
                _ => String::new(),
            },
            Item::WorkItem(wi) => match name {
                "id" => format!("#{}", wi.id),
                "title" => wi.title.clone(),
                "type" => wi.kind.as_str().to_owned(),
                "state" => wi.state.clone(),
                "assigned_to" => wi
                    .assigned_to
                    .as_deref()
                    .map(|id| self.person_name(id).to_owned())
                    .unwrap_or_default(),
                "priority" => wi.priority.map(|p| format!("P{p}")).unwrap_or_default(),
                "area" => last_segment(&wi.area_path).to_owned(),
                "iteration" => last_segment(&wi.iteration_path).to_owned(),
                "tags" => wi.tags.join(", "),
                "parent" => wi.parent_id.map(|id| format!("#{id}")).unwrap_or_default(),
                "project" => self.project_name(&wi.project_id).to_owned(),
                "created" => age(wi.created),
                "updated" => age(wi.changed),
                _ => String::new(),
            },
            Item::Run(run) => match name {
                "pipeline" => run.pipeline_name.clone(),
                "run" => run.run_number.clone(),
                "status" => run_status_label(run).to_owned(),
                "result" => run
                    .result
                    .map(|r| run_result_label(r).to_owned())
                    .unwrap_or_default(),
                "branch" => short_branch(&run.branch).to_owned(),
                "triggered_by" => run
                    .requested_for
                    .as_deref()
                    .map(|id| self.person_name(id).to_owned())
                    .unwrap_or_default(),
                "duration" => match (run.started, run.finish) {
                    (Some(start), Some(finish)) if finish >= start => duration(finish - start),
                    _ => String::new(),
                },
                "finished" => run.finish.map(age).unwrap_or_default(),
                "project" => self.project_name(&run.project_id).to_owned(),
                _ => String::new(),
            },
            Item::Person(p) => match name {
                "name" => p.display_name.clone(),
                "email" => p.unique_name.clone().unwrap_or_default(),
                "open_prs" => count(self.open_prs_by_author(&p.id), "open PR"),
                _ => String::new(),
            },
            Item::Repo(r) => match name {
                "name" => r.name.clone(),
                "project" => self.project_name(&r.project_id).to_owned(),
                "default_branch" => r
                    .default_branch
                    .as_deref()
                    .map(short_branch)
                    .unwrap_or_default()
                    .to_owned(),
                "open_prs" => count(self.open_prs_in_repo(&r.id), "open PR"),
                _ => String::new(),
            },
            Item::Project(p) => match name {
                "name" => p.name.clone(),
                "open_prs" => count(self.open_prs_in_project(&p.id), "open PR"),
                _ => String::new(),
            },
            Item::Command(ix) => match name {
                "title" => self.commands[*ix].title.to_owned(),
                _ => String::new(),
            },
        }
    }

    /// Full accessible label for a row, independent of the visual template
    /// (§8): kind, id, title, state, and people.
    pub fn accessible_label(&self, ix: u32, now: i64) -> String {
        match self.item(ix) {
            Item::PullRequest(pr) => format!(
                "Pull request {}, {}, {}, by {}, {}, updated {}",
                pr.id,
                pr.title,
                pr_status_label(pr),
                self.person_name(&pr.author),
                self.repo_name(&pr.repo_id),
                relative_time(pr.updated, now)
            ),
            Item::WorkItem(wi) => format!("{} {}, {}, {}", wi.kind, wi.id, wi.title, wi.state),
            Item::Run(run) => format!(
                "Pipeline run {} {}, {}",
                run.pipeline_name,
                run.run_number,
                run_status_label(run)
            ),
            Item::Person(p) => format!("Person {}", p.display_name),
            Item::Repo(r) => format!("Repository {}", r.name),
            Item::Project(p) => format!("Project {}", p.name),
            Item::Command(ix) => format!("Command {}", self.commands[*ix].title),
        }
    }
}

pub fn pr_status_label(pr: &PullRequest) -> &'static str {
    match (pr.status, pr.is_draft) {
        (PrStatus::Active, true) => "Draft",
        (PrStatus::Active, false) => "Active",
        (PrStatus::Completed, _) => "Completed",
        (PrStatus::Abandoned, _) => "Abandoned",
    }
}

pub fn run_status_label(run: &PipelineRun) -> &'static str {
    match (run.status, run.result) {
        (_, Some(result)) => run_result_label(result),
        (RunStatus::InProgress, None) => "Running",
        (RunStatus::NotStarted | RunStatus::Postponed, None) => "Queued",
        (RunStatus::Cancelling, None) => "Cancelling",
        (RunStatus::Completed, None) => "Completed",
    }
}

fn run_result_label(result: RunResult) -> &'static str {
    match result {
        RunResult::Succeeded => "Succeeded",
        RunResult::PartiallySucceeded => "Partially succeeded",
        RunResult::Failed => "Failed",
        RunResult::Canceled => "Canceled",
    }
}

fn vote_short(vote: Vote) -> &'static str {
    match vote {
        Vote::Approved => "Approved",
        Vote::ApprovedWithSuggestions => "Suggestions",
        Vote::NoVote => "",
        Vote::WaitingForAuthor => "Waiting",
        Vote::Rejected => "Rejected",
    }
}

/// `✓2 ⏳1 ✕1`: approvals (incl. with suggestions), waiting, rejected.
fn reviewers_summary(pr: &PullRequest) -> String {
    let mut approved = 0;
    let mut waiting = 0;
    let mut rejected = 0;
    for r in &pr.reviewers {
        match r.vote {
            Vote::Approved | Vote::ApprovedWithSuggestions => approved += 1,
            Vote::WaitingForAuthor => waiting += 1,
            Vote::Rejected => rejected += 1,
            Vote::NoVote => {}
        }
    }
    let mut parts = Vec::new();
    if approved > 0 {
        parts.push(format!("✓{approved}"));
    }
    if waiting > 0 {
        parts.push(format!("⏳{waiting}"));
    }
    if rejected > 0 {
        parts.push(format!("✕{rejected}"));
    }
    parts.join(" ")
}

fn last_segment(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

fn count(n: usize, noun: &str) -> String {
    match n {
        0 => String::new(),
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// Relative under 7 days (`now`, `2m`, `3h`, `Tue`), absolute after
/// (`Sep 12`), per principles.md §5.
pub fn relative_time(at: i64, now: i64) -> String {
    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let delta = now - at;
    let Ok(date) = time::OffsetDateTime::from_unix_timestamp(at.div_euclid(1000)) else {
        return String::new();
    };
    match delta {
        d if d < MINUTE => "now".into(),
        d if d < HOUR => format!("{}m", d / MINUTE),
        d if d < DAY => format!("{}h", d / HOUR),
        d if d < 7 * DAY => date.weekday().to_string()[..3].to_owned(),
        _ => format!("{} {}", &date.month().to_string()[..3], date.day()),
    }
}

#[cfg(test)]
mod tests {
    use adoken_model::{MergeStatus, Reviewer, StateCategory};

    use super::*;

    const NOW: i64 = 1_790_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn person(id: &str, name: &str) -> Person {
        Person {
            id: id.into(),
            display_name: name.into(),
            unique_name: Some(format!(
                "{}@contoso.com",
                name.to_lowercase().replace(' ', ".")
            )),
            is_group: false,
            image_url: None,
        }
    }

    fn pr(
        id: i64,
        title: &str,
        author: &str,
        status: PrStatus,
        reviewers: &[(&str, Vote)],
    ) -> PullRequest {
        PullRequest {
            id,
            repo_id: "r1".into(),
            project_id: "p1".into(),
            title: title.into(),
            description: "long text".into(),
            author: author.into(),
            reviewers: reviewers
                .iter()
                .map(|(who, vote)| Reviewer {
                    person: (*who).into(),
                    vote: *vote,
                    is_required: false,
                    has_declined: false,
                    added_at: None,
                    voted_at: None,
                })
                .collect(),
            status,
            is_draft: false,
            merge_status: MergeStatus::Succeeded,
            source_branch: "refs/heads/feature/retry".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: None,
            created: NOW - 48 * HOUR,
            updated: NOW - 2 * HOUR,
            closed: None,
            iterations: 2,
            last_push: None,
        }
    }

    fn wi(
        id: i64,
        title: &str,
        kind: WorkItemType,
        assigned: Option<&str>,
        state: &str,
    ) -> WorkItem {
        WorkItem {
            id,
            project_id: "p1".into(),
            rev: 1,
            kind,
            title: title.into(),
            state: state.into(),
            state_category: StateCategory::of_state(state),
            assigned_to: assigned.map(Into::into),
            created_by: "dana".into(),
            changed_by: "dana".into(),
            area_path: "Payments\\Checkout".into(),
            iteration_path: "Payments\\Sprint 42".into(),
            priority: Some(2),
            tags: vec!["login".into()],
            parent_id: None,
            linked_pr_ids: Vec::new(),
            description: Some("desc".into()),
            comment_count: 0,
            created: NOW - 72 * HOUR,
            changed: NOW - HOUR,
        }
    }

    fn catalog() -> Catalog {
        Catalog::new(
            CatalogData {
                org: Some("contoso".into()),
                me: Some("me".into()),
                people: vec![person("me", "Sam Rivera"), person("dana", "Dana Kim")],
                projects: vec![Project {
                    id: "p1".into(),
                    name: "Payments".into(),
                }],
                repos: vec![Repo {
                    id: "r1".into(),
                    project_id: "p1".into(),
                    name: "payments-api".into(),
                    default_branch: Some("refs/heads/main".into()),
                }],
                prs: vec![
                    pr(
                        4821,
                        "Retry idempotency keys",
                        "dana",
                        PrStatus::Active,
                        &[("me", Vote::NoVote)],
                    ),
                    pr(
                        4812,
                        "Retry budget for webhooks",
                        "me",
                        PrStatus::Active,
                        &[("dana", Vote::Approved)],
                    ),
                    pr(
                        4700,
                        "Retry old flow",
                        "dana",
                        PrStatus::Completed,
                        &[("me", Vote::Approved)],
                    ),
                ],
                work_items: vec![
                    wi(
                        4821,
                        "Review login flow",
                        WorkItemType::Bug,
                        Some("me"),
                        "Active",
                    ),
                    wi(
                        900,
                        "Login copy",
                        WorkItemType::Task,
                        Some("dana"),
                        "Closed",
                    ),
                ],
                runs: Vec::new(),
                org_url: None,
            },
            vec![CommandEntry {
                title: "Sync now",
                keys: Some("secondary-r"),
            }],
        )
    }

    fn ids(catalog: &Catalog, q: &str) -> Vec<String> {
        catalog
            .run(&Query::parse(q), 50, NOW)
            .into_iter()
            .map(|row| match row {
                Row::Pin(subject) => subject.key(),
                Row::Item { item, .. } => match catalog.item(item) {
                    Item::PullRequest(pr) => format!("!{}", pr.id),
                    Item::WorkItem(wi) => format!("#{}", wi.id),
                    Item::Command(_) => ">cmd".into(),
                    other => format!("{:?}", other.kind()),
                },
                Row::NotSynced(target) => format!("missing {target:?}"),
                Row::OtherOrg { org } => format!("other org {org}"),
            })
            .collect()
    }

    #[test]
    fn bare_id_ranks_both_kinds_first_and_never_typo_matches() {
        let c = catalog();
        let got = ids(&c, "4821");
        assert_eq!(&got[..2], ["!4821", "#4821"]);
        assert!(!got.contains(&"!4812".to_owned()), "{got:?}");
    }

    #[test]
    fn scoped_jumps_hit_or_say_not_synced() {
        let c = catalog();
        assert_eq!(ids(&c, "!4812")[0], "!4812");
        assert_eq!(ids(&c, "#4821")[0], "#4821");
        assert_eq!(ids(&c, "!999")[0], "missing PullRequest(999)");
        assert_eq!(ids(&c, "#999")[0], "missing WorkItem(999)");
    }

    #[test]
    fn links_open_in_app_or_flag_another_org() {
        let c = catalog();
        assert_eq!(
            ids(
                &c,
                "https://dev.azure.com/contoso/Payments/_git/payments-api/pullrequest/4821"
            ),
            ["!4821"]
        );
        assert_eq!(
            ids(
                &c,
                "https://dev.azure.com/fabrikam/Web/_git/site/pullrequest/912"
            ),
            ["other org fabrikam"]
        );
        assert_eq!(
            ids(&c, "https://contoso.visualstudio.com/P/_workitems/edit/77"),
            ["missing WorkItem(77)"]
        );
    }

    /// A link copied from a pull request (`review::web`) pasted into ⌘P
    /// offers that PR, from either org URL form, with escaped names.
    #[test]
    fn copied_pull_request_links_open_the_pull_request() {
        let c = catalog();
        for org_url in [
            "https://dev.azure.com/contoso/",
            "https://contoso.visualstudio.com/",
        ] {
            let url = crate::review::web::web_url(org_url, "Payments Team", "payments api", 4821)
                .expect("valid org URL");
            assert_eq!(ids(&c, &url), ["!4821"], "{url}");
        }
    }

    #[test]
    fn scope_and_filters_narrow() {
        let c = catalog();
        // AC-6: `!` + author:@me → only PRs I created.
        assert_eq!(ids(&c, "! retry author:@me"), ["!4812"]);
        assert_eq!(ids(&c, "!retry author:@dana is:open"), ["!4821"]);
        assert_eq!(ids(&c, "! -is:open"), ["!4700"]);
        assert_eq!(ids(&c, "! vote:approved"), ["!4700"]);
        assert_eq!(ids(&c, "! reviewer:dana vote:approved"), ["!4812"]);
        assert_eq!(ids(&c, "login type:bug"), ["#4821"]);
        assert_eq!(ids(&c, "# is:closed"), ["#900"]);
        assert_eq!(ids(&c, "# assigned:@me updated:<1d"), ["#4821"]);
        assert_eq!(ids(&c, "# priority:high tag:login state:active"), ["#4821"]);
        // Filters hide kinds they can't apply to: no PRs for type:.
        assert!(ids(&c, "retry type:bug").is_empty());
        assert_eq!(ids(&c, ">sync"), [">cmd"]);
    }

    #[test]
    fn empty_query_lists_my_work() {
        let c = catalog();
        // Review waiting on me, then my open PR, then my active work item.
        assert_eq!(ids(&c, ""), ["!4821", "!4812", "#4821"]);
    }

    #[test]
    fn fields_render_for_templates() {
        let c = catalog();
        let pr = c.by_target[&(DocKind::PullRequest, 4812)];
        assert_eq!(c.field(pr, "id", NOW), "!4812");
        assert_eq!(c.field(pr, "repo", NOW), "payments-api");
        assert_eq!(c.field(pr, "source_branch", NOW), "feature/retry");
        assert_eq!(c.field(pr, "reviewers_summary", NOW), "✓1");
        assert_eq!(c.field(pr, "updated", NOW), "2h");
        assert_eq!(c.field(pr, "my_vote", NOW), "");
        let old = c.by_target[&(DocKind::PullRequest, 4700)];
        assert_eq!(c.field(old, "my_vote", NOW), "Approved");
        let wi = c.by_target[&(DocKind::WorkItem, 4821)];
        assert_eq!(c.field(wi, "area", NOW), "Checkout");
        assert_eq!(c.field(wi, "assigned_to", NOW), "Sam Rivera");
        assert_eq!(c.field(wi, "priority", NOW), "P2");
        assert_eq!(c.field(wi, "nope", NOW), "");
    }

    #[test]
    fn people_rows_refine_to_their_prs() {
        let c = catalog();
        let dana = (0..c.len() as u32)
            .find(|&ix| matches!(c.item(ix), Item::Person(p) if p.id == "dana"))
            .unwrap();
        let refine = c.refine_query(dana).unwrap();
        assert_eq!(refine, "! author:@dana.kim is:open");
        assert_eq!(ids(&c, &refine), ["!4821"]);
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(NOW - 30_000, NOW), "now");
        assert_eq!(relative_time(NOW - 5 * 60_000, NOW), "5m");
        assert_eq!(relative_time(NOW - 3 * HOUR, NOW), "3h");
        // 1_790_000_000 s is Mon 2026-09-21 14:13 UTC.
        assert_eq!(relative_time(NOW - 48 * HOUR, NOW), "Sat");
        assert_eq!(relative_time(NOW - 30 * 24 * HOUR, NOW), "Aug 22");
        assert_eq!(duration(252_000), "4m 12s");
    }
}
