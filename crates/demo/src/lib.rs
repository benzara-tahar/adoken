//! Deterministic fake "Contoso" org for demo mode, gallery stories and sync
//! tests. **Fake data only**: nothing here talks to Azure DevOps.
//!
//! - [`dataset`] / [`dataset_at`]: the whole org as plain model values
//!   (3 projects, ~300 PRs, ~5k work items, pipelines and runs, mentions,
//!   a few PRs with real multi-file Rust/TS diffs and anchored threads).
//! - [`events`]: an endless, deterministic stream of [`SyncEvent`]s after
//!   `Dataset::now`, for driving the inbox in demo mode.
//! - [`open_store`] / [`seed`]: a store seeded with the dataset. The store is
//!   marked with meta `demo = "1"` so views can disable write actions.
//!
//! The same seed always yields the same dataset for the same `now`.

mod code;
pub mod rich;
mod rng;
mod stages;
mod vocab;

use std::collections::BTreeMap;
use std::path::Path;

use adoken_model::time::{DAY, HOUR, MINUTE};
use adoken_model::*;
use adoken_store::{Store, read, write};

use rng::Rng;
pub use rng::{guid, object_id};
pub use stages::demo_stages;

/// Fixed anchor for snapshot-stable data: 2026-09-25T09:30:00Z (a Friday).
pub const DEMO_NOW: Millis = 1_790_328_600_000;
/// Seed used by [`open_store`].
pub const DEFAULT_SEED: u64 = 0xAD0_4E4;

/// PR ids of the hand-written rich PRs (multi-file diffs + anchored threads).
pub const RICH_PR_IDS: [i64; 3] = [4312, 4318, 4325];
/// PR ids shaped to trigger the smart nudges (spec 0006) for [`me`]:
/// 4290 waiting-for-author vote, 4291 failed build, 4292 no votes for 4 days,
/// 4293 merge conflicts, 4294 my review pending 2 days, 4295 completed PR
/// linked to my still-active work item.
pub const SCENARIO_PRS: [i64; 6] = [4290, 4291, 4292, 4293, 4294, 4295];

/// Extra PR content the review view needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrExtras {
    pub iterations: Vec<PrIteration>,
    pub files: Vec<PrFileChange>,
    pub threads: Vec<PrThread>,
}

/// The whole fake org.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dataset {
    pub seed: u64,
    pub now: Millis,
    pub org: Org,
    /// My identity id ([`me`]).
    pub me: PersonId,
    pub people: Vec<Person>,
    pub projects: Vec<Project>,
    pub repos: Vec<Repo>,
    pub prs: Vec<PullRequest>,
    pub pr_extras: BTreeMap<i64, PrExtras>,
    /// Content-addressed file contents referenced by `PrFileChange` and
    /// `ThreadAnchor::iteration_blob`.
    pub blobs: Vec<(BlobId, String)>,
    pub work_items: Vec<WorkItem>,
    pub wi_comments: Vec<WorkItemComment>,
    pub pipelines: Vec<Pipeline>,
    pub runs: Vec<PipelineRun>,
    /// Stages per run id ([`demo_stages`]), as the build timeline would give.
    pub stages: BTreeMap<i64, Vec<RunStage>>,
    /// Mentions of me, newest first.
    pub mentions: Vec<Mention>,
    /// Events from the 48 h before `now` (the inbox backlog).
    pub past_events: Vec<SyncEvent>,
}

impl Dataset {
    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }
    pub fn pr(&self, id: i64) -> Option<&PullRequest> {
        self.prs.iter().find(|p| p.id == id)
    }
}

/// My identity in the demo org.
pub fn me() -> Person {
    let id = guid(&format!("person:{}", vocab::ME.0));
    Person {
        image_url: Some(image_url(&id)),
        id,
        display_name: vocab::ME.0.to_owned(),
        unique_name: Some(vocab::ME.1.to_owned()),
        is_group: false,
    }
}

/// The `imageUrl` a PR payload carries for `id` in the demo org. Demo mode
/// never fetches it: the app draws a local generated picture for people that
/// have one (spec 0012), initials for the rest.
fn image_url(id: &str) -> String {
    format!("https://dev.azure.com/contoso/_api/_common/identityImage?id={id}")
}

/// Dataset anchored at [`DEMO_NOW`] (stable for snapshots).
pub fn dataset(seed: u64) -> Dataset {
    dataset_at(seed, DEMO_NOW)
}

/// Dataset whose relative times ("3 h ago") are anchored at `now`.
pub fn dataset_at(seed: u64, now: Millis) -> Dataset {
    Gen::new(seed, now).run()
}

pub fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(DEMO_NOW)
}

/// Open (seeding once) the demo store at `<dir>/adoken.sqlite`, anchored at
/// wall-clock now. `dir` must be a demo-only directory, never the real data
/// dir.
pub fn open_store(dir: &Path) -> adoken_store::Result<Store> {
    open_store_at(dir, now_ms())
}

/// [`open_store`] with an explicit anchor (used on first seed only).
pub fn open_store_at(dir: &Path, now: Millis) -> adoken_store::Result<Store> {
    let store = Store::open(dir.join("adoken.sqlite"))?;
    if store.meta("demo_seeded")?.is_none() {
        seed(&store, &dataset_at(DEFAULT_SEED, now))?;
    }
    Ok(store)
}

/// Write the whole dataset into `store`: file contents into its blob cache,
/// everything else in one transaction.
pub fn seed(store: &Store, ds: &Dataset) -> adoken_store::Result<()> {
    for (id, blob) in &ds.blobs {
        store.blobs().put(id, blob.as_bytes())?;
    }
    store.write(|tx| {
        write::put_org(tx, &ds.org)?;
        for p in &ds.people {
            write::upsert_person(tx, p, p.id == ds.me)?;
        }
        for p in &ds.projects {
            write::upsert_project(tx, p)?;
        }
        for r in &ds.repos {
            write::upsert_repo(tx, r)?;
        }
        for p in &ds.pipelines {
            write::upsert_pipeline(tx, p)?;
        }
        for pr in &ds.prs {
            write::upsert_pr(tx, pr)?;
        }
        for (pr_id, extra) in &ds.pr_extras {
            write::put_pr_iterations(tx, *pr_id, &extra.iterations)?;
            write::put_pr_files(tx, *pr_id, &extra.files)?;
            for t in &extra.threads {
                write::upsert_thread(tx, t)?;
            }
            write::mark_pr_hydrated(tx, *pr_id, ds.now)?;
        }
        for wi in &ds.work_items {
            write::upsert_work_item(tx, wi)?;
        }
        for c in &ds.wi_comments {
            write::upsert_wi_comment(tx, c)?;
        }
        rich::seed(tx, ds)?;
        for r in &ds.runs {
            write::upsert_run(tx, r)?;
            if let Some(stages) = ds.stages.get(&r.id) {
                write::put_run_stages(tx, r, stages)?;
            }
        }
        for e in &ds.past_events {
            write::insert_event(tx, e)?;
        }
        for p in &ds.projects {
            for scope in ["pr.active", "wi.revisions", "build.done"] {
                write::put_cursor(
                    tx,
                    &read::Cursor {
                        scope: scope.to_owned(),
                        project_id: p.id.clone(),
                        baseline_done: true,
                        last_ok_ms: Some(ds.now),
                        ..Default::default()
                    },
                )?;
            }
        }
        // Cache reseeding must not recreate pins the user removed or overwrite
        // local order, labels and opened timestamps.
        if read::meta(tx, "demo_pins_seeded")?.is_none() {
            for pin in pins(ds) {
                write::add_pin(tx, &pin)?;
            }
            write::set_meta(tx, "demo_pins_seeded", "1")?;
        }
        write::set_meta(tx, "demo", "1")?;
        write::set_meta(tx, "demo_now", &ds.now.to_string())?;
        write::set_meta(tx, "demo_seeded", &ds.seed.to_string())?;
        Ok(())
    })
}

/// The four initial local pins, anchored to the dataset rather than wall time.
pub fn pins(ds: &Dataset) -> Vec<Pin> {
    let approved = ds
        .prs
        .iter()
        .find(|p| p.title == "Bump Azure identity client")
        .expect("approved demo PR");
    let wi = ds
        .work_items
        .iter()
        .find(|w| w.state_category == StateCategory::InProgress)
        .expect("active demo work item");
    let pipeline = &ds.pipelines[0];
    [
        (PinSubject::Pr(4293), ds.pr(4293).unwrap().title.clone()),
        (PinSubject::Pr(approved.id), approved.title.clone()),
        (PinSubject::WorkItem(wi.id), wi.title.clone()),
        (
            PinSubject::Definition {
                project_id: pipeline.project_id.clone(),
                id: pipeline.id,
            },
            pipeline.name.clone(),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(position, (subject, title))| Pin {
        seen_result: matches!(subject, PinSubject::Definition { .. })
            .then_some(RunResult::Succeeded),
        opened_at: Some(if position == 0 { ds.now - HOUR } else { ds.now }),
        subject,
        title,
        position: position as u32,
        label: None,
        pinned_at: ds.now - DAY,
        unavailable: None,
    })
    .collect()
}

/// Endless deterministic event stream after `ds.now`: one event every
/// 1-6 minutes about PRs, work items and runs I'm involved in. `at` is
/// strictly increasing; dedupe keys are unique.
pub fn events(ds: &Dataset) -> impl Iterator<Item = SyncEvent> + '_ {
    let mut rng = Rng::fork(ds.seed, "events");
    let mut at = ds.now;
    let mut n = 0u64;
    std::iter::from_fn(move || {
        at += rng.range(MINUTE, 6 * MINUTE);
        n += 1;
        Some(make_event(ds, &mut rng, at, &format!("live:{n}")))
    })
}

/// Demo "sync" ticker: appends [`events`] whose `at` has passed into the
/// store's `sync_event` table, so the inbox fills up live in demo mode.
/// Idempotent (deterministic dedupe keys). Drive [`LiveFeed::tick`] from a
/// background timer (e.g. every 30 s); never from the UI thread.
pub struct LiveFeed {
    ds: Dataset,
    emitted: usize,
}

impl LiveFeed {
    /// Rebuild the dataset the demo store was seeded with.
    pub fn for_store(store: &Store) -> adoken_store::Result<Option<LiveFeed>> {
        let Some(now) = store.meta("demo_now")?.and_then(|v| v.parse().ok()) else {
            return Ok(None);
        };
        let seed = store
            .meta("demo_seeded")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_SEED);
        Ok(Some(LiveFeed {
            ds: dataset_at(seed, now),
            emitted: 0,
        }))
    }

    /// Append every event with `at <= now` not yet written; returns how many
    /// were new.
    pub fn tick(&mut self, store: &Store, now: Millis) -> adoken_store::Result<usize> {
        let due: Vec<SyncEvent> = events(&self.ds)
            .skip(self.emitted)
            .take_while(|e| e.at <= now)
            .collect();
        if due.is_empty() {
            return Ok(0);
        }
        self.emitted += due.len();
        store.write(|tx| {
            let mut n = 0;
            for e in &due {
                n += write::insert_event(tx, e)?.is_some() as usize;
            }
            Ok(n)
        })
    }
}

fn make_event(ds: &Dataset, rng: &mut Rng, at: Millis, key: &str) -> SyncEvent {
    let mine: Vec<&PullRequest> = ds
        .prs
        .iter()
        .filter(|p| p.status == PrStatus::Active && (p.author == ds.me || p.is_reviewer(&ds.me)))
        .collect();
    let others: Vec<&Person> = ds
        .people
        .iter()
        .filter(|p| p.id != ds.me && !p.is_group)
        .collect();
    let actor = rng.pick(&others);
    let self_caused = rng.chance(0.08);
    let actor_id = if self_caused {
        ds.me.clone()
    } else {
        actor.id.clone()
    };
    let who = if self_caused {
        "You"
    } else {
        actor.display_name.as_str()
    };
    let pr = rng.pick(&mine);
    let roll = rng.below(100);
    // Dedupe keys follow sync's per-kind shapes where consumers parse them
    // (votes `person:vote:…`, status `what:…`); the `live:n` tail keeps
    // them unique.
    let mut dedupe_key = key.to_owned();
    let (kind, subject, summary) = match roll {
        0..=17 => (
            SyncEventKind::PrCommentAdded,
            Subject::Pr(pr.id),
            format!("{who} commented on !{} {}", pr.id, pr.title),
        ),
        18..=29 => {
            let vote = *rng.pick(&[
                Vote::Approved,
                Vote::ApprovedWithSuggestions,
                Vote::WaitingForAuthor,
                Vote::Rejected,
            ]);
            dedupe_key = format!("{actor_id}:{}:{key}", vote.as_i8());
            (
                SyncEventKind::PrVoteChanged,
                Subject::Pr(pr.id),
                format!("{who} voted “{}” on !{} {}", vote.label(), pr.id, pr.title),
            )
        }
        30..=39 => (
            SyncEventKind::ReviewRequested,
            Subject::Pr(pr.id),
            format!("{who} requested your review on !{} {}", pr.id, pr.title),
        ),
        40..=49 => (
            SyncEventKind::MentionedInPr,
            Subject::Pr(pr.id),
            format!("{who} mentioned you in !{} {}", pr.id, pr.title),
        ),
        50..=59 => (
            SyncEventKind::PrPushed,
            Subject::Pr(pr.id),
            format!("{who} pushed to !{} {}", pr.id, pr.title),
        ),
        60..=64 => {
            dedupe_key = format!("completed:{key}");
            (
                SyncEventKind::PrStatusChanged,
                Subject::Pr(pr.id),
                format!("{who} completed !{} {}", pr.id, pr.title),
            )
        }
        65..=67 => (
            SyncEventKind::PrMergeConflict,
            Subject::Pr(pr.id),
            format!("!{} {} has merge conflicts", pr.id, pr.title),
        ),
        68..=71 => (
            SyncEventKind::PrThreadResolved,
            Subject::Pr(pr.id),
            format!("{who} resolved your thread on !{}", pr.id),
        ),
        72..=85 => {
            let run = rng.pick(&ds.runs);
            let result = if rng.chance(0.3) {
                RunResult::Failed
            } else {
                RunResult::Succeeded
            };
            (
                SyncEventKind::BuildCompleted,
                Subject::Build(run.id),
                format!(
                    "{} #{} {}",
                    run.pipeline_name,
                    run.run_number,
                    result.as_str()
                ),
            )
        }
        86..=92 => {
            let wi = rng.pick(&ds.work_items);
            (
                SyncEventKind::WorkItemAssigned,
                Subject::WorkItem(wi.id),
                format!(
                    "{who} assigned {} {} to you: {}",
                    wi.kind.as_str(),
                    wi.id,
                    wi.title
                ),
            )
        }
        _ => {
            let wi = rng.pick(&ds.work_items);
            (
                SyncEventKind::WorkItemChanged,
                Subject::WorkItem(wi.id),
                format!("{who} changed {} {}: {}", wi.kind.as_str(), wi.id, wi.title),
            )
        }
    };
    SyncEvent {
        id: 0,
        kind,
        subject,
        dedupe_key,
        actor: Some(actor_id),
        self_caused,
        at,
        summary,
    }
}

// ── generator ─────────────────────────────────────────────────────────────

struct Gen {
    seed: u64,
    now: Millis,
    rng: Rng,
    me: PersonId,
    people: Vec<Person>,
    projects: Vec<Project>,
    repos: Vec<Repo>,
    prs: Vec<PullRequest>,
    pr_extras: BTreeMap<i64, PrExtras>,
    blobs: BTreeMap<BlobId, String>,
    work_items: Vec<WorkItem>,
    wi_comments: Vec<WorkItemComment>,
    pipelines: Vec<Pipeline>,
    runs: Vec<PipelineRun>,
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

impl Gen {
    fn new(seed: u64, now: Millis) -> Self {
        Gen {
            seed,
            now,
            rng: Rng::new(seed),
            me: me().id,
            people: Vec::new(),
            projects: Vec::new(),
            repos: Vec::new(),
            prs: Vec::new(),
            pr_extras: BTreeMap::new(),
            blobs: BTreeMap::new(),
            work_items: Vec::new(),
            wi_comments: Vec::new(),
            pipelines: Vec::new(),
            runs: Vec::new(),
        }
    }

    fn others(&self) -> Vec<PersonId> {
        self.people
            .iter()
            .filter(|p| p.id != self.me && !p.is_group)
            .map(|p| p.id.clone())
            .collect()
    }

    fn run(mut self) -> Dataset {
        self.org_structure();
        self.pull_requests();
        self.pipelines_and_runs();
        self.work_items();
        let mut ds = Dataset {
            seed: self.seed,
            now: self.now,
            org: Org {
                id: guid("org:contoso"),
                name: "contoso".into(),
                url: "https://dev.azure.com/contoso".into(),
            },
            me: self.me,
            people: self.people,
            projects: self.projects,
            repos: self.repos,
            prs: self.prs,
            pr_extras: self.pr_extras,
            blobs: self.blobs.into_iter().collect(),
            work_items: self.work_items,
            wi_comments: self.wi_comments,
            pipelines: self.pipelines,
            runs: self.runs,
            stages: BTreeMap::new(),
            mentions: Vec::new(),
            past_events: Vec::new(),
        };
        let approved = ds
            .prs
            .iter_mut()
            .find(|p| {
                p.status == PrStatus::Active
                    && !p.reviewers.is_empty()
                    && !SCENARIO_PRS.contains(&p.id)
                    && !RICH_PR_IDS.contains(&p.id)
            })
            .expect("ordinary active PR");
        approved.title = "Bump Azure identity client".into();
        approved.is_draft = false;
        approved.merge_status = MergeStatus::Succeeded;
        for reviewer in &mut approved.reviewers {
            reviewer.vote = Vote::Approved;
        }
        let approved_id = approved.id;
        ds.runs.retain(|r| r.pr_id != Some(approved_id));
        let pipeline = &ds.pipelines[0];
        let latest = ds
            .runs
            .iter_mut()
            .filter(|r| r.project_id == pipeline.project_id && r.pipeline_id == pipeline.id)
            .max_by_key(|r| (r.queued, r.id))
            .expect("pipeline run");
        latest.status = RunStatus::Completed;
        latest.result = Some(RunResult::Failed);
        latest.pr_id = None;
        latest.queued = ds.now;
        latest.started = Some(ds.now);
        latest.finish = Some(ds.now);
        ds.mentions = collect_mentions(&ds);
        ds.stages = ds.runs.iter().map(|r| (r.id, demo_stages(r))).collect();
        let mut rng = Rng::fork(ds.seed, "past-events");
        let mut at = ds.now - 48 * HOUR;
        let mut past = Vec::new();
        let mut n = 0;
        loop {
            at += rng.range(15 * MINUTE, 90 * MINUTE);
            if at >= ds.now {
                break;
            }
            n += 1;
            past.push(make_event(&ds, &mut rng, at, &format!("past:{n}")));
        }
        ds.past_events = past;
        ds.past_events.push(SyncEvent {
            id: 0,
            kind: SyncEventKind::PrCommentAdded,
            subject: Subject::Pr(4293),
            dedupe_key: "demo-pin-comment".into(),
            actor: ds
                .people
                .iter()
                .find(|p| p.id != ds.me && !p.is_group)
                .map(|p| p.id.clone()),
            self_caused: false,
            at: ds.now - MINUTE,
            summary: "A teammate commented on the merge conflict".into(),
        });
        ds.past_events.sort_by_key(|e| e.at);
        ds
    }

    fn org_structure(&mut self) {
        self.people.push(me());
        // Every other teammate has a profile picture, so demo mode shows both
        // avatar paths (spec 0012).
        for (i, name) in vocab::PEOPLE.iter().enumerate() {
            let email = format!("{}@contoso.com", slug(name).replace('-', "."));
            let id = guid(&format!("person:{name}"));
            self.people.push(Person {
                image_url: (i % 2 == 0).then(|| image_url(&id)),
                id,
                display_name: (*name).to_owned(),
                unique_name: Some(email),
                is_group: false,
            });
        }
        for g in vocab::GROUPS {
            self.people.push(Person {
                id: guid(&format!("group:{g}")),
                display_name: g.rsplit('\\').next().unwrap_or(g).to_owned(),
                unique_name: Some((*g).to_owned()),
                is_group: true,
                image_url: None,
            });
        }
        for (project, repos) in vocab::PROJECTS {
            let pid = guid(&format!("project:{project}"));
            self.projects.push(Project {
                id: pid.clone(),
                name: (*project).to_owned(),
            });
            for r in *repos {
                self.repos.push(Repo {
                    id: guid(&format!("repo:{r}")),
                    project_id: pid.clone(),
                    name: (*r).to_owned(),
                    default_branch: Some("refs/heads/main".into()),
                });
            }
        }
    }

    fn blob(&mut self, content: String) -> BlobId {
        let id = object_id(&content);
        self.blobs.entry(id.clone()).or_insert(content);
        id
    }

    fn pull_requests(&mut self) {
        let others = self.others();
        let now = self.now;
        for i in 0..300i64 {
            let id = 4101 + i;
            let rng = &mut self.rng;
            let repo = rng.pick(&self.repos).clone();
            let roll = rng.below(100);
            let status = match roll {
                0..=36 => PrStatus::Active,
                37..=86 => PrStatus::Completed,
                _ => PrStatus::Abandoned,
            };
            let author = if rng.chance(0.1) {
                self.me.clone()
            } else {
                rng.pick(&others).clone()
            };
            let (created, closed) = if status == PrStatus::Active {
                (now - rng.range(HOUR, 21 * DAY), None)
            } else {
                let created = now - rng.range(DAY, 120 * DAY);
                let closed = (created + rng.range(2 * HOUR, 5 * DAY)).min(now - HOUR);
                (created, Some(closed))
            };
            let end = closed.unwrap_or(now);
            let mut reviewers = Vec::new();
            let n_rev = rng.range(1, 4) as usize;
            for idx in rng.distinct(others.len(), n_rev + 1) {
                let person = others[idx].clone();
                if person == author || reviewers.len() >= n_rev {
                    continue;
                }
                reviewers.push(person);
            }
            if author != self.me && rng.chance(0.2) {
                reviewers.insert(0, self.me.clone());
            }
            if rng.chance(0.3) {
                let team = self.people.iter().find(|p| {
                    p.is_group
                        && p.unique_name.as_deref().is_some_and(|u| {
                            u.contains(
                                self.projects
                                    .iter()
                                    .find(|pr| pr.id == repo.project_id)
                                    .map(|p| p.name.as_str())
                                    .unwrap_or("?"),
                            )
                        })
                });
                if let Some(team) = team {
                    reviewers.push(team.id.clone());
                }
            }
            let reviewers: Vec<Reviewer> = reviewers
                .into_iter()
                .enumerate()
                .map(|(k, person)| {
                    let added = created + rng.range(0, 30) * MINUTE;
                    let vote = match status {
                        PrStatus::Completed => {
                            if rng.chance(0.8) {
                                Vote::Approved
                            } else {
                                Vote::ApprovedWithSuggestions
                            }
                        }
                        _ => match rng.below(10) {
                            0..=4 => Vote::NoVote,
                            5..=6 => Vote::Approved,
                            7 => Vote::ApprovedWithSuggestions,
                            8 => Vote::WaitingForAuthor,
                            _ => Vote::Rejected,
                        },
                    };
                    let voted_at = (vote != Vote::NoVote)
                        .then(|| (added + rng.range(20 * MINUTE, 2 * DAY)).min(end));
                    Reviewer {
                        person,
                        vote,
                        is_required: k == 0 && rng.chance(0.6),
                        has_declined: false,
                        added_at: Some(added),
                        voted_at,
                    }
                })
                .collect();
            let verb = rng.pick(vocab::PR_VERBS);
            let object = rng.pick(vocab::PR_OBJECTS);
            let suffix = rng.pick(vocab::PR_SUFFIXES);
            let title = format!("{verb} {object}{suffix}");
            let prefix = rng.pick(vocab::BRANCH_PREFIXES);
            let author_name = self
                .people
                .iter()
                .find(|p| p.id == author)
                .map(|p| p.display_name.clone())
                .unwrap_or_default();
            let branch = if *prefix == "users" {
                format!(
                    "refs/heads/users/{}/{}",
                    slug(author_name.split(' ').next().unwrap_or("dev")),
                    slug(object)
                )
            } else {
                format!("refs/heads/{prefix}/{}", slug(object))
            };
            let iterations = rng.range(1, 6) as u32;
            let last_push = (created + rng.range(0, (end - created).max(1))).min(end);
            let merge_status = match status {
                PrStatus::Active => match rng.below(20) {
                    0 => MergeStatus::Conflicts,
                    1 => MergeStatus::Queued,
                    _ => MergeStatus::Succeeded,
                },
                PrStatus::Completed => MergeStatus::Succeeded,
                PrStatus::Abandoned => MergeStatus::NotSet,
            };
            let updated = reviewers
                .iter()
                .filter_map(|r| r.voted_at)
                .chain([last_push, created])
                .chain(closed)
                .max()
                .unwrap_or(created);
            let pr = PullRequest {
                id,
                repo_id: repo.id.clone(),
                project_id: repo.project_id.clone(),
                title,
                description: rng.pick(vocab::PR_DESCRIPTIONS).to_string(),
                author,
                reviewers,
                status,
                is_draft: status == PrStatus::Active && rng.chance(0.1),
                merge_status,
                source_branch: branch,
                target_branch: if rng.chance(0.1) {
                    "refs/heads/release/2026.09".into()
                } else {
                    "refs/heads/main".into()
                },
                last_source_commit: Some(object_id(&format!("{id}:{iterations}"))),
                created,
                updated,
                closed,
                iterations,
                last_push: Some(last_push),
            };
            self.prs.push(pr);
        }
        self.nudge_scenarios();
        for k in 0..RICH_PR_IDS.len() {
            self.rich_pr(k);
        }
        let ids: Vec<i64> = self
            .prs
            .iter()
            .filter(|p| {
                !RICH_PR_IDS.contains(&p.id)
                    && (p.status == PrStatus::Active
                        || p.closed.is_some_and(|c| c > now - 30 * DAY))
            })
            .map(|p| p.id)
            .collect();
        for id in ids {
            self.generic_extras(id);
        }
    }

    fn pr_mut(&mut self, id: i64) -> &mut PullRequest {
        self.prs.iter_mut().find(|p| p.id == id).expect("demo pr")
    }

    /// Deterministic PRs that trigger each smart nudge (spec 0006) for me.
    fn nudge_scenarios(&mut self) {
        let now = self.now;
        let me = self.me.clone();
        let others = self.others();
        let hannah = others[2].clone();
        let tom = others[5].clone();
        let reviewer =
            |person: &PersonId, vote: Vote, added: Millis, voted: Option<Millis>| Reviewer {
                person: person.clone(),
                vote,
                is_required: true,
                has_declined: false,
                added_at: Some(added),
                voted_at: voted,
            };
        let setup = |pr: &mut PullRequest, title: &str, created: Millis| {
            pr.status = PrStatus::Active;
            pr.closed = None;
            pr.is_draft = false;
            pr.merge_status = MergeStatus::Succeeded;
            pr.title = title.to_owned();
            pr.created = created;
            pr.last_push = Some(created + HOUR);
            pr.updated = created + HOUR;
        };
        // N: my PR waiting for author (-5).
        {
            let pr = self.pr_mut(4290);
            setup(
                pr,
                "Add idempotency keys to payout batch API",
                now - 3 * DAY,
            );
            pr.author = me.clone();
            pr.reviewers = vec![reviewer(
                &hannah,
                Vote::WaitingForAuthor,
                now - 3 * DAY,
                Some(now - 2 * DAY),
            )];
            pr.updated = now - 2 * DAY;
        }
        // My PR with a failing build (runs added in pipelines_and_runs).
        {
            let pr = self.pr_mut(4291);
            setup(
                pr,
                "Migrate fraud rules to the new scoring service",
                now - 2 * DAY,
            );
            pr.author = me.clone();
            pr.reviewers = vec![reviewer(&tom, Vote::NoVote, now - 2 * DAY, None)];
        }
        // My PR with no votes for 4 days.
        {
            let pr = self.pr_mut(4292);
            setup(pr, "Document ledger reconciliation runbook", now - 4 * DAY);
            pr.author = me.clone();
            pr.reviewers = vec![
                reviewer(&hannah, Vote::NoVote, now - 4 * DAY, None),
                Reviewer {
                    is_required: false,
                    ..reviewer(&tom, Vote::NoVote, now - 4 * DAY, None)
                },
            ];
        }
        // My PR with merge conflicts.
        {
            let pr = self.pr_mut(4293);
            setup(pr, "Cache exchange rates per settlement batch", now - DAY);
            pr.author = me.clone();
            pr.merge_status = MergeStatus::Conflicts;
            pr.reviewers = vec![reviewer(
                &tom,
                Vote::Approved,
                now - DAY,
                Some(now - 20 * HOUR),
            )];
            pr.updated = now - 3 * HOUR;
        }
        // Review requested of me 2 days ago, still no vote.
        {
            let pr = self.pr_mut(4294);
            setup(
                pr,
                "Harden webhook signature validation",
                now - 2 * DAY - 3 * HOUR,
            );
            pr.author = hannah.clone();
            pr.reviewers = vec![reviewer(&me, Vote::NoVote, now - 2 * DAY, None)];
        }
        // Completed PR linked to an active work item of mine (WI scenario).
        {
            let pr = self.pr_mut(4295);
            setup(
                pr,
                "Fix timezone bug in daily settlement job",
                now - 4 * DAY,
            );
            pr.author = me.clone();
            pr.status = PrStatus::Completed;
            pr.closed = Some(now - 2 * DAY);
            pr.updated = now - 2 * DAY;
            pr.reviewers = vec![reviewer(
                &tom,
                Vote::Approved,
                now - 4 * DAY,
                Some(now - 2 * DAY - HOUR),
            )];
        }
        for id in 4290..=4295 {
            let pr = self.pr_mut(id);
            pr.last_source_commit = Some(object_id(&format!("{id}:{}", pr.iterations)));
        }
    }

    fn rich_pr(&mut self, k: usize) {
        let spec = &code::RICH[k];
        let id = RICH_PR_IDS[k];
        let now = self.now;
        let others = self.others();
        let repo = self
            .repos
            .iter()
            .find(|r| r.name == spec.repo)
            .expect("rich repo")
            .clone();
        let (author, r1, r2) = match k {
            0 => (others[0].clone(), others[3].clone(), others[4].clone()),
            1 => (others[1].clone(), others[6].clone(), others[7].clone()),
            _ => (self.me.clone(), others[0].clone(), others[8].clone()),
        };
        let created = now - [3 * DAY, 26 * HOUR, 30 * HOUR][k];
        let me = self.me.clone();
        let content = code::build(
            k,
            id,
            created,
            &code::Cast {
                author: &author,
                me: &me,
                r1: &r1,
                r2: &r2,
            },
        );
        let mut reviewers = vec![
            Reviewer {
                person: r1.clone(),
                vote: [
                    Vote::ApprovedWithSuggestions,
                    Vote::Approved,
                    Vote::WaitingForAuthor,
                ][k],
                is_required: true,
                has_declined: false,
                added_at: Some(created),
                voted_at: Some(created + 5 * HOUR),
            },
            Reviewer {
                person: r2.clone(),
                vote: Vote::NoVote,
                is_required: false,
                has_declined: false,
                added_at: Some(created),
                voted_at: None,
            },
        ];
        if author != me {
            reviewers.insert(
                0,
                Reviewer {
                    person: me.clone(),
                    vote: Vote::NoVote,
                    is_required: true,
                    has_declined: false,
                    added_at: Some(created),
                    voted_at: None,
                },
            );
        }
        let iterations: Vec<PrIteration> = (1..=content.iterations)
            .map(|n| PrIteration {
                id: n,
                source_commit: object_id(&format!("{id}:{n}")),
                created: created + (n as i64 - 1) * 24 * HOUR,
                description: Some(if n == 1 {
                    "Initial push".into()
                } else {
                    format!("Address review feedback ({n})")
                }),
                commit_count: Some(1 + (n % 3)),
                commits_truncated: false,
            })
            .collect();
        let last_push = iterations.last().map(|i| i.created);
        let updated = content
            .threads
            .iter()
            .map(|t| t.updated)
            .chain(last_push)
            .max()
            .unwrap_or(created)
            .min(now);
        {
            let pr = self.pr_mut(id);
            pr.repo_id = repo.id.clone();
            pr.project_id = repo.project_id.clone();
            pr.title = spec.title.to_owned();
            pr.description = spec.description.to_owned();
            pr.author = author;
            pr.reviewers = reviewers;
            pr.status = PrStatus::Active;
            pr.is_draft = false;
            pr.merge_status = MergeStatus::Succeeded;
            pr.source_branch = spec.branch.to_owned();
            pr.target_branch = "refs/heads/main".into();
            pr.created = created;
            pr.closed = None;
            pr.iterations = content.iterations;
            pr.last_push = last_push;
            pr.last_source_commit = iterations.last().map(|i| i.source_commit.clone());
            pr.updated = updated;
        }
        for (bid, text) in content.blobs {
            self.blobs.entry(bid).or_insert(text);
        }
        let mut threads = content.threads;
        threads.extend(self.vote_threads(id, 100));
        self.pr_extras.insert(
            id,
            PrExtras {
                iterations,
                files: content.files,
                threads,
            },
        );
    }

    /// System threads recording each cast vote (ADO posts these).
    fn vote_threads(&self, pr_id: i64, first_id: i64) -> Vec<PrThread> {
        let pr = self.prs.iter().find(|p| p.id == pr_id).expect("pr");
        pr.reviewers
            .iter()
            .filter_map(|r| Some((r, r.voted_at?)))
            .enumerate()
            .map(|(k, (r, at))| {
                let name = self
                    .people
                    .iter()
                    .find(|p| p.id == r.person)
                    .map(|p| p.display_name.as_str())
                    .unwrap_or("Someone");
                PrThread {
                    id: first_id + k as i64,
                    pr_id,
                    status: ThreadStatus::Closed,
                    anchor: None,
                    iteration: None,
                    comments: vec![Comment {
                        id: 1,
                        parent_id: None,
                        author: r.person.clone(),
                        body: format!("{name} voted {}", r.vote.as_i8()),
                        created: at,
                        updated: at,
                        is_system: true,
                        is_deleted: false,
                    }],
                    updated: at,
                }
            })
            .collect()
    }

    fn generic_extras(&mut self, id: i64) {
        let pr = self.prs.iter().find(|p| p.id == id).expect("pr").clone();
        let repo_name = self
            .repos
            .iter()
            .find(|r| r.id == pr.repo_id)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let mut rng = Rng::fork(self.seed, &format!("pr:{id}"));
        let end = pr.closed.unwrap_or(self.now);
        let iterations: Vec<PrIteration> = (1..=pr.iterations)
            .map(|n| PrIteration {
                id: n,
                source_commit: object_id(&format!("{id}:{n}")),
                created: if n == pr.iterations {
                    pr.last_push.unwrap_or(pr.created)
                } else {
                    pr.created
                        + (pr.last_push.unwrap_or(pr.created) - pr.created) * (n as i64 - 1)
                            / pr.iterations as i64
                },
                description: None,
                // Deterministic without drawing from `rng` (later draws
                // must not move).
                commit_count: Some(1 + ((id as u32).wrapping_add(n) % 3)),
                commits_truncated: false,
            })
            .collect();
        let ts = repo_name.contains("web");
        let n_files = rng.range(1, 3);
        let mut files = Vec::new();
        for f in 0..n_files {
            let (path, old, new) = generic_file(&mut rng, &repo_name, ts, f);
            let old_id = old.map(|o| self.blob(o));
            let new_id = self.blob(new);
            files.push(PrFileChange {
                path,
                original_path: None,
                change_type: if old_id.is_some() {
                    ChangeType::Edit
                } else {
                    ChangeType::Add
                },
                old_content: old_id,
                new_content: Some(new_id),
                change_tracking_id: Some(f as u32 + 1),
            });
        }
        let mut people: Vec<PersonId> = pr
            .reviewers
            .iter()
            .map(|r| r.person.clone())
            .filter(|p| self.people.iter().any(|x| x.id == *p && !x.is_group))
            .collect();
        people.push(pr.author.clone());
        let involves_me = pr.author == self.me || pr.is_reviewer(&self.me);
        let mut threads = Vec::new();
        // Scenario PRs keep only system threads (N4: no human comments).
        let n_threads = if SCENARIO_PRS.contains(&id) {
            0
        } else {
            rng.range(0, 4)
        };
        for t in 0..n_threads {
            let start = pr.created + rng.range(HOUR, (end - pr.created).max(2 * HOUR));
            let n_comments = rng.range(1, 3);
            let mut comments = Vec::new();
            let mut at = start.min(end);
            for c in 0..n_comments {
                let author = rng.pick(&people).clone();
                let body = if author != self.me && rng.chance(if involves_me { 0.2 } else { 0.03 })
                {
                    rng.pick(vocab::MENTION_COMMENTS)
                        .replace("{me}", &format!("@<{}>", self.me))
                } else {
                    rng.pick(vocab::COMMENTS).to_string()
                };
                comments.push(Comment {
                    id: c + 1,
                    parent_id: (c > 0).then_some(1),
                    author,
                    body,
                    created: at,
                    updated: at,
                    is_system: false,
                    is_deleted: false,
                });
                at = (at + rng.range(10 * MINUTE, 6 * HOUR)).min(end);
            }
            let status = if pr.status == PrStatus::Active {
                *rng.pick(&[
                    ThreadStatus::Active,
                    ThreadStatus::Active,
                    ThreadStatus::Fixed,
                    ThreadStatus::Closed,
                ])
            } else {
                ThreadStatus::Fixed
            };
            let anchor = files
                .first()
                .filter(|_| rng.chance(0.5))
                .map(|f| ThreadAnchor {
                    file_path: f.path.clone(),
                    right: Some(LineRange {
                        start: 3 + t as u32,
                        end: 4 + t as u32,
                    }),
                    left: None,
                    iteration_blob: None,
                });
            threads.push(PrThread {
                id: t + 1,
                pr_id: id,
                status,
                iteration: anchor.as_ref().map(|_| pr.iterations),
                anchor,
                updated: comments.last().map(|c| c.updated).unwrap_or(start),
                comments,
            });
        }
        threads.extend(self.vote_threads(id, 100));
        let last_activity = threads.iter().map(|t| t.updated).max();
        let pr_mut = self.pr_mut(id);
        if let Some(t) = last_activity {
            pr_mut.updated = pr_mut.updated.max(t);
        }
        self.pr_extras.insert(
            id,
            PrExtras {
                iterations,
                files,
                threads,
            },
        );
    }

    fn pipelines_and_runs(&mut self) {
        let now = self.now;
        for (k, (name, folder)) in vocab::PIPELINES.iter().enumerate() {
            let project_name = folder
                .trim_start_matches('\\')
                .split('\\')
                .next()
                .unwrap_or("");
            let project = self
                .projects
                .iter()
                .find(|p| p.name == project_name)
                .expect("pipeline project");
            self.pipelines.push(Pipeline {
                id: k as i64 + 1,
                project_id: project.id.clone(),
                name: (*name).to_owned(),
                folder: (*folder).to_owned(),
            });
        }
        let others = self.others();
        let mut run_id = 90_001;
        let pipelines = self.pipelines.clone();
        let active: Vec<PullRequest> = self
            .prs
            .iter()
            .filter(|p| p.status == PrStatus::Active)
            .cloned()
            .collect();
        let mut runs = Vec::new();
        for p in &pipelines {
            let mut rng = Rng::fork(self.seed, &format!("runs:{}", p.id));
            let mut t = now - 14 * DAY;
            let mut daily = 0;
            loop {
                t += rng.range(2 * HOUR, 9 * HOUR);
                if t >= now {
                    break;
                }
                daily += 1;
                let pr = (rng.chance(0.4))
                    .then(|| {
                        active
                            .iter()
                            .filter(|pr| {
                                pr.project_id == p.project_id
                                    && pr.created < t
                                    && !SCENARIO_PRS.contains(&pr.id)
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|v| !v.is_empty())
                    .map(|v| (*rng.pick(&v)).clone());
                let (reason, branch, requested_for, pr_id) = match &pr {
                    Some(pr) => (
                        RunReason::PullRequest,
                        format!("refs/pull/{}/merge", pr.id),
                        pr.author.clone(),
                        Some(pr.id),
                    ),
                    None if p.name.contains("Nightly") || p.name.contains("Reindex") => (
                        RunReason::Schedule,
                        "refs/heads/main".into(),
                        rng.pick(&others).clone(),
                        None,
                    ),
                    None => (
                        if rng.chance(0.15) {
                            RunReason::Manual
                        } else {
                            RunReason::IndividualCi
                        },
                        "refs/heads/main".into(),
                        if rng.chance(0.12) {
                            self.me.clone()
                        } else {
                            rng.pick(&others).clone()
                        },
                        None,
                    ),
                };
                let duration = rng.range(4 * MINUTE, 35 * MINUTE);
                let started = t + rng.range(10_000, 3 * MINUTE);
                let finish = started + duration;
                let (status, result, finish) = if finish > now {
                    (
                        if started > now {
                            RunStatus::NotStarted
                        } else {
                            RunStatus::InProgress
                        },
                        None,
                        None,
                    )
                } else {
                    let r = match rng.below(100) {
                        0..=79 => RunResult::Succeeded,
                        80..=91 => RunResult::Failed,
                        92..=96 => RunResult::PartiallySucceeded,
                        _ => RunResult::Canceled,
                    };
                    (RunStatus::Completed, Some(r), Some(finish))
                };
                runs.push(PipelineRun {
                    id: run_id,
                    project_id: p.project_id.clone(),
                    pipeline_id: p.id,
                    pipeline_name: p.name.clone(),
                    run_number: format!("{}.{daily}", 20_260_000 + (t / DAY) % 10_000),
                    status,
                    result,
                    reason,
                    branch,
                    source_version: Some(object_id(&format!("run:{run_id}"))),
                    requested_for: Some(requested_for),
                    pr_id,
                    queued: t,
                    started: (status != RunStatus::NotStarted).then_some(started),
                    finish,
                });
                run_id += 1;
            }
        }
        // Scenario: my PR 4291 has a failed latest build; 4290 a green one;
        // plus one of my runs in progress right now.
        for (pr_id, result, ago) in [
            (4291, RunResult::Failed, 19 * HOUR + 30 * MINUTE),
            (4290, RunResult::Succeeded, 26 * HOUR),
            (4293, RunResult::Succeeded, 5 * HOUR),
        ] {
            let pr = self.prs.iter().find(|p| p.id == pr_id).expect("pr");
            let p = pipelines
                .iter()
                .find(|p| p.project_id == pr.project_id)
                .expect("pipeline");
            runs.push(PipelineRun {
                id: run_id,
                project_id: p.project_id.clone(),
                pipeline_id: p.id,
                pipeline_name: p.name.clone(),
                run_number: format!("20260925.{}", run_id % 100),
                status: RunStatus::Completed,
                result: Some(result),
                reason: RunReason::PullRequest,
                branch: format!("refs/pull/{pr_id}/merge"),
                source_version: pr.last_source_commit.clone(),
                requested_for: Some(pr.author.clone()),
                pr_id: Some(pr_id),
                queued: now - ago - 12 * MINUTE,
                started: Some(now - ago - 11 * MINUTE),
                finish: Some(now - ago),
            });
            run_id += 1;
        }
        let p = &pipelines[1];
        runs.push(PipelineRun {
            id: run_id,
            project_id: p.project_id.clone(),
            pipeline_id: p.id,
            pipeline_name: p.name.clone(),
            run_number: "20260925.7".into(),
            status: RunStatus::InProgress,
            result: None,
            reason: RunReason::Manual,
            branch: "refs/heads/main".into(),
            source_version: Some(object_id("deploy")),
            requested_for: Some(self.me.clone()),
            pr_id: None,
            queued: now - 6 * MINUTE,
            started: Some(now - 5 * MINUTE),
            finish: None,
        });
        runs.sort_by_key(|r| r.id);
        self.runs = runs;
    }

    fn work_items(&mut self) {
        let now = self.now;
        let others = self.others();
        let mut rng = Rng::fork(self.seed, "work-items");
        let mut by_type: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
        let completed_prs: Vec<(i64, String)> = self
            .prs
            .iter()
            .map(|p| (p.id, p.project_id.clone()))
            .collect();
        for i in 0..5000i64 {
            let id = 20_001 + i;
            let kind = if i < 12 {
                WorkItemType::Epic
            } else if i < 72 {
                WorkItemType::Feature
            } else {
                match rng.below(100) {
                    0..=17 => WorkItemType::UserStory,
                    18..=44 => WorkItemType::Bug,
                    _ => WorkItemType::Task,
                }
            };
            let project = rng.pick(&self.projects).clone();
            let area = rng.pick(vocab::AREAS);
            let family = kind.family();
            let base = match family {
                TypeFamily::Epic => rng.pick(vocab::WI_EPIC),
                TypeFamily::Feature => rng.pick(vocab::WI_FEATURE),
                TypeFamily::Requirement => rng.pick(vocab::WI_STORY),
                TypeFamily::Bug => rng.pick(vocab::WI_BUG),
                _ => rng.pick(vocab::WI_TASK),
            };
            let title = if family == TypeFamily::Task {
                format!("{base}: {}", rng.pick(vocab::PR_OBJECTS))
            } else if rng.chance(0.5) {
                format!("{base} ({area})")
            } else {
                (*base).to_owned()
            };
            let created = now - rng.range(DAY, 400 * DAY);
            // Skewed toward recent activity.
            let span = now - created;
            let u = rng.below(1000) as i64;
            let changed = (now - span / 1_000_000 * u * u - rng.range(MINUTE, HOUR)).max(created);
            let state = {
                let states: &[&str] = match family {
                    TypeFamily::Task => &["New", "Active", "Active", "Closed", "Closed", "Doing"],
                    _ => &[
                        "New", "Active", "Active", "Resolved", "Closed", "Closed", "Closed",
                        "Removed",
                    ],
                };
                (*rng.pick(states)).to_owned()
            };
            let assigned_to = match rng.below(100) {
                0..=2 => Some(self.me.clone()),
                3..=72 => Some(rng.pick(&others).clone()),
                _ => None,
            };
            let parent_type = match family {
                TypeFamily::Feature => Some("Epic"),
                TypeFamily::Requirement | TypeFamily::Bug => Some("Feature"),
                TypeFamily::Task => Some(if rng.chance(0.5) { "User Story" } else { "Bug" }),
                _ => None,
            };
            let parent_id = parent_type
                .and_then(|t| by_type.get(t))
                .filter(|v| !v.is_empty())
                .map(|v| {
                    let same: Vec<&(i64, String)> =
                        v.iter().filter(|(_, p)| *p == project.id).collect();
                    if same.is_empty() {
                        v[rng.below(v.len() as u64) as usize].0
                    } else {
                        same[rng.below(same.len() as u64) as usize].0
                    }
                });
            let linked_pr_ids =
                if matches!(family, TypeFamily::Requirement | TypeFamily::Bug) && rng.chance(0.2) {
                    let same: Vec<i64> = completed_prs
                        .iter()
                        .filter(|(_, p)| *p == project.id)
                        .map(|(id, _)| *id)
                        .collect();
                    vec![*rng.pick(&same)]
                } else {
                    Vec::new()
                };
            let mine = assigned_to.as_deref() == Some(self.me.as_str());
            let n_tags = rng.range(0, 2) as usize;
            let tags = rng
                .distinct(vocab::TAGS.len(), n_tags)
                .into_iter()
                .map(|t| vocab::TAGS[t].to_owned())
                .collect();
            let wi = WorkItem {
                id,
                project_id: project.id.clone(),
                rev: rng.range(1, 30),
                kind,
                title,
                state_category: StateCategory::of_state(&state),
                state,
                assigned_to,
                created_by: rng.pick(&others).clone(),
                changed_by: rng.pick(&others).clone(),
                area_path: format!("{}\\{area}", project.name),
                iteration_path: format!("{}\\{}", project.name, rng.pick(vocab::SPRINTS)),
                priority: Some(rng.range(1, 4) as u8),
                tags,
                parent_id,
                linked_pr_ids,
                description: mine.then(|| format!("Tracking work for {}.\n\nAcceptance: covered by tests and verified in staging.", project.name)),
                comment_count: 0,
                created,
                changed,
            };
            by_type
                .entry(kind_key(&wi.kind))
                .or_default()
                .push((id, project.id.clone()));
            self.work_items.push(wi);
        }
        // Scenario: active WI of mine whose linked PR (4295) completed.
        let me = self.me.clone();
        let pr_project = self
            .prs
            .iter()
            .find(|p| p.id == 4295)
            .map(|p| p.project_id.clone())
            .expect("4295");
        {
            let wi = &mut self.work_items[100];
            wi.kind = WorkItemType::Bug;
            wi.title = "Timezone bug in daily settlement job".into();
            wi.project_id = pr_project;
            wi.state = "Doing".into();
            wi.state_category = StateCategory::InProgress;
            wi.assigned_to = Some(me.clone());
            wi.linked_pr_ids = vec![4295];
            wi.changed = now - 4 * DAY;
        }
        // Scenario: active WI of mine, quiet for 8 days.
        {
            let wi = &mut self.work_items[101];
            wi.kind = WorkItemType::UserStory;
            wi.title = "As finance I can reconcile ledger entries daily".into();
            wi.state = "Active".into();
            wi.state_category = StateCategory::InProgress;
            wi.assigned_to = Some(me.clone());
            wi.linked_pr_ids.clear();
            wi.changed = now - 8 * DAY;
            wi.changed_by = me.clone();
        }
        for i in [100, 101] {
            let wi = &mut self.work_items[i];
            wi.created = wi.created.min(wi.changed - 3 * DAY);
        }
        // Comments on my items, some mentioning me.
        let mine: Vec<usize> = (0..self.work_items.len())
            .filter(|&i| self.work_items[i].assigned_to.as_deref() == Some(me.as_str()))
            .collect();
        let mut comment_id = 1;
        for i in mine {
            let n = rng.range(0, 3);
            let (wid, created, changed) = {
                let w = &self.work_items[i];
                (w.id, w.created, w.changed)
            };
            for _ in 0..n {
                let author = rng.pick(&others).clone();
                let at = created + rng.range(0, (changed - created).max(1));
                let body = if rng.chance(0.25) {
                    format!("@<{me}> {}", rng.pick(vocab::WI_COMMENTS))
                } else {
                    rng.pick(vocab::WI_COMMENTS).to_string()
                };
                self.wi_comments.push(WorkItemComment {
                    id: comment_id,
                    work_item_id: wid,
                    author,
                    source: Some(body.clone()),
                    format: TextFormat::Markdown,
                    body,
                    mentions: Vec::new(),
                    created: at,
                    updated: at,
                });
                comment_id += 1;
            }
            self.work_items[i].comment_count = n as u32;
        }
    }
}

fn kind_key(kind: &WorkItemType) -> String {
    kind.as_str().to_owned()
}

/// A small plausible source file for generic PRs: `(path, old, new)`.
fn generic_file(rng: &mut Rng, repo: &str, ts: bool, k: i64) -> (String, Option<String>, String) {
    const RS_MODULES: &[&str] = &[
        "config", "handler", "client", "metrics", "store", "worker", "routes",
    ];
    const TS_MODULES: &[&str] = &[
        "Cart",
        "Checkout",
        "ProductCard",
        "SearchBox",
        "api",
        "session",
        "i18n",
    ];
    let module = if ts {
        *rng.pick(TS_MODULES)
    } else {
        *rng.pick(RS_MODULES)
    };
    let path = if ts {
        format!(
            "/src/{}{module}.{}",
            if module.chars().next().is_some_and(char::is_uppercase) {
                "components/"
            } else {
                "lib/"
            },
            if module.chars().next().is_some_and(char::is_uppercase) {
                "tsx"
            } else {
                "ts"
            }
        )
    } else {
        format!("/src/{module}.rs")
    };
    let n_fns = rng.range(3, 8);
    let mut old = String::new();
    let mut new = String::new();
    let fns: Vec<String> = (0..n_fns)
        .map(|i| {
            format!(
                "{}_{}",
                [
                    "load", "parse", "apply", "flush", "render", "validate", "retry"
                ][i as usize % 7],
                module.to_lowercase()
            )
        })
        .collect();
    let changed = rng.below(n_fns as u64) as usize;
    if ts {
        let header = format!("// {repo}: {module}\nimport {{ logger }} from \"../logger\";\n\n");
        old.push_str(&header);
        new.push_str(&header);
        for (i, f) in fns.iter().enumerate() {
            let body = format!(
                "export function {f}(input: string): string {{\n  logger.debug(\"{f}\", {{ input }});\n  const value = input.trim();\n  return value;\n}}\n\n"
            );
            old.push_str(&body);
            if i == changed {
                new.push_str(&format!("export function {f}(input: string): string {{\n  logger.debug(\"{f}\", {{ input }});\n  const value = input.trim();\n  if (value.length === 0) {{\n    throw new Error(\"{f}: empty input\");\n  }}\n  return value;\n}}\n\n"));
            } else {
                new.push_str(&body);
            }
        }
    } else {
        let header = format!("//! {repo}: {module}\n\nuse crate::error::Error;\n\n");
        old.push_str(&header);
        new.push_str(&header);
        for (i, f) in fns.iter().enumerate() {
            let body = format!(
                "pub fn {f}(input: &str) -> Result<String, Error> {{\n    tracing::debug!(input, \"{f}\");\n    Ok(input.trim().to_owned())\n}}\n\n"
            );
            old.push_str(&body);
            if i == changed {
                new.push_str(&format!("pub fn {f}(input: &str) -> Result<String, Error> {{\n    tracing::debug!(input, \"{f}\");\n    let value = input.trim();\n    if value.is_empty() {{\n        return Err(Error::EmptyInput(\"{f}\"));\n    }}\n    Ok(value.to_owned())\n}}\n\n"));
            } else {
                new.push_str(&body);
            }
        }
    }
    let _ = k;
    if rng.chance(0.15) {
        (path, None, new)
    } else {
        (path, Some(old), new)
    }
}

fn collect_mentions(ds: &Dataset) -> Vec<Mention> {
    let token = format!("@<{}>", ds.me);
    let resolve = |body: &str| {
        let mut text = body.to_owned();
        for p in &ds.people {
            let t = format!("@<{}>", p.id);
            if text.contains(&t) {
                text = text.replace(&t, &format!("@{}", p.display_name));
            }
        }
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let mut out = Vec::new();
    for (pr_id, extra) in &ds.pr_extras {
        for t in &extra.threads {
            for c in &t.comments {
                if c.body.contains(&token) && !c.is_deleted {
                    out.push(Mention {
                        source: MentionSource::Pr {
                            pr_id: *pr_id,
                            thread_id: t.id,
                            comment_id: c.id,
                        },
                        who: c.author.clone(),
                        mentioned: ds.me.clone(),
                        when: c.created,
                        snippet: resolve(&c.body),
                    });
                }
            }
        }
    }
    for c in &ds.wi_comments {
        if c.body.contains(&token) {
            out.push(Mention {
                source: MentionSource::WorkItem {
                    work_item_id: c.work_item_id,
                    comment_id: c.id,
                },
                who: c.author.clone(),
                mentioned: ds.me.clone(),
                when: c.created,
                snippet: resolve(&c.body),
            });
        }
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.when));
    out
}

#[cfg(test)]
mod tests;
