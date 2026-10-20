//! Shared helpers for the download-matrix integration tests (not a test target itself).
#![allow(dead_code, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use riptide_core::{ChecksumAlgo, DownloadId, DownloadRequest, DownloadState, Event};
use riptide_engine::{Engine, EngineConfig};
use riptide_net::checksum_of;
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::sync::broadcast;

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * KIB;
pub const GIB: u64 = 1024 * MIB;

/// A test server, a temp folder and an engine config over it.
pub struct World {
    pub server: TestServer,
    pub dir: tempfile::TempDir,
}

impl World {
    pub async fn new() -> Self {
        World {
            server: TestServer::spawn().await,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn config(&self) -> EngineConfig {
        EngineConfig {
            db_path: self.dir.path().join("riptide.sqlite"),
            default_dir: self.downloads(),
        }
    }

    pub fn downloads(&self) -> PathBuf {
        self.dir.path().join("downloads")
    }

    pub async fn engine(&self) -> Engine {
        Engine::open(self.config()).await.unwrap()
    }

    pub fn request(&self, path: &str) -> DownloadRequest {
        DownloadRequest::new(self.server.url(path))
    }
}

/// Waits until `pred` accepts an event, or panics after `timeout` listing what did arrive.
pub async fn wait_for(
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

pub async fn wait_state(
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

/// Waits for `Completed` and returns the final path.
pub async fn wait_completed_in(
    rx: &mut broadcast::Receiver<Event>,
    id: DownloadId,
    timeout: Duration,
) -> PathBuf {
    let done = wait_for(
        rx,
        timeout,
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Completed { path, .. } = done else {
        unreachable!()
    };
    path
}

pub async fn wait_completed(rx: &mut broadcast::Receiver<Event>, id: DownloadId) -> PathBuf {
    wait_completed_in(rx, id, Duration::from_secs(120)).await
}

/// Waits for `Failed` and returns `(retryable, message)`.
pub async fn wait_failed(
    rx: &mut broadcast::Receiver<Event>,
    id: DownloadId,
    timeout: Duration,
) -> bool {
    let ev = wait_for(
        rx,
        timeout,
        |e| matches!(e, Event::Failed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Failed { retryable, .. } = ev else {
        unreachable!()
    };
    retryable
}

/// The contract every successful download must meet: size, SHA-256 of the deterministic source,
/// the final path inside `folder`, state `Completed`, every segment done and no `.rtpart` left.
pub async fn assert_complete(
    engine: &Engine,
    id: DownloadId,
    path: &Path,
    folder: &Path,
    seed: u64,
    size: u64,
) {
    let meta = std::fs::metadata(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(meta.len(), size, "size of {}", path.display());
    assert_eq!(
        checksum_of(path.to_path_buf(), ChecksumAlgo::Sha256)
            .await
            .unwrap(),
        expected_sha256_hex(seed, size),
        "sha256 of {}",
        path.display()
    );
    let canon_folder = folder.canonicalize().unwrap();
    let canon_path = path.canonicalize().unwrap();
    assert!(
        canon_path.starts_with(&canon_folder),
        "{} escaped {}",
        path.display(),
        folder.display()
    );
    let record = engine.get(id).await.unwrap().unwrap();
    assert_eq!(record.state, DownloadState::Completed);
    assert!(engine
        .segments(id)
        .await
        .unwrap()
        .iter()
        .all(|s| s.is_complete()));
    assert_no_partials(path.parent().unwrap());
}

/// No `*.rtpart` file in `dir`.
pub fn assert_no_partials(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(!name.ends_with(".rtpart"), "partial file left: {name}");
    }
}

/// SHA-256 of the default seed at `size` (convenience).
pub fn sha(size: u64) -> String {
    expected_sha256_hex(DEFAULT_SEED, size)
}

/// Percent-encodes every byte that is not an unreserved ASCII character.
pub fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
