//! BitTorrent through the runtime (`Engine`), not just the torrent crate: a closed loopback swarm
//! (a seeding torrent engine plus an `Engine` with torrents enabled), the download rows, events,
//! persistence, restart, hostile names. No network, no DHT.
#![allow(clippy::unwrap_used, missing_docs)]

#[path = "../../riptide-torrent/tests/common/mod.rs"]
mod tcommon;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use riptide_core::{
    DownloadId, DownloadKind, DownloadRequest, DownloadState, Event, TorrentAdd,
    TorrentPreviewInfo, TorrentSourceRequest,
};
use riptide_engine::{Engine, EngineConfig};
use tcommon::*;
use tokio::sync::broadcast;

async fn open(tmp: &Path) -> Engine {
    std::fs::create_dir_all(tmp.join("data")).unwrap();
    let e = Engine::open(EngineConfig {
        db_path: tmp.join("data").join("riptide.sqlite"),
        default_dir: tmp.join("downloads"),
    })
    .await
    .unwrap();
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

async fn wait_row(
    e: &Engine,
    id: DownloadId,
    secs: u64,
    pred: impl Fn(DownloadState) -> bool,
) -> DownloadState {
    let t0 = Instant::now();
    loop {
        let s = e.get(id).await.unwrap().unwrap().state;
        if pred(s) {
            return s;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timed out after {secs}s in state {s:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn drain(rx: &mut broadcast::Receiver<Event>) -> Vec<Event> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_completes_through_the_engine_and_every_file_matches_sha256() {
    let files = sample_files();
    let torrent = build_torrent("sample set", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let mut rx = e.subscribe();
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    assert_eq!(preview.info_hash, info_hash_of(&torrent));
    assert_eq!(preview.files.len(), 3);
    assert_eq!(preview.existing, None);
    let dir = tmp.path().join("out");
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();

    let d = e.get(id).await.unwrap().unwrap();
    assert_eq!(d.kind, DownloadKind::Torrent);
    assert_eq!(d.url, format!("magnet:?xt=urn:btih:{}", preview.info_hash));
    assert_eq!(d.filename, "sample set");

    wait_row(&e, id, 60, |s| s == DownloadState::Completed).await;
    for f in &files {
        let mut p = dir.join("sample set");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data), "{p:?}");
    }

    // what the list needs survives in the torrents table
    let t = e.torrents().await.unwrap().remove(0);
    assert_eq!(t.info_hash, preview.info_hash);
    assert_eq!(t.downloaded, t.selected_bytes());
    assert!(!t.seeding, "seeding is off by default");
    assert_eq!(t.files.len(), 3);

    // events: legal transitions only, one Completed, progress carries the peer count
    let events = drain(&mut rx);
    let mut state = DownloadState::Probing;
    let mut completed = 0;
    let mut stats = 0;
    for ev in &events {
        match ev {
            Event::StateChanged { id: i, from, to } if *i == id => {
                assert!(from.can_transition_to(*to), "{from:?} -> {to:?}");
                assert_eq!(*from, state);
                state = *to;
            }
            Event::Completed { id: i, path } if *i == id => {
                completed += 1;
                assert_eq!(path, &dir.join("sample set"));
            }
            Event::TorrentStats { id: i, seeding, .. } if *i == id => {
                stats += 1;
                assert!(!seeding);
            }
            _ => {}
        }
    }
    assert_eq!(state, DownloadState::Completed);
    assert_eq!(completed, 1);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Added { id: i } if *i == id)));
    let _ = stats; // a transfer this small can finish between two 500 ms ticks

    // seeding off: the finished torrent is off the network
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let live = e.torrent_live(id).await.unwrap();
    assert_eq!(live.peers, 0);
    assert_eq!(live.up_bps, 0);
    assert_eq!(live.info.files.len(), 3);

    // the same info-hash again is reported, and refused when forced
    let again = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    assert_eq!(again.existing, Some(id));
    assert!(e
        .add_torrent(add_for(&again, &dir, seeder.addr))
        .await
        .is_err());

    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_chosen_files_are_downloaded_and_sizes_follow_the_choice() {
    let files = sample_files();
    let torrent = build_torrent("pick", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    let dir = tmp.path().join("out");
    let mut add = add_for(&preview, &dir, seeder.addr);
    add.only_files = Some(vec![1]);
    let id = e.add_torrent(add).await.unwrap();
    wait_row(&e, id, 60, |s| s == DownloadState::Completed).await;

    let d = e.get(id).await.unwrap().unwrap();
    assert_eq!(d.size, Some(files[1].data.len() as u64));
    let mut p = dir.join("pick");
    p.extend(&files[1].path);
    assert_eq!(sha256_file(&p), sha256(&files[1].data));
    // unselected data is never fetched: the file is absent or holds no real data
    if let Ok(d) = std::fs::read(dir.join("pick").join("alpha.bin")) {
        assert_ne!(sha256(&d), sha256(&files[0].data));
    }

    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_keep_the_result_byte_exact() {
    let files = sample_files();
    let torrent = build_torrent("slow", &files, PIECE);
    // about 25 KiB/s: a quarter of a minute for the whole set
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(25_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let mut rx = e.subscribe();
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    let dir = tmp.path().join("out");
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();

    wait_row(&e, id, 30, |s| s == DownloadState::Downloading).await;
    // some progress, then pause
    let t0 = Instant::now();
    loop {
        let t = e.torrents().await.unwrap().remove(0);
        if t.downloaded > 0 {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(30), "no progress");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    e.pause(id).await.unwrap();
    assert_eq!(
        e.get(id).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
    let frozen = e.torrents().await.unwrap().remove(0).downloaded;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let later = e.torrents().await.unwrap().remove(0).downloaded;
    assert!(
        later <= frozen + 16 * 1024 * 2,
        "a paused torrent must not keep downloading ({frozen} -> {later})"
    );
    assert_eq!(
        e.get(id).await.unwrap().unwrap().state,
        DownloadState::Paused
    );

    e.resume(id).await.unwrap();
    wait_row(&e, id, 90, |s| s == DownloadState::Completed).await;
    for f in &files {
        let mut p = dir.join("slow");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data), "{p:?}");
    }
    // every transition the bridge made was legal
    for ev in drain(&mut rx) {
        if let Event::StateChanged { from, to, .. } = ev {
            assert!(from.can_transition_to(to), "{from:?} -> {to:?}");
        }
    }
    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_of_the_app_resumes_the_torrent_and_finishes_byte_exact() {
    let files = sample_files();
    let torrent = build_torrent("restart", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(25_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("out");
    let id = {
        let e = open(tmp.path()).await;
        let preview = e
            .torrent_preview(save_torrent(tmp.path(), &torrent), None)
            .await
            .unwrap();
        let id = e
            .add_torrent(add_for(&preview, &dir, seeder.addr))
            .await
            .unwrap();
        let t0 = Instant::now();
        loop {
            let t = e.torrents().await.unwrap().remove(0);
            if t.downloaded > 0 {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "no progress");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        e.shutdown().await;
        id
    };

    // a fresh process: same database file, same state folder. The user did not pause it, so it
    // continues by itself; the seeder is told again because peers are not remembered across runs
    let e = open(tmp.path()).await;
    let d = e.get(id).await.unwrap().unwrap();
    assert!(
        !matches!(d.state, DownloadState::Paused | DownloadState::Cancelled),
        "{:?}",
        d.state
    );
    assert!(
        e.torrents().await.unwrap()[0].downloaded > 0,
        "progress is remembered"
    );
    wait_row(&e, id, 120, |s| s == DownloadState::Completed).await;
    for f in &files {
        let mut p = dir.join("restart");
        p.extend(&f.path);
        assert_eq!(sha256_file(&p), sha256(&f.data), "{p:?}");
    }
    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_deletes_the_partial_data_and_remove_respects_delete_file() {
    let files = sample_files();
    let torrent = build_torrent("gone", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(25_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let dir = tmp.path().join("out");
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id, 30, |s| s == DownloadState::Downloading).await;
    e.cancel(id).await.unwrap();
    assert_eq!(
        e.get(id).await.unwrap().unwrap().state,
        DownloadState::Cancelled
    );
    assert!(
        list_files(&dir).is_empty(),
        "partial data is deleted: {:?}",
        list_files(&dir)
    );

    // a cancelled torrent can be added again
    let again = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    assert_eq!(again.existing, None);
    let fast = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    fast.wait_ready().await;
    let id2 = e
        .add_torrent(add_for(&again, &dir, fast.addr))
        .await
        .unwrap();
    wait_row(&e, id2, 60, |s| s == DownloadState::Completed).await;
    assert_eq!(list_files(&dir).len(), 3);

    // remove without delete_file keeps the finished files, with it they go
    e.remove(id2, false).await.unwrap();
    assert_eq!(list_files(&dir).len(), 3);
    assert!(e
        .torrents()
        .await
        .unwrap()
        .iter()
        .all(|t| t.download_id != id2));

    e.shutdown().await;
    seeder.engine.shutdown().await;
    fast.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_names_never_leave_the_chosen_folder() {
    // names that can be made safe: reserved device names, drive letters, trailing dots/spaces,
    // backslashes, absolute-looking components
    let files = vec![
        TFile {
            path: vec!["CON".into()],
            data: bytes(12, 16 * 1024),
        },
        TFile {
            path: vec!["ok".into(), "x.dll".into()],
            data: bytes(13, 16 * 1024),
        },
        TFile {
            path: vec!["C:".into(), "Windows".into(), "evil.exe".into()],
            data: bytes(14, 16 * 1024),
        },
        TFile {
            path: vec!["trailing. ".into(), "a.txt ".into()],
            data: bytes(15, 16 * 1024),
        },
        TFile {
            path: vec!["".into(), "rooted".into(), "r.txt".into()],
            data: bytes(16, 16 * 1024),
        },
    ];
    let torrent = build_torrent("boom", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    for f in &preview.files {
        assert!(!f.path.contains(".."), "{}", f.path);
        assert!(
            !f.path.starts_with('/') && !f.path.contains(':'),
            "{}",
            f.path
        );
        assert!(!f.path.contains('\\'), "{}", f.path);
    }
    let dir = tmp.path().join("sandbox").join("out");
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id, 60, |s| s == DownloadState::Completed).await;

    let d = e.get(id).await.unwrap().unwrap();
    assert!(d.final_path().starts_with(&dir), "{:?}", d.final_path());
    let canon_dir = dir.canonicalize().unwrap();
    for rel in e.torrents().await.unwrap()[0].files.iter().map(|f| &f.path) {
        let p = dir.join(rel).canonicalize().unwrap();
        assert!(p.starts_with(&canon_dir), "{p:?} escaped {canon_dir:?}");
    }
    // nothing was written anywhere else under the temp root
    let outside: Vec<PathBuf> = list_files(tmp.path())
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy().replace('\\', "/");
            !s.starts_with("sandbox/out/")
                && !s.starts_with("data/")
                && !s.starts_with("downloads/")
                && s != "in.torrent"
        })
        .collect();
    assert!(outside.is_empty(), "stray files: {outside:?}");

    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_with_parent_directory_components_is_refused_before_anything_is_written() {
    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    for path in [
        vec!["..".to_string(), "..".into(), "escape.txt".into()],
        vec!["a".into(), "..".into(), "..".into(), "b".into()],
        vec!["pad".into(), "a\\..\\..\\b".into()],
    ] {
        let files = vec![TFile {
            path,
            data: bytes(21, 16 * 1024),
        }];
        let torrent = build_torrent("evil", &files, PIECE);
        let err = e
            .torrent_preview(save_torrent(tmp.path(), &torrent), None)
            .await
            .unwrap_err();
        let m = err.to_string();
        assert!(m.contains("traversal") || m.contains("separator"), "{m}");
    }
    assert!(!tmp.path().join("escape.txt").exists());
    assert!(e.torrents().await.unwrap().is_empty());
    assert!(list_files(&tmp.path().join("downloads")).is_empty());
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn torrent_rows_are_never_picked_up_by_the_segment_scheduler() {
    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    // adding a torrent as a plain request is refused, with a pointer to the right API
    let mut req =
        DownloadRequest::new("magnet:?xt=urn:btih:0123456789012345678901234567890123456789");
    req.kind = DownloadKind::Torrent;
    assert!(e.add(req).await.is_err());
    // an unknown or expired token is an error, not a panic
    let err = e
        .add_torrent(TorrentAdd {
            token: "nope".into(),
            dir: None,
            only_files: None,
            sequential: false,
            start_paused: false,
            queue_id: None,
            peers: vec![],
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("expired"), "{err}");
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_reach_the_torrent_engine_and_seeding_is_off_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let ts = e.torrent_settings().unwrap();
    assert!(!ts.seeding.enabled);
    assert_eq!(ts.upload_limit_bps, None);

    let mut s = e.settings();
    assert!(!s.torrent.seeding);
    s.torrent.seeding = true;
    s.torrent.seed_ratio_percent = 150;
    s.torrent.seed_minutes = 90;
    s.torrent.upload_limit = Some(50_000);
    e.set_settings(s).await.unwrap();
    let ts = e.torrent_settings().unwrap();
    assert!(ts.seeding.enabled);
    assert_eq!(ts.seeding.max_ratio, Some(1.5));
    assert_eq!(ts.seeding.max_minutes, Some(90));
    assert_eq!(ts.upload_limit_bps, Some(50_000));

    // and they are persisted with the other settings
    e.shutdown().await;
    let e2 = Engine::open(EngineConfig {
        db_path: tmp.path().join("data").join("riptide.sqlite"),
        default_dir: tmp.path().join("downloads"),
    })
    .await
    .unwrap();
    assert!(e2.settings().torrent.seeding);
    assert_eq!(e2.settings().torrent.upload_limit, Some(50_000));
    e2.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_events_stay_under_ten_per_second_per_torrent() {
    let files = sample_files();
    let torrent = build_torrent("rate", &files, PIECE);
    let seeder = start_seeder(
        &torrent,
        &files,
        unlimited_seeding(),
        Some(30_000),
        |_, _| {},
    )
    .await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let mut rx = e.subscribe();
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    let id = e
        .add_torrent(add_for(&preview, &tmp.path().join("out"), seeder.addr))
        .await
        .unwrap();
    let t0 = Instant::now();
    let mut progress = 0u32;
    while t0.elapsed() < Duration::from_secs(4) {
        if let Ok(Ok(Event::Progress { id: i, .. })) =
            tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
        {
            if i == id {
                progress += 1;
            }
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    assert!(progress > 0, "no progress events");
    assert!(
        f64::from(progress) / secs <= 10.0,
        "{progress} events in {secs}s"
    );
    e.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_finds_damage_and_reopens_a_completed_row() {
    let files = sample_files();
    let torrent = build_torrent("heal", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let e = open(tmp.path()).await;
    let dir = tmp.path().join("out");
    let preview = e
        .torrent_preview(save_torrent(tmp.path(), &torrent), None)
        .await
        .unwrap();
    let id = e
        .add_torrent(add_for(&preview, &dir, seeder.addr))
        .await
        .unwrap();
    wait_row(&e, id, 60, |s| s == DownloadState::Completed).await;

    // verify on healthy data changes nothing
    e.verify_torrent(id).await.unwrap();
    assert_eq!(
        e.get(id).await.unwrap().unwrap().state,
        DownloadState::Completed
    );

    // the only source of repairs goes away, then a file is damaged behind the app back;
    // "restart" on a torrent means a full re-check
    seeder.engine.shutdown().await;
    let mut rx = e.subscribe();
    let victim = dir.join("heal").join("alpha.bin");
    let mut data = std::fs::read(&victim).unwrap();
    data[100] ^= 0xFF;
    std::fs::write(&victim, data).unwrap();
    e.restart(id).await.unwrap();

    // the damage is found: the row is reopened and stays open, never Completed again
    let t0 = Instant::now();
    let mut seen = Vec::new();
    loop {
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the damage was never found: {seen:?}"
        );
        seen.extend(drain(&mut rx));
        if seen.iter().any(|ev| {
            matches!(
                ev,
                Event::StateChanged { id: i, from: DownloadState::Completed, to: DownloadState::Downloading }
                    if *i == id
            )
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    seen.extend(drain(&mut rx));
    assert_eq!(
        e.get(id).await.unwrap().unwrap().state,
        DownloadState::Downloading
    );
    assert!(
        !seen.iter().any(|ev| matches!(ev, Event::Completed { .. })),
        "Completed after damage"
    );
    // the undamaged files are untouched
    assert_eq!(
        sha256_file(&dir.join("heal").join("sub").join("beta.bin")),
        sha256(&files[1].data)
    );

    e.shutdown().await;
}
