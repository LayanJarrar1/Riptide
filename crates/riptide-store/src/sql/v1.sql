-- Riptide schema v1.
-- Applied inside one transaction by the migration runner; must not contain BEGIN/COMMIT.
-- `schema_version` itself is created by the runner before any migration runs.

CREATE TABLE queues (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT    NOT NULL UNIQUE,
    max_concurrent INTEGER NOT NULL DEFAULT 3 CHECK (max_concurrent >= 1),
    position       INTEGER NOT NULL DEFAULT 0,
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

-- Queue 1 is the built-in "Main" queue (riptide_core::QueueId::MAIN).
INSERT INTO queues (id, name, max_concurrent, position, enabled) VALUES (1, 'Main', 3, 0, 1);

CREATE TABLE categories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    key        TEXT    NOT NULL UNIQUE,      -- stable key; the UI translates it
    extensions TEXT    NOT NULL DEFAULT '',  -- comma-separated, lower-case, no dots
    dir        TEXT,                         -- NULL = default download folder
    position   INTEGER NOT NULL DEFAULT 0
);

INSERT INTO categories (key, extensions, position) VALUES
    ('compressed', 'zip,rar,7z,tar,gz,tgz,bz2,xz,zst,iso,cab', 0),
    ('documents',  'pdf,doc,docx,xls,xlsx,ppt,pptx,odt,ods,odp,txt,rtf,epub,mobi,csv', 1),
    ('music',      'mp3,flac,wav,aac,m4a,ogg,opus,wma,aiff', 2),
    ('video',      'mp4,mkv,avi,mov,wmv,webm,m4v,ts,flv,mpg,mpeg', 3),
    ('programs',   'exe,msi,msix,appx,apk,dmg,pkg,deb,rpm,appimage', 4),
    ('images',     'jpg,jpeg,png,gif,webp,bmp,svg,tif,tiff,heic,avif,psd', 5),
    ('other',      '', 6);

CREATE TABLE downloads (
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
    completed_at         INTEGER
);

CREATE INDEX idx_downloads_state ON downloads (state);
CREATE INDEX idx_downloads_queue ON downloads (queue_id, priority, added_at);

-- `written` is the number of bytes on disk from `start_byte`; the engine resumes at
-- start_byte + written. `end_byte` is exclusive.
CREATE TABLE segments (
    download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL CHECK (idx >= 0),
    start_byte  INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL CHECK (end_byte >= start_byte),
    written     INTEGER NOT NULL DEFAULT 0 CHECK (written >= 0 AND written <= end_byte - start_byte),
    state       TEXT    NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','active','done','failed')),
    PRIMARY KEY (download_id, idx)
) WITHOUT ROWID;

-- Secrets live apart from `downloads` so they can be wiped on completion/removal and excluded
-- from exports and logs (hard rule 4).
CREATE TABLE secrets (
    download_id INTEGER PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
    cookies     TEXT,
    headers     TEXT    NOT NULL DEFAULT '[]',  -- JSON array of [name, value]
    auth_user   TEXT,
    auth_pass   TEXT
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
