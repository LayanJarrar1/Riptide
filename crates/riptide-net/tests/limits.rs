//! Bandwidth limits and proxies against `riptide-testserver`: measured throughput stays within
//! ±15 % of the limit, a limit changed mid-transfer applies at once, a manual proxy really
//! carries the traffic, and the bytes on disk are byte-exact (SHA-256) every time.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riptide_core::{ChecksumAlgo, DownloadId};
use riptide_net::{
    build_client, checksum_of, part_path_for, probe, transfer, ClientOptions, HostLimiter, Outcome,
    ProxyChoice, ProxyUrl, RateLimiter, RequestContext, TransferConfig, TransferPlan,
};
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;

/// Probes and transfers `url` into `dir` with `client` and the given buckets; returns the
/// transfer's own duration (probe excluded) and the SHA-256 of the result.
async fn fetch(
    client: &reqwest::Client,
    url: &str,
    dir: &std::path::Path,
    name: &str,
    rate_limits: Vec<Arc<RateLimiter>>,
) -> (Duration, String) {
    let ctx = Arc::new(RequestContext::new(&url::Url::parse(url).unwrap()));
    let probed = probe(client, url, &ctx).await.expect("probe");
    let final_path = dir.join(name);
    let plan = TransferPlan {
        download_id: DownloadId(1),
        url: probed.final_url.clone(),
        size: probed.size,
        supports_ranges: probed.supports_ranges,
        validator: probed.validator().map(str::to_owned),
        segments: Vec::new(),
        part_path: part_path_for(&final_path),
        final_path: final_path.clone(),
        checksum: None,
    };
    let cfg = TransferConfig {
        connections: 8,
        min_segment: 256 * KIB,
        rate_limits,
        ..TransferConfig::default()
    };
    let (tx, mut rx) = mpsc::channel(1024);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let started = Instant::now();
    let outcome = transfer(
        client.clone(),
        ctx,
        plan,
        cfg,
        tx,
        CancellationToken::new(),
        HostLimiter::new(32),
    )
    .await
    .expect("transfer");
    let elapsed = started.elapsed();
    drain.await.unwrap();
    assert!(matches!(outcome, Outcome::Completed { .. }), "{outcome:?}");
    let sha = checksum_of(final_path, ChecksumAlgo::Sha256).await.unwrap();
    (elapsed, sha)
}

fn assert_rate_within(bytes: u64, elapsed: Duration, limit: u64) {
    let measured = bytes as f64 / elapsed.as_secs_f64();
    let ratio = measured / limit as f64;
    assert!(
        (0.85..=1.15).contains(&ratio),
        "measured {measured:.0} B/s for a {limit} B/s limit (ratio {ratio:.3})"
    );
}

#[tokio::test]
async fn per_download_limit_holds_throughput_within_15_percent() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = build_client(&ClientOptions::default()).unwrap();
    let size = 3 * MIB;
    let limit = MIB;
    let bucket = Arc::new(RateLimiter::new(Some(limit)));
    let (elapsed, sha) = fetch(
        &client,
        &server.url(&format!("/ok?size={size}")),
        dir.path(),
        "a.bin",
        vec![bucket],
    )
    .await;
    assert_eq!(sha, expected_sha256_hex(DEFAULT_SEED, size));
    assert_rate_within(size, elapsed, limit);
}

#[tokio::test]
async fn global_limit_is_shared_by_concurrent_downloads() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = build_client(&ClientOptions::default()).unwrap();
    let size = 2 * MIB;
    let limit = 2 * MIB;
    let global = Arc::new(RateLimiter::new(Some(limit)));
    // each download also has its own limit, which never binds here
    let own_a = Arc::new(RateLimiter::new(Some(100 * MIB)));
    let own_b = Arc::new(RateLimiter::new(None));
    let url = server.url(&format!("/ok?size={size}"));
    let started = Instant::now();
    let (a, b) = tokio::join!(
        fetch(
            &client,
            &url,
            dir.path(),
            "a.bin",
            vec![own_a, Arc::clone(&global)]
        ),
        fetch(
            &client,
            &url,
            dir.path(),
            "b.bin",
            vec![own_b, Arc::clone(&global)]
        ),
    );
    let total = started.elapsed();
    let expected = expected_sha256_hex(DEFAULT_SEED, size);
    assert_eq!(a.1, expected);
    assert_eq!(b.1, expected);
    // two probes run inside `total`; they are tiny next to 2 s of transfer
    assert_rate_within(2 * size, total, limit);
}

#[tokio::test]
async fn limit_change_applies_to_a_running_transfer() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = build_client(&ClientOptions::default()).unwrap();
    let size = 8 * MIB;
    let bucket = Arc::new(RateLimiter::new(Some(256 * KIB)));
    let lift = {
        let bucket = Arc::clone(&bucket);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            bucket.set_limit(Some(64 * MIB));
        })
    };
    let (elapsed, sha) = fetch(
        &client,
        &server.url(&format!("/ok?size={size}")),
        dir.path(),
        "c.bin",
        vec![bucket],
    )
    .await;
    lift.await.unwrap();
    assert_eq!(sha, expected_sha256_hex(DEFAULT_SEED, size));
    // at the old limit this would take 32 s; with the new one it is done in about a second
    assert!(
        elapsed < Duration::from_secs(4),
        "the raised limit did not apply: {elapsed:?}"
    );
}

/// A minimal HTTP forward proxy: reads the first request head, connects to the absolute-form
/// target and then relays bytes both ways (the origin accepts absolute-form request lines).
/// Every connection's first request head is recorded.
async fn spawn_forward_proxy() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut inbound, _)) = listener.accept().await else {
                return;
            };
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if inbound.read(&mut byte).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&head).to_string();
                let target = text
                    .split_whitespace()
                    .nth(1)
                    .and_then(|u| url::Url::parse(u).ok())
                    .and_then(|u| Some(format!("{}:{}", u.host_str()?, u.port()?)));
                log.lock().unwrap().push(text);
                let Some(target) = target else { return };
                let Ok(mut outbound) = TcpStream::connect(target).await else {
                    return;
                };
                if outbound.write_all(&head).await.is_err() {
                    return;
                }
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            });
        }
    });
    (format!("127.0.0.1:{}", addr.port()), seen)
}

#[tokio::test]
async fn manual_proxy_carries_the_requests_and_direct_bypasses_it() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let (proxy_addr, seen) = spawn_forward_proxy().await;
    let size = 2 * MIB;
    let url = server.url(&format!("/ok?size={size}"));

    let proxied = build_client(&ClientOptions {
        proxy: ProxyChoice::Manual(
            ProxyUrl::parse(&format!("http://alice:s3cret@{proxy_addr}")).unwrap(),
        ),
        ..ClientOptions::default()
    })
    .unwrap();
    let (_, sha) = fetch(&proxied, &url, dir.path(), "via-proxy.bin", Vec::new()).await;
    assert_eq!(sha, expected_sha256_hex(DEFAULT_SEED, size));
    let heads = seen.lock().unwrap().clone();
    assert!(!heads.is_empty(), "nothing went through the proxy");
    let first = &heads[0];
    assert!(
        first.contains(&format!("{}/ok?size={size}", server.base_url())),
        "absolute-form request to the origin: {first}"
    );
    assert!(
        first
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic"),
        "credentials from the proxy URL are sent to the proxy: {first}"
    );

    let before = seen.lock().unwrap().len();
    let direct = build_client(&ClientOptions {
        proxy: ProxyChoice::Direct,
        ..ClientOptions::default()
    })
    .unwrap();
    let (_, sha) = fetch(&direct, &url, dir.path(), "direct.bin", Vec::new()).await;
    assert_eq!(sha, expected_sha256_hex(DEFAULT_SEED, size));
    assert_eq!(
        seen.lock().unwrap().len(),
        before,
        "direct mode used the proxy"
    );
}
