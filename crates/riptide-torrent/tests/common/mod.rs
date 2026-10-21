//! Shared helpers: a tiny `.torrent` builder, deterministic data and engines wired for a closed
//! loopback swarm (no DHT, no trackers, no LAN discovery, no internet).
#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use riptide_torrent::{
    AddOptions, SeedingPolicy, TorrentEngine, TorrentPreview, TorrentSettings, TorrentSnapshot,
    TorrentSource, TorrentState,
};
use sha2::{Digest, Sha256};

/// Minimal bencode value.
pub enum B {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<B>),
    Dict(BTreeMap<Vec<u8>, B>),
}

impl B {
    pub fn s(v: &str) -> B {
        B::Bytes(v.as_bytes().to_vec())
    }
    pub fn dict(items: Vec<(&str, B)>) -> B {
        B::Dict(
            items
                .into_iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v))
                .collect(),
        )
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            B::Int(i) => out.extend_from_slice(format!("i{i}e").as_bytes()),
            B::Bytes(b) => {
                out.extend_from_slice(format!("{}:", b.len()).as_bytes());
                out.extend_from_slice(b);
            }
            B::List(l) => {
                out.push(b'l');
                l.iter().for_each(|v| v.write(out));
                out.push(b'e');
            }
            B::Dict(d) => {
                out.push(b'd');
                for (k, v) in d {
                    B::Bytes(k.clone()).write(out);
                    v.write(out);
                }
                out.push(b'e');
            }
        }
    }
}

/// A file inside a test torrent.
#[derive(Clone)]
pub struct TFile {
    pub path: Vec<String>,
    pub data: Vec<u8>,
}

/// Deterministic pseudo-random bytes.
pub fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

pub fn sha256(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_file(p: &Path) -> String {
    sha256(&std::fs::read(p).unwrap())
}

pub fn piece_hashes(files: &[TFile], piece_len: usize) -> Vec<u8> {
    use sha1::{Digest as _, Sha1};
    let all: Vec<u8> = files.iter().flat_map(|f| f.data.iter().copied()).collect();
    all.chunks(piece_len)
        .flat_map(|c| Sha1::digest(c).to_vec())
        .collect()
}

/// Builds the bytes of a `.torrent`. One file with a one-component path becomes a single-file
/// torrent; anything else is multi-file with `name` as folder.
pub fn build_torrent(name: &str, files: &[TFile], piece_len: u32) -> Vec<u8> {
    build_torrent_with(name, files, piece_len, Vec::new())
}

/// Like [`build_torrent`] with extra top-level keys (`url-list`, ...). Do not use the result with
/// [`info_hash_of`]: it assumes `info` is the last key.
pub fn build_torrent_with(
    name: &str,
    files: &[TFile],
    piece_len: u32,
    extra: Vec<(&str, B)>,
) -> Vec<u8> {
    let pieces = piece_hashes(files, piece_len as usize);
    let mut info = vec![
        ("name", B::s(name)),
        ("piece length", B::Int(i64::from(piece_len))),
        ("pieces", B::Bytes(pieces)),
    ];
    if files.len() == 1 && files[0].path.len() == 1 {
        info.push(("length", B::Int(files[0].data.len() as i64)));
    } else {
        info.push((
            "files",
            B::List(
                files
                    .iter()
                    .map(|f| {
                        B::dict(vec![
                            ("length", B::Int(f.data.len() as i64)),
                            ("path", B::List(f.path.iter().map(|p| B::s(p)).collect())),
                        ])
                    })
                    .collect(),
            ),
        ));
    }
    let mut top = vec![("info", B::dict(info))];
    top.extend(extra);
    B::dict(top).encode()
}

pub fn info_hash_of(torrent: &[u8]) -> String {
    use sha1::{Digest as _, Sha1};
    let start = torrent.windows(6).position(|w| w == b"4:info").unwrap() + 6;
    // the info dict runs to the final 'e' that closes the outer dict
    let info = &torrent[start..torrent.len() - 1];
    hex(&Sha1::digest(info))
}

/// Settings for a closed loopback swarm.
pub fn loopback_settings(state: &Path, downloads: &Path) -> TorrentSettings {
    let mut s = TorrentSettings::new(state.to_path_buf(), downloads.to_path_buf());
    s.dht = false;
    s.trackers = false;
    s.local_discovery = false;
    s.listen_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    s
}

/// A seeding engine for `torrent`, holding `data` (already laid out at the sanitised paths).
pub struct Seeder {
    pub engine: Arc<TorrentEngine>,
    pub addr: SocketAddr,
    pub id: String,
    pub dir: PathBuf,
    pub preview: TorrentPreview,
    _tmp: tempfile::TempDir,
}

pub async fn start_seeder(
    torrent: &[u8],
    files: &[TFile],
    policy: SeedingPolicy,
    upload_bps: Option<u32>,
    tweak: impl Fn(&mut Vec<u8>, usize),
) -> Seeder {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    let state = tmp.path().join("state");
    let mut s = loopback_settings(&state, &dir);
    s.seeding = policy;
    s.upload_limit_bps = upload_bps;
    let engine = TorrentEngine::start(s).await.unwrap();
    let opts = AddOptions {
        dir: Some(dir.clone()),
        adopt_existing: true,
        ..Default::default()
    };
    let preview = engine
        .preview(&TorrentSource::Bytes(torrent.to_vec()), &opts)
        .await
        .unwrap();
    for (f, entry) in files.iter().zip(&preview.files) {
        let p = dir.join(&entry.path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut data = f.data.clone();
        tweak(&mut data, entry.index);
        std::fs::write(p, data).unwrap();
    }
    let id = engine
        .add(TorrentSource::Bytes(torrent.to_vec()), opts)
        .await
        .unwrap();
    let addr = engine.listen_addr().expect("seeder listens");
    Seeder {
        engine,
        addr,
        id,
        dir,
        preview,
        _tmp: tmp,
    }
}

impl Seeder {
    pub async fn wait_ready(&self) {
        wait_state(&self.engine, &self.id, 60, |s| {
            matches!(s.state, TorrentState::Seeding)
        })
        .await;
    }
}

pub fn unlimited_seeding() -> SeedingPolicy {
    SeedingPolicy {
        enabled: true,
        max_ratio: None,
        max_minutes: None,
    }
}

/// Polls until `pred` holds (panics after `secs`).
pub async fn wait_state(
    engine: &TorrentEngine,
    id: &str,
    secs: u64,
    pred: impl Fn(&TorrentSnapshot) -> bool,
) -> TorrentSnapshot {
    let t0 = Instant::now();
    loop {
        if let Ok(s) = engine.snapshot(id) {
            if pred(&s) {
                return s;
            }
            if t0.elapsed() > Duration::from_secs(secs) {
                panic!("timed out after {secs}s; last snapshot: {s:#?}");
            }
        } else if t0.elapsed() > Duration::from_secs(secs) {
            panic!("timed out after {secs}s; torrent not visible");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Sample multi-file content: three files, sizes that are whole pieces of 16 KiB.
pub fn sample_files() -> Vec<TFile> {
    vec![
        TFile {
            path: vec!["alpha.bin".into()],
            data: bytes(1, 16 * 1024 * 6),
        },
        TFile {
            path: vec!["sub".into(), "beta.bin".into()],
            data: bytes(2, 16 * 1024 * 5),
        },
        TFile {
            path: vec!["sub".into(), "deeper".into(), "gamma.bin".into()],
            data: bytes(3, 16 * 1024 * 4),
        },
    ]
}

pub const PIECE: u32 = 16 * 1024;

/// All regular files below `dir`, relative, sorted.
pub fn list_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(base, &p, out);
                } else {
                    out.push(p.strip_prefix(base).unwrap().to_path_buf());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}
