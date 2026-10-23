-- Spec 0004 FR-1..FR-3: review files of real PRs (verified against live Azure DevOps 2026-09-30).

-- ── synced ─────────────────────────────────────────────────────────────── cache
-- Which iteration `pr_file` describes: the latest iteration's changes against the merge base
-- (`iterations/{it}/changes?$compareTo=0`), so `pr_file.old_blob` is the merge-base blob and
-- `new_blob` the source blob. Sync refetches the list when the PR's `last_source_commit` moves
-- past `source_commit` (a push). `base_commit` is the iteration's `commonRefCommit`.
CREATE TABLE pr_file_sync (
  pr_id INTEGER PRIMARY KEY REFERENCES pull_request(id) ON DELETE CASCADE,
  iteration INTEGER NOT NULL,
  source_commit TEXT NOT NULL,
  base_commit TEXT,
  synced_ms INTEGER NOT NULL
) STRICT;

-- File contents moved to the content-addressed file cache beside the store
-- (`<store dir>/blobs/`, 500 MB LRU, `adoken_store::BlobCache`): a store commit wakes every
-- view, and a blob fetch or LRU touch must not. `Store::open` copies the old rows over first.
DROP TABLE blob;
