//! A finished torrent stops all network activity: once nothing needs the torrent engine it is
//! stopped completely (listening port, DHT, uTP, peer connections) and started again when needed.
//! The test looks at the real sockets of this process with `netstat`. It lives in a file of its own
//! so no other test of the process owns sockets on the port it watches. Windows only.
#![cfg(windows)]
#![allow(clippy::unwrap_used, missing_docs)]

#[path = "../../riptide-torrent/tests/common/mod.rs"]
mod tcommon;

mod common;

use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use common::World;
use riptide_core::{
    DownloadId, DownloadState, TorrentAdd, TorrentPreviewInfo, TorrentSourceRequest,
};
use riptide_engine::Engine;
use tcommon::*;

/// Sockets of this process whose local port is `port`, from `netstat -ano`.
fn sockets_on(port: u16) -> Vec<String> {
    let out = std::process::Command::new("netstat")
        .args(["-ano"])
        .output()
        .unwrap();
    let pid = std::process::id().to_string();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            cols.len() >= 4
                && cols.last() == Some(&pid.as_str())
                && cols.get(1).is_some_and(|a| a.ends_with(&format!(":{port}")))
        })
        .map(|l| l.trim().to_string())
        .collect()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn save_torrent(dir: &Path, name: &str, bytes: &[u8]) -> TorrentSourceRequest {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    TorrentSourceRequest::File { path: p }
}

fn add_for(p: &TorrentPreviewInfo, dir: &Path, peer: SocketAddr) -> TorrentAdd {
    TorrentAdd {
        token: p.token.clone(),
        dir: Some(dir.to_path_buf()),
        only_files: None,
        sequential: false,
        start_paused: false,
        queue_id: None,
        peers: vec![peer],
    }
}

async fn wait_for(secs: u64, what: &str, mut ok: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !ok() {
        assert!(t0.elapsed() < Duration::from_secs(secs), "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_row(e: &Engine, id: DownloadId, state: DownloadState) {
    let t0 = Instant::now();
    loop {
        let s = e.get(id).await.unwrap().unwrap().state;
        if s == state {
            return;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "row stuck in {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct Set {
    torrent: Vec<u8>,
    seeder: Seeder,
}

async fn make(name: &str, seed: u64) -> Set {
    let files = vec![
        TFile {
            path: vec![format!("{name}-a.bin")],
            data: bytes(seed, 16 * 1024 * 3),
        },
        TFile {
            path: vec![format!("{name}-b.bin")],
            data: bytes(seed + 1, 16 * 1024 * 2),
        },
    ];
    let torrent = build_torrent(name, &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;
    Set { torrent, seeder }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_finished_torrent_leaves_no_socket_and_the_engine_comes_back_when_needed() {
    let port = free_port();
    let w = World::new().await;
    let e = w.engine().await;
    e.set_torrent_idle_grace(Duration::from_millis(500));
    e.enable_torrents_with(move |s| {
        s.dht = false;
        s.trackers = false;
        s.local_discovery = false;
        s.listen_ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        s.listen_port = port;
    })
    .await
    .unwrap();
    assert!(!sockets_on(port).is_empty(), "the watcher sees an open port");

    // 1. a torrent completes (seeding is off): the engine stops, nothing is left open
    let one = make("one", 10).await;
    let dir = w.dir.path().join("out");
    let p = e
        .torrent_preview(save_torrent(w.dir.path(), "one.torrent", &one.torrent), None)
        .await
        .unwrap();
    let id1 = e
        .add_torrent(add_for(&p, &dir, one.seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id1, DownloadState::Completed).await;
    wait_for(20, "the torrent engine stops after completion", || {
        !e.torrents_enabled()
    })
    .await;
    assert!(
        sockets_on(port).is_empty(),
        "sockets left open: {:?}",
        sockets_on(port)
    );
    assert_eq!(
        e.get(id1).await.unwrap().unwrap().state,
        DownloadState::Completed,
        "the finished row survives"
    );

    // 2. adding again after the stop starts the engine again (same settings) and completes
    let two = make("two", 20).await;
    let p = e
        .torrent_preview(save_torrent(w.dir.path(), "two.torrent", &two.torrent), None)
        .await
        .unwrap();
    assert!(e.torrents_enabled());
    assert!(!sockets_on(port).is_empty(), "it listens again while it works");
    let id2 = e
        .add_torrent(add_for(&p, &dir, two.seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id2, DownloadState::Completed).await;
    wait_for(20, "stopped again", || !e.torrents_enabled()).await;
    assert!(sockets_on(port).is_empty(), "{:?}", sockets_on(port));

    // 3. a re-check of the first torrent after a stop works and stops again
    e.verify_torrent(id1).await.unwrap();
    wait_row(&e, id1, DownloadState::Completed).await;
    wait_for(20, "stopped after the re-check", || !e.torrents_enabled()).await;
    assert!(sockets_on(port).is_empty());

    // 4. two adds arriving together from a stopped engine both work
    let three = make("three", 30).await;
    let four = make("four", 40).await;
    let p3 = e
        .torrent_preview(save_torrent(w.dir.path(), "three.torrent", &three.torrent), None)
        .await
        .unwrap();
    let p4 = e
        .torrent_preview(save_torrent(w.dir.path(), "four.torrent", &four.torrent), None)
        .await
        .unwrap();
    wait_for(20, "stopped with two previews waiting", || !e.torrents_enabled()).await;
    let (a, b) = tokio::join!(
        e.add_torrent(add_for(&p3, &dir, three.seeder.addr)),
        e.add_torrent(add_for(&p4, &dir, four.seeder.addr)),
    );
    let (id3, id4) = (a.unwrap(), b.unwrap());
    wait_row(&e, id3, DownloadState::Completed).await;
    wait_row(&e, id4, DownloadState::Completed).await;
    wait_for(20, "stopped after two adds", || !e.torrents_enabled()).await;
    assert!(sockets_on(port).is_empty());

    // 5. with seeding on the port stays open for as long as the torrent seeds
    let mut settings = e.settings();
    settings.torrent.seeding = true;
    settings.torrent.seed_ratio_percent = 0;
    settings.torrent.seed_minutes = 0;
    e.set_settings(settings).await.unwrap();
    let five = make("five", 50).await;
    let p5 = e
        .torrent_preview(save_torrent(w.dir.path(), "five.torrent", &five.torrent), None)
        .await
        .unwrap();
    let id5 = e
        .add_torrent(add_for(&p5, &dir, five.seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id5, DownloadState::Completed).await;
    tokio::time::sleep(Duration::from_secs(3)).await; // six times the grace
    assert!(e.torrents_enabled(), "a seeding torrent keeps the engine");
    assert!(!sockets_on(port).is_empty(), "and its port");

    // turning seeding off stops the torrent and then the engine
    let mut settings = e.settings();
    settings.torrent.seeding = false;
    e.set_settings(settings).await.unwrap();
    wait_for(30, "stopped after seeding was turned off", || {
        !e.torrents_enabled()
    })
    .await;
    assert!(sockets_on(port).is_empty(), "{:?}", sockets_on(port));

    e.shutdown().await;
    for s in [one, two, three, four, five] {
        s.seeder.engine.shutdown().await;
    }
}
