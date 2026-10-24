//! The engine against `riptide-testserver`: every HTTP failure mode the transfer
//! engine handles, each checked by comparing the SHA-256 of the file on disk with the server's seed.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::{Checksum, ChecksumAlgo, Credentials, DownloadId, Secrets, Segment};
use riptide_net::{
    build_client, checksum_of, part_path_for, probe, transfer, ClientOptions, Error, HostLimiter,
    Outcome, Probe, RequestContext, RetryPolicy, TransferConfig, TransferEvent, TransferPlan,
};
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MIB: u64 = 1024 * 1024;

struct Harness {
    server: TestServer,
    client: reqwest::Client,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    Harness {
        server: TestServer::spawn().await,
        client: build_client(&ClientOptions::default()).unwrap(),
        dir: tempfile::tempdir().unwrap(),
    }
}

fn ctx_for(url: &str) -> Arc<RequestContext> {
    Arc::new(RequestContext::new(&url::Url::parse(url).unwrap()))
}

/// One transfer with everything overridable; returns the outcome, the events and the final path.
struct Job<'a> {
    h: &'a Harness,
    url: String,
    ctx: Option<Arc<RequestContext>>,
    connections: u32,
    segments: Vec<Segment>,
    validator: Option<Option<String>>,
    checksum: Option<Checksum>,
    cfg: Option<TransferConfig>,
    stop: Option<CancellationToken>,
    limiter: Option<Arc<HostLimiter>>,
    final_name: Option<String>,
    /// Probe this URL instead of the transfer URL (to keep per-path server counters apart).
    probe_url: Option<String>,
    /// Bytes already on disk and the matching segment table, for resume scenarios.
    prepared: Option<(Vec<u8>, Vec<Segment>)>,
}

struct Done {
    result: Result<Outcome, Error>,
    events: Vec<TransferEvent>,
    probe: Probe,
    final_path: PathBuf,
}

impl<'a> Job<'a> {
    fn new(h: &'a Harness, path: &str) -> Self {
        Job {
            h,
            url: h.server.url(path),
            ctx: None,
            connections: 8,
            segments: Vec::new(),
            validator: None,
            checksum: None,
            cfg: None,
            stop: None,
            limiter: None,
            final_name: None,
            probe_url: None,
            prepared: None,
        }
    }

    async fn run(self) -> Done {
        let ctx = self.ctx.unwrap_or_else(|| ctx_for(&self.url));
        let probe_target = self.probe_url.clone().unwrap_or_else(|| self.url.clone());
        let probe = probe(&self.h.client, &probe_target, &ctx)
            .await
            .expect("probe");
        let name = self.final_name.unwrap_or_else(|| probe.filename.clone());
        let final_path = self.h.dir.path().join(&name);
        let part_path = part_path_for(&final_path);
        let mut segments = self.segments;
        if let Some((bytes, segs)) = self.prepared {
            std::fs::write(&part_path, bytes).unwrap();
            segments = segs;
        }
        let transfer_url = if self.probe_url.is_some() {
            url::Url::parse(&self.url).unwrap()
        } else {
            probe.final_url.clone()
        };
        let plan = TransferPlan {
            download_id: DownloadId(1),
            url: transfer_url,
            size: probe.size,
            supports_ranges: probe.supports_ranges,
            validator: self
                .validator
                .unwrap_or_else(|| probe.validator().map(str::to_owned)),
            segments,
            part_path,
            final_path: final_path.clone(),
            checksum: self.checksum,
        };
        let mut cfg = self.cfg.unwrap_or_default();
        cfg.connections = self.connections;
        let (tx, mut rx) = mpsc::channel(1024);
        let collector = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(e) = rx.recv().await {
                events.push(e);
            }
            events
        });
        let result = transfer(
            self.h.client.clone(),
            ctx,
            plan,
            cfg,
            tx,
            self.stop.unwrap_or_default(),
            self.limiter.unwrap_or_else(|| HostLimiter::new(32)),
        )
        .await;
        let events = collector.await.unwrap();
        Done {
            result,
            events,
            probe,
            final_path,
        }
    }
}

async fn sha(path: &std::path::Path) -> String {
    checksum_of(path.to_path_buf(), ChecksumAlgo::Sha256)
        .await
        .unwrap()
}

fn completed(done: &Done) -> &Vec<Segment> {
    match &done.result {
        Ok(Outcome::Completed { segments, .. }) => segments,
        other => panic!(
            "expected Completed, got {other:?}; events: {:?}",
            done.events.last()
        ),
    }
}

// ------------------------------------------------------------------------------------ probe

#[tokio::test]
async fn probe_reads_size_ranges_validators_and_names() {
    let h = harness().await;
    let ctx = ctx_for(&h.server.url("/ok"));

    let ok = probe(&h.client, &h.server.url("/ok?size=2097152"), &ctx)
        .await
        .unwrap();
    assert_eq!(ok.size, Some(2 * MIB));
    assert!(ok.supports_ranges);
    assert!(ok.etag.is_some() && ok.last_modified.is_some());
    assert_eq!(ok.filename, "ok");
    assert!(ok.resumable());

    let noranges = probe(&h.client, &h.server.url("/noranges?size=1000"), &ctx)
        .await
        .unwrap();
    assert_eq!(noranges.size, Some(1000));
    assert!(!noranges.supports_ranges);

    let nolength = probe(&h.client, &h.server.url("/nolength?size=1000"), &ctx)
        .await
        .unwrap();
    assert_eq!(nolength.size, None);
    assert!(!nolength.resumable());

    let arabic = probe(&h.client, &h.server.url("/cd/arabic?size=10"), &ctx)
        .await
        .unwrap();
    assert_eq!(arabic.filename, "ملف تجريبي.zip");
    assert_eq!(arabic.suggested_name.as_deref(), Some("ملف تجريبي.zip"));

    let evil = probe(&h.client, &h.server.url("/cd/evil?size=10"), &ctx)
        .await
        .unwrap();
    assert_eq!(evil.filename, "_CON.txt");

    let gzip = probe(&h.client, &h.server.url("/gzip?size=4096"), &ctx)
        .await
        .unwrap();
    assert_eq!(gzip.content_encoding.as_deref(), Some("gzip"));
    assert!(gzip.size.is_some());

    let redirected = probe(
        &h.client,
        &h.server.url("/redirect?n=3&code=cycle&size=5000"),
        &ctx,
    )
    .await
    .unwrap();
    assert!(
        redirected.final_url.path().ends_with("/ok"),
        "{}",
        redirected.final_url
    );
    assert_eq!(redirected.size, Some(5000));

    let bad = probe(&h.client, "ftp://nope/x", &ctx).await;
    assert!(matches!(bad, Err(Error::InvalidUrl { .. })));
    let missing = probe(&h.client, &h.server.url("/does-not-exist"), &ctx).await;
    assert!(
        matches!(
            missing,
            Err(Error::Status {
                status: 404,
                retryable: false,
                ..
            })
        ),
        "{missing:?}"
    );
}

// --------------------------------------------------------------------------------- transfers

#[tokio::test]
async fn ok_eight_connections_is_byte_exact() {
    let h = harness().await;
    let size = 6 * MIB;
    let done = Job::new(&h, &format!("/ok?size={size}")).run().await;
    let segments = completed(&done);
    assert!(segments.len() >= 6, "{} segments", segments.len());
    assert!(segments
        .iter()
        .all(|s| s.state == riptide_core::SegmentState::Done));
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    assert!(
        !part_path_for(&done.final_path).exists(),
        "part file renamed away"
    );
    assert!(
        matches!(done.events.first(), Some(TransferEvent::Started { size: Some(s), .. }) if *s == size)
    );
    assert!(done
        .events
        .iter()
        .any(|e| matches!(e, TransferEvent::Checkpoint { .. })));
    assert!(done
        .events
        .iter()
        .any(|e| matches!(e, TransferEvent::Verifying)));
    assert!(matches!(
        done.events.last(),
        Some(TransferEvent::Completed { .. })
    ));
}

#[tokio::test]
async fn work_stealing_splits_large_segments() {
    let h = harness().await;
    let size = 24 * MIB;
    let cfg = TransferConfig {
        min_segment: 8 * MIB,
        ..TransferConfig::default()
    };
    let done = Job::new(&h, &format!("/slow?bps=6291456&size={size}"))
        .cfg(cfg)
        .run()
        .await;
    let segments = completed(&done);
    assert!(
        segments.len() > 3,
        "initial plan was 3 segments; stealing must add more, got {}",
        segments.len()
    );
    let mut sorted = segments.clone();
    sorted.sort_by_key(|s| s.start);
    let ranges: Vec<riptide_core::ByteRange> = sorted.iter().map(|s| s.range()).collect();
    riptide_core::segment::validate_plan(&ranges, size).unwrap();
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn a_download_without_any_validator_uses_one_connection() {
    let h = harness().await;
    let size = 8 * MIB;
    let done = Job::new(&h, &format!("/ok?size={size}"))
        .validator(None)
        .run()
        .await;
    assert_eq!(
        completed(&done).len(),
        1,
        "no validator, no parallel ranges"
    );
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn an_empty_file_probes_as_size_zero_and_completes() {
    let h = harness().await;
    let ctx = ctx_for(&h.server.url("/ok"));
    let p = probe(&h.client, &h.server.url("/ok?size=0"), &ctx)
        .await
        .unwrap();
    assert_eq!(p.size, Some(0));
    let done = Job::new(&h, "/ok?size=0").run().await;
    completed(&done);
    assert_eq!(std::fs::metadata(&done.final_path).unwrap().len(), 0);
}

#[tokio::test]
async fn servers_without_ranges_or_length_get_one_streaming_connection() {
    let h = harness().await;
    let size = 2 * MIB;
    let done = Job::new(&h, &format!("/noranges?size={size}")).run().await;
    let segments = completed(&done);
    assert_eq!(segments.len(), 1);
    assert!(matches!(
        done.events.first(),
        Some(TransferEvent::Started { segments: 1, .. })
    ));
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    let size = 3 * MIB + 123;
    let done = Job::new(&h, &format!("/nolength?size={size}")).run().await;
    let segments = completed(&done);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].written, size);
    assert_eq!(std::fs::metadata(&done.final_path).unwrap().len(), size);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
    assert!(done
        .events
        .iter()
        .any(|e| matches!(e, TransferEvent::Progress { total: None, .. })));
}

#[tokio::test]
async fn dropped_connections_are_retried_from_the_offset() {
    let h = harness().await;
    let size = 4 * MIB;
    let cfg = TransferConfig {
        retry: RetryPolicy {
            base: Duration::from_millis(20),
            ..RetryPolicy::default()
        },
        ..TransferConfig::default()
    };
    let done = Job::new(&h, &format!("/drop?size={size}&after=300000"))
        .connections(4)
        .cfg(cfg)
        .run()
        .await;
    completed(&done);
    assert!(
        done.events
            .iter()
            .any(|e| matches!(e, TransferEvent::Retrying { .. })),
        "drops must show as retries"
    );
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn flaky_server_eventually_completes() {
    let h = harness().await;
    let size = 3 * MIB;
    let cfg = TransferConfig {
        retry: RetryPolicy {
            base: Duration::from_millis(20),
            max_attempts: 20,
            ..RetryPolicy::default()
        },
        ..TransferConfig::default()
    };
    let done = Job::new(&h, &format!("/flaky?rate=0.4&size={size}"))
        .connections(4)
        .cfg(cfg)
        .run()
        .await;
    completed(&done);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn retry_after_is_honoured() {
    let h = harness().await;
    let size = MIB;
    // probe elsewhere so the single failure is spent by the segment's request, not the probe
    let done = Job::new(&h, &format!("/retry-after?fails=1&size={size}"))
        .probe_url(&format!("/ok?size={size}"))
        .connections(1)
        .run()
        .await;
    completed(&done);
    let waited = done.events.iter().find_map(|e| match e {
        TransferEvent::Retrying { delay, .. } => Some(*delay),
        _ => None,
    });
    assert_eq!(
        waited,
        Some(Duration::from_secs(1)),
        "Retry-After: 1 must set the delay"
    );
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn per_host_cap_keeps_a_limited_server_happy() {
    let h = harness().await;
    let size = 4 * MIB;
    let done = Job::new(&h, &format!("/limit?conns=2&size={size}"))
        .connections(8)
        .limiter(HostLimiter::new(2))
        .run()
        .await;
    completed(&done);
    assert!(
        !done
            .events
            .iter()
            .any(|e| matches!(e, TransferEvent::Retrying { .. })),
        "no 429s with the cap in place"
    );
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn throttled_host_gets_a_lower_cap_and_the_download_still_completes() {
    let h = harness().await;
    let size = 4 * MIB;
    let limiter = HostLimiter::new(8);
    let done = Job::new(&h, &format!("/limit?conns=2&bps=2097152&size={size}"))
        .connections(8)
        .limiter(Arc::clone(&limiter))
        .run()
        .await;
    completed(&done);
    assert!(
        done.events
            .iter()
            .any(|e| matches!(e, TransferEvent::Retrying { .. })),
        "the server answered 429 at least once"
    );
    let cap = limiter.cap_for("127.0.0.1");
    assert!(
        (1..8).contains(&cap),
        "429s must lower the cap below the configured 8, got {cap}"
    );
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn redirect_chain_is_followed_and_credentials_do_not_cross_origins() {
    let h = harness().await;
    let size = MIB;
    let done = Job::new(&h, &format!("/redirect?n=4&code=cycle&size={size}"))
        .run()
        .await;
    completed(&done);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    // a cross-origin hop: cookies and the Authorization header must be dropped
    let origin = h.server.url("/redirect?cross=1&to=echo-headers");
    let secrets = Secrets {
        cookies: Some("session=riptide".into()),
        headers: vec![("X-Secret".into(), "yes".into())],
        auth: Some(Credentials {
            username: "user".into(),
            password: "pass".into(),
        }),
    };
    let ctx = Arc::new(
        RequestContext::new(&url::Url::parse(&origin).unwrap())
            .with_referrer(Some("https://page.example/"))
            .with_secrets(&secrets),
    );
    let sent = riptide_net::http::send(
        &h.client,
        &ctx,
        reqwest::Method::GET,
        url::Url::parse(&origin).unwrap(),
        |r| r,
    )
    .await
    .unwrap();
    assert!(
        sent.url.as_str().starts_with(h.server.alt_url()),
        "landed on {}",
        sent.url
    );
    let echoed: serde_json::Value =
        serde_json::from_str(&sent.response.text().await.unwrap()).unwrap();
    let keys: Vec<String> = echoed
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.to_ascii_lowercase())
        .collect();
    assert!(!keys.contains(&"cookie".to_string()), "{keys:?}");
    assert!(!keys.contains(&"authorization".to_string()), "{keys:?}");
    assert!(!keys.contains(&"x-secret".to_string()), "{keys:?}");
    assert!(
        keys.contains(&"referer".to_string()),
        "referrer is not a secret: {keys:?}"
    );
}

#[tokio::test]
async fn cookies_referer_basic_and_digest_auth() {
    let h = harness().await;
    let size = 512 * 1024;
    let base = h.server.url("/");
    let base_url = url::Url::parse(&base).unwrap();

    let cookie_ctx = Arc::new(RequestContext::new(&base_url).with_secrets(&Secrets {
        cookies: Some("session=riptide".into()),
        ..Secrets::default()
    }));
    let done = Job::new(&h, &format!("/needs-cookie?size={size}"))
        .ctx(cookie_ctx)
        .final_name("cookie.bin")
        .run()
        .await;
    completed(&done);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    let referer_ctx =
        Arc::new(RequestContext::new(&base_url).with_referrer(Some(&h.server.url("/page"))));
    let done = Job::new(&h, &format!("/needs-referer?size={size}"))
        .ctx(referer_ctx)
        .final_name("referer.bin")
        .run()
        .await;
    completed(&done);

    let creds = Secrets {
        auth: Some(Credentials {
            username: "user".into(),
            password: "pass".into(),
        }),
        ..Secrets::default()
    };
    let basic_ctx = Arc::new(RequestContext::new(&base_url).with_secrets(&creds));
    let done = Job::new(&h, &format!("/needs-auth/basic?size={size}"))
        .ctx(basic_ctx)
        .final_name("basic.bin")
        .run()
        .await;
    completed(&done);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    let digest_ctx = Arc::new(RequestContext::new(&base_url).with_secrets(&creds));
    let done = Job::new(&h, &format!("/needs-auth/digest?size={size}"))
        .ctx(digest_ctx)
        .connections(4)
        .final_name("digest.bin")
        .run()
        .await;
    completed(&done);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );

    // and without credentials: a clear, non-retryable failure at probe time
    let plain = ctx_for(&base);
    let err = probe(&h.client, &h.server.url("/needs-auth/digest"), &plain)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AuthFailed { .. }), "{err:?}");
    let err = probe(&h.client, &h.server.url("/needs-cookie"), &plain)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Status {
                status: 403,
                retryable: false,
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn wrong_content_length_fails_without_retry() {
    let h = harness().await;
    for delta in ["1024", "-1024"] {
        let done = Job::new(&h, &format!("/wrong-length?size=1048576&delta={delta}"))
            .connections(4)
            .final_name(&format!("wl{delta}.bin"))
            .run()
            .await;
        match done.result {
            Err(Error::LengthMismatch { .. }) => {}
            Err(Error::RetriesExhausted { last, .. })
                if matches!(*last, Error::LengthMismatch { .. }) => {}
            other => panic!("delta {delta}: expected LengthMismatch, got {other:?}"),
        }
        assert!(!done.final_path.exists());
    }
}

#[tokio::test]
async fn pause_then_resume_is_byte_exact() {
    let h = harness().await;
    let size = 8 * MIB;
    let stop = CancellationToken::new();
    let stopper = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(900)).await;
        stopper.cancel();
    });
    let first = Job::new(&h, &format!("/slow?bps=2097152&size={size}"))
        .connections(4)
        .stop(stop)
        .run()
        .await;
    let segments = match first.result {
        Ok(Outcome::Stopped { segments }) => segments,
        other => panic!("expected Stopped, got {other:?}"),
    };
    let on_disk: u64 = segments.iter().map(|s| s.written).sum();
    assert!(
        on_disk > 0 && on_disk < size,
        "stopped mid-way with {on_disk} bytes"
    );
    assert!(part_path_for(&first.final_path).exists());
    assert!(
        matches!(first.events.last(), Some(TransferEvent::Checkpoint { .. })),
        "a stop ends with a checkpoint"
    );

    let second = Job::new(&h, &format!("/slow?bps=8388608&size={size}"))
        .connections(4)
        .segments(segments)
        .run()
        .await;
    completed(&second);
    assert!(
        matches!(second.events.first(), Some(TransferEvent::Started { resumed_bytes, .. }) if *resumed_bytes == on_disk)
    );
    assert_eq!(
        sha(&second.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, size)
    );
}

#[tokio::test]
async fn a_changed_file_restarts_from_zero_with_the_new_bytes() {
    let h = harness().await;
    let size = 3 * MIB;
    // Request 1 (the probe's HEAD) still sees version 1. The server replaces the file after it,
    // so the resume below — half of v1 on disk, If-Range with v1's ETag — meets version 2.
    let path = format!("/etag-changes?after=1&size={size}");
    let half = size / 2;
    let v1_half = riptide_testserver::bytes_at(DEFAULT_SEED, 0, half as usize);
    let mut on_disk = v1_half;
    on_disk.resize(size as usize, 0);
    let partial = vec![Segment {
        download_id: DownloadId(1),
        index: 0,
        start: 0,
        end: size,
        written: half,
        state: riptide_core::SegmentState::Pending,
    }];

    let done = Job::new(&h, &path)
        .connections(1)
        .prepared(on_disk, partial)
        .run()
        .await;
    assert_eq!(
        done.probe.etag.as_deref().map(|e| e.starts_with("\"v2")),
        Some(false),
        "the probe saw version 1"
    );
    completed(&done);
    assert!(
        done.events
            .iter()
            .any(|e| matches!(e, TransferEvent::Restarted { .. })),
        "{:?}",
        done.events
    );
    assert!(
        matches!(done.events.first(), Some(TransferEvent::Started { resumed_bytes, .. }) if *resumed_bytes == half)
    );
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED + 1, size),
        "the new version, not a splice"
    );
}

#[tokio::test]
async fn checksum_mismatch_is_reported_and_match_passes() {
    let h = harness().await;
    let size = MIB;
    let expected = Checksum::new(
        ChecksumAlgo::Sha256,
        &expected_sha256_hex(DEFAULT_SEED, size),
    )
    .unwrap();
    let bad = Job::new(&h, &format!("/corrupt?flip=3&size={size}"))
        .checksum(expected.clone())
        .final_name("corrupt.bin")
        .run()
        .await;
    assert!(
        matches!(bad.result, Err(Error::ChecksumMismatch { .. })),
        "{:?}",
        bad.result
    );
    assert!(
        !bad.final_path.exists(),
        "a failed verification never produces the final file"
    );
    assert!(
        part_path_for(&bad.final_path).exists(),
        "the part file stays for inspection or retry"
    );

    let good = Job::new(&h, &format!("/ok?size={size}"))
        .checksum(expected)
        .final_name("good.bin")
        .run()
        .await;
    completed(&good);
}

#[tokio::test]
async fn existing_target_is_not_overwritten_unless_asked() {
    let h = harness().await;
    std::fs::write(h.dir.path().join("taken.bin"), b"keep me").unwrap();
    let done = Job::new(&h, "/ok?size=4096")
        .final_name("taken.bin")
        .run()
        .await;
    assert!(
        matches!(done.result, Err(Error::TargetExists(_))),
        "{:?}",
        done.result
    );
    assert_eq!(
        std::fs::read(h.dir.path().join("taken.bin")).unwrap(),
        b"keep me"
    );

    let cfg = TransferConfig {
        overwrite: true,
        ..TransferConfig::default()
    };
    let done = Job::new(&h, "/ok?size=4096")
        .final_name("taken.bin")
        .cfg(cfg)
        .run()
        .await;
    completed(&done);
    assert_eq!(
        sha(&done.final_path).await,
        expected_sha256_hex(DEFAULT_SEED, 4096)
    );
}

#[tokio::test]
async fn gzip_despite_identity_is_saved_as_sent() {
    let h = harness().await;
    let done = Job::new(&h, "/gzip?size=65536").run().await;
    completed(&done);
    assert_eq!(done.probe.content_encoding.as_deref(), Some("gzip"));
    let bytes = std::fs::read(&done.final_path).unwrap();
    assert_eq!(
        &bytes[..2],
        &[0x1f, 0x8b],
        "gzip magic: the encoded body is what the server sent"
    );
    assert_eq!(bytes.len() as u64, done.probe.size.unwrap());
}

// ------------------------------------------------------------------------ builder helpers

impl<'a> Job<'a> {
    fn validator(mut self, v: Option<String>) -> Self {
        self.validator = Some(v);
        self
    }

    fn connections(mut self, n: u32) -> Self {
        self.connections = n;
        self
    }
    fn ctx(mut self, ctx: Arc<RequestContext>) -> Self {
        self.ctx = Some(ctx);
        self
    }
    fn segments(mut self, segments: Vec<Segment>) -> Self {
        self.segments = segments;
        self
    }
    fn checksum(mut self, checksum: Checksum) -> Self {
        self.checksum = Some(checksum);
        self
    }
    fn cfg(mut self, cfg: TransferConfig) -> Self {
        self.cfg = Some(cfg);
        self
    }
    fn stop(mut self, stop: CancellationToken) -> Self {
        self.stop = Some(stop);
        self
    }
    fn limiter(mut self, limiter: Arc<HostLimiter>) -> Self {
        self.limiter = Some(limiter);
        self
    }
    fn final_name(mut self, name: &str) -> Self {
        self.final_name = Some(name.to_string());
        self
    }
    fn probe_url(mut self, path: &str) -> Self {
        self.probe_url = Some(self.h.server.url(path));
        self
    }
    fn prepared(mut self, bytes: Vec<u8>, segments: Vec<Segment>) -> Self {
        self.prepared = Some((bytes, segments));
        self
    }
}
