//! Manual check against the real yt-dlp and the network (never in CI):
//!
//! ```text
//! $env:RIPTIDE_YTDLP  = "apps\desktop\src-tauri\binaries\dev\yt-dlp.exe"
//! $env:RIPTIDE_FFMPEG = "apps\desktop\src-tauri\binaries\dev\ffmpeg.exe"
//! $env:RIPTIDE_MEDIA_URL = "https://www.youtube.com/watch?v=aqz-KE-bpKQ"   # optional
//! cargo test -p riptide-media --test real -- --ignored --nocapture
//! ```
//!
//! Probes the page, downloads 720p, stops half-way, resumes, and checks the merged MP4.
#![allow(clippy::unwrap_used, missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use riptide_core::MediaSelection;
use riptide_media::{partial_files, DownloadJob, MediaOutcome, YtDlp};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
#[ignore = "needs the network and a real yt-dlp (RIPTIDE_YTDLP)"]
async fn real_probe_download_pause_resume() {
    let exe = PathBuf::from(std::env::var("RIPTIDE_YTDLP").expect("RIPTIDE_YTDLP"));
    let ffmpeg = std::env::var("RIPTIDE_FFMPEG").ok().map(PathBuf::from);
    let url = std::env::var("RIPTIDE_MEDIA_URL")
        .unwrap_or_else(|_| "https://www.youtube.com/watch?v=aqz-KE-bpKQ".into());
    let y = YtDlp::new(exe, ffmpeg);
    eprintln!("yt-dlp {}", y.version().await.unwrap());
    let info = y.probe(&url, None).await.unwrap();
    eprintln!("title: {} ({:?} s)", info.title, info.duration);
    for c in &info.choices {
        eprintln!("  {:<10} {:?} {:?}", c.key, c.height, c.approx_size);
    }
    let choice = info.choices.iter().find(|c| c.key == "h720").unwrap();

    let dir = std::env::var("RIPTIDE_MEDIA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("riptide-real-media"));
    let _ = std::fs::remove_dir_all(&dir);
    let job = DownloadJob {
        url: url.clone(),
        selection: MediaSelection::MaxHeight(720),
        dir: dir.clone(),
        stem: "Big Buck Bunny 720p".into(),
        cookies: None,
        rate_limit: None,
        proxy: None,
        expected_total: choice.approx_size,
    };

    // first run: stop after ~20 MB
    let (tx, mut rx) = mpsc::channel(256);
    let stop = CancellationToken::new();
    let run = {
        let (y, job, stop) = (y.clone(), job.clone(), stop.clone());
        tokio::spawn(async move { y.download(&job, tx, stop).await })
    };
    while let Some(p) = rx.recv().await {
        if p.downloaded > 20_000_000 {
            eprintln!("stopping at {p:?}");
            stop.cancel();
            break;
        }
    }
    let out = tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(out, MediaOutcome::Stopped);
    let partials = partial_files(&dir, &job.stem);
    eprintln!("partials after stop: {partials:?}");
    assert!(!partials.is_empty());

    // second run: resume
    let (tx, mut rx) = mpsc::channel(256);
    let resumed = tokio::spawn(async move {
        let mut first = None;
        while let Some(p) = rx.recv().await {
            first.get_or_insert(p);
        }
        first
    });
    let out = y.download(&job, tx, CancellationToken::new()).await.unwrap();
    eprintln!("first progress after resume: {:?}", resumed.await.unwrap());
    let MediaOutcome::Completed { path, size } = out else {
        panic!("{out:?}")
    };
    eprintln!("done: {} ({size} bytes)", path.display());
    assert!(size > 10_000_000);
    assert!(partial_files(&dir, &job.stem).is_empty());
}
