//! Media records through the runtime with the fake yt-dlp (`riptide-media`'s
//! `riptide-fake-yt-dlp`): same states and events as HTTP, pause/resume byte-exact, DRM refused,
//! cancel cleans up. No network.
#![allow(clippy::unwrap_used, missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use riptide_core::{DownloadId, DownloadKind, DownloadRequest, DownloadState, Event};
use riptide_engine::{Engine, EngineConfig};
use riptide_media::YtDlp;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

/// The fake yt-dlp next to this test binary (`target/<profile>/`), built on demand when this
/// crate's tests run alone.
fn fake_yt_dlp() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().unwrap().parent().unwrap().to_path_buf();
    let name = format!("riptide-fake-yt-dlp{}", std::env::consts::EXE_SUFFIX);
    let path = profile_dir.join(&name);
    if !path.is_file() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = std::process::Command::new(cargo);
        cmd.args([
            "build",
            "-p",
            "riptide-media",
            "--bin",
            "riptide-fake-yt-dlp",
        ]);
        if profile_dir.file_name().is_some_and(|n| n == "release") {
            cmd.arg("--release");
        }
        assert!(cmd.status().unwrap().success(), "building the fake yt-dlp");
    }
    assert!(path.is_file(), "{}", path.display());
    path
}

async fn engine(dir: &tempfile::TempDir) -> Engine {
    let e = Engine::open(EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
        organize_new_install: false,
    })
    .await
    .unwrap();
    e.set_media_tools(Some(YtDlp::new(fake_yt_dlp(), None)));
    e
}

fn media(url: &str, format: &str) -> DownloadRequest {
    let mut r = DownloadRequest::new(url);
    r.kind = DownloadKind::Media;
    r.media_format = Some(format.into());
    r
}

fn byte(stream: u64, i: u64) -> u8 {
    ((i * 31 + stream * 7 + i / 251) % 256) as u8
}

fn expected_sha() -> String {
    let mut h = Sha256::new();
    for (stream, size) in [(0u64, 768 * 1024u64), (1, 256 * 1024)] {
        let bytes: Vec<u8> = (0..size).map(|i| byte(stream, i)).collect();
        h.update(&bytes);
    }
    hex::encode(h.finalize())
}

async fn wait_for(
    rx: &mut broadcast::Receiver<Event>,
    timeout: Duration,
    mut pred: impl FnMut(&Event) -> bool,
) -> Event {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                if pred(&ev) {
                    return ev;
                }
                seen.push(format!("{ev:?}"));
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("event channel closed: {e}"),
            Err(_) => panic!("timed out; saw: {seen:#?}"),
        }
    }
}

fn state_is(id: DownloadId, to: DownloadState) -> impl FnMut(&Event) -> bool {
    move |e| matches!(e, Event::StateChanged { id: i, to: t, .. } if *i == id && *t == to)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_download_pauses_resumes_and_completes_byte_exact() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let mut rx = engine.subscribe();
    let mut request = media("https://www.youtube.com/watch?v=slow", "h720");
    request.filename = Some("Big Buck Bunny".into());
    let id = engine.add(request).await.unwrap();

    let d = engine.get(id).await.unwrap().unwrap();
    assert_eq!(d.kind, DownloadKind::Media);
    assert_eq!(d.media_format.as_deref(), Some("h720"));
    assert_eq!(d.filename, "Big Buck Bunny.mp4");

    wait_for(&mut rx, Duration::from_secs(30), |e| {
        matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 200 * 1024)
    })
    .await;
    engine.pause(id).await.unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(20),
        state_is(id, DownloadState::Paused),
    )
    .await;
    let d = engine.get(id).await.unwrap().unwrap();
    assert!(d.size.is_some(), "the probe's estimate is the size");
    assert!(
        !riptide_media::partial_files(&d.dir, "Big Buck Bunny").is_empty(),
        "partial files stay while paused"
    );

    engine.resume(id).await.unwrap();
    let done = wait_for(
        &mut rx,
        Duration::from_secs(60),
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Completed { path, .. } = done else {
        unreachable!()
    };
    assert_eq!(path.file_name().unwrap(), "Big Buck Bunny.mp4");
    let sha = hex::encode(Sha256::digest(std::fs::read(&path).unwrap()));
    assert_eq!(sha, expected_sha());
    let d = engine.get(id).await.unwrap().unwrap();
    assert_eq!(d.state, DownloadState::Completed);
    assert_eq!(d.size, Some(1024 * 1024));
    assert!(riptide_media::partial_files(&d.dir, "Big Buck Bunny").is_empty());
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_names_come_from_the_title_drm_fails_and_cancel_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let mut rx = engine.subscribe();

    // no name given: the page title names the file, audio lands as .mp3
    let id = engine
        .add(media("https://youtu.be/aqz-KE-bpKQ", "audio-mp3"))
        .await
        .unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(30),
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let d = engine.get(id).await.unwrap().unwrap();
    assert_eq!(
        d.filename,
        "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film.mp3"
    );

    // DRM: refused at the probe, not retryable
    let drm = engine
        .add(media("https://www.youtube.com/watch?v=drm", "best"))
        .await
        .unwrap();
    let failed = wait_for(
        &mut rx,
        Duration::from_secs(30),
        |e| matches!(e, Event::Failed { id: i, .. } if *i == drm),
    )
    .await;
    let Event::Failed {
        error, retryable, ..
    } = failed
    else {
        unreachable!()
    };
    assert!(!retryable);
    assert!(error.contains("protected content"), "{error}");

    // cancel mid-way deletes yt-dlp's partial files
    let mut slow = media("https://www.youtube.com/watch?v=slow", "best");
    slow.filename = Some("Cancel me".into());
    let c = engine.add(slow).await.unwrap();
    wait_for(&mut rx, Duration::from_secs(30), |e| {
        matches!(e, Event::Progress { id: i, downloaded, .. } if *i == c && *downloaded > 100 * 1024)
    })
    .await;
    engine.cancel(c).await.unwrap();
    wait_for(
        &mut rx,
        Duration::from_secs(20),
        state_is(c, DownloadState::Cancelled),
    )
    .await;
    let d = engine.get(c).await.unwrap().unwrap();
    assert!(riptide_media::partial_files(&d.dir, "Cancel me").is_empty());

    // an unknown quality is refused at add
    assert!(engine
        .add(media("https://youtu.be/x", "bv*+ba --exec calc"))
        .await
        .is_err());
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_media_needs_the_tool_and_returns_the_menu() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let info = engine
        .probe_media("https://www.youtube.com/watch?v=aqz-KE-bpKQ", None)
        .await
        .unwrap();
    assert!(info.choices.iter().any(|c| c.key == "audio-m4a"));
    assert!(engine
        .probe_media("https://www.youtube.com/watch?v=live", None)
        .await
        .unwrap_err()
        .to_string()
        .contains("live streams are not supported"));
    engine.set_media_tools(None);
    assert!(engine
        .probe_media("https://youtu.be/x", None)
        .await
        .is_err());
    engine.shutdown().await;
}
