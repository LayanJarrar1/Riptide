-- Riptide schema v4: BitTorrent downloads.
--
-- 1. `downloads.kind` may now be 'torrent'. SQLite cannot change a CHECK constraint in place, so
--    the table is rebuilt the documented way (new table, copy, drop, rename). The runner turns
--    foreign keys off around the whole migration (they cannot be switched inside a transaction),
--    so dropping the old table does not cascade into `segments` / `secrets`, and it runs
--    PRAGMA foreign_key_check afterwards. Every row keeps its id.
-- 2. `torrents` holds what only a torrent has: info-hash, planned files, selection, seeding flag
--    and the lifetime upload / verified-download totals.
-- Applied inside one transaction; must not contain BEGIN/COMMIT.

CREATE TABLE downloads_v4 (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    url                  TEXT    NOT NULL,
    final_url            TEXT,
    filename             TEXT    NOT NULL,
    dir                  TEXT    NOT NULL,
    size                 INTEGER CHECK (size IS NULL OR size >= 0),
    supports_ranges      INTEGER NOT NULL DEFAULT 0 CHECK (supports_ranges IN (0, 1)),
    etag                 TEXT,
    last_modified        TEXT,
    content_type         TEXT,
    state                TEXT    NOT NULL
                         CHECK (state IN ('queued','probing','downloading','paused',
                                          'verifying','completed','failed','cancelled')),
    retryable            INTEGER CHECK (retryable IS NULL OR retryable IN (0, 1)),
    category_id          INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    queue_id             INTEGER NOT NULL DEFAULT 1 REFERENCES queues(id) ON DELETE SET DEFAULT,
    priority             TEXT    NOT NULL DEFAULT 'normal' CHECK (priority IN ('low','normal','high')),
    speed_limit          INTEGER CHECK (speed_limit IS NULL OR speed_limit > 0),
    connections          INTEGER NOT NULL DEFAULT 8 CHECK (connections BETWEEN 1 AND 32),
    checksum_algo        TEXT    CHECK (checksum_algo IS NULL OR checksum_algo IN ('md5','sha1','sha256')),
    checksum_expected    TEXT,
    checksum_actual_algo TEXT    CHECK (checksum_actual_algo IS NULL OR checksum_actual_algo IN ('md5','sha1','sha256')),
    checksum_actual      TEXT,
    error                TEXT,
    added_at             INTEGER NOT NULL,
    started_at           INTEGER,
    completed_at         INTEGER,
    kind                 TEXT    NOT NULL DEFAULT 'http' CHECK (kind IN ('http', 'media', 'torrent')),
    media_format         TEXT,
    queue_pos            INTEGER NOT NULL DEFAULT 0
);

INSERT INTO downloads_v4 (id, url, final_url, filename, dir, size, supports_ranges, etag,
    last_modified, content_type, state, retryable, category_id, queue_id, priority, speed_limit,
    connections, checksum_algo, checksum_expected, checksum_actual_algo, checksum_actual, error,
    added_at, started_at, completed_at, kind, media_format, queue_pos)
SELECT id, url, final_url, filename, dir, size, supports_ranges, etag,
    last_modified, content_type, state, retryable, category_id, queue_id, priority, speed_limit,
    connections, checksum_algo, checksum_expected, checksum_actual_algo, checksum_actual, error,
    added_at, started_at, completed_at, kind, media_format, queue_pos
FROM downloads;

-- The AUTOINCREMENT high-water mark belongs to the old table and would be lost with it: ids of
-- downloads removed before the upgrade must never come back (the UI keys caches and the "already
-- downloaded" hints by id). The copy above left a row for downloads_v4 only up to the highest
-- id still present.
DELETE FROM sqlite_sequence WHERE name = 'downloads_v4';
INSERT INTO sqlite_sequence (name, seq)
    SELECT 'downloads_v4', seq FROM sqlite_sequence WHERE name = 'downloads';

DROP TABLE downloads;
ALTER TABLE downloads_v4 RENAME TO downloads;

CREATE INDEX idx_downloads_state ON downloads (state);
CREATE INDEX idx_downloads_queue ON downloads (queue_id, queue_pos);

CREATE TABLE torrents (
    download_id INTEGER PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
    info_hash   TEXT    NOT NULL UNIQUE CHECK (length(info_hash) = 40),
    name        TEXT    NOT NULL,
    files       TEXT    NOT NULL,                       -- JSON [{index, path, len}]
    selected    TEXT,                                   -- JSON [index, ...]; NULL = all files
    sequential  INTEGER NOT NULL DEFAULT 0 CHECK (sequential IN (0, 1)),
    seeding     INTEGER NOT NULL DEFAULT 0 CHECK (seeding IN (0, 1)),
    uploaded    INTEGER NOT NULL DEFAULT 0 CHECK (uploaded >= 0),
    downloaded  INTEGER NOT NULL DEFAULT 0 CHECK (downloaded >= 0),
    peers       INTEGER NOT NULL DEFAULT 0 CHECK (peers >= 0)
);
