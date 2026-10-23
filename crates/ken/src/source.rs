//! Store → [`Action`]s (blocking IO: background executor only).
//!
//! Inputs, all local:
//! - `sync_event` rows after the Ken cursor. My own review actions arrive as
//!   self-caused events (in demo mode, from the demo event stream): comment
//!   → Quick Jab, vote → by value, resolve → Clean Cut, completion → Merge
//!   Wave. Completions of my PRs are K.O.s (of PRs I approved, Assists);
//!   finished runs of my PRs feed Comeback.
//! - Viewed marks from the review view's local `review_viewed` table, each
//!   (PR, file, version) scored once via `ken_viewed`, so toggling a file
//!   back and forth counts once.
//!
//! Nothing here writes outside the `ken_*` tables.

use std::collections::HashMap;

use adoken_model::{
    Millis, PersonId, PrDetail, RunResult, Subject, SyncEvent, SyncEventKind, Vote,
};
use adoken_store::rusqlite::{self, OptionalExtension};
use adoken_store::{Store, read};

use crate::engine::{Action, Origin, PrFacts};
use crate::persist;

/// The review view's viewed-file table (`crates/app/src/review/data.rs`).
const REVIEW_VIEWED_TABLE: &str = "review_viewed";
/// Events read per poll.
const EVENT_BATCH: usize = 500;

/// One poll's worth of input.
#[derive(Debug, Default)]
pub struct Batch {
    pub actions: Vec<Action>,
    /// New cursor (last event id read), if any event was read.
    pub cursor: Option<i64>,
    /// A first sync / backfill is still running.
    pub backfilling: bool,
}

/// Where a store starts scoring: the newest event id when Ken is first
/// enabled (turning it on never scores history, ken.md §1).
pub fn starting_cursor(store: &Store) -> adoken_store::Result<i64> {
    store.with_conn(|c| {
        Ok(
            c.query_row("SELECT COALESCE(MAX(id), 0) FROM sync_event", [], |r| {
                r.get(0)
            })?,
        )
    })
}

/// First enable on a store: viewed marks that already exist are history,
/// claimed without scoring (like the event cursor in [`starting_cursor`]).
pub fn baseline_viewed(store: &Store, now: Millis) -> adoken_store::Result<()> {
    new_viewed(store, now).map(drop)
}

/// Read everything new since `cursor` and turn it into actions.
pub fn poll(store: &Store, cursor: i64, now: Millis) -> adoken_store::Result<Batch> {
    let Some(me) = store.me()?.map(|p| p.id) else {
        return Ok(Batch::default());
    };
    let backfilling = store.with_conn(|c| {
        Ok(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_cursor WHERE baseline_done = 0)",
            [],
            |r| r.get::<_, bool>(0),
        )?)
    })?;
    let mut facts = FactCache::new(me.clone());
    let mut actions = Vec::new();

    let viewed = new_viewed(store, now)?;
    for (pr_id, _, _) in viewed {
        if let Some(pr) = facts.get(store, pr_id)? {
            actions.push(Action::Viewed { pr, at: now });
        }
    }

    let events = store.sync_events_since(cursor, EVENT_BATCH)?;
    let next_cursor = events.last().map(|e| e.id);
    for event in &events {
        if let Some(action) = event_actions(store, event, &me, &mut facts)? {
            actions.extend(action);
        }
    }
    Ok(Batch {
        actions,
        cursor: next_cursor,
        backfilling,
    })
}

fn event_actions(
    store: &Store,
    event: &SyncEvent,
    me: &PersonId,
    facts: &mut FactCache,
) -> adoken_store::Result<Option<Vec<Action>>> {
    let mine = event.self_caused || event.actor.as_deref() == Some(me.as_str());
    let at = event.at;
    let out = match (event.kind, &event.subject) {
        (SyncEventKind::PrCommentAdded, Subject::Pr(id)) if mine => {
            let Some(pr) = facts.get(store, *id)? else {
                return Ok(None);
            };
            let body = facts.comment_body(*id, &event.dedupe_key);
            vec![Action::Comment {
                pr,
                body,
                at,
                origin: Origin::Sync,
            }]
        }
        (SyncEventKind::PrVoteChanged, Subject::Pr(id)) if mine => {
            let Some(pr) = facts.get(store, *id)? else {
                return Ok(None);
            };
            // Sync's key: `person:vote:commit`.
            let mut parts = event.dedupe_key.splitn(3, ':');
            let (_, vote, commit) = (parts.next(), parts.next(), parts.next());
            let Some(vote) = vote.and_then(|v| v.parse::<i8>().ok()) else {
                return Ok(None);
            };
            let commit = commit
                .filter(|c| !c.is_empty() && !c.starts_with("live"))
                .map(str::to_owned);
            vec![Action::Vote {
                pr,
                vote: Vote::from_i8(vote),
                commit,
                at,
                origin: Origin::Sync,
            }]
        }
        (SyncEventKind::PrThreadResolved, Subject::Pr(id)) if mine => {
            let Some(pr) = facts.get(store, *id)? else {
                return Ok(None);
            };
            let Some(thread_id) = event
                .dedupe_key
                .split(':')
                .next()
                .and_then(|t| t.parse::<i64>().ok())
            else {
                return Ok(None);
            };
            let Some(others_commented) = facts.others_commented(*id, thread_id) else {
                return Ok(None);
            };
            vec![Action::Resolve {
                pr,
                others_commented,
                at,
                origin: Origin::Sync,
            }]
        }
        (SyncEventKind::PrStatusChanged, Subject::Pr(id))
            if event.dedupe_key.split(':').next() == Some("completed") =>
        {
            let Some(pr) = facts.get(store, *id)? else {
                return Ok(None);
            };
            let i_approved = facts.my_vote(*id).is_some_and(|v| v.as_i8() > 0);
            let mut out = Vec::new();
            if mine {
                out.push(Action::Complete {
                    pr: pr.clone(),
                    at,
                    origin: Origin::Sync,
                });
            }
            out.push(Action::Merged { pr, at, i_approved });
            out
        }
        (SyncEventKind::BuildCompleted, Subject::Build(run_id)) => {
            let Some(run) = store.run(*run_id)? else {
                return Ok(None);
            };
            let (Some(pr_id), Some(result)) = (run.pr_id, run.result) else {
                return Ok(None);
            };
            let Some(pr) = facts.get(store, pr_id)? else {
                return Ok(None);
            };
            let succeeded = match result {
                RunResult::Succeeded => true,
                RunResult::Failed => false,
                RunResult::PartiallySucceeded | RunResult::Canceled => return Ok(None),
            };
            vec![Action::Build { pr, succeeded, at }]
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

/// Viewed marks not scored yet, claimed now.
fn new_viewed(store: &Store, now: Millis) -> adoken_store::Result<Vec<(i64, String, String)>> {
    let rows = store.with_conn(|c| {
        let exists: Option<i64> = c
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [REVIEW_VIEWED_TABLE],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Ok(Vec::new());
        }
        let mut stmt = c.prepare_cached(
            "SELECT pr_id, path, object_id FROM review_viewed
             EXCEPT SELECT pr_id, path, object_id FROM ken_viewed",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    persist::claim_viewed(store, &rows, now)
}

/// PR details loaded once per poll.
struct FactCache {
    me: PersonId,
    details: HashMap<i64, Option<PrDetail>>,
}

impl FactCache {
    fn new(me: PersonId) -> Self {
        Self {
            me,
            details: HashMap::new(),
        }
    }

    fn detail(&mut self, store: &Store, id: i64) -> adoken_store::Result<Option<&PrDetail>> {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.details.entry(id) {
            entry.insert(store.pr(id)?);
        }
        Ok(self.details.get(&id).and_then(Option::as_ref))
    }

    fn get(&mut self, store: &Store, id: i64) -> adoken_store::Result<Option<PrFacts>> {
        let me = self.me.clone();
        let opened_at = store.with_conn(|c| read::last_viewed(c, &format!("pr:{id}")))?;
        let Some(detail) = self.detail(store, id)? else {
            return Ok(None);
        };
        Ok(Some(facts(detail, &me, opened_at)))
    }

    fn cached(&self, id: i64) -> Option<&PrDetail> {
        self.details.get(&id).and_then(Option::as_ref)
    }

    /// Body of the comment a `thread:comment` key names, if synced.
    fn comment_body(&self, pr_id: i64, key: &str) -> Option<String> {
        let (thread, comment) = key.split_once(':')?;
        let (thread, comment) = (thread.parse::<i64>().ok()?, comment.parse::<i64>().ok()?);
        self.cached(pr_id)?
            .threads
            .iter()
            .find(|t| t.id == thread)?
            .comments
            .iter()
            .find(|c| c.id == comment)
            .map(|c| c.body.clone())
    }

    /// Whether someone other than me commented on the thread; `None` when
    /// the thread isn't in the store.
    fn others_commented(&self, pr_id: i64, thread_id: i64) -> Option<bool> {
        let thread = self
            .cached(pr_id)?
            .threads
            .iter()
            .find(|t| t.id == thread_id)?;
        Some(
            thread
                .comments
                .iter()
                .any(|c| !c.is_system && !c.is_deleted && c.author != self.me),
        )
    }

    fn my_vote(&self, pr_id: i64) -> Option<Vote> {
        self.cached(pr_id)?.pr.vote_of(&self.me)
    }
}

fn facts(detail: &PrDetail, me: &str, opened_at: Option<Millis>) -> PrFacts {
    let pr = &detail.pr;
    let other = |who: &str| who != me && who != pr.author;
    let first_vote = pr
        .reviewers
        .iter()
        .filter(|r| other(&r.person))
        .filter_map(|r| r.voted_at)
        .min();
    let first_comment = detail
        .threads
        .iter()
        .flat_map(|t| &t.comments)
        .filter(|c| !c.is_system && other(&c.author))
        .map(|c| c.created)
        .min();
    PrFacts {
        id: pr.id,
        title: pr.title.clone(),
        mine: pr.author == me,
        is_draft: pr.is_draft,
        published_at: Some(pr.created),
        others_engaged_at: first_vote.into_iter().chain(first_comment).min(),
        opened_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::time::MINUTE;
    use adoken_store::rusqlite::params;

    fn demo() -> (tempfile::TempDir, Store, adoken_demo_like::Ids) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        persist::migrate(&store).unwrap();
        let ids = adoken_demo_like::seed(&store);
        (dir, store, ids)
    }

    /// A tiny hand-built org: me, a teammate, one PR each, one thread.
    mod adoken_demo_like {
        use adoken_model::*;
        use adoken_store::{Store, write};

        pub const NOW: Millis = 1_790_328_600_000;

        pub struct Ids {
            pub me: String,
            pub mate: String,
            pub their_pr: i64,
            pub my_pr: i64,
        }

        fn pr(id: i64, author: &str, reviewers: Vec<Reviewer>) -> PullRequest {
            PullRequest {
                id,
                repo_id: "r".into(),
                project_id: "p".into(),
                title: format!("PR {id}"),
                description: String::new(),
                author: author.into(),
                reviewers,
                status: PrStatus::Active,
                is_draft: false,
                merge_status: MergeStatus::Succeeded,
                source_branch: "refs/heads/x".into(),
                target_branch: "refs/heads/main".into(),
                last_source_commit: None,
                created: NOW - 3_600_000,
                updated: NOW,
                closed: None,
                iterations: 1,
                last_push: None,
            }
        }

        fn reviewer(person: &str, vote: Vote) -> Reviewer {
            Reviewer {
                person: person.into(),
                vote,
                is_required: false,
                has_declined: false,
                added_at: None,
                voted_at: None,
            }
        }

        pub fn seed(store: &Store) -> Ids {
            let ids = Ids {
                me: "me".into(),
                mate: "mate".into(),
                their_pr: 10,
                my_pr: 20,
            };
            store
                .write(|tx| {
                    write::put_org(
                        tx,
                        &Org {
                            id: "o".into(),
                            name: "Contoso".into(),
                            url: "https://example.invalid".into(),
                        },
                    )?;
                    for (id, me) in [(&ids.me, true), (&ids.mate, false)] {
                        write::upsert_person(
                            tx,
                            &Person {
                                id: id.clone(),
                                display_name: id.clone(),
                                unique_name: None,
                                is_group: false,
                                image_url: None,
                            },
                            me,
                        )?;
                    }
                    write::upsert_project(
                        tx,
                        &Project {
                            id: "p".into(),
                            name: "P".into(),
                        },
                    )?;
                    write::upsert_repo(
                        tx,
                        &Repo {
                            id: "r".into(),
                            project_id: "p".into(),
                            name: "api".into(),
                            default_branch: None,
                        },
                    )?;
                    write::upsert_pr(
                        tx,
                        &pr(
                            ids.their_pr,
                            &ids.mate,
                            vec![reviewer(&ids.me, Vote::Approved)],
                        ),
                    )?;
                    write::upsert_pr(
                        tx,
                        &pr(ids.my_pr, &ids.me, vec![reviewer(&ids.mate, Vote::NoVote)]),
                    )?;
                    write::upsert_thread(
                        tx,
                        &PrThread {
                            id: 5,
                            pr_id: ids.their_pr,
                            status: ThreadStatus::Fixed,
                            anchor: None,
                            iteration: None,
                            comments: vec![Comment {
                                id: 1,
                                parent_id: None,
                                author: ids.mate.clone(),
                                body: "Why not a retry budget here?".into(),
                                created: NOW - 60_000,
                                updated: NOW - 60_000,
                                is_system: false,
                                is_deleted: false,
                            }],
                            updated: NOW,
                        },
                    )?;
                    Ok(())
                })
                .unwrap();
            ids
        }
    }

    use adoken_demo_like::NOW;

    fn event(kind: SyncEventKind, subject: Subject, key: &str, actor: &str) -> SyncEvent {
        SyncEvent {
            id: 0,
            kind,
            subject,
            dedupe_key: key.into(),
            actor: Some(actor.into()),
            self_caused: actor == "me",
            at: NOW,
            summary: String::new(),
        }
    }

    fn insert(store: &Store, events: &[SyncEvent]) {
        store
            .write(|tx| {
                for e in events {
                    adoken_store::write::insert_event(tx, e)?;
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn my_events_become_actions_and_others_are_ignored() {
        let (_dir, store, ids) = demo();
        let start = starting_cursor(&store).unwrap();
        insert(
            &store,
            &[
                event(
                    SyncEventKind::PrVoteChanged,
                    Subject::Pr(ids.their_pr),
                    "me:10:live:1",
                    "me",
                ),
                event(
                    SyncEventKind::PrCommentAdded,
                    Subject::Pr(ids.their_pr),
                    "5:1",
                    "mate",
                ),
                event(
                    SyncEventKind::PrCommentAdded,
                    Subject::Pr(ids.their_pr),
                    "live:2",
                    "me",
                ),
                event(
                    SyncEventKind::PrThreadResolved,
                    Subject::Pr(ids.their_pr),
                    "5:fixed",
                    "me",
                ),
                event(
                    SyncEventKind::PrStatusChanged,
                    Subject::Pr(ids.my_pr),
                    "completed:live:3",
                    "me",
                ),
                event(
                    SyncEventKind::PrStatusChanged,
                    Subject::Pr(ids.their_pr),
                    "completed",
                    "mate",
                ),
            ],
        );
        let batch = poll(&store, start, NOW + MINUTE).unwrap();
        assert!(!batch.backfilling);
        assert!(batch.cursor.unwrap() > start);
        let kinds: Vec<&str> = batch
            .actions
            .iter()
            .map(|a| match a {
                Action::Vote {
                    vote: Vote::Approved,
                    commit: None,
                    ..
                } => "vote",
                Action::Comment { body: None, .. } => "comment",
                Action::Resolve {
                    others_commented: true,
                    ..
                } => "resolve",
                Action::Complete { pr, .. } if pr.mine => "complete",
                Action::Merged {
                    pr,
                    i_approved: false,
                    ..
                } if pr.mine => "ko",
                Action::Merged {
                    i_approved: true, ..
                } => "assist",
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["vote", "comment", "resolve", "complete", "ko", "assist"]
        );
        // The only comment is the author's own: no other reviewer engaged.
        let Action::Vote { pr, .. } = &batch.actions[0] else {
            unreachable!()
        };
        assert_eq!(pr.others_engaged_at, None);
        assert!(!pr.mine);

        // Nothing again from the new cursor.
        let again = poll(&store, batch.cursor.unwrap(), NOW + MINUTE).unwrap();
        assert!(again.actions.is_empty());
    }

    #[test]
    fn viewed_marks_score_once_per_file_version() {
        let (_dir, store, ids) = demo();
        store
            .migrate_ext(
                "review",
                &["CREATE TABLE review_viewed (
                     pr_id INTEGER NOT NULL, path TEXT NOT NULL, object_id TEXT NOT NULL,
                     PRIMARY KEY (pr_id, path, object_id)) STRICT, WITHOUT ROWID;
                   CREATE TABLE review_pref (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT, WITHOUT ROWID;"],
            )
            .unwrap();
        let mark = |object: &str| {
            store
                .write(|tx| {
                    tx.execute(
                        "INSERT OR IGNORE INTO review_viewed VALUES (?1, 'src/a.rs', ?2)",
                        params![ids.their_pr, object],
                    )?;
                    Ok(())
                })
                .unwrap()
        };
        let unmark = || {
            store
                .write(|tx| {
                    tx.execute("DELETE FROM review_viewed", [])?;
                    Ok(())
                })
                .unwrap()
        };
        // Marks made before Ken was first enabled are history.
        mark("v0");
        baseline_viewed(&store, NOW).unwrap();
        let start = starting_cursor(&store).unwrap();
        mark("v1");
        let viewed = |store: &Store| {
            poll(store, start, NOW)
                .unwrap()
                .actions
                .into_iter()
                .filter(|a| matches!(a, Action::Viewed { .. }))
                .count()
        };
        assert_eq!(viewed(&store), 1);
        unmark();
        mark("v1");
        assert_eq!(viewed(&store), 0, "toggling off and on scores once");
        mark("v2");
        assert_eq!(viewed(&store), 1, "a new version of the file is new work");
    }

    #[test]
    fn unfinished_baseline_means_backfilling() {
        let (_dir, store, _) = demo();
        store
            .write(|tx| {
                adoken_store::write::put_cursor(
                    tx,
                    &read::Cursor {
                        scope: "pr.active".into(),
                        project_id: "p".into(),
                        baseline_done: false,
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(poll(&store, 0, NOW).unwrap().backfilling);
    }
}
