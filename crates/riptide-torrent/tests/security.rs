//! Hostile `.torrent` files and magnet links. Every test here fails if the sanitising storage or
//! the pre-validation is removed.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use riptide_torrent::{
    inspect_torrent, AddOptions, Error, TorrentEngine, TorrentSource, TorrentState,
};

fn f(path: &[&str], len: usize, seed: u64) -> TFile {
    TFile {
        path: path.iter().map(|s| s.to_string()).collect(),
        data: bytes(seed, len),
    }
}

fn hostile_files() -> Vec<TFile> {
    let long = format!("{}.mkv", "ب".repeat(300));
    vec![
        f(&["CON"], 20_000, 1),
        f(&["nul.txt"], 20_000, 2),
        f(&["dir. ", "file."], 20_000, 3),
        f(&["C:", "boot.ini"], 20_000, 4),
        f(&["photo\u{202E}gpj.exe"], 20_000, 5),
        f(&["a.txt"], 20_000, 6),
        f(&["A.TXT"], 20_000, 7),
        f(&["a.txt."], 20_000, 8),
        f(&[&long], 20_000, 9),
        f(&["ctl\u{1}\u{7}name"], 20_000, 10),
        f(&["LPT1", "aux", "x"], 20_000, 11),
    ]
}

#[test]
fn the_stock_library_would_accept_these_names_so_our_storage_must_not() {
    // control: librqbit's own validation lets the hostile names through unchanged
    let files = hostile_files();
    let torrent = build_torrent("con. ", &files, PIECE);
    let parsed = librqbit::torrent_from_bytes(&torrent).unwrap();
    let info = parsed.info.data.validate().expect("upstream accepts it");
    let stock: Vec<String> = info
        .iter_file_details()
        .map(|d| d.filename.to_pathbuf().to_string_lossy().into_owned())
        .collect();
    assert!(stock.iter().any(|p| p == "CON"), "{stock:?}");
    assert!(stock.iter().any(|p| p.contains('\u{202E}')), "{stock:?}");
    assert!(stock.iter().any(|p| p.ends_with("file.")), "{stock:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_names_are_sanitised_and_nothing_lands_outside_the_chosen_folder() {
    let files = hostile_files();
    let torrent = build_torrent("con. ", &files, PIECE);

    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().join("parent");
    let chosen = parent.join("chosen");
    std::fs::create_dir_all(&chosen).unwrap();
    let eng = TorrentEngine::start(loopback_settings(&tmp.path().join("state"), &chosen))
        .await
        .unwrap();
    let opts = AddOptions {
        dir: Some(chosen.clone()),
        peers: vec![seeder.addr],
        ..Default::default()
    };
    let preview = eng
        .preview(&TorrentSource::Bytes(torrent.clone()), &opts)
        .await
        .unwrap();
    // what the user is shown is what gets written
    for e in &preview.files {
        for c in e.path.components() {
            let s = c.as_os_str().to_string_lossy();
            assert!(!s.ends_with('.') && !s.ends_with(' '), "{s}");
            assert!(!s.contains('\u{202E}') && !s.contains(':'), "{s}");
            assert!(s.len() <= 200);
        }
    }
    let paths: Vec<String> = preview
        .files
        .iter()
        .map(|e| e.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(paths.contains(&"_con/_CON".to_string()), "{paths:?}");
    assert!(paths.contains(&"_con/_nul.txt".to_string()), "{paths:?}");
    assert!(paths.contains(&"_con/dir/file".to_string()), "{paths:?}");
    assert!(paths.contains(&"_con/C/boot.ini".to_string()), "{paths:?}");
    assert!(
        paths.contains(&"_con/photogpj.exe".to_string()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"_con/_LPT1/aux/x".to_string())
            || paths.contains(&"_con/_LPT1/_aux/x".to_string()),
        "{paths:?}"
    );
    let lower: std::collections::HashSet<_> = paths.iter().map(|p| p.to_lowercase()).collect();
    assert_eq!(lower.len(), paths.len(), "names collide: {paths:?}");

    let id = eng
        .add(TorrentSource::Bytes(preview.torrent_bytes.clone()), opts)
        .await
        .unwrap();
    wait_state(&eng, &id, 60, |s| {
        matches!(s.state, TorrentState::Completed)
    })
    .await;

    // every byte is where the preview said, under the chosen folder, and correct
    for (src, e) in files.iter().zip(&preview.files) {
        assert_eq!(
            sha256_file(&chosen.join(&e.path)),
            sha256(&src.data),
            "{:?}",
            e.path
        );
    }
    // the parent of the chosen folder holds nothing but it
    let siblings: Vec<_> = std::fs::read_dir(&parent).unwrap().flatten().collect();
    assert_eq!(siblings.len(), 1);
    let written = list_files(&chosen);
    assert_eq!(written.len(), files.len(), "{written:?}");

    eng.shutdown().await;
    seeder.engine.shutdown().await;
}

#[test]
fn path_traversal_in_a_torrent_is_rejected() {
    let files = vec![
        f(&["..", "..", "evil.txt"], 20_000, 1),
        f(&["ok.txt"], 20_000, 2),
    ];
    let torrent = build_torrent("t", &files, PIECE);
    // rejected before anything exists on disk
    assert!(inspect_torrent(&torrent, &|_| false).is_err());
}

#[test]
fn absurd_torrents_are_refused_without_panicking() {
    let piece = vec![0u8; 20];
    let mk = |info: Vec<(&str, B)>| B::dict(vec![("info", B::dict(info))]).encode();
    let file = |len: i64| {
        B::dict(vec![
            ("length", B::Int(len)),
            ("path", B::List(vec![B::s("f")])),
        ])
    };
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();

    cases.push((
        "piece length 0",
        mk(vec![
            ("name", B::s("x")),
            ("length", B::Int(1000)),
            ("piece length", B::Int(0)),
            ("pieces", B::Bytes(piece.clone())),
        ]),
    ));
    cases.push((
        "piece length 2^31",
        mk(vec![
            ("name", B::s("x")),
            ("length", B::Int(1000)),
            ("piece length", B::Int(1 << 31)),
            ("pieces", B::Bytes(piece.clone())),
        ]),
    ));
    cases.push((
        "ten million one-byte pieces",
        mk(vec![
            ("name", B::s("x")),
            ("length", B::Int(10_000_000)),
            ("piece length", B::Int(1)),
            ("pieces", B::Bytes(piece.clone())),
        ]),
    ));
    cases.push((
        "piece count above the limit",
        mk(vec![
            ("name", B::s("x")),
            ("length", B::Int(1 << 40)),
            ("piece length", B::Int(16 * 1024)),
            ("pieces", B::Bytes(piece.clone())),
        ]),
    ));
    cases.push((
        "total size overflow",
        mk(vec![
            ("name", B::s("x")),
            ("piece length", B::Int(16 * 1024)),
            ("pieces", B::Bytes(piece.clone())),
            (
                "files",
                B::List(vec![file(i64::MAX), file(i64::MAX), file(i64::MAX)]),
            ),
        ]),
    ));
    cases.push((
        "sixty thousand files",
        mk(vec![
            ("name", B::s("x")),
            ("piece length", B::Int(16 * 1024)),
            ("pieces", B::Bytes(vec![0u8; 20 * 60_000])),
            ("files", B::List((0..60_000).map(|_| file(1)).collect())),
        ]),
    ));
    cases.push((
        "hash list does not match the size",
        mk(vec![
            ("name", B::s("x")),
            ("length", B::Int(1 << 20)),
            ("piece length", B::Int(16 * 1024)),
            ("pieces", B::Bytes(piece.clone())),
        ]),
    ));
    cases.push((
        "neither length nor files",
        mk(vec![
            ("name", B::s("x")),
            ("piece length", B::Int(16 * 1024)),
            ("pieces", B::Bytes(piece.clone())),
        ]),
    ));
    cases.push((
        "zero-length data",
        mk(vec![
            ("name", B::s("x")),
            ("length", B::Int(0)),
            ("piece length", B::Int(16 * 1024)),
            ("pieces", B::Bytes(vec![])),
        ]),
    ));
    cases.push((
        "100k nested lists",
        [vec![b'l'; 100_000], vec![b'e'; 100_000]].concat(),
    ));
    cases.push(("garbage", b"not a torrent at all".to_vec()));
    cases.push(("empty", Vec::new()));
    cases.push(("oversized", vec![b'0'; 9 * 1024 * 1024]));

    for (name, bytes) in cases {
        match inspect_torrent(&bytes, &|_| false) {
            Err(Error::InvalidTorrent(msg)) => eprintln!("rejected {name}: {msg}"),
            Err(e) => eprintln!("rejected {name}: {e}"),
            Ok(p) => panic!("{name} was accepted: {p:?}"),
        }
    }
}

#[test]
fn a_big_but_legitimate_torrent_is_accepted() {
    // 10 000 files, 5 GiB, 1 MiB pieces: nothing absurd about it
    let n = 10_000;
    let file = |i: usize| {
        B::dict(vec![
            ("length", B::Int(512 * 1024)),
            ("path", B::List(vec![B::s("d"), B::s(&format!("f{i}.bin"))])),
        ])
    };
    let total: u64 = 512 * 1024 * n as u64;
    let pieces = total.div_ceil(1 << 20) as usize;
    let t = B::dict(vec![(
        "info",
        B::dict(vec![
            ("name", B::s("big")),
            ("piece length", B::Int(1 << 20)),
            ("pieces", B::Bytes(vec![7u8; 20 * pieces])),
            ("files", B::List((0..n).map(file).collect())),
        ]),
    )])
    .encode();
    let p = inspect_torrent(&t, &|_| false).unwrap();
    assert_eq!(p.files.len(), n);
    assert_eq!(p.total_bytes, total);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostile_magnet_links_are_refused_before_the_network_is_touched() {
    let tmp = tempfile::tempdir().unwrap();
    let eng = TorrentEngine::start(loopback_settings(
        &tmp.path().join("s"),
        &tmp.path().join("d"),
    ))
    .await
    .unwrap();
    for bad in [
        "magnet:?xt=urn:btih:nothex",
        "magnet:?dn=only-a-name",
        "http://example.com/a.torrent",
        "file:///etc/passwd",
        "magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335aa7c1367a88a&xt=urn:btih:0000000000000000000000000000000000000000",
    ] {
        let r = eng
            .add(TorrentSource::Magnet(bad.to_string()), AddOptions::default())
            .await;
        assert!(matches!(r, Err(Error::InvalidMagnet(_))), "{bad}: {r:?}");
    }
    assert!(eng.list().is_empty());
    eng.shutdown().await;
}

/// Makes `link` point at `target` (a junction on Windows, a symlink elsewhere).
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
async fn a_link_inside_the_download_folder_cannot_redirect_writes_outside() {
    let files = vec![f(&["a.bin"], 40_000, 1), f(&["b.bin"], 40_000, 2)];
    let torrent = build_torrent("pack", &files, PIECE);
    let seeder = start_seeder(&torrent, &files, unlimited_seeding(), None, |_, _| {}).await;
    seeder.wait_ready().await;

    let tmp = tempfile::tempdir().unwrap();
    let chosen = tmp.path().join("chosen");
    let outside: PathBuf = tmp.path().join("outside");
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
            TorrentSource::Bytes(torrent.clone()),
            AddOptions {
                dir: Some(chosen.clone()),
                peers: vec![seeder.addr],
                ..Default::default()
            },
        )
        .await;
    assert!(
        r.is_err(),
        "the write through the link must be refused: {r:?}"
    );
    eprintln!("link refused: {r:?}");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        std::fs::read_dir(&outside).unwrap().next().is_none(),
        "something was written through the link"
    );
    eng.shutdown().await;
    seeder.engine.shutdown().await;
}
