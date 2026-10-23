//! Pure view-model for PR review (spec 0004): list tabs, the loaded PR with
//! per-file diffs and anchored threads, local "viewed" state, and display
//! helpers. No gpui. Blob reads go through closures (store in the app, the
//! demo dataset in stories); every function here blocks, so call it on a
//! background executor.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use adoken_diff::anchor::{self, Anchored, LineMap, Position};
use adoken_diff::lex::Lang;
use adoken_diff::rows::{self, SplitRow, UnifiedRow};
use adoken_diff::{FileDiff, Side, Text};
use adoken_model::{
    ChangeType, MergeStatus, Millis, Person, PersonId, PipelineRun, PrDetail, PrFileChange,
    PrStatus, PrThread, PullRequest, Reviewer, Vote, time,
};
use adoken_store::Store;
use adoken_store::rusqlite::{self, params};
use adoken_ui::file_tree::{self, ChangeKind, TreeRow};

use super::writes::{DRAFTS_MIGRATION, Draft};
use crate::pipelines::model::Outcome;

// ── list ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListTab {
    /// Active PRs I authored.
    Mine,
    /// Active, non-draft PRs where I'm a reviewer and haven't approved.
    ToReview,
    /// Everything synced, newest activity first.
    All,
}

impl ListTab {
    pub const ALL: [Self; 3] = [Self::Mine, Self::ToReview, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mine => "Mine",
            Self::ToReview => "To review",
            Self::All => "All",
        }
    }

    pub fn matches(self, pr: &PullRequest, me: &str) -> bool {
        match self {
            Self::Mine => pr.status == PrStatus::Active && pr.author == me,
            Self::ToReview => {
                pr.status == PrStatus::Active
                    && !pr.is_draft
                    && pr.author != me
                    && pr
                        .reviewers
                        .iter()
                        .any(|r| r.person == me && !r.has_declined && awaits(r.vote))
            }
            Self::All => true,
        }
    }
}

/// My review is still owed: no vote, or I asked the author for changes and
/// they may have pushed since.
fn awaits(vote: Vote) -> bool {
    matches!(vote, Vote::NoVote | Vote::WaitingForAuthor)
}

/// Indices into `prs` for `tab` in repository `repo` (`None` = all
/// repositories), keeping the store's newest-first order.
pub fn filter(prs: &[PullRequest], tab: ListTab, me: &str, repo: Option<&str>) -> Vec<usize> {
    prs.iter()
        .enumerate()
        .filter(|(_, pr)| in_repo(pr, repo) && tab.matches(pr, me))
        .map(|(ix, _)| ix)
        .collect()
}

fn in_repo(pr: &PullRequest, repo: Option<&str>) -> bool {
    repo.is_none_or(|repo| pr.repo_id == repo)
}

/// The Mine / To review / All segment labels; counts follow the repo
/// filter ("All" carries none: it's everything synced).
pub fn tab_labels(prs: &[PullRequest], me: &str, repo: Option<&str>) -> Vec<String> {
    ListTab::ALL
        .iter()
        .map(|tab| match tab {
            ListTab::All => tab.label().to_owned(),
            _ => format!("{} {}", tab.label(), filter(prs, *tab, me, repo).len()),
        })
        .collect()
}

/// A keyboard move in the list (`↑`/`↓`, `j`/`k`, PageUp/PageDown,
/// Home/End).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    By(isize),
    First,
    Last,
}

/// The list selection after `step` over `len` rows: clamped at both ends
/// (no wrap), `None` only for an empty list. No selection moves from the
/// first row.
pub fn move_selection(selected: Option<usize>, len: usize, step: Move) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match step {
        Move::First => 0,
        Move::Last => last,
        Move::By(delta) => selected
            .unwrap_or(0)
            .min(last)
            .saturating_add_signed(delta)
            .min(last),
    })
}

/// Rows a PageUp/PageDown moves: the rows that fit in `viewport` (px),
/// at least one (also before the first paint, when the viewport is 0).
pub fn page_rows(viewport: f32, row_height: f32) -> usize {
    if row_height <= 0.0 {
        return 1;
    }
    ((viewport / row_height).floor() as usize).max(1)
}

/// Everything a list row shows beyond the PR itself, read from the store
/// with the list (spec 0004 §3.1).
#[derive(Debug, Clone, Default)]
pub struct RowFacts {
    /// Active human threads per PR.
    pub comments: HashMap<i64, u32>,
    /// CI status per PR: the worst latest run of its pipelines.
    pub checks: HashMap<i64, Outcome>,
    /// Auto-complete I set in adoken.
    pub auto_complete: HashSet<i64>,
    /// My Complete that Azure DevOps is still merging.
    pub completing: HashSet<i64>,
}

impl RowFacts {
    pub fn read(store: &Store) -> adoken_store::Result<Self> {
        Ok(Self {
            comments: store.active_thread_counts()?,
            checks: checks(&store.latest_pr_runs()?),
            auto_complete: store.auto_complete_ids()?,
            completing: store.completing_ids()?,
        })
    }
}

/// Worst first: a PR's CI status is the most urgent of its pipelines'
/// latest runs.
fn severity(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::Failed => 0,
        Outcome::Partial => 1,
        Outcome::Running => 2,
        Outcome::Queued => 3,
        Outcome::Canceled => 4,
        Outcome::Succeeded => 5,
    }
}

/// CI status per PR from each pipeline's latest run for it
/// ([`adoken_store::Store::latest_pr_runs`]).
pub fn checks(runs: &[PipelineRun]) -> HashMap<i64, Outcome> {
    let mut out: HashMap<i64, Outcome> = HashMap::new();
    for run in runs {
        let Some(pr) = run.pr_id else { continue };
        let outcome = Outcome::of(run);
        out.entry(pr)
            .and_modify(|worst| {
                if severity(outcome) < severity(*worst) {
                    *worst = outcome;
                }
            })
            .or_insert(outcome);
    }
    out
}

/// A row's status badges, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    Draft,
    Active,
    Completed,
    Abandoned,
    /// My Complete is merging (FR-10).
    Completing,
    MergeConflict,
    AutoComplete,
}

impl RowStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Active => "Active",
            Self::Completed => "Completed",
            Self::Abandoned => "Abandoned",
            Self::Completing => "Completing…",
            Self::MergeConflict => "Merge conflict",
            Self::AutoComplete => "Auto-complete",
        }
    }
}

/// The lifecycle badge (Completing replaces Active while my Complete
/// merges), then Merge conflict and Auto-complete on active PRs.
pub fn row_status(pr: &PullRequest, facts: &RowFacts) -> Vec<RowStatus> {
    let active = pr.status == PrStatus::Active;
    let lifecycle = match pr.status {
        PrStatus::Active if facts.completing.contains(&pr.id) => RowStatus::Completing,
        PrStatus::Active if pr.is_draft => RowStatus::Draft,
        PrStatus::Active => RowStatus::Active,
        PrStatus::Completed => RowStatus::Completed,
        PrStatus::Abandoned => RowStatus::Abandoned,
    };
    let mut out = vec![lifecycle];
    if active && pr.merge_status == MergeStatus::Conflicts {
        out.push(RowStatus::MergeConflict);
    }
    if active && facts.auto_complete.contains(&pr.id) {
        out.push(RowStatus::AutoComplete);
    }
    out
}

/// Reviewers of a row grouped by vote: approved, approved with
/// suggestions, waiting for author, rejected, then required reviewers who
/// haven't voted (drawn dimmed). Optional reviewers without a vote and
/// reviewers who declined aren't shown; order within a group is the PR's.
pub fn reviewer_groups(pr: &PullRequest) -> Vec<(Vote, Vec<&Reviewer>)> {
    const ORDER: [Vote; 5] = [
        Vote::Approved,
        Vote::ApprovedWithSuggestions,
        Vote::WaitingForAuthor,
        Vote::Rejected,
        Vote::NoVote,
    ];
    ORDER
        .iter()
        .filter_map(|vote| {
            let members: Vec<&Reviewer> = pr
                .reviewers
                .iter()
                .filter(|r| r.vote == *vote && !r.has_declined)
                .filter(|r| *vote != Vote::NoVote || r.is_required)
                .collect();
            (!members.is_empty()).then_some((*vote, members))
        })
        .collect()
}

/// One repository in the list's repository filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoOption {
    pub id: String,
    pub name: String,
    /// Pull requests in the current tab.
    pub count: usize,
}

/// Repositories with pull requests in `tab`, most first, then by name.
/// `active` stays listed (count 0) when the tab has none of its PRs, so
/// the current choice is always visible. `names`: repo id → name; an
/// unknown repo shows its id.
pub fn repo_options(
    prs: &[PullRequest],
    tab: ListTab,
    me: &str,
    names: &HashMap<String, String>,
    active: Option<&str>,
) -> Vec<RepoOption> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for pr in prs.iter().filter(|pr| tab.matches(pr, me)) {
        *counts.entry(pr.repo_id.as_str()).or_default() += 1;
    }
    if let Some(active) = active {
        counts.entry(active).or_default();
    }
    let mut options: Vec<RepoOption> = counts
        .into_iter()
        .map(|(id, count)| RepoOption {
            id: id.to_owned(),
            name: names.get(id).cloned().unwrap_or_else(|| id.to_owned()),
            count,
        })
        .collect();
    options.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    options
}

/// Label of the picker row that clears the repository filter.
pub const ALL_REPOS: &str = "All repositories";

/// The repository picker's rows for `query`: `None` is "All repositories",
/// `Some(ix)` indexes `options`. Case-insensitive substring match on the
/// name; "All repositories" leads while the query is empty (or matches it).
pub fn picker_rows(options: &[RepoOption], query: &str) -> Vec<Option<usize>> {
    let query = query.trim().to_lowercase();
    let all = query.is_empty() || ALL_REPOS.to_lowercase().contains(&query);
    all.then_some(None)
        .into_iter()
        .chain(
            options
                .iter()
                .enumerate()
                .filter(|(_, option)| option.name.to_lowercase().contains(&query))
                .map(|(ix, _)| Some(ix)),
        )
        .collect()
}

/// A saved repository filter still applies only while the repository is
/// synced; one that's gone (deleted, renamed away, project dropped) falls
/// back to all repositories.
pub fn resolve_repo(saved: Option<String>, repos: &HashMap<String, String>) -> Option<String> {
    saved.filter(|id| repos.contains_key(id))
}

// ── display helpers ──────────────────────────────────────────────────────

/// principles §5: "now", "5m", "3h", "2d" under 7 days, then "Sep 12".
pub fn age(now: Millis, then: Millis) -> String {
    let delta = (now - then).max(0);
    if delta < time::MINUTE {
        "now".to_owned()
    } else if delta < time::HOUR {
        format!("{}m", delta / time::MINUTE)
    } else if delta < time::DAY {
        format!("{}h", delta / time::HOUR)
    } else if delta < 7 * time::DAY {
        format!("{}d", delta / time::DAY)
    } else {
        ::time::OffsetDateTime::from_unix_timestamp(then / 1000).map_or_else(
            |_| format!("{}d", delta / time::DAY),
            |date| {
                let month = date.month().to_string();
                format!("{} {}", &month[..3], date.day())
            },
        )
    }
}

/// Replace `@<identity-guid>` with `@Display Name` (FR-11). Unknown ids stay
/// as written.
pub fn resolve_mentions(body: &str, people: &HashMap<PersonId, Person>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("@<") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('>') {
            Some(end) => {
                let id = &after[..end];
                match people.get(id) {
                    Some(person) => {
                        out.push('@');
                        out.push_str(&person.display_name);
                    }
                    None => out.push_str(&rest[start..start + 2 + end + 1]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn change_kind(change: ChangeType) -> ChangeKind {
    match change {
        ChangeType::Add => ChangeKind::Add,
        ChangeType::Edit => ChangeKind::Edit,
        ChangeType::Delete => ChangeKind::Delete,
        ChangeType::Rename => ChangeKind::Rename,
    }
}

/// ADO paths start with `/`; thread anchors and file changes may disagree.
pub fn normalize_path(path: &str) -> &str {
    path.trim_start_matches('/')
}

/// Spec 0004 §3: generated files render collapsed until "Load anyway".
pub fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with(".lock")
        || name == "package-lock.json"
        || name == "pnpm-lock.yaml"
        || name.contains(".min.")
        || name.ends_with(".snap")
        || path.contains("/generated/")
}

/// Spec 0004 §3: binary files render collapsed and cost no blob request
/// until expanded (AC-12). Decided from the path; content with NULs found
/// after a fetch collapses too ([`DiffJob::run`]).
pub fn is_binary_path(path: &str) -> bool {
    const BINARY: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tif", "tiff", "psd", "pdf",
        "zip", "gz", "tgz", "bz2", "xz", "7z", "rar", "jar", "war", "nupkg", "dll", "exe", "so",
        "dylib", "a", "lib", "o", "obj", "pdb", "class", "wasm", "bin", "dat", "db", "sqlite",
        "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "mov", "wav", "ogg", "avi", "mkv",
        "doc", "docx", "xls", "xlsx", "ppt", "pptx", "snk", "pfx", "p12",
    ];
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| BINARY.iter().any(|b| b.eq_ignore_ascii_case(ext)))
}

/// Git's own test: a NUL in the first 8000 bytes means binary.
pub fn looks_binary(text: &str) -> bool {
    text.as_bytes().iter().take(8_000).any(|b| *b == 0)
}

/// Spec 0004 §3: files above these also render collapsed.
pub const MAX_AUTO_LINES: usize = 20_000;
pub const MAX_AUTO_BYTES: usize = 1 << 20;

/// FR-3: files prefetched on open in tree order (plus the selection's
/// neighbors).
pub const PREFETCH_FIRST: usize = 5;

// ── loaded PR ────────────────────────────────────────────────────────────

/// Why a file's diff isn't shown yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collapsed {
    Generated,
    Binary,
    Large,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub change: PrFileChange,
    /// Normalized display path.
    pub path: String,
    pub lang: Lang,
    /// `None` until computed (background) or while collapsed.
    pub diff: Option<Arc<FileDiff>>,
    pub collapsed: Option<Collapsed>,
    /// Indices into [`LoadedPr::threads`], placed against `diff`.
    pub anchored: Anchored<usize>,
    /// Threads on this path (inline + older), for the tree's count.
    pub thread_count: usize,
    /// Its blobs couldn't be fetched (offline, throttled, gone): shown
    /// instead of the diff, with a retry.
    pub unavailable: Option<String>,
}

impl FileEntry {
    /// FR-7 key part: the target blob, or the base one for deletions.
    pub fn object_id(&self) -> &str {
        self.change
            .new_content
            .as_deref()
            .or(self.change.old_content.as_deref())
            .unwrap_or("")
    }
}

/// Everything the review renders, built off the UI thread.
#[derive(Debug, Clone)]
pub struct LoadedPr {
    pub detail: PrDetail,
    pub people: HashMap<PersonId, Person>,
    pub me: PersonId,
    pub now: Millis,
    /// Displayed iteration (the latest).
    pub iteration: u32,
    pub files: Vec<FileEntry>,
    pub tree: Vec<TreeRow>,
    /// File indices in tree order: the `j`/`k` order.
    pub order: Vec<usize>,
    /// Human threads (a non-system, non-deleted comment), PR-level ones
    /// included (spec 0019 FR-8).
    pub threads: Arc<Vec<PrThread>>,
    /// `(path, objectId)` pairs marked viewed.
    pub viewed: HashSet<(String, String)>,
    /// No write access: every review control is replaced by one hint.
    pub read_only: bool,
    /// Auto-complete I set in adoken: the source commit I had reviewed
    /// (spec 0004 FR-12).
    pub auto_complete: Option<String>,
    /// My Complete that Azure DevOps accepted and is still merging: when it
    /// was accepted (spec 0004 FR-10). Set from the store after [`build`].
    pub completing: Option<Millis>,
    /// Unsent composer text, newest first (FR-8).
    pub drafts: Vec<Draft>,
}

impl LoadedPr {
    pub fn pr(&self) -> &PullRequest {
        &self.detail.pr
    }

    pub fn name(&self, id: &str) -> String {
        self.people
            .get(id)
            .map_or_else(|| id.to_owned(), |p| p.display_name.clone())
    }

    pub fn is_viewed(&self, file: usize) -> bool {
        let entry = &self.files[file];
        self.viewed
            .contains(&(entry.path.clone(), entry.object_id().to_owned()))
    }

    /// "Completing…": my accepted Complete is still merging (the PR is
    /// active and the queued merge hasn't failed).
    pub fn is_completing(&self) -> bool {
        let pr = self.pr();
        self.completing.is_some() && pr.status == PrStatus::Active && !pr.merge_status.is_failure()
    }

    /// The displayed file at repo path `path` (either form: `/src/a.rs`
    /// or `src/a.rs`).
    pub fn file_at(&self, path: &str) -> Option<&FileEntry> {
        let path = normalize_path(path);
        self.files.iter().find(|f| f.path == path)
    }
}

/// Inputs for [`build`]: the detail plus a blob reader. Kept as a closure so
/// the app reads through the store and gallery stories read the demo dataset.
pub struct Source<'a> {
    pub detail: PrDetail,
    pub people: Vec<Person>,
    pub me: PersonId,
    pub now: Millis,
    pub viewed: HashSet<(String, String)>,
    pub read_only: bool,
    pub auto_complete: Option<String>,
    pub drafts: Vec<Draft>,
    pub blob: &'a dyn Fn(&str) -> Option<String>,
}

/// Builds the loaded PR; computes diffs for `eager` files in tree order
/// (the rest stay `None` for [`DiffJob`] later). Blocking: run on a
/// background executor.
pub fn build(source: Source<'_>, eager: usize) -> LoadedPr {
    let Source {
        detail,
        people,
        me,
        now,
        viewed,
        read_only,
        auto_complete,
        drafts,
        blob,
    } = source;
    let iteration = latest_iteration(&detail);
    let threads: Vec<PrThread> = detail
        .threads
        .iter()
        .filter(|t| is_human(t))
        .cloned()
        .collect();
    let (files, tree, order) = file_entries(&detail.files, &threads);
    let mut loaded = LoadedPr {
        detail,
        people: people.into_iter().map(|p| (p.id.clone(), p)).collect(),
        me,
        now,
        iteration,
        files,
        tree,
        order,
        threads: Arc::new(threads),
        viewed,
        read_only,
        auto_complete,
        completing: None,
        drafts,
    };
    let eager_files: Vec<usize> = loaded.order.iter().copied().take(eager).collect();
    for file in eager_files {
        if loaded.files[file].collapsed.is_none() {
            let outcome = loaded.job(file).run(blob, false);
            apply_diff(&mut loaded, file, outcome);
        }
    }
    loaded
}

fn latest_iteration(detail: &PrDetail) -> u32 {
    detail
        .iterations
        .iter()
        .map(|it| it.id)
        .max()
        .unwrap_or(detail.pr.iterations)
}

/// Tree entries for `changes`: generated and binary files start collapsed
/// (AC-12: no blob request until expanded).
fn file_entries(
    changes: &[PrFileChange],
    threads: &[PrThread],
) -> (Vec<FileEntry>, Vec<TreeRow>, Vec<usize>) {
    let files: Vec<FileEntry> = changes
        .iter()
        .map(|change| {
            let path = normalize_path(&change.path).to_owned();
            let thread_count = threads
                .iter()
                .filter(|t| thread_path(t) == Some(path.as_str()))
                .count();
            let collapsed = if is_generated(&path) {
                Some(Collapsed::Generated)
            } else if is_binary_path(&path) {
                Some(Collapsed::Binary)
            } else {
                None
            };
            FileEntry {
                lang: Lang::from_path(&path),
                collapsed,
                path,
                change: change.clone(),
                diff: None,
                anchored: Anchored::default(),
                thread_count,
                unavailable: None,
            }
        })
        .collect();
    let tree = file_tree::build(&files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>());
    let order = file_tree::file_order(&tree);
    (files, tree, order)
}

/// Replace the file list (a newer push, or the first fetch of a PR opened
/// before its list was synced). Threads, drafts and viewed state stay;
/// every diff is recomputed.
pub fn set_files(loaded: &mut LoadedPr, detail: PrDetail) {
    loaded.detail.files = detail.files;
    loaded.detail.iterations = detail.iterations;
    loaded.iteration = latest_iteration(&loaded.detail);
    let (files, tree, order) = file_entries(&loaded.detail.files, &loaded.threads);
    loaded.files = files;
    loaded.tree = tree;
    loaded.order = order;
}

/// Merge a fresh read of the PR's threads (sync landed, spec 0019 FR-9):
/// a known thread is replaced in place (its index, and so its placement,
/// stays); a new human thread is appended. `None` when nothing changed,
/// else the files that gained a thread (their placement must be
/// recomputed).
pub fn merge_threads(loaded: &mut LoadedPr, fresh: Vec<PrThread>) -> Option<Vec<usize>> {
    let mut added: Vec<usize> = Vec::new();
    let mut changed = false;
    for thread in fresh.into_iter().filter(is_human) {
        match loaded.threads.iter().position(|t| t.id == thread.id) {
            Some(ix) if loaded.threads[ix] == thread => {}
            Some(ix) => {
                Arc::make_mut(&mut loaded.threads)[ix] = thread;
                changed = true;
            }
            None => {
                let path = thread_path(&thread).map(str::to_owned);
                Arc::make_mut(&mut loaded.threads).push(thread);
                changed = true;
                if let Some(file) = path.and_then(|p| loaded.files.iter().position(|f| f.path == p))
                {
                    loaded.files[file].thread_count += 1;
                    if !added.contains(&file) {
                        added.push(file);
                    }
                }
            }
        }
    }
    changed.then_some(added)
}

/// What to load when `selected` is shown: the selection itself, then FR-3's
/// prefetch set (the first [`PREFETCH_FIRST`] files in tree order and the
/// selection's neighbors). Collapsed files (AC-12: no blob request until
/// expanded) and files already computed are skipped.
pub fn load_plan(loaded: &LoadedPr, selected: usize) -> Vec<usize> {
    let order = &loaded.order;
    let pos = order.iter().position(|f| *f == selected);
    let neighbors = pos
        .into_iter()
        .flat_map(|p| [p.checked_sub(1), Some(p + 1)])
        .flatten()
        .filter_map(|p| order.get(p).copied());
    let mut out: Vec<usize> = Vec::new();
    let candidates = std::iter::once(selected)
        .filter(|f| *f < loaded.files.len())
        .chain(neighbors)
        .chain(order.iter().copied().take(PREFETCH_FIRST));
    for file in candidates {
        let entry = &loaded.files[file];
        if entry.collapsed.is_none() && entry.diff.is_none() && !out.contains(&file) {
            out.push(file);
        }
    }
    out
}

/// A thread a person wrote in (not only system events).
pub fn is_human(thread: &PrThread) -> bool {
    thread
        .comments
        .iter()
        .any(|c| !c.is_system && !c.is_deleted)
}

fn thread_path(thread: &PrThread) -> Option<&str> {
    thread
        .anchor
        .as_ref()
        .map(|anchor| normalize_path(&anchor.file_path))
}

/// What computing one file's diff needs; `Send`, so it can move to a
/// background executor while the view keeps the [`LoadedPr`].
pub struct DiffJob {
    pub file: usize,
    path: String,
    change: PrFileChange,
    threads: Arc<Vec<PrThread>>,
    iteration: u32,
}

impl LoadedPr {
    pub fn job(&self, file: usize) -> DiffJob {
        let entry = &self.files[file];
        DiffJob {
            file,
            path: entry.path.clone(),
            change: entry.change.clone(),
            threads: self.threads.clone(),
            iteration: self.iteration,
        }
    }
}

/// What a [`DiffJob`] produced.
#[derive(Debug, Clone)]
pub enum DiffOutcome {
    Ready(Arc<FileDiff>, Anchored<usize>),
    /// Binary or too large for automatic display.
    Collapsed(Collapsed),
    /// A side's blob isn't available locally: fetch it and run again.
    Missing,
}

impl DiffJob {
    /// The blobs this file's diff reads (base, then target).
    pub fn blob_ids(&self) -> Vec<String> {
        [&self.change.old_content, &self.change.new_content]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    /// Diff + thread placement. Blocking; `force` ignores the binary and
    /// large-file collapse ("Load anyway").
    pub fn run(&self, blob: &dyn Fn(&str) -> Option<String>, force: bool) -> DiffOutcome {
        let read = |id: &Option<String>| match id.as_deref() {
            Some(id) => blob(id),
            None => Some(String::new()),
        };
        let (Some(old), Some(new)) = (
            read(&self.change.old_content),
            read(&self.change.new_content),
        ) else {
            return DiffOutcome::Missing;
        };
        if !force {
            if looks_binary(&old) || looks_binary(&new) {
                return DiffOutcome::Collapsed(Collapsed::Binary);
            }
            let too_large =
                |text: &str| text.len() > MAX_AUTO_BYTES || text.lines().count() > MAX_AUTO_LINES;
            if too_large(&old) || too_large(&new) {
                return DiffOutcome::Collapsed(Collapsed::Large);
            }
        }
        let diff = FileDiff::compute(Text::new(old), Text::new(new));
        let anchored = place_threads(&self.path, &self.threads, self.iteration, &diff, blob);
        DiffOutcome::Ready(Arc::new(diff), anchored)
    }
}

/// Applies a [`DiffJob::run`] result. `Missing` leaves the file loading.
pub fn apply_diff(loaded: &mut LoadedPr, file: usize, outcome: DiffOutcome) {
    let entry = &mut loaded.files[file];
    match outcome {
        DiffOutcome::Ready(diff, anchored) => {
            entry.collapsed = None;
            entry.unavailable = None;
            entry.diff = Some(diff);
            entry.anchored = anchored;
        }
        DiffOutcome::Collapsed(reason) => {
            entry.collapsed = Some(reason);
            entry.diff = None;
            entry.anchored = Anchored::default();
        }
        DiffOutcome::Missing => {}
    }
}

/// FR-5: anchor this file's threads, mapping older-iteration right-side
/// anchors through the file as it was at that iteration.
fn place_threads(
    path: &str,
    threads: &[PrThread],
    iteration: u32,
    diff: &FileDiff,
    blob: &dyn Fn(&str) -> Option<String>,
) -> Anchored<usize> {
    let mut maps: HashMap<&str, LineMap> = HashMap::new();
    let mut items = Vec::new();
    for (ix, thread) in threads.iter().enumerate() {
        let Some(anchor) = thread.anchor.as_ref() else {
            continue;
        };
        if normalize_path(&anchor.file_path) != path {
            continue;
        }
        let (side, range) = match (anchor.right, anchor.left) {
            (Some(range), _) => (Side::Right, range),
            (None, Some(range)) => (Side::Left, range),
            // File-level thread: pin to the first line.
            (None, None) => (Side::Right, adoken_model::LineRange { start: 1, end: 1 }),
        };
        let stale = thread.iteration.is_some_and(|it| it != iteration);
        if stale
            && side == Side::Right
            && let Some(id) = anchor.iteration_blob.as_deref()
            && !maps.contains_key(id)
            && let Some(then) = blob(id)
        {
            maps.insert(id, LineMap::new(&then, diff.new.source()));
        }
        items.push((ix, side, range, stale, anchor.iteration_blob.as_deref()));
    }
    anchor::anchor_all(
        diff,
        items.into_iter().map(|(ix, side, range, stale, blob_id)| {
            let map = if stale {
                blob_id.and_then(|id| maps.get(id))
            } else {
                None
            };
            (
                ix,
                Position {
                    side,
                    start: range.start,
                    end: range.end,
                },
                map,
                stale,
            )
        }),
    )
}

// ── diff pane rows ───────────────────────────────────────────────────────

/// One item of the virtualized diff pane: diff rows interleaved with the
/// thread cards anchored after them, the open line composer, then the
/// older-code section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneRow {
    Unified(UnifiedRow),
    Split(SplitRow),
    /// Index into [`LoadedPr::threads`].
    Thread(usize),
    /// "Comments on older code (n)".
    OlderHeader(usize),
    Older(usize),
    Collapsed(Collapsed),
    Loading,
    /// The file's blobs couldn't be fetched ([`FileEntry::unavailable`]).
    Unavailable,
    NoChanges,
    /// The line comment composer (after the line's threads).
    Composer,
}

/// Rows for one file. Folds that hide an anchored line (or the composer's
/// line) open automatically: a thread is never hidden behind an expander.
/// `composer` is the display line a new comment is being written on.
pub fn pane_rows(
    entry: &FileEntry,
    layout: Layout,
    expanded: &HashSet<u32>,
    composer: Option<u32>,
) -> Vec<PaneRow> {
    let Some(diff) = entry.diff.as_deref() else {
        return vec![match entry.collapsed {
            Some(reason) => PaneRow::Collapsed(reason),
            None if entry.unavailable.is_some() => PaneRow::Unavailable,
            None => PaneRow::Loading,
        }];
    };
    let mut out = Vec::new();
    if diff.is_unchanged() {
        out.push(PaneRow::NoChanges);
    }
    let mut open = expanded.clone();
    for fold in rows::folds(diff) {
        let hidden = fold.start as usize..(fold.start + fold.len) as usize;
        if entry
            .anchored
            .inline
            .iter()
            .map(|(line, _)| *line)
            .chain(composer.map(|l| l as usize))
            .any(|line| hidden.contains(&line))
        {
            open.insert(fold.start);
        }
    }
    let threads_at = |out: &mut Vec<PaneRow>, line: u32| {
        out.extend(
            entry
                .anchored
                .at(line as usize)
                .map(|t| PaneRow::Thread(*t)),
        );
        if composer == Some(line) {
            out.push(PaneRow::Composer);
        }
    };
    match layout {
        Layout::Unified => {
            for row in rows::unified(diff, &open) {
                out.push(PaneRow::Unified(row));
                if let UnifiedRow::Line(line) = row {
                    threads_at(&mut out, line);
                }
            }
        }
        Layout::Split => {
            for row in rows::split(diff, &open) {
                out.push(PaneRow::Split(row));
                if let SplitRow::Pair { left, right } = row {
                    if let Some(line) = left {
                        threads_at(&mut out, line);
                    }
                    if let Some(line) = right.filter(|r| Some(*r) != left) {
                        threads_at(&mut out, line);
                    }
                }
            }
        }
    }
    if !entry.anchored.older.is_empty() {
        out.push(PaneRow::OlderHeader(entry.anchored.older.len()));
        out.extend(entry.anchored.older.iter().map(|t| PaneRow::Older(*t)));
    }
    out
}

// ── local state (FR-7 viewed, AC-16 layout preference) ───────────────────

const MIGRATIONS: &[&str] = &[
    "
CREATE TABLE review_viewed (
  pr_id INTEGER NOT NULL, path TEXT NOT NULL, object_id TEXT NOT NULL,
  PRIMARY KEY (pr_id, path, object_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE review_pref (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT, WITHOUT ROWID;
",
    DRAFTS_MIGRATION,
];

pub fn migrate(store: &Store) -> adoken_store::Result<()> {
    store.migrate_ext("review", MIGRATIONS)
}

pub fn viewed(store: &Store, pr_id: i64) -> adoken_store::Result<HashSet<(String, String)>> {
    store.with_conn(|c| {
        let mut stmt =
            c.prepare_cached("SELECT path, object_id FROM review_viewed WHERE pr_id = ?1")?;
        let rows = stmt.query_map([pr_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
}

pub fn set_viewed(
    store: &Store,
    pr_id: i64,
    path: &str,
    object_id: &str,
    viewed: bool,
) -> adoken_store::Result<()> {
    store.write(|tx| {
        if viewed {
            tx.execute(
                "INSERT OR IGNORE INTO review_viewed(pr_id, path, object_id) VALUES (?1, ?2, ?3)",
                params![pr_id, path, object_id],
            )?;
        } else {
            tx.execute(
                "DELETE FROM review_viewed WHERE pr_id = ?1 AND path = ?2 AND object_id = ?3",
                params![pr_id, path, object_id],
            )?;
        }
        Ok(())
    })
}

const LAYOUT_KEY: &str = "diff_layout";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Unified,
    Split,
}

impl Layout {
    /// Q-3: side-by-side at ≥ 1400 px unless the user chose.
    pub const SPLIT_MIN_WIDTH: f32 = 1400.0;

    pub fn resolve(preference: Option<Layout>, window_width: f32) -> Layout {
        preference.unwrap_or(if window_width >= Self::SPLIT_MIN_WIDTH {
            Layout::Split
        } else {
            Layout::Unified
        })
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Unified => Self::Split,
            Self::Split => Self::Unified,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Unified => "unified",
            Self::Split => "split",
        }
    }
}

fn pref(store: &Store, key: &str) -> adoken_store::Result<Option<String>> {
    store.with_conn(|c| {
        c.query_row("SELECT value FROM review_pref WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })
        .map_err(Into::into)
    })
}

/// `None` forgets the key.
fn set_pref(store: &Store, key: &str, value: Option<&str>) -> adoken_store::Result<()> {
    store.write(|tx| {
        match value {
            Some(value) => tx.execute(
                "INSERT INTO review_pref(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?,
            None => tx.execute("DELETE FROM review_pref WHERE key = ?1", [key])?,
        };
        Ok(())
    })
}

pub fn layout_preference(store: &Store) -> adoken_store::Result<Option<Layout>> {
    Ok(match pref(store, LAYOUT_KEY)?.as_deref() {
        Some("unified") => Some(Layout::Unified),
        Some("split") => Some(Layout::Split),
        _ => None,
    })
}

pub fn set_layout_preference(store: &Store, layout: Layout) -> adoken_store::Result<()> {
    set_pref(store, LAYOUT_KEY, Some(layout.as_str()))
}

/// The PR list's repository filter (a repo id). The store is per org, so
/// this is per org too.
const REPO_FILTER_KEY: &str = "list_repo";

pub fn repo_filter_preference(store: &Store) -> adoken_store::Result<Option<String>> {
    pref(store, REPO_FILTER_KEY)
}

/// `None` = all repositories.
pub fn set_repo_filter_preference(store: &Store, repo: Option<&str>) -> adoken_store::Result<()> {
    set_pref(store, REPO_FILTER_KEY, repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::{
        Comment, LineRange, MergeStatus, PrIteration, Reviewer, ThreadAnchor, ThreadStatus,
    };

    const ME: &str = "me";

    fn pr(id: i64, author: &str, reviewers: &[(&str, Vote)]) -> PullRequest {
        PullRequest {
            id,
            repo_id: "r".into(),
            project_id: "p".into(),
            title: format!("PR {id}"),
            description: String::new(),
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
            status: PrStatus::Active,
            is_draft: false,
            merge_status: MergeStatus::Succeeded,
            source_branch: "refs/heads/feature".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: None,
            created: 0,
            updated: 0,
            closed: None,
            iterations: 3,
            last_push: None,
        }
    }

    #[test]
    fn tabs_split_mine_to_review_and_all() {
        let mut draft = pr(4, "ana", &[(ME, Vote::NoVote)]);
        draft.is_draft = true;
        let mut done = pr(5, ME, &[]);
        done.status = PrStatus::Completed;
        let prs = vec![
            pr(1, ME, &[("ana", Vote::Approved)]),
            pr(2, "ana", &[(ME, Vote::NoVote)]),
            pr(3, "ana", &[(ME, Vote::Approved)]),
            draft,
            done,
            pr(6, "bo", &[(ME, Vote::WaitingForAuthor)]),
        ];
        assert_eq!(filter(&prs, ListTab::Mine, ME, None), [0]);
        assert_eq!(filter(&prs, ListTab::ToReview, ME, None), [1, 5]);
        assert_eq!(filter(&prs, ListTab::All, ME, None).len(), 6);
    }

    fn at(mut pr: PullRequest, repo: &str) -> PullRequest {
        pr.repo_id = repo.into();
        pr
    }

    /// api: mine + to review + approved; web: to review ×2; docs: mine.
    fn across_repos() -> Vec<PullRequest> {
        vec![
            at(pr(1, ME, &[]), "api"),
            at(pr(2, "ana", &[(ME, Vote::NoVote)]), "web"),
            at(pr(3, "ana", &[(ME, Vote::NoVote)]), "api"),
            at(pr(4, "bo", &[(ME, Vote::WaitingForAuthor)]), "web"),
            at(pr(5, "bo", &[(ME, Vote::Approved)]), "api"),
            at(pr(6, ME, &[]), "docs"),
        ]
    }

    #[test]
    fn repo_filter_narrows_every_tab_and_its_count() {
        let prs = across_repos();
        assert_eq!(filter(&prs, ListTab::Mine, ME, Some("api")), [0]);
        assert_eq!(filter(&prs, ListTab::ToReview, ME, Some("api")), [2]);
        assert_eq!(filter(&prs, ListTab::All, ME, Some("api")), [0, 2, 4]);
        assert_eq!(filter(&prs, ListTab::ToReview, ME, Some("web")), [1, 3]);
        assert!(filter(&prs, ListTab::Mine, ME, Some("web")).is_empty());
        assert_eq!(
            tab_labels(&prs, ME, Some("web")),
            ["Mine 0", "To review 2", "All"]
        );
        assert_eq!(tab_labels(&prs, ME, None), ["Mine 2", "To review 3", "All"]);
    }

    #[test]
    fn repo_options_count_the_tab_most_first_then_by_name() {
        let prs = across_repos();
        let names = HashMap::from([
            ("api".to_owned(), "Api".to_owned()),
            ("web".to_owned(), "web".to_owned()),
        ]);
        let summary = |options: Vec<RepoOption>| -> Vec<(String, usize)> {
            options.into_iter().map(|o| (o.name, o.count)).collect()
        };
        // All: api 3, web 2, docs 1 (no name: its id).
        assert_eq!(
            summary(repo_options(&prs, ListTab::All, ME, &names, None)),
            [("Api".into(), 3), ("web".into(), 2), ("docs".into(), 1)]
        );
        // Mine: a tie at 1 sorts by name, case-insensitively.
        assert_eq!(
            summary(repo_options(&prs, ListTab::Mine, ME, &names, None)),
            [("Api".into(), 1), ("docs".into(), 1)]
        );
        // The active repo stays listed when this tab has none of its PRs.
        assert_eq!(
            summary(repo_options(&prs, ListTab::Mine, ME, &names, Some("web"))),
            [("Api".into(), 1), ("docs".into(), 1), ("web".into(), 0)]
        );
    }

    #[test]
    fn picker_rows_lead_with_all_and_match_names_case_insensitively() {
        let option = |name: &str| RepoOption {
            id: name.into(),
            name: name.into(),
            count: 1,
        };
        let options = [
            option("platform-api"),
            option("Web-App"),
            option("api-docs"),
        ];
        assert_eq!(picker_rows(&options, ""), [None, Some(0), Some(1), Some(2)]);
        assert_eq!(picker_rows(&options, " API "), [Some(0), Some(2)]);
        assert_eq!(picker_rows(&options, "web"), [Some(1)]);
        assert_eq!(picker_rows(&options, "all"), [None]);
        assert!(picker_rows(&options, "zzz").is_empty());
    }

    #[test]
    fn saved_repo_falls_back_to_all_when_it_no_longer_exists() {
        let repos = HashMap::from([("api".to_owned(), "api".to_owned())]);
        assert_eq!(resolve_repo(Some("api".into()), &repos), Some("api".into()));
        assert_eq!(resolve_repo(Some("gone".into()), &repos), None);
        assert_eq!(resolve_repo(None, &repos), None);
    }

    #[test]
    fn ages_are_compact() {
        let now = 100 * time::DAY;
        assert_eq!(age(now, now + 5), "now");
        assert_eq!(age(now, now - 5 * time::MINUTE), "5m");
        assert_eq!(age(now, now - 3 * time::HOUR), "3h");
        assert_eq!(age(now, now - 2 * time::DAY), "2d");
        assert_eq!(age(now, now - 21 * time::DAY), "Mar 21");
    }

    #[test]
    fn list_moves_clamp_at_both_ends_and_jump() {
        assert_eq!(move_selection(None, 0, Move::By(1)), None);
        assert_eq!(move_selection(Some(0), 0, Move::Last), None);
        assert_eq!(move_selection(None, 5, Move::By(1)), Some(1));
        assert_eq!(move_selection(Some(0), 5, Move::By(-1)), Some(0), "no wrap");
        assert_eq!(move_selection(Some(4), 5, Move::By(1)), Some(4), "no wrap");
        assert_eq!(move_selection(Some(1), 5, Move::By(10)), Some(4), "page");
        assert_eq!(move_selection(Some(3), 5, Move::By(-10)), Some(0), "page");
        assert_eq!(move_selection(Some(2), 5, Move::First), Some(0));
        assert_eq!(move_selection(Some(2), 5, Move::Last), Some(4));
        // A stale selection past a shrunken list moves from its last row.
        assert_eq!(move_selection(Some(9), 5, Move::By(-1)), Some(3));
    }

    #[test]
    fn page_is_the_rows_that_fit_and_never_zero() {
        assert_eq!(page_rows(800.0, 80.0), 10);
        assert_eq!(page_rows(799.0, 80.0), 9);
        assert_eq!(page_rows(0.0, 80.0), 1, "before the first paint");
        assert_eq!(page_rows(40.0, 80.0), 1);
    }

    fn run(pr_id: Option<i64>, result: Option<adoken_model::RunResult>) -> PipelineRun {
        use adoken_model::{RunReason, RunStatus};
        PipelineRun {
            id: 1,
            project_id: "p".into(),
            pipeline_id: 1,
            pipeline_name: "CI".into(),
            run_number: "1".into(),
            status: if result.is_some() {
                RunStatus::Completed
            } else {
                RunStatus::InProgress
            },
            result,
            reason: RunReason::PullRequest,
            branch: "refs/pull/1/merge".into(),
            source_version: None,
            requested_for: None,
            pr_id,
            queued: 0,
            started: None,
            finish: None,
        }
    }

    #[test]
    fn ci_status_is_the_worst_pipeline() {
        use adoken_model::RunResult;
        let runs = [
            run(Some(1), Some(RunResult::Succeeded)),
            run(Some(1), None),
            run(Some(2), Some(RunResult::Succeeded)),
            run(Some(2), Some(RunResult::Failed)),
            run(Some(2), None),
            run(Some(3), Some(RunResult::Succeeded)),
            run(None, Some(RunResult::Failed)),
        ];
        let checks = checks(&runs);
        assert_eq!(
            checks.get(&1),
            Some(&Outcome::Running),
            "running beats green"
        );
        assert_eq!(checks.get(&2), Some(&Outcome::Failed), "a failure wins");
        assert_eq!(checks.get(&3), Some(&Outcome::Succeeded));
        assert_eq!(checks.len(), 3, "runs without a PR are ignored");
    }

    #[test]
    fn row_status_follows_lifecycle_then_merge_and_auto_complete() {
        let mut facts = RowFacts::default();
        let mut active = pr(1, "a", &[]);
        assert_eq!(row_status(&active, &facts), [RowStatus::Active]);
        active.is_draft = true;
        assert_eq!(row_status(&active, &facts), [RowStatus::Draft]);
        active.is_draft = false;
        active.merge_status = MergeStatus::Conflicts;
        facts.auto_complete.insert(1);
        assert_eq!(
            row_status(&active, &facts),
            [
                RowStatus::Active,
                RowStatus::MergeConflict,
                RowStatus::AutoComplete
            ]
        );
        facts.completing.insert(1);
        assert_eq!(row_status(&active, &facts)[0], RowStatus::Completing);
        // A closed PR shows only how it ended.
        let mut done = active.clone();
        done.status = PrStatus::Completed;
        assert_eq!(row_status(&done, &facts), [RowStatus::Completed]);
    }

    #[test]
    fn reviewers_group_by_vote_with_only_required_no_votes() {
        let mut pr = pr(
            1,
            "a",
            &[
                ("rejects", Vote::Rejected),
                ("optional", Vote::NoVote),
                ("required", Vote::NoVote),
                ("approves", Vote::Approved),
                ("waits", Vote::WaitingForAuthor),
                ("suggests", Vote::ApprovedWithSuggestions),
                ("declined", Vote::Approved),
                ("approves2", Vote::Approved),
            ],
        );
        pr.reviewers[2].is_required = true;
        pr.reviewers[6].has_declined = true;
        let groups: Vec<(Vote, Vec<&str>)> = reviewer_groups(&pr)
            .into_iter()
            .map(|(vote, rs)| (vote, rs.iter().map(|r| r.person.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            [
                (Vote::Approved, vec!["approves", "approves2"]),
                (Vote::ApprovedWithSuggestions, vec!["suggests"]),
                (Vote::WaitingForAuthor, vec!["waits"]),
                (Vote::Rejected, vec!["rejects"]),
                (Vote::NoVote, vec!["required"]),
            ]
        );
    }

    #[test]
    fn mentions_resolve_known_ids_only() {
        let people = HashMap::from([(
            "g1".to_owned(),
            Person {
                id: "g1".into(),
                display_name: "Ana Silva".into(),
                unique_name: None,
                is_group: false,
                image_url: None,
            },
        )]);
        assert_eq!(
            resolve_mentions("cc @<g1> and @<g2>, see @<", &people),
            "cc @Ana Silva and @<g2>, see @<"
        );
    }

    #[test]
    fn generated_globs() {
        assert!(is_generated("/Cargo.lock"));
        assert!(is_generated("web/dist/app.min.js"));
        assert!(is_generated("src/generated/client.ts"));
        assert!(!is_generated("src/lock.rs"));
    }

    fn change(path: &str, old: Option<&str>, new: Option<&str>) -> PrFileChange {
        PrFileChange {
            path: path.into(),
            original_path: None,
            change_type: ChangeType::Edit,
            old_content: old.map(Into::into),
            new_content: new.map(Into::into),
            change_tracking_id: None,
        }
    }

    fn loaded_with(files: Vec<PrFileChange>) -> LoadedPr {
        let blob = |_: &str| None;
        build(
            Source {
                detail: PrDetail {
                    pr: pr(1, "ana", &[]),
                    iterations: Vec::new(),
                    files,
                    threads: Vec::new(),
                },
                people: Vec::new(),
                me: ME.into(),
                now: 0,
                viewed: HashSet::new(),
                read_only: true,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            0,
        )
    }

    /// AC-12: binaries and generated files start collapsed and are never
    /// loaded ahead; FR-3: the selection, its neighbors and the first five
    /// in tree order are.
    #[test]
    fn load_plan_is_selection_neighbors_and_first_five_never_collapsed_files() {
        let mut files = vec![
            change("/assets/logo.png", Some("p1"), Some("p2")),
            change("/Cargo.lock", Some("l1"), Some("l2")),
        ];
        files.extend((0..9).map(|i| change(&format!("/src/f{i}.rs"), None, Some("x"))));
        let loaded = loaded_with(files);
        let path = |f: usize| loaded.files[f].path.clone();
        let collapsed: Vec<(String, Option<Collapsed>)> = loaded.files[..2]
            .iter()
            .map(|f| (f.path.clone(), f.collapsed))
            .collect();
        assert_eq!(
            collapsed,
            [
                ("assets/logo.png".to_owned(), Some(Collapsed::Binary)),
                ("Cargo.lock".to_owned(), Some(Collapsed::Generated)),
            ]
        );
        let f6 = loaded
            .files
            .iter()
            .position(|f| f.path == "src/f6.rs")
            .unwrap();
        let plan: Vec<String> = load_plan(&loaded, f6).into_iter().map(path).collect();
        assert_eq!(plan[..3], ["src/f6.rs", "src/f5.rs", "src/f7.rs"]);
        assert!(
            plan.iter()
                .all(|p| p != "Cargo.lock" && p != "assets/logo.png")
        );
        let firsts: Vec<String> = loaded
            .order
            .iter()
            .take(PREFETCH_FIRST)
            .filter(|f| loaded.files[**f].collapsed.is_none())
            .map(|f| path(*f))
            .collect();
        assert!(!firsts.is_empty());
        assert!(firsts.iter().all(|p| plan.contains(p)), "{plan:?}");
        assert_eq!(plan.len(), 3 + firsts.len(), "no duplicates, nothing else");
        // A collapsed selection loads nothing itself.
        let lock = loaded
            .files
            .iter()
            .position(|f| f.path == "Cargo.lock")
            .unwrap();
        assert!(!load_plan(&loaded, lock).contains(&lock));
    }

    #[test]
    fn missing_blobs_wait_and_binary_content_collapses() {
        let loaded = loaded_with(vec![change("/src/a.rs", Some("old"), Some("new"))]);
        let job = loaded.job(0);
        assert_eq!(job.blob_ids(), ["old", "new"]);
        let nothing = |_: &str| None;
        assert!(matches!(job.run(&nothing, false), DiffOutcome::Missing));
        let binary = |id: &str| Some(if id == "old" { "a\0b" } else { "text" }.to_owned());
        assert!(matches!(
            job.run(&binary, false),
            DiffOutcome::Collapsed(Collapsed::Binary)
        ));
        assert!(matches!(job.run(&binary, true), DiffOutcome::Ready(..)));
        assert!(is_binary_path("Docs/Spec.PDF"));
        assert!(!is_binary_path("src/lib.rs"));
    }

    #[test]
    fn layout_follows_width_until_user_chooses() {
        assert_eq!(Layout::resolve(None, 1440.0), Layout::Split);
        assert_eq!(Layout::resolve(None, 1200.0), Layout::Unified);
        assert_eq!(
            Layout::resolve(Some(Layout::Unified), 1440.0),
            Layout::Unified
        );
    }

    fn thread(
        id: i64,
        path: &str,
        right: (u32, u32),
        iteration: u32,
        blob: Option<&str>,
    ) -> PrThread {
        PrThread {
            id,
            pr_id: 1,
            status: ThreadStatus::Active,
            anchor: Some(ThreadAnchor {
                file_path: path.into(),
                right: Some(LineRange {
                    start: right.0,
                    end: right.1,
                }),
                left: None,
                iteration_blob: blob.map(Into::into),
            }),
            iteration: Some(iteration),
            comments: vec![Comment {
                id: 1,
                parent_id: None,
                author: "ana".into(),
                body: "why?".into(),
                created: 0,
                updated: 0,
                is_system: false,
                is_deleted: false,
            }],
            updated: 0,
        }
    }

    #[test]
    fn build_places_threads_across_iterations_and_counts_per_file() {
        let base: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        let iter1: String = (1..=30)
            .map(|i| format!("l{i}\n"))
            .chain(["x\n".into()])
            .collect();
        // Iteration 2 inserts 5 lines at the top and drops l20..l22.
        let iter2: String = (1..=5)
            .map(|i| format!("new{i}\n"))
            .chain(
                (1..=30)
                    .filter(|i| !(20..=22).contains(i))
                    .map(|i| format!("l{i}\n")),
            )
            .collect();
        let blobs = HashMap::from([
            ("base".to_owned(), base),
            ("it1".to_owned(), iter1),
            ("it2".to_owned(), iter2),
        ]);
        let detail = PrDetail {
            pr: pr(1, "ana", &[(ME, Vote::NoVote)]),
            iterations: vec![
                PrIteration {
                    id: 1,
                    source_commit: "a".into(),
                    created: 0,
                    description: None,
                    commit_count: None,
                    commits_truncated: false,
                },
                PrIteration {
                    id: 2,
                    source_commit: "b".into(),
                    created: 0,
                    description: None,
                    commit_count: None,
                    commits_truncated: false,
                },
            ],
            files: vec![PrFileChange {
                path: "/src/a.rs".into(),
                original_path: None,
                change_type: ChangeType::Edit,
                old_content: Some("base".into()),
                new_content: Some("it2".into()),
                change_tracking_id: Some(1),
            }],
            threads: vec![
                thread(10, "/src/a.rs", (10, 11), 1, Some("it1")),
                thread(11, "/src/a.rs", (21, 21), 1, Some("it1")),
                thread(12, "/src/a.rs", (3, 3), 2, None),
                thread(13, "/src/a.rs", (3, 3), 1, None),
            ],
        };
        let blob = |id: &str| blobs.get(id).cloned();
        let loaded = build(
            Source {
                detail,
                people: Vec::new(),
                me: ME.into(),
                now: 0,
                viewed: HashSet::new(),
                read_only: true,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            1,
        );
        let file = &loaded.files[0];
        assert_eq!(file.thread_count, 4);
        let diff = file.diff.as_ref().expect("eager diff");
        let inline: Vec<(u32, i64)> = file
            .anchored
            .inline
            .iter()
            .map(|(line, t)| (diff.lines[*line].new.unwrap(), loaded.threads[*t].id))
            .collect();
        // Thread 10 (it1 lines 10–11) moved down 5; thread 12 is current.
        assert_eq!(inline, [(3, 12), (16, 10)]);
        let older: Vec<i64> = file
            .anchored
            .older
            .iter()
            .map(|t| loaded.threads[*t].id)
            .collect();
        // 11: its line was deleted; 13: stale with no blob to map through.
        assert_eq!(older, [11, 13]);
        // The fold that would hide thread 10's line opens; older code trails.
        let rows = pane_rows(file, Layout::Unified, &HashSet::new(), None);
        let t10 = loaded.threads.iter().position(|t| t.id == 10).unwrap();
        let at = rows
            .iter()
            .position(|r| *r == PaneRow::Thread(t10))
            .unwrap();
        assert!(matches!(rows[at - 1], PaneRow::Unified(UnifiedRow::Line(l))
            if diff.lines[l as usize].new == Some(16)));
        assert_eq!(rows[rows.len() - 3], PaneRow::OlderHeader(2));
        let split = pane_rows(file, Layout::Split, &HashSet::new(), None);
        assert_eq!(
            split
                .iter()
                .filter(|r| matches!(r, PaneRow::Thread(_)))
                .count(),
            2
        );
        // The composer on thread 10's line comes after that line's thread.
        let line = rows[at - 1];
        let PaneRow::Unified(UnifiedRow::Line(line)) = line else {
            unreachable!()
        };
        let with_composer = pane_rows(file, Layout::Unified, &HashSet::new(), Some(line));
        let composer = with_composer
            .iter()
            .position(|r| *r == PaneRow::Composer)
            .unwrap();
        assert_eq!(with_composer[composer - 1], PaneRow::Thread(t10));
        // A composer inside a fold opens it.
        let folded_line = (0..diff.lines.len() as u32)
            .find(|l| !rows.contains(&PaneRow::Unified(UnifiedRow::Line(*l))))
            .expect("a folded line");
        assert!(
            pane_rows(file, Layout::Unified, &HashSet::new(), Some(folded_line))
                .contains(&PaneRow::Unified(UnifiedRow::Line(folded_line)))
        );
    }

    #[test]
    fn viewed_layout_and_repo_filter_persist_in_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        migrate(&store).unwrap();
        migrate(&store).unwrap();
        set_viewed(&store, 7, "src/a.rs", "obj1", true).unwrap();
        set_viewed(&store, 7, "src/b.rs", "obj2", true).unwrap();
        set_viewed(&store, 7, "src/b.rs", "obj2", false).unwrap();
        let viewed = viewed(&store, 7).unwrap();
        assert_eq!(
            viewed,
            HashSet::from([("src/a.rs".to_owned(), "obj1".to_owned())])
        );
        // A new push changes the objectId: the file reads as un-viewed.
        assert!(!viewed.contains(&("src/a.rs".to_owned(), "obj9".to_owned())));

        assert_eq!(layout_preference(&store).unwrap(), None);
        set_layout_preference(&store, Layout::Unified).unwrap();
        assert_eq!(layout_preference(&store).unwrap(), Some(Layout::Unified));

        assert_eq!(repo_filter_preference(&store).unwrap(), None);
        set_repo_filter_preference(&store, Some("api")).unwrap();
        set_repo_filter_preference(&store, Some("web")).unwrap();
        assert_eq!(repo_filter_preference(&store).unwrap(), Some("web".into()));
        // Back to all repositories: nothing saved, and the layout is kept.
        set_repo_filter_preference(&store, None).unwrap();
        assert_eq!(repo_filter_preference(&store).unwrap(), None);
        assert_eq!(layout_preference(&store).unwrap(), Some(Layout::Unified));
    }
}
