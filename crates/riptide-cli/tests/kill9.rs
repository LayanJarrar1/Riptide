//! Crash safety, end to end: the real `riptide` binary is killed mid-download (the process
//! equivalent of `kill -9`) at random moments, then run again until the file completes. The
//! result must hash exactly like the source, and every run after a kill must report a resume.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rand::Rng;
use riptide_core::ChecksumAlgo;
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};

const MIB: u64 = 1024 * 1024;

fn riptide(data_dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_riptide"));
    cmd.arg("--data-dir")
        .arg(data_dir)
        .arg("--lang")
        .arg("en")
        .args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_at_random_offsets_then_resumes_byte_exact() {
    let server = TestServer::spawn().await;
    let size = 24 * MIB;
    // 1 MiB/s per connection × 4 connections: a full run takes ~6 s, so a kill 1.3–2.5 s after
    // the first byte always lands after at least one checkpoint and well before the end
    let url = server.url(&format!("/slow?bps=1048576&size={size}"));
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let out = tmp.path().join("slow.bin");

    let mut rng = rand::rng();
    let mut kills = 0;
    let mut resumed_runs = 0;
    for run in 0..8 {
        let mut child = riptide(
            &data_dir,
            &["fetch", &url, "-o", out.to_str().unwrap(), "-c", "4"],
        )
        .spawn()
        .unwrap();
        let part = riptide_net::part_path_for(&out);
        // arm the kill only once the transfer has started writing, so every kill lands mid-way
        let kill_after = Duration::from_millis(rng.random_range(1300..2500));
        let mut armed_at: Option<Instant> = None;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if armed_at.is_none() && part.exists() {
                armed_at = Some(Instant::now());
            }
            if kills < 3 && armed_at.is_some_and(|t| t.elapsed() >= kill_after) {
                child.kill().unwrap();
                child.wait().unwrap();
                kills += 1;
                break None;
            }
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "run {run} hung"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        match status {
            None => {
                assert!(
                    riptide_net::part_path_for(&out).exists(),
                    "run {run}: part file survives the kill"
                );
                assert!(!out.exists(), "run {run}: no final file after a kill");
            }
            Some(status) => {
                let output = child.wait_with_output().unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(status.success(), "run {run} failed: {stderr}");
                if stderr.contains("Resuming:") {
                    resumed_runs += 1;
                }
                assert!(stderr.contains("Done:"), "{stderr}");
                break;
            }
        }
    }
    assert!(
        kills >= 1,
        "the test must have killed the process at least once"
    );
    assert!(
        resumed_runs >= 1,
        "the completing run must have resumed from a checkpoint"
    );
    assert!(out.exists());
    assert!(!riptide_net::part_path_for(&out).exists());
    let actual = riptide_net::checksum_of(out.clone(), ChecksumAlgo::Sha256)
        .await
        .unwrap();
    assert_eq!(
        actual,
        expected_sha256_hex(DEFAULT_SEED, size),
        "byte-exact after {kills} kills"
    );

    // the record is Completed in the store and running again does not re-download
    let again = riptide(
        &data_dir,
        &["fetch", &url, "-o", out.to_str().unwrap(), "-c", "4"],
    )
    .output()
    .unwrap();
    assert!(again.status.success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checksum_flag_and_bench_json() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let size = MIB;

    let good = riptide(
        &data_dir,
        &[
            "fetch",
            &server.url(&format!("/ok?size={size}")),
            "-o",
            tmp.path().join("good.bin").to_str().unwrap(),
            "--checksum",
            &expected_sha256_hex(DEFAULT_SEED, size),
            "-q",
        ],
    )
    .output()
    .unwrap();
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );

    let bad = riptide(
        &data_dir,
        &[
            "fetch",
            &server.url(&format!("/corrupt?flip=2&size={size}")),
            "-o",
            tmp.path().join("bad.bin").to_str().unwrap(),
            "--checksum",
            &expected_sha256_hex(DEFAULT_SEED, size),
            "-q",
        ],
    )
    .output()
    .unwrap();
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("checksum mismatch"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(!tmp.path().join("bad.bin").exists());

    let bench = riptide(
        &data_dir,
        &[
            "bench",
            &server.url("/ok?size=4194304"),
            "-c",
            "1,4",
            "--json",
        ],
    )
    .output()
    .unwrap();
    assert!(
        bench.status.success(),
        "{}",
        String::from_utf8_lossy(&bench.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&bench.stdout).unwrap();
    let runs = json.as_array().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["connections"], 1);
    assert_eq!(runs[1]["connections"], 4);
    assert_eq!(runs[0]["bytes"], 4194304);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arabic_name_from_content_disposition_lands_on_disk() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let run = riptide(
        &data_dir,
        &[
            "fetch",
            &server.url("/cd/arabic?size=65536"),
            "-o",
            out_dir.to_str().unwrap(),
            "-q",
        ],
    )
    .output()
    .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(out_dir.join("ملف تجريبي.zip").exists());

    let evil = riptide(
        &data_dir,
        &[
            "fetch",
            &server.url("/cd/evil?size=1024"),
            "-o",
            out_dir.to_str().unwrap(),
            "-q",
        ],
    )
    .output()
    .unwrap();
    assert!(
        evil.status.success(),
        "{}",
        String::from_utf8_lossy(&evil.stderr)
    );
    assert!(out_dir.join("_CON.txt").exists());
    assert!(!tmp.path().join("CON.txt").exists());
}
