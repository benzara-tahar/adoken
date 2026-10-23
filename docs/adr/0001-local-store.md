# ADR 0001: Local store and search index

- **Status:** Proposed (2026-09-25). Becomes Accepted once the user signs off and the [benchmark plan](#benchmark-plan) gates B1-B4 pass on the reference machines.
- **Deciders:** @BenzyPrime
- **Owners:** `crates/store` (schema, migrations, connections), `crates/sync` (cursors, writer), `crates/search` (Cmd+P index)
- **Supersedes:** none

## Context

adoken is a local-first Azure DevOps (ADO) client. It serves reads from a local copy of ADO data and runs sync in the background. The store has to hold these entities for one org and many projects:

| Entity | Typical org | Large org (design ceiling) | Write pattern |
|---|---|---|---|
| Pull requests (+ reviewers) | 5k-20k | 100k | Active set re-polled every cycle; closed PRs appended |
| PR threads / comments | 50k | 500k | Refetched per hydrated PR |
| Work items | 20k-100k | 500k | Incremental revisions feed |
| Work item comments | 20k | 300k | Per-item paging, hot set only |
| Mentions of me | 100s | 10k | Derived from comments |
| Pipeline runs (builds) | 10k | 100k (after pruning) | Appended; in-progress rows updated |
| Notifications, snoozes, nudges | 1k-10k | 50k | Local only, user-authored |

Budgets from `AGENTS.md`, restated as store requirements:

- **Cmd+P results in under 50 ms on 100k items**, fuzzy and typo-tolerant. Keystroke-to-results should also fit in one 16 ms frame where possible.
- **Cold start under 500 ms.** Opening the store, running no-op migrations, and rendering the first screen must not scan the corpus.
- **Offline:** every read view works with no network; sync resumes where it stopped.
- **Incremental sync:** no full re-downloads in the steady state. Crash-safe cursors.
- **Secrets:** the PAT lives in the OS keychain only, never in the DB or logs.
- **Portability:** macOS first, Windows at v0.3, Linux best-effort. No system libraries we don't vendor.
- **License:** adoken is MIT, so dependency licenses must allow static linking into an MIT binary that others may ship closed. GPL crates (Zed's `fuzzy` and `sqlez`) are out, even as reading material to copy from.

### ADO API facts that shape the store

These facts decide the cursor design, so they are listed before the options.

| Entity | Endpoint (api-version 7.1) | Incremental mechanism | Caveat |
|---|---|---|---|
| Work items | [`wit/reporting/workitemrevisions`](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/reporting-work-item-revisions/read-reporting-revisions-get?view=azure-devops-rest-7.1) | `continuationToken` watermark, `isLastBatch`, `includeLatestOnly`, `includeDeleted`, `startDateTime` (first call only: "cannot be used in conjunction with watermark") | Long-text fields (Description, Repro Steps) are omitted unless `$expand`. |
| Work items (hot set) | [`wit/wiql`](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/wiql/query-by-wiql?view=azure-devops-rest-7.1) + [`wit/workitemsbatch`](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-items-batch?view=azure-devops-rest-7.1) | `[System.ChangedDate] > @cursor` with `timePrecision=true` | Results are hard-capped at [20,000 items (VS402337)](https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/object-limits?view=azure-devops#work-items-and-queries). Batch GET takes at most 200 ids. `@RecentMentions`, `@Follows`, and `@MyRecentActivity` work in the [web portal only](https://learn.microsoft.com/en-us/azure/devops/boards/queries/query-operators-variables?view=azure-devops). |
| WI comments | [`wit/workItems/{id}/comments`](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/comments/get-comments?view=azure-devops-rest-7.1-preview.4) | `continuationToken` paging | Per item, so only the hot set is fetched. |
| Pull requests | [`git/pullrequests`](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-requests-by-project?view=azure-devops-rest-7.1) | `searchCriteria.minTime/maxTime` + `queryTimeRangeType` = `created` or `closed`, with `$top/$skip` | No "updated since" filter. The list response truncates descriptions to 400 characters. |
| PR threads | [`pullRequests/{id}/threads`](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/list?view=azure-devops-rest-7.1) | None server-side. Each thread and comment carries `lastUpdatedDate`, so diffs are computed locally. | Per PR, so polling cost scales with the hydrated set. |
| Pipeline runs | [`build/builds`](https://learn.microsoft.com/en-us/rest/api/azure/devops/build/builds/list?view=azure-devops-rest-7.1) | `minTime` + `queryOrder=finishTimeAscending` + `continuationToken`; `statusFilter` for in-progress runs | The Pipelines [`runs` API](https://learn.microsoft.com/en-us/rest/api/azure/devops/pipelines/runs/list?view=azure-devops-rest-7.1) is per pipeline (top 10,000) with no time filter, so it isn't used for sync. |
| Notifications | none for a personal inbox | Derived locally from sync diffs | Service hooks need project-admin rights and a public endpoint, so they don't fit a desktop client. |
| Rate limits | [TSTU model](https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops) | Honor `Retry-After`, `X-RateLimit-Remaining`, `X-RateLimit-Limit`, `X-RateLimit-Delay` | 200 TSTUs per sliding 5-minute window. |

The workload that follows: writes come in bursty, batched upserts from one sync writer. Reads are many small indexed lookups for list views, the PR review view, and the notification center, plus one latency-critical fuzzy query per keystroke.

## Options considered

### A. SQLite (rusqlite) with FTS5

- One file with ACID transactions, WAL mode ([readers don't block the writer](https://sqlite.org/wal.html)), secondary indexes, and JSON support. FTS5 ships in the amalgamation with BM25 ranking, `highlight()`, and `snippet()` ([docs](https://sqlite.org/fts5.html)).
- The FTS5 **trigram** tokenizer adds indexed substring `LIKE`/`GLOB` matching. It can't match substrings shorter than 3 characters, and `detail=none|column` rules out MATCH tokens longer than 3 characters ([§4.3.4](https://sqlite.org/fts5.html#the_trigram_tokenizer)).
- FTS5 has no edit-distance matching. Typo tolerance means either OR-ing trigrams, which was measured too slow below, or doing it outside SQLite.
- `rusqlite` 0.40 `bundled` compiles SQLite **3.53.2** (verified at runtime), so there is no dependency on the system SQLite. Encryption is available later through the `bundled-sqlcipher` feature ([rusqlite README](https://github.com/rusqlite/rusqlite#optional-features)).
- The reference app [zeron](https://github.com/zeronsh/zeron) already uses `rusqlite` (bundled, WAL, `synchronous=NORMAL`) in `crates/sync/src/store.rs`.

### A'. sqlx instead of rusqlite

- sqlx is async-first and needs an async runtime (its SQLite driver runs blocking calls on worker threads behind futures). GPUI runs its own executor, and SQLite I/O is synchronous anyway, so the async layer adds a thread hop with no gain. Compile-time checked queries need `DATABASE_URL` or offline metadata in the build.
- Rejected. `rusqlite` on a dedicated writer thread is simpler and is what zeron does.

### B. Tantivy next to SQLite

- A Lucene-class engine: BM25, and [`FuzzyTermQuery`](https://docs.rs/tantivy/0.26.2/tantivy/query/struct.FuzzyTermQuery.html) with Levenshtein distance up to 2 and transposition cost 1. Its per-term typo tolerance on long text beats anything FTS5 offers.
- Costs:
  - A second store with its own segment, commit, and reload lifecycle that must stay consistent with SQLite, and must be rebuildable from it after crashes or schema changes.
  - A writer heap budget (the docs example uses 15 MB).
  - Build time and binary size.
- It doesn't solve Cmd+P either: Tantivy fuzzy matching is per term, while Cmd+P needs fzf-style subsequence matching over `"PR 4312 fix flaky checkout · payments-api · Alice"`.
- Deferred, not rejected. See the triggers under [Consequences](#revisit-triggers).

### C. In-memory fuzzy matchers: nucleo, skim, frizbee

- [`nucleo-matcher`](https://github.com/helix-editor/nucleo) (Helix): fzf scoring, Smith-Waterman with affine gaps, about 6x faster than skim's `fuzzy-matcher`. The upstream README calls the matcher "finished". It has **no typo tolerance**, only subsequence matching. License MPL-2.0 (file-level copyleft, fine as an unmodified dependency).
- [`frizbee`](https://github.com/saghen/frizbee) (blink.cmp, and now the matcher inside skim, atuin, and television): SIMD Smith-Waterman that is **typo-tolerant** (substitution and deletion, `max_typos`), with built-in multi-threaded `match_list_parallel`. It is about 4x faster than nucleo on the upstream benchmark. MIT. Pre-1.0 (0.13), and it doesn't fold diacritics (`á` ≠ `a`), so haystacks have to be normalized at index time.
- Zed's `fuzzy` crate is GPL-3.0 and GPUI-coupled. Not an option.
- These matchers are not stores. They need a corpus in memory, which SQLite feeds (measured below).

### D. redb, sled, fjall (embedded KV)

- [`redb`](https://github.com/cberner/redb) is stable, ACID, and fast. [`sled`](https://github.com/spacejam/sled) says of itself: "if reliability is your primary constraint, use SQLite. sled is beta", and its on-disk format will change before 1.0.
- All three would make us hand-build secondary indexes, a query layer, and full-text search: the work SQLite already does. Rejected.

### E. DuckDB

- A columnar OLAP engine. Row-level upserts from sync are its weak spot. Its FTS extension "will not update automatically when the input table changes" ([docs](https://duckdb.org/docs/current/core_extensions/full_text_search.html)), and extensions autoload from the network. The native library is large.
- A good fit for future analytics (cycle-time dashboards) through its SQLite scanner over our file, which needs no decision now. Rejected as the primary store.

### F. Vector search: sqlite-vec, LanceDB

- [`sqlite-vec`](https://github.com/asg017/sqlite-vec) runs in the same SQLite file with brute-force KNN, float, int8, or binary vectors. It is pre-v1 ("expect breaking changes"); latest stable crate is 0.1.9.
- `lancedb` 0.39 is a separate Arrow and DataFusion stack in its own storage format. Far too heavy for a desktop cache.
- The real cost of semantic search is the embedding model (download size, CPU, privacy of org text), not the vector store. No v0.1 feature needs it.

## Evidence: local benchmark

Throwaway harness (not committed; its logic becomes the B-series benches below):

- Machine: Apple M4 Pro, 14 cores, `--release`.
- Data: seeded synthetic lines shaped like Cmd+P haystacks, e.g. `"Bug 104211 fix flaky checkout retry … payments-api Alice Martin"`, 80 bytes on average.
- Crates: `rusqlite` 0.40 (SQLite 3.53.2), `nucleo-matcher` 0.3.1, `frizbee` 0.13.0.
- Figures are medians of 9 runs. Times are per query over the whole corpus, before any narrowing of the previous result set.

| Measurement | 100k | 500k |
|---|---|---|
| Load all titles from SQLite into `Vec<String>` | 7.5 ms | 35 ms |
| SQLite size: base table / FTS5 `unicode61` / FTS5 `trigram` | 8.7 / 12.0 / 23.5 MiB | 44 / 60 / 117 MiB |
| Upsert 1,000 changed rows, maintaining 2 FTS5 indexes, one transaction | 33 ms | 78 ms |
| FTS5 `unicode61` prefix-AND, `ORDER BY rank LIMIT 50` (worst case: short prefix `retr`) | 1.0-3.8 ms | 5-20 ms |
| FTS5 trigram `LIKE '%substr%'` | < 0.05 ms | < 0.15 ms |
| FTS5 trigram, typo tolerance as OR of trigrams + bm25 | 21-46 ms | **117-286 ms** ✗ |
| nucleo-matcher, 1 thread (worst: `retr`, 86%+ hit rate) | 2-44 ms | 8-205 ms |
| nucleo-matcher, 14 threads | 0.3-5.4 ms | 1.2-23 ms |
| frizbee `max_typos=0`, 1 thread | 1-11 ms | 5-54 ms |
| frizbee `max_typos=1`, 1 thread | 2-27 ms | 10-134 ms |
| frizbee `max_typos=1`, `match_list_parallel` | 0.3-3.3 ms | 1.1-15 ms |
| frizbee `max_typos=None` (unbounded, every row matches) | 2.4-6.6 ms parallel | 7.5-33 ms parallel ✗ (useless ranking) |

What this shows:

1. **Typo tolerance belongs in memory, not in FTS5.** OR-ing trigrams is 6-14x over budget at 500k and ranks poorly.
2. **frizbee is about 2x nucleo** on this corpus at `max_typos=0`, and it is the only one that handles substitutions and transpositions (`chekcout`). Parallel frizbee with one typo stays under 16 ms at 500k on the M4 Pro. The single-thread worst case is 27 ms at 100k, so even a 4-core floor machine fits the 50 ms budget ([INFERENCE] from linear scaling; gate B1 verifies).
3. **Loading the corpus from SQLite is cheap** (35 ms at 500k), so the in-memory index needs no separate persistence. It is rebuilt from SQLite after the first frame.
4. **FTS5 `unicode61` + prefix is fast enough for long-text search.** The trigram index costs about 2.7x the base table on short titles, which would be worse on bodies, so it isn't used for bodies in v0.1.

## Decision

### 1. SQLite is the single local store

One file per org: `<data_dir>/orgs/<org-id>/adoken.sqlite` (`<org-id>` is the lowercased organization name: it is known before the first request, unlike the org GUID; `adoken_ado::org_store_path`), where `<data_dir>` is `~/Library/Application Support/dev.adoken` on macOS and `%APPDATA%\adoken` on Windows. Directory mode `0700`, file mode `0600`. The file holds both kinds of table:

- **cache** tables mirror ADO. They can always be rebuilt by resyncing, and a migration may drop them.
- **local** tables hold user-authored state (notification read/snooze state, delivery windows, nudge dismissals, view history, spec 0014 pins with their order, labels and seen state, spec 0015 work item description and comment drafts and recent assignees, and spec 0016's per-lane last successful sync time). Migrations must preserve these faithfully, including cache rebuilds.

One file keeps a sync page (upserts + search docs + FTS + notifications + cursor advance) inside **one atomic transaction**. Two files joined with `ATTACH` would lose that: in WAL mode, cross-file commits are atomic only per file ([docs](https://sqlite.org/lang_attach.html)).

User preferences (theme, sounds, backgrounds) are not in the DB. They live in a TOML settings file.

File contents for PR diffs (spec 0004 FR-2) are not in the DB either: they sit beside it in `blobs/<ab>/<git object id>` (`adoken_store::BlobCache`, reached through `Store::blobs`), content-addressed and verified against the id, capped at 500 MB with least-recently-used eviction (reads touch the file's mtime). Keeping them out of SQLite means a blob download or LRU touch never counts as a store commit (which would wake every view), and the pre-migration backup never copies them. Migration 0006 moved the rows of the former `blob` table there.

**Connection model**

- **One writer connection**, owned by a dedicated `std::thread` in `crates/sync`. Every write goes through a channel and is applied in batched transactions: one ADO page per transaction.
- **A read pool of 2-4 connections**, opened with `SQLITE_OPEN_READ_ONLY` and `PRAGMA query_only=1`, used from GPUI's background executor.
- No async SQLite wrapper.

**Pragmas at open**

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;      -- durable across app crashes; a power loss may drop the last commits, which resync or idempotent keys recover
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -32768;       -- 32 MiB
PRAGMA mmap_size = 268435456;     -- 256 MiB
PRAGMA journal_size_limit = 67108864;
-- on idle and at shutdown:
PRAGMA optimize;
```

### 2. Search is two tiers, both fed by SQLite

- **Tier 1, Cmd+P (titles and identifiers): an in-memory `frizbee` index.**
  - Loaded from the `search_doc` table on the background executor **after the first frame** (35 ms at 500k).
  - Kept current by `SearchDelta { upserts, deletes }` messages that the sync writer emits after each commit.
  - Each haystack is one precomputed line per entity: kind, id, title, repo or project, key people, branch. It is diacritic-folded at build time (NFKD, combining marks stripped) because frizbee doesn't fold.
  - `max_typos` scales with needle length (0 below 4 characters, then `len / 4`, capped at 2). This follows frizbee's documented per-pattern pattern.
  - Matching runs through `match_list_parallel`. When a keystroke extends the previous query and the typo budget is unchanged, the new query filters the previous hit set instead of the whole corpus. The next keystroke cancels in-flight work.
  - Final ranking = frizbee score plus boosts owned by the search spec: mine, assigned, recently viewed, open vs closed.
  - The rest of the app only sees `search::query(&str) -> Vec<Hit>`. Replacing frizbee with `nucleo-matcher` would be a change local to `crates/search`. That is the planned fallback if frizbee's pre-1.0 API churns badly.
- **Tier 2, long text: SQLite FTS5.** One external-content table over `search_doc(title, body)` (PR and WI descriptions, PR and WI comments), `tokenize = "unicode61 remove_diacritics 2"`, `prefix = '2 3'`, `detail=full` so `snippet()`/`highlight()` work. Queries are prefix-AND with `ORDER BY rank LIMIT n`. Results stream into Cmd+P as a "Found in text" section after tier 1, and they also serve Cmd+P while tier 1 is still loading.
- **Not in v0.1:** trigram FTS, Tantivy, vector search. Revisit triggers are listed below.

### 3. Crates

The versions are the current releases on crates.io as of 2026-09-25. The workspace owner adds them to `[workspace.dependencies]` when the store, sync, and search crates first need them.

| Crate | Version | Features | License | Used by | Why |
|---|---|---|---|---|---|
| `rusqlite` | `0.40.2` | `bundled` (SQLite 3.53.2, FTS5 on) | MIT | store, sync | Same stack as zeron; vendored SQLite; SQLCipher path later |
| `rusqlite_migration` | `2.6.0` | default | Apache-2.0 | store | `user_version`-based, forward-only, `validate()` in tests; MSRV 1.95 matches the workspace |
| `frizbee` | `0.13.0` | default (`std`) | MIT | search | Typo-tolerant SIMD matcher with parallel matching; fastest in our bench |
| `serde_json` | `1` | default | MIT/Apache-2.0 | ado, store | `extra` JSON columns (stored as SQLite JSONB via `jsonb()`) |
| `keyring` | `4.2.0` | per auth spec | MIT/Apache-2.0 | auth | PAT storage, plus the future SQLCipher key. Mentioned here for the "never in DB" rule; the auth spec owns it |

Evaluated and not adopted in v0.1: `sqlx` 0.9.0, `tokio-rusqlite` 0.8.0, `tantivy` 0.26.2, `nucleo-matcher` 0.3.1 (fallback), `redb` 4.3.0, `sled` 0.34.7, `fjall` 3.1.10, `duckdb` 1.10505.0, `lancedb` 0.39.0, `sqlite-vec` 0.1.9.

### 4. Schema sketch (migration `0001_init.sql`)

Conventions:

- All tables are `STRICT`.
- Timestamps are `INTEGER` Unix milliseconds UTC. The original ADO strings are kept only in cursors.
- Natural ADO ids are primary keys: PR ids, work item ids, and build ids are unique per organization (collection).
- `extra` holds unpromoted fields as JSONB, so a new column is a cheap backfill, not a resync.

```sql
-- ── meta ───────────────────────────────────────────────────────────── cache
CREATE TABLE sync_cursor (
  scope        TEXT NOT NULL,             -- 'wi.revisions' | 'pr.active' | 'pr.closed' | 'pr.backfill' | 'build.done' | 'build.live' | 'wi.hot'
  project_id   TEXT NOT NULL DEFAULT '',
  token        TEXT,                      -- opaque ADO continuationToken, if the API has one
  high_water   TEXT,                      -- max server timestamp seen (ISO-8601, verbatim from ADO)
  backfill_to  TEXT,                      -- oldest window already backfilled (pr.backfill)
  baseline_done INTEGER NOT NULL DEFAULT 0,  -- 0 = first sync: diffs don't emit notifications
  last_ok_ms   INTEGER, last_error TEXT,
  PRIMARY KEY (scope, project_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE identity (
  id TEXT PRIMARY KEY, display_name TEXT NOT NULL, unique_name TEXT,
  is_me INTEGER NOT NULL DEFAULT 0, is_group INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE project (id TEXT PRIMARY KEY, name TEXT NOT NULL, synced INTEGER NOT NULL DEFAULT 1) STRICT;
CREATE TABLE repo (
  id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  name TEXT NOT NULL, default_branch TEXT
) STRICT;

-- ── pull requests ──────────────────────────────────────────────────── cache
CREATE TABLE pull_request (
  id            INTEGER PRIMARY KEY,        -- pullRequestId
  repo_id       TEXT NOT NULL REFERENCES repo(id) ON DELETE CASCADE,
  title         TEXT NOT NULL,
  description   TEXT,                       -- full text after hydrate; the list API truncates to 400 chars
  status        TEXT NOT NULL,              -- active | completed | abandoned
  is_draft      INTEGER NOT NULL,
  author_id     TEXT NOT NULL REFERENCES identity(id),
  source_ref    TEXT NOT NULL, target_ref TEXT NOT NULL,
  merge_status  TEXT, last_source_commit TEXT,
  created_ms    INTEGER NOT NULL, closed_ms INTEGER,
  fingerprint   INTEGER NOT NULL,           -- hash of list-payload fields; change detection without an "updated since" API
  hydrated_ms   INTEGER,                    -- threads + full description fetched at
  extra         BLOB                        -- JSONB
) STRICT;
CREATE INDEX pr_by_status  ON pull_request(status, repo_id, created_ms DESC);
CREATE INDEX pr_by_author  ON pull_request(author_id, status);

CREATE TABLE pr_reviewer (
  pr_id INTEGER NOT NULL REFERENCES pull_request(id) ON DELETE CASCADE,
  identity_id TEXT NOT NULL REFERENCES identity(id),
  vote INTEGER NOT NULL, is_required INTEGER NOT NULL, has_declined INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (pr_id, identity_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX pr_reviewer_by_identity ON pr_reviewer(identity_id, pr_id);

CREATE TABLE pr_thread (
  pr_id INTEGER NOT NULL REFERENCES pull_request(id) ON DELETE CASCADE,
  id INTEGER NOT NULL,
  status TEXT,                                -- active | fixed | wontFix | closed | byDesign | pending
  file_path TEXT, right_line INTEGER, left_line INTEGER, iteration INTEGER,
  updated_ms INTEGER NOT NULL,
  PRIMARY KEY (pr_id, id)
) STRICT, WITHOUT ROWID;

-- ── work items ─────────────────────────────────────────────────────── cache
CREATE TABLE work_item (
  id INTEGER PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  rev INTEGER NOT NULL,                       -- upserts only move forward: WHERE excluded.rev > work_item.rev
  type TEXT NOT NULL, title TEXT NOT NULL, state TEXT NOT NULL, reason TEXT,
  assigned_to_id TEXT REFERENCES identity(id),
  area_path TEXT, iteration_path TEXT, priority INTEGER, tags TEXT,
  parent_id INTEGER,
  description TEXT,                           -- plain text; hot set only (long-text fields need a separate fetch). Amended by spec 0015 (migration 0009): stays the plain-text projection for search and previews; raw source + format columns, synced_ms / full_synced_ms added (§6)
  comment_count INTEGER NOT NULL DEFAULT 0,
  created_ms INTEGER NOT NULL, changed_ms INTEGER NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0,
  extra BLOB
) STRICT;
CREATE INDEX wi_by_assignee ON work_item(assigned_to_id, state, changed_ms DESC);
CREATE INDEX wi_by_changed  ON work_item(changed_ms DESC);

-- ── comments (PR + WI unified: one FTS path, one mention extractor) ── cache
CREATE TABLE comment (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL CHECK (source IN ('pr', 'wi')),
  parent_id INTEGER NOT NULL,                 -- pr id or work item id
  thread_id INTEGER NOT NULL DEFAULT 0,       -- 0 for WI comments
  ado_comment_id INTEGER NOT NULL,
  author_id TEXT NOT NULL REFERENCES identity(id),
  body TEXT NOT NULL,                         -- markdown (PR) or HTML→text (WI); WI comments also keep raw source + format (spec 0015, migration 0009)
  created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0,
  UNIQUE (source, parent_id, thread_id, ado_comment_id)
) STRICT;

CREATE TABLE mention (
  comment_id INTEGER NOT NULL REFERENCES comment(id) ON DELETE CASCADE,
  identity_id TEXT NOT NULL REFERENCES identity(id),
  PRIMARY KEY (comment_id, identity_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX mention_by_identity ON mention(identity_id);

-- ── pipelines ──────────────────────────────────────────────────────── cache
CREATE TABLE pipeline_run (
  id INTEGER PRIMARY KEY,                     -- build id
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  definition_id INTEGER NOT NULL, definition_name TEXT NOT NULL,
  run_number TEXT NOT NULL,
  status TEXT NOT NULL, result TEXT, reason TEXT,
  source_branch TEXT, source_version TEXT,
  requested_for_id TEXT REFERENCES identity(id),
  pr_id INTEGER,                              -- set when reason = pullRequest
  queued_ms INTEGER, started_ms INTEGER, finished_ms INTEGER
) STRICT;
CREATE INDEX run_by_pr   ON pipeline_run(pr_id) WHERE pr_id IS NOT NULL;
CREATE INDEX run_by_user ON pipeline_run(requested_for_id, queued_ms DESC);

-- ── search corpus (maintained in the same transaction as entity upserts) ── cache
CREATE TABLE search_doc (
  id INTEGER PRIMARY KEY,
  kind INTEGER NOT NULL,                      -- pr | work_item | comment | run | repo | project
  entity_key TEXT NOT NULL UNIQUE,            -- 'pr:4312', 'wi:88121', 'c:991', 'run:5521'
  project_id TEXT,
  haystack TEXT NOT NULL,                     -- tier-1 Cmd+P line, diacritic-folded
  title TEXT NOT NULL,
  body TEXT,                                  -- tier-2 long text
  updated_ms INTEGER NOT NULL
) STRICT;
CREATE VIRTUAL TABLE search_fts USING fts5(
  title, body,
  content = 'search_doc', content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3'
);
-- + the standard AFTER INSERT/UPDATE/DELETE external-content triggers from https://sqlite.org/fts5.html#external_content_tables

-- ── local (user-authored; migrations must preserve) ─────────────────── local
CREATE TABLE notification (
  id INTEGER PRIMARY KEY,
  dedupe_key TEXT NOT NULL UNIQUE,            -- deterministic: 'pr:4312:comment:7:2', 'run:5521:failed'
  kind TEXT NOT NULL, source TEXT NOT NULL,   -- source: pr | wi | pipeline | mention | nudge
  priority INTEGER NOT NULL,
  entity_key TEXT NOT NULL, actor_id TEXT,
  summary TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  delivered_ms INTEGER, read_ms INTEGER, snoozed_until_ms INTEGER, dismissed_ms INTEGER
) STRICT;
CREATE INDEX notification_inbox ON notification(dismissed_ms, snoozed_until_ms, created_ms DESC);

CREATE TABLE delivery_window (
  id INTEGER PRIMARY KEY, weekday_mask INTEGER NOT NULL,
  start_min INTEGER NOT NULL, end_min INTEGER NOT NULL, tz TEXT NOT NULL,
  mode TEXT NOT NULL                          -- deliver | quiet
) STRICT;
CREATE TABLE nudge_state (
  nudge_key TEXT PRIMARY KEY, last_fired_ms INTEGER, dismissed_until_ms INTEGER
) STRICT, WITHOUT ROWID;
CREATE TABLE view_state (
  entity_key TEXT PRIMARY KEY, last_viewed_ms INTEGER NOT NULL, seen_rev INTEGER
) STRICT, WITHOUT ROWID;
```

The notification-rule and nudge-threshold tables belong to the notify and nudges specs. They are listed here only to fix the cache/local split.

### 5. Sync cursor strategy

Common rules for every scope:

- **Page = transaction.** Upserts, `search_doc`/FTS maintenance, derived `mention` rows, emitted `notification` rows, and the `sync_cursor` advance commit together. A crash resumes from the last committed page.
- **Idempotent upserts** keyed on natural ids, guarded by monotonic fields (`rev` for work items, `updated_ms` for threads and comments), so replayed pages are no-ops.
- **Overlap window.** Time-based cursors re-query from `high_water - 5 min`. This absorbs clock skew and ADO's eventual consistency; idempotency dedupes the overlap.
- **Baseline.** The first sync of a scope sets `baseline_done=1` only when it finishes. Until then, diffs don't emit notifications, so a fresh install or a cache reset doesn't flood the inbox.
- **Identity of "me"** comes from `GET {org}/_apis/connectionData` (`authenticatedUser.id`) and is stored as `identity.is_me`. WIQL uses explicit identities, not portal-only macros.
- **Budget.** Adaptive polling: focused app 60 s, background 5 min, backoff on `X-RateLimit-Remaining` below 20% of `X-RateLimit-Limit`, hard stop on `Retry-After`.

| Scope | Request | Cursor | Notes |
|---|---|---|---|
| `wi.revisions` (per synced project) | `GET {project}/_apis/wit/reporting/workitemrevisions?includeLatestOnly=true&includeDeleted=true&fields=<promoted fields>` | First call: `startDateTime = now - horizon` (default 365 d). Then: stored `continuationToken` until `isLastBatch`; next cycle resumes from the stored token. | Carries every change to promoted fields in synced projects. Deleted items set `is_deleted=1`. `System.CommentCount` changes mark the item for comment fetch when it is in the hot set. |
| `wi.hot` | `POST {org}/_apis/wit/wiql?timePrecision=true` with `[System.AssignedTo] = '<me>' OR [System.CreatedBy] = '<me>'` `AND [System.ChangedDate] > '<high_water>'`, then `workitemsbatch` (≤ 200 ids, fields incl. `System.Description`) | `high_water` = max `System.ChangedDate` | Runs **first** on onboarding so "My work" renders before the full revisions backfill. The date bound keeps it well under the 20,000 cap. Also hydrates long-text fields for hot items. |
| WI comments (hot set) | `GET wit/workItems/{id}/comments?$top=200&continuationToken=` | per item: `comment_count` last seen | Only for hot-set items (assigned, created, followed in app, viewed in the last 30 days) whose count changed. |
| `pr.active` (per project) | `GET {project}/_apis/git/pullrequests?searchCriteria.status=active&$top=1000&$skip=n` | none (full pass, small set) | Row `fingerprint` diff finds changed PRs. PRs that leave the active set are fetched by id to learn completed or abandoned. |
| `pr.closed` | same, with `status=all&queryTimeRangeType=closed&minTime=<high_water - overlap>` | `high_water` = max `closedDate` | Catches completions and abandons between passes. |
| `pr.backfill` | same, with `status=all&queryTimeRangeType=created&minTime/maxTime` windows walking backward | `backfill_to` | Search corpus history to a horizon (default 180 d, configurable). Resumable. Lowest priority. |
| PR threads | `GET repositories/{repo}/pullRequests/{id}/threads` | per PR: max thread `lastUpdatedDate` → `hydrated_ms` | No server-side "since" filter. **Hot PRs** (I am author or reviewer, opened or pinned in app) are refetched when the fingerprint changes and every 2 min while active. **Other active PRs** in synced projects every 30 min, only while there are ≤ 300 of them (tunable after measuring TSTU cost in B5). Comments are diffed locally → `comment`, `mention`, `notification`. |
| `build.done` (per project) | `GET {project}/_apis/build/builds?queryOrder=finishTimeAscending&minTime=<high_water - overlap>&continuationToken=` | `high_water` = max `finishTime` | Scoped to `requestedFor=<me>`, builds of hot PRs, and pinned definitions. Pruned locally after 30 days (setting). |
| `build.live` | `…/builds?statusFilter=inProgress&requestedFor=<me>`, and the same with `statusFilter=notStarted` | none (small set) | Drives "my running pipelines" and failure notifications. |

**Mentions.** ADO exposes no mentions feed; `@RecentMentions` is portal-only. Mentions are extracted locally from comment bodies: PR markdown carries `@<identity-guid>` tokens, and WI comment HTML carries mention anchors with the identity id ([INFERENCE]: both formats must be confirmed against live payloads in the sync spike before the mention extractor is specced). Known limitation: mentions in PRs outside the hydrated set are found only on the 30-minute pass, or not at all above the 300-PR cap. The notifications spec must say so in the UI.

**Notifications** are generated only by the diff step above. The `dedupe_key` is deterministic, so replays and the overlap window can't duplicate an entry.

### 6. Migrations

- `rusqlite_migration` with SQL files embedded via `include_str!` from `crates/store/migrations/NNNN_slug.sql`. The version lives in `PRAGMA user_version`. A unit test calls `MIGRATIONS.validate()`.
- **Append-only:** a shipped migration file is never edited.
- **Cache tables:** an incompatible change is a *resync migration*: `DROP` + `CREATE`, delete the affected `sync_cursor` rows (which resets `baseline_done`), and `INSERT INTO search_fts(search_fts) VALUES('rebuild')` if `search_doc` changed. No data-transform code for data ADO can resend.
- **Local tables:** real data-preserving migrations, using SQLite's [12-step ALTER procedure](https://sqlite.org/lang_altertable.html#otheralter) when needed. Each release adds a fixture DB from the previous release, and a test migrates it and asserts the local rows survive.
- **Safety:** before running pending migrations on an existing file, take `VACUUM INTO '<file>.pre-v{N}.bak'`. Keep the newest backup only.
- **Downgrade:** if `user_version` is newer than the binary knows, don't open the file. Offer "Reset local data", which moves the file aside and resyncs. Never write to a newer schema.
- **Tokenizer or FTS option changes** are resync-class migrations on `search_fts` only.

**Amendments after the initial schema (2026-10-01, specs accepted the same day):**

- **Migration 0009 `0009_work_item_editing.sql` (spec 0015 FR-32, FR-34).**
  - Cache: `work_item` gains `description_source`, `description_format` (`html` | `markdown`), `synced_ms` (set whenever the row is written from an Azure DevOps response) and `full_synced_ms` (set when full fields and relations were applied), and `reason` (`System.Reason` from full answers; the sketch above already listed it, but the shipped migrations didn't have it until 0009). `description` stays the plain-text projection. `comment` gains `body_source` and `format` for work item comments. `wi_text_field(work_item_id, field, source, format)` holds the raw Repro Steps and Acceptance Criteria (0015 FR-3, Q-10).
  - Cache: `wi_link(work_item_id, ord, rel, url, target_id, pr_id, name, comment, rev)` holds every relation of a full work item; `ord` is the server index, so a remove can address `/relations/{index}` (0015 FR-22). A partial index `wi_link_by_target` serves reverse lookups.
  - Cache, 24 h TTL (0015 FR-33): `wi_type_meta(project_id, type, states_ms, fields_ms)`, `wi_type_state`, `wi_type_transition`, `wi_type_field` (kind `list` | `text` | `person` | `unknown`), `wi_class_tree(project_id, fetched_ms)` and `wi_class_node(project_id, kind 'area' | 'iteration', ord, path, name, depth, start_ms, finish_ms)`. Every one has `project_id` → `project` `ON DELETE CASCADE`.
  - Local (no foreign keys into cache tables, so they survive cache rebuilds): `wi_draft(work_item_id, kind 'description' | 'comment', body, base_rev, updated_ms)` (0015 FR-20) and `wi_recent_assignee(identity_id, display_name, unique_name, used_ms)`, which keeps the newest 5 (0015 FR-13).
  - Backfill like migration 0002: it clears the `wi.hot` cursor's `high_water` so the next pass refetches all my items in full, and marks the comments of my items that have any as due. A cache rebuild clears `synced_ms` / `full_synced_ms` with their rows.
- **Migration 0010 `0010_sync_activity.sql` (spec 0016 FR-10).** Local: `lane_sync(lane TEXT PRIMARY KEY, last_ok_ms INTEGER NOT NULL)`, one row per scheduler lane (`my_prs`, `live_runs`, `all_prs`, `work_items`, `done_runs`, `index`). It is written in its own transaction right after the lane's last page commits, so it can lag the data but never lead it, and it survives cache rebuilds. Pass records and the network log stay in memory and add no table (0016 FR-13, FR-26).
- **Next migration (spec 0017 FR-4, FR-22; accepted 2026-10-01).** Cache: `pr_iteration` gains `commit_count INTEGER` and `commits_truncated INTEGER` (both nullable; filled from the iterations request with `includeCommits=true`, NULL for iterations synced before), and `work_item` gains `state_since_ms INTEGER` (nullable; filled from `Microsoft.VSTS.Common.StateChangeDate`, which the work item request already fetches; Today's loose-end "Open {N}d" falls back to `changed_ms` while it is NULL). No backfill request: rows fill on their next sync, and a cache rebuild clears them with their rows.

### 7. Encryption at rest

- **PAT and future OAuth tokens:** OS keychain only, never in SQLite or logs. This is an unchanged project rule.
- **v0.1:** no application-level DB encryption. Protection comes from:
  - OS full-disk encryption (FileVault, BitLocker). Onboarding reads `fdesetup status` on macOS and shows a non-blocking hint if FileVault is off.
  - Per-user `0700`/`0600` permissions in Application Support.
  - The DB directory is excluded from backups (`NSURLIsExcludedFromBackupKey`): the cache can be resynced, so backups would only widen the leak surface.
  - Sign-out or "Forget org" deletes the org directory (`.sqlite`, `-wal`, `-shm`, `.bak`).
  - Logs never contain entity text.
- **Upgrade path (prepared, not built):**
  - `rusqlite` feature `bundled-sqlcipher`. On macOS it links the system Security framework, so it needs no OpenSSL. On Windows it needs `bundled-sqlcipher-vendored-openssl`.
  - A random 256-bit key goes in the keychain next to the PAT and is applied with `PRAGMA key` at open.
  - Zetetic reports [5-15% typical overhead](https://www.zetetic.net/sqlcipher/performance/).
  - Existing files convert with `sqlcipher_export()`, or by resyncing the cache plus copying the local tables.
  - `Store::open` takes the key as an optional argument from day one, so turning encryption on changes no callers.
- **Trigger:** decide before v0.3 (Windows, where BitLocker is not guaranteed on Home editions) or on the first org or security-review request, whichever comes first. Gate B6 must show ≤ 15% regression.

## Consequences

### Positive

- One boring, proven storage engine, the same one zeron uses. The whole store is inspectable with the `sqlite3` CLI.
- Atomic page commits make sync crash-safe and notifications exactly-once without extra machinery.
- Cmd+P speed doesn't depend on disk. Typo tolerance is real (substitutions and transpositions), not just subsequence matching.
- The in-memory index has no persistence of its own to corrupt; it is rebuilt from SQLite in milliseconds.

### Negative / costs

- **Memory.** The tier-1 corpus lives in RAM: about 40 MB of haystack strings at 500k documents, about 8 MB at 100k, plus frizbee scratch space. Measured in B3.
- **Pre-1.0 dependency.** frizbee (0.13) is pre-1.0. Mitigations: pinned exact version, a local `search::query` seam, and `nucleo-matcher` as a drop-in fallback.
- **No typo tolerance for long text in v0.1.** Body search is word and prefix only.
- **Polling-bound freshness for threads and mentions.** No push, no "updated since" for PR threads; the 30-minute outer ring is a real latency.
- **Plaintext DB** until the encryption trigger fires. We rely on FDE.

### Revisit triggers

- **Tantivy:** users report body-text misses that typo tolerance would fix, or FTS5 tier-2 p95 exceeds 50 ms at 500k docs with real comment bodies (B2).
- **Trigram FTS:** users need infix search inside identifiers in bodies (CamelCase, paths) that `unicode61` can't split. First try `detail=none` trigram limited to `title`, and measure size.
- **Vector search (no earlier than v0.4):** an accepted spec needs semantic features ("related PRs", duplicate-bug detection, natural-language queries). Plan of record: `sqlite-vec` in the same file with int8 or binary quantization (500k × 384-dim int8 ≈ 190 MB; binary ≈ 24 MB) and locally computed embeddings. Prerequisites: sqlite-vec reaches 1.0 or gets pinned, KNN at 500k measures ≤ 50 ms, and the embedding model's size and privacy story is accepted.
- **DuckDB:** an analytics or dashboard spec. Attach read-only through its SQLite scanner; don't migrate.

## Benchmark plan

Harness:

- `crates/store/benches/` and `crates/search/benches/` with `criterion` 0.8.2 (dev-dependency).
- A seeded synthetic generator ported from the throwaway harness: corpus shape as in [Evidence](#evidence-local-benchmark), plus bodies and comments drawn from a length distribution (median 300 B, p99 8 KB).
- A **real-shape dataset**: an opt-in export from the dogfood org, with text replaced by same-length tokens from a fixed vocabulary, so ids, lengths, and distributions are real and contents are not.

Reference machines:

- M4 Pro (dev)
- **Floor:** 8 GB M1 MacBook Air
- Windows 4-core laptop (added at v0.3)

| Gate | Measures | Pass criteria |
|---|---|---|
| **B1 Cmd+P latency** | Simulated typing of 30 golden queries character by character (`c`, `ch`, `che`, …), including typo variants, on 100k and 500k corpora. Reports p50/p95/p99 per keystroke, including narrowing and ranking. | 100k on the floor: p95 ≤ 16 ms, p99 ≤ 50 ms. 500k on the floor: p95 ≤ 50 ms. |
| **B2 Long-text search** | FTS5 prefix-AND + `rank` + `snippet()` on 500k `search_doc` rows with bodies; 1-, 2-, and 4-term queries, short-prefix worst cases. | p95 ≤ 50 ms at 500k on the floor. |
| **B3 Cold start and memory** | Open + migrations (no-op) + first-screen queries (inbox, my PRs, my WIs); separately, corpus load into the tier-1 index; RSS after load. | First-screen data ≤ 100 ms (leaves 400 ms for the UI). Index ready ≤ 150 ms at 500k. RSS for index ≤ 100 MB at 500k. |
| **B4 Sync write throughput** | Upsert pages of 200 and 1,000 mixed entities, with FTS triggers, mention extraction, and notification diff, into a 500k DB. Measures commit latency and reader latency while the writer runs. | 1,000-row page commit ≤ 150 ms. Concurrent B1 p95 unchanged ±10%. |
| **B5 API cost** | Against the dogfood org: requests per cycle and `X-RateLimit-*` headroom for the cursor table above (hot-set sizes 10, 50, 200; active PRs 50, 300). | Steady state uses ≤ 25% of the 200 TSTU / 5 min budget. Sets the final cadence and the 300-PR cap. |
| **B6 SQLCipher delta** | B1-B4 rerun with `bundled-sqlcipher` (run before the encryption decision). | ≤ 15% regression on B2-B4. B1 unaffected (in-memory). |
| **Relevance** | Top-1 and MRR@5 on the 30 golden queries (labelled expected targets), frizbee vs nucleo-matcher, with and without boosts. | Top-1 ≥ 0.8 and MRR@5 ≥ 0.9 before v0.1 ships. Regressions block merges. |

B1-B4 run in CI on every PR that touches `crates/store`, `crates/sync`, or `crates/search`, at 100k on the CI runner. A regression of more than 10% against the baseline stored on `main` fails the check. Floor-machine and 500k runs happen before each release, and the results are recorded in the release notes.

## References

- SQLite: [FTS5](https://sqlite.org/fts5.html) (trigram tokenizer §4.3.4, external content §4.4.3, detail option §4.6), [WAL](https://sqlite.org/wal.html), [ATTACH atomicity](https://sqlite.org/lang_attach.html), [ALTER TABLE](https://sqlite.org/lang_altertable.html)
- [rusqlite README: bundled / SQLCipher features](https://github.com/rusqlite/rusqlite), [SQLCipher performance](https://www.zetetic.net/sqlcipher/performance/)
- [frizbee](https://github.com/saghen/frizbee) and its [benchmarks](https://github.com/saghen/frizbee/blob/main/BENCHMARKS.md), [nucleo](https://github.com/helix-editor/nucleo)
- [Tantivy FuzzyTermQuery](https://docs.rs/tantivy/0.26.2/tantivy/query/struct.FuzzyTermQuery.html), [redb](https://github.com/cberner/redb), [sled](https://github.com/spacejam/sled), [DuckDB FTS](https://duckdb.org/docs/current/core_extensions/full_text_search.html), [sqlite-vec](https://github.com/asg017/sqlite-vec)
- Azure DevOps REST 7.1: [reporting work item revisions](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/reporting-work-item-revisions/read-reporting-revisions-get?view=azure-devops-rest-7.1), [WIQL](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/wiql/query-by-wiql?view=azure-devops-rest-7.1), [work items batch](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-items-batch?view=azure-devops-rest-7.1), [WI comments](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/comments/get-comments?view=azure-devops-rest-7.1-preview.4), [pull requests by project](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-requests-by-project?view=azure-devops-rest-7.1), [PR threads](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/list?view=azure-devops-rest-7.1), [builds list](https://learn.microsoft.com/en-us/rest/api/azure/devops/build/builds/list?view=azure-devops-rest-7.1), [pipeline runs](https://learn.microsoft.com/en-us/rest/api/azure/devops/pipelines/runs/list?view=azure-devops-rest-7.1), [query macros](https://learn.microsoft.com/en-us/azure/devops/boards/queries/query-operators-variables?view=azure-devops), [object limits](https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/object-limits?view=azure-devops#work-items-and-queries), [rate limits](https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops)
- Reference implementation: zeron `crates/sync/src/store.rs` (https://github.com/zeronsh/zeron) (rusqlite + WAL pattern; MIT, @winglee)
