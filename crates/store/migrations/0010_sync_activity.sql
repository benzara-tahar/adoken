-- Spec 0016 FR-10: the last successful pass per scheduler lane, so the
-- Activity drawer's rows show real "last synced" times after a restart,
-- while paused or offline at launch. Local only: never synced.
CREATE TABLE lane_sync (
    lane        TEXT PRIMARY KEY,
    last_ok_ms  INTEGER NOT NULL
) WITHOUT ROWID;
