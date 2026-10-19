//! Download matrix: sizes from 0 B to beyond 4 GiB, through every server mode.
//!
//! The 0 B .. 100 MiB cases always run. The 1 GiB and 4 GiB + 1 B cases are opt-in
//! (`RIPTIDE_BIG_TESTS=1`): bodies are generated and hashed by streaming (never held in memory),
//! the partial file is sparse-preallocated, and the whole folder is deleted when the test ends.
//! They prove that u64 offsets and segment boundaries survive crossing 2^32.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::*;
use riptide_core::DownloadState;
use riptide_testserver::DEFAULT_SEED;

fn big_enabled() -> bool {
    std::env::var("RIPTIDE_BIG_TESTS").is_ok_and(|v| v == "1")
}

async fn run_one(w: &World, engine: &riptide_engine::Engine, route: &str, size: u64) {
    let mut rx = engine.subscribe();
    let id = engine
        .add(w.request(&format!("{route}?size={size}")))
        .await
        .unwrap();
    let path = wait_completed_in(&mut rx, id, Duration::from_secs(600)).await;
    assert_complete(engine, id, &path, &w.downloads(), DEFAULT_SEED, size).await;
    let record = engine.get(id).await.unwrap().unwrap();
    assert_eq!(record.size, Some(size).filter(|_| route == "/ok" || route == "/noranges"));
    // free the name for the next case
    engine.remove(id, true).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tiny_sizes_through_every_server_mode() {
    let w = World::new().await;
    let engine = w.engine().await;
    for route in ["/ok", "/noranges", "/nolength"] {
        for size in [0u64, 1, 2, 7, 8, 9, 1023, 1024, KIB + 1, 64 * KIB, MIB - 1, MIB, MIB + 1] {
            run_one(&w, &engine, route, size).await;
        }
    }
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_and_hundred_mebibytes_are_byte_exact() {
    let w = World::new().await;
    let engine = w.engine().await;
    run_one(&w, &engine, "/ok", 10 * MIB).await;
    run_one(&w, &engine, "/noranges", 10 * MIB).await;
    run_one(&w, &engine, "/ok", 100 * MIB).await;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_gibibyte_is_byte_exact() {
    if !big_enabled() {
        eprintln!("skipped: set RIPTIDE_BIG_TESTS=1 to run");
        return;
    }
    let w = World::new().await;
    let engine = w.engine().await;
    run_one(&w, &engine, "/ok", GIB).await;
    engine.shutdown().await;
}

/// 4 GiB + 1 B: the last segment starts and ends beyond `u32::MAX`; every offset, `Range`
/// header, segment boundary and the rename must use u64.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn more_than_four_gibibytes_proves_u64_offsets() {
    if !big_enabled() {
        eprintln!("skipped: set RIPTIDE_BIG_TESTS=1 to run");
        return;
    }
    let size = 4 * GIB + 1;
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let id = engine
        .add(w.request(&format!("/ok?size={size}")))
        .await
        .unwrap();
    let path = wait_completed_in(&mut rx, id, Duration::from_secs(1800)).await;
    assert_complete(&engine, id, &path, &w.downloads(), DEFAULT_SEED, size).await;
    let segs = engine.segments(id).await.unwrap();
    assert!(
        segs.iter().any(|s| s.end > u64::from(u32::MAX)),
        "no segment reaches beyond 4 GiB: {segs:?}"
    );
    assert_eq!(segs.iter().map(|s| s.end).max(), Some(size));
    assert_eq!(
        engine.get(id).await.unwrap().unwrap().state,
        DownloadState::Completed
    );
    engine.remove(id, true).await.unwrap();
    engine.shutdown().await;
}
