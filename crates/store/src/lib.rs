//! Local store for synced Azure DevOps data (ADR 0001).
//!
//! One SQLite file per org, WAL mode. A single writer connection (behind a
//! mutex; one sync page = one [`Store::write`] transaction) and a small pool of
//! read-only connections for views. Every call blocks: run it on a background
//! executor, never on the UI thread. Reads are index-backed.
//!
//! - Schema: `migrations/NNNN_slug.sql`, forward-only via `rusqlite_migration`,
//!   `VACUUM INTO` backup before upgrading an existing file, refusal to open a
//!   newer schema.
//! - Other crates (notify, nudges) own their local tables through
//!   [`Store::migrate_ext`].
//! - Typed reads live in [`read`], idempotent upserts in [`write`]; both take
//!   a plain `&Connection` so sync can diff and write inside one transaction.
//!
//! Connection handling follows zeron's `crates/sync/src/store.rs`
//! (<https://github.com/zeronsh/zeron>, MIT,
//! @winglee): bundled SQLite, WAL, `synchronous=NORMAL`.

mod blobs;
mod filter;
pub mod read;
pub mod wi;
pub mod write;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

pub use blobs::{BlobCache, DEFAULT_CAP_BYTES as BLOB_CAP_BYTES, is_object_id};
pub use filter::{PrFilter, RunFilter, WorkItemFilter};
pub use read::{Cursor, TextHit};
pub use rusqlite;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use rusqlite_migration::{M, Migrations};

use adoken_model::{
    Mention, Millis, Org, Person, PersonId, Pipeline, PipelineRun, PrDetail, PrThread, Project,
    PullRequest, Repo, RunStage, SyncEvent, WorkItem,
};
use adoken_search::SearchDoc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("{what} schema v{found} is newer than this build supports (v{supported})")]
    SchemaTooNew {
        what: String,
        found: i64,
        supported: i64,
    },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt row: {0}")]
    Corrupt(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Core migrations. Append-only: a shipped file is never edited.
const MIGRATION_SLICE: &[M<'static>] = &[
    M::up(include_str!("../migrations/0001_init.sql")),
    M::up(include_str!("../migrations/0002_timelines_comments.sql")),
    M::up(include_str!("../migrations/0003_pr_writes.sql")),
    M::up(include_str!("../migrations/0004_identity_image.sql")),
    M::up(include_str!(
        "../migrations/0005_pr_tracking_completing.sql"
    )),
    M::up(include_str!("../migrations/0006_pr_file_sync.sql")),
    M::up(include_str!("../migrations/0007_pins.sql")),
    M::up(include_str!(
        "../migrations/0008_work_item_terminal_since.sql"
    )),
    M::up(include_str!("../migrations/0009_work_item_editing.sql")),
    M::up(include_str!("../migrations/0010_sync_activity.sql")),
    M::up(include_str!("../migrations/0011_today_activity.sql")),
];
const MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATION_SLICE);

/// Latest core schema version (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i64 = MIGRATION_SLICE.len() as i64;

/// Read pool size (ADR 0001: 2-4 read-only connections). Opened lazily so
/// cold start pays for one connection at most.
const MAX_READERS: usize = 4;

/// The blob cache's directory beside the store file.
const BLOB_DIR: &str = "blobs";

const COMMON_PRAGMAS: &str = "
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -32768;
PRAGMA mmap_size = 268435456;
";

/// Cheap-to-clone handle; `Send + Sync`.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    writer: Mutex<Connection>,
    pool: Mutex<Pool>,
    pool_cv: Condvar,
    generation: AtomicU64,
    subscribers: Mutex<Vec<mpsc::Sender<u64>>>,
    blobs: BlobCache,
}

struct Pool {
    idle: Vec<Connection>,
    opened: usize,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.inner.path)
            .finish()
    }
}

impl Store {
    /// Open (creating if needed) the store at `path`, applying pending
    /// migrations. The parent directory is created `0700`, the file `0600`.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
            restrict(dir, 0o700)?;
        }
        let mut writer = Connection::open(&path)?;
        restrict(&path, 0o600)?;
        let mode: String = writer.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            log::warn!("store: journal_mode is {mode}, expected wal");
        }
        writer.execute_batch(COMMON_PRAGMAS)?;
        writer.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA journal_size_limit = 67108864;",
        )?;
        let blobs = BlobCache::new(path.parent().unwrap_or(Path::new(".")).join(BLOB_DIR));
        migrate(&mut writer, &path, &blobs)?;
        Ok(Store {
            inner: Arc::new(Inner {
                path,
                writer: Mutex::new(writer),
                pool: Mutex::new(Pool {
                    idle: Vec::with_capacity(MAX_READERS),
                    opened: 0,
                }),
                pool_cv: Condvar::new(),
                generation: AtomicU64::new(0),
                subscribers: Mutex::new(Vec::new()),
                blobs,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Content-addressed file contents beside this store (spec 0004 FR-2).
    pub fn blobs(&self) -> &BlobCache {
        &self.inner.blobs
    }

    /// Run `f` on a pooled read-only connection (`query_only=1`).
    pub fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let conn = self.reader()?;
        f(conn.conn.as_ref().expect("pooled connection present"))
    }

    /// Run `f` in one IMMEDIATE transaction on the writer and commit. An error
    /// rolls everything back (ADR 0001: page + cursor commit atomically).
    /// Subscribers are notified after a successful commit.
    pub fn write<R>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<R>) -> Result<R> {
        let out = {
            let mut conn = lock(&self.inner.writer);
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = f(&tx)?;
            tx.commit()?;
            out
        };
        let generation = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        lock(&self.inner.subscribers).retain(|tx| tx.send(generation).is_ok());
        Ok(out)
    }

    /// Commit counter; bumps once per successful [`Store::write`].
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// Receive the new [`Store::generation`] after every commit (e.g. to
    /// refresh a view or reload the Cmd+P index). Dropping the receiver
    /// unsubscribes.
    pub fn subscribe(&self) -> mpsc::Receiver<u64> {
        let (tx, rx) = mpsc::channel();
        lock(&self.inner.subscribers).push(tx);
        rx
    }

    /// Forward-only migrations for tables another crate owns (`ns` = crate
    /// name, tables prefixed `<ns>_`). `steps[i]` moves the namespace from
    /// version `i` to `i + 1`; applied steps are never re-run, so shipped
    /// steps must never change. Each call applies all pending steps in one
    /// transaction.
    pub fn migrate_ext(&self, ns: &str, steps: &[&str]) -> Result<()> {
        self.write(|tx| {
            let current: i64 = tx
                .query_row(
                    "SELECT version FROM ext_migration WHERE ns = ?1",
                    [ns],
                    |r| r.get(0),
                )
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(0),
                    e => Err(e),
                })?;
            let supported = steps.len() as i64;
            if current > supported {
                return Err(Error::SchemaTooNew {
                    what: ns.to_owned(),
                    found: current,
                    supported,
                });
            }
            for step in &steps[current as usize..] {
                tx.execute_batch(step)?;
            }
            tx.execute(
                "INSERT INTO ext_migration(ns, version) VALUES (?1, ?2)
                 ON CONFLICT(ns) DO UPDATE SET version = excluded.version",
                rusqlite::params![ns, supported],
            )?;
            Ok(())
        })
    }

    /// `PRAGMA optimize`; call on idle and at shutdown.
    pub fn optimize(&self) -> Result<()> {
        lock(&self.inner.writer).execute_batch("PRAGMA optimize;")?;
        Ok(())
    }

    // ── typed reads (see `read` for semantics) ───────────────────────────

    pub fn org(&self) -> Result<Option<Org>> {
        self.with_conn(read::org)
    }
    pub fn me(&self) -> Result<Option<Person>> {
        self.with_conn(read::me)
    }
    pub fn people(&self) -> Result<Vec<Person>> {
        self.with_conn(read::people)
    }
    pub fn person(&self, id: &str) -> Result<Option<Person>> {
        self.with_conn(|c| read::person(c, id))
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.with_conn(read::projects)
    }
    /// Projects sync covers; [`Store::projects`] also lists projects known
    /// only through a pinned item (spec 0014).
    pub fn synced_projects(&self) -> Result<Vec<Project>> {
        self.with_conn(read::synced_projects)
    }
    pub fn repos(&self) -> Result<Vec<Repo>> {
        self.with_conn(read::repos)
    }
    pub fn prs(&self, filter: &PrFilter) -> Result<Vec<PullRequest>> {
        self.with_conn(|c| read::prs(c, filter))
    }
    pub fn pr(&self, id: i64) -> Result<Option<PrDetail>> {
        self.with_conn(|c| read::pr_detail(c, id))
    }
    pub fn pr_threads_batch(&self, ids: &[i64]) -> Result<Vec<(i64, Vec<PrThread>)>> {
        self.with_conn(|c| read::pr_threads_batch(c, ids))
    }
    /// A cached blob as text ([`BlobCache::text`]).
    pub fn blob(&self, id: &str) -> Result<Option<String>> {
        Ok(self.inner.blobs.text(id)?)
    }
    pub fn work_items(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>> {
        self.with_conn(|c| read::work_items(c, filter))
    }
    pub fn work_item(&self, id: i64) -> Result<Option<WorkItem>> {
        self.with_conn(|c| read::work_item(c, id))
    }
    /// Work items linked to pull request `pr_id` ([`read::pr_work_items`]).
    pub fn pr_work_items(&self, pr_id: i64) -> Result<Vec<WorkItem>> {
        self.with_conn(|c| read::pr_work_items(c, pr_id))
    }
    pub fn pipelines(&self) -> Result<Vec<Pipeline>> {
        self.with_conn(read::pipelines)
    }
    pub fn runs(&self, filter: &RunFilter) -> Result<Vec<PipelineRun>> {
        self.with_conn(|c| read::runs(c, filter))
    }
    pub fn run(&self, id: i64) -> Result<Option<PipelineRun>> {
        self.with_conn(|c| read::run(c, id))
    }
    /// Stages of run `id` in pipeline order (empty until synced).
    pub fn run_stages(&self, id: i64) -> Result<Vec<RunStage>> {
        self.with_conn(|c| read::run_stages(c, id))
    }
    /// Mentions of `who`, newest first.
    pub fn mentions(&self, who: &PersonId, limit: usize) -> Result<Vec<Mention>> {
        self.with_conn(|c| read::mentions(c, who, limit))
    }
    /// Persisted sync events with `id > after_id`, ascending by id.
    pub fn sync_events_since(&self, after_id: i64, limit: usize) -> Result<Vec<SyncEvent>> {
        self.with_conn(|c| read::sync_events_since(c, after_id, limit))
    }
    /// Tier-1 Cmd+P corpus. Ids: PR id, work item id, pipeline definition
    /// id; for repos/projects/people the entity row's stable rowid. Map a doc
    /// back with [`Store::entity_key`].
    pub fn search_docs(&self) -> Result<Vec<SearchDoc>> {
        self.with_conn(read::search_docs)
    }
    /// Entity key (`pr:4312`, `wi:88121`, `pipe:<project>:<id>`,
    /// `repo:<guid>`, `project:<guid>`, `person:<guid>`) of a search doc.
    pub fn entity_key(&self, doc: &SearchDoc) -> Result<Option<String>> {
        self.with_conn(|c| read::entity_key(c, doc.kind, doc.id))
    }
    /// Tier-2 long-text search (FTS5 prefix-AND over titles, descriptions,
    /// comments), best first.
    pub fn search_text(&self, query: &str, limit: usize) -> Result<Vec<TextHit>> {
        self.with_conn(|c| read::search_text(c, query, limit))
    }
    pub fn cursor(&self, scope: &str, project_id: &str) -> Result<Option<Cursor>> {
        self.with_conn(|c| read::cursor(c, scope, project_id))
    }
    /// Most recent successful sync of any scope.
    pub fn last_sync(&self) -> Result<Option<Millis>> {
        self.with_conn(read::last_sync)
    }
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        self.with_conn(|c| read::meta(c, key))
    }
    /// Auto-complete I set in adoken: `(reviewed source commit, set at)`.
    pub fn auto_complete(&self, pr_id: i64) -> Result<Option<(String, Millis)>> {
        self.with_conn(|c| read::auto_complete(c, pr_id))
    }

    /// My Complete that Azure DevOps is still merging: `(source commit,
    /// accepted at)`.
    pub fn completing(&self, pr_id: i64) -> Result<Option<(String, Millis)>> {
        self.with_conn(|c| read::completing(c, pr_id))
    }

    /// Open human threads per PR ([`read::active_thread_counts`]).
    pub fn active_thread_counts(&self) -> Result<std::collections::HashMap<i64, u32>> {
        self.with_conn(read::active_thread_counts)
    }

    /// Latest run of each pipeline per PR ([`read::latest_pr_runs`]).
    pub fn latest_pr_runs(&self) -> Result<Vec<PipelineRun>> {
        self.with_conn(read::latest_pr_runs)
    }

    /// PRs with auto-complete I set in adoken.
    pub fn auto_complete_ids(&self) -> Result<std::collections::HashSet<i64>> {
        self.with_conn(read::auto_complete_ids)
    }

    /// PRs Azure DevOps is still merging after my Complete.
    pub fn completing_ids(&self) -> Result<std::collections::HashSet<i64>> {
        self.with_conn(read::completing_ids)
    }

    /// Every pin in pin order (spec 0014, local).
    pub fn pins(&self) -> Result<Vec<adoken_model::Pin>> {
        self.with_conn(read::pins)
    }

    fn reader(&self) -> Result<PooledConn<'_>> {
        let mut pool = lock(&self.inner.pool);
        loop {
            if let Some(conn) = pool.idle.pop() {
                return Ok(PooledConn {
                    store: self,
                    conn: Some(conn),
                });
            }
            if pool.opened < MAX_READERS {
                pool.opened += 1;
                drop(pool);
                return match open_reader(&self.inner.path) {
                    Ok(conn) => Ok(PooledConn {
                        store: self,
                        conn: Some(conn),
                    }),
                    Err(e) => {
                        lock(&self.inner.pool).opened -= 1;
                        self.inner.pool_cv.notify_one();
                        Err(e)
                    }
                };
            }
            pool = self
                .inner
                .pool_cv
                .wait(pool)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

struct PooledConn<'a> {
    store: &'a Store,
    conn: Option<Connection>,
}

impl Drop for PooledConn<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            lock(&self.store.inner.pool).idle.push(conn);
            self.store.inner.pool_cv.notify_one();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn open_reader(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.execute_batch(COMMON_PRAGMAS)?;
    conn.execute_batch("PRAGMA query_only = 1;")?;
    Ok(conn)
}

fn migrate(conn: &mut Connection, path: &Path, blobs: &BlobCache) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            what: "store".to_owned(),
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current > 0 {
        // ADR 0001 §6: keep the newest pre-upgrade backup only.
        let dir = path.parent().unwrap_or(Path::new("."));
        let stem = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for entry in std::fs::read_dir(dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{stem}.pre-v")) && name.ends_with(".bak") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let backup = dir.join(format!("{stem}.pre-v{SCHEMA_VERSION}.bak"));
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy()])?;
        restrict(&backup, 0o600)?;
    }
    if (1..BLOBS_LEFT_SQLITE).contains(&current) {
        export_blobs(conn, blobs)?;
    }
    MIGRATIONS.to_latest(conn)?;
    Ok(())
}

/// Schema version (0006) whose migration drops the `blob` table.
const BLOBS_LEFT_SQLITE: i64 = 6;

/// Before 0006 drops the SQLite `blob` table, move its rows (demo stores
/// seeded by an older build) into the file cache.
fn export_blobs(conn: &Connection, blobs: &BlobCache) -> Result<()> {
    let mut st = conn.prepare("SELECT id, content FROM blob")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, content) = row?;
        if is_object_id(&id) {
            blobs.put(&id, content.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
