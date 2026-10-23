//! Local notify state in the org store (ADR 0001 "local" tables, namespace
//! `notify`) plus the rules/settings TOML next to it.
//!
//! Blocking: call from a background executor, never the UI thread.

use std::path::PathBuf;

use adoken_model::{Millis, RunResult, Subject};
use adoken_store::rusqlite::{OptionalExtension, Transaction, params};
use adoken_store::{Result, Store};
use serde::{Deserialize, Serialize};

use crate::engine::{Changes, EngineState, Held, Notification};
use crate::filter::Filter;
use crate::rules::Mute;
use crate::settings::NotifySettings;

/// Forward-only; never edit a shipped step.
const MIGRATIONS: &[&str] = &["
CREATE TABLE notify_item (
  id INTEGER PRIMARY KEY,
  subject TEXT NOT NULL UNIQUE,
  updated_ms INTEGER NOT NULL,
  done_ms INTEGER,
  body TEXT NOT NULL              -- JSON `Notification`
) STRICT;
CREATE INDEX notify_item_done ON notify_item(done_ms);
CREATE TABLE notify_seen (
  key TEXT PRIMARY KEY,           -- event dedupe key (FR-1)
  at_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX notify_seen_at ON notify_seen(at_ms);
CREATE TABLE notify_build (
  pipeline_id INTEGER NOT NULL,
  branch TEXT NOT NULL,
  result TEXT NOT NULL,
  PRIMARY KEY (pipeline_id, branch)
) STRICT, WITHOUT ROWID;
CREATE TABLE notify_queue (
  seq INTEGER PRIMARY KEY,        -- ops from other crates (nudges), drained by the inbox
  body TEXT NOT NULL              -- JSON `QueueOp`
) STRICT;
CREATE TABLE notify_kv (
  key TEXT PRIMARY KEY,           -- mutes | held | filter | cursor | permission_dismissed
  value TEXT NOT NULL             -- JSON
) STRICT, WITHOUT ROWID;
"];

const SETTINGS_FILE: &str = adoken_settings::FILE_NAME;

/// Work other crates hand to the inbox without depending on its engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueOp {
    /// A nudge (spec 0006) becomes a notification; `key` dedupes.
    Nudge {
        key: String,
        subject: Subject,
        title: String,
        message: String,
        at: Millis,
    },
    /// The nudge's condition cleared: its row is marked done.
    Resolve { subject: Subject, at: Millis },
}

/// How far the caller got: saved in the same transaction as the changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Last `sync_event.id` ingested.
    pub cursor: Option<i64>,
    /// Last `notify_queue.seq` applied (and deleted).
    pub queue_upto: Option<i64>,
}

fn enqueue(store: &Store, op: &QueueOp) -> Result<()> {
    migrate(store)?;
    let body = serde_json::to_string(op).map_err(json_err)?;
    store.write(|tx| {
        tx.execute("INSERT INTO notify_queue(body) VALUES (?1)", [body])?;
        Ok(())
    })
}

/// Called by the nudges pass (blocking).
pub fn submit_nudge(
    store: &Store,
    key: &str,
    subject: &Subject,
    title: &str,
    message: &str,
    at: Millis,
) -> Result<()> {
    enqueue(
        store,
        &QueueOp::Nudge {
            key: key.to_owned(),
            subject: subject.clone(),
            title: title.to_owned(),
            message: message.to_owned(),
            at,
        },
    )
}

/// Called by the nudges pass when a nudge's condition clears (blocking).
pub fn resolve_nudge(store: &Store, subject: &Subject, at: Millis) -> Result<()> {
    enqueue(
        store,
        &QueueOp::Resolve {
            subject: subject.clone(),
            at,
        },
    )
}

/// Pending queue ops in order, with their sequence numbers.
pub fn pending(store: &Store) -> Result<Vec<(i64, QueueOp)>> {
    store.with_conn(|c| {
        let mut stmt = c.prepare("SELECT seq, body FROM notify_queue ORDER BY seq")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(seq, body)| serde_json::from_str(&body).ok().map(|op| (seq, op)))
            .collect())
    })
}

/// Everything needed to rebuild the engine, plus the sync-event cursor.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub state: EngineState,
    /// Last `sync_event.id` ingested.
    pub cursor: i64,
}

/// The preferences file (`[notify]` in `settings.toml`): preferences live in
/// TOML, not the DB (ADR 0001).
pub fn settings_path(store: &Store) -> PathBuf {
    store
        .path()
        .parent()
        .map(|dir| dir.join(SETTINGS_FILE))
        .unwrap_or_else(|| PathBuf::from(SETTINGS_FILE))
}

pub fn migrate(store: &Store) -> Result<()> {
    store.migrate_ext("notify", MIGRATIONS)
}

fn json_err(error: serde_json::Error) -> adoken_store::Error {
    adoken_store::Error::Sqlite(adoken_store::rusqlite::Error::ToSqlConversionFailure(
        Box::new(error),
    ))
}

/// Migrate, then load everything (indexed full scans; 10k rows ≈ ms).
pub fn load(store: &Store) -> Result<Loaded> {
    migrate(store)?;
    let settings = std::fs::read_to_string(settings_path(store))
        .ok()
        .and_then(|text| match NotifySettings::from_settings_toml(&text) {
            Ok(settings) => Some(settings),
            Err(error) => {
                log::warn!("notify: ignoring unreadable {SETTINGS_FILE}: {error}");
                None
            }
        })
        .unwrap_or_default();
    store.with_conn(|c| {
        let mut items = Vec::new();
        let mut stmt = c.prepare("SELECT body FROM notify_item")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let body: String = row.get(0)?;
            match serde_json::from_str::<Notification>(&body) {
                Ok(item) => items.push(item),
                Err(error) => log::warn!("notify: skipping unreadable row: {error}"),
            }
        }
        let seen = c
            .prepare("SELECT key, at_ms FROM notify_seen")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Millis>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let builds = c
            .prepare("SELECT pipeline_id, branch, result FROM notify_build")?
            .query_map([], |r| {
                Ok((
                    (r.get::<_, i64>(0)?, r.get::<_, String>(1)?),
                    r.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|row| {
                row.map(|(key, result)| RunResult::parse(&result).map(|r| (key, r)))
                    .transpose()
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let kv = |key: &str| -> Result<Option<String>> {
            Ok(
                c.query_row("SELECT value FROM notify_kv WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()?,
            )
        };
        fn parse<T: serde::de::DeserializeOwned + Default>(value: Option<String>) -> T {
            value
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default()
        }
        let mutes: Vec<Mute> = parse(kv("mutes")?);
        let held: Vec<Held> = parse(kv("held")?);
        let filter: Filter = parse(kv("filter")?);
        let cursor: i64 = parse(kv("cursor")?);
        let permission_notice_dismissed: bool = parse(kv("permission_dismissed")?);
        Ok(Loaded {
            state: EngineState {
                settings,
                filter,
                items,
                seen,
                mutes,
                held,
                builds,
                permission_notice_dismissed,
            },
            cursor,
        })
    })
}

fn put_kv(tx: &Transaction<'_>, key: &str, value: &impl serde::Serialize) -> Result<()> {
    let value = serde_json::to_string(value).map_err(json_err)?;
    tx.execute(
        "INSERT INTO notify_kv(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Apply `changes` (and advance the event cursor) in one transaction, so an
/// event is ingested exactly once across restarts. Settings go to the TOML
/// file.
pub fn save(store: &Store, changes: &Changes, progress: Progress) -> Result<()> {
    if let Some(settings) = &changes.settings {
        let path = settings_path(store);
        let table = settings
            .to_table()
            .map_err(|e| adoken_store::Error::Io(std::io::Error::other(e)))?;
        // Merged into the shared file: other modules' tables stay intact.
        adoken_settings::update(&path, |document| {
            adoken_settings::set_table(document, crate::settings::SETTINGS_TABLE, &table)
        })
        .map_err(|e| adoken_store::Error::Io(std::io::Error::other(e)))?;
    }
    let only_settings = Changes {
        settings: None,
        ..changes.clone()
    }
    .is_empty();
    if only_settings && progress == Progress::default() {
        return Ok(());
    }
    store.write(|tx| {
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO notify_item(id, subject, updated_ms, done_ms, body)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET subject = excluded.subject,
                   updated_ms = excluded.updated_ms, done_ms = excluded.done_ms,
                   body = excluded.body",
            )?;
            for n in &changes.upserted {
                let body = serde_json::to_string(n).map_err(json_err)?;
                upsert.execute(params![
                    n.id,
                    n.subject.key(),
                    n.updated_at,
                    n.done_at,
                    body
                ])?;
            }
            let mut delete = tx.prepare_cached("DELETE FROM notify_item WHERE id = ?1")?;
            for id in &changes.deleted {
                delete.execute([id])?;
            }
            let mut seen = tx.prepare_cached(
                "INSERT INTO notify_seen(key, at_ms) VALUES (?1, ?2) ON CONFLICT(key) DO NOTHING",
            )?;
            for (key, at) in &changes.seen {
                seen.execute(params![key, at])?;
            }
            let mut build = tx.prepare_cached(
                "INSERT INTO notify_build(pipeline_id, branch, result) VALUES (?1, ?2, ?3)
                 ON CONFLICT(pipeline_id, branch) DO UPDATE SET result = excluded.result",
            )?;
            for ((pipeline, branch), result) in &changes.builds {
                build.execute(params![pipeline, branch, result.as_str()])?;
            }
        }
        if let Some(cutoff) = changes.seen_pruned_before {
            tx.execute("DELETE FROM notify_seen WHERE at_ms < ?1", [cutoff])?;
        }
        if let Some(mutes) = &changes.mutes {
            put_kv(tx, "mutes", mutes)?;
        }
        if let Some(held) = &changes.held {
            put_kv(tx, "held", held)?;
        }
        if let Some(filter) = &changes.filter {
            put_kv(tx, "filter", filter)?;
        }
        if let Some(dismissed) = changes.permission_notice_dismissed {
            put_kv(tx, "permission_dismissed", &dismissed)?;
        }
        if let Some(cursor) = progress.cursor {
            put_kv(tx, "cursor", &cursor)?;
        }
        if let Some(upto) = progress.queue_upto {
            tx.execute("DELETE FROM notify_queue WHERE seq <= ?1", [upto])?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use adoken_model::{SyncEvent, SyncEventKind};

    use super::*;
    use crate::classify::{EventContext, Priority, RuleKey};
    use crate::clock::FakeClock;
    use crate::delivery::{DeliveryEnv, Effects};
    use crate::engine::{Engine, Incoming};
    use crate::snooze::{DEFAULT_MORNING, SnoozeChoice};

    fn event(key: &str, subject: Subject, kind: SyncEventKind) -> Incoming {
        Incoming::from_sync(
            &SyncEvent {
                id: 0,
                kind,
                subject,
                dedupe_key: key.into(),
                actor: Some("p".into()),
                self_caused: false,
                at: 1_000,
                summary: "s".into(),
            },
            EventContext {
                title: "!1 T".into(),
                ..EventContext::default()
            },
        )
    }

    #[test]
    fn state_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        let loaded = load(&store).unwrap();
        assert_eq!(loaded.cursor, 0);
        assert!(loaded.state.items.is_empty());

        let clock = FakeClock::new(10_000, 0);
        let mut engine = Engine::from_state(Arc::new(clock.clone()), loaded.state);
        let mut settings = engine.settings().clone();
        settings.priorities.set(RuleKey::Push, Priority::P1);
        engine.set_settings(settings);
        let env = DeliveryEnv::default();
        let mut fx = Effects::default();
        engine.ingest(
            event("a", Subject::Pr(1), SyncEventKind::MentionedInPr),
            &env,
            &mut fx,
        );
        engine.ingest(
            event("b", Subject::Pr(2), SyncEventKind::PrPushed),
            &env,
            &mut fx,
        );
        let id = engine.list(&Filter::default())[0].id;
        engine.snooze(id, SnoozeChoice::TomorrowMorning, DEFAULT_MORNING);
        engine.mute(Mute::WorkItemType { name: "Bug".into() });
        save(
            &store,
            &engine.take_changes(),
            Progress {
                cursor: Some(42),
                queue_upto: None,
            },
        )
        .unwrap();

        // Reopen from disk.
        drop(store);
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        let loaded = load(&store).unwrap();
        assert_eq!(loaded.cursor, 42);
        let again = Engine::from_state(Arc::new(clock), loaded.state);
        assert_eq!(again.items().len(), 2);
        assert!(again.get(id).unwrap().snooze.is_some());
        assert_eq!(again.mutes().len(), 1);
        assert_eq!(again.settings().priorities.get(RuleKey::Push), Priority::P1);
        let mut fx = Effects::default();
        let mut again = again;
        assert_eq!(
            again.ingest(
                event("a", Subject::Pr(1), SyncEventKind::MentionedInPr),
                &env,
                &mut fx
            ),
            crate::engine::Outcome::Duplicate
        );
        // Nothing dirty → nothing written.
        let changes = again.take_changes();
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[test]
    fn queue_ops_drain_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        submit_nudge(
            &store,
            "N1:pr:9",
            &Subject::Pr(9),
            "!9 T",
            "Waiting 3 days",
            5,
        )
        .unwrap();
        resolve_nudge(&store, &Subject::Pr(9), 6).unwrap();
        let ops = pending(&store).unwrap();
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[1].1, QueueOp::Resolve { .. }));
        let progress = Progress {
            cursor: None,
            queue_upto: Some(ops[1].0),
        };
        save(&store, &Changes::default(), progress).unwrap();
        assert!(pending(&store).unwrap().is_empty());
    }
}
