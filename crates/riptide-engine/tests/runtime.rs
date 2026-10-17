//! The runtime against `riptide-testserver`: add → events → completed; pause/resume; cancel;
//! remove; crash recovery across `open`; settings persistence.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_core::{ChecksumAlgo, DownloadId, DownloadRequest, DownloadState, Event};
use riptide_engine::{Engine, EngineConfig, EngineSettings};
use riptide_net::checksum_of;
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::sync::broadcast;

const MIB: u64 = 1024 * 1024;

struct World {
    server: TestServer,
    dir: tempfile::TempDir,
}

impl World {
    async fn new() -> Self {
        World {
            server: TestServer::spawn().await,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn config(&self) -> EngineConfig {
        EngineConfig {
            db_path: self.dir.path().join("riptide.sqlite"),
            default_dir: self.dir.path().join("downloads"),
        }
    }

    async fn engine(&self) -> Engine {
        Engine::open(self.config()).await.unwrap()
    }

    fn request(&self, path: &str) -> DownloadRequest {
        DownloadRequest::new(self.server.url(path))
    }
}

/// Waits until `pred` sees an event, or panics after `timeout` listing what did arrive.
async fn wait_for(
    rx: &mut broadcast::Receiver<Event>,
    timeout: Duration,
    mut pred: impl FnMut(&Event) -> bool,
) -> Event {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut seen: Vec<String> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                if pred(&ev) {
                    return ev;
                }
                if !ev.is_progress() {
                    seen.push(format!("{ev:?}"));
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("event channel closed: {e}"),
            Err(_) => panic!("timed out waiting for an event; saw: {seen:#?}"),
        }
    }
}

async fn wait_state(
    rx: &mut broadcast::Receiver<Event>,
    id: DownloadId,
    to: DownloadState,
    timeout: Duration,
) {
    wait_for(
        rx,
        timeout,
        |e| matches!(e, Event::StateChanged { id: i, to: t, .. } if *i == id && *t == to),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn add_runs_to_completed_with_the_expected_events_and_bytes() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 3 * MIB;
    let id = engine
        .add(w.request(&format!("/ok?size={size}")))
        .await
        .unwrap();

    wait_for(
        &mut rx,
        Duration::from_secs(5),
        |e| matches!(e, Event::Added { id: i } if *i == id),
    )
    .await;
    wait_state(&mut rx, id, DownloadState::Probing, Duration::from_secs(5)).await;
    let probed = wait_for(
        &mut rx,
        Duration::from_secs(5),
        |e| matches!(e, Event::Probed { id: i, .. } if *i == id),
    )
    .await;
    assert!(
        matches!(probed, Event::Probed { size: Some(s), supports_ranges: true, .. } if s == size)
    );
    wait_state(
        &mut rx,
        id,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    let mut saw_progress = false;
    let completed = wait_for(&mut rx, Duration::from_secs(30), |e| {
        if matches!(e, Event::Progress { id: i, .. } if *i == id) {
            saw_progress = true;
        }
        matches!(e, Event::Completed { id: i, .. } if *i == id)
    })
    .await;
    assert!(saw_progress, "progress events were emitted");
    let Event::Completed { path, .. } = completed else {
        unreachable!()
    };
    assert_eq!(path, w.dir.path().join("downloads").join("ok"));
    assert_eq!(
        checksum_of(path, ChecksumAlgo::Sha256).await.unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    let record = engine.get(id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Completed);
    assert!(record.completed_at.is_some());
    assert!(engine
        .segments(id)
        .await
        .unwrap()
        .iter()
        .all(|s| s.is_complete()));
    assert!(engine.running_ids().is_empty());
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_resume_cancel_and_remove() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();

    // a slow one we can pause mid-way
    let id = engine
        .add(w.request("/slow?bps=2097152&size=16777216"))
        .await
        .unwrap();
    wait_state(
        &mut rx,
        id,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    wait_for(
        &mut rx,
        Duration::from_secs(5),
        |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0),
    )
    .await;
    engine.pause(id).await.unwrap();
    wait_state(&mut rx, id, DownloadState::Paused, Duration::from_secs(5)).await;
    let on_disk: u64 = engine
        .segments(id)
        .await
        .unwrap()
        .iter()
        .map(|s| s.written)
        .sum();
    assert!(on_disk > 0, "a checkpoint was persisted before pausing");
    assert!(engine.running_ids().is_empty());

    engine.resume(id).await.unwrap();
    wait_state(&mut rx, id, DownloadState::Queued, Duration::from_secs(5)).await;
    wait_state(
        &mut rx,
        id,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    engine.cancel(id).await.unwrap();
    wait_state(
        &mut rx,
        id,
        DownloadState::Cancelled,
        Duration::from_secs(5),
    )
    .await;
    let record = engine.get(id).await.unwrap().unwrap();
    assert!(
        !riptide_net::part_path_for(&record.final_path()).exists(),
        "cancel removes the part file"
    );

    // paused-from-queued, then removed
    let paused = engine
        .add(DownloadRequest {
            start_paused: true,
            ..w.request("/ok?size=1024")
        })
        .await
        .unwrap();
    assert_eq!(
        engine.get(paused).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
    engine.remove(paused, false).await.unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(5),
        |e| matches!(e, Event::Removed { id: i } if *i == paused),
    )
    .await;
    assert!(engine.get(paused).await.unwrap().is_none());

    // remove while running
    let running = engine
        .add(w.request("/slow?bps=1048576&size=8388608"))
        .await
        .unwrap();
    wait_state(
        &mut rx,
        running,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    engine.remove(running, false).await.unwrap();
    assert!(engine.get(running).await.unwrap().is_none());
    assert!(engine.running_ids().is_empty());
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_limit_and_order_are_respected() {
    let w = World::new().await;
    let engine = w.engine().await;
    engine
        .set_settings(EngineSettings {
            max_active: 2,
            ..engine.settings()
        })
        .await
        .unwrap();
    let mut rx = engine.subscribe();
    let mut ids = Vec::new();
    for i in 0..4 {
        ids.push(
            engine
                .add(DownloadRequest {
                    filename: Some(format!("f{i}.bin")),
                    ..w.request("/slow?bps=4194304&size=4194304")
                })
                .await
                .unwrap(),
        );
    }
    // the first two start (their events may arrive in either order), the others wait
    let mut started = std::collections::HashSet::new();
    wait_for(&mut rx, Duration::from_secs(10), |e| {
        if let Event::StateChanged {
            id,
            to: DownloadState::Downloading,
            ..
        } = e
        {
            started.insert(*id);
        }
        started.contains(&ids[0]) && started.contains(&ids[1])
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(engine.running_ids().len(), 2);
    assert_eq!(
        engine.get(ids[2]).await.unwrap().unwrap().state,
        DownloadState::Queued
    );
    assert_eq!(
        engine.get(ids[3]).await.unwrap().unwrap().state,
        DownloadState::Queued
    );
    // eventually all four complete (again in any order)
    let mut done = std::collections::HashSet::new();
    wait_for(&mut rx, Duration::from_secs(60), |e| {
        if let Event::Completed { id, .. } = e {
            done.insert(*id);
        }
        ids.iter().all(|id| done.contains(id))
    })
    .await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_survives_the_process_and_resumes_on_the_next_open() {
    let w = World::new().await;
    let size = 24 * MIB;
    let id = {
        let engine = w.engine().await;
        let mut rx = engine.subscribe();
        let id = engine
            .add(w.request(&format!("/slow?bps=4194304&size={size}")))
            .await
            .unwrap();
        wait_state(
            &mut rx,
            id,
            DownloadState::Downloading,
            Duration::from_secs(5),
        )
        .await;
        wait_for(&mut rx, Duration::from_secs(10), |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 2 * MIB)).await;
        // the "process" ends without pausing: the record stays Downloading on disk
        engine.shutdown().await;
        assert_eq!(
            engine.get(id).await.unwrap().unwrap().state,
            DownloadState::Downloading
        );
        id
    };
    let on_disk_before: u64 = {
        let store = riptide_store::Store::open(w.dir.path().join("riptide.sqlite")).unwrap();
        store.segments(id).unwrap().iter().map(|s| s.written).sum()
    };
    assert!(on_disk_before > 0);

    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    // recovery re-queues it and the scheduler picks it up again
    let completed = wait_for(
        &mut rx,
        Duration::from_secs(60),
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Completed { path, .. } = completed else {
        unreachable!()
    };
    assert_eq!(
        checksum_of(path, ChecksumAlgo::Sha256).await.unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failures_probe_and_settings() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();

    let missing = engine.add(w.request("/no-such-path")).await.unwrap();
    let failed = wait_for(
        &mut rx,
        Duration::from_secs(10),
        |e| matches!(e, Event::Failed { id: i, .. } if *i == missing),
    )
    .await;
    assert!(
        matches!(
            failed,
            Event::Failed {
                retryable: false,
                ..
            }
        ),
        "{failed:?}"
    );
    assert_eq!(
        engine.get(missing).await.unwrap().unwrap().state,
        DownloadState::Failed { retryable: false }
    );

    let probe = engine
        .probe(&w.server.url("/cd/arabic?size=2048"), None, None)
        .await
        .unwrap();
    assert_eq!(probe.filename, "ملف تجريبي.zip");
    assert_eq!(probe.size, Some(2048));
    assert!(probe.resumable);
    assert!(engine.probe("ftp://x/y", None, None).await.is_err());

    let mut s = engine.settings();
    s.connections = 4;
    s.per_host = 3;
    s.default_dir = w
        .dir
        .path()
        .join("elsewhere")
        .to_string_lossy()
        .into_owned();
    engine.set_settings(s.clone()).await.unwrap();
    engine.shutdown().await;
    let again = w.engine().await;
    assert_eq!(again.settings(), s, "settings persist across open");
    let id = again.add(w.request("/ok?size=4096")).await.unwrap();
    let mut rx = again.subscribe();
    let done = wait_for(
        &mut rx,
        Duration::from_secs(10),
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Completed { path, .. } = done else {
        unreachable!()
    };
    assert!(
        path.starts_with(w.dir.path().join("elsewhere")),
        "new default dir is used: {}",
        path.display()
    );
    assert_eq!(again.get(id).await.unwrap().unwrap().connections, 4);
    again.shutdown().await;
}

// ---------------------------------------------------------------------------------------------
// files lane: category folders, duplicates, restart from zero, checksum / sanitizer evidence,
// disk space
// ---------------------------------------------------------------------------------------------

fn category_id_of(w: &World, key: &str) -> riptide_core::CategoryId {
    let store = riptide_store::Store::open(w.dir.path().join("riptide.sqlite")).unwrap();
    store
        .categories()
        .unwrap()
        .into_iter()
        .find(|c| c.key == key)
        .map(|c| c.id)
        .unwrap()
}

async fn wait_completed(rx: &mut broadcast::Receiver<Event>, id: DownloadId) -> std::path::PathBuf {
    let done = wait_for(
        rx,
        Duration::from_secs(60),
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Completed { path, .. } = done else {
        unreachable!()
    };
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn category_folders_decide_where_a_request_without_a_dir_lands() {
    let w = World::new().await;
    let engine = w.engine().await;
    let zips = w.dir.path().join("zips");
    let mut s = engine.settings();
    s.category_dirs = [
        (
            "compressed".to_string(),
            zips.to_string_lossy().into_owned(),
        ),
        ("documents".to_string(), "relative/docs".to_string()),
        ("music".to_string(), "   ".to_string()),
        ("archives".to_string(), zips.to_string_lossy().into_owned()),
    ]
    .into_iter()
    .collect();
    engine.set_settings(s).await.unwrap();
    // only an absolute folder under a known key survives
    assert_eq!(
        engine.settings().category_dirs.keys().collect::<Vec<_>>(),
        ["compressed"]
    );
    assert_eq!(
        engine
            .settings()
            .dir_for(riptide_core::Category::Compressed),
        zips
    );
    assert_eq!(
        engine.settings().dir_for(riptide_core::Category::Documents),
        w.dir.path().join("downloads")
    );
    let mut rx = engine.subscribe();

    // a .zip with no dir goes to the category folder and carries the category row id
    let zip = engine
        .add(DownloadRequest {
            filename: Some("a.zip".into()),
            ..w.request("/ok?size=4096")
        })
        .await
        .unwrap();
    assert_eq!(wait_completed(&mut rx, zip).await, zips.join("a.zip"));
    let record = engine.get(zip).await.unwrap().unwrap();
    assert_eq!(record.dir, zips);
    assert_eq!(record.category_id, Some(category_id_of(&w, "compressed")));

    // a category without a folder uses the default
    let pdf = engine
        .add(DownloadRequest {
            filename: Some("b.pdf".into()),
            ..w.request("/ok?size=4096")
        })
        .await
        .unwrap();
    assert_eq!(
        wait_completed(&mut rx, pdf).await,
        w.dir.path().join("downloads").join("b.pdf")
    );
    assert_eq!(
        engine.get(pdf).await.unwrap().unwrap().category_id,
        Some(category_id_of(&w, "documents"))
    );

    // an explicit dir is never moved, whatever the category
    let elsewhere = w.dir.path().join("elsewhere");
    let explicit = engine
        .add(DownloadRequest {
            filename: Some("c.zip".into()),
            dir: Some(elsewhere.clone()),
            ..w.request("/ok?size=4096")
        })
        .await
        .unwrap();
    assert_eq!(
        wait_completed(&mut rx, explicit).await,
        elsewhere.join("c.zip")
    );

    // the URL says "arabic" (no extension → Other → default folder); the server's
    // Content-Disposition names a .zip, so the category and the folder follow the probe
    let renamed = engine.add(w.request("/cd/arabic?size=2048")).await.unwrap();
    assert_eq!(
        engine.get(renamed).await.unwrap().unwrap().category_id,
        Some(category_id_of(&w, "other"))
    );
    assert_eq!(
        wait_completed(&mut rx, renamed).await,
        zips.join("ملف تجريبي.zip")
    );
    assert_eq!(
        engine.get(renamed).await.unwrap().unwrap().category_id,
        Some(category_id_of(&w, "compressed"))
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn find_duplicate_matches_the_url_without_fragment_or_the_name_in_the_folder() {
    let w = World::new().await;
    let engine = w.engine().await;
    let url = w.server.url("/ok?size=1024");
    let id = engine
        .add(DownloadRequest {
            filename: Some("dup.bin".into()),
            start_paused: true,
            ..DownloadRequest::new(url.clone())
        })
        .await
        .unwrap();
    let parsed = |s: &str| riptide_engine::parse_http_url(s).unwrap();

    // same URL, with or without a fragment
    assert_eq!(
        engine
            .find_duplicate(&parsed(&format!("{url}#section")), None, None)
            .await
            .unwrap()
            .map(|d| d.id),
        Some(id)
    );
    // a different URL but the same name in the same (default) folder
    let other = w.server.url("/ok?size=2048");
    assert_eq!(
        engine
            .find_duplicate(&parsed(&other), Some("dup.bin"), None)
            .await
            .unwrap()
            .map(|d| d.id),
        Some(id)
    );
    // same name, another folder: fine
    assert!(engine
        .find_duplicate(&parsed(&other), Some("dup.bin"), Some(w.dir.path()))
        .await
        .unwrap()
        .is_none());
    // another name: fine
    assert!(engine
        .find_duplicate(&parsed(&other), Some("fresh.bin"), None)
        .await
        .unwrap()
        .is_none());
    // the on-disk name is still made unique when the user adds anyway
    let mut rx = engine.subscribe();
    let again = engine
        .add(DownloadRequest {
            filename: Some("dup.bin".into()),
            ..DownloadRequest::new(url.clone())
        })
        .await
        .unwrap();
    wait_completed(&mut rx, again).await;
    engine.resume(id).await.unwrap();
    let path = wait_completed(&mut rx, id).await;
    assert_eq!(path.file_name().unwrap(), "dup (1).bin");
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_from_the_middle_downloads_the_exact_bytes_again() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 8 * MIB;
    let id = engine
        .add(w.request(&format!("/slow?bps=4194304&size={size}")))
        .await
        .unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(10),
        |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > MIB),
    )
    .await;

    engine.restart(id).await.unwrap();
    // the running transfer leaves through Paused and goes back to Queued
    wait_state(&mut rx, id, DownloadState::Paused, Duration::from_secs(5)).await;
    wait_state(&mut rx, id, DownloadState::Queued, Duration::from_secs(5)).await;
    // progress starts again from nothing
    wait_for(&mut rx, Duration::from_secs(10), |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded < MIB / 2)).await;
    let path = wait_completed(&mut rx, id).await;
    assert_eq!(
        checksum_of(path.clone(), ChecksumAlgo::Sha256)
            .await
            .unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    // restarting a finished download deletes the file the engine wrote and re-queues it;
    // a busy queue (max_active 1) keeps it waiting so the disk can be inspected
    engine
        .set_settings(EngineSettings {
            max_active: 1,
            ..engine.settings()
        })
        .await
        .unwrap();
    let blocker = engine
        .add(DownloadRequest {
            filename: Some("blocker.bin".into()),
            ..w.request("/slow?bps=1048576&size=16777216")
        })
        .await
        .unwrap();
    wait_state(
        &mut rx,
        blocker,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    assert!(path.exists());
    engine.restart(id).await.unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(5),
        |e| matches!(e, Event::Added { id: i } if *i == id),
    )
    .await;
    assert!(!path.exists(), "the finished file is gone");
    assert!(!riptide_net::part_path_for(&path).exists());
    let record = engine.get(id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Queued);
    assert_eq!(record.size, None);
    assert_eq!(record.etag, None);
    assert_eq!(record.last_modified, None);
    assert_eq!(record.final_url, None);
    assert_eq!(record.completed_at, None);
    assert!(engine.segments(id).await.unwrap().is_empty());
    engine.remove(blocker, false).await.unwrap();
    let path = wait_completed(&mut rx, id).await;
    assert_eq!(
        checksum_of(path, ChecksumAlgo::Sha256).await.unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_of_a_paused_download_stays_paused_with_nothing_left_on_disk() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let id = engine
        .add(w.request("/slow?bps=2097152&size=8388608"))
        .await
        .unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(10),
        |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0),
    )
    .await;
    engine.pause(id).await.unwrap();
    wait_state(&mut rx, id, DownloadState::Paused, Duration::from_secs(5)).await;
    let before = engine.get(id).await.unwrap().unwrap();
    assert!(!engine.segments(id).await.unwrap().is_empty());
    assert!(riptide_net::part_path_for(&before.final_path()).exists());

    engine.restart(id).await.unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(5),
        |e| matches!(e, Event::Added { id: i } if *i == id),
    )
    .await;
    let after = engine.get(id).await.unwrap().unwrap();
    assert_eq!(after.state, DownloadState::Paused);
    assert_eq!(after.size, None);
    assert!(engine.segments(id).await.unwrap().is_empty());
    assert!(!riptide_net::part_path_for(&before.final_path()).exists());
    assert!(engine.running_ids().is_empty());
    assert!(matches!(
        engine.restart(DownloadId(999)).await,
        Err(riptide_engine::Error::NotFound(_))
    ));
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checksum_mismatch_fails_permanently_and_never_touches_a_good_file() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let downloads = w.dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("good.bin"), b"keep me").unwrap();

    let size = MIB;
    let expected = riptide_core::Checksum::new(
        ChecksumAlgo::Sha256,
        &expected_sha256_hex(DEFAULT_SEED, size),
    )
    .unwrap();
    let id = engine
        .add(DownloadRequest {
            filename: Some("good.bin".into()),
            checksum: Some(expected),
            ..w.request(&format!("/corrupt?flip=3&size={size}"))
        })
        .await
        .unwrap();
    let failed = wait_for(
        &mut rx,
        Duration::from_secs(30),
        |e| matches!(e, Event::Failed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Failed {
        error, retryable, ..
    } = failed
    else {
        unreachable!()
    };
    assert!(!retryable, "a bad digest is not a network hiccup");
    assert!(error.to_lowercase().contains("checksum"), "{error}");
    let record = engine.get(id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Failed { retryable: false });
    assert!(record
        .error
        .as_deref()
        .is_some_and(|e| e.to_lowercase().contains("checksum")));
    assert_eq!(
        std::fs::read(downloads.join("good.bin")).unwrap(),
        b"keep me",
        "the file that was already there is untouched"
    );
    assert_eq!(
        record.filename, "good (1).bin",
        "the new file got its own name"
    );
    assert!(
        !record.final_path().exists(),
        "a failed verification never produces the final file"
    );
    assert!(
        riptide_net::part_path_for(&record.final_path()).exists(),
        "the part file stays for inspection"
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn evil_content_disposition_lands_with_a_safe_name_inside_the_folder() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let id = engine.add(w.request("/cd/evil?size=2048")).await.unwrap();
    let path = wait_completed(&mut rx, id).await;
    let downloads = w.dir.path().join("downloads");
    assert_eq!(path, downloads.join("_CON.txt"));
    assert_eq!(path.parent(), Some(downloads.as_path()));
    assert_eq!(engine.get(id).await.unwrap().unwrap().filename, "_CON.txt");
    assert_eq!(
        checksum_of(path, ChecksumAlgo::Sha256).await.unwrap(),
        expected_sha256_hex(DEFAULT_SEED, 2048)
    );
    engine.shutdown().await;
}

#[cfg(windows)]
#[test]
fn free_space_answers_for_the_default_folder_before_it_exists() {
    let dir = tempfile::tempdir().unwrap();
    let free = riptide_engine::free_space(&dir.path().join("downloads")).unwrap();
    assert!(free > 0);
}
