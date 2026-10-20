//! Opening databases written by earlier builds.
#![allow(clippy::unwrap_used)]

use riptide_core::{DownloadId, DownloadState, Priority, QueueId, SegmentState};
use riptide_store::{Store, SCHEMA_VERSION};
use rusqlite::Connection;

const V1_FIXTURE: &str = include_str!("fixtures/v1.sql");

fn fixture_db(sql: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("riptide.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(sql).unwrap();
    drop(conn);
    (dir, path)
}

#[test]
fn migrate_from_empty_file_creates_latest_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.sqlite");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(store.list_downloads().unwrap().is_empty());
    drop(store);
    // reopening is a no-op
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn migrate_from_v1_fixture_keeps_every_row() {
    let (_dir, path) = fixture_db(V1_FIXTURE);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(store.journal_mode().unwrap(), "wal");

    let downloads = store.list_downloads().unwrap();
    assert_eq!(downloads.len(), 2);

    let big = &downloads[0];
    assert_eq!(big.id, DownloadId(1));
    assert_eq!(big.size, Some(4_294_967_296));
    assert!(big.supports_ranges);
    assert_eq!(big.state, DownloadState::Downloading);
    assert_eq!(big.etag.as_deref(), Some("\"v1\""));
    assert_eq!(
        big.checksum_expected.as_ref().map(|c| c.hex.len()),
        Some(64)
    );
    assert_eq!(big.queue_id, QueueId::MAIN);
    assert_eq!(
        big.kind,
        riptide_core::DownloadKind::Http,
        "v1 rows become plain HTTP downloads"
    );
    assert_eq!(big.media_format, None);

    let pdf = &downloads[1];
    assert_eq!(pdf.filename, "ملف.pdf", "Arabic names survive");
    assert_eq!(pdf.state, DownloadState::Failed { retryable: false });
    assert_eq!(pdf.priority, Priority::High);
    assert_eq!(pdf.speed_limit, Some(500_000));
    assert_eq!(pdf.queue_id, QueueId(2));
    assert_eq!(pdf.error.as_deref(), Some("Content-Length mismatch"));

    let segs = store.segments(DownloadId(1)).unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].state, SegmentState::Done);
    assert_eq!(segs[1].written, 1_000_000);
    assert_eq!(
        store.downloaded_bytes(DownloadId(1)).unwrap(),
        2_147_483_648 + 1_000_000
    );

    let secrets = store.secrets(DownloadId(1)).unwrap().unwrap();
    assert_eq!(secrets.cookies.as_deref(), Some("session=fixture"));
    assert_eq!(
        secrets.headers,
        [("X-Token".to_string(), "fixture-token".to_string())]
    );

    assert_eq!(store.setting("lang").unwrap().as_deref(), Some("ar"));
    assert_eq!(store.queues().unwrap().len(), 2);
    assert_eq!(store.categories().unwrap().len(), 7);
}

#[test]
fn v1_to_v3_adds_media_and_queue_columns_and_keeps_item_order() {
    let (_dir, path) = fixture_db(V1_FIXTURE);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(SCHEMA_VERSION, 5);

    // the v1 queues survive with an empty schedule and no post action
    let queues = store.queues().unwrap();
    assert_eq!(queues.len(), 2);
    assert_eq!(queues[1].name, "Night");
    assert!(!queues[1].enabled);
    assert!(queues.iter().all(|q| q.schedule.is_unscheduled()));
    assert!(queues.iter().all(|q| q.on_done.is_none()));

    // existing downloads keep their old order (by id) through the new position column
    let positions: Vec<_> = store
        .list_downloads()
        .unwrap()
        .iter()
        .map(|d| (d.id, d.queue_position))
        .collect();
    assert_eq!(positions, [(DownloadId(1), 1), (DownloadId(2), 2)]);

    // new rows continue the numbering within their queue
    let mut d = riptide_core::Download::new(DownloadId(0), "https://a/b", "b", ".", 1);
    d.queue_id = QueueId(2);
    let id = store.insert_download(&d).unwrap();
    assert_eq!(store.get_download(id).unwrap().unwrap().queue_position, 3);

    // and the new columns work on a migrated file
    let mut spec = riptide_core::QueueSpec::new("Weekend", 2);
    spec.schedule = riptide_core::Schedule::only_at_night();
    spec.on_done = Some(riptide_core::PostAction::Sleep);
    let q = store.create_queue(&spec).unwrap();
    assert_eq!(store.get_queue(q).unwrap().unwrap().spec(), spec);
}

#[test]
fn v2_database_with_a_media_row_migrates_to_v3() {
    use riptide_core::DownloadKind;
    use riptide_store::{migrate, MIGRATIONS};

    // a database exactly as the media release (schema v2) left it
    let (_dir, path) = fixture_db(V1_FIXTURE);
    {
        let mut conn = Connection::open(&path).unwrap();
        let v = migrate::run_with(&mut conn, &MIGRATIONS[..2]).unwrap();
        assert_eq!(v, 2);
        conn.execute(
            "UPDATE downloads SET kind = 'media', media_format = 'h720' WHERE id = 2",
            [],
        )
        .unwrap();
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);

    let all = store.list_downloads().unwrap();
    assert_eq!(all[0].kind, DownloadKind::Http);
    assert_eq!(all[1].kind, DownloadKind::Media);
    assert_eq!(all[1].media_format.as_deref(), Some("h720"));
    assert_eq!(
        all.iter().map(|d| d.queue_position).collect::<Vec<_>>(),
        [1, 2]
    );

    // a media record added now joins its queue like any other
    let mut d = riptide_core::Download::new(DownloadId(0), "https://v/watch", "v.mp4", ".", 1);
    d.kind = DownloadKind::Media;
    d.queue_id = QueueId(2);
    let id = store.insert_download(&d).unwrap();
    let back = store.get_download(id).unwrap().unwrap();
    assert_eq!((back.kind, back.queue_position), (DownloadKind::Media, 3));
}

#[test]
fn newer_schema_is_refused_without_touching_the_file() {
    let (_dir, path) = fixture_db(V1_FIXTURE);
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (42, 'future', 0)",
            [],
        )
        .unwrap();
    }
    let err = Store::open(&path).unwrap_err();
    assert!(
        matches!(err, riptide_store::Error::TooNew { found: 42, .. }),
        "{err}"
    );
    // and the rows are still there for the newer build
    let conn = Connection::open(&path).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM downloads", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn two_connections_share_the_file_under_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.sqlite");
    let mut writer = Store::open(&path).unwrap();
    let reader = Store::open(&path).unwrap();
    let d = riptide_core::Download::new(DownloadId(0), "https://a/b", "b", ".", 1);
    let id = writer.insert_download(&d).unwrap();
    assert_eq!(
        reader.get_download(id).unwrap().map(|d| d.url),
        Some("https://a/b".to_string())
    );
}

#[test]
fn v3_database_migrates_to_v4_keeping_segments_secrets_and_ids() {
    use riptide_core::{DownloadKind, TorrentFileInfo, TorrentInfo};
    use riptide_store::{migrate, MIGRATIONS};

    // a database exactly as alpha.4 (schema v3) left it, with data in every child table
    let (_dir, path) = fixture_db(V1_FIXTURE);
    let (segments_before, secrets_before) = {
        let mut conn = Connection::open(&path).unwrap();
        migrate::run_with(&mut conn, &MIGRATIONS[..3]).unwrap();
        let s: i64 = conn
            .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))
            .unwrap();
        let k: i64 = conn
            .query_row("SELECT COUNT(*) FROM secrets", [], |r| r.get(0))
            .unwrap();
        (s, k)
    };
    assert!(segments_before > 0 && secrets_before > 0);

    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    // foreign keys are enforced again after the migration
    let fk: i64 = store
        .connection()
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);
    let ids: Vec<_> = store
        .list_downloads()
        .unwrap()
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(ids, [DownloadId(1), DownloadId(2)]);
    let segs: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))
        .unwrap();
    let secs: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM secrets", [], |r| r.get(0))
        .unwrap();
    assert_eq!((segs, secs), (segments_before, secrets_before));

    // the rebuilt table takes a torrent row, the old kinds still work, bad kinds are refused
    let mut d = riptide_core::Download::new(DownloadId(0), "magnet:?xt=urn:btih:x", "t", ".", 1);
    d.kind = DownloadKind::Torrent;
    let id = store.insert_download(&d).unwrap();
    assert!(id.0 > 2, "ids continue after the old ones");
    store
        .insert_torrent(&TorrentInfo {
            download_id: id,
            info_hash: "e".repeat(40),
            name: "t".into(),
            files: vec![TorrentFileInfo {
                index: 0,
                path: "t".into(),
                len: 1,
            }],
            selected: None,
            sequential: false,
            seeding: false,
            uploaded: 0,
            downloaded: 0,
            peers: 0,
        })
        .unwrap();
    assert_eq!(
        store.get_download(id).unwrap().unwrap().kind,
        DownloadKind::Torrent
    );
    assert!(store
        .connection()
        .execute("UPDATE downloads SET kind = 'bogus' WHERE id = 1", [])
        .is_err());

    // removing a download still cascades to its segments
    store.remove_download(DownloadId(1)).unwrap();
    let left: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM segments WHERE download_id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn v3_and_v4_databases_migrate_to_v5_keeping_their_data() {
    use riptide_core::{DownloadKind, TorrentFileInfo, TorrentInfo};
    use riptide_store::{migrate, MIGRATIONS};

    // a v3 database (alpha.4) with rows in every child table goes straight to v5
    let (_d3, p3) = fixture_db(V1_FIXTURE);
    {
        let mut conn = Connection::open(&p3).unwrap();
        migrate::run_with(&mut conn, &MIGRATIONS[..3]).unwrap();
    }
    let store = Store::open(&p3).unwrap();
    assert_eq!(store.schema_version().unwrap(), 5);
    assert_eq!(store.list_downloads().unwrap().len(), 2);
    assert_eq!(store.segments(DownloadId(1)).unwrap().len(), 2);
    assert!(store.secrets(DownloadId(1)).unwrap().is_some());
    for table in ["torrents", "seen_downloads", "seen_keys"] {
        let n: i64 = store
            .connection()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table} exists and is empty");
    }
    drop(store);

    // a v4 database (torrents, no seen index) with a torrent keeps it and gains the index
    let (_d4, p4) = fixture_db(V1_FIXTURE);
    {
        let mut conn = Connection::open(&p4).unwrap();
        migrate::run_with(&mut conn, &MIGRATIONS[..4]).unwrap();
        conn.execute(
            "INSERT INTO downloads (url, filename, dir, state, added_at, kind) \
             VALUES ('magnet:?xt=urn:btih:x', 'T', '.', 'paused', 1, 'torrent')",
            [],
        )
        .unwrap();
        let id: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO torrents (download_id, info_hash, name, files, uploaded, downloaded) \
             VALUES (?1, ?2, 'T', '[]', 7, 9)",
            rusqlite::params![id, "f".repeat(40)],
        )
        .unwrap();
    }
    let store = Store::open(&p4).unwrap();
    assert_eq!(store.schema_version().unwrap(), 5);
    let t = store.list_torrents().unwrap().remove(0);
    assert_eq!((t.uploaded, t.downloaded), (7, 9));
    assert_eq!(
        store.get_download(t.download_id).unwrap().unwrap().kind,
        DownloadKind::Torrent
    );
    store
        .connection()
        .execute(
            "INSERT INTO seen_downloads (download_id, title, filename, dir, completed_at) \
             VALUES (1, 't', 'f', 'd', 1)",
            [],
        )
        .unwrap();
    let _ = TorrentInfo {
        download_id: DownloadId(1),
        info_hash: String::new(),
        name: String::new(),
        files: Vec::<TorrentFileInfo>::new(),
        selected: None,
        sequential: false,
        seeding: false,
        uploaded: 0,
        downloaded: 0,
        peers: 0,
    };
}
