//! The runner against the fake yt-dlp (`src/bin/fake_yt_dlp.rs`): probe, progress, pause and
//! resume, DRM and live refusal, cookies. No network.
#![allow(clippy::unwrap_used, missing_docs)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use riptide_core::MediaSelection;
use riptide_media::args::FORBIDDEN;
use riptide_media::{partial_files, DownloadJob, Error, MediaOutcome, MediaProgress, YtDlp};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn fake() -> YtDlp {
    YtDlp::new(
        env!("CARGO_BIN_EXE_riptide-fake-yt-dlp"),
        Some(PathBuf::from("ffmpeg-not-used")),
    )
}

fn byte(stream: u64, i: u64) -> u8 {
    ((i * 31 + stream * 7 + i / 251) % 256) as u8
}

fn expected_sha(streams: &[(u64, u64)]) -> String {
    let mut h = Sha256::new();
    for (stream, size) in streams {
        let bytes: Vec<u8> = (0..*size).map(|i| byte(*stream, i)).collect();
        h.update(&bytes);
    }
    hex::encode(h.finalize())
}

fn sha_of(p: &Path) -> String {
    hex::encode(Sha256::digest(std::fs::read(p).unwrap()))
}

fn job(url: &str, dir: &Path, selection: MediaSelection) -> DownloadJob {
    DownloadJob {
        url: url.into(),
        selection,
        dir: dir.to_path_buf(),
        stem: "Big Buck Bunny".into(),
        cookies: None,
        rate_limit: None,
        proxy: None,
        expected_total: Some(1024 * 1024),
    }
}

#[tokio::test]
async fn version_and_update() {
    let y = fake();
    assert_eq!(y.version().await.unwrap(), "2099.01.01-fake");
    assert_eq!(y.update().await.unwrap(), "2099.01.01-fake");
}

#[tokio::test]
async fn probe_parses_and_refuses_drm_and_live() {
    let y = fake();
    let info = y
        .probe("https://www.youtube.com/watch?v=aqz-KE-bpKQ", Some("SID=x"))
        .await
        .unwrap();
    assert_eq!(info.id, "aqz-KE-bpKQ");
    assert!(info.choices.iter().any(|c| c.key == "h720"));
    assert!(matches!(
        y.probe("https://www.youtube.com/watch?v=drm", None).await,
        Err(Error::Drm)
    ));
    assert!(matches!(
        y.probe("https://www.youtube.com/watch?v=live", None).await,
        Err(Error::Live)
    ));
    let e = y
        .probe("https://www.youtube.com/watch?v=missing", None)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("Video unavailable"), "{e}");
    assert!(matches!(
        y.probe("--exec=calc", None).await,
        Err(Error::InvalidUrl)
    ));
}

#[tokio::test]
async fn download_merges_reports_progress_and_cleans_up_cookies() {
    let dir = tempfile::tempdir().unwrap();
    let y = fake();
    let mut j = job(
        "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
        dir.path(),
        MediaSelection::MaxHeight(720),
    );
    j.cookies = Some("SID=topsecret; PREF=x".into());
    let (tx, mut rx) = mpsc::channel::<MediaProgress>(1024);
    let out = y.download(&j, tx, CancellationToken::new()).await.unwrap();
    let MediaOutcome::Completed { path, size } = out else {
        panic!("{out:?}")
    };
    assert_eq!(path, dir.path().join("Big Buck Bunny.mp4"));
    assert_eq!(size, 1024 * 1024);
    assert_eq!(
        sha_of(&path),
        expected_sha(&[(0, 768 * 1024), (1, 256 * 1024)])
    );
    assert!(partial_files(dir.path(), "Big Buck Bunny").is_empty());

    let mut last = None;
    while let Ok(p) = rx.try_recv() {
        if let Some(prev) = last {
            let prev: MediaProgress = prev;
            assert!(p.downloaded >= prev.downloaded, "progress never goes back");
        }
        last = Some(p);
    }
    let last = last.expect("progress was reported");
    assert!(
        last.downloaded > 768 * 1024,
        "audio adds to the video: {last:?}"
    );
    assert_eq!(last.total, Some(1024 * 1024));

    let args: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("args.json")).unwrap())
            .unwrap();
    for f in FORBIDDEN {
        assert!(!args.iter().any(|a| a == f));
    }
    assert!(args
        .windows(2)
        .any(|w| w[0] == "-S" && w[1].starts_with("res:720")));
    let cookie_path = args
        .windows(2)
        .find(|w| w[0] == "--cookies")
        .map(|w| PathBuf::from(&w[1]))
        .expect("cookies passed as a file");
    assert!(
        !cookie_path.exists(),
        "the cookie file is deleted after the run"
    );
    let seen = std::fs::read_to_string(dir.path().join("cookies-seen.txt")).unwrap();
    assert!(seen.starts_with("# Netscape HTTP Cookie File"));
    assert!(seen.contains("\tSID\ttopsecret"));
}

#[tokio::test]
async fn stop_keeps_partials_and_a_second_run_resumes_byte_exact() {
    let dir = tempfile::tempdir().unwrap();
    let y = fake();
    let j = job(
        "https://www.youtube.com/watch?v=slow",
        dir.path(),
        MediaSelection::Best,
    );
    let (tx, mut rx) = mpsc::channel::<MediaProgress>(1024);
    let stop = CancellationToken::new();
    let run = {
        let (y, j, stop) = (y.clone(), j.clone(), stop.clone());
        tokio::spawn(async move { y.download(&j, tx, stop).await })
    };
    // stop in the middle of the video stream
    let mut stopped_at = 0;
    while let Some(p) = rx.recv().await {
        if p.downloaded >= 300 * 1024 {
            stopped_at = p.downloaded;
            stop.cancel();
            break;
        }
    }
    assert!(stopped_at > 0);
    let out = tokio::time::timeout(Duration::from_secs(20), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(out, MediaOutcome::Stopped);
    let partials = partial_files(dir.path(), "Big Buck Bunny");
    assert!(!partials.is_empty(), "the .part file stays for the resume");
    assert!(!dir.path().join("Big Buck Bunny.mp4").exists());

    // resume: the first progress line already counts the bytes on disk
    let (tx, mut rx) = mpsc::channel::<MediaProgress>(1024);
    let out = y.download(&j, tx, CancellationToken::new()).await.unwrap();
    let first = rx.try_recv().unwrap();
    assert!(
        first.downloaded >= stopped_at - 64 * 1024,
        "{first:?} vs {stopped_at}"
    );
    let MediaOutcome::Completed { path, .. } = out else {
        panic!("{out:?}")
    };
    assert_eq!(
        sha_of(&path),
        expected_sha(&[(0, 768 * 1024), (1, 256 * 1024)])
    );
    assert!(partial_files(dir.path(), "Big Buck Bunny").is_empty());
}

#[tokio::test]
async fn audio_only_and_failures() {
    let dir = tempfile::tempdir().unwrap();
    let y = fake();
    let (tx, _rx) = mpsc::channel(16);
    let out = y
        .download(
            &job("https://youtu.be/x", dir.path(), MediaSelection::AudioMp3),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let MediaOutcome::Completed { path, .. } = out else {
        panic!()
    };
    assert_eq!(path, dir.path().join("Big Buck Bunny.mp3"));
    assert_eq!(sha_of(&path), expected_sha(&[(1, 256 * 1024)]));

    let (tx, _rx) = mpsc::channel(16);
    let e = y
        .download(
            &job("https://youtu.be/fail", dir.path(), MediaSelection::Best),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(e.is_retryable(), "{e}");
    let (tx, _rx) = mpsc::channel(16);
    let e = y
        .download(
            &job("https://youtu.be/drm", dir.path(), MediaSelection::Best),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Drm), "{e}");
}
