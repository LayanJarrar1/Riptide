//! Regression test for the per-download thread and handle leak: after hundreds of small
//! downloads the process must not hold more threads or handles than after a handful.
//!
//! Windows only (the counters are Win32); elsewhere the file compiles to nothing.
#![cfg(windows)]
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::World;

#[repr(C)]
struct ThreadEntry32 {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_process_id: u32,
    base_pri: i32,
    delta_pri: i32,
    flags: u32,
}

extern "system" {
    fn GetCurrentProcess() -> isize;
    fn GetCurrentProcessId() -> u32;
    fn GetProcessHandleCount(process: isize, count: *mut u32) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> isize;
    fn Thread32First(snapshot: isize, entry: *mut ThreadEntry32) -> i32;
    fn Thread32Next(snapshot: isize, entry: *mut ThreadEntry32) -> i32;
    fn CloseHandle(handle: isize) -> i32;
}

const TH32CS_SNAPTHREAD: u32 = 0x4;

fn handle_count() -> u32 {
    let mut n = 0u32;
    // SAFETY: plain Win32 query on the current process with a valid out pointer.
    unsafe {
        GetProcessHandleCount(GetCurrentProcess(), &mut n);
    }
    n
}

fn thread_count() -> u32 {
    // SAFETY: Toolhelp snapshot of the threads, walked with a correctly sized entry, then closed.
    unsafe {
        let pid = GetCurrentProcessId();
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        let mut entry = ThreadEntry32 {
            size: std::mem::size_of::<ThreadEntry32>() as u32,
            usage: 0,
            thread_id: 0,
            owner_process_id: 0,
            base_pri: 0,
            delta_pri: 0,
            flags: 0,
        };
        let mut n = 0;
        let mut ok = Thread32First(snap, &mut entry);
        while ok != 0 {
            if entry.owner_process_id == pid {
                n += 1;
            }
            ok = Thread32Next(snap, &mut entry);
        }
        CloseHandle(snap);
        n
    }
}

async fn run_downloads(world: &World, engine: &riptide_engine::Engine, from: usize, count: usize) {
    let size: u64 = std::env::var("LEAK_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    // added all at once, like a batch: the scheduler runs a few at a time and the rest wait queued
    let mut rx = engine.subscribe();
    let mut pending = std::collections::HashSet::new();
    for i in from..from + count {
        let id = engine
            .add(world.request(&format!("/ok?size={size}&seed={i}")))
            .await
            .unwrap();
        pending.insert(id);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    while !pending.is_empty() {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Ok(riptide_core::Event::Completed { id, .. })) => {
                pending.remove(&id);
            }
            Ok(Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            other => panic!("{} downloads unfinished: {other:?}", pending.len()),
        }
    }
}

/// 200 small downloads must not grow the process's threads or handles by more than a small,
/// constant amount (the first runs warm up pools, so the baseline is taken after 20).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hundreds_of_small_downloads_do_not_leak_threads_or_handles() {
    let world = World::new().await;
    let engine = world.engine().await;

    run_downloads(&world, &engine, 0, 20).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (threads0, handles0) = (thread_count(), handle_count());

    run_downloads(&world, &engine, 20, 200).await;
    // the blocking pool keeps idle workers for a while and closes them; give it that time
    tokio::time::sleep(Duration::from_secs(12)).await;
    let (threads1, handles1) = (thread_count(), handle_count());

    eprintln!("threads {threads0} -> {threads1}, handles {handles0} -> {handles1}");
    assert!(
        threads1 <= threads0 + 20,
        "threads grew from {threads0} to {threads1} over 200 downloads"
    );
    assert!(
        handles1 <= handles0 + 150,
        "handles grew from {handles0} to {handles1} over 200 downloads"
    );
}
