//! Files that were not selected must not stay behind as full-size empty files that look like real
//! downloads (the bytes of a boundary piece shared with a selected file used to make the whole file
//! appear, sparse and the size of the original). Each test fails if the cleanup is removed.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use riptide_torrent::{AddOptions, TorrentEngine, TorrentSource, TorrentState};

fn f(name: &str, len: usize, seed: u64) -> TFile {
    TFile {
        path: vec![name.to_string()],
        data: bytes(seed, len),
    }
}

/// Three files whose sizes do not line up with the pieces, so the neighbours of the middle file
/// share boundary pieces with it.
fn set() -> (Vec<TFile>, Vec<u8>) {
    let files = vec![
        f("first.bin", 20_000, 1),
        f("middle.bin", 50_000, 2),
        f("last.bin", 20_000, 3),
    ];
    let torrent = build_torrent("trio", &files, PIECE);
    (files, torrent)
}

async fn start(tmp: &std::path::Path) -> Arc<TorrentEngine> {
    TorrentEngine::start(loopback_settings(&tmp.join("state"), &tmp.join("out")))
        .await
        .unwrap()
}

fn path(tmp: &std::path::Path, name: &str) -> PathBuf {
    tmp.join("out").join("trio").join(name)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unselected_files_do_not_stay_behind_and_selecting_them_later_completes_byte_exact() {
    let (files, torrent) = set();
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;
    let tmp = tempfile::tempdir().unwrap();
    let eng = start(tmp.path()).await;
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(tmp.path().join("out")),
                only_files: Some(vec![1]),
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
    assert_eq!(
        sha256_file(&path(tmp.path(), "middle.bin")),
        sha256(&files[1].data)
    );
    eng.shutdown().await;

    // the neighbours only ever held boundary bytes: they are gone, not full-size empty files
    for name in ["first.bin", "last.bin"] {
        assert!(
            !path(tmp.path(), name).exists(),
            "{name} was left behind as a placeholder"
        );
    }
    assert!(path(tmp.path(), "middle.bin").exists());

    // selecting them later recreates them properly and completes byte-exact
    let eng = start(tmp.path()).await;
    eng.select_files(&id, &[0, 1, 2]).await.unwrap();
    wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed) && s.file_progress == [20_000, 50_000, 20_000]
    })
    .await;
    for (name, file) in [
        ("first.bin", &files[0]),
        ("middle.bin", &files[1]),
        ("last.bin", &files[2]),
    ] {
        assert_eq!(
            sha256_file(&path(tmp.path(), name)),
            sha256(&file.data),
            "{name}"
        );
    }
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_that_was_fully_downloaded_and_then_deselected_is_kept() {
    let (files, torrent) = set();
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;
    let tmp = tempfile::tempdir().unwrap();
    let eng = start(tmp.path()).await;
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(tmp.path().join("out")),
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

    // the user narrows the selection to the middle file: what was downloaded stays
    let eng = start(tmp.path()).await;
    wait_state(&eng, &id, 30, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;
    eng.select_files(&id, &[1]).await.unwrap();
    wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed) && s.file_progress == [20_000, 50_000, 20_000]
    })
    .await;
    eng.shutdown().await;
    for (name, file) in [("first.bin", &files[0]), ("last.bin", &files[2])] {
        assert_eq!(
            sha256_file(&path(tmp.path(), name)),
            sha256(&file.data),
            "{name} held real data and was deleted"
        );
    }
    seeder.engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn files_of_another_torrent_or_the_user_are_never_removed_as_placeholders() {
    let (files, torrent) = set();
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    // a user file where the unselected file would go: the planner renames around it
    std::fs::create_dir_all(out.join("trio")).unwrap();
    std::fs::write(out.join("trio").join("first.bin"), b"my own notes").unwrap();
    let eng = start(tmp.path()).await;
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(out.clone()),
                only_files: Some(vec![1]),
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
    assert_eq!(
        std::fs::read(out.join("trio").join("first.bin")).unwrap(),
        b"my own notes"
    );
    seeder.engine.shutdown().await;
}

#[test]
fn only_created_unselected_files_with_at_most_boundary_data_are_placeholders() {
    use riptide_torrent::registry::Entry;
    let mk = |progress: Vec<u64>, selected: Option<Vec<usize>>, created: Vec<&str>| Entry {
        dir: PathBuf::from("out"),
        files: Some(vec!["a".into(), "b".into(), "c".into()]),
        name: "n".into(),
        total_bytes: 0,
        selected_bytes: 0,
        selected,
        sequential: false,
        adopt: false,
        user_paused: false,
        completed: true,
        seeding_done: true,
        added_at: 0,
        finished_at: None,
        progress_bytes: 0,
        error: None,
        peers: Vec::new(),
        created: Some(created.into_iter().map(PathBuf::from).collect()),
        piece_length: Some(100),
        file_lens: Some(vec![1000, 1000, 1000]),
        file_progress: Some(progress),
    };
    let all = ["a", "b", "c"];
    let list = |e: &Entry| riptide_torrent::storage::placeholders(e);
    // boundary bytes only: a and c go, b is selected
    assert_eq!(
        list(&mk(vec![30, 900, 150], Some(vec![1]), all.to_vec())),
        vec![PathBuf::from("a"), PathBuf::from("c")]
    );
    // a complete file stays, even a small one; real data (more than two pieces) stays; so does
    // everything when all is wanted
    assert_eq!(
        list(&mk(vec![1000, 900, 150], Some(vec![1]), all.to_vec())),
        vec![PathBuf::from("c")]
    );
    assert!(list(&mk(vec![900, 900, 900], Some(vec![1]), all.to_vec())).is_empty());
    assert!(list(&mk(vec![0, 0, 0], None, all.to_vec())).is_empty());
    // not created by this torrent: never listed
    assert_eq!(
        list(&mk(vec![0, 900, 0], Some(vec![1]), vec!["b", "c"])),
        vec![PathBuf::from("c")]
    );
    // progress unknown or of the wrong length: nothing
    let mut e = mk(vec![0, 0, 0], Some(vec![1]), all.to_vec());
    e.file_progress = None;
    assert!(list(&e).is_empty());
    e.file_progress = Some(vec![0]);
    assert!(list(&e).is_empty());
}
