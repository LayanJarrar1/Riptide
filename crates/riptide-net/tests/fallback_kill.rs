//! Killing the process right at the "server lied in Content-Range" restart, and deleting /
//! truncating / extending the part file between runs, must never end in a short or zero-filled
//! file reported as complete.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use riptide_core::{DownloadId, Segment};
use riptide_net::{
    build_client, part_path_for, probe, transfer, ClientOptions, HostLimiter, Outcome,
    RequestContext, RetryPolicy, TransferConfig, TransferEvent, TransferPlan,
};
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio_util::sync::CancellationToken;

const SIZE: u64 = 1_000_000;

async fn plan_for(
    url: &str,
    final_path: &std::path::Path,
    segments: Vec<Segment>,
) -> (reqwest::Client, Arc<RequestContext>, TransferPlan) {
    let client = build_client(&ClientOptions::default()).unwrap();
    let ctx = Arc::new(RequestContext::new(&url::Url::parse(url).unwrap()));
    let p = probe(&client, url, &ctx).await.unwrap();
    let plan = TransferPlan {
        download_id: DownloadId(1),
        url: p.final_url.clone(),
        size: p.size,
        supports_ranges: p.supports_ranges,
        validator: p.validator().map(str::to_owned),
        segments,
        part_path: part_path_for(final_path),
        final_path: final_path.to_path_buf(),
        checksum: None,
    };
    (client, ctx, plan)
}

fn cfg() -> TransferConfig {
    TransferConfig {
        connections: 1,
        retry: RetryPolicy::none(),
        ..TransferConfig::default()
    }
}

async fn run_to_end(url: &str, final_path: &std::path::Path, segments: Vec<Segment>) {
    let (client, ctx, plan) = plan_for(url, final_path, segments).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let out = transfer(
        client,
        ctx,
        plan,
        cfg(),
        tx,
        CancellationToken::new(),
        HostLimiter::new(8),
    )
    .await
    .unwrap();
    assert!(matches!(out, Outcome::Completed { .. }));
}

async fn sha_of(path: &std::path::Path) -> (u64, String) {
    let len = std::fs::metadata(path).unwrap().len();
    let hex = riptide_net::checksum_of(path.to_path_buf(), riptide_core::ChecksumAlgo::Sha256)
        .await
        .unwrap();
    (len, hex)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kill_right_at_the_fallback_restart_then_resume_is_byte_exact() {
    let server = TestServer::spawn().await;
    let url = server.url(&format!("/range-lie?size={SIZE}&lie=524288&bps=1000000"));
    for round in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("lie.bin");
        let (client, ctx, plan) = plan_for(&url, &final_path, Vec::new()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
        let handle = tokio::spawn(transfer(
            client,
            ctx,
            plan,
            cfg(),
            tx,
            CancellationToken::new(),
            HostLimiter::new(8),
        ));
        // the table a database would hold when the process dies at the moment of `Restarted`
        let mut persisted: Vec<Segment> = Vec::new();
        let mut killed = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            match ev {
                TransferEvent::Checkpoint { segments } => persisted = segments,
                TransferEvent::Restarted { .. } => {
                    // a tiny, round-dependent delay moves the "kill" around the restart
                    tokio::time::sleep(Duration::from_millis(round * 40)).await;
                    handle.abort();
                    killed = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(killed, "the fallback restart never happened");
        let _ = handle.await;
        // second run: the stored table, whatever is left of the part file
        run_to_end(&url, &final_path, persisted).await;
        let (len, hex) = sha_of(&final_path).await;
        assert_eq!(len, SIZE, "round {round}");
        assert_eq!(
            hex,
            expected_sha256_hex(DEFAULT_SEED, SIZE),
            "round {round}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deleted_truncated_or_extended_part_file_restarts_instead_of_completing() {
    let server = TestServer::spawn().await;
    let url = server.url(&format!("/slow?bps=400000&size={SIZE}"));
    for what in ["delete", "truncate", "extend"] {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("p.bin");
        let part = part_path_for(&final_path);
        let (client, ctx, plan) = plan_for(&url, &final_path, Vec::new()).await;
        let stop = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
        let handle = tokio::spawn(transfer(
            client,
            ctx,
            plan,
            TransferConfig {
                connections: 2,
                retry: RetryPolicy::none(),
                checkpoint_interval: Duration::from_millis(100),
                ..TransferConfig::default()
            },
            tx,
            stop.clone(),
            HostLimiter::new(8),
        ));
        let mut persisted: Vec<Segment> = Vec::new();
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            if let TransferEvent::Checkpoint { segments } = ev {
                let got: u64 = segments.iter().map(|s| s.written).sum();
                persisted = segments;
                if got >= 200_000 {
                    stop.cancel();
                    break;
                }
            }
        }
        let _ = handle.await;
        assert!(persisted.iter().map(|s| s.written).sum::<u64>() > 0);
        match what {
            "delete" => std::fs::remove_file(&part).unwrap(),
            "truncate" => std::fs::OpenOptions::new()
                .write(true)
                .open(&part)
                .unwrap()
                .set_len(SIZE / 2)
                .unwrap(),
            _ => std::fs::OpenOptions::new()
                .write(true)
                .open(&part)
                .unwrap()
                .set_len(SIZE + 4096)
                .unwrap(),
        }
        run_to_end(&url, &final_path, persisted).await;
        let (len, hex) = sha_of(&final_path).await;
        assert_eq!(len, SIZE, "{what}");
        assert_eq!(hex, expected_sha256_hex(DEFAULT_SEED, SIZE), "{what}");
    }
}

/// Runs a transfer with the crash fault point on and returns the last stored table.
async fn crash_after_rename(url: &str, final_path: &std::path::Path) -> Vec<Segment> {
    let (client, ctx, plan) = plan_for(url, final_path, Vec::new()).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let table = tokio::spawn(async move {
        let mut last = Vec::new();
        while let Some(ev) = rx.recv().await {
            if let TransferEvent::Checkpoint { segments } = ev {
                last = segments;
            }
        }
        last
    });
    let cfg = TransferConfig {
        crash_after_rename: true,
        ..cfg()
    };
    let r = transfer(
        client,
        ctx,
        plan,
        cfg,
        tx,
        CancellationToken::new(),
        HostLimiter::new(8),
    )
    .await;
    assert!(r.is_err(), "the injected crash must surface");
    assert!(final_path.exists(), "renamed before the crash");
    assert!(!part_path_for(final_path).exists());
    table.await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn crash_between_rename_and_completed_is_adopted_on_the_next_run() {
    let server = TestServer::spawn().await;
    for route in ["ok", "noranges", "nolength"] {
        let url = server.url(&format!("/{route}?size={SIZE}"));
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("c.bin");
        let table = crash_after_rename(&url, &final_path).await;
        assert!(!table.is_empty(), "{route}: a stored table");
        run_to_end(&url, &final_path, table).await;
        let (len, hex) = sha_of(&final_path).await;
        assert_eq!(len, SIZE, "{route}");
        assert_eq!(hex, expected_sha256_hex(DEFAULT_SEED, SIZE), "{route}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_different_file_at_the_final_name_is_never_adopted_or_overwritten() {
    let server = TestServer::spawn().await;
    let url = server.url(&format!("/ok?size={SIZE}"));
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("d.bin");
    let table = crash_after_rename(&url, &final_path).await;

    // 1. another size: not ours, not touched, a clear error
    std::fs::write(&final_path, b"somebody else's data").unwrap();
    let (client, ctx, plan) = plan_for(&url, &final_path, table.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let r = transfer(
        client,
        ctx,
        plan,
        cfg(),
        tx,
        CancellationToken::new(),
        HostLimiter::new(8),
    )
    .await;
    assert!(r.is_err());
    assert_eq!(std::fs::read(&final_path).unwrap(), b"somebody else's data");

    let _ = std::fs::remove_file(part_path_for(&final_path)); // case 1 left a part file behind
                                                              // 2. same size, other content, checksum known: rejected, not touched
    let other = vec![7u8; SIZE as usize];
    std::fs::write(&final_path, &other).unwrap();
    let (client, ctx, mut plan) = plan_for(&url, &final_path, table).await;
    plan.checksum = Some(riptide_core::Checksum {
        algo: riptide_core::ChecksumAlgo::Sha256,
        hex: expected_sha256_hex(DEFAULT_SEED, SIZE),
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let r = transfer(
        client,
        ctx,
        plan,
        cfg(),
        tx,
        CancellationToken::new(),
        HostLimiter::new(8),
    )
    .await;
    assert!(
        matches!(r, Err(riptide_net::Error::ChecksumMismatch { .. })),
        "{r:?}"
    );
    assert_eq!(std::fs::read(&final_path).unwrap(), other);
}
