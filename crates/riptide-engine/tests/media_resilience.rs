//! The media retry policy end to end with the fake yt-dlp: cookies are never sent first and
//! only when a login is needed, a refused page climbs the player-client ladder, the download
//! reuses what the probe found (byte-exact), a final failure carries the friendly error class,
//! and cookie values never reach the logs. No network.
#![allow(clippy::unwrap_used, missing_docs)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use riptide_core::{DownloadId, DownloadKind, DownloadRequest, Event, Secrets};
use riptide_engine::{Engine, EngineConfig};
use riptide_media::YtDlp;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

const SECRET: &str = "SID=SECRETCOOKIEVALUE123";

static LOGS: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct Capture;
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        LOGS.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(|| Capture)
            .try_init();
    });
}

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
    path
}

fn expected_sha() -> String {
    let byte = |stream: u64, i: u64| ((i * 31 + stream * 7 + i / 251) % 256) as u8;
    let mut h = Sha256::new();
    for (stream, size) in [(0u64, 768 * 1024u64), (1, 256 * 1024)] {
        let bytes: Vec<u8> = (0..size).map(|i| byte(stream, i)).collect();
        h.update(&bytes);
    }
    hex::encode(h.finalize())
}

async fn open_engine(dir: &tempfile::TempDir) -> Engine {
    capture_logs();
    let e = Engine::open(EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
    })
    .await
    .unwrap();
    e.set_media_tools(Some(YtDlp::new(fake_yt_dlp(), None)));
    e
}

/// A media request for a fake scenario page with the browser's cookies attached.
fn request(scenario: &str, log: &str) -> DownloadRequest {
    let mut r = DownloadRequest::new(format!(
        "https://www.youtube.com/watch?v={scenario}&log={log}"
    ));
    r.kind = DownloadKind::Media;
    r.media_format = Some("best".into());
    r.filename = Some("Clip".into());
    r.secrets = Some(Secrets {
        cookies: Some(SECRET.into()),
        ..Secrets::default()
    });
    r
}

/// What one fake yt-dlp run recorded: (mode, cookies passed, `--extractor-args` value).
type Run = (String, bool, Option<String>);

fn runs(log: &str) -> Vec<Run> {
    let path = std::env::temp_dir().join(format!("riptide-fake-{log}.log"));
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    text.lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            let a = v["args"].as_array().unwrap();
            let clients = a
                .iter()
                .position(|x| x == "--extractor-args")
                .map(|i| a[i + 1].as_str().unwrap().to_string());
            (
                v["mode"].as_str().unwrap().to_string(),
                v["cookies"].as_bool().unwrap(),
                clients,
            )
        })
        .collect()
}

async fn finish(rx: &mut broadcast::Receiver<Event>, id: DownloadId) -> Event {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Ok(ev @ (Event::Completed { id: i, .. } | Event::Failed { id: i, .. })))
                if i == id =>
            {
                return ev;
            }
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("channel closed: {e}"),
            Err(_) => panic!("timed out"),
        }
    }
}

async fn run_ok(scenario: &str, log: &str) -> Vec<Run> {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(&dir).await;
    let mut rx = engine.subscribe();
    let id = engine.add(request(scenario, log)).await.unwrap();
    let Event::Completed { path, .. } = finish(&mut rx, id).await else {
        panic!("{scenario} should complete");
    };
    let sha = hex::encode(Sha256::digest(std::fs::read(&path).unwrap()));
    assert_eq!(sha, expected_sha(), "byte-exact");
    engine.shutdown().await;
    runs(log)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_cookies_on_the_first_attempt_or_in_the_download() {
    let r = run_ok("plain", "e-plain").await;
    assert_eq!(r.len(), 2, "{r:?}");
    assert_eq!(r[0].0, "probe");
    assert_eq!(r[1].0, "download");
    assert!(
        r.iter().all(|(_, cookies, _)| !cookies),
        "cookies never passed: {r:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn age_gate_retries_with_cookies_and_the_download_keeps_them() {
    let r = run_ok("agegate", "e-age").await;
    let shape: Vec<(&str, bool)> = r.iter().map(|(m, c, _)| (m.as_str(), *c)).collect();
    assert_eq!(
        shape,
        [("probe", false), ("probe", true), ("download", true)],
        "{r:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_cookie_page_succeeds_without_cookies() {
    let r = run_ok("reloadcookies", "e-stale").await;
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(r.iter().all(|(_, cookies, _)| !cookies), "{r:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_page_succeeds_on_the_second_client_and_the_download_reuses_it() {
    let r = run_ok("needclient", "e-ladder").await;
    let shape: Vec<(&str, Option<&str>)> = r
        .iter()
        .map(|(m, _, c)| (m.as_str(), c.as_deref()))
        .collect();
    let android = Some("youtube:player_client=android_vr");
    assert_eq!(
        shape,
        [("probe", None), ("probe", android), ("download", android)],
        "{r:?}"
    );
    assert!(r.iter().all(|(_, cookies, _)| !cookies));
    // and the third step of the ladder works the same way
    let r = run_ok("needclient-tv", "e-ladder-tv").await;
    assert_eq!(r.len(), 4, "{r:?}");
    assert_eq!(r[3].2.as_deref(), Some("youtube:player_client=tv,mweb"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_final_refusal_surfaces_the_friendly_class_and_never_logs_cookie_values() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(&dir).await;
    let mut rx = engine.subscribe();
    let id = engine
        .add(request("alwaysrefuse", "e-refuse"))
        .await
        .unwrap();
    let Event::Failed {
        error, retryable, ..
    } = finish(&mut rx, id).await
    else {
        panic!("should fail");
    };
    assert!(error.starts_with("[rt:refused] "), "{error}");
    assert!(error.contains("page needs to be reloaded"), "{error}");
    assert!(error.contains("JavaScript runtime"), "{error}");
    assert!(retryable, "a site refusal is worth another try later");
    assert_eq!(runs("e-refuse").len(), 4, "bounded at four attempts");
    let stored = engine
        .get(id)
        .await
        .unwrap()
        .unwrap()
        .error
        .unwrap_or_default();
    assert!(!stored.contains("SECRETCOOKIEVALUE"));
    engine.shutdown().await;

    // a login wall with no cookies to offer: the login class
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(&dir).await;
    let mut rx = engine.subscribe();
    let mut r = request("agegate", "e-login");
    r.secrets = None;
    let id = engine.add(r).await.unwrap();
    let Event::Failed { error, .. } = finish(&mut rx, id).await else {
        panic!("should fail");
    };
    assert!(error.starts_with("[rt:login] "), "{error}");
    let _ = runs("e-login");
    engine.shutdown().await;

    let logs = String::from_utf8_lossy(&LOGS.lock().unwrap()).into_owned();
    assert!(logs.contains("yt-dlp probe"), "the capture works");
    assert!(
        !logs.contains("SECRETCOOKIEVALUE"),
        "cookie values must never be logged"
    );
}
