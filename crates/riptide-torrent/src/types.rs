//! Public data types: settings, add options, sources, snapshots and events.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use riptide_core::DownloadState;
use serde::{Deserialize, Serialize};

/// Seeding policy. **Off by default**: a finished torrent stops all network activity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeedingPolicy {
    /// Keep uploading after the download is complete. Only ever true because the user said so.
    pub enabled: bool,
    /// Stop seeding once `uploaded / size` reaches this ratio (`None` = no ratio limit).
    pub max_ratio: Option<f32>,
    /// Stop seeding after this many minutes (`None` = no time limit).
    pub max_minutes: Option<u32>,
}

impl Default for SeedingPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_ratio: Some(1.0),
            max_minutes: Some(24 * 60),
        }
    }
}

/// Engine settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentSettings {
    /// Where Riptide keeps its torrent state (registry + the library's session files).
    pub state_dir: PathBuf,
    /// Download folder used when [`AddOptions::dir`] is not given.
    pub default_dir: PathBuf,
    /// Seeding policy (off by default).
    pub seeding: SeedingPolicy,
    /// Upload bandwidth cap in bytes per second (`None` = unlimited). Applies while
    /// downloading too, because BitTorrent peers trade pieces.
    pub upload_limit_bps: Option<u32>,
    /// Download bandwidth cap in bytes per second (`None` = unlimited).
    pub download_limit_bps: Option<u32>,
    /// Use the mainline DHT (trackerless peer discovery). Ignored for private torrents.
    pub dht: bool,
    /// Address to listen on for incoming peers.
    pub listen_ip: IpAddr,
    /// Port to listen on (0 = pick one).
    pub listen_port: u16,
    /// Ask the router for a port mapping (UPnP). Off by default.
    pub upnp: bool,
    /// Maximum peers per torrent (`None` = library default).
    pub peer_limit: Option<usize>,
    /// Contact trackers (turn off only for offline tests).
    pub trackers: bool,
    /// Local service discovery (multicast on the LAN).
    pub local_discovery: bool,
}

impl TorrentSettings {
    /// Settings with the privacy-first defaults, keeping state under `state_dir`.
    pub fn new(state_dir: PathBuf, default_dir: PathBuf) -> Self {
        Self {
            state_dir,
            default_dir,
            seeding: SeedingPolicy::default(),
            upload_limit_bps: None,
            download_limit_bps: None,
            dht: true,
            listen_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            listen_port: 0,
            upnp: false,
            peer_limit: None,
            trackers: true,
            local_discovery: true,
        }
    }
}

/// What to add.
#[derive(Debug, Clone)]
pub enum TorrentSource {
    /// A `magnet:` link.
    Magnet(String),
    /// The bytes of a `.torrent` file.
    Bytes(Vec<u8>),
    /// A `.torrent` file on disk.
    File(PathBuf),
}

/// Options for [`crate::TorrentEngine::add`].
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    /// Download folder (defaults to [`TorrentSettings::default_dir`]). Every file of the torrent
    /// is written below it, whatever the torrent says.
    pub dir: Option<PathBuf>,
    /// Files to download, by index from [`crate::TorrentPreview::files`]. `None` = all.
    pub only_files: Option<Vec<usize>>,
    /// Download the selected files one after the other, in order (first file first), instead of
    /// all at once. librqbit has no piece-level sequential mode, so this is per file.
    pub sequential: bool,
    /// Add without starting.
    pub start_paused: bool,
    /// Known peers to try first (`ip:port`).
    pub peers: Vec<SocketAddr>,
    /// The folder already holds (part of) this torrent's data: verify and reuse it instead of
    /// choosing fresh names.
    pub adopt_existing: bool,
    /// Absolute paths other downloads (HTTP transfers, their `.rtpart` files) have claimed. The
    /// planner picks other names for any file that would land on one of them, exactly as it does
    /// for a file that already exists on disk.
    pub reserved: Vec<PathBuf>,
    /// A layout planned earlier (relative path of every file, torrent order, padding files
    /// included): used as it is instead of planning again, so what the caller already recorded
    /// and what is written cannot differ.
    pub layout: Option<Vec<PathBuf>>,
}

/// Coarse state of a torrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TorrentState {
    /// Checking pieces already on disk.
    Checking,
    /// Waiting for metadata / peers or transferring.
    Downloading,
    /// All selected pieces verified; uploading by user choice.
    Seeding,
    /// Stopped by the user.
    Paused,
    /// All selected pieces verified; network activity stopped.
    Completed,
    /// Stopped by an error.
    Failed {
        /// Human-readable cause.
        error: String,
    },
}

/// A point-in-time view of one torrent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TorrentSnapshot {
    /// Info-hash (40 hex digits): the id of the torrent.
    pub id: String,
    /// Display name.
    pub name: String,
    /// State.
    pub state: TorrentState,
    /// Folder the files are written to.
    pub dir: PathBuf,
    /// Size of the selected files.
    pub total_bytes: u64,
    /// Selected bytes downloaded and verified.
    pub downloaded_bytes: u64,
    /// Bytes uploaded since the engine started.
    pub uploaded_bytes: u64,
    /// Download speed, bytes per second.
    pub down_bps: u64,
    /// Upload speed, bytes per second.
    pub up_bps: u64,
    /// Connected peers.
    pub peers_connected: u32,
    /// Peers known (seen) so far.
    pub peers_seen: u32,
    /// Seconds to go at the current speed.
    pub eta_secs: Option<u64>,
    /// Verified bytes per file (all files, torrent order).
    pub file_progress: Vec<u64>,
    /// Files selected for download (`None` = all).
    pub selected: Option<Vec<usize>>,
}

impl TorrentSnapshot {
    /// Maps onto the Riptide download state machine (a torrent is a download kind in the UI).
    pub fn download_state(&self) -> DownloadState {
        match &self.state {
            TorrentState::Checking => DownloadState::Verifying,
            TorrentState::Downloading => DownloadState::Downloading,
            TorrentState::Seeding | TorrentState::Completed => DownloadState::Completed,
            TorrentState::Paused => DownloadState::Paused,
            TorrentState::Failed { .. } => DownloadState::Failed { retryable: true },
        }
    }

    /// Progress from 0.0 to 1.0.
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.downloaded_bytes as f64 / self.total_bytes as f64).min(1.0)
        }
    }
}

/// Events the engine emits; consume them with [`crate::TorrentEngine::subscribe`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TorrentEvent {
    /// A torrent was added.
    Added {
        /// Info-hash.
        id: String,
    },
    /// Progress, speed and peers (at most twice a second per torrent).
    Progress(TorrentSnapshot),
    /// All selected pieces are verified.
    Completed {
        /// Info-hash.
        id: String,
    },
    /// Paused by the user (or on restart).
    Paused {
        /// Info-hash.
        id: String,
    },
    /// Resumed by the user.
    Resumed {
        /// Info-hash.
        id: String,
    },
    /// Files whose every piece has just been verified, while the torrent as a whole is still
    /// running. Lets a caller treat a finished file as usable (for example mark it as downloaded
    /// from the internet) without waiting for the rest of the torrent.
    FilesCompleted {
        /// Info-hash.
        id: String,
        /// Indices from [`crate::TorrentPreview::files`].
        indices: Vec<usize>,
    },
    /// Seeding stopped because the ratio or time limit was reached.
    SeedingStopped {
        /// Info-hash.
        id: String,
    },
    /// The torrent failed.
    Failed {
        /// Info-hash.
        id: String,
        /// Cause.
        error: String,
    },
    /// Removed.
    Removed {
        /// Info-hash.
        id: String,
    },
}
