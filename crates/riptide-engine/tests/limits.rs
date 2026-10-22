//! Speed limits and proxy settings through the engine API: the global limit holds a running
//! download within ±15 %, a per-download limit changed mid-transfer applies without a restart,
//! proxy settings rebuild the client and survive a reopen; bytes stay byte-exact.
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use riptide_core::{ChecksumAlgo, DownloadId, DownloadRequest, DownloadState, Event};
use riptide_engine::{Engine, EngineConfig, EngineSettings, ProxyMode, ProxySettings};
use riptide_net::checksum_of;
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;

fn config(dir: &tempfile::TempDir) -> EngineConfig {
    EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
        organize_new_install: false,
    }
}

/// Waits for `id` to reach `to`; panics after `timeout`.
async fn wait_state(
    rx: &mut broadcast::Receiver<Event>,
    id: DownloadId,
    to: DownloadState,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(Event::StateChanged { id: i, to: t, .. })) if i == id && t == to => return,
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("event channel closed: {e}"),
            Err(_) => panic!("timed out waiting for {to:?}"),
        }
    }
}

async fn sha_of(engine: &Engine, id: DownloadId) -> String {
    let d = engine.get(id).await.unwrap().unwrap();
    checksum_of(d.final_path(), ChecksumAlgo::Sha256)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_limit_holds_a_running_download_within_15_percent() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();
    let limit = MIB;
    engine
        .set_settings(EngineSettings {
            global_speed_limit: Some(limit),
            ..engine.settings()
        })
        .await
        .unwrap();
    let mut rx = engine.subscribe();
    let size = 3 * MIB;
    let id = engine
        .add(DownloadRequest::new(
            server.url(&format!("/ok?size={size}")),
        ))
        .await
        .unwrap();
    wait_state(
        &mut rx,
        id,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    let started = Instant::now();
    wait_state(
        &mut rx,
        id,
        DownloadState::Verifying,
        Duration::from_secs(10),
    )
    .await;
    let ratio = size as f64 / started.elapsed().as_secs_f64() / limit as f64;
    assert!((0.85..=1.15).contains(&ratio), "ratio {ratio:.3}");
    wait_state(
        &mut rx,
        id,
        DownloadState::Completed,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(
        sha_of(&engine, id).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_download_limit_changes_while_running_without_a_restart() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();
    let mut rx = engine.subscribe();
    let size = 16 * MIB;
    let mut request = DownloadRequest::new(server.url(&format!("/ok?size={size}")));
    request.speed_limit = Some(256 * KIB);
    let id = engine.add(request).await.unwrap();
    wait_state(
        &mut rx,
        id,
        DownloadState::Downloading,
        Duration::from_secs(5),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let stored = engine.get(id).await.unwrap().unwrap();
    assert_eq!(stored.speed_limit, Some(256 * KIB));
    assert_eq!(stored.state, DownloadState::Downloading, "still limited");

    engine.set_speed_limit(id, None).await.unwrap();
    // at 256 KiB/s the rest would need ~60 s
    wait_state(
        &mut rx,
        id,
        DownloadState::Completed,
        Duration::from_secs(8),
    )
    .await;
    assert_eq!(engine.get(id).await.unwrap().unwrap().speed_limit, None);
    assert_eq!(
        sha_of(&engine, id).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    assert!(engine
        .set_speed_limit(DownloadId(999), Some(1))
        .await
        .is_err());
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proxy_settings_validate_rebuild_the_client_and_persist() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();
    let url = server.url("/ok?size=1000");
    assert_eq!(engine.settings().proxy.mode, ProxyMode::System);
    engine.probe(&url, None, None).await.unwrap();

    let bad = EngineSettings {
        proxy: ProxySettings {
            mode: ProxyMode::Manual,
            url: "ftp://proxy.example".into(),
        },
        ..engine.settings()
    };
    assert!(
        engine.set_settings(bad).await.is_err(),
        "invalid URL refused"
    );
    assert_eq!(engine.settings().proxy.mode, ProxyMode::System);

    // a proxy that answers every request with 502: requests now fail, and it counts them
    let (proxy_port, hits) = spawn_refusing_proxy().await;
    let manual = EngineSettings {
        proxy: ProxySettings {
            mode: ProxyMode::Manual,
            url: format!("http://user:secret@127.0.0.1:{proxy_port}"),
        },
        global_speed_limit: Some(5 * MIB),
        ..engine.settings()
    };
    engine.set_settings(manual.clone()).await.unwrap();
    assert!(engine.probe(&url, None, None).await.is_err());
    assert!(
        hits.load(Ordering::SeqCst) > 0,
        "the request went through the proxy"
    );
    let shown = format!("{:?}", engine.settings());
    assert!(
        !shown.contains("secret"),
        "Debug leaks the password: {shown}"
    );
    engine.shutdown().await;
    drop(engine);

    let engine = Engine::open(config(&dir)).await.unwrap();
    assert_eq!(engine.settings().proxy, manual.proxy, "persisted");
    assert_eq!(engine.settings().global_speed_limit, Some(5 * MIB));
    let before = hits.load(Ordering::SeqCst);
    assert!(engine.probe(&url, None, None).await.is_err());
    assert!(hits.load(Ordering::SeqCst) > before);
    let before = hits.load(Ordering::SeqCst);
    engine
        .set_settings(EngineSettings {
            proxy: ProxySettings {
                mode: ProxyMode::None,
                url: String::new(),
            },
            ..engine.settings()
        })
        .await
        .unwrap();
    engine.probe(&url, None, None).await.unwrap();
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "mode none bypasses the proxy"
    );
    engine.shutdown().await;
}

const RESPONSE: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// A "proxy" that answers `502` to every request; returns its port and a hit counter.
async fn spawn_refusing_proxy() -> (u16, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                if socket.read(&mut buf).await.unwrap_or(0) > 0 {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = socket.write_all(RESPONSE).await;
                }
            });
        }
    });
    (port, hits)
}
