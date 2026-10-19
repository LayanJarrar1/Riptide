//! Batch downloads through the engine API: expand a pattern, add every item in one
//! transaction, download them against `/batch/` on the test server and compare every SHA-256.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_core::batch::expand;
use riptide_core::{BatchOptions, ChecksumAlgo, DownloadId, DownloadState, Event};
use riptide_engine::{BatchSkipReason, Engine, EngineConfig};
use riptide_net::checksum_of;
use riptide_testserver::{batch_item, expected_sha256_hex, TestServer};

fn config(dir: &tempfile::TempDir) -> EngineConfig {
    EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
    }
}

/// Polls until every id is in a terminal state (or panics after `timeout`).
async fn wait_all_terminal(engine: &Engine, ids: &[DownloadId], timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let all = engine.list().await.unwrap();
        let pending = ids
            .iter()
            .filter(|id| {
                all.iter().find(|d| d.id == **id).is_none_or(|d| {
                    !(d.state.is_terminal() || matches!(d.state, DownloadState::Failed { .. }))
                })
            })
            .count();
        if pending == 0 {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{pending} downloads still running"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pattern_batch_adds_paused_then_downloads_every_item_byte_exact() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("batch-out");
    let engine = Engine::open(config(&dir)).await.unwrap();
    let mut rx = engine.subscribe();

    let urls = expand(&server.url("/batch/file[001-120].zip")).unwrap();
    assert_eq!(urls.len(), 120);
    let options = BatchOptions {
        dir: Some(out.clone()),
        start_paused: true,
        ..BatchOptions::default()
    };
    let outcome = engine.add_batch(options.requests(&urls)).await.unwrap();
    assert_eq!(outcome.added.len(), 120);
    assert!(outcome.skipped.is_empty());

    // one Added event per item, in order, and everything waits (start later)
    let mut seen = Vec::new();
    while seen.len() < 120 {
        if let Event::Added { id } = rx.recv().await.unwrap() {
            seen.push(id);
        }
    }
    assert_eq!(seen, outcome.added);
    let listed = engine.list().await.unwrap();
    assert_eq!(listed.len(), 120);
    assert!(listed.iter().all(|d| d.state == DownloadState::Paused));
    assert!(listed.iter().all(|d| d.dir == out));
    assert_eq!(listed[0].url, urls[0]);
    assert_eq!(listed[119].url, urls[119]);

    engine.resume_all().await.unwrap();
    wait_all_terminal(&engine, &outcome.added, Duration::from_secs(120)).await;

    for (id, url) in outcome.added.iter().zip(&urls) {
        let d = engine.get(*id).await.unwrap().unwrap();
        assert_eq!(d.state, DownloadState::Completed, "{url}: {:?}", d.error);
        let name = url.rsplit('/').next().unwrap();
        assert_eq!(d.filename, name);
        let item = batch_item(name).unwrap();
        assert_eq!(d.size, Some(item.size), "{name}");
        let sha = checksum_of(d.final_path(), ChecksumAlgo::Sha256)
            .await
            .unwrap();
        assert_eq!(
            sha,
            expected_sha256_hex(item.seed, item.size),
            "{name} differs"
        );
    }
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn letter_pattern_starts_immediately_and_missing_items_fail_alone() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();

    // file119 and file120 exist; file121 and file122 are outside the pattern (404)
    let mut all = expand(&server.url("/batch/file[119-122].zip")).unwrap();
    all.extend(expand(&server.url("/batch/img_[a-e].jpg")).unwrap());
    let options = BatchOptions {
        dir: Some(dir.path().join("out")),
        ..BatchOptions::default()
    };
    let outcome = engine.add_batch(options.requests(&all)).await.unwrap();
    assert_eq!(outcome.added.len(), 9);
    wait_all_terminal(&engine, &outcome.added, Duration::from_secs(60)).await;

    for (id, url) in outcome.added.iter().zip(&all) {
        let d = engine.get(*id).await.unwrap().unwrap();
        let name = url.rsplit('/').next().unwrap();
        match batch_item(name) {
            Some(item) => {
                assert_eq!(d.state, DownloadState::Completed, "{name}: {:?}", d.error);
                let sha = checksum_of(d.final_path(), ChecksumAlgo::Sha256)
                    .await
                    .unwrap();
                assert_eq!(sha, expected_sha256_hex(item.seed, item.size), "{name}");
            }
            None => {
                assert!(
                    matches!(d.state, DownloadState::Failed { .. }),
                    "{name} should fail, is {:?}",
                    d.state
                );
                assert!(!d.final_path().exists(), "{name} left a file");
            }
        }
    }
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicates_and_invalid_urls_are_skipped_and_reported() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();
    let options = BatchOptions {
        dir: Some(dir.path().join("out")),
        start_paused: true,
        ..BatchOptions::default()
    };

    let first = engine
        .add_batch(options.requests(&[
            server.url("/batch/file001.zip"),
            server.url("/batch/file002.zip"),
        ]))
        .await
        .unwrap();
    assert_eq!(first.added.len(), 2);

    let second = engine
        .add_batch(options.requests(&[
            server.url("/batch/file002.zip#again"), // already listed (fragment ignored)
            server.url("/batch/file003.zip"),
            server.url("/batch/file003.zip"), // repeats an earlier item of this batch
            "ftp://example.test/x".to_string(), // not http(s)
            "not a url".to_string(),
            server.url("/batch/file004.zip"),
        ]))
        .await
        .unwrap();
    assert_eq!(second.added.len(), 2);
    assert_eq!(second.skipped.len(), 4);
    assert_eq!(second.skipped[0].reason, BatchSkipReason::Duplicate);
    assert_eq!(second.skipped[0].existing, Some(first.added[1]));
    assert_eq!(second.skipped[1].reason, BatchSkipReason::Duplicate);
    assert_eq!(second.skipped[1].existing, None);
    assert_eq!(second.skipped[2].reason, BatchSkipReason::Invalid);
    assert_eq!(second.skipped[3].reason, BatchSkipReason::Invalid);
    assert_eq!(engine.list().await.unwrap().len(), 4);

    // an empty batch is fine and adds nothing
    let none = engine.add_batch(Vec::new()).await.unwrap();
    assert!(none.added.is_empty() && none.skipped.is_empty());

    // over the cap is refused outright
    let too_many = options.requests(&vec!["http://h/x"; riptide_core::MAX_EXPANSION + 1]);
    assert!(engine.add_batch(too_many).await.is_err());
    assert_eq!(engine.list().await.unwrap().len(), 4);
    engine.shutdown().await;
}
