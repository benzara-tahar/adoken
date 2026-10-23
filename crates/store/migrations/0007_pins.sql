-- Spec 0014 Pins.

-- ── local (user-authored; migrations must preserve) ─────────────────── local
-- The user's pins (FR-15): no foreign keys into cache tables, so dropping or
-- rebuilding a cache table never cascades into them. `subject_key` is the
-- palette's entity key (`pr:4821`, `wi:48213`, `pipe:<project id>:<definition id>`).
-- `title` is the last known Azure DevOps title (kept for "Not available" rows,
-- FR-13), `label` the local rename (FR-14), `seen_result` a definition's latest
-- completed run result when last opened (FR-10), `unavailable` the 404 / 403
-- reason sync recorded.
CREATE TABLE pin (
  subject_key TEXT PRIMARY KEY,
  position INTEGER NOT NULL,
  label TEXT,
  title TEXT NOT NULL DEFAULT '',
  pinned_ms INTEGER NOT NULL,
  opened_ms INTEGER,
  seen_result TEXT,
  unavailable TEXT
) STRICT, WITHOUT ROWID;
CREATE INDEX pin_by_position ON pin(position);

-- Opens from the pins (sidebar, rail, ⌃n, ⌘J), counted on this Mac for the ⌘J
-- footer stat ("Opened from pins 41× this week", §8); pruned after 7 days.
CREATE TABLE pin_open (at_ms INTEGER NOT NULL) STRICT;
CREATE INDEX pin_open_by_time ON pin_open(at_ms);

-- The change dot (FR-10) reads a subject's events since the pin was opened.
CREATE INDEX sync_event_by_subject ON sync_event(subject_key, at_ms);

-- ── synced ─────────────────────────────────────────────────────────────── cache
-- A pinned definition's headline is its latest run (FR-9); its change dot the
-- latest completed run's result (FR-10).
CREATE INDEX run_by_definition ON pipeline_run(project_id, definition_id, queued_ms DESC);
