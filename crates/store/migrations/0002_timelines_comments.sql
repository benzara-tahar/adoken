-- v0.0.3 follow-up (spec 0002 FR-1, spec 0008 FR-5): build timelines and work item
-- discussions come from sync.

-- ── run stages ──────────────────────────────────────────────────────── cache
-- Was owned by the app (`migrate_ext("pipelines")`); v0.0.3 stores already have it,
-- hence IF NOT EXISTS. Sync writes it from the build timeline, the demo org seeds it.
CREATE TABLE IF NOT EXISTS pipelines_stage (
  run_id INTEGER NOT NULL,
  ord INTEGER NOT NULL,
  name TEXT NOT NULL,
  outcome TEXT NOT NULL,
  started_ms INTEGER,
  finished_ms INTEGER,
  PRIMARY KEY (run_id, ord)
) STRICT, WITHOUT ROWID;
DELETE FROM ext_migration WHERE ns = 'pipelines';

-- Run state (`status:result:finished_ms`) the stored stages were fetched for; NULL = never,
-- 'live' = fetched while the run was still going (always due again).
ALTER TABLE pipeline_run ADD COLUMN timeline_fp TEXT;
CREATE INDEX run_timeline_due ON pipeline_run(queued_ms DESC)
  WHERE timeline_fp IS NOT (status || ':' || coalesce(result, '') || ':' || coalesce(finished_ms, ''));

-- ── work item discussions ─────────────────────────────────────────────── cache
-- 1 = the item's comments must be (re)fetched; set by sync for "mine" items whose
-- comment count changed, cleared after the fetch.
ALTER TABLE work_item ADD COLUMN comments_due INTEGER NOT NULL DEFAULT 0;
CREATE INDEX wi_comments_due ON work_item(changed_ms DESC) WHERE comments_due = 1;

-- Existing stores: my items with comments get their discussion fetched, and the next
-- `wi.hot` pass refetches every one of my items (rev guard reset) so stored descriptions
-- are re-rendered by the current HTML → text conversion.
UPDATE work_item SET comments_due = 1
  WHERE comment_count > 0 AND is_deleted = 0
    AND (assigned_to_id IN (SELECT id FROM identity WHERE is_me = 1)
      OR created_by_id IN (SELECT id FROM identity WHERE is_me = 1));
UPDATE work_item SET rev = 0 WHERE description IS NOT NULL;
UPDATE sync_cursor SET high_water = NULL WHERE scope = 'wi.hot';
