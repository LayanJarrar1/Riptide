//! Review round of alpha.5 (P1-2, P2-4, P2-5, P2-7): torrents and the torrent engine being off.
//! Each test fails if its fix is reverted.
#![allow(clippy::unwrap_used, missing_docs)]

#[path = "../../riptide-torrent/tests/common/mod.rs"]
mod tcommon;

mod common;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{World, MIB};
use riptide_core::{
    DownloadId, DownloadRequest, DownloadState, TorrentAdd, TorrentPreviewInfo,
    TorrentSourceRequest,
};
use riptide_engine::Engine;
use tcommon::*;

async fn enable(e: &Engine) {
    e.enable_torrents_with(|s| {
        s.dht = false;
        s.trackers = false;
        s.local_discovery = false;
        s.listen_ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    })
    .await
    .unwrap();
}

fn state_dir(w: &World) -> PathBuf {
    w.dir.path().join("torrents")
}

fn save_torrent(tmp: &Path, bytes: &[u8]) -> TorrentSourceRequest {
    let p = tmp.join("in.torrent");
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

async fn wait_row(e: &Engine, id: DownloadId, secs: u64, pred: impl Fn(DownloadState) -> bool) {
    let t0 = Instant::now();
    loop {
        let s = e.get(id).await.unwrap().unwrap().state;
        if pred(s) {
            return;
        }
        assert!(t0.elapsed() < Duration::from_secs(secs), "state {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A slow seeder, a first run that starts the download, and the folder it writes to. The first
/// engine is shut down; the caller reopens the same database with the torrent engine off.
struct FirstRun {
    id: DownloadId,
    dir: PathBuf,
    hash: String,
    _seeder: Seeder,
}

async fn first_run(w: &World) -> FirstRun {
    let files = sample_files();
    let torrent = build_torrent("ghost", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(20_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;
    let e = w.engine().await;
    enable(&e).await;
    let dir = w.dir.path().join("out");
    let preview = e
        .torrent_preview(save_torrent(w.dir.path(), &torrent), None)
        .await
        .unwrap();
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id, 30, |s| s == DownloadState::Downloading).await;
    assert!(!list_files(&dir).is_empty(), "the torrent opened its files");
    e.shutdown().await;
    FirstRun {
        id,
        dir,
        hash: preview.info_hash,
        _seeder: seeder,
    }
}

fn registry_ids(w: &World) -> Vec<String> {
    riptide_torrent::offline::list(&state_dir(w))
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_while_the_torrent_engine_is_off_leaves_no_ghost() {
    let w = World::new().await;
    let r = first_run(&w).await;
    assert_eq!(registry_ids(&w), vec![r.hash.clone()]);

    let e = w.engine().await; // torrents not enabled
    assert!(!e.torrents_enabled());
    e.remove(r.id, true).await.unwrap();
    assert!(
        registry_ids(&w).is_empty(),
        "the stored torrent state still holds the removed torrent"
    );
    assert!(list_files(&r.dir).is_empty(), "unfinished data is deleted");

    // starting the engine later finds nothing to run
    enable(&e).await;
    assert!(e.torrents().await.unwrap().is_empty());
    assert!(registry_ids(&w).is_empty());
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_while_the_torrent_engine_is_off_leaves_no_ghost() {
    let w = World::new().await;
    let r = first_run(&w).await;
    let e = w.engine().await;
    e.cancel(r.id).await.unwrap();
    assert_eq!(
        e.get(r.id).await.unwrap().unwrap().state,
        DownloadState::Cancelled
    );
    assert!(registry_ids(&w).is_empty());
    assert!(list_files(&r.dir).is_empty());
    enable(&e).await;
    assert!(registry_ids(&w).is_empty());
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_registry_entry_without_a_row_is_dropped_before_the_engine_restores_it() {
    let w = World::new().await;
    let r = first_run(&w).await;
    // the row vanishes behind the torrent engine's back (an older build, a restored backup)
    {
        let mut store = riptide_store::Store::open(w.dir.path().join("riptide.sqlite")).unwrap();
        store.remove_download(r.id).unwrap();
    }
    assert_eq!(registry_ids(&w), vec![r.hash.clone()]);
    let e = w.engine().await;
    enable(&e).await;
    assert!(
        registry_ids(&w).is_empty(),
        "a torrent without a row was restored"
    );
    assert!(list_files(&r.dir).is_empty());
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_all_still_pauses_http_downloads_when_the_torrent_engine_is_off() {
    let w = World::new().await;
    let r = first_run(&w).await;
    let e = w.engine().await;
    assert!(!e.torrents_enabled());
    let mut rx = e.subscribe();
    let http = e
        .add(DownloadRequest {
            filename: Some("slow.bin".into()),
            ..w.request(&format!("/slow?bps={MIB}&size={}&seed=3", 16 * MIB))
        })
        .await
        .unwrap();
    common::wait_for(
        &mut rx,
        Duration::from_secs(30),
        |ev| matches!(ev, riptide_core::Event::Progress { id, .. } if *id == http),
    )
    .await;
    e.pause_all().await.unwrap();
    assert_eq!(
        e.get(http).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
    // the torrent that was left downloading is recorded as paused too, so no engine revives it
    assert_eq!(
        e.get(r.id).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
    let snap = riptide_torrent::offline::list(&state_dir(&w)).unwrap();
    assert!(matches!(
        snap[0].state,
        riptide_torrent::TorrentState::Paused
    ));
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_engine_starts_at_launch_only_for_torrents_that_need_the_network() {
    let w = World::new().await;
    let r = first_run(&w).await; // a torrent left downloading
    let tweak = |s: &mut riptide_torrent::TorrentSettings| {
        s.dht = false;
        s.trackers = false;
        s.local_discovery = false;
        s.listen_ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    };

    // paused by the user while the app was off: no port, no DHT node at the next launch
    let e = w.engine().await;
    e.pause(r.id).await.unwrap();
    assert!(!e.enable_torrents_if_active_with(tweak).await.unwrap());
    assert!(!e.torrents_enabled());
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unfinished_unpaused_torrent_starts_the_engine_at_launch() {
    let w = World::new().await;
    let _r = first_run(&w).await;
    let e = w.engine().await;
    assert!(!e.torrents_enabled());
    let started = e
        .enable_torrents_if_active_with(|s| {
            s.dht = false;
            s.trackers = false;
            s.local_discovery = false;
            s.listen_ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        })
        .await
        .unwrap();
    assert!(started && e.torrents_enabled());
    e.shutdown().await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_finished_file_is_marked_while_the_rest_of_the_torrent_still_runs() {
    // the third file can never be completed: the seeder holds the wrong bytes for it
    let files = sample_files();
    let torrent = build_torrent("partial set", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        None,
        |data, index| {
            if index == 2 {
                data.iter_mut().for_each(|b| *b ^= 0xFF);
            }
        },
    )
    .await;
    let lens: Vec<u64> = files.iter().map(|f| f.data.len() as u64).collect();
    wait_state(&seeder.engine, &seeder.id, 60, |s| {
        s.file_progress.len() == 3 && s.file_progress[0] >= lens[0] && s.file_progress[1] >= lens[1]
    })
    .await;

    let w = World::new().await;
    let e = w.engine().await;
    enable(&e).await;
    let dir = w.dir.path().join("out");
    let preview = e
        .torrent_preview(save_torrent(w.dir.path(), &torrent), None)
        .await
        .unwrap();
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();

    let first = dir.join("partial set").join("alpha.bin");
    let mut stream = first.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    let t0 = Instant::now();
    loop {
        if std::fs::read(&stream).is_ok_and(|b| String::from_utf8_lossy(&b).contains("ZoneId=3")) {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "alpha.bin was never marked while the torrent was running"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let state = e.get(id).await.unwrap().unwrap().state;
    assert_ne!(
        state,
        DownloadState::Completed,
        "the torrent must still be running"
    );
    e.shutdown().await;
}

/// A torrent whose names are hostile (so the planned layout differs from them) and that lists
/// `seed` as its web seed.
fn seeded_torrent(seed: &str) -> Vec<u8> {
    let f = |path: &[&str], seed: u64| TFile {
        path: path.iter().map(|s| (*s).to_string()).collect(),
        data: bytes(seed, 20_000),
    };
    let files = vec![
        f(&["CON"], 1),
        f(&["a b.txt"], 2),
        f(&["A B.TXT"], 3),
        f(&["sub", "c#1.bin"], 4),
    ];
    build_torrent_with(
        "set.",
        &files,
        PIECE,
        vec![("url-list", B::List(vec![B::s(seed)]))],
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn web_seed_downloads_use_the_names_of_the_torrent_and_refuse_private_addresses() {
    let w = World::new().await;
    let e = w.engine().await;
    enable(&e).await;

    // a seed on the machine itself, on the LAN, or at the cloud metadata address is refused
    for private in [
        "http://127.0.0.1:9/seed/",
        "http://localhost/seed/",
        "http://192.168.1.1/seed/",
        "http://169.254.169.254/seed/",
        "http://[::1]/seed/",
    ] {
        let p = e
            .torrent_preview(save_torrent(w.dir.path(), &seeded_torrent(private)), None)
            .await
            .unwrap();
        assert_eq!(p.web_seeds.len(), 1, "{private}");
        let r = e
            .torrent_web_seed_add(&p.token, None, Some(w.dir.path().join("mirror")), true)
            .await;
        assert!(r.is_err(), "{private} must be refused: {r:?}");
    }
    assert!(e.list().await.unwrap().is_empty(), "nothing was added");

    // a public address is accepted; the files are fetched under the torrent's own names
    let p = e
        .torrent_preview(
            save_torrent(w.dir.path(), &seeded_torrent("http://93.184.216.34/seed/")),
            None,
        )
        .await
        .unwrap();
    let dir = w.dir.path().join("mirror");
    let done = e
        .torrent_web_seed_add(&p.token, None, Some(dir.clone()), true)
        .await
        .unwrap();
    assert_eq!(done.added.len(), 4);
    let mut urls = Vec::new();
    for id in &done.added {
        let d = e.get(*id).await.unwrap().unwrap();
        assert_eq!(d.state, DownloadState::Paused);
        urls.push(d.url.clone());
    }
    urls.sort();
    assert_eq!(
        urls,
        vec![
            "http://93.184.216.34/seed/set./A%20B.TXT",
            "http://93.184.216.34/seed/set./CON",
            "http://93.184.216.34/seed/set./a%20b.txt",
            "http://93.184.216.34/seed/set./sub/c%231.bin",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
    e.shutdown().await;
}
