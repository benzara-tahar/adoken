-- ADR 0001 §4 schema. Timestamps are INTEGER Unix ms UTC. Natural ADO ids are keys.
-- "cache" tables mirror ADO and may be dropped by resync migrations;
-- "local" tables are user-authored and must be preserved.

-- ── meta ───────────────────────────────────────────────────────────── cache
CREATE TABLE sync_cursor (
  scope         TEXT NOT NULL,
  project_id    TEXT NOT NULL DEFAULT '',
  token         TEXT,
  high_water    TEXT,
  backfill_to   TEXT,
  baseline_done INTEGER NOT NULL DEFAULT 0,
  last_ok_ms    INTEGER,
  last_error    TEXT,
  PRIMARY KEY (scope, project_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT, WITHOUT ROWID;

CREATE TABLE org (id TEXT PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL) STRICT;

CREATE TABLE identity (
  id TEXT PRIMARY KEY, display_name TEXT NOT NULL, unique_name TEXT,
  is_me INTEGER NOT NULL DEFAULT 0, is_group INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE project (id TEXT PRIMARY KEY, name TEXT NOT NULL, synced INTEGER NOT NULL DEFAULT 1) STRICT;
CREATE TABLE repo (
  id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  name TEXT NOT NULL, default_branch TEXT
) STRICT;
CREATE INDEX repo_by_project ON repo(project_id);

-- ── pull requests ──────────────────────────────────────────────────── cache
CREATE TABLE pull_request (
  id            INTEGER PRIMARY KEY,
  repo_id       TEXT NOT NULL REFERENCES repo(id) ON DELETE CASCADE,
  project_id    TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  title         TEXT NOT NULL,
  description   TEXT NOT NULL DEFAULT '',
  status        TEXT NOT NULL,
  is_draft      INTEGER NOT NULL,
  author_id     TEXT NOT NULL REFERENCES identity(id),
  source_ref    TEXT NOT NULL, target_ref TEXT NOT NULL,
  merge_status  TEXT NOT NULL, last_source_commit TEXT,
  iterations    INTEGER NOT NULL DEFAULT 0, last_push_ms INTEGER,
  created_ms    INTEGER NOT NULL, updated_ms INTEGER NOT NULL, closed_ms INTEGER,
  fingerprint   INTEGER NOT NULL,
  hydrated_ms   INTEGER,
  extra         BLOB
) STRICT;
CREATE INDEX pr_by_status  ON pull_request(status, updated_ms DESC);
CREATE INDEX pr_by_author  ON pull_request(author_id, status, updated_ms DESC);
CREATE INDEX pr_by_repo    ON pull_request(repo_id, status);

CREATE TABLE pr_reviewer (
  pr_id INTEGER NOT NULL REFERENCES pull_request(id) ON DELETE CASCADE,
  identity_id TEXT NOT NULL REFERENCES identity(id),
  vote INTEGER NOT NULL, is_required INTEGER NOT NULL, has_declined INTEGER NOT NULL DEFAULT 0,
  added_ms INTEGER, voted_ms INTEGER,
  ord INTEGER NOT NULL,
  PRIMARY KEY (pr_id, identity_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX pr_reviewer_by_identity ON pr_reviewer(identity_id, pr_id);

CREATE TABLE pr_iteration (
  pr_id INTEGER NOT NULL REFERENCES pull_request(id) ON DELETE CASCADE,
  id INTEGER NOT NULL,
  source_commit TEXT NOT NULL, created_ms INTEGER NOT NULL, description TEXT,
  PRIMARY KEY (pr_id, id)
) STRICT, WITHOUT ROWID;

CREATE TABLE pr_file (
  pr_id INTEGER NOT NULL REFERENCES pull_request(id) ON DELETE CASCADE,
  ord INTEGER NOT NULL,
  path TEXT NOT NULL, original_path TEXT, change_type TEXT NOT NULL,
  old_blob TEXT, new_blob TEXT,
  PRIMARY KEY (pr_id, ord)
) STRICT, WITHOUT ROWID;

-- Content-addressed file contents for diffs (git object id → text).
CREATE TABLE blob (id TEXT PRIMARY KEY, content TEXT NOT NULL) STRICT, WITHOUT ROWID;

CREATE TABLE pr_thread (
  pr_id INTEGER NOT NULL REFERENCES pull_request(id) ON DELETE CASCADE,
  id INTEGER NOT NULL,
  status TEXT NOT NULL,
  file_path TEXT,
  right_start INTEGER, right_end INTEGER, left_start INTEGER, left_end INTEGER,
  iteration INTEGER, iteration_blob TEXT,
  updated_ms INTEGER NOT NULL,
  PRIMARY KEY (pr_id, id)
) STRICT, WITHOUT ROWID;

-- ── work items ─────────────────────────────────────────────────────── cache
CREATE TABLE work_item (
  id INTEGER PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  rev INTEGER NOT NULL,
  type TEXT NOT NULL, title TEXT NOT NULL, state TEXT NOT NULL, state_category TEXT NOT NULL,
  assigned_to_id TEXT REFERENCES identity(id),
  created_by_id TEXT NOT NULL REFERENCES identity(id),
  changed_by_id TEXT NOT NULL REFERENCES identity(id),
  area_path TEXT NOT NULL, iteration_path TEXT NOT NULL, priority INTEGER, tags TEXT NOT NULL DEFAULT '',
  parent_id INTEGER,
  description TEXT,
  comment_count INTEGER NOT NULL DEFAULT 0,
  created_ms INTEGER NOT NULL, changed_ms INTEGER NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0,
  extra BLOB
) STRICT;
CREATE INDEX wi_by_assignee ON work_item(assigned_to_id, state_category, changed_ms DESC);
CREATE INDEX wi_by_changed  ON work_item(changed_ms DESC);
CREATE INDEX wi_by_project  ON work_item(project_id, changed_ms DESC);

CREATE TABLE wi_pr_link (
  work_item_id INTEGER NOT NULL REFERENCES work_item(id) ON DELETE CASCADE,
  pr_id INTEGER NOT NULL,
  PRIMARY KEY (work_item_id, pr_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX wi_pr_link_by_pr ON wi_pr_link(pr_id);

-- ── comments (PR + WI unified: one FTS path, one mention extractor) ── cache
CREATE TABLE comment (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL CHECK (source IN ('pr', 'wi')),
  parent_id INTEGER NOT NULL,
  thread_id INTEGER NOT NULL DEFAULT 0,
  ado_comment_id INTEGER NOT NULL,
  reply_to INTEGER,
  author_id TEXT NOT NULL REFERENCES identity(id),
  body TEXT NOT NULL,
  is_system INTEGER NOT NULL DEFAULT 0,
  created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0,
  UNIQUE (source, parent_id, thread_id, ado_comment_id)
) STRICT;

CREATE TABLE mention (
  comment_id INTEGER NOT NULL REFERENCES comment(id) ON DELETE CASCADE,
  identity_id TEXT NOT NULL,
  PRIMARY KEY (comment_id, identity_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX mention_by_identity ON mention(identity_id);

-- ── pipelines ──────────────────────────────────────────────────────── cache
CREATE TABLE pipeline (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  id INTEGER NOT NULL, name TEXT NOT NULL, folder TEXT NOT NULL DEFAULT '\',
  PRIMARY KEY (project_id, id)
) STRICT, WITHOUT ROWID;

CREATE TABLE pipeline_run (
  id INTEGER PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  definition_id INTEGER NOT NULL, definition_name TEXT NOT NULL,
  run_number TEXT NOT NULL,
  status TEXT NOT NULL, result TEXT, reason TEXT NOT NULL,
  source_branch TEXT NOT NULL, source_version TEXT,
  requested_for_id TEXT REFERENCES identity(id),
  pr_id INTEGER,
  queued_ms INTEGER NOT NULL, started_ms INTEGER, finished_ms INTEGER
) STRICT;
CREATE INDEX run_by_pr     ON pipeline_run(pr_id) WHERE pr_id IS NOT NULL;
CREATE INDEX run_by_user   ON pipeline_run(requested_for_id, queued_ms DESC);
CREATE INDEX run_by_queued ON pipeline_run(queued_ms DESC);
CREATE INDEX run_by_branch ON pipeline_run(source_branch, queued_ms DESC);

-- ── search corpus (maintained in the same transaction as entity upserts) ── cache
CREATE TABLE search_doc (
  id INTEGER PRIMARY KEY,
  kind INTEGER NOT NULL,
  entity_key TEXT NOT NULL UNIQUE,
  doc_id INTEGER NOT NULL,
  project_id TEXT,
  title TEXT NOT NULL,
  subtitle TEXT NOT NULL,
  body TEXT,
  updated_ms INTEGER NOT NULL
) STRICT;
CREATE VIRTUAL TABLE search_fts USING fts5(
  title, body,
  content = 'search_doc', content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3'
);
CREATE TRIGGER search_doc_ai AFTER INSERT ON search_doc BEGIN
  INSERT INTO search_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;
CREATE TRIGGER search_doc_ad AFTER DELETE ON search_doc BEGIN
  INSERT INTO search_fts(search_fts, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
END;
CREATE TRIGGER search_doc_au AFTER UPDATE ON search_doc BEGIN
  INSERT INTO search_fts(search_fts, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
  INSERT INTO search_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;

-- ── sync events (spec 0002 FR-11), persisted, deduplicated ─────────── local
CREATE TABLE sync_event (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  subject_key TEXT NOT NULL,
  dedupe_key TEXT NOT NULL,
  actor_id TEXT,
  self_caused INTEGER NOT NULL,
  at_ms INTEGER NOT NULL,
  summary TEXT NOT NULL,
  UNIQUE (kind, subject_key, dedupe_key)
) STRICT;

-- ── local (user-authored; migrations must preserve) ─────────────────── local
CREATE TABLE view_state (
  entity_key TEXT PRIMARY KEY, last_viewed_ms INTEGER NOT NULL, seen_rev INTEGER
) STRICT, WITHOUT ROWID;

-- Forward-only migrations owned by other crates (notify, nudges): Store::migrate_ext.
CREATE TABLE ext_migration (ns TEXT PRIMARY KEY, version INTEGER NOT NULL) STRICT, WITHOUT ROWID;
