-- v0.0.5 review writes (spec 0004 FR-12): auto-complete guard.

-- ── local (user-authored; migrations must preserve) ─────────────────── local
-- PRs where I set auto-complete in adoken, with the source commit I had reviewed when I set
-- it. Sync emits `PrPushed` for these PRs (spec 0002 FR-11) and notify raises the P1
-- "new commits after you set auto-complete" alert while the pushed commit differs. Cleared
-- when I cancel auto-complete or the PR leaves the active state.
CREATE TABLE pr_auto_complete (
  pr_id INTEGER PRIMARY KEY,
  source_commit TEXT NOT NULL,
  set_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
