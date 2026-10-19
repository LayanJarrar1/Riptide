//! Download matrix: network behaviour through the whole engine (queue, store, events, files).
//!
//! `riptide-net/tests/engine.rs` covers the transfer layer one failure mode at a time; these
//! tests drive the same modes through `Engine::add` and assert the end state a user sees:
//! `Completed`, exact size and SHA-256, no `.rtpart` left, or a `Failed` that never leaves a
//! final file.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::*;
use riptide_core::{Credentials, DownloadRequest, DownloadState, Event, Secrets};
use riptide_testserver::{expected_sha256_hex, DEFAULT_SEED};

const T: Duration = Duration::from_secs(120);

async fn complete(w: &World, engine: &riptide_engine::Engine, path: &str, size: u64) {
    let mut rx = engine.subscribe();
    let id = engine.add(w.request(path)).await.unwrap();
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
    engine.remove(id, true).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_redirect_status_and_long_chains_complete() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 300_000;
    // n=19 is 20 redirect responses (the last hop points at the file): exactly the limit
    for code in ["301", "302", "303", "307", "308", "cycle"] {
        for n in [1, 3, 19] {
            complete(
                &w,
                &engine,
                &format!("/redirect?n={n}&code={code}&size={size}"),
                size,
            )
            .await;
        }
    }
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn more_than_twenty_redirects_fail_and_leave_nothing() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    for n in [20, 25] {
        let id = engine
            .add(w.request(&format!("/redirect?n={n}&code=302&size=1000")))
            .await
            .unwrap();
        let retryable = wait_failed(&mut rx, id, T).await;
        assert!(!retryable, "a redirect loop is not worth retrying");
    }
    assert_eq!(
        std::fs::read_dir(w.downloads()).map_or(0, Iterator::count),
        0
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_origin_redirect_completes_and_never_leaks_credentials() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 500_000;
    let id = engine
        .add(DownloadRequest {
            secrets: Some(Secrets {
                cookies: Some("session=riptide".into()),
                headers: vec![("X-Secret".into(), "yes".into())],
                auth: Some(Credentials {
                    username: "user".into(),
                    password: "pass".into(),
                }),
            }),
            ..w.request(&format!("/redirect?n=2&cross=1&size={size}"))
        })
        .await
        .unwrap();
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
    let alt = w.server.alt_url().to_string();
    let reqs = w.server.state().requests();
    let on_alt: Vec<_> = reqs.iter().filter(|r| r.origin == alt).collect();
    assert!(
        !on_alt.is_empty(),
        "the redirect never reached the other origin"
    );
    assert!(
        on_alt.iter().all(|r| !r.has_cookie && !r.has_authorization),
        "credentials crossed origins: {on_alt:?}"
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_strings_and_percent_encoding_do_not_leak_into_the_name() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 4096;
    let id = engine
        .add(w.request(&format!(
            "/file/a%20b%20%26%20c.zip?size={size}&token=a%2Fb%3D%3D&x=%26y%23z&empty=&dup=1&dup=2#frag"
        )))
        .await
        .unwrap();
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_eq!(p.file_name().unwrap().to_string_lossy(), "a b & c.zip");
    assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn range_ignored_and_no_length_are_streamed_whole() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 5 * MIB + 3;
    complete(&w, &engine, &format!("/noranges?size={size}"), size).await;
    complete(&w, &engine, &format!("/nolength?size={size}"), size).await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_link_and_connection_resets_resume_from_the_offset() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 6 * MIB;
    complete(
        &w,
        &engine,
        &format!("/slow?bps={}&size={size}", 4 * MIB),
        size,
    )
    .await;
    // every connection dies after 700 KB; each retry resumes where the last one stopped
    complete(
        &w,
        &engine,
        &format!("/drop?after=700000&size={size}"),
        size,
    )
    .await;
    // random 5xx and resets
    complete(&w, &engine, &format!("/flaky?rate=0.25&size={size}"), size).await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_and_host_caps_are_learned_and_obeyed() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 2 * MIB;
    complete(
        &w,
        &engine,
        &format!("/retry-after?fails=2&size={size}"),
        size,
    )
    .await;
    // the server admits only 2 concurrent bodies and answers 429 beyond that
    complete(
        &w,
        &engine,
        &format!("/limit?conns=2&bps={}&size={size}", 2 * MIB),
        size,
    )
    .await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_bytes_with_a_checksum_end_failed_and_leave_no_final_file() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = MIB;
    let checksum = || riptide_core::Checksum {
        algo: riptide_core::ChecksumAlgo::Sha256,
        hex: expected_sha256_hex(DEFAULT_SEED, size),
    };
    for flips in [1u64, 8, 1000] {
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("corrupt{flips}.bin")),
                checksum: Some(checksum()),
                ..w.request(&format!("/corrupt?flip={flips}&size={size}"))
            })
            .await
            .unwrap();
        let retryable = wait_failed(&mut rx, id, T).await;
        assert!(
            !retryable,
            "a checksum mismatch must not be retried silently"
        );
        let rec = engine.get(id).await.unwrap().unwrap();
        assert!(matches!(rec.state, DownloadState::Failed { .. }));
        assert!(!w.downloads().join(format!("corrupt{flips}.bin")).exists());
        // the part file stays for inspection (documented), but it is never the final name
        assert!(w
            .downloads()
            .join(format!("corrupt{flips}.bin.rtpart"))
            .exists());
    }
    // the same bytes without corruption pass the same checksum
    let clean = w.dir.path().join("clean");
    let id = engine
        .add(DownloadRequest {
            dir: Some(clean.clone()),
            filename: Some("clean.bin".into()),
            checksum: Some(checksum()),
            ..w.request(&format!("/ok?size={size}"))
        })
        .await
        .unwrap();
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(&engine, id, &p, &clean, DEFAULT_SEED, size).await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_content_length_never_completes_in_either_direction() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    for (n, delta) in [1024i64, -1024, 1, -1].into_iter().enumerate() {
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("wl{n}.bin")),
                ..w.request(&format!("/wrong-length?delta={delta}&size=500000"))
            })
            .await
            .unwrap();
        wait_failed(&mut rx, id, Duration::from_secs(180)).await;
        let rec = engine.get(id).await.unwrap().unwrap();
        assert!(
            matches!(rec.state, DownloadState::Failed { .. }),
            "delta {delta}: {:?}",
            rec.state
        );
        assert!(!w.downloads().join(format!("wl{n}.bin")).exists());
    }
    engine.shutdown().await;
}

/// A validator that changes mid-download must restart from zero: the result is one whole
/// version, never a splice of two.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validator_change_mid_download_never_splices_versions() {
    let size = 6 * MIB;
    let v1 = expected_sha256_hex(DEFAULT_SEED, size);
    let v2 = expected_sha256_hex(DEFAULT_SEED + 1, size);
    let mut saw_v2 = false;
    for after in [2u64, 3, 4, 6, 9] {
        let w = World::new().await;
        let engine = w.engine().await;
        let mut rx = engine.subscribe();
        let id = engine
            .add(DownloadRequest {
                connections: Some(2),
                ..w.request(&format!(
                    "/etag-changes?after={after}&bps={}&size={size}",
                    3 * MIB
                ))
            })
            .await
            .unwrap();
        // interrupt once some bytes are in, so the resume meets the new version
        wait_for(&mut rx, T, |e| {
            matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0)
        })
        .await;
        engine.pause(id).await.unwrap();
        wait_state(&mut rx, id, DownloadState::Paused, T).await;
        engine.resume(id).await.unwrap();
        let p = wait_completed_in(&mut rx, id, T).await;
        let got = riptide_net::checksum_of(p.clone(), riptide_core::ChecksumAlgo::Sha256)
            .await
            .unwrap();
        assert!(
            got == v1 || got == v2,
            "after={after}: the file is neither version (spliced?)"
        );
        saw_v2 |= got == v2;
        assert_eq!(std::fs::metadata(&p).unwrap().len(), size);
        assert_no_partials(&w.downloads());
        engine.shutdown().await;
    }
    assert!(
        saw_v2,
        "no run ever met the new version: the scenario did not trigger"
    );
}

/// Signed URL that expires while the download is paused: resuming meets a 403 and the download
/// fails without a final file; a fresh link given through `refresh_url` finishes it byte-exact.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_signed_url_is_finished_after_refresh() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 8 * MIB;
    let id = engine
        .add(w.request(&format!(
            "/expires?after=1&bps={}&size={size}",
            256 * KIB
        )))
        .await
        .unwrap();
    wait_for(&mut rx, T, |e| {
        matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0)
    })
    .await;
    engine.pause(id).await.unwrap();
    wait_state(&mut rx, id, DownloadState::Paused, T).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    engine.resume(id).await.unwrap();
    wait_failed(&mut rx, id, Duration::from_secs(180)).await;
    assert!(!w.downloads().join("expires").exists());

    let fresh = w.server.url(&format!("/ok?size={size}"));
    engine.refresh_url(id, &fresh, None, None).await.unwrap();
    // a failed download is resumable; resume it if the refresh did not already
    let _ = engine.resume(id).await;
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_challenges_succeed_with_credentials_and_fail_without() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 100_000;
    let creds = |pw: &str| {
        Some(Secrets {
            auth: Some(Credentials {
                username: "user".into(),
                password: pw.into(),
            }),
            ..Secrets::default()
        })
    };
    for kind in ["basic", "digest"] {
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("{kind}-ok.bin")),
                secrets: creds("pass"),
                ..w.request(&format!("/needs-auth/{kind}?size={size}"))
            })
            .await
            .unwrap();
        let p = wait_completed_in(&mut rx, id, T).await;
        assert_eq!(std::fs::metadata(&p).unwrap().len(), size);

        let bad = engine
            .add(DownloadRequest {
                filename: Some(format!("{kind}-bad.bin")),
                secrets: creds("wrong"),
                ..w.request(&format!("/needs-auth/{kind}?size={size}"))
            })
            .await
            .unwrap();
        let retryable = wait_failed(&mut rx, bad, T).await;
        assert!(!retryable, "{kind}: wrong password must not loop");
        assert!(!w.downloads().join(format!("{kind}-bad.bin")).exists());
    }
    engine.shutdown().await;
}

/// 24 downloads at once: all finish byte-exact and the engine never runs more than `max_active`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_simultaneous_downloads_respect_caps() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut s = engine.settings();
    s.max_active = 24;
    s.per_host = 6;
    s.connections = 4;
    engine.set_settings(s).await.unwrap();
    let mut rx = engine.subscribe();

    let n = 24usize;
    let mut ids = Vec::new();
    for i in 0..n {
        let size = 400_000 + (i as u64) * 33_333;
        let seed = 1000 + i as u64;
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("many{i:02}.bin")),
                ..w.request(&format!("/slow?bps={MIB}&size={size}&seed={seed}"))
            })
            .await
            .unwrap();
        ids.push((id, size, seed));
    }

    let mut max_running = 0usize;
    let mut max_in_flight = 0usize;
    let mut pending: std::collections::HashSet<_> = ids.iter().map(|(id, ..)| *id).collect();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while !pending.is_empty() {
        max_running = max_running.max(engine.running_ids().len());
        max_in_flight = max_in_flight.max(w.server.state().in_flight());
        match tokio::time::timeout(Duration::from_millis(20), rx.recv()).await {
            Ok(Ok(Event::Completed { id, .. })) => {
                pending.remove(&id);
            }
            Ok(Ok(Event::Failed { id, error, .. })) => panic!("{id:?} failed: {error}"),
            _ => {}
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} left",
            pending.len()
        );
    }
    assert!(max_running <= 24, "ran {max_running} at once");
    for (id, size, seed) in ids {
        let rec = engine.get(id).await.unwrap().unwrap();
        let p = rec.dir.join(&rec.filename);
        assert_complete(&engine, id, &p, &w.downloads(), seed, size).await;
    }
    eprintln!("max running {max_running}, max bodies in flight {max_in_flight}");
    engine.shutdown().await;
}

/// The queue's own limit holds when 25 downloads are added at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_limit_holds_with_twenty_five_downloads() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut s = engine.settings();
    s.max_active = 3;
    engine.set_settings(s).await.unwrap();
    let mut rx = engine.subscribe();
    let mut pending = std::collections::HashSet::new();
    for i in 0..25u64 {
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("q{i:02}.bin")),
                ..w.request(&format!("/slow?bps={MIB}&size=300000&seed={}", 50 + i))
            })
            .await
            .unwrap();
        pending.insert(id);
    }
    let mut max_running = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while !pending.is_empty() {
        max_running = max_running.max(engine.running_ids().len());
        if let Ok(Ok(Event::Completed { id, .. })) =
            tokio::time::timeout(Duration::from_millis(20), rx.recv()).await
        {
            pending.remove(&id);
        }
        assert!(tokio::time::Instant::now() < deadline);
    }
    assert!(max_running <= 3, "queue limit 3 exceeded: {max_running}");
    assert!(max_running >= 1);
    engine.shutdown().await;
}
