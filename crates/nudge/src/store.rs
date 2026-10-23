//! Store adapter: reads a [`Snapshot`] from the local store and persists the
//! [`Ledger`] in the `nudge_*` local tables (ADR 0001 "local" tables;
//! migrations must preserve them). Every call blocks: run it on a background
//! executor.

use std::collections::HashSet;

use adoken_model::{Millis, PrStatus, StateCategory, Subject};
use adoken_store::rusqlite::{self, params};
use adoken_store::{PrFilter, Result, RunFilter, Store, WorkItemFilter, read};

use crate::engine::{Ledger, NudgeSink, SubjectState};
use crate::kind::NudgeKind;
use crate::rules::Nudge;
use crate::snapshot::{DataHealth, Snapshot};

/// Forward-only; shipped steps never change.
const MIGRATIONS: &[&str] = &["
CREATE TABLE nudge_subject (
  subject          TEXT PRIMARY KEY,           -- 'pr:912', 'wi:48213'
  last_fired_ms    INTEGER,
  open             TEXT NOT NULL DEFAULT '',   -- comma-separated N-ids fired and unresolved
  snoozed_until_ms INTEGER,
  dismissed        TEXT NOT NULL DEFAULT '',   -- comma-separated N-ids marked done
  opted_out        INTEGER NOT NULL DEFAULT 0  -- 'Not for this item'
) STRICT, WITHOUT ROWID;
CREATE TABLE nudge_fired (at_ms INTEGER NOT NULL) STRICT;
CREATE TABLE nudge_meta (key TEXT PRIMARY KEY, value INTEGER) STRICT, WITHOUT ROWID;
"];

pub fn migrate(store: &Store) -> Result<()> {
    store.migrate_ext("nudge", MIGRATIONS)
}

/// Everything the rules read, in one read transaction's worth of indexed
/// queries, plus Today's context for the local day that began at
/// `day_start`. `None` until "me" is known (before onboarding finishes).
pub fn load_snapshot(
    store: &Store,
    health: DataHealth,
    day_start: Millis,
) -> Result<Option<Snapshot>> {
    store.with_conn(|c| {
        let Some(me) = read::me(c)? else {
            return Ok(None);
        };
        let mut snap = Snapshot::empty(me.id.clone());
        snap.health = health;
        for p in read::people(c)? {
            if p.is_group {
                snap.groups.insert(p.id.clone());
            }
            snap.names.insert(p.id, p.display_name);
        }
        snap.prs = read::prs(
            c,
            &PrFilter {
                status: Some(PrStatus::Active),
                involving: Some(me.id.clone()),
                ..PrFilter::default()
            },
        )?;
        snap.work_items = read::work_items(
            c,
            &WorkItemFilter {
                assigned_to: Some(me.id.clone()),
                state_category: Some(StateCategory::InProgress),
                ..WorkItemFilter::default()
            },
        )?;

        let involved: Vec<i64> = snap.prs.iter().map(|pr| pr.id).collect();
        snap.threads = read::pr_threads_batch(c, &involved)?.into_iter().collect();
        for pr in snap.prs.iter().filter(|pr| pr.author == me.id) {
            snap.runs.extend(read::runs(
                c,
                &RunFilter {
                    pr_id: Some(pr.id),
                    limit: Some(1),
                    ..RunFilter::default()
                },
            )?);
        }

        let mut known: HashSet<i64> = involved.into_iter().collect();
        let linked: Vec<i64> = snap
            .work_items
            .iter()
            .flat_map(|wi| wi.linked_pr_ids.iter().copied())
            .filter(|id| known.insert(*id))
            .collect();
        for id in linked {
            snap.prs.extend(read::pr(c, id)?);
        }

        snap.merged_today = read::prs(
            c,
            &PrFilter {
                status: Some(PrStatus::Completed),
                involving: Some(me.id.clone()),
                updated_since: Some(day_start),
                ..PrFilter::default()
            },
        )?
        .into_iter()
        .filter(|pr| pr.closed.is_some_and(|t| t >= day_start))
        .collect();
        day_reads(c, &mut snap, day_start)?;
        for pr in snap.prs.iter().filter(|pr| pr.status == PrStatus::Active) {
            let files = read::pr_file_count(c, pr.id)?;
            if files > 0 {
                snap.file_counts.insert(pr.id, files);
            }
        }
        Ok(Some(snap))
    })
}

/// Spec 0017's reads on top of the nudge snapshot (its §7 budget): today's
/// pushes and the stored head of my PRs, the comments on my merges today,
/// work items done today, and how long my loose ends have been in their
/// state. Fills those fields of `snap`, whose PRs, merges and work items
/// are already read. Public so the bench can time them alone.
pub fn load_day(store: &Store, snap: &mut Snapshot, day_start: Millis) -> Result<()> {
    store.with_conn(|c| day_reads(c, snap, day_start))
}

fn day_reads(c: &rusqlite::Connection, snap: &mut Snapshot, day_start: Millis) -> Result<()> {
    let me = snap.me.clone();
    let merged_mine: Vec<i64> = snap
        .merged_today
        .iter()
        .filter(|pr| pr.author == me)
        .map(|pr| pr.id)
        .collect();
    snap.threads
        .extend(read::pr_threads_batch(c, &merged_mine)?);
    let mine = snap
        .prs
        .iter()
        .filter(|pr| pr.status == PrStatus::Active)
        .chain(&snap.merged_today)
        .filter(|pr| pr.author == me);
    for pr in mine {
        let (today, listed) = read::pr_iterations_since(c, pr.id, day_start)?;
        if !today.is_empty() {
            snap.iterations_today.insert(pr.id, today);
        }
        if let Some(head) = pr.last_source_commit.as_deref()
            && listed.is_none_or(|it| !it.eq_ignore_ascii_case(head))
        {
            snap.iterations_behind.insert(pr.id);
        }
    }
    snap.done_today = read::work_items_completed_since(c, &me, day_start)?;
    let open: HashSet<i64> = snap.work_items.iter().map(|wi| wi.id).collect();
    snap.state_since.extend(
        read::work_items_state_since(c, &me, StateCategory::InProgress)?
            .into_iter()
            .filter(|(id, _)| open.contains(id)),
    );
    Ok(())
}

fn kinds_to_text(kinds: &[NudgeKind]) -> String {
    kinds.iter().map(|k| k.id()).collect::<Vec<_>>().join(",")
}

fn kinds_from_text(text: &str) -> Vec<NudgeKind> {
    text.split(',').filter_map(NudgeKind::parse).collect()
}

const TODAY_DISMISSED: &str = "today_dismissed_day";

pub fn load_ledger(store: &Store) -> Result<Ledger> {
    store.with_conn(|c| {
        let subjects = c
            .prepare_cached(
                "SELECT subject, last_fired_ms, open, snoozed_until_ms, dismissed, opted_out
                 FROM nudge_subject",
            )?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    SubjectState {
                        last_fired: r.get(1)?,
                        open: kinds_from_text(&r.get::<_, String>(2)?),
                        snoozed_until: r.get(3)?,
                        dismissed: kinds_from_text(&r.get::<_, String>(4)?),
                        opted_out: r.get(5)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let fired = c
            .prepare_cached("SELECT at_ms FROM nudge_fired ORDER BY at_ms")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let today: Option<i64> = c
            .prepare_cached("SELECT value FROM nudge_meta WHERE key = ?1")?
            .query_row([TODAY_DISMISSED], |r| r.get(0))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        Ok(Ledger::from_parts(subjects, fired, today))
    })
}

/// Replaces the persisted ledger in one transaction (it is small: pruned to
/// live subjects and two days of fire times).
pub fn save_ledger(store: &Store, ledger: &Ledger) -> Result<()> {
    store.write(|tx| {
        tx.execute_batch("DELETE FROM nudge_subject; DELETE FROM nudge_fired;")?;
        let mut insert = tx.prepare_cached(
            "INSERT INTO nudge_subject
               (subject, last_fired_ms, open, snoozed_until_ms, dismissed, opted_out)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for (subject, s) in ledger.subjects() {
            insert.execute(params![
                subject,
                s.last_fired,
                kinds_to_text(&s.open),
                s.snoozed_until,
                kinds_to_text(&s.dismissed),
                s.opted_out,
            ])?;
        }
        let mut fired = tx.prepare_cached("INSERT INTO nudge_fired (at_ms) VALUES (?1)")?;
        for at in ledger.fired() {
            fired.execute([at])?;
        }
        tx.execute(
            "INSERT INTO nudge_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![TODAY_DISMISSED, ledger.today_dismissed_day()],
        )?;
        Ok(())
    })
}

/// Delivers nudges to the notification center (spec 0005) through its local
/// queue: P2, source Nudges, deduplicated by [`Nudge::key`]. Local writes
/// only. Operations are buffered because [`NudgeSink`] can't fail; call
/// [`InboxSink::finish`] to write them.
pub struct InboxSink<'a> {
    store: &'a Store,
    ops: Vec<InboxOp>,
}

enum InboxOp {
    Deliver {
        key: String,
        subject: Subject,
        title: String,
        message: String,
        at: Millis,
    },
    Resolve {
        subject: Subject,
        at: Millis,
    },
}

impl<'a> InboxSink<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self {
            store,
            ops: Vec::new(),
        }
    }

    pub fn finish(self) -> Result<()> {
        for op in self.ops {
            match op {
                InboxOp::Deliver {
                    key,
                    subject,
                    title,
                    message,
                    at,
                } => adoken_notify::persist::submit_nudge(
                    self.store, &key, &subject, &title, &message, at,
                )?,
                InboxOp::Resolve { subject, at } => {
                    adoken_notify::persist::resolve_nudge(self.store, &subject, at)?
                }
            }
        }
        Ok(())
    }
}

impl NudgeSink for InboxSink<'_> {
    fn deliver(&mut self, nudge: &Nudge, at: Millis) {
        let message = match &nudge.detail {
            Some(detail) => format!("{} · {detail}", nudge.message),
            None => nudge.message.clone(),
        };
        self.ops.push(InboxOp::Deliver {
            key: nudge.key(),
            subject: nudge.subject.clone(),
            title: nudge.label.clone(),
            message,
            at,
        });
    }

    fn resolve(&mut self, subject: &Subject, _kind: NudgeKind, at: Millis) {
        self.ops.push(InboxOp::Resolve {
            subject: subject.clone(),
            at,
        });
    }
}
