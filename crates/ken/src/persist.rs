//! Local-only Ken tables (ken.md §11) through `Store::migrate_ext("ken")`.
//! Never synced; exported only by an explicit fight-card export. Every call
//! blocks: run on a background executor.

use adoken_model::Millis;
use adoken_store::Store;
use adoken_store::rusqlite::{self, OptionalExtension, params};

use crate::engine::{Journal, Ken};
use crate::moves::{HitKind, Rank};

const MIGRATIONS: &[&str] = &["
CREATE TABLE ken_hits (
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  kind TEXT NOT NULL,
  pr_id INTEGER NOT NULL,
  combo_id INTEGER NOT NULL
) STRICT;
CREATE INDEX ken_hits_by_at ON ken_hits(at);
CREATE TABLE ken_combos (
  id INTEGER PRIMARY KEY,
  started_at INTEGER NOT NULL,
  ended_at INTEGER NOT NULL,
  hits INTEGER NOT NULL,
  best_rank TEXT
) STRICT;
CREATE INDEX ken_combos_by_start ON ken_combos(started_at);
CREATE TABLE ken_kos (
  pr_id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  double INTEGER NOT NULL,
  title TEXT NOT NULL
) STRICT;
CREATE INDEX ken_kos_by_at ON ken_kos(at);
CREATE TABLE ken_streak (day INTEGER PRIMARY KEY, state TEXT NOT NULL) STRICT;
CREATE TABLE ken_badges (
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  kind TEXT NOT NULL,
  pr_id INTEGER
) STRICT;
CREATE INDEX ken_badges_by_at ON ken_badges(at);
-- Viewed marks already scored (pr, file, version) with when they were seen.
CREATE TABLE ken_viewed (
  pr_id INTEGER NOT NULL, path TEXT NOT NULL, object_id TEXT NOT NULL, at INTEGER NOT NULL,
  PRIMARY KEY (pr_id, path, object_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE ken_kv (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT, WITHOUT ROWID;
"];

/// `ken_kv` keys.
const STATE_KEY: &str = "state";
const CURSOR_KEY: &str = "cursor";
const OFFERED_WEEK_KEY: &str = "offered_week";

pub fn migrate(store: &Store) -> adoken_store::Result<()> {
    store.migrate_ext("ken", MIGRATIONS)
}

/// Everything the ingest loop resumes from.
#[derive(Debug, Default)]
pub struct Loaded {
    pub ken: Ken,
    /// Last `sync_event.id` ingested; `None` = never enabled on this store.
    pub cursor: Option<i64>,
}

pub fn load(store: &Store) -> adoken_store::Result<Loaded> {
    store.with_conn(|c| {
        let ken = get(c, STATE_KEY)?
            .and_then(|json| match serde_json::from_str(&json) {
                Ok(ken) => Some(ken),
                Err(error) => {
                    log::warn!("ken: discarding unreadable state: {error}");
                    None
                }
            })
            .unwrap_or_default();
        let cursor = get(c, CURSOR_KEY)?.and_then(|v| v.parse().ok());
        Ok(Loaded { ken, cursor })
    })
}

/// Persist the journal, the engine state and the cursor in one transaction.
pub fn save(
    store: &Store,
    ken: &Ken,
    journal: &Journal,
    cursor: Option<i64>,
) -> adoken_store::Result<()> {
    let state = serde_json::to_string(ken).expect("ken state serializes");
    store.write(|tx| {
        for hit in &journal.hits {
            tx.execute(
                "INSERT INTO ken_hits(at, kind, pr_id, combo_id) VALUES (?1, ?2, ?3, ?4)",
                params![hit.at, hit.kind.as_str(), hit.pr, hit.combo_id],
            )?;
        }
        for combo in &journal.combos {
            tx.execute(
                "INSERT OR REPLACE INTO ken_combos(id, started_at, ended_at, hits, best_rank)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    combo.id,
                    combo.started_at,
                    combo.ended_at,
                    combo.hits,
                    combo.best_rank.map(Rank::as_str)
                ],
            )?;
        }
        for ko in &journal.kos {
            tx.execute(
                "INSERT OR IGNORE INTO ken_kos(pr_id, at, double, title) VALUES (?1, ?2, ?3, ?4)",
                params![ko.pr, ko.at, ko.double, ko.title],
            )?;
        }
        for badge in &journal.badges {
            tx.execute(
                "INSERT INTO ken_badges(at, kind, pr_id) VALUES (?1, ?2, ?3)",
                params![badge.at, badge.kind, badge.pr],
            )?;
        }
        for (day, state) in &journal.streak {
            tx.execute(
                "INSERT OR REPLACE INTO ken_streak(day, state) VALUES (?1, ?2)",
                params![day, state.as_str()],
            )?;
        }
        put(tx, STATE_KEY, &state)?;
        if let Some(cursor) = cursor {
            put(tx, CURSOR_KEY, &cursor.to_string())?;
        }
        Ok(())
    })
}

/// Save engine state only (streak pause/resume): no journal, cursor kept.
pub fn save_state(store: &Store, ken: &Ken) -> adoken_store::Result<()> {
    save(store, ken, &Journal::default(), None)
}

/// Settings › Arcade mode › [Reset stats]: every Ken row and the state go.
/// The cursor stays so nothing already seen is re-scored.
pub fn reset(store: &Store) -> adoken_store::Result<()> {
    store.write(|tx| {
        tx.execute_batch(
            "DELETE FROM ken_hits; DELETE FROM ken_combos; DELETE FROM ken_kos;
             DELETE FROM ken_streak; DELETE FROM ken_badges;",
        )?;
        tx.execute("DELETE FROM ken_kv WHERE key = ?1", [STATE_KEY])?;
        Ok(())
    })
}

/// The local week (Monday day number) whose fight card was last offered.
pub fn offered_week(store: &Store) -> adoken_store::Result<Option<i64>> {
    store.with_conn(|c| Ok(get(c, OFFERED_WEEK_KEY)?.and_then(|v| v.parse().ok())))
}

pub fn set_offered_week(store: &Store, week: i64) -> adoken_store::Result<()> {
    store.write(|tx| put(tx, OFFERED_WEEK_KEY, &week.to_string()))
}

/// Mark viewed rows scored; returns the ones not seen before. `rows` are
/// `(pr_id, path, object_id)` from the review view's local table.
pub fn claim_viewed(
    store: &Store,
    rows: &[(i64, String, String)],
    at: Millis,
) -> adoken_store::Result<Vec<(i64, String, String)>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    store.write(|tx| {
        let mut fresh = Vec::new();
        let mut insert = tx.prepare_cached(
            "INSERT OR IGNORE INTO ken_viewed(pr_id, path, object_id, at) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for row in rows {
            if insert.execute(params![row.0, row.1, row.2, at])? > 0 {
                fresh.push(row.clone());
            }
        }
        Ok(fresh)
    })
}

// ── reads for the fight card ─────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HitsInRange {
    pub hits: u32,
    /// Distinct PRs with a review hit (not counting completions).
    pub prs_reviewed: u32,
}

pub fn hits_between(store: &Store, from: Millis, to: Millis) -> adoken_store::Result<HitsInRange> {
    store.with_conn(|c| {
        let (hits, prs_reviewed) = c.query_row(
            "SELECT COUNT(*), COUNT(DISTINCT CASE WHEN kind != ?3 THEN pr_id END)
             FROM ken_hits WHERE at >= ?1 AND at < ?2",
            params![from, to, HitKind::Complete.as_str()],
            |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?)),
        )?;
        Ok(HitsInRange { hits, prs_reviewed })
    })
}

/// Best finished combo that started in range.
pub fn best_combo_between(
    store: &Store,
    from: Millis,
    to: Millis,
) -> adoken_store::Result<Option<u32>> {
    store.with_conn(|c| {
        Ok(c.query_row(
            "SELECT MAX(hits) FROM ken_combos WHERE started_at >= ?1 AND started_at < ?2",
            params![from, to],
            |r| r.get::<_, Option<u32>>(0),
        )?)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KoEntry {
    pub pr: i64,
    pub at: Millis,
    pub double: bool,
    pub title: String,
}

pub fn kos_between(store: &Store, from: Millis, to: Millis) -> adoken_store::Result<Vec<KoEntry>> {
    store.with_conn(|c| {
        let mut stmt = c.prepare_cached(
            "SELECT pr_id, at, double, title FROM ken_kos WHERE at >= ?1 AND at < ?2 ORDER BY at",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok(KoEntry {
                pr: r.get(0)?,
                at: r.get(1)?,
                double: r.get(2)?,
                title: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
}

pub fn badge_count_between(
    store: &Store,
    kind: &str,
    from: Millis,
    to: Millis,
) -> adoken_store::Result<u32> {
    store.with_conn(|c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM ken_badges WHERE kind = ?1 AND at >= ?2 AND at < ?3",
            params![kind, from, to],
            |r| r.get(0),
        )?)
    })
}

fn get(c: &rusqlite::Connection, key: &str) -> adoken_store::Result<Option<String>> {
    Ok(
        c.query_row("SELECT value FROM ken_kv WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?,
    )
}

fn put(c: &rusqlite::Connection, key: &str, value: &str) -> adoken_store::Result<()> {
    c.execute(
        "INSERT INTO ken_kv(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ComboRow, HitRow, KoRow};
    use crate::streak::DayState;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
        migrate(&store).unwrap();
        migrate(&store).unwrap(); // idempotent
        (dir, store)
    }

    #[test]
    fn journal_state_and_cursor_round_trip() {
        let (_dir, store) = store();
        assert_eq!(load(&store).unwrap().cursor, None);
        let journal = Journal {
            hits: vec![
                HitRow {
                    at: 10,
                    kind: HitKind::Viewed,
                    pr: 1,
                    combo_id: 1,
                },
                HitRow {
                    at: 20,
                    kind: HitKind::Comment,
                    pr: 2,
                    combo_id: 1,
                },
                HitRow {
                    at: 30,
                    kind: HitKind::Complete,
                    pr: 3,
                    combo_id: 1,
                },
            ],
            combos: vec![ComboRow {
                id: 1,
                started_at: 10,
                ended_at: 630,
                hits: 3,
                best_rank: Some(Rank::Flurry),
            }],
            kos: vec![KoRow {
                pr: 3,
                at: 30,
                double: false,
                title: "Retry idempotency keys".into(),
            }],
            badges: Vec::new(),
            streak: vec![(20_717, DayState::Hit)],
        };
        let ken = Ken::default();
        save(&store, &ken, &journal, Some(42)).unwrap();
        let loaded = load(&store).unwrap();
        assert_eq!(loaded.cursor, Some(42));
        assert_eq!(loaded.ken, ken);
        assert_eq!(
            hits_between(&store, 0, 100).unwrap(),
            HitsInRange {
                hits: 3,
                prs_reviewed: 2
            }
        );
        assert_eq!(hits_between(&store, 15, 25).unwrap().hits, 1);
        assert_eq!(best_combo_between(&store, 0, 100).unwrap(), Some(3));
        assert_eq!(best_combo_between(&store, 11, 100).unwrap(), None);
        assert_eq!(kos_between(&store, 0, 100).unwrap().len(), 1);

        reset(&store).unwrap();
        assert_eq!(hits_between(&store, 0, 100).unwrap().hits, 0);
        assert_eq!(
            load(&store).unwrap().cursor,
            Some(42),
            "cursor survives reset"
        );
    }

    #[test]
    fn viewed_rows_are_claimed_once() {
        let (_dir, store) = store();
        let rows = vec![(7, "src/a.rs".to_owned(), "obj1".to_owned())];
        assert_eq!(claim_viewed(&store, &rows, 1).unwrap(), rows);
        assert!(claim_viewed(&store, &rows, 2).unwrap().is_empty());
        let next_version = vec![(7, "src/a.rs".to_owned(), "obj2".to_owned())];
        assert_eq!(claim_viewed(&store, &next_version, 3).unwrap().len(), 1);
    }

    #[test]
    fn offered_week_round_trips() {
        let (_dir, store) = store();
        assert_eq!(offered_week(&store).unwrap(), None);
        set_offered_week(&store, 20_717).unwrap();
        assert_eq!(offered_week(&store).unwrap(), Some(20_717));
    }
}
