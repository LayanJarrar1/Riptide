-- Riptide schema v6: time spent downloading, for the duration and average-speed columns.
-- `active_ms` is NULL for every row that exists before this migration: their time was never
-- recorded, and an average made up from added/completed timestamps would include paused and
-- queued time. `active_since` is set while the download is in `downloading` (the start of the
-- open span, moved forward at each checkpoint). Applied in one transaction; no BEGIN/COMMIT.

ALTER TABLE downloads ADD COLUMN active_ms INTEGER;
ALTER TABLE downloads ADD COLUMN active_since INTEGER;
