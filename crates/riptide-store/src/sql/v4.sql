-- Riptide schema v4: "have I downloaded this already?" index (alpha.5 AI/dedup lane).
-- Self-contained on purpose (no foreign key to `downloads`): the memory of a finished file
-- survives removing the row from the list. Applied in one transaction; no BEGIN/COMMIT.

CREATE TABLE seen_downloads (
    download_id  INTEGER PRIMARY KEY,
    title        TEXT    NOT NULL,
    filename     TEXT    NOT NULL,
    dir          TEXT    NOT NULL,
    size         INTEGER,
    sha256       TEXT,
    completed_at INTEGER NOT NULL,
    -- 1 when the file was not found at `dir/filename` the last time it was looked for
    stale        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_seen_downloads_sha ON seen_downloads (sha256);

CREATE TABLE seen_keys (
    key         TEXT    NOT NULL,
    download_id INTEGER NOT NULL REFERENCES seen_downloads (download_id) ON DELETE CASCADE,
    PRIMARY KEY (key, download_id)
);
CREATE INDEX idx_seen_keys_download ON seen_keys (download_id);
