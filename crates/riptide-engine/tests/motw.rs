//! Mark-of-the-Web on completed downloads (Windows/NTFS): the `Zone.Identifier` stream carries
//! `ZoneId=3` and the sanitised URLs, the content hash is untouched, and the setting turns it off.
#![cfg(windows)]
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::time::Duration;

use riptide_core::{ChecksumAlgo, DownloadId, DownloadRequest, DownloadState, Event};
use riptide_engine::{Engine, EngineConfig, EngineSettings};
use riptide_net::checksum_of;
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::sync::broadcast;

fn config(dir: &tempfile::TempDir) -> EngineConfig {
    EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
        organize_new_install: false,
    }
}

fn stream_of(file: &Path) -> std::ffi::OsString {
    let mut s = file.as_os_str().to_os_string();
    s.push(":Zone.Identifier");
    s
}

/// False when the temp folder is not on NTFS (no alternate data streams).
fn streams_supported(dir: &Path) -> bool {
    let probe = dir.join("probe.bin");
    std::fs::write(&probe, b"x").unwrap();
    std::fs::write(stream_of(&probe), b"x").is_ok()
}

async fn wait_completed(rx: &mut broadcast::Receiver<Event>, id: DownloadId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(Event::StateChanged { id: i, to, .. }))
                if i == id && to == DownloadState::Completed =>
            {
                return
            }
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for Completed"),
        }
    }
}

async fn download_one(engine: &Engine, server: &TestServer, size: u64) -> std::path::PathBuf {
    let mut rx = engine.subscribe();
    let mut request = DownloadRequest::new(server.url(&format!("/ok?size={size}&token=SECRET")));
    request.referrer = Some("https://example.com/page?session=SECRET".into());
    let id = engine.add(request).await.unwrap();
    wait_completed(&mut rx, id).await;
    let d = engine.get(id).await.unwrap().unwrap();
    assert_eq!(
        checksum_of(d.final_path(), ChecksumAlgo::Sha256)
            .await
            .unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size),
        "content is byte-exact"
    );
    d.final_path()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_completed_download_carries_zone_id_3_and_the_hash_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        streams_supported(dir.path()),
        "the temp folder must be on NTFS"
    );
    let server = TestServer::spawn().await;
    let engine = Engine::open(config(&dir)).await.unwrap();
    let file = download_one(&engine, &server, 300_000).await;
    let text = std::fs::read_to_string(stream_of(&file)).unwrap();
    assert!(
        text.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"),
        "{text:?}"
    );
    assert!(
        text.contains("ReferrerUrl=https://example.com/page\r\n"),
        "{text:?}"
    );
    assert!(text.contains("HostUrl=http://127.0.0.1:"), "{text:?}");
    assert!(
        !text.contains("SECRET") && !text.contains('?'),
        "no query strings: {text:?}"
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_the_setting_off_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        streams_supported(dir.path()),
        "the temp folder must be on NTFS"
    );
    let server = TestServer::spawn().await;
    let engine = Engine::open(config(&dir)).await.unwrap();
    engine
        .set_settings(EngineSettings {
            mark_of_the_web: false,
            ..engine.settings()
        })
        .await
        .unwrap();
    let file = download_one(&engine, &server, 100_000).await;
    assert!(
        std::fs::read(stream_of(&file)).is_err(),
        "no Zone.Identifier stream"
    );
    engine.shutdown().await;
}
