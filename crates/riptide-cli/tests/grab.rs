//! `riptide grab` end to end: the real binary against `/site/` on the in-process test server.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output, Stdio};

use riptide_net::checksum_of;
use riptide_testserver::{site_file_sha256_hex, TestServer};

fn riptide(data_dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_riptide"));
    cmd.arg("--data-dir").arg(data_dir).args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

async fn run(mut cmd: Command) -> Output {
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

fn as_refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_prints_the_files_without_downloading_anything() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("out");
    let url = server.url("/site/");
    let out = run(riptide(
        &tmp.path().join("data"),
        &[
            "--lang",
            "en",
            "grab",
            &url,
            "--depth",
            "3",
            "--types",
            "video,documents",
            "--list",
            "-o",
            out_dir.to_str().unwrap(),
        ],
    ))
    .await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort_unstable();
    // video: clip.mp4; documents: report.pdf, notes.txt, export.csv (sizes come from the probe)
    assert_eq!(lines.len(), 4, "{stdout}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("video\t") && l.contains("clip.mp4")),
        "{stdout}"
    );
    assert!(lines
        .iter()
        .any(|l| l.starts_with("documents\t") && l.contains("export.csv")));
    assert!(
        lines.iter().all(|l| !l.contains("secret.pdf")),
        "robots.txt keeps it out"
    );
    assert!(
        text(&out.stderr).contains("Found 4 file(s)"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        text(&out.stderr).contains("robots.txt"),
        "{}",
        text(&out.stderr)
    );
    let downloaded = std::fs::read_dir(&out_dir).unwrap().count();
    assert_eq!(downloaded, 0, "--list downloads nothing");

    // JSON
    let out = run(riptide(
        &tmp.path().join("data"),
        &["grab", &url, "--depth", "0", "--list", "--json", "-q"],
    ))
    .await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["stop"], "finished");
    let files = json["files"].as_array().unwrap();
    assert_eq!(files.len(), 4, "{json}");
    assert!(files
        .iter()
        .all(|f| f["size"].is_u64() && f["referrer"] == url.as_str()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grab_downloads_every_file_byte_exact_and_skips_what_exists() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let out_dir = tmp.path().join("out");
    let url = server.url("/site/");
    let args = |lang: &'static str| -> Vec<String> {
        [
            "--lang",
            lang,
            "grab",
            &url,
            "--depth",
            "3",
            "--include",
            "**/files/**",
            "-o",
            out_dir.to_str().unwrap(),
        ]
        .map(String::from)
        .to_vec()
    };

    let a = args("en");
    let out = run(riptide(&data, &as_refs(&a))).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("7 downloaded, 0 failed"),
        "{}",
        text(&out.stdout)
    );
    for name in [
        "report.pdf",
        "song.mp3",
        "clip.mp4",
        "archive.zip",
        "photo.jpg",
        "notes.txt",
        "setup.exe",
    ] {
        let path = out_dir.join(name);
        assert_eq!(
            checksum_of(path, riptide_core::ChecksumAlgo::Sha256)
                .await
                .unwrap(),
            site_file_sha256_hex(&format!("files/{name}")).unwrap(),
            "{name}"
        );
    }
    assert_eq!(
        std::fs::read_dir(&out_dir).unwrap().count(),
        7,
        "nothing else, no .rtpart"
    );

    // again, in Arabic: everything is there already
    let b = args("ar");
    let out = run(riptide(&data, &as_refs(&b))).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("تخطي 7 ملف"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        text(&out.stdout).contains("تم تحميل 0 ملف"),
        "{}",
        text(&out.stdout)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_arguments_fail_with_a_message() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let out = run(riptide(
        &data,
        &[
            "grab",
            "http://127.0.0.1:1/",
            "--types",
            "nonsense",
            "--list",
        ],
    ))
    .await;
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("unknown file type"),
        "{}",
        text(&out.stderr)
    );

    let out = run(riptide(&data, &["grab", "ftp://example.com/", "--list"])).await;
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("invalid start URL"),
        "{}",
        text(&out.stderr)
    );
}
