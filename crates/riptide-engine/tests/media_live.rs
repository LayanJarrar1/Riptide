//! The real thing: the pinned yt-dlp and ffmpeg against a real video page. Needs the network,
//! so it never runs in CI; run it by hand after staging the tools:
//!
//! ```text
//! node apps/desktop/scripts/fetch-media-tools.mjs
//! RIPTIDE_YTDLP=…/yt-dlp-<target>.exe RIPTIDE_FFMPEG=…/ffmpeg-<target>.exe \
//!   cargo test -p riptide-engine --test media_live -- --ignored --nocapture
//! ```
//!
//! `RIPTIDE_LIVE_MEDIA_URL` picks another page (default: Big Buck Bunny on YouTube, uploaded by
//! the Blender Foundation under CC BY 3.0). The test probes, downloads the smallest video
//! quality with a pause and resume in the middle, and checks that ffmpeg merged a playable file.
#![allow(clippy::unwrap_used, missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use riptide_core::{DownloadKind, DownloadRequest, DownloadState, Event, MediaChoiceKind};
use riptide_engine::{Engine, EngineConfig};
use riptide_media::{Attempt, PlayerClients, YtDlp};
use tokio::sync::broadcast;

const DEFAULT_URL: &str = "https://www.youtube.com/watch?v=aqz-KE-bpKQ";

fn tool(var: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(var).unwrap_or_else(|| panic!("set {var}")));
    assert!(path.is_file(), "{var} = {} is not a file", path.display());
    path
}

async fn next(
    rx: &mut broadcast::Receiver<Event>,
    timeout: Duration,
    mut pred: impl FnMut(&Event) -> bool,
) -> Event {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) if pred(&ev) => return ev,
            Ok(Ok(Event::Failed { error, .. })) => panic!("download failed: {error}"),
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("event channel closed: {e}"),
            Err(_) => panic!("timed out"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "network: real yt-dlp + ffmpeg against a real video page"]
async fn real_video_probes_pauses_resumes_and_merges() {
    let url = std::env::var("RIPTIDE_LIVE_MEDIA_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let ffmpeg = tool("RIPTIDE_FFMPEG");
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
        organize_new_install: false,
    })
    .await
    .unwrap();
    engine.set_media_tools(Some(YtDlp::new(
        tool("RIPTIDE_YTDLP"),
        Some(ffmpeg.clone()),
    )));

    let info = engine.probe_media(&url, None).await.unwrap();
    eprintln!("title: {:?}, {} choices", info.title, info.choices.len());
    for c in &info.choices {
        eprintln!(
            "  {} {:?} {:?} {} ~{:?}",
            c.key, c.kind, c.height, c.ext, c.approx_size
        );
    }
    assert!(!info.title.is_empty());
    assert!(info.choices.iter().any(|c| c.key == "best"));
    // the smallest video quality keeps the test short but still needs a video+audio merge
    let choice = info
        .choices
        .iter()
        .filter(|c| c.kind == MediaChoiceKind::Video)
        .min_by_key(|c| c.height.unwrap_or(u32::MAX))
        .expect("a video quality");
    eprintln!("downloading {}", choice.key);

    let mut rx = engine.subscribe();
    let mut request = DownloadRequest::new(&url);
    request.kind = DownloadKind::Media;
    request.media_format = Some(choice.key.clone());
    let id = engine.add(request).await.unwrap();

    next(&mut rx, Duration::from_secs(180), |e| {
        matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 512 * 1024)
    })
    .await;
    engine.pause(id).await.unwrap();
    next(
        &mut rx,
        Duration::from_secs(30),
        |e| matches!(e, Event::StateChanged { id: i, to: DownloadState::Paused, .. } if *i == id),
    )
    .await;
    eprintln!("paused; resuming");
    engine.resume(id).await.unwrap();
    let done = next(
        &mut rx,
        Duration::from_secs(900),
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
    let Event::Completed { path, .. } = done else {
        unreachable!()
    };
    let size = std::fs::metadata(&path).unwrap().len();
    eprintln!("completed: {} ({size} bytes)", path.display());
    assert!(size > 100 * 1024);
    assert!(
        path.starts_with(dir.path()),
        "written inside the download folder"
    );

    // ffmpeg can read the merged file end to end, and it has both a video and an audio stream
    let out = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args(["-map", "0", "-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ffmpeg decode: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let probe = std::process::Command::new(&ffmpeg)
        .arg("-i")
        .arg(&path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stderr);
    assert!(text.contains("Video:"), "{text}");
    assert!(text.contains("Audio:"), "{text}");

    let d = engine.get(id).await.unwrap().unwrap();
    assert_eq!(d.state, DownloadState::Completed);
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    assert!(
        riptide_media::partial_files(&d.dir, &stem).is_empty(),
        "no partial files left"
    );
    engine.shutdown().await;
}

/// For each reference video and each rung of the player-client ladder (`default`,
/// `tv_embedded`): probes without cookies, then downloads a real 6 second clip, and prints which
/// rungs worked. Run it on a machine where the field test failed with "The page needs to be
/// reloaded." to see which rung works there:
///
/// ```text
/// RIPTIDE_YTDLP=... RIPTIDE_FFMPEG=... cargo test -p riptide-engine --test media_live ///   real_videos_report_the_working_strategy -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "network: real yt-dlp against real video pages"]
async fn real_videos_report_the_working_strategy() {
    let ffmpeg = tool("RIPTIDE_FFMPEG");
    let exe = tool("RIPTIDE_YTDLP");
    let tools = YtDlp::new(exe.clone(), Some(ffmpeg.clone()));
    eprintln!("yt-dlp {}", tools.version().await.unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    for id in ["aqz-KE-bpKQ", "Ng274mLQ7-c"] {
        let url = format!("https://www.youtube.com/watch?v={id}");
        // what the engine itself would pick (no cookies, ladder from the top)
        match tools.probe_resilient(&url, None, None).await {
            Ok((info, attempt)) => eprintln!(
                "{id}: engine strategy: clients={} cookies={} ({} choices)",
                attempt.clients.key(),
                attempt.cookies,
                info.choices.len()
            ),
            Err(e) => eprintln!("{id}: engine strategy: FAILED: {e}"),
        }
        let mut worked = Vec::new();
        for clients in PlayerClients::LADDER {
            // the rung on its own: probe, then a real 6 s clip (smallest format, merged by ffmpeg)
            let single = Attempt {
                cookies: false,
                clients,
            };
            let probe_ok = matches!(
                tools.probe_resilient(&url, None, Some(single)).await,
                Ok((_, a)) if a == single
            );
            let out = dir.path().join(format!("{id}-{}.%(ext)s", clients.key()));
            let mut cmd = std::process::Command::new(&exe);
            cmd.args(["--ignore-config", "--no-playlist", "--no-color"])
                .args(["-f", "wv*+wa/w", "--merge-output-format", "mp4"])
                .args(["--download-sections", "*0-6", "--ffmpeg-location"])
                .arg(&ffmpeg);
            if let Some(extra) = clients.extractor_args() {
                cmd.args(["--extractor-args", extra]);
            }
            cmd.arg("-o").arg(&out).arg("--").arg(&url);
            let clip = cmd.output().unwrap();
            let clip_ok = clip.status.success()
                && std::fs::read_dir(dir.path()).unwrap().flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.starts_with(&format!("{id}-{}.", clients.key()))
                        && n.ends_with(".mp4")
                        && e.metadata().is_ok_and(|m| m.len() > 10_000)
                });
            eprintln!(
                "{id}: rung {:<12} probe={} clip={}",
                clients.key(),
                if probe_ok { "ok" } else { "FAILED" },
                if clip_ok { "ok" } else { "FAILED" }
            );
            if !clip_ok {
                let err = String::from_utf8_lossy(&clip.stderr);
                if let Some(line) = err.lines().rev().find(|l| l.starts_with("ERROR")) {
                    eprintln!("    {line}");
                }
            }
            if probe_ok && clip_ok {
                worked.push(clients.key());
            }
        }
        eprintln!("{id}: working rungs: {worked:?}");
        if worked.is_empty() {
            failures.push(id);
        }
    }
    assert!(failures.is_empty(), "no rung worked for {failures:?}");
}
