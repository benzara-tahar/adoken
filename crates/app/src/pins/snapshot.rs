//! The pins' status snapshot (spec 0014 §7): every pin with its headline,
//! change dot and staleness, computed from the store on a background
//! executor after store commits (debounced, like Today's day). The sidebar
//! frame only lays it out.

use std::collections::HashMap;

use adoken_model::{
    Millis, Pin, PinSubject, PipelineRun, PrStatus, RunResult, RunStatus, StateCategory,
    time::MINUTE,
};
use adoken_store::rusqlite::Connection;
use adoken_store::{RunFilter, Store, read};

use super::status::{self, Headline, Kind};

/// Runs of a definition read for its headline and change dot.
const DEFINITION_RUNS: usize = 20;
/// 0003's stale rule: older than 3 × the warm interval (2 min).
pub const STALE_AFTER: Millis = 3 * 2 * MINUTE;

/// One pin as the sidebar, rail, peek and `⌘J` show it.
#[derive(Clone, Debug, PartialEq)]
pub struct PinView {
    pub pin: Pin,
    pub kind: Kind,
    /// The Azure DevOps title: stored item, else the pin's last known one.
    pub title: String,
    pub headline: Headline,
    /// FR-10 change dot.
    pub changed: bool,
    /// Last successful sync, when older than the stale threshold.
    pub stale_since: Option<Millis>,
    /// When the PR closed / the work item reached Completed or Removed.
    pub closed_at: Option<Millis>,
    /// Definitions: the latest completed, not canceled, run's result (what
    /// opening the pin records as seen).
    pub latest_result: Option<RunResult>,
    /// `⌘↩` target; `None` in the demo org or without an org URL.
    pub web_url: Option<String>,
}

impl PinView {
    pub fn subject(&self) -> &PinSubject {
        &self.pin.subject
    }

    /// The label if set, else the Azure DevOps title.
    pub fn display_title(&self) -> &str {
        self.pin.label.as_deref().unwrap_or(&self.title)
    }

    /// A row for a subject pinned this frame, before the snapshot knows it
    /// (⌘D shows the change on the next frame, §7).
    pub fn provisional(pin: Pin) -> Self {
        let kind = Kind::of(&pin.subject);
        PinView {
            title: pin.title.clone(),
            kind,
            headline: Headline::Fetching,
            changed: false,
            stale_since: None,
            closed_at: None,
            latest_result: pin.seen_result,
            web_url: None,
            pin,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub pins: Vec<PinView>,
    /// Opens from the pins in the last 7 days (the `⌘J` footer stat, §8).
    pub opens_this_week: u32,
    /// Store generation this snapshot read.
    pub generation: u64,
}

impl Snapshot {
    pub fn summary(&self) -> status::Summary {
        summary(&self.pins)
    }
}

pub fn summary(pins: &[PinView]) -> status::Summary {
    status::Summary {
        total: pins.len(),
        changed: pins.iter().filter(|p| p.changed).count(),
        failing: pins.iter().filter(|p| p.headline.is_failing()).count(),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub now: Millis,
    /// `[pins] auto_unpin` (FR-12).
    pub auto_unpin: bool,
    /// `None` turns staleness off (the demo org never syncs).
    pub stale_after: Option<Millis>,
}

/// Reads the snapshot; also returns the pins due for auto-unpin (FR-12),
/// which the caller removes (silently).
pub fn read(store: &Store, options: Options) -> adoken_store::Result<(Snapshot, Vec<PinSubject>)> {
    let (snapshot, due) = read_candidates(store, options)?;
    Ok((snapshot, due.into_iter().map(|pin| pin.subject).collect()))
}

pub(super) fn read_candidates(
    store: &Store,
    options: Options,
) -> adoken_store::Result<(Snapshot, Vec<Pin>)> {
    let generation = store.generation();
    let (mut snapshot, due) = store.with_conn(|connection| {
        let transaction = connection.unchecked_transaction()?;
        let result = read_conn_candidates(&transaction, options)?;
        transaction.commit()?;
        Ok(result)
    })?;
    snapshot.generation = generation;
    Ok((snapshot, due))
}

pub fn read_conn(
    c: &Connection,
    options: Options,
) -> adoken_store::Result<(Snapshot, Vec<PinSubject>)> {
    let (snapshot, due) = read_conn_candidates(c, options)?;
    Ok((snapshot, due.into_iter().map(|pin| pin.subject).collect()))
}

pub(super) fn read_conn_candidates(
    c: &Connection,
    options: Options,
) -> adoken_store::Result<(Snapshot, Vec<Pin>)> {
    let now = options.now;
    let pins = read::pins(c)?;
    let mut snapshot = Snapshot {
        opens_this_week: read::pin_opens_since(c, now - 7 * adoken_model::time::DAY)?,
        ..Snapshot::default()
    };
    if pins.is_empty() {
        return Ok((snapshot, Vec::new()));
    }
    let demo = read::meta(c, "demo")?.is_some();
    let org_url = read::org(c)?.map(|org| org.url).filter(|_| !demo);
    let names = Names::read(c, org_url.is_some())?;
    let stale_since = options.stale_after.and_then(|after| {
        read::last_sync(c)
            .ok()
            .flatten()
            .filter(|at| now - at > after)
    });
    let mut due = Vec::new();
    for pin in pins {
        let view = view(c, pin, now, stale_since, org_url.as_deref(), &names)?;
        if status::auto_unpin_due(&view.headline, view.closed_at, now, options.auto_unpin) {
            due.push(view.pin.clone());
            continue;
        }
        snapshot.pins.push(view);
    }
    Ok((snapshot, due))
}

/// Project and repo names for web links.
#[derive(Default)]
struct Names {
    projects: HashMap<String, String>,
    repos: HashMap<String, String>,
}

impl Names {
    fn read(c: &Connection, wanted: bool) -> adoken_store::Result<Self> {
        if !wanted {
            return Ok(Names::default());
        }
        Ok(Names {
            projects: read::projects(c)?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect(),
            repos: read::repos(c)?
                .into_iter()
                .map(|r| (r.id, r.name))
                .collect(),
        })
    }
}

fn web_path(org: &str, segments: &[&str], query: Option<(&str, &str)>) -> Option<String> {
    let mut url = url::Url::parse(org).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(segments);
    if let Some((k, v)) = query {
        url.query_pairs_mut().append_pair(k, v);
    }
    Some(url.into())
}

fn view(
    c: &Connection,
    pin: Pin,
    _now: Millis,
    stale_since: Option<Millis>,
    org_url: Option<&str>,
    names: &Names,
) -> adoken_store::Result<PinView> {
    let mut v = PinView::provisional(pin);
    v.title = v.pin.title.clone();
    let project_name = |id: &str| names.projects.get(id).map_or(id, String::as_str).to_owned();
    match v.pin.subject.clone() {
        PinSubject::Pr(id) => {
            if let Some(pr) = read::pr(c, id)? {
                let runs = read::runs(
                    c,
                    &RunFilter {
                        pr_id: Some(id),
                        ..RunFilter::default()
                    },
                )?;
                let ci = crate::review::data::checks(&latest_per_pipeline(runs))
                    .get(&id)
                    .copied();
                v.kind = status::Kind::Pr {
                    draft: pr.is_draft && pr.status == PrStatus::Active,
                };
                v.headline = Headline::Pr(status::pr_headline(&pr, ci));
                v.closed_at = pr.closed;
                v.web_url = org_url.and_then(|org| {
                    let project = project_name(&pr.project_id);
                    let repo = names.repos.get(&pr.repo_id)?;
                    crate::review::web::web_url(org, &project, repo, pr.id)
                });
                v.title = pr.title;
            }
            if let Some(subject) = v.pin.subject.event_subject() {
                v.changed = read::changed_since(c, &subject, v.pin.seen_at())?;
            }
        }
        PinSubject::WorkItem(id) => {
            if let Some(wi) = read::work_item(c, id)? {
                v.kind = Kind::WorkItem(Some(wi.kind.clone()));
                v.closed_at = if matches!(
                    wi.state_category,
                    StateCategory::Completed | StateCategory::Removed
                ) {
                    read::work_item_closed_at(c, id)?
                } else {
                    None
                };
                v.headline = Headline::WorkItem {
                    state: wi.state,
                    category: wi.state_category,
                };
                v.web_url = org_url.and_then(|org| {
                    web_path(
                        org,
                        &[
                            &project_name(&wi.project_id),
                            "_workitems",
                            "edit",
                            &id.to_string(),
                        ],
                        None,
                    )
                });
                v.title = wi.title;
            }
            if let Some(subject) = v.pin.subject.event_subject() {
                v.changed = read::changed_since(c, &subject, v.pin.seen_at())?;
            }
        }
        PinSubject::Definition { project_id, id } => {
            let runs = read::definition_runs(c, &project_id, id, DEFINITION_RUNS)?;
            let pipeline = read::pipeline(c, &project_id, id)?;
            v.headline = Headline::Definition(status::definition_headline(runs.first()));
            v.latest_result = runs
                .iter()
                .filter(|r| r.status == RunStatus::Completed)
                .filter_map(|r| r.result)
                .find(|r| *r != RunResult::Canceled);
            v.changed = status::definition_changed(v.pin.seen_result, v.latest_result);
            let name = pipeline
                .map(|p| p.name)
                .or_else(|| runs.first().map(|r| r.pipeline_name.clone()));
            if let Some(name) = name {
                v.title = name;
            } else if v.title.is_empty() {
                v.headline = Headline::Fetching;
            }
            v.web_url = org_url.and_then(|org| {
                web_path(
                    org,
                    &[&project_name(&project_id), "_build"],
                    Some(("definitionId", &id.to_string())),
                )
            });
        }
    }
    if let Some(why) = v.pin.unavailable {
        v.headline = Headline::NotAvailable(why);
        v.changed = false;
    } else if !matches!(v.headline, Headline::Fetching) {
        v.stale_since = stale_since;
    }
    Ok(v)
}

/// Keep each pipeline's newest run (the PR list's CI rule).
fn latest_per_pipeline(runs: Vec<PipelineRun>) -> Vec<PipelineRun> {
    let mut latest: HashMap<i64, PipelineRun> = HashMap::new();
    for run in runs {
        match latest.get(&run.pipeline_id) {
            Some(kept) if (kept.queued, kept.id) >= (run.queued, run.id) => {}
            _ => {
                latest.insert(run.pipeline_id, run);
            }
        }
    }
    latest.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::time::HOUR;
    use adoken_store::write;

    fn options(now: Millis) -> Options {
        Options {
            now,
            auto_unpin: true,
            stale_after: Some(STALE_AFTER),
        }
    }

    fn pin(subject: PinSubject, at: Millis) -> Pin {
        Pin {
            subject,
            position: 0,
            label: None,
            title: String::new(),
            pinned_at: at,
            opened_at: None,
            seen_result: None,
            unavailable: None,
        }
    }

    fn seeded() -> (tempfile::TempDir, Store, adoken_demo::Dataset) {
        let dir = tempfile::tempdir().unwrap();
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        adoken_demo::seed(&store, &ds).unwrap();
        store
            .write(|connection| {
                for pin in adoken_store::read::pins(connection)? {
                    write::unpin(connection, &pin.subject)?;
                }
                Ok(())
            })
            .unwrap();
        (dir, store, ds)
    }

    /// AC-7 (PR half): a non-self-caused comment after the open lights the
    /// dot, my own vote doesn't, and opening clears it.
    #[test]
    fn pr_dot_follows_other_peoples_events_since_the_open() {
        let (_d, store, ds) = seeded();
        let t = ds.now + HOUR;
        let pr = ds
            .prs
            .iter()
            .find(|p| p.status == PrStatus::Active)
            .unwrap()
            .id;
        let subject = PinSubject::Pr(pr);
        store
            .write(|tx| write::add_pin(tx, &pin(subject.clone(), t)))
            .unwrap();
        let event = |kind, key: &str, at, self_caused| adoken_model::SyncEvent {
            id: 0,
            kind,
            subject: adoken_model::Subject::Pr(pr),
            dedupe_key: key.into(),
            actor: None,
            self_caused,
            at,
            summary: String::new(),
        };
        let dot = || read(&store, options(t + 10)).unwrap().0.pins[0].changed;
        assert!(!dot());
        store
            .write(|tx| {
                write::insert_event(
                    tx,
                    &event(adoken_model::SyncEventKind::PrVoteChanged, "v", t + 2, true),
                )
            })
            .unwrap();
        assert!(!dot(), "my own vote doesn't light it");
        store
            .write(|tx| {
                write::insert_event(
                    tx,
                    &event(
                        adoken_model::SyncEventKind::PrCommentAdded,
                        "c",
                        t + 1,
                        false,
                    ),
                )
            })
            .unwrap();
        assert!(dot(), "a teammate's comment at T+1 lights it");
        store
            .write(|tx| write::mark_pin_opened(tx, &subject, t + 5, None, true))
            .unwrap();
        assert!(!dot(), "opening clears it");
    }

    /// AC-8 through the snapshot: a completed PR is listed at T+23h59m and
    /// due at T+24h; with auto-unpin off it stays.
    #[test]
    fn closed_prs_are_due_for_auto_unpin_after_a_day() {
        let (_d, store, ds) = seeded();
        let mut pr = ds
            .prs
            .iter()
            .find(|p| p.status == PrStatus::Active)
            .unwrap()
            .clone();
        let t = ds.now;
        pr.status = PrStatus::Completed;
        pr.closed = Some(t);
        pr.updated = t + 1;
        store
            .write(|tx| {
                write::upsert_pr(tx, &pr)?;
                write::add_pin(tx, &pin(PinSubject::Pr(pr.id), t - HOUR))
            })
            .unwrap();
        let at = |now, auto_unpin| {
            read(
                &store,
                Options {
                    auto_unpin,
                    ..options(now)
                },
            )
            .unwrap()
        };
        let (listed, due) = at(t + 24 * HOUR - MINUTE, true);
        assert_eq!(listed.pins.len(), 1);
        assert!(due.is_empty());
        assert_eq!(listed.pins[0].headline.word(t), "Completed");
        let (gone, due) = at(t + 24 * HOUR, true);
        assert!(gone.pins.is_empty());
        assert_eq!(due, vec![PinSubject::Pr(pr.id)]);
        let (kept, due) = at(t + 72 * HOUR, false);
        assert_eq!(kept.pins.len(), 1);
        assert!(due.is_empty());
    }

    /// AC-16: pins ignore the project filter; the snapshot has no project
    /// input at all and lists pins from every project.
    #[test]
    fn pins_from_every_project_are_listed() {
        let (_d, store, ds) = seeded();
        let by_project: Vec<i64> = ds
            .projects
            .iter()
            .filter_map(|p| {
                ds.prs
                    .iter()
                    .find(|pr| pr.project_id == p.id && pr.status == PrStatus::Active)
                    .map(|pr| pr.id)
            })
            .collect();
        assert!(by_project.len() > 1);
        store
            .write(|tx| {
                for id in &by_project {
                    write::add_pin(tx, &pin(PinSubject::Pr(*id), ds.now))?;
                }
                Ok(())
            })
            .unwrap();
        let (snapshot, _) = read(&store, options(ds.now)).unwrap();
        assert_eq!(snapshot.pins.len(), by_project.len());
    }

    #[test]
    fn stale_after_three_warm_intervals_and_never_in_demo_mode() {
        let (_d, store, ds) = seeded();
        let def = &ds.pipelines[0];
        store
            .write(|tx| {
                write::add_pin(
                    tx,
                    &pin(
                        PinSubject::Definition {
                            project_id: def.project_id.clone(),
                            id: def.id,
                        },
                        ds.now,
                    ),
                )
            })
            .unwrap();
        let last = store.last_sync().unwrap().unwrap();
        let view = |now, stale_after| {
            read(
                &store,
                Options {
                    stale_after,
                    ..options(now)
                },
            )
            .unwrap()
            .0
            .pins[0]
                .clone()
        };
        assert_eq!(
            view(last + STALE_AFTER, Some(STALE_AFTER)).stale_since,
            None
        );
        assert_eq!(
            view(last + STALE_AFTER + 1, Some(STALE_AFTER)).stale_since,
            Some(last)
        );
        assert_eq!(view(last + 10 * HOUR, None).stale_since, None);
        assert_eq!(view(last, None).title, def.name);
    }
}
