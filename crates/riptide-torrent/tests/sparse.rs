//! Regression test for a defect found with a real torrent: choosing only a tiny file of a torrent
//! whose big file ends in the same piece wrote the neighbour's bytes at the end of the big file,
//! and Windows reserved the whole 263 MB for it. Files we create are now sparse, so the disk use
//! of a file nobody asked for stays near what was really written. Windows only (other platforms
//! have sparse files by default).
#![cfg(windows)]
#![allow(clippy::unwrap_used, unsafe_code, missing_docs)]

mod common;

use common::*;
use riptide_torrent::{AddOptions, TorrentEngine, TorrentSource, TorrentState};

/// Bytes a file really occupies on disk.
fn size_on_disk(path: &std::path::Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::GetCompressedFileSizeW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut high: u32 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; `high` is a live u32.
    let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
    (u64::from(high) << 32) | u64::from(low)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_nobody_asked_for_does_not_reserve_its_whole_length() {
    // the big file's last piece is shared with the small one that follows it
    let big_len = 8 * 1024 * 1024 + 5000;
    let files = vec![
        TFile {
            path: vec!["big.bin".into()],
            data: bytes(41, big_len),
        },
        TFile {
            path: vec!["small.txt".into()],
            data: bytes(42, 3000),
        },
    ];
    let torrent = build_torrent("shared piece", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("downloads");
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state"), &dir))
        .await
        .unwrap();
    let id = eng
        .add(
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(dir.clone()),
                peers: vec![seeder.addr],
                only_files: Some(vec![1]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    wait_state(&eng, &id, 90, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;

    let small = dir.join("shared piece").join("small.txt");
    assert_eq!(sha256_file(&small), sha256(&files[1].data));

    // the piece shared with the small file put a few KB at the end of the big file; the file may
    // exist at its full logical length but must not occupy it
    let big = dir.join("shared piece").join("big.bin");
    if big.exists() {
        let used = size_on_disk(&big);
        assert!(
            used < 1024 * 1024,
            "big.bin reserves {used} bytes on disk for a file that was not chosen"
        );
    }
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}
