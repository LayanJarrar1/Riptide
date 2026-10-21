//! Real `.torrent` files published by Debian and the Internet Archive (`tests/fixtures/real/`,
//! downloaded 2026-10-20): parsed offline, no network. Also what happens to damaged copies.
#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;

use riptide_torrent::{inspect_torrent, inspect_torrent_file, Error};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/real")
        .join(name)
}

#[test]
fn debian_netinst_is_one_iso_with_a_piece_per_256k() {
    let p = inspect_torrent_file(&fixture("debian-13.7.0-amd64-netinst.iso.torrent"), &|_| {
        false
    })
    .unwrap();
    println!(
        "debian: {} files={} total={} piece={} pieces={}",
        p.name,
        p.files.len(),
        p.total_bytes,
        p.piece_length,
        p.piece_count
    );
    assert_eq!(p.name, "debian-13.7.0-amd64-netinst.iso");
    assert_eq!(p.files.len(), 1);
    assert_eq!(p.info_hash.len(), 40);
    assert_eq!(p.total_bytes, p.files[0].len);
    assert_eq!(
        p.piece_count,
        p.total_bytes.div_ceil(u64::from(p.piece_length))
    );
    assert!(!p.private);
}

#[test]
fn internet_archive_item_torrents_list_every_file_with_sane_names() {
    for name in [
        "ia-bigbuckbunny_mpd_demo_001_archive.torrent",
        "ia-blender-radio-preview-sintel_archive.torrent",
    ] {
        let p = inspect_torrent_file(&fixture(name), &|_| false).unwrap();
        println!(
            "{name}: {} files={} total={} pieces={}",
            p.name,
            p.files.len(),
            p.total_bytes,
            p.piece_count
        );
        assert!(p.files.len() > 1, "{name} is a multi-file item");
        assert_eq!(p.total_bytes, p.files.iter().map(|f| f.len).sum::<u64>());
        for f in &p.files {
            let path = f.path.to_string_lossy();
            assert!(!path.contains(".."), "{path}");
            assert!(!path.starts_with('/') && !path.contains(':'), "{path}");
            assert!(!path.is_empty());
        }
    }
}

#[test]
fn truncated_and_corrupted_copies_say_what_is_wrong_and_never_panic() {
    let good = std::fs::read(fixture("debian-13.7.0-amd64-netinst.iso.torrent")).unwrap();
    assert!(inspect_torrent(&good, &|_| false).is_ok());
    // cut anywhere: always a clear InvalidTorrent
    for cut in [0, 1, 10, 100, good.len() / 2, good.len() - 1] {
        match inspect_torrent(&good[..cut], &|_| false) {
            Err(Error::InvalidTorrent(m)) => assert!(!m.is_empty()),
            other => panic!("cut at {cut}: {other:?}"),
        }
    }
    // flip bytes at many places: either still valid or InvalidTorrent, never a panic or another error
    for i in (0..good.len()).step_by(997) {
        let mut bad = good.clone();
        bad[i] ^= 0xff;
        match inspect_torrent(&bad, &|_| false) {
            Ok(_) | Err(Error::InvalidTorrent(_)) => {}
            other => panic!("flip at {i}: {other:?}"),
        }
    }
    // not a torrent at all
    for junk in [
        &b"hello"[..],
        &b"<html><body>404</body></html>"[..],
        &[0u8; 64][..],
        &b"d4:infod4:name1:xee"[..],
    ] {
        assert!(
            matches!(
                inspect_torrent(junk, &|_| false),
                Err(Error::InvalidTorrent(_))
            ),
            "{junk:?}"
        );
    }
}

#[test]
fn web_seeds_of_real_torrents_are_listed_and_only_http_ones() {
    let debian = inspect_torrent_file(&fixture("debian-13.7.0-amd64-netinst.iso.torrent"), &|_| {
        false
    })
    .unwrap();
    assert_eq!(
        debian.web_seeds,
        [
            "https://cdimage.debian.org/cdimage/release/13.7.0/amd64/iso-cd/debian-13.7.0-amd64-netinst.iso",
            "https://cdimage.debian.org/cdimage/archive/13.7.0/amd64/iso-cd/debian-13.7.0-amd64-netinst.iso"
        ]
    );
    let ia = inspect_torrent_file(
        &fixture("ia-bigbuckbunny_mpd_demo_001_archive.torrent"),
        &|_| false,
    )
    .unwrap();
    assert_eq!(ia.web_seeds.len(), 3, "{:?}", ia.web_seeds);
    assert_eq!(ia.web_seeds[0], "https://archive.org/download/");
    assert!(ia.web_seeds.iter().all(|u| u.starts_with("http")));
}

#[test]
fn web_seed_parsing_is_defensive() {
    use riptide_torrent::meta::{web_seeds, MAX_WEB_SEEDS};
    // single string, list, hostile schemes, credentials, repeats, over the cap, junk
    assert_eq!(
        web_seeds(b"d8:url-list18:https://a.test/f.be"),
        ["https://a.test/f.b"]
    );
    let list = b"d8:url-listl18:https://a.test/f.b18:https://a.test/f.b14:ftp://x.test/y22:javascript:alert(1)//x23:https://u:p@evil.test/zi5e4:abcdee";
    assert_eq!(web_seeds(list), ["https://a.test/f.b"]);
    let mut many = String::from("d8:url-listl");
    for i in 0..30 {
        let u = format!("http://h{i}.test/x");
        many.push_str(&format!("{}:{u}", u.len()));
    }
    many.push_str("ee");
    assert_eq!(web_seeds(many.as_bytes()).len(), MAX_WEB_SEEDS);
    for junk in [
        &b""[..],
        &b"hello"[..],
        &b"d8:url-list"[..],
        &b"d8:url-listl3:abc"[..],
        &b"d3:fooi1e8:url-listli2eee"[..],
    ] {
        let _ = web_seeds(junk); // must not panic
    }
    assert!(web_seeds(b"d3:fooi1e8:url-listli2eee").is_empty());
    // an unrelated key before the list is skipped properly
    assert_eq!(
        web_seeds(b"d4:infod4:name1:xe8:url-list18:https://a.test/f.be"),
        ["https://a.test/f.b"]
    );
}
