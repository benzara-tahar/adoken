-- Spec 0017 FR-4: commits per push. `commit_count` = commits of the
-- iteration that the previous iteration didn't list; NULL = synced before
-- this migration (unknown). `commits_truncated` = Azure DevOps cut the list
-- short (`hasMoreCommits`): the count is a lower bound.
ALTER TABLE pr_iteration ADD COLUMN commit_count INTEGER;
ALTER TABLE pr_iteration ADD COLUMN commits_truncated INTEGER NOT NULL DEFAULT 0;

-- Spec 0017 Loose ends "Open Nd" (owner override of Q-2): when the work item
-- entered its current state (`System.StateChangeDate`). NULL = unknown;
-- readers fall back to `changed_ms`.
ALTER TABLE work_item ADD COLUMN state_since_ms INTEGER;
