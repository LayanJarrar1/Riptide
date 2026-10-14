-- A database exactly as Riptide 0.1 (schema v1) would have left it, with a few rows.
-- Used by tests/migrations.rs: opening it with the current build must keep every row readable.
-- When schema v2 lands, keep this file as-is and add a test that migrates it forward.

CREATE TABLE schema_version (
    version    INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    applied_at INTEGER NOT NULL
);
INSERT INTO schema_version (version, name, applied_at) VALUES (1, 'initial', 1760000000000);

CREATE TABLE queues (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT    NOT NULL UNIQUE,
    max_concurrent INTEGER NOT NULL DEFAULT 3 CHECK (max_concurrent >= 1),
    position       INTEGER NOT NULL DEFAULT 0,
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);
INSERT INTO queues (id, name, max_concurrent, position, enabled) VALUES (1, 'Main', 3, 0, 1);
INSERT INTO queues (id, name, max_concurrent, position, enabled) VALUES (2, 'Night', 1, 1, 0);

CREATE TABLE categories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    key        TEXT    NOT NULL UNIQUE,
    extensions TEXT    NOT NULL DEFAULT '',
    dir        TEXT,
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

INSERT INTO downloads (id, url, final_url, filename, dir, size, supports_ranges, etag, last_modified,
    content_type, state, retryable, category_id, queue_id, priority, speed_limit, connections,
    checksum_algo, checksum_expected, checksum_actual_algo, checksum_actual, error, added_at,
    started_at, completed_at)
VALUES
    (1, 'https://example.com/big.iso', 'https://cdn.example.com/big.iso', 'big.iso', 'C:\Users\u\Downloads',
     4294967296, 1, '"v1"', 'Wed, 01 Oct 2025 00:00:00 GMT', 'application/octet-stream',
     'downloading', NULL, 1, 1, 'normal', NULL, 8,
     'sha256', '0000000000000000000000000000000000000000000000000000000000000000', NULL, NULL, NULL,
     1760000000000, 1760000001000, NULL),
    (2, 'https://example.com/ملف.pdf', NULL, 'ملف.pdf', 'C:\Users\u\Documents',
     1048576, 0, NULL, NULL, 'application/pdf',
     'failed', 0, 2, 2, 'high', 500000, 1,
     NULL, NULL, NULL, NULL, 'Content-Length mismatch',
     1760000002000, 1760000003000, NULL);

CREATE TABLE segments (
    download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL CHECK (idx >= 0),
    start_byte  INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL CHECK (end_byte >= start_byte),
    written     INTEGER NOT NULL DEFAULT 0 CHECK (written >= 0 AND written <= end_byte - start_byte),
    state       TEXT    NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','active','done','failed')),
    PRIMARY KEY (download_id, idx)
) WITHOUT ROWID;
INSERT INTO segments (download_id, idx, start_byte, end_byte, written, state) VALUES
    (1, 0, 0,          2147483648, 2147483648, 'done'),
    (1, 1, 2147483648, 4294967296, 1000000,    'active');

CREATE TABLE secrets (
    download_id INTEGER PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
    cookies     TEXT,
    headers     TEXT    NOT NULL DEFAULT '[]',
    auth_user   TEXT,
    auth_pass   TEXT
);
INSERT INTO secrets (download_id, cookies, headers, auth_user, auth_pass)
VALUES (1, 'session=fixture', '[["X-Token","fixture-token"]]', NULL, NULL);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
INSERT INTO settings (key, value) VALUES ('lang', 'ar'), ('theme', 'dark');
