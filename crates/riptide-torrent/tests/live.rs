//! Live smoke test against the public BitTorrent network. Needs internet access, so it is
//! `#[ignore]`d and never runs in CI:
//!
//! ```text
//! cargo test -p riptide-torrent --test live -- --ignored --nocapture
//! ```
//!
//! It uses the Blender Foundation's *Big Buck Bunny* (Creative Commons Attribution 3.0), the
//! well-known WebTorrent demo torrent, and downloads only its small files (subtitles, poster),
//! never the 270 MB movie.

#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use common::{loopback_settings, sha256_file};
use riptide_torrent::{AddOptions, TorrentEngine, TorrentSource, TorrentState};

const MAGNET: &str = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c\
&dn=Big+Buck+Bunny\
&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce\
&tr=udp%3A%2F%2Fopen.tracker.cl%3A1337%2Fannounce\
&tr=udp%3A%2F%2Fexplodie.org%3A6969\
&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A6969%2Fannounce\
&tr=https%3A%2F%2Ftracker.gbitt.info%3A443%2Fannounce";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs internet access and public peers"]
async fn big_buck_bunny_small_files_over_dht_and_trackers() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("downloads");
    // real network: DHT, trackers and LAN discovery on, default listen address
    let mut settings = loopback_settings(&tmp.path().join("state"), &dir);
    settings.dht = true;
    settings.trackers = true;
    settings.local_discovery = true;
    settings.listen_ip = std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);
    let eng = TorrentEngine::start(settings).await.unwrap();

    let t0 = Instant::now();
    let opts = AddOptions {
        dir: Some(dir.clone()),
        ..Default::default()
    };
    let preview = match eng
        .preview(&TorrentSource::Magnet(MAGNET.into()), &opts)
        .await
    {
        Ok(p) => p,
        Err(e) => panic!("LIVE RESULT: no metadata after {:?}: {e}", t0.elapsed()),
    };
    println!(
        "LIVE: metadata after {:?}: {:?}, {} files, {} bytes",
        t0.elapsed(),
        preview.name,
        preview.files.len(),
        preview.total_bytes
    );
    for f in &preview.files {
        println!(
            "LIVE:   [{}] {} ({} bytes)",
            f.index,
            f.path.display(),
            f.len
        );
    }
    let small: Vec<usize> = preview
        .files
        .iter()
        .filter(|f| f.len < 5 * 1024 * 1024 && !f.padding)
        .map(|f| f.index)
        .collect();
    assert!(!small.is_empty(), "no small file to download");

    let id = eng
        .add(
            TorrentSource::Bytes(preview.torrent_bytes.clone()),
            AddOptions {
                only_files: Some(small.clone()),
                ..opts
            },
        )
        .await
        .unwrap();
    let t1 = Instant::now();
    let mut max_peers = 0;
    loop {
        let s = eng.snapshot(&id).unwrap();
        max_peers = max_peers.max(s.peers_connected);
        if t1.elapsed().as_secs() % 5 == 0 {
            println!(
                "LIVE: {:?} {}/{} bytes, {} peers connected ({} seen), {} B/s",
                s.state,
                s.downloaded_bytes,
                s.total_bytes,
                s.peers_connected,
                s.peers_seen,
                s.down_bps
            );
        }
        if matches!(s.state, TorrentState::Completed) {
            break;
        }
        assert!(
            t1.elapsed() < Duration::from_secs(240),
            "LIVE RESULT: not finished after 240 s; max peers connected {max_peers}; {s:?}"
        );
        tokio::time::sleep(Duration::from_millis(900)).await;
    }
    println!(
        "LIVE RESULT: completed in {:?}, max peers connected {max_peers}",
        t1.elapsed()
    );
    for i in small {
        let p = dir.join(&preview.files[i].path);
        println!("LIVE: sha256 {} {}", sha256_file(&p), p.display());
    }
    eng.shutdown().await;
}
