//! End-to-end: a secret logged through `tracing` never reaches the log file.
#![allow(clippy::unwrap_used)]

use std::fs;

use riptide_log::{build, list_log_files, LogConfig};

#[test]
fn cookie_value_never_appears_in_the_log_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = LogConfig::new(tmp.path()).with_filter("trace");
    let (subscriber, guard) = build(&cfg).unwrap();

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            cookie = "session=abc123secret; theme=dark",
            url = "https://cdn.example.com/f.zip?token=zzz-token",
            "fetched"
        );
        tracing::warn!("Authorization: Bearer tok-999 was rejected");
        tracing::debug!(headers = ?vec![("Cookie", "sid=deadbeef"), ("Accept", "*/*")], "probe");
        tracing::error!(password = "hunter2", user = "bob", "auth failed");
        tracing::info!("plain line about download 7 at https://example.com/file.iso");
    });
    drop(guard); // flushes the background writer

    let files = list_log_files(tmp.path(), "riptide").unwrap();
    assert_eq!(files.len(), 1, "one daily file: {files:?}");
    let text = fs::read_to_string(&files[0]).unwrap();

    for secret in [
        "abc123secret",
        "zzz-token",
        "tok-999",
        "deadbeef",
        "hunter2",
    ] {
        assert!(
            !text.contains(secret),
            "secret {secret:?} leaked into the log:\n{text}"
        );
    }
    assert!(text.contains("[REDACTED]"), "{text}");
    assert!(text.contains("fetched"), "{text}");
    assert!(text.contains("auth failed"), "{text}");
    assert!(
        text.contains("user=\"bob\"") || text.contains("user=bob"),
        "{text}"
    );
    assert!(
        text.contains("https://cdn.example.com/f.zip?[stripped]"),
        "{text}"
    );
    assert!(
        text.contains("https://example.com/file.iso"),
        "innocent URLs stay intact:\n{text}"
    );
}
