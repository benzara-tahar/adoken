//! Store → engine input: enrich sync events with the facts rules match on,
//! drain the nudge queue, and compute the first-run "needs you" baseline
//! (spec 0005 States: first sync). Blocking: background executor only.

use std::collections::{HashMap, HashSet};

use adoken_model::time::DAY;
use adoken_model::{
    MentionSource, Millis, PersonId, PinSubject, PrDetail, PrStatus, RunResult, RunStatus, Subject,
    SyncEvent, SyncEventKind, Vote, WorkItem, short_branch,
};
use adoken_store::{PrFilter, Result, RunFilter, Store, WorkItemFilter};

use crate::classify::EventContext;
use crate::engine::Incoming;
use crate::persist::{self, Progress, QueueOp};

/// Events read per poll.
const BATCH: usize = 500;

/// Name lookups cached per poll (people, repos, pipelines), and the pins
/// (spec 0014 FR-16).
pub struct Enricher {
    me: Option<PersonId>,
    people: HashMap<PersonId, String>,
    repos: HashMap<String, String>,
    pinned_definitions: HashSet<(String, i64)>,
}

impl Enricher {
    pub fn load(store: &Store) -> Result<Self> {
        let mut pinned_definitions = HashSet::new();
        for pin in store.pins()? {
            match pin.subject {
                PinSubject::Pr(_) | PinSubject::WorkItem(_) => {}
                PinSubject::Definition { project_id, id } => {
                    pinned_definitions.insert((project_id, id));
                }
            }
        }
        Ok(Self {
            me: store.me()?.map(|p| p.id),
            people: store
                .people()?
                .into_iter()
                .map(|p| (p.id, p.display_name))
                .collect(),
            repos: store.repos()?.into_iter().map(|r| (r.id, r.name)).collect(),
            pinned_definitions,
        })
    }

    pub fn me(&self) -> Option<&str> {
        self.me.as_deref()
    }

    fn is_me(&self, who: &str) -> bool {
        self.me.as_deref() == Some(who)
    }

    pub fn person_name(&self, id: &str) -> Option<String> {
        self.people.get(id).cloned()
    }

    /// Facts for one event (reads the subject from the store).
    pub fn context(&self, store: &Store, event: &SyncEvent) -> Result<EventContext> {
        let mut ctx = EventContext {
            actor_name: event.actor.as_deref().and_then(|a| self.person_name(a)),
            ..EventContext::default()
        };
        match &event.subject {
            Subject::Pr(id) => {
                ctx.title = format!("!{id}");
                let detail = store.pr(*id)?;
                if let Some(detail) = &detail {
                    let pr = &detail.pr;
                    ctx.title = format!("!{id} {}", pr.title);
                    ctx.project = Some(pr.project_id.clone());
                    ctx.repo_name = self.repos.get(&pr.repo_id).cloned();
                    ctx.repo_id = Some(pr.repo_id.clone());
                    ctx.branch = Some(short_branch(&pr.target_branch).to_owned());
                    ctx.mine = self.is_me(&pr.author);
                    ctx.required_reviewer = pr
                        .reviewers
                        .iter()
                        .any(|r| self.is_me(&r.person) && r.is_required);
                    if event.kind == SyncEventKind::PrVoteChanged {
                        ctx.vote = event
                            .actor
                            .as_deref()
                            .and_then(|a| pr.vote_of(a))
                            .or_else(|| vote_from_key(&event.dedupe_key));
                    }
                    // Spec 0004 FR-12: a push past the commit I reviewed when
                    // I set auto-complete in adoken.
                    if event.kind == SyncEventKind::PrPushed {
                        ctx.auto_complete_mine = store
                            .auto_complete(*id)?
                            .is_some_and(|(reviewed, _)| reviewed != event.dedupe_key);
                    }
                }
                ctx.pin_only = !self.pr_relates_to_me(store, event, detail.as_ref())?;
            }
            Subject::WorkItem(id) => {
                ctx.title = format!("#{id}");
                let wi = store.work_item(*id)?;
                if let Some(wi) = &wi {
                    ctx.title = format!("#{id} {}", wi.title);
                    ctx.project = Some(wi.project_id.clone());
                    ctx.work_item_type = Some(wi.kind.as_str().to_owned());
                    ctx.mine = wi.assigned_to.as_deref().is_some_and(|a| self.is_me(a));
                }
                ctx.pin_only = !self.work_item_relates_to_me(event, wi.as_ref());
            }
            Subject::Build(id) => {
                ctx.title = format!("Run {id}");
                if let Some(run) = store.run(*id)? {
                    ctx.title = format!("{} {}", run.pipeline_name, run.run_number);
                    ctx.project = Some(run.project_id.clone());
                    ctx.pipeline_id = Some(run.pipeline_id);
                    ctx.pipeline_name = Some(run.pipeline_name.clone());
                    ctx.branch = Some(short_branch(&run.branch).to_owned());
                    ctx.build_result = run.result;
                    let requested_by_me =
                        run.requested_for.as_deref().is_some_and(|p| self.is_me(p));
                    let for_my_pr = match run.pr_id {
                        Some(pr) => store.pr(pr)?.is_some_and(|d| self.is_me(&d.pr.author)),
                        None => false,
                    };
                    ctx.mine = requested_by_me || for_my_pr;
                    ctx.pinned = self
                        .pinned_definitions
                        .contains(&(run.project_id.clone(), run.pipeline_id));
                }
            }
            Subject::Account => ctx.title = "Azure DevOps token".into(),
        }
        Ok(ctx)
    }

    /// Whether this PR event has the relationship that made sync emit its
    /// kind before pins (0002 FR-11). Pin-only activity must never notify,
    /// including when the pin was removed before this poll (0014 FR-16).
    fn pr_relates_to_me(
        &self,
        store: &Store,
        event: &SyncEvent,
        detail: Option<&PrDetail>,
    ) -> Result<bool> {
        use SyncEventKind::*;
        // Addressed to me whatever the PR.
        if matches!(
            event.kind,
            ReviewRequested | MentionedInPr | PrThreadResolved
        ) {
            return Ok(true);
        }
        let Some(detail) = detail else {
            return Ok(false);
        };
        let pr = &detail.pr;
        let author = self.is_me(&pr.author);
        let reviewing = pr.reviewers.iter().any(|r| self.is_me(&r.person));
        Ok(match event.kind {
            PrVoteChanged | PrMergeConflict => author,
            PrPushed => reviewing || store.auto_complete(pr.id)?.is_some(),
            // Self-caused: I closed it, or Azure DevOps finished the
            // Complete I queued in adoken (its marker is gone by now).
            PrStatusChanged => {
                author || reviewing || event.self_caused || store.completing(pr.id)?.is_some()
            }
            // Key `thread:comment`: also a thread I commented in.
            PrCommentAdded => {
                author
                    || reviewing
                    || event
                        .dedupe_key
                        .split(':')
                        .next()
                        .and_then(|t| t.parse::<i64>().ok())
                        .and_then(|t| detail.threads.iter().find(|x| x.id == t))
                        .is_some_and(|t| t.comments.iter().any(|c| self.is_me(&c.author)))
            }
            _ => true,
        })
    }

    /// [`Self::pr_relates_to_me`] for a pinned work item.
    fn work_item_relates_to_me(&self, event: &SyncEvent, wi: Option<&WorkItem>) -> bool {
        match event.kind {
            SyncEventKind::WorkItemChanged => wi.is_some_and(|wi| {
                wi.assigned_to.as_deref().is_some_and(|a| self.is_me(a))
                    || self.is_me(&wi.created_by)
            }),
            _ => true,
        }
    }
}

/// `PrVoteChanged` dedupe key is "reviewer id + vote" (0002 FR-11); used
/// when the store's current vote has moved on.
fn vote_from_key(key: &str) -> Option<Vote> {
    let value: i8 = key.rsplit([':', '|', ' ']).next()?.parse().ok()?;
    Some(Vote::from_i8(value))
}

/// One poll's worth of input.
#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub incoming: Vec<Incoming>,
    /// Nudge rows whose condition cleared.
    pub resolved: Vec<Subject>,
    pub progress: Progress,
    /// More events are waiting (poll again right away).
    pub more: bool,
}

/// Reads new sync events after `cursor` and pending queue ops. Name lookups
/// load only when there is something to enrich (idle polls cost two
/// indexed reads).
pub fn fetch(store: &Store, cursor: i64) -> Result<Batch> {
    let events = store.sync_events_since(cursor, BATCH)?;
    let mut batch = Batch {
        more: events.len() == BATCH,
        ..Batch::default()
    };
    if let Some(last) = events.last() {
        batch.progress.cursor = Some(last.id);
        let enricher = Enricher::load(store)?;
        for event in &events {
            let ctx = enricher.context(store, event)?;
            batch.incoming.push(Incoming::from_sync(event, ctx));
        }
    }
    let ops = persist::pending(store)?;
    batch.progress.queue_upto = ops.last().map(|(seq, _)| *seq);
    for (_, op) in ops {
        match op {
            QueueOp::Nudge {
                key,
                subject,
                title,
                message,
                at,
            } => batch.incoming.push(Incoming::nudge(
                &key,
                subject,
                at,
                message,
                EventContext {
                    title,
                    ..EventContext::default()
                },
            )),
            QueueOp::Resolve { subject, .. } => batch.resolved.push(subject),
        }
    }
    Ok(batch)
}

/// First run: what currently needs me, computed from state rather than a
/// flood of historic events. Ingest these without interruptions.
pub fn baseline(store: &Store, enricher: &Enricher, now: Millis) -> Result<Vec<Incoming>> {
    let Some(me) = enricher.me.clone() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut push = |key: String,
                    kind: SyncEventKind,
                    subject: Subject,
                    actor: Option<PersonId>,
                    at: Millis,
                    summary: String|
     -> Result<()> {
        let event = SyncEvent {
            id: 0,
            kind,
            subject,
            dedupe_key: key,
            actor,
            self_caused: false,
            at,
            summary,
        };
        let ctx = enricher.context(store, &event)?;
        let mut incoming = Incoming::from_sync(&event, ctx);
        incoming.dedupe_key = format!("baseline|{}", incoming.dedupe_key);
        out.push(incoming);
        Ok(())
    };

    // Reviews waiting on me.
    let waiting = store.prs(&PrFilter {
        status: Some(PrStatus::Active),
        reviewer: Some(me.clone()),
        limit: Some(50),
        ..PrFilter::default()
    })?;
    for pr in waiting
        .iter()
        .filter(|pr| pr.vote_of(&me) == Some(Vote::NoVote) && !pr.is_draft)
    {
        let added = pr
            .reviewers
            .iter()
            .find(|r| r.person == me)
            .and_then(|r| r.added_at)
            .unwrap_or(pr.updated);
        let author = enricher
            .person_name(&pr.author)
            .unwrap_or_else(|| "Someone".into());
        push(
            format!("review:{}", pr.id),
            SyncEventKind::ReviewRequested,
            Subject::Pr(pr.id),
            Some(pr.author.clone()),
            added,
            format!("{author} requested your review"),
        )?;
    }

    // Recent mentions.
    for mention in store.mentions(&me, 30)? {
        if now - mention.when > 7 * DAY {
            continue;
        }
        let (kind, key) = match &mention.source {
            MentionSource::Pr { comment_id, .. } => (SyncEventKind::MentionedInPr, comment_id),
            MentionSource::WorkItem { comment_id, .. } => {
                (SyncEventKind::MentionedInWorkItem, comment_id)
            }
        };
        let who = enricher
            .person_name(&mention.who)
            .unwrap_or_else(|| "Someone".into());
        push(
            format!("mention:{key}"),
            kind,
            mention.source.subject(),
            Some(mention.who.clone()),
            mention.when,
            format!("{who}: {}", mention.snippet),
        )?;
    }

    // My recently failed runs.
    let runs = store.runs(&RunFilter {
        requested_for: Some(me.clone()),
        status: Some(RunStatus::Completed),
        limit: Some(40),
        ..RunFilter::default()
    })?;
    for run in runs {
        let finished = run.finish.unwrap_or(run.queued);
        if run.result != Some(RunResult::Failed) || now - finished > 2 * DAY {
            continue;
        }
        push(
            format!("run:{}", run.id),
            SyncEventKind::BuildCompleted,
            Subject::Build(run.id),
            None,
            finished,
            format!(
                "{} failed on {}",
                run.pipeline_name,
                short_branch(&run.branch)
            ),
        )?;
    }

    // Work recently assigned to me.
    let assigned = store.work_items(&WorkItemFilter {
        assigned_to: Some(me.clone()),
        open_only: true,
        changed_since: Some(now - 3 * DAY),
        limit: Some(15),
        ..WorkItemFilter::default()
    })?;
    for wi in assigned {
        let by = enricher
            .person_name(&wi.changed_by)
            .unwrap_or_else(|| "Someone".into());
        push(
            format!("wi:{}:{}", wi.id, wi.rev),
            SyncEventKind::WorkItemAssigned,
            Subject::WorkItem(wi.id),
            Some(wi.changed_by.clone()),
            wi.changed,
            format!("{by} assigned {} #{} to you", wi.kind.as_str(), wi.id),
        )?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vote_parses_from_the_dedupe_key() {
        assert_eq!(vote_from_key("abc-123:-10"), Some(Vote::Rejected));
        assert_eq!(vote_from_key("abc 10"), Some(Vote::Approved));
        assert_eq!(vote_from_key("nonsense"), None);
    }

    /// Spec 0014 AC-14 (notify side): someone else's failed run is P2
    /// "Build failed (pinned pipeline)" while its definition is pinned, and
    /// no longer once it's unpinned.
    #[test]
    fn failed_runs_of_pinned_definitions_classify_as_pinned_failures() {
        use crate::classify::{PriorityTable, RuleKey};
        use adoken_model::{Person, Pin, PipelineRun, Project, RunReason, RunResult};
        use adoken_store::write;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        persist::migrate(&store).unwrap();
        let person = |id: &str| Person {
            id: id.into(),
            display_name: id.into(),
            unique_name: None,
            is_group: false,
            image_url: None,
        };
        let definition = PinSubject::Definition {
            project_id: "p1".into(),
            id: 7,
        };
        store
            .write(|tx| {
                write::upsert_person(tx, &person("me"), true)?;
                write::upsert_person(tx, &person("priya"), false)?;
                write::upsert_project(
                    tx,
                    &Project {
                        id: "p1".into(),
                        name: "Payments".into(),
                    },
                )?;
                write::upsert_run(
                    tx,
                    &PipelineRun {
                        id: 55,
                        project_id: "p1".into(),
                        pipeline_id: 7,
                        pipeline_name: "payments-ci".into(),
                        run_number: "1".into(),
                        status: RunStatus::Completed,
                        result: Some(RunResult::Failed),
                        reason: RunReason::IndividualCi,
                        branch: "refs/heads/main".into(),
                        source_version: None,
                        requested_for: Some("priya".into()),
                        pr_id: None,
                        queued: 1,
                        started: Some(2),
                        finish: Some(3),
                    },
                )?;
                write::insert_event(
                    tx,
                    &SyncEvent {
                        id: 0,
                        kind: SyncEventKind::BuildCompleted,
                        subject: Subject::Build(55),
                        dedupe_key: "55".into(),
                        actor: Some("priya".into()),
                        self_caused: false,
                        at: 3,
                        summary: "payments-ci #1 failed".into(),
                    },
                )?;
                write::add_pin(
                    tx,
                    &Pin {
                        subject: definition.clone(),
                        position: 0,
                        label: None,
                        title: "payments-ci".into(),
                        pinned_at: 0,
                        opened_at: None,
                        seen_result: None,
                        unavailable: None,
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let rule = |store: &Store| {
            let batch = fetch(store, 0).unwrap();
            let incoming = &batch.incoming[0];
            RuleKey::of(incoming.kind, &incoming.ctx)
        };
        assert_eq!(rule(&store), RuleKey::BuildFailedOther);
        assert_eq!(
            PriorityTable::default().get(rule(&store)),
            crate::classify::Priority::P2
        );
        store.write(|tx| write::unpin(tx, &definition)).unwrap();
        assert_eq!(rule(&store), RuleKey::BuildFinished);
    }
}
