//! `riptide fetch` marks its result as downloaded from the internet (Windows / NTFS).
#![cfg(windows)]
#![allow(clippy::unwrap_used)]

use std::process::{Command, Stdio};

use riptide_testserver::TestServer;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_writes_the_zone_identifier_stream() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("file.bin");
    let url = server.url("/ok?size=200000&token=SECRET");
    let status = Command::new(env!("CARGO_BIN_EXE_riptide"))
        .arg("--data-dir")
        .arg(tmp.path().join("data"))
        .args(["--lang", "en", "fetch", &url, "-o"])
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let mut stream = out.clone().into_os_string();
    stream.push(":Zone.Identifier");
    let text = std::fs::read_to_string(stream).unwrap();
    assert!(
        text.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"),
        "{text:?}"
    );
    assert!(!text.contains("SECRET"), "{text:?}");
}
