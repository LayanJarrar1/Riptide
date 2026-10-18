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
fn v1_to_v2_adds_queue_columns_and_keeps_item_order() {
    let (_dir, path) = fixture_db(V1_FIXTURE);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(SCHEMA_VERSION, 2);

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
