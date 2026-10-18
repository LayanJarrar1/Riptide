//! Queues through the engine API against `riptide-testserver`: per-queue and global limits,
//! start/stop, order, move, delete, the schedule timer on a fake clock, `QueueFinished`, and the
//! per-host cap learned from `429`. Every finished file is compared by SHA-256.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use riptide_core::{
    ChecksumAlgo, DownloadId, DownloadRequest, DownloadState, Event, LocalDateTime, ManualClock,
    PostAction, QueueId, QueueSpec, Schedule, SECS_PER_DAY,
};
use riptide_engine::{Engine, EngineConfig, EngineSettings};
use riptide_net::checksum_of;
use riptide_testserver::{expected_sha256_hex, TestServer, DEFAULT_SEED};
use tokio::sync::broadcast;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;
const T: Duration = Duration::from_secs(30);

struct World {
    server: TestServer,
    dir: tempfile::TempDir,
}

impl World {
    async fn new() -> Self {
        World {
            server: TestServer::spawn().await,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn config(&self) -> EngineConfig {
        EngineConfig {
            db_path: self.dir.path().join("riptide.sqlite"),
            default_dir: self.dir.path().join("downloads"),
        }
    }

    async fn engine(&self) -> Engine {
        Engine::open(self.config()).await.unwrap()
    }

    /// A one-connection download that takes about `size / bps` seconds.
    fn slow(&self, queue: QueueId, size: u64, bps: u64, tag: &str) -> DownloadRequest {
        let mut r = DownloadRequest::new(
            self.server
                .url(&format!("/slow?bps={bps}&size={size}&tag={tag}")),
        );
        r.queue_id = Some(queue);
        r.connections = Some(1);
        r.filename = Some(format!("{tag}.bin"));
        r
    }

    fn quick(&self, queue: QueueId, size: u64, tag: &str) -> DownloadRequest {
        let mut r = DownloadRequest::new(self.server.url(&format!("/ok?size={size}&tag={tag}")));
        r.queue_id = Some(queue);
        r.connections = Some(1);
        r.filename = Some(format!("{tag}.bin"));
        r
    }
}

async fn wait_for(
    rx: &mut broadcast::Receiver<Event>,
    timeout: Duration,
    mut pred: impl FnMut(&Event) -> bool,
) -> Event {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut seen: Vec<String> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                if pred(&ev) {
                    return ev;
                }
                if !ev.is_progress() {
                    seen.push(format!("{ev:?}"));
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("event channel closed: {e}"),
            Err(_) => panic!("timed out waiting for an event; saw: {seen:#?}"),
        }
    }
}

async fn wait_completed(rx: &mut broadcast::Receiver<Event>, id: DownloadId) {
    wait_for(
        rx,
        T,
        |e| matches!(e, Event::Completed { id: i, .. } if *i == id),
    )
    .await;
}

/// Waits until every id in `ids` has completed (in any order).
async fn wait_all_completed(rx: &mut broadcast::Receiver<Event>, ids: &[DownloadId]) {
    let mut left: Vec<DownloadId> = ids.to_vec();
    while !left.is_empty() {
        if let Event::Completed { id, .. } =
            wait_for(rx, T, |e| matches!(e, Event::Completed { .. })).await
        {
            left.retain(|l| *l != id);
        }
    }
}

async fn assert_sha(engine: &Engine, id: DownloadId, size: u64) {
    let d = engine.get(id).await.unwrap().unwrap();
    assert_eq!(d.state, DownloadState::Completed, "{}", d.filename);
    assert_eq!(
        checksum_of(d.final_path(), ChecksumAlgo::Sha256)
            .await
            .unwrap(),
        expected_sha256_hex(DEFAULT_SEED, size),
        "{}",
        d.filename
    );
}

async fn state_of(engine: &Engine, id: DownloadId) -> DownloadState {
    engine.get(id).await.unwrap().unwrap().state
}

async fn eventually<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out: {what}");
}

/// 2026-10-19 is a Monday.
fn mon(h: u32, m: u32) -> LocalDateTime {
    LocalDateTime::from_ymd_hms(2026, 10, 19, h, m, 0).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_queue_and_global_limits_hold_with_real_downloads() {
    let w = World::new().await;
    let engine = w.engine().await;
    // global 3; queue A runs 2, queue B runs 1
    engine
        .set_settings(EngineSettings {
            max_active: 3,
            ..engine.settings()
        })
        .await
        .unwrap();
    let a = engine.create_queue(QueueSpec::new("A", 2)).await.unwrap();
    let b = engine.create_queue(QueueSpec::new("B", 1)).await.unwrap();
    let mut rx = engine.subscribe();

    let size = 512 * KIB;
    let mut ids = Vec::new();
    for i in 0..6 {
        ids.push((
            a,
            engine
                .add(w.slow(a, size, MIB, &format!("a{i}")))
                .await
                .unwrap(),
        ));
    }
    for i in 0..3 {
        ids.push((
            b,
            engine
                .add(w.slow(b, size, MIB, &format!("b{i}")))
                .await
                .unwrap(),
        ));
    }
    let queue_of: HashMap<DownloadId, QueueId> = ids.iter().map(|(q, id)| (*id, *q)).collect();

    // sample the real running set while everything drains
    let stop = Arc::new(AtomicBool::new(false));
    let max_a = Arc::new(AtomicUsize::new(0));
    let max_b = Arc::new(AtomicUsize::new(0));
    let max_total = Arc::new(AtomicUsize::new(0));
    let sampler = {
        let (engine, stop) = (engine.clone(), Arc::clone(&stop));
        let (max_a, max_b, max_total) = (
            Arc::clone(&max_a),
            Arc::clone(&max_b),
            Arc::clone(&max_total),
        );
        let queue_of = queue_of.clone();
        tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                let running = engine.running_ids();
                let in_a = running.iter().filter(|id| queue_of[id] == a).count();
                let in_b = running.iter().filter(|id| queue_of[id] == b).count();
                max_a.fetch_max(in_a, Ordering::SeqCst);
                max_b.fetch_max(in_b, Ordering::SeqCst);
                max_total.fetch_max(running.len(), Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };

    let all: Vec<DownloadId> = ids.iter().map(|(_, id)| *id).collect();
    wait_all_completed(&mut rx, &all).await;
    stop.store(true, Ordering::SeqCst);
    sampler.await.unwrap();

    assert!(max_a.load(Ordering::SeqCst) <= 2, "queue A limit");
    assert!(max_b.load(Ordering::SeqCst) <= 1, "queue B limit");
    assert!(max_total.load(Ordering::SeqCst) <= 3, "global max_active");
    assert_eq!(max_a.load(Ordering::SeqCst), 2, "queue A used its slots");
    assert_eq!(max_total.load(Ordering::SeqCst), 3, "global slots used");
    for (_, id) in ids {
        assert_sha(&engine, id, size).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_a_queue_requeues_running_items_and_starting_resumes_them() {
    let w = World::new().await;
    let engine = w.engine().await;
    let q = engine.create_queue(QueueSpec::new("Q", 2)).await.unwrap();
    let mut rx = engine.subscribe();
    let size = MIB;
    let a = engine.add(w.slow(q, size, 256 * KIB, "a")).await.unwrap();
    let b = engine.add(w.slow(q, size, 256 * KIB, "b")).await.unwrap();
    eventually("both downloading", || async {
        state_of(&engine, a).await == DownloadState::Downloading
            && state_of(&engine, b).await == DownloadState::Downloading
    })
    .await;

    engine.stop_queue(q).await.unwrap();
    eventually("both back in the queue", || async {
        state_of(&engine, a).await == DownloadState::Queued
            && state_of(&engine, b).await == DownloadState::Queued
            && engine.running_ids().is_empty()
    })
    .await;
    assert!(!engine.queue(q).await.unwrap().unwrap().enabled);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(state_of(&engine, a).await, DownloadState::Queued);
    assert_eq!(state_of(&engine, b).await, DownloadState::Queued);
    assert!(
        engine.running_ids().is_empty(),
        "nothing runs in a stopped queue"
    );

    engine.start_queue(q).await.unwrap();
    wait_all_completed(&mut rx, &[a, b]).await;
    assert_sha(&engine, a, size).await;
    assert_sha(&engine, b, size).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn items_added_to_a_stopped_queue_wait_until_it_is_started() {
    let w = World::new().await;
    let engine = w.engine().await;
    let q = engine
        .create_queue(QueueSpec::new("Later", 1))
        .await
        .unwrap();
    engine.stop_queue(q).await.unwrap();
    let mut rx = engine.subscribe();
    let id = engine.add(w.quick(q, 256 * KIB, "x")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(state_of(&engine, id).await, DownloadState::Queued);
    // Main is unaffected by the stopped queue
    let main = engine
        .add(w.quick(QueueId::MAIN, 256 * KIB, "m"))
        .await
        .unwrap();
    wait_completed(&mut rx, main).await;

    engine.start_queue(q).await.unwrap();
    wait_completed(&mut rx, id).await;
    assert_sha(&engine, id, 256 * KIB).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reorder_and_move_decide_the_start_order() {
    let w = World::new().await;
    let engine = w.engine().await;
    let q = engine.create_queue(QueueSpec::new("Seq", 1)).await.unwrap();
    engine.stop_queue(q).await.unwrap();
    let mut rx = engine.subscribe();
    let size = 128 * KIB;
    let a = engine.add(w.quick(q, size, "a")).await.unwrap();
    let b = engine.add(w.quick(q, size, "b")).await.unwrap();
    let c = engine.add(w.quick(q, size, "c")).await.unwrap();
    // d starts in Main (stopped there by pausing first so it does not run before the move)
    let mut dreq = w.quick(QueueId::MAIN, size, "d");
    dreq.start_paused = true;
    let d = engine.add(dreq).await.unwrap();

    engine.reorder(q, &[c, a]).await.unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::QueueOrderChanged { queue_id } if *queue_id == q),
    )
    .await;
    engine.move_to_queue(d, q).await.unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::DownloadMoved { id, from, to } if *id == d && *from == QueueId::MAIN && *to == q),
    )
    .await;
    engine.resume(d).await.unwrap();
    let items = engine.queue_items(q).await.unwrap();
    assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [c, a, b, d]);

    engine.start_queue(q).await.unwrap();
    let mut order = Vec::new();
    while order.len() < 4 {
        if let Event::Completed { id, .. } =
            wait_for(&mut rx, T, |e| matches!(e, Event::Completed { .. })).await
        {
            order.push(id);
        }
    }
    assert_eq!(
        order,
        [c, a, b, d],
        "max_concurrent 1 follows the manual order"
    );
    for id in order {
        assert_sha(&engine, id, size).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deleting_a_queue_moves_its_items_to_main() {
    let w = World::new().await;
    let engine = w.engine().await;
    let q = engine
        .create_queue(QueueSpec::new("Temp", 1))
        .await
        .unwrap();
    engine.stop_queue(q).await.unwrap();
    let a = engine.add(w.quick(q, 64 * KIB, "a")).await.unwrap();
    let b = engine.add(w.quick(q, 64 * KIB, "b")).await.unwrap();
    let mut rx = engine.subscribe();

    engine.delete_queue(q).await.unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::QueueDeleted { queue_id } if *queue_id == q),
    )
    .await;
    assert!(engine.queue(q).await.unwrap().is_none());
    assert!(engine.delete_queue(QueueId::MAIN).await.is_err());
    for id in [a, b] {
        assert_eq!(
            engine.get(id).await.unwrap().unwrap().queue_id,
            QueueId::MAIN
        );
    }
    // Main is started, so they now run
    wait_all_completed(&mut rx, &[a, b]).await;
    assert_sha(&engine, a, 64 * KIB).await;
    assert_sha(&engine, b, 64 * KIB).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_finished_is_announced_once_with_the_post_action() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut spec = QueueSpec::new("Night", 2);
    spec.on_done = Some(PostAction::Sleep);
    let q = engine.create_queue(spec).await.unwrap();
    let mut rx = engine.subscribe();
    let size = 256 * KIB;
    let a = engine.add(w.quick(q, size, "a")).await.unwrap();
    let b = engine.add(w.quick(q, size, "b")).await.unwrap();

    let mut completed = Vec::new();
    let finished = wait_for(&mut rx, T, |e| {
        if let Event::Completed { id, .. } = e {
            completed.push(*id);
        }
        matches!(e, Event::QueueFinished { queue_id, .. } if *queue_id == q)
    })
    .await;
    assert_eq!(
        finished,
        Event::QueueFinished {
            queue_id: q,
            action: Some(PostAction::Sleep)
        }
    );
    completed.sort();
    assert_eq!(completed, [a, b], "announced only after the last item");

    // and not a second time
    let again = tokio::time::timeout(
        Duration::from_millis(500),
        wait_for(&mut rx, T, |e| matches!(e, Event::QueueFinished { .. })),
    )
    .await;
    assert!(again.is_err(), "QueueFinished must be emitted once");

    // adding more work re-arms the announcement
    let c = engine.add(w.quick(q, size, "c")).await.unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::QueueFinished { queue_id, .. } if *queue_id == q),
    )
    .await;
    assert_sha(&engine, c, size).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schedule_starts_and_stops_a_queue_on_a_fake_clock() {
    let w = World::new().await;
    let clock = Arc::new(ManualClock::new(mon(12, 0)));
    let engine = Engine::open_with_clock(w.config(), clock.clone())
        .await
        .unwrap();
    let mut spec = QueueSpec::new("Night", 1);
    spec.schedule = Schedule::only_at_night();
    let q = engine.create_queue(spec).await.unwrap();

    // noon: outside the window, the timer stops the new queue without any manual call
    eventually("night queue stopped at noon", || async {
        !engine.queue(q).await.unwrap().unwrap().enabled
    })
    .await;
    let mut rx = engine.subscribe();
    let size = 256 * KIB;
    let id = engine.add(w.quick(q, size, "n")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(state_of(&engine, id).await, DownloadState::Queued);

    // a manual start outside the window sticks until the next edge
    engine.start_queue(q).await.unwrap();
    wait_completed(&mut rx, id).await;
    engine.stop_queue(q).await.unwrap();
    let id2 = engine.add(w.quick(q, size, "n2")).await.unwrap();

    // 23:00: the window opens
    clock.set(mon(23, 0));
    engine.check_schedules().await.unwrap();
    assert!(engine.queue(q).await.unwrap().unwrap().enabled);
    wait_completed(&mut rx, id2).await;
    assert_sha(&engine, id2, size).await;

    // 07:00 next morning: the window closes and the queue stops
    clock.set(mon(7, 0).plus_secs(SECS_PER_DAY));
    engine.check_schedules().await.unwrap();
    assert!(!engine.queue(q).await.unwrap().unwrap().enabled);
    let id3 = engine.add(w.quick(q, size, "n3")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(state_of(&engine, id3).await, DownloadState::Queued);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_shot_start_at_fires_once_and_is_cleared() {
    let w = World::new().await;
    let clock = Arc::new(ManualClock::new(mon(9, 0)));
    let engine = Engine::open_with_clock(w.config(), clock.clone())
        .await
        .unwrap();
    let mut spec = QueueSpec::new("Later", 1);
    spec.schedule = Schedule::start_once_at(mon(14, 30));
    let q = engine.create_queue(spec).await.unwrap();
    engine.check_schedules().await.unwrap();
    assert!(!engine.queue(q).await.unwrap().unwrap().enabled);
    let mut rx = engine.subscribe();
    let size = 128 * KIB;
    let id = engine.add(w.quick(q, size, "o")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(state_of(&engine, id).await, DownloadState::Queued);

    clock.set(mon(14, 30));
    engine.check_schedules().await.unwrap();
    wait_completed(&mut rx, id).await;
    let queue = engine.queue(q).await.unwrap().unwrap();
    assert!(queue.enabled);
    assert!(
        queue.schedule.is_unscheduled(),
        "the fired one-shot is gone"
    );

    // a later manual stop is not undone by the (now absent) schedule
    engine.stop_queue(q).await.unwrap();
    clock.set(mon(18, 0));
    engine.check_schedules().await.unwrap();
    assert!(!engine.queue(q).await.unwrap().unwrap().enabled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn throttling_lowers_the_host_cap_and_it_is_remembered() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 4 * MIB;
    let mut req = DownloadRequest::new(
        w.server
            .url(&format!("/limit?conns=2&bps=2097152&size={size}")),
    );
    req.connections = Some(8);
    let id = engine.add(req).await.unwrap();
    wait_completed(&mut rx, id).await;
    assert_sha(&engine, id, size).await;

    let caps = engine.host_caps();
    let cap = *caps.get("127.0.0.1").expect("a cap was learned");
    assert!((1..8).contains(&cap), "learned cap {cap}");

    // it survives a restart (the write is asynchronous: wait for it)
    engine.shutdown().await;
    drop(engine);
    let mut reopened = None;
    for _ in 0..40 {
        let e = w.engine().await;
        if e.host_caps().get("127.0.0.1") == Some(&cap) {
            reopened = Some(e);
            break;
        }
        e.shutdown().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let engine = reopened.expect("learned cap persisted");

    engine.forget_host_cap("127.0.0.1").await.unwrap();
    assert!(engine.host_caps().is_empty());
    engine.shutdown().await;
    let engine = w.engine().await;
    assert!(engine.host_caps().is_empty(), "forgetting persists too");
}
