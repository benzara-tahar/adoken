-- Spec 0015 work item detail and editing (FR-32, FR-33, FR-34; ADR 0001 amendment).

-- ── work items ───────────────────────────────────────────────────────── cache
-- `description` stays the plain-text projection (search, previews); the raw source and its
-- format (`html` | `markdown`, Azure DevOps' `multilineFieldsFormat`) feed the rich view.
-- `synced_ms`: the row was last written from an Azure DevOps response (any lane, a write's
-- answer, a per-item sync). `full_synced_ms`: full fields and relations were applied (NULL =
-- title-index row only: "Partly synced").
ALTER TABLE work_item ADD COLUMN description_source TEXT;
ALTER TABLE work_item ADD COLUMN description_format TEXT;
ALTER TABLE work_item ADD COLUMN synced_ms INTEGER;
ALTER TABLE work_item ADD COLUMN full_synced_ms INTEGER;
-- `System.Reason` as Azure DevOps set it (by rule, on a state change); full answers only.
ALTER TABLE work_item ADD COLUMN reason TEXT;

-- Other long-text fields shown read-only under the description (Q-10): Repro Steps
-- (`Microsoft.VSTS.TCM.ReproSteps`) and Acceptance Criteria
-- (`Microsoft.VSTS.Common.AcceptanceCriteria`).
CREATE TABLE wi_text_field (
  work_item_id INTEGER NOT NULL REFERENCES work_item(id) ON DELETE CASCADE,
  field TEXT NOT NULL,
  source TEXT NOT NULL,
  format TEXT NOT NULL,
  PRIMARY KEY (work_item_id, field)
) STRICT, WITHOUT ROWID;

-- Every relation in server order (`ord` is the index `remove /relations/{index}` takes), with
-- the revision it was read at. `target_id`: a work item link's target; `pr_id`: a pull request
-- artifact link's PR.
CREATE TABLE wi_link (
  work_item_id INTEGER NOT NULL REFERENCES work_item(id) ON DELETE CASCADE,
  ord INTEGER NOT NULL,
  rel TEXT NOT NULL,
  url TEXT NOT NULL,
  target_id INTEGER,
  pr_id INTEGER,
  name TEXT,
  comment TEXT,
  rev INTEGER NOT NULL,
  PRIMARY KEY (work_item_id, ord)
) STRICT, WITHOUT ROWID;
CREATE INDEX wi_link_by_target ON wi_link(target_id) WHERE target_id IS NOT NULL;

-- ── comments ─────────────────────────────────────────────────────────── cache
-- Work item comments keep Azure DevOps' raw text and format (`html` | `markdown`); `body`
-- stays the plain projection with `@<guid>` mentions.
ALTER TABLE comment ADD COLUMN body_source TEXT;
ALTER TABLE comment ADD COLUMN format TEXT;

-- ── metadata (FR-33: fetched on first use, 24 h) ──────────────────────── cache
-- Per (project, type): when its states/transitions and its fields were fetched.
CREATE TABLE wi_type_meta (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  states_ms INTEGER,
  fields_ms INTEGER,
  PRIMARY KEY (project_id, type)
) STRICT, WITHOUT ROWID;

CREATE TABLE wi_type_state (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  ord INTEGER NOT NULL,
  name TEXT NOT NULL,
  category TEXT NOT NULL,
  color TEXT,
  PRIMARY KEY (project_id, type, ord)
) STRICT, WITHOUT ROWID;

CREATE TABLE wi_type_transition (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  from_state TEXT NOT NULL,
  ord INTEGER NOT NULL,
  to_state TEXT NOT NULL,
  PRIMARY KEY (project_id, type, from_state, ord)
) STRICT, WITHOUT ROWID;

-- Fields of a type (FR-9, only after a rule error). `kind`: `list` (with `allowed`, one value
-- per line), `text`, `person`, `unknown`; NULL until resolved.
CREATE TABLE wi_type_field (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  reference TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT,
  allowed TEXT,
  PRIMARY KEY (project_id, type, reference)
) STRICT, WITHOUT ROWID;

-- Area and iteration trees per project, pre-order (`ord`); iteration dates in ms.
CREATE TABLE wi_class_tree (
  project_id TEXT PRIMARY KEY REFERENCES project(id) ON DELETE CASCADE,
  fetched_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE wi_class_node (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('area', 'iteration')),
  ord INTEGER NOT NULL,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  depth INTEGER NOT NULL,
  start_ms INTEGER,
  finish_ms INTEGER,
  PRIMARY KEY (project_id, kind, ord)
) STRICT, WITHOUT ROWID;

-- ── local (user-authored; migrations must preserve) ─────────────────── local
-- Unsent description and comment text (FR-20), one per item and kind (`description` |
-- `comment`), with the revision the edit started from. No foreign keys into cache tables:
-- a cache rebuild keeps them.
CREATE TABLE wi_draft (
  work_item_id INTEGER NOT NULL,
  kind TEXT NOT NULL,
  body TEXT NOT NULL,
  base_rev INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL,
  PRIMARY KEY (work_item_id, kind)
) STRICT, WITHOUT ROWID;

-- People I assigned in adoken (FR-13), with what the picker shows and sends, so the list
-- survives a cache rebuild.
CREATE TABLE wi_recent_assignee (
  identity_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  unique_name TEXT,
  used_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX wi_recent_assignee_by_use ON wi_recent_assignee(used_ms DESC);

-- ── backfill (FR-34) ─────────────────────────────────────────────────── cache
-- Like 0002: the next `wi.hot` pass refetches every one of my items (their raw description,
-- relations and freshness land then), and my items' discussions are fetched once more for
-- their raw text. Pinned items refresh on every pass anyway.
UPDATE work_item SET comments_due = 1
  WHERE comment_count > 0 AND is_deleted = 0
    AND (assigned_to_id IN (SELECT id FROM identity WHERE is_me = 1)
      OR created_by_id IN (SELECT id FROM identity WHERE is_me = 1));
UPDATE sync_cursor SET high_water = NULL WHERE scope = 'wi.hot';
