-- Riptide schema v2: queues grow a schedule and a post-queue action, and downloads get a
-- manual position inside their queue (drag-reorder).
-- Applied inside one transaction by the migration runner; must not contain BEGIN/COMMIT.

-- JSON of riptide_core::Schedule; NULL = unscheduled.
ALTER TABLE queues ADD COLUMN schedule TEXT;
-- JSON of riptide_core::PostAction; NULL = nothing to do when the queue drains.
ALTER TABLE queues ADD COLUMN on_done TEXT;

-- Smaller runs first among equal priorities. Existing rows keep their old order (added_at, id).
ALTER TABLE downloads ADD COLUMN queue_pos INTEGER NOT NULL DEFAULT 0;
UPDATE downloads SET queue_pos = id;

DROP INDEX idx_downloads_queue;
CREATE INDEX idx_downloads_queue ON downloads (queue_id, queue_pos);
