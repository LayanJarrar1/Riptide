//! Engine review round (S7 low disk, S2 automatic retry, N1 default folder, N4 checksum retry).
//! Each test fails if its fix is reverted.
#![allow(clippy::unwrap_used, missing_docs)]

mod common;

use std::time::Duration;

use common::{wait_completed, wait_failed, wait_for, World, MIB};
use riptide_core::{Checksum, ChecksumAlgo, DownloadRequest, DownloadState, Event};
use riptide_engine::EngineSettings;
use riptide_testserver::{expected_sha256_hex, DEFAULT_SEED};

const T: Duration = Duration::from_secs(60);

/// The text of the last `Failed` event of `id` found in the stored record.
async fn error_of(engine: &riptide_engine::Engine, id: riptide_core::DownloadId) -> String {
    engine
        .get(id)
        .await
        .unwrap()
        .unwrap()
        .error
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_that_cannot_fit_fails_fast_and_runs_once_there_is_room() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    // 10 MiB left on the volume; the download needs 1 MiB plus the 64 MiB margin
    engine.set_test_free_space(|_| Ok(10 * MIB));
    let id = engine.add(w.request("/ok?size=1048576")).await.unwrap();
    let retryable = wait_failed(&mut rx, id, T).await;
    assert!(!retryable, "freeing space is the fix, not waiting");
    let msg = error_of(&engine, id).await;
    // the UI files a failure under "disk" when the text says disk or space
    assert!(
        msg.contains("disk space") && !msg.contains("checksum"),
        "{msg}"
    );
    assert!(
        !w.downloads().join("ok").exists(),
        "nothing was written to a full disk"
    );

    // an unreadable free-space figure never blocks; plenty of room runs it
    engine.set_test_free_space(|_| Ok(10 * 1024 * MIB));
    engine.resume(id).await.unwrap();
    let path = wait_completed(&mut rx, id).await;
    assert_eq!(std::fs::metadata(path).unwrap().len(), MIB);
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_default_folder_must_be_absolute() {
    let w = World::new().await;
    let engine = w.engine().await;
    let good = w
        .dir
        .path()
        .join("elsewhere")
        .to_string_lossy()
        .into_owned();
    let keep = engine.settings().default_dir;
    assert_eq!(keep, w.downloads().to_string_lossy());

    for bad in ["", "   ", "relative/dir", "..\\up", "./here"] {
        engine
            .set_settings(EngineSettings {
                default_dir: bad.into(),
                connections: 4,
                ..engine.settings()
            })
            .await
            .unwrap();
        let s = engine.settings();
        assert_eq!(s.default_dir, keep, "{bad:?} must keep the previous folder");
        assert_eq!(s.connections, 4, "the rest of the settings still applies");
    }
    engine
        .set_settings(EngineSettings {
            default_dir: format!("  {good}  "),
            ..engine.settings()
        })
        .await
        .unwrap();
    assert_eq!(engine.settings().default_dir, good);
    engine.shutdown().await;

    // a stored blank or relative value falls back to the folder the app configured (the OS
    // Downloads folder) at the next start
    {
        let mut store = riptide_store::Store::open(w.config().db_path).unwrap();
        let bad = EngineSettings {
            default_dir: "relative".into(),
            ..EngineSettings::default()
        };
        store
            .set_setting(EngineSettings::KEY, &serde_json::to_string(&bad).unwrap())
            .unwrap();
    }
    let engine = w.engine().await;
    assert_eq!(
        engine.settings().default_dir,
        w.downloads().to_string_lossy()
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_checksum_mismatch_discards_the_bad_bytes_so_retry_downloads_again() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = MIB;
    let id = engine
        .add(DownloadRequest {
            filename: Some("bad.bin".into()),
            checksum: Some(Checksum {
                algo: ChecksumAlgo::Sha256,
                hex: expected_sha256_hex(DEFAULT_SEED, size),
            }),
            ..w.request(&format!("/corrupt?flip=3&size={size}"))
        })
        .await
        .unwrap();
    wait_failed(&mut rx, id, T).await;
    let part = w.downloads().join("bad.bin.rtpart");
    assert!(!part.exists(), "the corrupt part file is deleted");
    assert!(
        engine.segments(id).await.unwrap().is_empty(),
        "and so is the segment table"
    );
    assert!(error_of(&engine, id).await.contains("checksum"));

    // Retry starts over: the transfer runs again (a probe and a download), it does not just
    // re-verify the old file
    engine.resume(id).await.unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Probed { id: i, .. } if *i == id),
    )
    .await;
    wait_failed(&mut rx, id, T).await;
    assert!(!part.exists());
    engine.shutdown().await;
}

/// A local address nothing listens on: connecting fails like it does with the network down.
fn dead_url() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}/file.bin")
}

/// Counts `Failed` events of `id`: waits (up to a minute) for the first, then counts until
/// `quiet` passes without another.
async fn failures_until_quiet(
    rx: &mut tokio::sync::broadcast::Receiver<Event>,
    id: riptide_core::DownloadId,
    quiet: Duration,
) -> usize {
    wait_failed(rx, id, T).await;
    let mut n = 1;
    while let Ok(Ok(ev)) = tokio::time::timeout(quiet, rx.recv()).await {
        if matches!(ev, Event::Failed { id: i, .. } if i == id) {
            n += 1;
        }
    }
    n
}

/// A dead address fails after the probe's own retries (about 15 s), so one automatic retry is
/// enough to show the glue; the cap of three and the timer rules are unit-tested in the crate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_network_failure_is_queued_again_by_the_timer() {
    let w = World::new().await;
    let engine = w.engine().await;
    engine.set_test_retry_delays(vec![Duration::from_millis(100)]);
    let mut rx = engine.subscribe();
    let id = engine.add(DownloadRequest::new(dead_url())).await.unwrap();
    // the first failure plus the one automatic retry, then nothing more
    let n = failures_until_quiet(&mut rx, id, Duration::from_secs(25)).await;
    assert_eq!(n, 2);
    assert!(matches!(
        engine.get(id).await.unwrap().unwrap().state,
        DownloadState::Failed { .. }
    ));
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_automatic_retry_when_the_setting_is_off_or_the_failure_is_not_the_network() {
    let w = World::new().await;
    let engine = w.engine().await;
    engine.set_test_retry_delays(vec![Duration::from_millis(100); 3]);
    let mut rx = engine.subscribe();

    // HTTP answers are not network failures
    let id = engine.add(w.request("/no-such-endpoint")).await.unwrap();
    assert_eq!(
        failures_until_quiet(&mut rx, id, Duration::from_secs(3)).await,
        1
    );

    // setting off
    engine
        .set_settings(EngineSettings {
            auto_retry: false,
            ..engine.settings()
        })
        .await
        .unwrap();
    let id = engine.add(DownloadRequest::new(dead_url())).await.unwrap();
    assert_eq!(
        failures_until_quiet(&mut rx, id, Duration::from_secs(3)).await,
        1
    );
    engine.shutdown().await;
}

/// A shutdown that is not followed by the end of the process (an update installer that failed)
/// must leave a working engine: the interrupted download is queued again and finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revive_brings_a_shut_down_engine_back() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 4 * MIB;
    let id = engine
        .add(w.request(&format!("/slow?bps=2097152&size={size}")))
        .await
        .unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0),
    )
    .await;
    engine.shutdown().await;
    assert_eq!(
        engine.get(id).await.unwrap().unwrap().state,
        DownloadState::Downloading,
        "a shutdown leaves the record for the next start"
    );
    // the installer did not take over after all
    engine.revive().await.unwrap();
    let path = wait_completed(&mut rx, id).await;
    assert_eq!(
        riptide_net::checksum_of(path, ChecksumAlgo::Sha256)
            .await
            .unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    // and it takes new work
    let again = engine.add(w.request("/ok?size=1024")).await.unwrap();
    wait_completed(&mut rx, again).await;
    engine.shutdown().await;
}
