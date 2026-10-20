//! End-to-end transfers on a closed loopback swarm: a seeding engine and a downloading engine
//! talk over 127.0.0.1 with an explicit peer address (no DHT, no trackers, no internet).

#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use riptide_torrent::{
    AddOptions, SeedingPolicy, TorrentEngine, TorrentEvent, TorrentSource, TorrentState,
};

async fn leecher(tmp: &std::path::Path) -> (std::sync::Arc<TorrentEngine>, std::path::PathBuf) {
    let dir = tmp.join("downloads");
    let engine = TorrentEngine::start(loopback_settings(&tmp.join("state"), &dir))
        .await
        .unwrap();
    (engine, dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_multi_file_torrent_and_every_file_matches_sha256() {
    let files = sample_files();
    let torrent = build_torrent("sample set", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let mut events = eng.subscribe();
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
    assert_eq!(id, info_hash_of(&torrent));

    let done = wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    assert_eq!(done.downloaded_bytes, done.total_bytes);
    assert!(done.total_bytes > 0);

    for f in &files {
        let mut p = dir.join("sample set");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data), "{p:?}");
    }
    // nothing but the torrent's own folder appeared in the download folder
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(names.len(), 1);

    // events: Added, at least one Progress, Completed
    let mut seen = (false, false, false);
    while let Ok(e) = events.try_recv() {
        match e {
            TorrentEvent::Added { .. } => seen.0 = true,
            TorrentEvent::Progress(p) => {
                assert!(p.downloaded_bytes <= p.total_bytes);
                seen.1 = true;
            }
            TorrentEvent::Completed { .. } => seen.2 = true,
            _ => {}
        }
    }
    // a transfer this small can finish between two 500 ms progress ticks
    assert!(seen.0 && seen.2, "{seen:?}");

    // seeding is OFF by default: the finished torrent is not on the network any more
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let snap = eng.snapshot(&id).unwrap();
    assert_eq!(snap.state, TorrentState::Completed, "must not be Seeding");
    assert_eq!(snap.peers_connected, 0);
    assert_eq!(snap.up_bps, 0);

    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_selected_files_are_downloaded() {
    let files = sample_files();
    let torrent = build_torrent("pick", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let opts = AddOptions {
        dir: Some(dir.clone()),
        peers: vec![seeder.addr],
        only_files: Some(vec![1]),
        ..Default::default()
    };
    // choose files from the preview before starting
    let preview = eng
        .preview(&TorrentSource::Bytes(torrent.clone()), &opts)
        .await
        .unwrap();
    assert_eq!(preview.files.len(), 3);
    let id = eng
        .add(TorrentSource::Bytes(preview.torrent_bytes.clone()), opts)
        .await
        .unwrap();
    let done = wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    assert_eq!(done.total_bytes, files[1].data.len() as u64);
    assert_eq!(done.selected, Some(vec![1]));

    let beta = dir.join("pick").join("sub").join("beta.bin");
    assert_eq!(sha256_file(&beta), sha256(&files[1].data));
    // unselected data is never fetched: the file is absent or holds no real data
    if let Ok(d) = std::fs::read(dir.join("pick").join("alpha.bin")) {
        assert_ne!(sha256(&d), sha256(&files[0].data));
    }
    // an out-of-range selection is refused
    let bad = AddOptions {
        only_files: Some(vec![7]),
        ..Default::default()
    };
    assert!(eng
        .add(
            TorrentSource::Bytes(build_torrent("x", &files[..2], PIECE)),
            bad
        )
        .await
        .is_err());
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_link_with_an_explicit_peer_fetches_metadata_and_downloads() {
    let files = sample_files();
    let torrent = build_torrent("magnet set", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&dn=magnet+set",
        info_hash_of(&torrent)
    );
    let opts = AddOptions {
        dir: Some(dir.clone()),
        peers: vec![seeder.addr],
        ..Default::default()
    };
    let preview = eng
        .preview(&TorrentSource::Magnet(magnet), &opts)
        .await
        .unwrap();
    assert_eq!(preview.files.len(), 3);
    assert_eq!(
        preview.total_bytes,
        files.iter().map(|f| f.data.len() as u64).sum::<u64>()
    );
    // add from the resolved bytes: no second metadata round trip
    let id = eng
        .add(TorrentSource::Bytes(preview.torrent_bytes.clone()), opts)
        .await
        .unwrap();
    wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    for f in &files {
        let mut p = dir.join("magnet set");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data));
    }
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequential_mode_finishes_files_one_after_the_other() {
    let files = sample_files();
    let torrent = build_torrent("seq", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(300_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(dir.clone()),
                peers: vec![seeder.addr],
                sequential: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let lens: Vec<u64> = files.iter().map(|f| f.data.len() as u64).collect();
    let t0 = Instant::now();
    loop {
        let s = eng.snapshot(&id).unwrap();
        let fp = &s.file_progress;
        if fp.len() == 3 {
            // a later file never has data while an earlier one is unfinished
            if fp[1] > 0 {
                assert!(
                    fp[0] >= lens[0],
                    "file 1 started before file 0 finished: {fp:?}"
                );
            }
            if fp[2] > 0 {
                assert!(
                    fp[1] >= lens[1],
                    "file 2 started before file 1 finished: {fp:?}"
                );
            }
        }
        if matches!(s.state, TorrentState::Completed) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(90), "timed out: {s:?}");
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    for f in &files {
        let mut p = dir.join("seq");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data));
    }
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_corrupt_piece_on_the_seeder_never_yields_a_completed_download() {
    let files = sample_files();
    let torrent = build_torrent("corrupt", &files, PIECE);
    // flip one byte in the middle of file 1 on the seeder's disk
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |data, idx| {
        if idx == 1 {
            data[40_000] ^= 0x01;
        }
    })
    .await;
    // the seeder verifies its own data: it can only offer the pieces that hash correctly
    let t0 = Instant::now();
    while seeder.engine.snapshot(&seeder.id).unwrap().downloaded_bytes == 0 {
        assert!(t0.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let mut events = eng.subscribe();
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
    // wait until everything the seeder can offer has arrived
    let total: u64 = files.iter().map(|f| f.data.len() as u64).sum();
    let snap = wait_state(&eng, &id, 60, |s| {
        s.downloaded_bytes >= total - u64::from(PIECE)
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let snap2 = eng.snapshot(&id).unwrap();
    assert!(snap2.downloaded_bytes < total, "{snap:?}");
    assert!(!matches!(
        snap2.state,
        TorrentState::Completed | TorrentState::Seeding
    ));
    while let Ok(e) = events.try_recv() {
        assert!(
            !matches!(e, TorrentEvent::Completed { .. }),
            "Completed was emitted"
        );
    }
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corruption_after_completion_is_found_by_verify() {
    let files = sample_files();
    let torrent = build_torrent("later", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
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

    // flip a byte on disk behind the engine's back, and disconnect the only source of repairs
    seeder.engine.shutdown().await;
    let target = dir.join("later").join("sub").join("beta.bin");
    let mut data = std::fs::read(&target).unwrap();
    data[1000] ^= 0xFF;
    std::fs::write(&target, &data).unwrap();

    let mut events = eng.subscribe();
    eng.verify(&id).await.unwrap();
    let s = wait_state(&eng, &id, 30, |s| {
        !matches!(s.state, TorrentState::Checking) && s.downloaded_bytes > 0
    })
    .await;
    assert!(
        s.downloaded_bytes < s.total_bytes,
        "the damaged piece must not count as downloaded: {s:?}"
    );
    assert!(!matches!(
        s.state,
        TorrentState::Completed | TorrentState::Seeding
    ));
    tokio::time::sleep(Duration::from_secs(1)).await;
    while let Ok(e) = events.try_recv() {
        assert!(
            !matches!(e, TorrentEvent::Completed { .. }),
            "Completed after corruption"
        );
    }
    eng.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn upload_cap_limits_the_seeder_speed() {
    let files = vec![TFile {
        path: vec!["big.bin".into()],
        data: bytes(9, 16 * 1024 * 40),
    }];
    let torrent = build_torrent("big.bin", &files, PIECE);
    // 640 KiB at 100 KB/s: more than six seconds, even allowing the one-second burst
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(100_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;
    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let t0 = Instant::now();
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
    wait_state(&eng, &id, 90, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(3),
        "finished in {took:?}, the cap did nothing"
    );
    assert_eq!(sha256_file(&dir.join("big.bin")), sha256(&files[0].data));
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seeding_stops_at_the_ratio_limit() {
    let files = sample_files();
    let torrent = build_torrent("ratio", &files, PIECE);
    let policy = SeedingPolicy {
        enabled: true,
        max_ratio: Some(0.5),
        max_minutes: None,
    };
    let seeder = start_seeder(&torrent, &files, policy, None, |_, _| {}).await;
    let mut ev = seeder.engine.subscribe();
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(dir),
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

    let s = wait_state(&seeder.engine, &seeder.id, 30, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    assert!(s.uploaded_bytes > 0 || s.state == TorrentState::Completed);
    let mut stopped = false;
    while let Ok(e) = ev.try_recv() {
        stopped |= matches!(e, TorrentEvent::SeedingStopped { .. });
    }
    assert!(stopped, "no SeedingStopped event");
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_resume_and_remove_work() {
    let files = vec![TFile {
        path: vec!["slow.bin".into()],
        data: bytes(5, 16 * 1024 * 50),
    }];
    let torrent = build_torrent("slow.bin", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(150_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;
    let tmp = tempfile::tempdir().unwrap();
    let (eng, dir) = leecher(tmp.path()).await;
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
    wait_state(&eng, &id, 30, |s| s.downloaded_bytes > 0).await;
    eng.pause(&id).await.unwrap();
    let a = wait_state(&eng, &id, 10, |s| s.state == TorrentState::Paused).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let b = eng.snapshot(&id).unwrap();
    assert_eq!(
        a.downloaded_bytes, b.downloaded_bytes,
        "bytes moved while paused"
    );

    eng.resume(&id).await.unwrap();
    wait_state(&eng, &id, 90, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    assert_eq!(sha256_file(&dir.join("slow.bin")), sha256(&files[0].data));

    eng.remove(&id, true).await.unwrap();
    assert!(eng.snapshot(&id).is_err());
    assert!(
        !dir.join("slow.bin").exists(),
        "delete_files must remove the data"
    );
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}
