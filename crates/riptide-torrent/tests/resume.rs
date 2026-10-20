//! Crash safety: a downloading process is killed outright (TerminateProcess / SIGKILL, no
//! shutdown code runs) and a new process continues from what is on disk.

#![allow(clippy::unwrap_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::*;
use riptide_torrent::{AddOptions, TorrentEngine, TorrentSource, TorrentState};

/// Body of the child process. Runs only when the parent starts it (`RT_KILL_*` set).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "helper process of resume_after_kill; not a test on its own"]
async fn kill_child() {
    let Ok(state) = std::env::var("RT_KILL_STATE") else {
        return;
    };
    let dir = PathBuf::from(std::env::var("RT_KILL_DIR").unwrap());
    let torrent = std::fs::read(std::env::var("RT_KILL_TORRENT").unwrap()).unwrap();
    let peer = std::env::var("RT_KILL_PEER").unwrap().parse().unwrap();

    let eng = TorrentEngine::start(loopback_settings(&PathBuf::from(state), &dir))
        .await
        .unwrap();
    let id = eng
        .add(
            TorrentSource::Bytes(torrent),
            AddOptions {
                dir: Some(dir),
                peers: vec![peer],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    loop {
        let s = eng.snapshot(&id).unwrap();
        println!("PROGRESS {}", s.downloaded_bytes);
        std::io::stdout().flush().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_after_kill_continues_from_disk_and_matches_sha256() {
    let files = vec![TFile {
        path: vec!["resume.bin".into()],
        data: bytes(77, 16 * 1024 * 64),
    }];
    let torrent = build_torrent("resume.bin", &files, PIECE);
    // 1 MiB at ~200 kB/s: the child is still far from done when it is killed
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(200_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let dir = tmp.path().join("downloads");
    let torrent_path = tmp.path().join("t.torrent");
    std::fs::write(&torrent_path, &torrent).unwrap();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "kill_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("RT_KILL_STATE", &state)
        .env("RT_KILL_DIR", &dir)
        .env("RT_KILL_TORRENT", &torrent_path)
        .env("RT_KILL_PEER", seeder.addr.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<u64>();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(n) = line.strip_prefix("PROGRESS ") {
                if let Ok(n) = n.trim().parse() {
                    if tx.send(n).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let t0 = Instant::now();
    let mut at_kill = 0;
    while at_kill < 250_000 {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(n) => at_kill = n,
            Err(_) => assert!(
                t0.elapsed() < Duration::from_secs(60),
                "child made no progress"
            ),
        }
    }
    // no graceful shutdown: the process is terminated on the spot
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        at_kill < 16 * 1024 * 64,
        "child finished before it was killed"
    );

    // the registry survived and says "not completed"; partial data is on disk
    let reg = std::fs::read_to_string(state.join("riptide-torrents.json")).unwrap();
    assert!(reg.contains("\"completed\": false"), "{reg}");
    assert!(dir.join("resume.bin").exists());

    // a fresh process (new engine, same folders) continues and finishes byte-exact
    let eng = TorrentEngine::start(loopback_settings(&state, &dir))
        .await
        .unwrap();
    let id = info_hash_of(&torrent);
    let early = wait_state(&eng, &id, 30, |s| {
        !matches!(s.state, TorrentState::Checking) && s.downloaded_bytes > 0
    })
    .await;
    assert!(
        early.downloaded_bytes >= 100_000,
        "resume should keep what was already on disk: {early:?}"
    );
    wait_state(&eng, &id, 90, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    assert_eq!(sha256_file(&dir.join("resume.bin")), sha256(&files[0].data));
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paused_and_completed_torrents_stay_off_the_network_after_a_restart() {
    let files = vec![TFile {
        path: vec!["stay.bin".into()],
        data: bytes(5, 16 * 1024 * 8),
    }];
    let torrent = build_torrent("stay.bin", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let dir = tmp.path().join("downloads");
    let eng = TorrentEngine::start(loopback_settings(&state, &dir))
        .await
        .unwrap();
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(dir.clone()),
                peers: vec![seeder.addr],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    eng.shutdown().await;

    // restart: completed + seeding off => not even loaded into the library
    let eng = TorrentEngine::start(loopback_settings(&state, &dir))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let s = eng.snapshot(&id).unwrap();
    assert_eq!(s.state, TorrentState::Completed);
    assert_eq!(s.peers_connected, 0);
    assert_eq!(s.up_bps, 0);
    assert_eq!(s.downloaded_bytes, s.total_bytes);

    // a paused-by-the-user torrent also stays paused
    let other = vec![TFile {
        path: vec!["paused.bin".into()],
        data: bytes(6, 16 * 1024 * 8),
    }];
    let t2 = build_torrent("paused.bin", &other, PIECE);
    let id2 = eng
        .add(
            TorrentSource::Bytes(t2),
            AddOptions {
                dir: Some(dir.clone()),
                start_paused: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(eng.snapshot(&id2).unwrap().state, TorrentState::Paused);
    eng.shutdown().await;
    let eng = TorrentEngine::start(loopback_settings(&state, &dir))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(eng.snapshot(&id2).unwrap().state, TorrentState::Paused);
    assert!(
        !dir.join("paused.bin").exists(),
        "a paused torrent must not touch the disk"
    );
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}
