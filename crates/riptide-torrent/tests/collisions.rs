//! Two torrents (or a torrent and a download) that want the same file name must never share a
//! file, and removing a torrent must never delete a file it did not create. Review round of
//! alpha.5 (P1-1, P3 junction). Each test fails if its fix is reverted.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use riptide_torrent::registry::{Entry, Registry};
use riptide_torrent::{offline, AddOptions, TorrentEngine, TorrentSource, TorrentState};

fn one(name: &str, len: usize, seed: u64) -> (Vec<TFile>, Vec<u8>) {
    let files = vec![TFile {
        path: vec![name.to_string()],
        data: bytes(seed, len),
    }];
    let torrent = build_torrent(name, &files, PIECE);
    (files, torrent)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_later_added_torrent_does_not_take_the_name_of_one_that_has_not_started() {
    // A and B both ship "same.bin" with different content (so different info-hashes)
    let (files_a, torrent_a) = one("same.bin", 16 * 1024 * 3 + 100, 11);
    let (files_b, torrent_b) = one("same.bin", 16 * 1024 * 5, 22);
    let seeder = start_seeder(&torrent_b, &files_b, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("out");
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state"), &dir))
        .await
        .unwrap();
    // A is recorded but never started: nothing is on disk
    let a = eng
        .add(
            TorrentSource::Bytes(torrent_a.clone()),
            AddOptions {
                dir: Some(dir.clone()),
                start_paused: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let opts_b = AddOptions {
        dir: Some(dir.clone()),
        peers: vec![seeder.addr],
        ..Default::default()
    };
    let prev_b = eng
        .preview(&TorrentSource::Bytes(torrent_b.clone()), &opts_b)
        .await
        .unwrap();
    assert_ne!(
        prev_b.files[0].path,
        PathBuf::from("same.bin"),
        "B must not plan the name A already holds"
    );
    let b = eng
        .add(TorrentSource::Bytes(torrent_b.clone()), opts_b)
        .await
        .unwrap();
    wait_state(&eng, &b, 60, |s| matches!(s.state, TorrentState::Completed)).await;
    let b_file = dir.join(&prev_b.files[0].path);
    assert_eq!(sha256_file(&b_file), sha256(&files_b[0].data));

    // resuming A must not open (and resize) B's file
    eng.resume(&a).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(
        sha256_file(&b_file),
        sha256(&files_b[0].data),
        "B's verified file was touched by A"
    );
    assert!(dir.join("same.bin").exists(), "A has its own file");
    assert_eq!(
        std::fs::metadata(dir.join("same.bin")).unwrap().len(),
        files_a[0].data.len() as u64
    );

    // and cancelling A deletes A's file only
    eng.remove(&a, true).await.unwrap();
    assert!(!dir.join("same.bin").exists());
    assert_eq!(sha256_file(&b_file), sha256(&files_b[0].data));
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn names_claimed_by_downloads_and_part_files_are_avoided_too() {
    let (_files, torrent) = one("movie.mkv", 16 * 1024 * 2, 5);
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state"), &dir))
        .await
        .unwrap();
    let plan = |opts: AddOptions| {
        let eng = eng.clone();
        let t = torrent.clone();
        async move {
            eng.preview(&TorrentSource::Bytes(t), &opts)
                .await
                .unwrap()
                .files[0]
                .path
                .clone()
        }
    };
    let base = AddOptions {
        dir: Some(dir.clone()),
        ..Default::default()
    };
    assert_eq!(plan(base.clone()).await, PathBuf::from("movie.mkv"));
    // an HTTP download of this engine claims the name (queued, nothing on disk)
    let claimed = AddOptions {
        reserved: vec![dir.join("Movie.mkv")],
        ..base.clone()
    };
    assert_ne!(plan(claimed).await, PathBuf::from("movie.mkv"));
    // a partial download that will be renamed to the name
    std::fs::write(dir.join("movie.mkv.rtpart"), b"x").unwrap();
    assert_ne!(plan(base).await, PathBuf::from("movie.mkv"));
    eng.shutdown().await;
}

fn entry(dir: &Path, files: &[&str], created: Option<Vec<PathBuf>>) -> Entry {
    Entry {
        dir: dir.to_path_buf(),
        files: Some(files.iter().map(PathBuf::from).collect()),
        name: "n".into(),
        total_bytes: 3,
        selected_bytes: 3,
        selected: None,
        sequential: false,
        adopt: false,
        user_paused: true,
        completed: false,
        seeding_done: false,
        added_at: 1,
        finished_at: None,
        progress_bytes: 0,
        error: None,
        peers: Vec::new(),
        created,
    }
}

#[test]
fn removing_a_torrent_deletes_only_files_it_created() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let dir = tmp.path().join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("theirs.bin"), b"another download's verified file").unwrap();
    std::fs::write(dir.join("mine.bin"), b"partial").unwrap();
    let reg = Registry::open(state.join(offline::REGISTRY_FILE)).unwrap();
    let id = "ab".repeat(20);
    reg.insert(
        &id,
        entry(
            &dir,
            &["theirs.bin", "mine.bin"],
            Some(vec![PathBuf::from("mine.bin")]),
        ),
    );
    reg.flush().unwrap();

    offline::remove(&state, &id, true).unwrap();
    assert!(
        dir.join("theirs.bin").exists(),
        "not created by the torrent"
    );
    assert!(!dir.join("mine.bin").exists(), "created by the torrent");
}

#[test]
fn retain_drops_entries_without_a_row_and_their_unfinished_data() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let dir = tmp.path().join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ghost.bin"), b"partial").unwrap();
    std::fs::write(dir.join("kept.bin"), b"partial").unwrap();
    std::fs::write(dir.join("done.bin"), b"finished").unwrap();
    let reg = Registry::open(state.join(offline::REGISTRY_FILE)).unwrap();
    let (ghost, kept, done) = ("aa".repeat(20), "bb".repeat(20), "cc".repeat(20));
    reg.insert(
        &ghost,
        entry(&dir, &["ghost.bin"], Some(vec!["ghost.bin".into()])),
    );
    reg.insert(
        &kept,
        entry(&dir, &["kept.bin"], Some(vec!["kept.bin".into()])),
    );
    let mut finished = entry(&dir, &["done.bin"], Some(vec!["done.bin".into()]));
    finished.completed = true;
    reg.insert(&done, finished);
    reg.flush().unwrap();

    let keep = std::collections::HashSet::from([kept.clone()]);
    let mut dropped = offline::retain(&state, &keep).unwrap();
    dropped.sort();
    assert_eq!(dropped, vec![ghost.clone(), done.clone()]);
    assert!(!dir.join("ghost.bin").exists());
    assert!(dir.join("kept.bin").exists());
    assert!(dir.join("done.bin").exists(), "finished data stays on disk");
    let left: Vec<String> = offline::list(&state)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(left, vec![kept]);
}

fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_folder_is_created_through_a_link_before_the_escape_is_detected() {
    let files = vec![
        TFile {
            path: vec!["sub".into(), "deep".into(), "a.bin".into()],
            data: bytes(1, 40_000),
        },
        TFile {
            path: vec!["b.bin".into()],
            data: bytes(2, 40_000),
        },
    ];
    let torrent = build_torrent("pack", &files, PIECE);
    let tmp = tempfile::tempdir().unwrap();
    let chosen = tmp.path().join("chosen");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&chosen).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    if !make_dir_link(&chosen.join("pack"), &outside) {
        eprintln!("cannot create a directory link here; skipping");
        return;
    }
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state"), &chosen))
        .await
        .unwrap();
    let r = eng
        .add(
            TorrentSource::Bytes(torrent),
            AddOptions {
                dir: Some(chosen.clone()),
                ..Default::default()
            },
        )
        .await;
    assert!(r.is_err(), "the link must be refused: {r:?}");
    assert!(
        std::fs::read_dir(&outside).unwrap().next().is_none(),
        "a folder was created outside through the link"
    );
    eng.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plan_handed_to_add_is_used_as_it_is() {
    let (_files, torrent) = one("movie.mkv", 16 * 1024 * 2, 5);
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("out");
    let state = tmp.path().join("state");
    let eng = TorrentEngine::start(loopback_settings(&state, &dir))
        .await
        .unwrap();
    let id = eng
        .add(
            TorrentSource::Bytes(torrent),
            AddOptions {
                dir: Some(dir.clone()),
                start_paused: true,
                layout: Some(vec![PathBuf::from("planned earlier.bin")]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    eng.shutdown().await;
    let reg = Registry::open(state.join(offline::REGISTRY_FILE)).unwrap();
    assert_eq!(
        reg.get(&id).unwrap().files.unwrap(),
        vec![PathBuf::from("planned earlier.bin")],
        "add planned again"
    );
    // a plan that does not fit is refused, and so is one that leaves the folder
    let (_f2, t2) = one("other.bin", 16 * 1024, 6);
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state2"), &dir))
        .await
        .unwrap();
    for bad in [vec![], vec![PathBuf::from("../escape.bin")]] {
        let r = eng
            .add(
                TorrentSource::Bytes(t2.clone()),
                AddOptions {
                    dir: Some(dir.clone()),
                    start_paused: true,
                    layout: Some(bad),
                    ..Default::default()
                },
            )
            .await;
        assert!(r.is_err());
    }
    eng.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn files_created_before_a_failed_start_do_not_stay_behind() {
    let files = vec![
        TFile {
            path: vec!["a.bin".into()],
            data: bytes(1, 40_000),
        },
        TFile {
            path: vec!["sub".into(), "b.bin".into()],
            data: bytes(2, 40_000),
        },
    ];
    let torrent = build_torrent("pack", &files, PIECE);
    let tmp = tempfile::tempdir().unwrap();
    let chosen = tmp.path().join("chosen");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(chosen.join("pack")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    // the second file's folder leads out of the download folder
    if !make_dir_link(&chosen.join("pack").join("sub"), &outside) {
        eprintln!("cannot create a directory link here; skipping");
        return;
    }
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state"), &chosen))
        .await
        .unwrap();
    let r = eng
        .add(
            TorrentSource::Bytes(torrent),
            AddOptions {
                dir: Some(chosen.clone()),
                ..Default::default()
            },
        )
        .await;
    assert!(r.is_err(), "{r:?}");
    assert!(
        !chosen.join("pack").join("a.bin").exists(),
        "the first file was left behind by the failed add"
    );
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    eng.shutdown().await;
}
