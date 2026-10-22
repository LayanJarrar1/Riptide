//! The torrent commands as the webview calls them (`riptide_desktop_lib::torrent`), against a
//! loopback swarm: a hostile torrent must leave nothing outside the folder the user chose, and a
//! normal one must arrive byte-exact. No network.
#![allow(clippy::unwrap_used, missing_docs)]

#[path = "../../../../crates/riptide-torrent/tests/common/mod.rs"]
mod tcommon;

use std::time::{Duration, Instant};

use riptide_core::{DownloadState, TorrentAdd, TorrentSourceRequest};
use riptide_desktop_lib::torrent;
use riptide_engine::{Engine, EngineConfig};
use tcommon::*;

async fn engine(root: &std::path::Path) -> Engine {
    std::fs::create_dir_all(root.join("data")).unwrap();
    let e = Engine::open(EngineConfig {
        db_path: root.join("data").join("riptide.sqlite"),
        default_dir: root.join("downloads"),
        organize_new_install: false,
    })
    .await
    .unwrap();
    // the commands start the torrent engine themselves; this run keeps it on loopback
    e.enable_torrents_with(|s| {
        s.dht = false;
        s.trackers = false;
        s.local_discovery = false;
        s.listen_ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    })
    .await
    .unwrap();
    e
}

async fn wait_done(e: &Engine, id: i64) {
    let t0 = Instant::now();
    loop {
        let d = e.get(riptide_core::DownloadId(id)).await.unwrap().unwrap();
        if d.state == DownloadState::Completed {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "state {:?}",
            d.state
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_torrent_names_stay_inside_the_chosen_folder() {
    let files = vec![
        TFile {
            path: vec!["CON".into()],
            data: bytes(1, 16 * 1024),
        },
        TFile {
            path: vec![
                "C:".into(),
                "Windows".into(),
                "System32".into(),
                "evil.dll".into(),
            ],
            data: bytes(2, 16 * 1024),
        },
        TFile {
            path: vec!["aux.txt".into()],
            data: bytes(3, 16 * 1024),
        },
        TFile {
            path: vec!["dots. .".into(), "end .".into()],
            data: bytes(4, 16 * 1024),
        },
    ];
    let torrent = build_torrent("hostile", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path()).await;
    let file = tmp.path().join("hostile.torrent");
    std::fs::write(&file, &torrent).unwrap();
    let chosen = tmp.path().join("chosen folder");

    let preview = torrent::preview(
        &e,
        TorrentSourceRequest::File { path: file },
        Some(chosen.to_string_lossy().into_owned()),
    )
    .await
    .unwrap();
    assert_eq!(preview.files.len(), 4);
    for f in &preview.files {
        assert!(
            !f.path.contains(':') && !f.path.starts_with('/'),
            "{}",
            f.path
        );
        assert!(
            f.path
                .split('/')
                .all(|c| !c.is_empty() && c != ".." && c != "."),
            "{}",
            f.path
        );
    }
    let id = torrent::add(
        &e,
        TorrentAdd {
            token: preview.token,
            dir: Some(chosen.clone()),
            only_files: None,
            sequential: false,
            start_paused: false,
            queue_id: None,
            peers: vec![seeder.addr],
        },
    )
    .await
    .unwrap();
    wait_done(&e, id).await;

    // everything the torrent wrote is below the chosen folder, and nothing else was created
    let canon = chosen.canonicalize().unwrap();
    for rel in &e.torrents().await.unwrap()[0].files {
        let p = chosen.join(&rel.path).canonicalize().unwrap();
        assert!(p.starts_with(&canon), "{p:?}");
    }
    for rel in list_files(tmp.path()) {
        let s = rel.to_string_lossy().replace('\\', "/");
        assert!(
            s.starts_with("chosen folder/") || s.starts_with("data/") || s == "hostile.torrent",
            "unexpected file {s}"
        );
    }
    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_good_torrent_arrives_byte_exact_through_the_commands() {
    let files = sample_files();
    let torrent = build_torrent("good set", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path()).await;
    let file = tmp.path().join("good.torrent");
    std::fs::write(&file, &torrent).unwrap();
    let preview = torrent::preview(&e, TorrentSourceRequest::File { path: file }, None)
        .await
        .unwrap();
    let dir = tmp.path().join("out");
    let id = torrent::add(
        &e,
        TorrentAdd {
            token: preview.token,
            dir: Some(dir.clone()),
            only_files: None,
            sequential: true,
            start_paused: false,
            queue_id: None,
            peers: vec![seeder.addr],
        },
    )
    .await
    .unwrap();
    wait_done(&e, id).await;
    for f in &files {
        let mut p = dir.join("good set");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data), "{p:?}");
    }
    e.shutdown().await;
    seeder.engine.shutdown().await;
}
