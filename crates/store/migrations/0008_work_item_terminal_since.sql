ALTER TABLE work_item ADD COLUMN terminal_since_ms INTEGER;
UPDATE work_item SET terminal_since_ms = changed_ms
WHERE state_category IN ('Completed', 'Removed');
