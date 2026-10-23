-- Spec 0004 FR-6, FR-10 (verified against live Azure DevOps 2026-09-30).

-- ── synced ───────────────────────────────────────────────────────────────
-- `changeEntries[].changeTrackingId` of the displayed iteration. New line comments send it
-- so Azure DevOps tracks them across later pushes. NULL = unknown (never sent).
ALTER TABLE pr_file ADD COLUMN change_tracking_id INTEGER;

-- ── local (user-authored; migrations must preserve) ─────────────────── local
-- PRs whose Complete Azure DevOps accepted but still merges asynchronously (the PATCH answers
-- `status: active, mergeStatus: queued`). The review shows "Completing…" and blocks another
-- Complete / auto-complete while the merge is queued. Cleared when sync sees the PR leave the
-- active state or the queued merge fail (`conflicts`, `failure`, `rejectedByPolicy`).
CREATE TABLE pr_completing (
  pr_id INTEGER PRIMARY KEY,
  source_commit TEXT NOT NULL,
  set_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
