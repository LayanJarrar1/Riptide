//! A server that stops sending mid-body must not hang a transfer forever: the read timeout
//! (`READ_TIMEOUT`, 60 s) turns the stall into a retryable error, and with retries disabled the
//! transfer ends `Err` with the part file kept for a later resume.
//!
//! Slow (about 65 s of real time), so it is ignored by default:
//! `cargo test -p riptide-net --test stall -- --ignored`.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use riptide_core::DownloadId;
use riptide_net::{
    build_client, part_path_for, probe, transfer, ClientOptions, HostLimiter, RequestContext,
    RetryPolicy, TransferConfig, TransferPlan,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const SIZE: u64 = 100_000;

/// Answers `HEAD` honestly; answers `GET` with a few body bytes and then goes silent.
async fn stalling_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                if head.starts_with("HEAD") {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {SIZE}\r\nAccept-Ranges: bytes\r\nETag: \"stall\"\r\n\r\n"
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    return;
                }
                let resp = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {SIZE}\r\nContent-Range: bytes 0-{}/{SIZE}\r\nETag: \"stall\"\r\n\r\n",
                    SIZE - 1
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.write_all(&[7u8; 1000]).await;
                // hold the connection open without sending anything more
                tokio::time::sleep(Duration::from_secs(3600)).await;
            });
        }
    });
    format!("http://{addr}/stall.bin")
}

#[tokio::test]
#[ignore = "takes about 65 s: waits for the 60 s read timeout"]
async fn a_stalled_body_times_out_instead_of_hanging() {
    let url = stalling_server().await;
    let dir = tempfile::tempdir().unwrap();
    let client = build_client(&ClientOptions::default()).unwrap();
    let ctx = Arc::new(RequestContext::new(&url::Url::parse(&url).unwrap()));
    let p = probe(&client, &url, &ctx).await.unwrap();
    assert_eq!(p.size, Some(SIZE));
    let final_path = dir.path().join("stall.bin");
    let plan = TransferPlan {
        download_id: DownloadId(1),
        url: p.final_url.clone(),
        size: p.size,
        supports_ranges: p.supports_ranges,
        validator: p.validator().map(str::to_owned),
        segments: Vec::new(),
        part_path: part_path_for(&final_path),
        final_path: final_path.clone(),
        checksum: None,
    };
    let cfg = TransferConfig {
        connections: 1,
        retry: RetryPolicy::none(),
        ..TransferConfig::default()
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        transfer(
            client,
            ctx,
            plan,
            cfg,
            tx,
            CancellationToken::new(),
            HostLimiter::new(8),
        ),
    )
    .await
    .expect("the transfer hung past 120 s: no read timeout");
    let err = result.expect_err("a stalled body cannot complete");
    assert!(err.is_retryable(), "a stall is a retryable error: {err}");
    assert!(started.elapsed() >= Duration::from_secs(55), "too early?");
    assert!(!final_path.exists());
    assert!(
        part_path_for(&final_path).exists(),
        "kept for a later resume"
    );
}
