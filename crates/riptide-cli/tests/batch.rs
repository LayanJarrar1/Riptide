//! `riptide batch`: the real binary expands a pattern, prints it with `--list`, and downloads
//! every item byte-exact from the test server's `/batch/` endpoints.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output, Stdio};

use riptide_core::ChecksumAlgo;
use riptide_testserver::{batch_item, expected_sha256_hex, TestServer};

fn riptide(data_dir: &Path, lang: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_riptide"));
    cmd.arg("--data-dir")
        .arg(data_dir)
        .arg("--lang")
        .arg(lang)
        .args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn run(mut cmd: Command) -> Output {
    cmd.output().unwrap()
}

#[test]
fn list_prints_the_expansion_one_per_line_without_downloading() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run(riptide(
        tmp.path(),
        "en",
        &["batch", "https://h.test/img_[a-c][1-2].jpg", "--list"],
    ));
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "https://h.test/img_a1.jpg",
            "https://h.test/img_a2.jpg",
            "https://h.test/img_b1.jpg",
            "https://h.test/img_b2.jpg",
            "https://h.test/img_c1.jpg",
            "https://h.test/img_c2.jpg",
        ]
    );
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("6 addresses"));
}

#[test]
fn a_bad_pattern_fails_with_a_message_in_the_chosen_language() {
    let tmp = tempfile::tempdir().unwrap();
    for (lang, needle) in [("en", "no closing"), ("ar", "قوس")] {
        let out = run(riptide(
            tmp.path(),
            lang,
            &["batch", "https://h.test/f[1-", "--list"],
        ));
        assert!(!out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains(needle), "{lang}: {stderr}");
    }
    let out = run(riptide(
        tmp.path(),
        "en",
        &["batch", "https://h.test/f[1-99999]", "--list"],
    ));
    assert!(!out.status.success());
    assert!(String::from_utf8(out.stderr).unwrap().contains("10000"));
}

#[test]
fn help_is_localized() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run(riptide(tmp.path(), "ar", &["batch", "--help"]));
    assert!(String::from_utf8(out.stdout).unwrap().contains("نمط"));
    let out = run(riptide(tmp.path(), "en", &["batch", "--help"]));
    assert!(String::from_utf8(out.stdout).unwrap().contains("pattern"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_every_item_byte_exact_and_reports_missing_ones() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let out_dir = tmp.path().join("out");
    // file119 and file120 exist; file121 is outside the pattern (404)
    let pattern = server.url("/batch/file[119-121].zip");
    let output = tokio::task::spawn_blocking({
        let (data_dir, out_dir) = (data_dir.clone(), out_dir.clone());
        move || {
            run(riptide(
                &data_dir,
                "en",
                &["batch", &pattern, "--dir", out_dir.to_str().unwrap(), "-q"],
            ))
        }
    })
    .await
    .unwrap();
    // one item failed: non-zero exit and a summary, but the others were downloaded
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("2 of 3 downloaded, 1 failed"), "{stderr}");
    for name in ["file119.zip", "file120.zip"] {
        let item = batch_item(name).unwrap();
        let sha = riptide_net::checksum_of(out_dir.join(name), ChecksumAlgo::Sha256)
            .await
            .unwrap();
        assert_eq!(sha, expected_sha256_hex(item.seed, item.size), "{name}");
    }
    assert!(!out_dir.join("file121.zip").exists());
}
