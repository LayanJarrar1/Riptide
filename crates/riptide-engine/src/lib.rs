//! The download manager runtime: one [`Engine`] owns the SQLite store, a queue scheduler, the
//! running transfers and an event broadcast. The desktop app wraps it in Tauri commands; the
//! CLI's `--standalone` mode drives it in-process. Nothing here knows about a UI.
//!
//! ```text
//!  add / pause / resume / cancel / remove ──► Engine ──► Store (every change is a transaction)
//!                                              │  scheduler: ≤ max_active running, oldest Queued first
//!                                              │  task per download: probe → transfer → verify
//!                                              └─► broadcast<Event>  (UI, CLI, logs)
//! ```
//!
//! Crash recovery: on `open`, downloads left in an active state by a previous process are put
//! back in the queue along legal arrows and resume from their checkpoints.
//!
//! # Files: categories, duplicates, restart, disk space
//!
//! * **Category folders.** Every download gets a [`riptide_core::Category`] from its file name
//!   (`category_id` on the record). A request without a `dir` lands in
//!   [`EngineSettings::category_dirs`]`[category]` when the user set one, else in
//!   `default_dir`; the folder is re-resolved once after the probe, because the server's
//!   `Content-Disposition` name may change the category. A request *with* a `dir` is never
//!   moved.
//! * **Duplicates.** [`Engine::find_duplicate`] finds a record with the same URL (fragment
//!   ignored) or the same name in the same folder, so a UI can ask before adding twice. Adding
//!   anyway is allowed: the on-disk name is made unique (`name (1).ext`).
//! * **Restart from zero.** [`Engine::restart`] stops the transfer, deletes the part file (and
//!   a finished file the engine wrote), forgets validators, size and segments, and puts the
//!   record back in the queue (or leaves it paused). A record that had reached a terminal
//!   state is reborn in place and announced with [`Event::Added`] again.
//! * **Disk space.** [`free_space`] reports the bytes available on the volume of a folder,
//!   even one that does not exist yet.
//!
//! # Media pages
//!
//! A record with [`DownloadKind::Media`] (a YouTube / Vimeo / ... page) is run by
//! `riptide-media` (the bundled yt-dlp, see [`Engine::set_media_tools`]) instead of
//! `riptide-net`, through the same states and events: `Probing` asks yt-dlp for the page (title,
//! size estimate; live and DRM-only pages fail here), `Downloading` runs yt-dlp with progress,
//! `Verifying` → `Completed` once the merged file is on disk. Pause stops the process and resume
//! runs it again (`--continue` picks up its `.part` files); cancel and remove delete them.
//!
//! # BitTorrent
//!
//! A [`DownloadKind::Torrent`] record is run by `riptide-torrent` (librqbit), not by the segment
//! scheduler; see the `torrent` module for the add flow (preview, file choice, add), the state
//! mapping and the persistence. Call [`Engine::enable_torrents`] once after [`Engine::open`].
//!
//! # Site grabber
//!
//! [`Engine::crawl`] runs `riptide-crawl` with the engine's HTTP client (proxy, user agent, TLS
//! verification) and returns a [`riptide_crawl::CrawlResult`]: the files a site links to, to
//! depth N, within scope and caps, `robots.txt` obeyed. It adds nothing; the caller shows the
//! preview and passes the chosen files through
//! [`riptide_crawl::CrawlResult::download_requests`] to [`Engine::add`].
//!
//! # Queues, schedules, post-queue events
//!
//! Every download belongs to a queue ([`riptide_core::Queue`]: name, `max_concurrent`, manual
//! order, started/stopped switch, [`riptide_core::Schedule`], `on_done`). Three limits apply
//! together: [`EngineSettings::max_active`] (global), the queue's `max_concurrent`, and the
//! queue being started. See the [`queues`] module for the API (`create_queue`, `update_queue`,
//! `delete_queue`, `move_to_queue`, `start_queue`, `stop_queue`, `reorder`), the schedule timer
//! (a sleeping PC is not woken in v1) and [`Event::QueueFinished`], which only *announces* the
//! post-queue action: shutting down or sleeping is the app layer's job.
#![cfg_attr(test, allow(clippy::unwrap_used))]
// `deny`, not `forbid`: `disk::sys` on Windows is one documented Win32 call.
#![deny(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

mod torrent;

use riptide_core::category::TORRENTS_FOLDER;
use riptide_core::{
    Category, Clock, Download, DownloadId, DownloadKind, DownloadRequest, DownloadState, Event,
    MediaInfo, MediaSelection, QueueId, Secrets, Segment,
};
use riptide_media::{
    Attempt, DownloadJob, Ladder, MediaOutcome, MediaProgress, PlayerClients, YtDlp,
};
use riptide_net::{
    build_client, part_path_for, probe, transfer, ClientOptions, HostLimiter, Outcome, ProxyChoice,
    ProxyUrl, RateLimiter, RequestContext, TransferConfig, TransferEvent, TransferPlan,
};
use riptide_store::Store;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;

pub mod disk;
pub mod motw;
pub mod queues;
pub mod seen;

pub use seen::{SeenHint, SeenMatch, SeenQuery};

pub use disk::free_space;
pub use queues::SystemClock;
pub use riptide_net::parse_http_url;
pub use torrent::TorrentOptions;

/// Errors from the runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Persistence.
    #[error(transparent)]
    Store(#[from] riptide_store::Error),
    /// Network / transfer.
    #[error(transparent)]
    Net(#[from] riptide_net::Error),
    /// Model rule (illegal transition, invalid value).
    #[error(transparent)]
    Core(#[from] riptide_core::Error),
    /// No such download.
    #[error("download {} not found", .0 .0)]
    NotFound(DownloadId),
    /// Media page (yt-dlp).
    #[error(transparent)]
    Media(#[from] riptide_media::Error),
    /// The site grabber could not start (bad start URL or setting).
    #[error(transparent)]
    Crawl(#[from] riptide_crawl::Error),
    /// BitTorrent engine.
    #[error(transparent)]
    Torrent(#[from] riptide_torrent::Error),
    /// The "skip duplicates" option is on and this was downloaded before.
    #[error("already downloaded: {0}")]
    AlreadyDownloaded(String),
    /// Disk.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The target volume has less free space than the download still needs. The wording keeps
    /// the words "disk" and "space": the UI classifies a failure by them and shows its own
    /// localised "disk full" text for it.
    #[error("not enough free disk space: {needed} bytes are needed on the target drive; free some space, then retry")]
    DiskFull {
        /// Bytes still to write, plus a safety margin.
        needed: u64,
        /// Bytes free on the volume when the check ran.
        available: u64,
    },
    /// A background task died.
    #[error("internal: {0}")]
    Internal(String),
}

/// `category_dirs` key of the torrents folder (not a [`Category`]: a torrent may hold anything).
pub const TORRENTS_KEY: &str = "torrents";

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Settings the runtime reads; persisted as JSON in the store's `settings` table under
/// [`EngineSettings::KEY`]. UI-only preferences (language, theme) live in the app.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineSettings {
    /// Where downloads go unless the request says otherwise.
    pub default_dir: String,
    /// Connections per download (1..=32) when the request does not say.
    pub connections: u32,
    /// Simultaneous connections per host across all downloads.
    pub per_host: u32,
    /// Downloads running at once across all queues. Changing it also sets the *Main* queue's own
    /// `max_concurrent` to the same value; other queues keep their own limit (the lower of the
    /// two applies).
    pub max_active: u32,
    /// `User-Agent` for requests that do not carry a browser's.
    pub user_agent: Option<String>,
    /// Folder per category key (`"video"`, `"documents"`, … — see
    /// [`riptide_core::Category::key`]), plus `"torrents"` for torrents. A missing or empty entry
    /// means the organised folder (when `organize` is on) or `default_dir`. Only absolute paths
    /// under known keys survive [`Engine::set_settings`].
    #[serde(default)]
    pub category_dirs: BTreeMap<String, String>,
    /// Sort new downloads into one folder per category under [`Self::organize_root`]
    /// (`Riptide/Documents`, `Riptide/Videos`, …). On for a new installation; off for settings
    /// saved before it existed, so an upgrade does not change where files go. Existing files are
    /// never moved.
    #[serde(default)]
    pub organize: bool,
    /// Root of the organised folders; empty means `<default_dir>/Riptide`.
    #[serde(default)]
    pub organize_root: String,
    /// Cap in bytes per second across all downloads together; `None` = unlimited. Applies to
    /// running downloads within a second.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub global_speed_limit: Option<u64>,
    /// Which proxy new connections use.
    #[serde(default)]
    pub proxy: ProxySettings,
    /// Windows: mark finished downloads as coming from the internet (the NTFS
    /// `Zone.Identifier` stream browsers write), so SmartScreen and Office warn before an
    /// executable or macro document opens. On by default; see [`motw`]. Ignored elsewhere.
    #[serde(default = "default_true")]
    pub mark_of_the_web: bool,
    /// Follow a redirect from a public host to a private network address (NAS relay services
    /// such as QuickConnect, split-horizon intranet names). Off by default: a public page
    /// could otherwise send a download to a device on your network. Downloads that start on a
    /// private address are never affected.
    #[serde(default)]
    pub allow_private_redirects: bool,
    /// Settings -> Torrents: seeding rules, upload cap, DHT, port.
    #[serde(default)]
    pub torrent: TorrentOptions,
    /// Put a download that failed on a network error (no connection, timeout, reset, DNS) back
    /// in the queue after 1, 5 and 15 minutes, at most three times. Not for HTTP errors, disk
    /// errors, checksum mismatches or a changed file. Any action of the user starts the count
    /// over. On by default.
    #[serde(default = "default_true")]
    pub auto_retry: bool,
}

fn default_true() -> bool {
    true
}

/// A folder setting that can be used as it is: not blank, absolute.
fn is_usable_dir(dir: &str) -> bool {
    let dir = dir.trim();
    !dir.is_empty() && Path::new(dir).is_absolute()
}

/// How the engine reaches the network.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// The operating system's proxy (Windows: the per-user WinINet settings; also the
    /// `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` variables). PAC scripts are not evaluated.
    #[default]
    System,
    /// Direct connections.
    None,
    /// [`ProxySettings::url`].
    Manual,
}

/// Proxy settings. `Debug` hides credentials in the URL.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxySettings {
    /// System, none or manual.
    pub mode: ProxyMode,
    /// `http://`, `https://` or `socks5://` URL, optionally with `user:password@`; used only
    /// in [`ProxyMode::Manual`].
    pub url: String,
}

impl std::fmt::Debug for ProxySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = match ProxyUrl::parse(&self.url) {
            Ok(u) => u.redacted(),
            Err(_) if self.url.is_empty() => String::new(),
            Err(_) => "[invalid]".to_string(),
        };
        f.debug_struct("ProxySettings")
            .field("mode", &self.mode)
            .field("url", &url)
            .finish()
    }
}

impl ProxySettings {
    /// The client-level choice; a manual URL is validated here.
    pub fn choice(&self) -> Result<ProxyChoice> {
        Ok(match self.mode {
            ProxyMode::System => ProxyChoice::System,
            ProxyMode::None => ProxyChoice::Direct,
            ProxyMode::Manual => ProxyChoice::Manual(ProxyUrl::parse(&self.url)?),
        })
    }
}

impl EngineSettings {
    /// Key in the `settings` table.
    pub const KEY: &'static str = "engine";

    /// Defaults for `default_dir`.
    pub fn with_default_dir(dir: &Path) -> Self {
        EngineSettings {
            default_dir: dir.to_string_lossy().into_owned(),
            ..Self::default()
        }
    }

    /// Where a download of `category` goes when the request names no folder: the category's
    /// own folder if the user set one, else its organised folder (`organize` on), else
    /// `default_dir`.
    pub fn dir_for(&self, category: Category) -> PathBuf {
        self.custom_dir(category.key()).unwrap_or_else(|| {
            if self.organize {
                self.organized_root().join(category.folder_name())
            } else {
                PathBuf::from(&self.default_dir)
            }
        })
    }

    /// Where a torrent goes when the request names no folder; see [`Self::dir_for`].
    pub fn torrent_dir(&self) -> PathBuf {
        self.custom_dir(TORRENTS_KEY).unwrap_or_else(|| {
            if self.organize {
                self.organized_root().join(TORRENTS_FOLDER)
            } else {
                PathBuf::from(&self.default_dir)
            }
        })
    }

    /// The root of the organised folders: `organize_root`, else `<default_dir>/Riptide`.
    pub fn organized_root(&self) -> PathBuf {
        let root = self.organize_root.trim();
        if root.is_empty() {
            Path::new(&self.default_dir).join("Riptide")
        } else {
            PathBuf::from(root)
        }
    }

    /// Every organised folder: one per category and one for torrents.
    pub fn organized_dirs(&self) -> Vec<PathBuf> {
        Category::ALL
            .into_iter()
            .map(|c| self.dir_for(c))
            .chain(std::iter::once(self.torrent_dir()))
            .collect()
    }

    fn custom_dir(&self, key: &str) -> Option<PathBuf> {
        self.category_dirs
            .get(key)
            .filter(|d| !d.trim().is_empty())
            .map(|d| PathBuf::from(d.trim()))
    }

    fn sanitized(mut self) -> Self {
        self.connections = self.connections.clamp(1, riptide_core::MAX_CONNECTIONS);
        self.per_host = self.per_host.clamp(1, 64);
        self.max_active = self.max_active.clamp(1, 32);
        // unknown keys, blanks and relative paths are dropped: a category folder is either a
        // real absolute folder or "use the default"
        self.category_dirs = self
            .category_dirs
            .into_iter()
            .filter_map(|(key, dir)| {
                let dir = dir.trim();
                ((Category::from_key(&key).is_some() || key == TORRENTS_KEY)
                    && !dir.is_empty()
                    && Path::new(dir).is_absolute())
                .then(|| (key, dir.to_owned()))
            })
            .collect();
        // the organised root is an absolute folder or "the default" (empty)
        let root = self.organize_root.trim();
        self.organize_root = if Path::new(root).is_absolute() {
            root.to_owned()
        } else {
            String::new()
        };
        // 0 means unlimited, as everywhere else
        self.global_speed_limit = self.global_speed_limit.filter(|&l| l > 0);
        self.proxy.url = self.proxy.url.trim().to_string();
        self.torrent = self.torrent.sanitized();
        self
    }

    /// Options for the shared HTTP client. An unusable manual proxy (only possible for a
    /// settings row written by hand) falls back to the system proxy, with a warning.
    fn client_options(&self) -> ClientOptions {
        let proxy = self.proxy.choice().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ignoring the stored proxy");
            ProxyChoice::System
        });
        ClientOptions {
            user_agent: self.user_agent.clone(),
            accept_invalid_certs: false,
            proxy,
        }
    }
}

impl Default for EngineSettings {
    fn default() -> Self {
        EngineSettings {
            default_dir: String::new(),
            connections: riptide_core::DEFAULT_CONNECTIONS,
            per_host: 8,
            max_active: 3,
            user_agent: None,
            category_dirs: BTreeMap::new(),
            organize: false,
            organize_root: String::new(),
            global_speed_limit: None,
            proxy: ProxySettings::default(),
            mark_of_the_web: true,
            allow_private_redirects: false,
            torrent: TorrentOptions::default(),
            auto_retry: true,
        }
    }
}

/// What a probe found, in a shape the UI can show before adding.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeInfo {
    /// URL after redirects.
    pub final_url: String,
    /// Sanitised file name.
    pub filename: String,
    /// Size if known.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size: Option<u64>,
    /// Whether the server honours ranges.
    pub supports_ranges: bool,
    /// `Content-Type` if any.
    pub content_type: Option<String>,
    /// Ranges + size + validator: a partial file can be resumed in place.
    pub resumable: bool,
}

/// How to open an engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// The SQLite file (created if missing).
    pub db_path: PathBuf,
    /// Default download folder for a fresh settings row.
    pub default_dir: PathBuf,
    /// On a new installation (no settings saved yet), turn on [`EngineSettings::organize`] and
    /// create the organised folders once. The desktop app sets it; tools and tests do not.
    pub organize_new_install: bool,
}

/// What `add` left to the engine for a media record.
#[derive(Debug, Clone, Copy)]
struct MediaFlags {
    explicit_name: bool,
    auto_category: bool,
    auto_dir: bool,
}

/// Why a running transfer is being stopped; decides the state it ends in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    Pause,
    Cancel,
    Remove,
    Shutdown,
    /// `restart` takes over once the task has stopped; the task itself leaves the state alone.
    Restart,
    /// The queue was stopped: back to `Queued` (via `Paused`), so starting the queue resumes it.
    Requeue,
}

/// Creates the organised folders of a new installation. Best effort: a folder that cannot be
/// created (permissions, a file in the way) is logged and made later by the download that
/// needs it. Never touches what is already there.
fn create_organized_dirs(settings: &EngineSettings) {
    for dir in settings.organized_dirs() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(dir = %dir.display(), error = %e, "could not create a download folder");
        }
    }
}

/// Per-download flags kept in the `settings` table (the record has no columns for them).
fn flag_key(id: DownloadId, flag: &str) -> String {
    format!("download.{}.{flag}", id.0)
}

/// The user chose the file name; the probe must not override it.
const FLAG_EXPLICIT_NAME: &str = "explicit_name";
/// The category came from the file name, not the request; it follows the probed name.
const FLAG_AUTO_CATEGORY: &str = "auto_category";
/// The folder came from the category, not the request; it follows the category.
const FLAG_AUTO_DIR: &str = "auto_dir";
/// A media record got its final name from the first probe; later runs keep it (the partial
/// files carry that name).
const FLAG_MEDIA_NAMED: &str = "media_named";
/// `"1"` when the media attempt that worked used the browser's cookies, `"0"` when it did not:
/// the download then runs the same way (a probe that worked anonymously keeps the cookies out
/// of the yt-dlp job altogether).
const FLAG_MEDIA_COOKIES: &str = "media_cookies";
/// The [`PlayerClients`] key of the media attempt that worked; the next run starts with it.
const FLAG_MEDIA_CLIENT: &str = "media_client";
/// Every per-download flag, for cleanup.
const ALL_FLAGS: [&str; 6] = [
    FLAG_EXPLICIT_NAME,
    FLAG_AUTO_CATEGORY,
    FLAG_AUTO_DIR,
    FLAG_MEDIA_NAMED,
    FLAG_MEDIA_COOKIES,
    FLAG_MEDIA_CLIENT,
];

/// Crash recovery: puts every record a previous run left in an active state back in the queue,
/// along legal arrows. Also used by [`Engine::revive`].
fn requeue_active(s: &mut Store) -> riptide_store::Result<()> {
    for d in s.list_downloads()? {
        // a torrent is brought back by `enable_torrents`, from the torrent engine state
        if d.kind == DownloadKind::Torrent {
            continue;
        }
        match d.state {
            DownloadState::Probing | DownloadState::Downloading => {
                // a span still open here was cut by a crash: keep the checkpointed time
                s.abandon_active_span(d.id)?;
                s.set_state(d.id, DownloadState::Paused, None)?;
                s.set_state(d.id, DownloadState::Queued, None)?;
            }
            DownloadState::Verifying => {
                s.set_state(
                    d.id,
                    DownloadState::Failed { retryable: true },
                    Some("interrupted while verifying"),
                )?;
                s.set_state(d.id, DownloadState::Queued, None)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Extra room demanded on top of what a download still has to write.
const DISK_MARGIN: u64 = 64 * 1024 * 1024;

/// `Err(DiskFull)` when the volume cannot hold what is left of a download of known `size` of
/// which `written` bytes are already on disk. An unknown size or an unreadable free-space figure
/// never blocks a download.
fn check_free_space(size: Option<u64>, written: u64, free: std::io::Result<u64>) -> Result<()> {
    let Some(size) = size else { return Ok(()) };
    let needed = size.saturating_sub(written).saturating_add(DISK_MARGIN);
    match free {
        Ok(available) if available < needed => Err(Error::DiskFull { needed, available }),
        _ => Ok(()),
    }
}

/// Free-space query; replaced in tests.
type FreeSpaceFn = Arc<dyn Fn(&Path) -> std::io::Result<u64> + Send + Sync>;

/// Consecutive failed progress writes after which a download is stopped.
const MAX_CHECKPOINT_FAILURES: u32 = 5;

/// Counts consecutive failed checkpoint writes.
#[derive(Debug, Default)]
struct CheckpointFailures(u32);

impl CheckpointFailures {
    /// Records one write; `true` exactly when this failure is the
    /// [`MAX_CHECKPOINT_FAILURES`]-th in a row.
    fn record(&mut self, ok: bool) -> bool {
        if ok {
            self.0 = 0;
            false
        } else {
            self.0 += 1;
            self.0 == MAX_CHECKPOINT_FAILURES
        }
    }
}

/// Shown when the database refuses progress writes. Keeps "disk" and "space" so the UI files
/// it under its localised disk problem.
const DB_WRITE_FAILED: &str = "the database could not be written (the disk may be full or the file locked), so the download was stopped; free some disk space, then retry";

/// Waits before the automatic retries of a download that failed on the network.
const AUTO_RETRY_DELAYS: [std::time::Duration; 3] = [
    std::time::Duration::from_secs(60),
    std::time::Duration::from_secs(5 * 60),
    std::time::Duration::from_secs(15 * 60),
];

/// Automatic-retry bookkeeping of one download (in memory only).
#[derive(Default)]
struct AutoRetry {
    /// Retries scheduled so far.
    attempts: u32,
    /// The pending timer.
    timer: Option<JoinHandle<()>>,
}

/// A failure that waiting and trying again can plausibly cure: the network was down (connect,
/// timeout, reset, DNS), not the server's answer, the disk, the checksum or a changed file.
fn is_network_failure(e: &Error) -> bool {
    fn net(n: &riptide_net::Error) -> bool {
        match n {
            riptide_net::Error::RetriesExhausted { last, .. } => net(last),
            riptide_net::Error::Request(_) | riptide_net::Error::Io(_) => n.is_retryable(),
            _ => false,
        }
    }
    matches!(e, Error::Net(n) if net(n))
}

/// A checksum mismatch, also when it travels inside another error.
fn is_checksum_mismatch(e: &Error) -> bool {
    fn net(n: &riptide_net::Error) -> bool {
        match n {
            riptide_net::Error::ChecksumMismatch { .. } => true,
            riptide_net::Error::RetriesExhausted { last, .. } => net(last),
            _ => false,
        }
    }
    matches!(e, Error::Net(n) if net(n))
}

/// Remembers which way of running yt-dlp worked for a record.
fn store_attempt(
    s: &mut riptide_store::Store,
    id: DownloadId,
    a: Attempt,
) -> riptide_store::Result<()> {
    s.set_setting(
        &flag_key(id, FLAG_MEDIA_COOKIES),
        if a.cookies { "1" } else { "0" },
    )?;
    s.set_setting(&flag_key(id, FLAG_MEDIA_CLIENT), a.clients.key())
}

/// Longest title (in characters) used as a media file name; yt-dlp appends `.f137.mp4.part`.
const MAX_MEDIA_STEM_CHARS: usize = 120;

/// `<name>` without the extension the selection produces.
fn media_stem(filename: &str, selection: MediaSelection) -> String {
    filename
        .strip_suffix(&format!(".{}", selection.final_ext()))
        .unwrap_or(filename)
        .to_string()
}

/// `<title>.<ext>`, sanitised and short enough for yt-dlp's suffixes.
fn media_filename(title: &str, selection: MediaSelection) -> String {
    let title: String = title.trim().chars().take(MAX_MEDIA_STEM_CHARS).collect();
    let stem = riptide_core::sanitize_filename(&title);
    let stem = if stem.is_empty() || stem == "_" {
        "video".to_string()
    } else {
        stem
    };
    riptide_core::sanitize_filename(&format!("{stem}.{}", selection.final_ext()))
}

/// Deletes what an unfinished download left on disk: the `.rtpart` file, or yt-dlp's partial
/// files for a media record.
fn delete_partials(d: &Download) {
    match d.kind {
        DownloadKind::Http => {
            let _ = std::fs::remove_file(part_path_for(&d.final_path()));
        }
        DownloadKind::Media => {
            let selection = d
                .media_format
                .as_deref()
                .and_then(MediaSelection::parse)
                .unwrap_or(MediaSelection::Best);
            let stem = media_stem(&d.filename, selection);
            for p in riptide_media::partial_files(&d.dir, &stem) {
                let _ = std::fs::remove_file(p);
            }
        }
        // the torrent engine owns (and deletes) its files; see `Engine::torrent_cancel`
        DownloadKind::Torrent => {}
    }
}

/// A folder compared the way the file system does: separators folded, trailing slash dropped,
/// case ignored on Windows.
fn dir_key(dir: &Path) -> String {
    let s = dir
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// How long `pause` / `cancel` / `remove` wait for a transfer to stop.
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

struct Running {
    stop: CancellationToken,
    reason: Arc<Mutex<StopReason>>,
    task: JoinHandle<()>,
    /// Turns `true` when the task has ended and left the `running` map.
    done: tokio::sync::watch::Receiver<bool>,
    /// The download's own bandwidth bucket (its `speed_limit`), shared by its connections.
    limit: Arc<RateLimiter>,
    /// Queue the download counts against; kept current by `move_to_queue` / `delete_queue`.
    queue_id: QueueId,
}

struct Inner {
    store: Arc<Mutex<Store>>,
    /// Rebuilt when the proxy or user agent changes; running transfers keep the old one.
    client: RwLock<reqwest::Client>,
    limiter: RwLock<Arc<HostLimiter>>,
    /// Per-host connection caps learned from `429` / `503`, persisted as `host_cap.<host>`.
    host_caps: Arc<Mutex<HashMap<String, u32>>>,
    /// Local wall-clock source for queue schedules.
    clock: Arc<dyn Clock>,
    /// Wakes the schedule timer (queues changed, shutdown).
    sched_wake: Notify,
    /// Per scheduled queue: the last "should be running" answer the timer applied.
    sched_applied: Mutex<HashMap<QueueId, bool>>,
    /// Serialises schedule passes: the timer task and a caller of `check_schedules` must not
    /// interleave (the second would see the level marked applied before the queue changed).
    sched_pass: tokio::sync::Mutex<()>,
    /// Queues whose `QueueFinished` was announced and that have not started anything since.
    announced: Mutex<HashSet<QueueId>>,
    /// [`EngineSettings::global_speed_limit`], shared by every connection of every download.
    global_limit: Arc<RateLimiter>,
    events: broadcast::Sender<Event>,
    running: Mutex<HashMap<DownloadId, Running>>,
    wake: Notify,
    settings: RwLock<EngineSettings>,
    shutting_down: AtomicBool,
    /// Test knobs (0 = off): stop-wait timeout and an artificial delay before a stopping task
    /// reacts, to exercise the stuck-stop path.
    test_stop_timeout_ms: std::sync::atomic::AtomicU64,
    test_stop_delay_ms: std::sync::atomic::AtomicU64,
    test_crash_after_rename: AtomicBool,
    /// The bundled yt-dlp (and ffmpeg) for media records; `None` until the app sets it.
    media: RwLock<Option<YtDlp>>,
    /// Recent video probes (10 minutes, in memory), shared by the extension and the Add dialog.
    probe_cache: riptide_media::ProbeCache,
    /// The BitTorrent engine and its bridge to the download rows; `None` until
    /// [`Engine::enable_torrents`].
    torrents: RwLock<Option<Arc<torrent::TorrentRuntime>>>,
    /// Held while the torrent engine starts or stops (also when it stops because nothing needs
    /// it), so two callers never start two sessions and nobody starts one halfway through a stop.
    torrents_starting: tokio::sync::Mutex<()>,
    /// Previews waiting for the user's choice. They outlive the torrent engine: it may stop while
    /// the dialog is open.
    torrent_previews: Mutex<std::collections::VecDeque<torrent::Pending>>,
    torrent_next_token: std::sync::atomic::AtomicU64,
    /// The last last-look at the torrent settings (tests turn DHT and trackers off); reapplied
    /// whenever the engine is started again.
    torrent_tweak: Mutex<Option<torrent::Tweak>>,
    /// How long nothing needs the torrent engine before it is stopped (milliseconds).
    torrent_idle_grace_ms: std::sync::atomic::AtomicU64,
    /// Folder next to the database file; the torrent engine keeps its state below it.
    data_dir: PathBuf,
    /// Serialises the choice of an on-disk file name, so two downloads that start at the same
    /// moment can never settle on the same name (and share one part file).
    name_lock: tokio::sync::Mutex<()>,
    /// How much room the target volume has; the OS answer unless a test replaced it.
    free_space_fn: RwLock<FreeSpaceFn>,
    /// Automatic retries after network failures, per download.
    auto_retries: Mutex<HashMap<DownloadId, AutoRetry>>,
    /// Waits of the automatic retries ([`AUTO_RETRY_DELAYS`] unless a test shortened them).
    retry_delays: RwLock<Vec<std::time::Duration>>,
}

/// The runtime. Cheap to clone; all clones share one state.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

async fn db<T, F>(store: &Arc<Mutex<Store>>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Store) -> riptide_store::Result<T> + Send + 'static,
{
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || {
        let mut guard = store.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    })
    .await
    .map_err(|e| Error::Internal(format!("store task: {e}")))?
    .map_err(Error::from)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Row id of `category` in the store's `categories` table (`None` if the seed lacks it).
fn category_row_id(
    store: &Store,
    category: Category,
) -> riptide_store::Result<Option<riptide_core::CategoryId>> {
    Ok(store
        .categories()?
        .into_iter()
        .find(|c| c.key == category.key())
        .map(|c| c.id))
}

/// A request turned into the records it will insert, before the database is touched.
struct Prepared {
    download: Download,
    secrets: Secrets,
    category: Category,
    auto_dir: bool,
    auto_category: bool,
    explicit_name: bool,
}

/// What survives a move to another origin: only `Referer` and `User-Agent`. Cookies, credentials
/// and every other header (`Authorization`, API keys, ...) belong to the origin they were
/// captured for.
fn without_origin_bound(old: Secrets) -> Secrets {
    Secrets {
        cookies: None,
        auth: None,
        headers: old
            .headers
            .into_iter()
            .filter(|(k, _)| {
                k.eq_ignore_ascii_case("referer") || k.eq_ignore_ascii_case("user-agent")
            })
            .collect(),
    }
}

/// Validates `request` and builds the record [`Engine::add`] inserts.
fn prepare_add(request: &DownloadRequest, settings: &EngineSettings) -> Result<Prepared> {
    let url = riptide_net::parse_http_url(&request.url)?;
    // `user:password@` in the link is a credential, not part of the address: the record keeps
    // the clean URL and the secrets table gets the credentials (secrets stay out of the record)
    let (clean_url, url_auth) = riptide_net::split_userinfo(&url);
    let media_selection = match request.kind {
        DownloadKind::Http => None,
        DownloadKind::Torrent => {
            return Err(Error::Core(riptide_core::Error::Invalid(
                "torrents are added with torrent_preview and add_torrent".into(),
            )))
        }
        DownloadKind::Media => {
            let key = request.media_format.as_deref().unwrap_or("best");
            Some(MediaSelection::parse(key).ok_or_else(|| {
                Error::Core(riptide_core::Error::Invalid(format!(
                    "unknown media quality {key:?}"
                )))
            })?)
        }
    };
    let filename = match media_selection {
        // a media name is `<title>.<ext>`; the extension follows the chosen quality
        Some(selection) => {
            let title = request
                .filename
                .as_deref()
                .map(|f| media_stem(f, selection))
                .unwrap_or_else(|| "video".into());
            media_filename(&title, selection)
        }
        None => request
            .filename
            .as_deref()
            .map(riptide_core::sanitize_filename)
            .unwrap_or_else(|| {
                riptide_core::sanitize_filename(&riptide_net::probe::filename_from_url(&url))
            }),
    };
    // folder: the request's, else the category's (by extension), else the default; the
    // folder of an explicit category is resolved at insert time, where its key can be looked up
    let category = Category::from_filename(&filename);
    let auto_dir = request.dir.is_none();
    let auto_category = request.category_id.is_none();
    let dir = request
        .dir
        .clone()
        .unwrap_or_else(|| settings.dir_for(category));
    let mut d = Download::new(
        DownloadId(0),
        if url_auth.is_some() {
            clean_url.to_string()
        } else {
            request.url.clone()
        },
        filename,
        dir,
        now_millis(),
    );
    d.connections = request
        .connections
        .unwrap_or(settings.connections)
        .clamp(1, riptide_core::MAX_CONNECTIONS);
    d.priority = request.priority.unwrap_or_default();
    d.checksum_expected = request.checksum.clone();
    d.queue_id = request.queue_id.unwrap_or(QueueId::MAIN);
    d.category_id = request.category_id;
    d.speed_limit = request.speed_limit;
    d.kind = request.kind;
    d.media_format = media_selection.map(MediaSelection::key);
    if media_selection.is_some() {
        d.connections = 1;
    }
    if request.start_paused {
        d.state = DownloadState::Paused;
    }

    // referrer and user agent ride along as same-origin headers (the record has no columns)
    let mut secrets = request.secrets.clone().unwrap_or_default();
    if let (Some((username, password)), None) = (url_auth, &secrets.auth) {
        secrets.auth = Some(riptide_core::Credentials { username, password });
    }
    if let Some(r) = &request.referrer {
        secrets.headers.push(("Referer".into(), r.clone()));
    }
    if let Some(ua) = &request.user_agent {
        secrets.headers.push(("User-Agent".into(), ua.clone()));
    }
    Ok(Prepared {
        download: d,
        secrets,
        category,
        auto_dir,
        auto_category,
        explicit_name: request.filename.is_some(),
    })
}

/// Inserts a prepared download with its secrets and auto-decision flags. Runs inside the
/// caller's transaction (or on its own for a single add).
fn insert_prepared(
    s: &mut Store,
    p: Prepared,
    settings: &EngineSettings,
) -> riptide_store::Result<DownloadId> {
    let Prepared {
        download: mut d,
        secrets,
        category,
        auto_dir,
        auto_category,
        explicit_name,
    } = p;
    match d.category_id {
        None => d.category_id = category_row_id(s, category)?,
        Some(explicit) if auto_dir => {
            let row = s.categories()?.into_iter().find(|c| c.id == explicit);
            if let Some(c) = row.and_then(|c| Category::from_key(&c.key)) {
                d.dir = settings.dir_for(c);
            }
        }
        Some(_) => {}
    }
    let id = s.insert_download(&d)?;
    if !secrets.is_empty() {
        s.set_secrets(id, &secrets)?;
    }
    // what the request left to the engine is remembered, so the probe (which may rename the
    // file) can re-decide it; what the user chose is never overridden
    for (flag, set) in [
        (FLAG_EXPLICIT_NAME, explicit_name),
        (FLAG_AUTO_CATEGORY, auto_category),
        (FLAG_AUTO_DIR, auto_dir),
    ] {
        if set {
            s.set_setting(&flag_key(id, flag), "1")?;
        }
    }
    Ok(id)
}

/// Why [`Engine::add_batch`] did not add a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum BatchSkipReason {
    /// The URL is already in the list, or appears earlier in the same batch.
    Duplicate,
    /// Not a valid `http(s)` URL (or the request itself was rejected).
    Invalid,
}

/// A request [`Engine::add_batch`] left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct BatchSkip {
    /// The request's URL.
    pub url: String,
    /// Why it was skipped.
    pub reason: BatchSkipReason,
    /// For a duplicate of an existing download, that download.
    pub existing: Option<DownloadId>,
}

/// The result of [`Engine::add_batch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct BatchOutcome {
    /// The new downloads, in request order.
    pub added: Vec<DownloadId>,
    /// What was not added, in request order.
    pub skipped: Vec<BatchSkip>,
}

/// `url` without its fragment, for duplicate comparison; `None` if it does not parse.
fn comparable_url(url: &str) -> Option<String> {
    let mut parsed = riptide_net::split_userinfo(&Url::parse(url).ok()?).0;
    parsed.set_fragment(None);
    Some(parsed.into())
}

impl Engine {
    /// Picks the on-disk name for a download that starts from zero and saves it before anyone
    /// else can pick: a name is taken when the file exists, or when another download of this
    /// engine (queued, running, paused or failed with its part file kept) already claims it.
    /// Without the claims two downloads starting together both saw "free" and wrote into one
    /// part file. This download's own stale part file is deliberately not a claim: it is reused.
    async fn reserve_file_name(&self, record: &mut Download) -> Result<()> {
        let _guard = self.inner.name_lock.lock().await;
        let id = record.id;
        let claimed: Vec<(String, String)> = self
            .list()
            .await?
            .into_iter()
            .filter(|d| {
                d.id != id
                    && !matches!(d.state, DownloadState::Completed | DownloadState::Cancelled)
            })
            .map(|d| (dir_key(&d.dir), d.filename.to_lowercase()))
            .collect();
        // a name that is another download's part file would be opened as that part file
        if record.filename.to_lowercase().ends_with(".rtpart") {
            record.filename.push('_');
        }
        let taken = |p: &Path| {
            p.exists()
                || p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy().to_lowercase();
                    let parent = p.parent().map(dir_key);
                    claimed.iter().any(|(dir, name)| {
                        Some(dir) == parent.as_ref()
                            && (*name == n || format!("{name}.rtpart") == n)
                    })
                })
        };
        let unique = riptide_core::unique_path(&record.dir, &record.filename, taken);
        if let Some(name) = unique.file_name() {
            record.filename = name.to_string_lossy().into_owned();
        }
        let to_save = record.clone();
        db(&self.inner.store, move |s| s.update_download(&to_save)).await
    }

    /// Opens the store, loads settings, re-queues downloads a crash left active, and starts
    /// the scheduler.
    pub async fn open(config: EngineConfig) -> Result<Engine> {
        Self::open_with_clock(config, Arc::new(SystemClock)).await
    }

    /// Like [`Engine::open`] with an explicit wall clock for queue schedules (tests drive a
    /// [`riptide_core::ManualClock`] and call [`Engine::check_schedules`]).
    pub async fn open_with_clock(config: EngineConfig, clock: Arc<dyn Clock>) -> Result<Engine> {
        let db_path = config.db_path.clone();
        let store = tokio::task::spawn_blocking(move || Store::open(&db_path))
            .await
            .map_err(|e| Error::Internal(format!("store open task: {e}")))??;
        let store = Arc::new(Mutex::new(store));

        let default_dir = config.default_dir.clone();
        let organize_new_install = config.organize_new_install;
        let (settings, first_run) = db(&store, move |s| {
            let saved = s.setting(EngineSettings::KEY)?;
            let first_run = saved.is_none();
            let mut loaded = saved
                .and_then(|raw| serde_json::from_str::<EngineSettings>(&raw).ok())
                .unwrap_or_else(|| EngineSettings::with_default_dir(&default_dir));
            if first_run && organize_new_install {
                loaded.organize = true;
            }
            let mut loaded = loaded.sanitized();
            if !is_usable_dir(&loaded.default_dir) {
                loaded.default_dir = default_dir.to_string_lossy().into_owned();
            }
            if first_run && organize_new_install {
                // saved now, so the folders below are created once, not at every start
                let raw = serde_json::to_string(&loaded)
                    .map_err(|e| riptide_store::Error::Corrupt(e.to_string()))?;
                s.set_setting(EngineSettings::KEY, &raw)?;
            }
            Ok((loaded, first_run))
        })
        .await?;
        if first_run && settings.organize {
            create_organized_dirs(&settings);
        }

        // crash recovery: anything still "active" belongs back in the queue
        db(&store, requeue_active).await?;

        let learned: HashMap<String, u32> = db(&store, |s| {
            Ok(s.settings()?
                .into_iter()
                .filter_map(|(k, v)| {
                    let host = k.strip_prefix(queues::HOST_CAP_PREFIX)?;
                    let cap = v.parse::<u32>().ok().filter(|c| *c > 0)?;
                    Some((host.to_string(), cap))
                })
                .collect())
        })
        .await?;
        let host_caps = Arc::new(Mutex::new(learned));

        let client = build_client(&settings.client_options())?;
        let (events, _) = broadcast::channel(2048);
        let inner = Arc::new(Inner {
            limiter: RwLock::new(queues::make_limiter(
                settings.per_host as usize,
                &store,
                &host_caps,
            )),
            host_caps,
            clock,
            sched_wake: Notify::new(),
            sched_applied: Mutex::new(HashMap::new()),
            sched_pass: tokio::sync::Mutex::new(()),
            announced: Mutex::new(HashSet::new()),
            store,
            client: RwLock::new(client),
            global_limit: Arc::new(RateLimiter::new(settings.global_speed_limit)),
            events,
            running: Mutex::new(HashMap::new()),
            wake: Notify::new(),
            settings: RwLock::new(settings),
            shutting_down: AtomicBool::new(false),
            test_stop_timeout_ms: std::sync::atomic::AtomicU64::new(0),
            test_stop_delay_ms: std::sync::atomic::AtomicU64::new(0),
            test_crash_after_rename: AtomicBool::new(false),
            media: RwLock::new(None),
            probe_cache: riptide_media::ProbeCache::default(),
            torrents: RwLock::new(None),
            torrents_starting: tokio::sync::Mutex::new(()),
            torrent_previews: Mutex::new(std::collections::VecDeque::new()),
            torrent_next_token: std::sync::atomic::AtomicU64::new(1),
            torrent_tweak: Mutex::new(None),
            torrent_idle_grace_ms: std::sync::atomic::AtomicU64::new(
                torrent::DEFAULT_IDLE_GRACE.as_millis() as u64,
            ),
            data_dir: config
                .db_path
                .parent()
                .map(Path::to_path_buf)
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| PathBuf::from(".")),
            name_lock: tokio::sync::Mutex::new(()),
            free_space_fn: RwLock::new(Arc::new(|p: &Path| free_space(p))),
            auto_retries: Mutex::new(HashMap::new()),
            retry_delays: RwLock::new(AUTO_RETRY_DELAYS.to_vec()),
        });
        let engine = Engine { inner };
        tokio::spawn(engine.clone().scheduler());
        tokio::spawn(engine.clone().schedule_timer());
        Ok(engine)
    }

    /// Sets (or clears) the yt-dlp that runs media records. Applies from the next start.
    pub fn set_media_tools(&self, tools: Option<YtDlp>) {
        if let Ok(mut m) = self.inner.media.write() {
            *m = tools;
        }
        self.inner.probe_cache.clear(); // a new yt-dlp may list other formats
    }

    /// The yt-dlp media records use, if set.
    pub fn media_tools(&self) -> Option<YtDlp> {
        self.inner.media.read().ok().and_then(|m| m.clone())
    }

    fn require_media(&self) -> Result<YtDlp> {
        self.media_tools()
            .ok_or_else(|| Error::Media(riptide_media::Error::ToolMissing(PathBuf::from("yt-dlp"))))
    }

    /// Asks yt-dlp what a video page offers (title, formats, the quality menu) without adding
    /// it. `cookies` is the browser's `Cookie` header for the page; it goes to yt-dlp in a
    /// temporary file and nowhere else.
    pub async fn probe_media(&self, url: &str, cookies: Option<&str>) -> Result<MediaInfo> {
        riptide_net::parse_http_url(url)?;
        let tool = self.require_media()?;
        let cookies = cookies.filter(|c| !c.trim().is_empty());
        let Some(page) = riptide_media::normalize_url(url) else {
            return Ok(tool.probe(url, cookies).await?);
        };
        // A page that probed fine without cookies is cached for everyone; one that needed the
        // login is cached per cookie header (hashed, never stored), so it is never served to a
        // caller without that login.
        let anon = format!("anon|{page}");
        let with_cookies = cookies.map(|c| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            c.hash(&mut h);
            format!("ck{:x}|{page}", h.finish())
        });
        let lookup = |cache: &riptide_media::ProbeCache| {
            cache
                .get(&anon)
                .or_else(|| with_cookies.as_deref().and_then(|k| cache.get(k)))
        };
        if let Some(hit) = lookup(&self.inner.probe_cache) {
            return Ok(hit);
        }
        // one yt-dlp run per page: later callers wait here and then find the answer cached
        let _turn = self.inner.probe_cache.lock(&page).await;
        if let Some(hit) = lookup(&self.inner.probe_cache) {
            return Ok(hit);
        }
        let (info, attempt) = tool.probe_resilient(url, cookies, None).await?;
        let key = match (attempt.cookies, with_cookies) {
            (true, Some(k)) => k,
            _ => anon,
        };
        self.inner.probe_cache.put(key, info.clone());
        Ok(info)
    }

    /// Crawls a site for downloadable files (the *site grabber*) with the engine's own HTTP
    /// client: its proxy, its `User-Agent`, TLS verification on, redirects capped at 20.
    ///
    /// Nothing is added to the download list: the result is a preview. Turn it into downloads
    /// with [`riptide_crawl::CrawlResult::download_requests`] and [`Engine::add`] (the file
    /// list is the user's to edit first). Progress arrives on `events` as it happens, ending
    /// with `Done`; `cancel` stops the crawl and the partial result is returned. The crawl's
    /// cookies and credentials (`spec.secrets`) only ever go to the start page's origin, and
    /// `robots.txt` is obeyed unless `spec.respect_robots` is off. See `riptide-crawl`.
    pub async fn crawl(
        &self,
        spec: riptide_crawl::CrawlSpec,
        events: Option<mpsc::Sender<riptide_crawl::CrawlEvent>>,
        cancel: CancellationToken,
    ) -> Result<riptide_crawl::CrawlResult> {
        // a crawl that starts on a public host gets a client whose DNS resolver only returns
        // public addresses (and a guard that refuses private IP links and redirects); one that
        // starts on the owner's LAN or loopback uses the normal client
        // (an unusable start URL is reported by the crawl itself)
        let guard = match riptide_net::parse_http_url(&spec.start) {
            Ok(start) => riptide_net::NetGuard::for_start(&start, None).await,
            Err(_) => riptide_net::NetGuard::off(),
        };
        let client = if guard.blocks() {
            riptide_net::build_client_guarded(&self.settings().client_options(), &guard)?
        } else {
            self.client()
        };
        Ok(riptide_crawl::crawl_with_guard(spec, client, events, cancel, guard).await?)
    }

    /// Engine events (added, probed, state changes, progress, completed, failed, removed).
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Current settings.
    pub fn settings(&self) -> EngineSettings {
        self.inner
            .settings
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Replaces the settings (persisted). Running transfers keep their connection count; the
    /// global speed limit applies to them within a second; a new proxy or user agent applies
    /// to connections opened after this call. An invalid manual proxy URL is refused.
    pub async fn set_settings(&self, settings: EngineSettings) -> Result<()> {
        let mut settings = settings.sanitized();
        let previous = self.settings();
        // the default folder is an absolute path or it stays what it was: an empty or relative
        // value would put files wherever the process happens to run
        settings.default_dir = settings.default_dir.trim().to_owned();
        if !is_usable_dir(&settings.default_dir) {
            settings.default_dir = previous.default_dir.clone();
        }
        let new_client =
            if previous.proxy != settings.proxy || previous.user_agent != settings.user_agent {
                settings.proxy.choice()?;
                Some(build_client(&settings.client_options())?)
            } else {
                None
            };
        let raw = serde_json::to_string(&settings).map_err(|e| Error::Internal(e.to_string()))?;
        db(&self.inner.store, move |s| {
            s.set_setting(EngineSettings::KEY, &raw)
        })
        .await?;
        if let Ok(mut l) = self.inner.limiter.write() {
            *l = queues::make_limiter(
                settings.per_host as usize,
                &self.inner.store,
                &self.inner.host_caps,
            );
        }
        if let Some(client) = new_client {
            if let Ok(mut c) = self.inner.client.write() {
                *c = client;
            }
            tracing::info!(proxy = ?settings.proxy, "HTTP client rebuilt");
        }
        self.inner
            .global_limit
            .set_limit(settings.global_speed_limit);
        self.apply_torrent_options(&settings.torrent);
        let max_active = settings.max_active;
        if let Ok(mut s) = self.inner.settings.write() {
            *s = settings;
        }
        if previous.max_active != max_active {
            self.sync_main_queue_limit(max_active).await?;
        }
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Every download, oldest first.
    pub async fn list(&self) -> Result<Vec<Download>> {
        db(&self.inner.store, |s| s.list_downloads()).await
    }

    /// One download.
    pub async fn get(&self, id: DownloadId) -> Result<Option<Download>> {
        db(&self.inner.store, move |s| s.get_download(id)).await
    }

    /// Segments of a download.
    pub async fn segments(&self, id: DownloadId) -> Result<Vec<Segment>> {
        db(&self.inner.store, move |s| s.segments(id)).await
    }

    /// Probes a URL without adding it (the add dialog's preview).
    pub async fn probe(
        &self,
        url: &str,
        secrets: Option<&Secrets>,
        referrer: Option<&str>,
    ) -> Result<ProbeInfo> {
        let parsed = riptide_net::parse_http_url(url)?;
        let (clean, url_auth) = riptide_net::split_userinfo(&parsed);
        let mut ctx = RequestContext::new(&clean)
            .with_referrer(referrer)
            .with_guard(self.redirect_guard());
        let mut merged = secrets.cloned().unwrap_or_default();
        if let (Some((username, password)), None) = (url_auth, &merged.auth) {
            merged.auth = Some(riptide_core::Credentials { username, password });
        }
        if !merged.is_empty() {
            ctx = ctx.with_secrets(&merged);
        }
        let p = probe(&self.client(), clean.as_str(), &ctx).await?;
        Ok(ProbeInfo {
            final_url: riptide_net::split_userinfo(&p.final_url).0.to_string(),
            filename: p.filename.clone(),
            size: p.size,
            supports_ranges: p.supports_ranges,
            content_type: p.content_type.clone(),
            resumable: p.resumable(),
        })
    }

    /// Adds a download. It starts when a queue slot is free (or waits in `Paused` when
    /// `start_paused`). Returns its id.
    pub async fn add(&self, request: DownloadRequest) -> Result<DownloadId> {
        self.add_checked(request, false).await
    }

    /// [`Engine::add`] with an explicit answer to "skip duplicates": `allow_duplicates` adds
    /// even when the option is on and the same thing was downloaded before.
    pub async fn add_checked(
        &self,
        request: DownloadRequest,
        allow_duplicates: bool,
    ) -> Result<DownloadId> {
        if !allow_duplicates && self.skip_duplicates().await {
            let q = SeenQuery {
                url: Some(request.url.clone()),
                ..SeenQuery::default()
            };
            if let Some(Some(m)) = self.seen_lookup(vec![q]).await?.into_iter().next() {
                if !m.stale {
                    return Err(Error::AlreadyDownloaded(m.title));
                }
            }
        }
        let settings = self.settings();
        let prepared = prepare_add(&request, &settings)?;
        let id = db(&self.inner.store, move |s| {
            insert_prepared(s, prepared, &settings)
        })
        .await?;
        self.emit(Event::Added { id });
        self.inner.wake.notify_one();
        Ok(id)
    }

    /// Adds many downloads in ONE store transaction: either every accepted item is in the list
    /// afterwards or (on a store error) none is. Each request is handled exactly like
    /// [`Engine::add`]; shared options are simply copied into every request (see
    /// [`riptide_core::BatchOptions`]).
    ///
    /// Items that are not added are reported, not failed: a request whose URL is already in
    /// the list (same URL ignoring the fragment, as the original or the final URL) or repeats
    /// an earlier request of this batch is [`BatchSkipReason::Duplicate`]; an unparseable or
    /// non-http(s) URL is [`BatchSkipReason::Invalid`]. At most [`riptide_core::MAX_EXPANSION`]
    /// requests are accepted per call. [`Event::Added`] is emitted for every new download, in
    /// order, after the commit.
    pub async fn add_batch(&self, requests: Vec<DownloadRequest>) -> Result<BatchOutcome> {
        if requests.len() > riptide_core::MAX_EXPANSION {
            return Err(Error::Core(riptide_core::Error::Invalid(format!(
                "a batch holds at most {} downloads",
                riptide_core::MAX_EXPANSION
            ))));
        }
        let settings = self.settings();
        let mut known: HashMap<String, DownloadId> = HashMap::new();
        for d in self.list().await? {
            for url in [Some(d.url.as_str()), d.final_url.as_deref()]
                .into_iter()
                .flatten()
            {
                if let Some(key) = comparable_url(url) {
                    known.entry(key).or_insert(d.id);
                }
            }
        }
        let mut in_batch: HashSet<String> = HashSet::new();
        let mut skipped = Vec::new();
        let mut prepared = Vec::new();
        // "skip duplicates" also covers what was downloaded before and is still on disk
        let already: Vec<bool> = if self.skip_duplicates().await {
            let queries = requests
                .iter()
                .map(|r| SeenQuery {
                    url: Some(r.url.clone()),
                    ..SeenQuery::default()
                })
                .collect();
            self.seen_lookup(queries)
                .await?
                .into_iter()
                .map(|m| m.is_some_and(|m| !m.stale))
                .collect()
        } else {
            vec![false; requests.len()]
        };
        for (request, was_downloaded) in requests.iter().zip(already) {
            let skip = |reason, existing| BatchSkip {
                url: request.url.clone(),
                reason,
                existing,
            };
            if riptide_net::parse_http_url(&request.url).is_err() {
                skipped.push(skip(BatchSkipReason::Invalid, None));
                continue;
            }
            if was_downloaded {
                skipped.push(skip(BatchSkipReason::Duplicate, None));
                continue;
            }
            let key = comparable_url(&request.url).unwrap_or_else(|| request.url.clone());
            if let Some(&id) = known.get(&key) {
                skipped.push(skip(BatchSkipReason::Duplicate, Some(id)));
            } else if !in_batch.insert(key) {
                skipped.push(skip(BatchSkipReason::Duplicate, None));
            } else {
                match prepare_add(request, &settings) {
                    Ok(p) => prepared.push(p),
                    Err(_) => skipped.push(skip(BatchSkipReason::Invalid, None)),
                }
            }
        }
        let added = if prepared.is_empty() {
            Vec::new()
        } else {
            db(&self.inner.store, move |s| {
                s.transaction(|s| {
                    prepared
                        .into_iter()
                        .map(|p| insert_prepared(s, p, &settings))
                        .collect::<riptide_store::Result<Vec<_>>>()
                })
            })
            .await?
        };
        for &id in &added {
            self.emit(Event::Added { id });
        }
        if !added.is_empty() {
            self.inner.wake.notify_one();
        }
        Ok(BatchOutcome { added, skipped })
    }

    /// Sets a download's own speed limit in bytes per second (`None` or `0` = unlimited).
    /// Persisted; a running download slows down or speeds up within a second, without a
    /// restart. The global limit still applies on top.
    pub async fn set_speed_limit(&self, id: DownloadId, limit: Option<u64>) -> Result<()> {
        let limit = limit.filter(|&l| l > 0);
        if let Some(r) = self
            .inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
        {
            r.limit.set_limit(limit);
        }
        db(&self.inner.store, move |s| {
            let mut d = s
                .get_download(id)?
                .ok_or(riptide_store::Error::NotFound(id))?;
            d.speed_limit = limit;
            s.update_download(&d)
        })
        .await
    }

    /// The HTTP client new connections use.
    /// The redirect rule for downloads and probes: public-to-private is refused unless the user
    /// allowed it in the settings.
    fn redirect_guard(&self) -> riptide_net::NetGuard {
        riptide_net::NetGuard::off()
            .with_allow_private_redirects(self.settings().allow_private_redirects)
    }

    /// Whether `url` targets a private, loopback, link-local or otherwise special address (an IP
    /// literal, `localhost`, or a name that resolves to one). Downloads the user types are never
    /// checked; addresses that untrusted content supplies (a torrent's web seed, a search result)
    /// are, with this.
    pub async fn is_private_target(&self, url: &str) -> bool {
        match riptide_net::parse_http_url(url) {
            Ok(u) => riptide_net::NetGuard::blocking().is_private_host(&u).await,
            Err(_) => false,
        }
    }

    /// `Err` ([`riptide_net::Error::PrivateAddress`]) when [`Engine::is_private_target`]. Redirects
    /// of the download that follows are held to the usual rule (public to private is refused).
    pub async fn refuse_private_target(&self, url: &str) -> Result<()> {
        let parsed = riptide_net::parse_http_url(url)?;
        riptide_net::NetGuard::blocking()
            .deny_private(&parsed)
            .await?;
        Ok(())
    }

    fn client(&self) -> reqwest::Client {
        self.inner
            .client
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    /// Pauses a queued or running download. For a running one it returns once the transfer
    /// has stopped and the record is `Paused`, so a `resume` right after is never ignored.
    pub async fn pause(&self, id: DownloadId) -> Result<()> {
        self.clear_auto_retry(id);
        if self.is_torrent(id).await? {
            return self.torrent_pause(id).await;
        }
        if self.stop_and_wait(id, StopReason::Pause).await? {
            return Ok(());
        }
        let d = self.get(id).await?.ok_or(Error::NotFound(id))?;
        if d.state == DownloadState::Queued {
            self.transition(id, DownloadState::Paused, None).await?;
        }
        Ok(())
    }

    /// Resumes a paused download or retries a failed one.
    pub async fn resume(&self, id: DownloadId) -> Result<()> {
        self.clear_auto_retry(id);
        self.resume_now(id).await
    }

    /// [`Engine::resume`] without the user-action bookkeeping (the automatic retry uses it).
    async fn resume_now(&self, id: DownloadId) -> Result<()> {
        let d = self.get(id).await?.ok_or(Error::NotFound(id))?;
        if d.kind == DownloadKind::Torrent {
            return self.torrent_resume(id).await;
        }
        if matches!(
            d.state,
            DownloadState::Paused | DownloadState::Failed { .. }
        ) {
            self.transition(id, DownloadState::Queued, None).await?;
            self.inner.wake.notify_one();
        }
        Ok(())
    }

    /// Cancels a download; the row stays, the part file is deleted.
    pub async fn cancel(&self, id: DownloadId) -> Result<()> {
        self.clear_auto_retry(id);
        if let Some(d) = self
            .get(id)
            .await?
            .filter(|d| d.kind == DownloadKind::Torrent)
        {
            return self.torrent_cancel(&d).await;
        }
        if self.stop_and_wait(id, StopReason::Cancel).await? {
            return Ok(());
        }
        let d = self.get(id).await?.ok_or(Error::NotFound(id))?;
        if !d.state.is_terminal() {
            self.transition(id, DownloadState::Cancelled, None).await?;
            delete_partials(&d);
        }
        Ok(())
    }

    /// Removes a download from the list, stopping it first; `delete_file` also removes the
    /// finished file. The part file always goes.
    pub async fn remove(&self, id: DownloadId, delete_file: bool) -> Result<()> {
        self.clear_auto_retry(id);
        self.stop_and_wait(id, StopReason::Remove).await?;
        let d = self.get(id).await?.ok_or(Error::NotFound(id))?;
        if d.kind == DownloadKind::Torrent {
            self.torrent_remove(&d, delete_file).await?;
        }
        delete_partials(&d);
        if delete_file && d.state == DownloadState::Completed && d.kind != DownloadKind::Torrent {
            let _ = std::fs::remove_file(d.final_path());
        }
        db(&self.inner.store, move |s| {
            s.remove_download(id)?;
            for flag in ALL_FLAGS {
                s.set_setting(&flag_key(id, flag), "")?;
            }
            s.remove_setting(&seen::hint_key(id))?;
            Ok(())
        })
        .await?;
        self.emit(Event::Removed { id });
        self.inner.wake.notify_one();
        Ok(())
    }

    /// A record that would be a duplicate of a new request: same URL (fragment ignored, the
    /// original or the final one), or the same file name in the same folder. `filename` and
    /// `dir` default to what [`Engine::add`] would choose. The most recently added match wins;
    /// `None` when the list has nothing like it.
    pub async fn find_duplicate(
        &self,
        url: &Url,
        filename: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<Option<Download>> {
        let mut wanted = riptide_net::split_userinfo(url).0;
        wanted.set_fragment(None);
        let wanted: String = wanted.into();
        let filename = filename
            .map(riptide_core::sanitize_filename)
            .unwrap_or_else(|| {
                riptide_core::sanitize_filename(&riptide_net::probe::filename_from_url(url))
            });
        let dir = dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.settings().dir_for(Category::from_filename(&filename)));
        let all = self.list().await?;
        Ok(all.into_iter().rev().find(|d| {
            comparable_url(&d.url).as_deref() == Some(wanted.as_str())
                || d.final_url.as_deref().and_then(comparable_url).as_deref()
                    == Some(wanted.as_str())
                || (d.filename == filename && d.dir == dir)
        }))
    }

    /// Starts a download over from byte zero: stops it if it is running, deletes the part
    /// file (and the finished file when the record is `Completed` / `Failed` and the file is
    /// the one the engine wrote), forgets validators, size, checksum and segments, and puts
    /// the record in `Queued` — or leaves it `Paused` if it was paused.
    ///
    /// Events: the real transitions (`Paused`, `Queued`) as [`Event::StateChanged`]; when the
    /// state does not change (it was `Paused` or `Queued`) or the record is reborn from a
    /// terminal state, [`Event::Added`] is emitted again so listeners re-read the record.
    pub async fn restart(&self, id: DownloadId) -> Result<()> {
        self.clear_auto_retry(id);
        if self.is_torrent(id).await? {
            // "start over" for a torrent is a full re-check: pieces that verify are kept
            return self.verify_torrent(id).await;
        }
        self.stop_and_wait(id, StopReason::Restart).await?;
        let before = self.get(id).await?.ok_or(Error::NotFound(id))?;
        let was_paused = before.state == DownloadState::Paused;

        // 1. leave the active states along legal arrows (the task has already stopped)
        match before.state {
            DownloadState::Probing | DownloadState::Downloading => {
                self.transition(id, DownloadState::Paused, None).await?;
            }
            DownloadState::Verifying => {
                self.transition(
                    id,
                    DownloadState::Failed { retryable: true },
                    Some("restarted while verifying".into()),
                )
                .await?;
            }
            _ => {}
        }

        // 2. the record: nothing learned from the server survives; segments go. This lands
        //    before the files are touched: a crash in between leaves a stale part file that
        //    the next start truncates, never segments that claim bytes a deleted file had.
        let mut fresh = self.get(id).await?.ok_or(Error::NotFound(id))?;
        let reborn = fresh.state.is_terminal();
        fresh.final_url = None;
        fresh.size = None;
        fresh.supports_ranges = false;
        fresh.etag = None;
        fresh.last_modified = None;
        fresh.content_type = None;
        fresh.checksum_actual = None;
        fresh.error = None;
        fresh.started_at = None;
        fresh.completed_at = None;
        if reborn {
            fresh.state = if was_paused {
                DownloadState::Paused
            } else {
                DownloadState::Queued
            };
        }
        let to_save = fresh.clone();
        db(&self.inner.store, move |s| {
            s.update_download(&to_save)?;
            s.reset_active(id)?;
            s.replace_segments(id, &[])
        })
        .await?;

        // 3. the files: the part file always; the finished file only if it is ours (the
        //    engine renamed it there and, when the size is known, it still has that size)
        let final_path = before.final_path();
        delete_partials(&before);
        let ours = matches!(
            before.state,
            DownloadState::Completed | DownloadState::Failed { .. }
        ) && match std::fs::metadata(&final_path) {
            Ok(meta) => {
                before.size.is_none_or(|s| s == meta.len())
                    && (before.state == DownloadState::Completed || before.size.is_some())
            }
            Err(_) => false,
        };
        if ours {
            std::fs::remove_file(&final_path)?;
        }

        // 4. back in the queue (unless it was paused)
        match fresh.state {
            DownloadState::Paused if !was_paused => {
                self.transition(id, DownloadState::Queued, None).await?;
            }
            DownloadState::Failed { .. } => {
                self.transition(id, DownloadState::Queued, None).await?;
            }
            _ => self.emit(Event::Added { id }),
        }
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Gives an existing download a fresh URL (an expired signed link) and, optionally, fresh
    /// secrets and referrer: the browser extension's `refresh`.
    ///
    /// A running transfer is paused first and queued again afterwards; a failed one is retried;
    /// a paused one stays paused. The record keeps its validator (`ETag` / `Last-Modified`), so
    /// the resume still goes through `If-Range` and restarts from zero if the new URL serves a
    /// different file (never splice two versions). New `secrets` replace the stored ones (the stored
    /// `User-Agent` header is carried over when the new set has none). When the new URL has a
    /// different origin (scheme, host or port) than the old one, the stored cookies, credentials
    /// and custom headers are dropped (only `Referer` and `User-Agent` stay); `referrer` replaces the
    /// stored `Referer` header. Emits [`Event::Added`] (or the state change) so listeners
    /// re-read the record.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] for an unknown id, [`Error::Net`] for a URL that is not `http(s)`,
    /// [`Error::Core`] (`Invalid`) for a completed or cancelled download.
    pub async fn refresh_url(
        &self,
        id: DownloadId,
        url: &str,
        referrer: Option<&str>,
        secrets: Option<Secrets>,
    ) -> Result<()> {
        let parsed = riptide_net::parse_http_url(url)?;
        self.clear_auto_retry(id);
        let (clean_url, url_auth) = riptide_net::split_userinfo(&parsed);
        let before = self.get(id).await?.ok_or(Error::NotFound(id))?;
        if matches!(
            before.state,
            DownloadState::Completed | DownloadState::Cancelled
        ) {
            return Err(Error::Core(riptide_core::Error::Invalid(format!(
                "download {} is {}; there is nothing to refresh",
                id.0, before.state
            ))));
        }
        let was_running = self.stop_and_wait(id, StopReason::Pause).await?;

        let mut fresh = self.get(id).await?.ok_or(Error::NotFound(id))?;
        fresh.url = if url_auth.is_some() {
            clean_url.to_string()
        } else {
            url.to_string()
        };
        fresh.final_url = None;
        fresh.error = None;
        let referrer = referrer.map(str::to_string);
        // stored credentials were captured for the old address; they never follow the record to
        // another origin (secrets are never sent to another site). An unparseable old URL counts as another origin.
        let cross_origin = Url::parse(&before.url).map_or(true, |old| {
            riptide_net::Origin::of(&old) != riptide_net::Origin::of(&parsed)
        });
        db(&self.inner.store, move |s| {
            let changed =
                secrets.is_some() || referrer.is_some() || url_auth.is_some() || cross_origin;
            let mut old = s.secrets(id)?.unwrap_or_default();
            if cross_origin {
                old = without_origin_bound(old);
            }
            let mut next = match secrets {
                Some(mut new) => {
                    let has_ua = new
                        .headers
                        .iter()
                        .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
                    if !has_ua {
                        new.headers.extend(
                            old.headers
                                .iter()
                                .filter(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
                                .cloned(),
                        );
                    }
                    new
                }
                None => old,
            };
            if let (Some((username, password)), None) = (url_auth, &next.auth) {
                next.auth = Some(riptide_core::Credentials { username, password });
            }
            if let Some(r) = referrer {
                next.headers
                    .retain(|(k, _)| !k.eq_ignore_ascii_case("referer"));
                next.headers.push(("Referer".into(), r));
            }
            s.update_download(&fresh)?;
            if changed {
                if next.is_empty() {
                    s.delete_secrets(id)?;
                } else {
                    s.set_secrets(id, &next)?;
                }
            }
            Ok(())
        })
        .await?;

        let now = self.get(id).await?.ok_or(Error::NotFound(id))?;
        match now.state {
            DownloadState::Failed { .. } => {
                self.transition(id, DownloadState::Queued, None).await?;
            }
            DownloadState::Paused if was_running => {
                self.transition(id, DownloadState::Queued, None).await?;
            }
            _ => self.emit(Event::Added { id }),
        }
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Pauses every queued or running download.
    pub async fn pause_all(&self) -> Result<()> {
        let all = self.list().await?;
        // queued items first, so no slot freed below can start (and probe) one of them; each
        // is re-read right before the change and only paused if it is still queued, untouched
        for d in &all {
            if d.state != DownloadState::Queued {
                continue;
            }
            let still_queued = self
                .get(d.id)
                .await?
                .is_some_and(|f| f.state == DownloadState::Queued);
            if still_queued && !self.running_ids().contains(&d.id) {
                self.transition(d.id, DownloadState::Paused, None).await?;
            }
        }
        for d in &all {
            if d.kind == DownloadKind::Torrent
                && matches!(d.state, DownloadState::Probing | DownloadState::Downloading)
            {
                // one torrent that cannot be paused must not leave the HTTP transfers running
                if let Err(e) = self.torrent_pause(d.id).await {
                    tracing::warn!(id = d.id.0, error = %e, "pause all: a torrent was not paused");
                }
            }
        }
        // then stop every running transfer together (including any that started meanwhile)
        let waits: Vec<_> = self
            .running_ids()
            .into_iter()
            .filter_map(|id| self.signal_stop(id, StopReason::Pause).map(|rx| (id, rx)))
            .collect();
        for (id, rx) in waits {
            self.wait_done(id, rx).await?;
        }
        Ok(())
    }

    /// Resumes every paused or failed download.
    pub async fn resume_all(&self) -> Result<()> {
        for d in self.list().await? {
            if matches!(
                d.state,
                DownloadState::Paused | DownloadState::Failed { .. }
            ) {
                self.resume(d.id).await?;
            }
        }
        Ok(())
    }

    /// Stops every running transfer (their records stay `Downloading` and resume on the next
    /// `open`), then returns. Call before the process exits.
    pub async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        let timers: Vec<AutoRetry> = self
            .inner
            .auto_retries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .map(|(_, r)| r)
            .collect();
        for t in timers.into_iter().filter_map(|r| r.timer) {
            t.abort();
        }
        // downloads stay "downloading" in the database across a quit; their time stops here
        let _ = db(&self.inner.store, |s| s.close_active_spans()).await;
        self.shutdown_torrents().await;
        self.inner.wake.notify_one();
        self.inner.sched_wake.notify_one();
        let running: Vec<Running> = {
            let mut map = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
            map.drain().map(|(_, r)| r).collect()
        };
        for r in &running {
            if let Ok(mut reason) = r.reason.lock() {
                *reason = StopReason::Shutdown;
            }
            r.stop.cancel();
        }
        for r in running {
            let _ = r.task.await;
        }
    }

    /// Brings an engine back to life after [`Engine::shutdown`] that was not followed by the end
    /// of the process (an update installer that failed to start): the records the shutdown left
    /// active are queued again, exactly as after a crash, and the scheduler and the schedule
    /// timer run again. The caller restarts the torrent engine ([`Engine::enable_torrents`]) if
    /// it needs it. Does nothing on an engine that is not shut down.
    pub async fn revive(&self) -> Result<()> {
        if !self.inner.shutting_down.load(Ordering::SeqCst) {
            return Ok(());
        }
        // the old scheduler and timer tasks were woken by `shutdown`; give them time to see the
        // flag and end, so the new ones are the only ones
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        db(&self.inner.store, requeue_active).await?;
        self.inner.shutting_down.store(false, Ordering::SeqCst);
        tokio::spawn(self.clone().scheduler());
        tokio::spawn(self.clone().schedule_timer());
        self.inner.wake.notify_one();
        Ok(())
    }

    async fn is_torrent(&self, id: DownloadId) -> Result<bool> {
        Ok(self
            .get(id)
            .await?
            .is_some_and(|d| d.kind == DownloadKind::Torrent))
    }

    /// Ids of downloads with a running task.
    pub fn running_ids(&self) -> Vec<DownloadId> {
        self.inner
            .running
            .lock()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------ internals

    fn emit(&self, event: Event) {
        let _ = self.inner.events.send(event);
    }

    async fn transition(
        &self,
        id: DownloadId,
        to: DownloadState,
        error: Option<String>,
    ) -> Result<DownloadState> {
        let err = error.clone();
        let from = db(&self.inner.store, move |s| {
            s.set_state(id, to, err.as_deref())
        })
        .await?;
        self.emit(Event::StateChanged { id, from, to });
        if let DownloadState::Failed { retryable } = to {
            self.emit(Event::Failed {
                id,
                error: error.unwrap_or_default(),
                retryable,
            });
        }
        Ok(from)
    }

    /// Marks the running task with `reason` and cancels it. `true` if it was running.
    fn stop_running(&self, id: DownloadId, reason: StopReason) -> bool {
        let map = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
        match map.get(&id) {
            Some(r) => {
                if let Ok(mut current) = r.reason.lock() {
                    *current = reason;
                }
                r.stop.cancel();
                true
            }
            None => false,
        }
    }

    /// Marks the running task with `reason`, cancels it and returns its completion signal. The
    /// entry stays in the `running` map until the task has really ended.
    fn signal_stop(
        &self,
        id: DownloadId,
        reason: StopReason,
    ) -> Option<tokio::sync::watch::Receiver<bool>> {
        let map = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
        let r = map.get(&id)?;
        if let Ok(mut current) = r.reason.lock() {
            *current = reason;
        }
        r.stop.cancel();
        Some(r.done.clone())
    }

    /// Test hook: make transfers fail right after the atomic rename (see
    /// `TransferConfig::crash_after_rename`).
    #[doc(hidden)]
    pub fn set_test_crash_after_rename(&self, on: bool) {
        self.inner
            .test_crash_after_rename
            .store(on, Ordering::SeqCst);
    }

    /// Test hook: replace the free-space query (bytes free on the volume of a folder).
    #[doc(hidden)]
    pub fn set_test_free_space(
        &self,
        f: impl Fn(&Path) -> std::io::Result<u64> + Send + Sync + 'static,
    ) {
        if let Ok(mut slot) = self.inner.free_space_fn.write() {
            *slot = Arc::new(f);
        }
    }

    /// Test hook: replace the waits of the automatic retries.
    #[doc(hidden)]
    pub fn set_test_retry_delays(&self, delays: Vec<std::time::Duration>) {
        if let Ok(mut d) = self.inner.retry_delays.write() {
            *d = delays;
        }
    }

    /// Forgets the automatic-retry count and cancels the pending timer of `id`. Every action
    /// of the user calls it, so a manual start always begins a fresh count.
    fn clear_auto_retry(&self, id: DownloadId) {
        let removed = self
            .inner
            .auto_retries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        if let Some(timer) = removed.and_then(|r| r.timer) {
            timer.abort();
        }
    }

    /// After a network failure, before the `Failed` state is written: counts one automatic retry
    /// and returns how long to wait, or `None` when the setting is off, the engine is stopping
    /// or the three attempts are used up. [`Engine::arm_auto_retry`] starts the timer once the
    /// state is saved; a user action in between ([`Engine::clear_auto_retry`]) cancels it.
    fn reserve_auto_retry(&self, id: DownloadId) -> Option<std::time::Duration> {
        if !self.settings().auto_retry || self.inner.shutting_down.load(Ordering::SeqCst) {
            return None;
        }
        let delays = self
            .inner
            .retry_delays
            .read()
            .map(|d| d.clone())
            .unwrap_or_default();
        let mut map = self
            .inner
            .auto_retries
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let entry = map.entry(id).or_default();
        let delay = delays.get(entry.attempts as usize).copied();
        if delay.is_some() {
            entry.attempts += 1;
        }
        delay
    }

    /// Starts the timer reserved by [`Engine::reserve_auto_retry`]; does nothing if the user
    /// acted on the download since (its entry is gone).
    fn arm_auto_retry(&self, id: DownloadId, delay: std::time::Duration) {
        let mut map = self
            .inner
            .auto_retries
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let Some(entry) = map.get_mut(&id) else {
            return;
        };
        tracing::info!(
            id = id.0,
            attempt = entry.attempts,
            ?delay,
            "retry scheduled"
        );
        let engine = self.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            engine.auto_retry_now(id).await;
        });
        if let Some(old) = entry.timer.replace(timer) {
            old.abort();
        }
    }

    /// The timer fired: queue the download again if it is still sitting in `Failed`.
    async fn auto_retry_now(&self, id: DownloadId) {
        if !self.settings().auto_retry || self.inner.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        if let Some(entry) = self
            .inner
            .auto_retries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(&id)
        {
            entry.timer = None;
        }
        let failed = matches!(
            self.get(id).await,
            Ok(Some(d)) if matches!(d.state, DownloadState::Failed { .. })
        );
        if failed {
            if let Err(e) = self.resume_now(id).await {
                tracing::warn!(id = id.0, error = %e, "automatic retry could not queue the download");
            }
        }
    }

    /// Test hook: shorten the stop wait and/or make a stopping task hang for `delay_ms`.
    #[doc(hidden)]
    pub fn set_test_stop_knobs(&self, timeout_ms: u64, delay_ms: u64) {
        self.inner
            .test_stop_timeout_ms
            .store(timeout_ms, Ordering::SeqCst);
        self.inner
            .test_stop_delay_ms
            .store(delay_ms, Ordering::SeqCst);
    }

    /// Waits for the task of `id` to end. If it does not within the timeout the task is
    /// aborted, its entry dropped and the record put in a non-running state, so nothing is left
    /// claiming to download; the caller gets an error and can simply try again.
    async fn wait_done(
        &self,
        id: DownloadId,
        mut done: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        let ms = self.inner.test_stop_timeout_ms.load(Ordering::SeqCst);
        let limit = if ms > 0 {
            std::time::Duration::from_millis(ms)
        } else {
            STOP_TIMEOUT
        };
        // a dropped sender also means the task is gone
        if tokio::time::timeout(limit, done.wait_for(|d| *d))
            .await
            .is_ok()
        {
            return Ok(());
        }
        let stale = self
            .inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        if let Some(r) = stale {
            r.task.abort();
            let _ = r.task.await;
        }
        if let Some(d) = self.get(id).await? {
            let to = match d.state {
                DownloadState::Probing | DownloadState::Downloading => Some(DownloadState::Paused),
                DownloadState::Verifying => Some(DownloadState::Failed { retryable: true }),
                _ => None,
            };
            if let Some(to) = to {
                let _ = self.transition(id, to, None).await;
            }
        }
        self.inner.wake.notify_one();
        Err(Error::Core(riptide_core::Error::Invalid(
            "the transfer did not stop in time; try again".into(),
        )))
    }

    /// Stops the running task of `id` (if any) and waits until it has ended. `true` if one ran.
    async fn stop_and_wait(&self, id: DownloadId, reason: StopReason) -> Result<bool> {
        match self.signal_stop(id, reason) {
            Some(rx) => self.wait_done(id, rx).await.map(|()| true),
            None => Ok(false),
        }
    }

    /// Starts queued downloads while slots are free; sleeps until something changes.
    async fn scheduler(self) {
        loop {
            if self.inner.shutting_down.load(Ordering::SeqCst) {
                return;
            }
            let max_active = self.settings().max_active as usize;
            let started = match self.start_next(max_active).await {
                Ok(started) => started,
                Err(e) => {
                    tracing::error!(error = %e, "scheduler");
                    false
                }
            };
            if !started {
                self.inner.wake.notified().await;
            }
        }
    }

    async fn start_next(&self, max_active: usize) -> Result<bool> {
        let (running_ids, per_queue) = {
            let map = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
            let mut per_queue: HashMap<QueueId, usize> = HashMap::new();
            for r in map.values() {
                *per_queue.entry(r.queue_id).or_default() += 1;
            }
            (map.keys().copied().collect::<Vec<_>>(), per_queue)
        };
        if running_ids.len() >= max_active {
            return Ok(false);
        }
        let (queues, mut queued) = db(&self.inner.store, |s| {
            Ok((
                s.queues()?,
                s.list_downloads_in_state(DownloadState::Queued)?,
            ))
        })
        .await?;
        queued.retain(|d| !running_ids.contains(&d.id) && d.kind != DownloadKind::Torrent);
        let Some(id) = riptide_core::pick_next(&queues, &queued, &per_queue, max_active) else {
            return Ok(false);
        };
        let Some(next) = queued.into_iter().find(|d| d.id == id) else {
            return Ok(false);
        };
        self.inner
            .announced
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&next.queue_id);

        let stop = CancellationToken::new();
        let reason = Arc::new(Mutex::new(StopReason::Pause));
        let engine = self.clone();
        let id = next.id;
        let task_stop = stop.clone();
        let task_reason = Arc::clone(&reason);
        let limit = Arc::new(RateLimiter::new(next.speed_limit));
        let task_limit = Arc::clone(&limit);
        // the map lock is held across the spawn so the task's own `remove` cannot run first
        let (done_tx, done_rx) = tokio::sync::watch::channel(false);
        let mut running = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
        let task = tokio::spawn(async move {
            engine
                .run_download(id, task_stop, task_reason, task_limit)
                .await;
            let _ = done_tx.send(true);
        });
        running.insert(
            id,
            Running {
                stop,
                reason,
                task,
                done: done_rx,
                limit,
                queue_id: next.queue_id,
            },
        );
        Ok(true)
    }

    /// One download from `Queued` to its end state.
    async fn run_download(
        self,
        id: DownloadId,
        stop: CancellationToken,
        reason: Arc<Mutex<StopReason>>,
        limit: Arc<RateLimiter>,
    ) {
        let outcome = {
            let inner = self.run_download_inner(id, stop.clone(), &reason, &limit);
            tokio::pin!(inner);
            tokio::select! {
                r = &mut inner => r,
                () = stop.cancelled() => {
                    // test knob: a task that is slow to react to its stop
                    let delay_ms = self.inner.test_stop_delay_ms.load(Ordering::SeqCst);
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    inner.await
                }
            }
        };
        if let Err(e) = outcome {
            let retryable = match &e {
                Error::Net(n) => n.is_retryable(),
                Error::Media(m) => m.is_retryable(),
                _ => false,
            };
            tracing::warn!(id = id.0, error = %e, retryable, "download failed");
            if is_checksum_mismatch(&e) {
                // the bytes on disk are wrong: a retry must download them again, not verify
                // the same file forever
                self.discard_partial_data(id).await;
            }
            let retry_in = (is_network_failure(&e) && !stop.is_cancelled())
                .then(|| self.reserve_auto_retry(id))
                .flatten();
            let saved = self
                .transition(id, DownloadState::Failed { retryable }, Some(e.to_string()))
                .await
                .is_ok();
            match retry_in {
                Some(delay) if saved => self.arm_auto_retry(id, delay),
                Some(_) => self.clear_auto_retry(id),
                None => {}
            }
        } else {
            // finished or stopped: the next failure starts a fresh count
            self.clear_auto_retry(id);
        }
        {
            let mut map = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
            // only this task's own entry (a successor may already have been inserted)
            if map
                .get(&id)
                .is_some_and(|r| Arc::ptr_eq(&r.reason, &reason))
            {
                map.remove(&id);
            }
        }
        self.inner.wake.notify_one();
    }

    /// Deletes the part file and the segment table of `id`, so the next start begins at zero.
    /// The segments go first (a crash in between leaves a stale file that the next start
    /// truncates, never segments that claim bytes a deleted file had).
    async fn discard_partial_data(&self, id: DownloadId) {
        let Ok(Some(d)) = self.get(id).await else {
            return;
        };
        if let Err(e) = db(&self.inner.store, move |s| s.replace_segments(id, &[])).await {
            tracing::warn!(id = id.0, error = %e, "could not clear the segments of a bad download");
        }
        delete_partials(&d);
    }

    async fn run_download_inner(
        &self,
        id: DownloadId,
        stop: CancellationToken,
        reason: &Arc<Mutex<StopReason>>,
        limit: &Arc<RateLimiter>,
    ) -> Result<()> {
        let store = &self.inner.store;
        let secrets = db(store, move |s| s.secrets(id)).await?.unwrap_or_default();
        let (explicit_name, auto_category, auto_dir) = db(store, move |s| {
            let flag = |name: &str| -> riptide_store::Result<bool> {
                Ok(s.setting(&flag_key(id, name))?.is_some_and(|v| v == "1"))
            };
            Ok((
                flag(FLAG_EXPLICIT_NAME)?,
                flag(FLAG_AUTO_CATEGORY)?,
                flag(FLAG_AUTO_DIR)?,
            ))
        })
        .await?;

        self.transition(id, DownloadState::Probing, None).await?;
        // read the record *after* the transition: `update_download` below writes every column,
        // including `state`, and must not roll it back to Queued
        let mut record = self.get(id).await?.ok_or(Error::NotFound(id))?;
        if record.kind == DownloadKind::Media {
            let flags = MediaFlags {
                explicit_name,
                auto_category,
                auto_dir,
            };
            return self
                .run_media(record, secrets, flags, stop, reason, limit)
                .await;
        }

        let url = riptide_net::parse_http_url(&record.url)?;
        let ctx = Arc::new(
            RequestContext::new(&url)
                .with_secrets(&secrets)
                .with_guard(self.redirect_guard()),
        );
        // one client for the whole run: a proxy change applies from the next start
        let client = self.client();
        let probed = tokio::select! {
            () = stop.cancelled() => {
                return self.stopped(id, reason, Vec::new()).await;
            }
            p = probe(&client, &record.url, &ctx) => p?,
        };

        // existing segments are only worth keeping if the server still serves the same file
        let old_validator = record.etag.clone().or(record.last_modified.clone());
        let mut segments = db(store, move |s| s.segments(id)).await?;
        if old_validator.as_deref() != probed.validator() {
            segments.clear();
        }

        record.final_url = Some(riptide_net::split_userinfo(&probed.final_url).0.to_string());
        record.size = probed.size;
        record.supports_ranges = probed.supports_ranges;
        record.etag = probed.etag.clone();
        record.last_modified = probed.last_modified.clone();
        record.content_type = probed.content_type.clone();
        if !explicit_name && segments.is_empty() {
            record.filename = probed.filename.clone();
            // the server's name decides the category; a category-chosen folder follows it
            if auto_category {
                let category = Category::detect(&record.filename, record.content_type.as_deref());
                let category_id = db(store, move |s| category_row_id(s, category)).await?;
                if category_id.is_some() {
                    record.category_id = category_id;
                }
                if auto_dir {
                    record.dir = self.settings().dir_for(category);
                }
            }
        }
        if segments.is_empty() {
            // a fresh start must not clobber a file that is already there
            self.reserve_file_name(&mut record).await?;
        }
        // the bucket is the truth while running: `set_speed_limit` may have changed it after
        // the record was read
        record.speed_limit = limit.limit();
        let to_save = record.clone();
        db(store, move |s| s.update_download(&to_save)).await?;
        self.emit(Event::Probed {
            id,
            size: record.size,
            supports_ranges: record.supports_ranges,
            filename: record.filename.clone(),
            content_type: record.content_type.clone(),
        });

        // fail fast, with a message the UI can show, instead of filling the disk halfway: the
        // part file is preallocated sparse, so a full volume would only show up as write errors
        let written: u64 = segments.iter().map(|s| s.written).sum();
        let free_fn = self
            .inner
            .free_space_fn
            .read()
            .map(|f| Arc::clone(&f))
            .unwrap_or_else(|p| Arc::clone(&p.into_inner()));
        check_free_space(record.size, written, free_fn(&record.dir))?;

        self.transition(id, DownloadState::Downloading, None)
            .await?;
        let final_path = record.final_path();
        std::fs::create_dir_all(&record.dir)?;
        let plan = TransferPlan {
            download_id: id,
            url: probed.final_url.clone(),
            size: probed.size,
            supports_ranges: probed.supports_ranges,
            validator: probed.validator().map(str::to_owned),
            segments,
            part_path: part_path_for(&final_path),
            final_path: final_path.clone(),
            checksum: record.checksum_expected.clone(),
        };
        let cfg = TransferConfig {
            connections: record.connections,
            rate_limits: vec![Arc::clone(limit), Arc::clone(&self.inner.global_limit)],
            crash_after_rename: self.inner.test_crash_after_rename.load(Ordering::SeqCst),
            ..TransferConfig::default()
        };
        let limiter = self
            .inner
            .limiter
            .read()
            .map(|l| Arc::clone(&l))
            .unwrap_or_else(|_| HostLimiter::new(8));

        let (tx, mut rx) = mpsc::channel::<TransferEvent>(512);
        let forward_engine = self.clone();
        let forward_store = Arc::clone(store);
        let forward_stop = stop.clone();
        let db_failed = Arc::new(AtomicBool::new(false));
        let forward_db_failed = Arc::clone(&db_failed);
        let forwarder = tokio::spawn(async move {
            let mut failures = CheckpointFailures::default();
            while let Some(ev) = rx.recv().await {
                match ev {
                    TransferEvent::Progress {
                        downloaded,
                        total,
                        speed_bps,
                        eta_secs,
                        connections,
                    } => {
                        forward_engine.emit(Event::Progress {
                            id,
                            downloaded,
                            total,
                            speed_bps,
                            eta_secs,
                            connections,
                        });
                    }
                    TransferEvent::SegmentProgress {
                        index,
                        written,
                        len,
                    } => {
                        forward_engine.emit(Event::SegmentProgress {
                            id,
                            index,
                            written,
                            len,
                        });
                    }
                    TransferEvent::Checkpoint { segments } => {
                        let saved = db(&forward_store, move |s| {
                            s.replace_segments(id, &segments)?;
                            s.checkpoint_active(id)
                        })
                        .await;
                        if let Err(e) = &saved {
                            tracing::warn!(id = id.0, error = %e, "progress could not be saved");
                        }
                        if failures.record(saved.is_ok()) {
                            // progress is no longer durable: stop instead of downloading on
                            forward_db_failed.store(true, Ordering::SeqCst);
                            forward_stop.cancel();
                        }
                    }
                    TransferEvent::Verifying => {
                        if let Err(e) = forward_engine
                            .transition(id, DownloadState::Verifying, None)
                            .await
                        {
                            tracing::warn!(id = id.0, error = %e, "could not record the verifying state");
                        }
                    }
                    TransferEvent::Restarted { reason } => {
                        tracing::info!(id = id.0, %reason, "restarted")
                    }
                    TransferEvent::Retrying {
                        index,
                        attempt,
                        error,
                        ..
                    } => {
                        tracing::debug!(id = id.0, index, attempt, %error, "retry");
                    }
                    TransferEvent::Started { .. } | TransferEvent::Completed { .. } => {}
                }
            }
        });

        let final_url = probed.final_url.to_string();
        let result = transfer(client, ctx, plan, cfg, tx, stop.clone(), limiter).await;
        let _ = forwarder.await;

        if db_failed.load(Ordering::SeqCst) && !matches!(result, Ok(Outcome::Completed { .. })) {
            // the transfer stopped because progress could not be written: say so
            tracing::error!(
                id = id.0,
                "database writes failed repeatedly; download stopped"
            );
            self.transition(
                id,
                DownloadState::Failed { retryable: true },
                Some(DB_WRITE_FAILED.into()),
            )
            .await?;
            return Ok(());
        }
        match result {
            Ok(Outcome::Completed { path, .. }) => {
                let current = self
                    .get(id)
                    .await?
                    .map(|d| d.state)
                    .unwrap_or(DownloadState::Downloading);
                if current != DownloadState::Verifying {
                    self.transition(id, DownloadState::Verifying, None).await?;
                }
                // the file on disk is the truth: a stream of unknown length, or a server whose
                // announced size was wrong, leaves the record with a size that is not the file's
                if let (Ok(meta), Some(mut d)) = (std::fs::metadata(&path), self.get(id).await?) {
                    if d.size != Some(meta.len()) {
                        d.size = Some(meta.len());
                        db(&self.inner.store, move |s| s.update_download(&d)).await?;
                    }
                }
                self.mark_downloaded(&path, &secrets, Some(final_url)).await;
                self.transition(id, DownloadState::Completed, None).await?;
                self.spawn_record_seen(id);
                self.emit(Event::Completed { id, path });
                self.announce_drained_after(id).await;
                Ok(())
            }
            Ok(Outcome::Stopped { segments }) => self.stopped(id, reason, segments).await,
            Err(e) => Err(e.into()),
        }
    }

    /// Writes the Mark-of-the-Web stream on a file this run just finished (never on files that
    /// were already on disk), unless the user turned it off. Best effort; see [`motw`]. The
    /// referrer comes from the stored `Referer` header, and only its sanitised form is written.
    async fn mark_downloaded(&self, path: &Path, secrets: &Secrets, host_url: Option<String>) {
        if !self.settings().mark_of_the_web {
            return;
        }
        let referrer = secrets
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("referer"))
            .map(|(_, v)| v.clone());
        let path = path.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || {
            motw::mark_from_internet(&path, referrer.as_deref(), host_url.as_deref())
        })
        .await;
    }

    /// Applies the state a stop asked for.
    async fn stopped(
        &self,
        id: DownloadId,
        reason: &Arc<Mutex<StopReason>>,
        _segments: Vec<Segment>,
    ) -> Result<()> {
        let why = reason.lock().map(|r| *r).unwrap_or(StopReason::Pause);
        let current = self.get(id).await?.map(|d| d.state);
        match why {
            StopReason::Pause => {
                if matches!(
                    current,
                    Some(DownloadState::Probing | DownloadState::Downloading)
                ) {
                    self.transition(id, DownloadState::Paused, None).await?;
                }
            }
            StopReason::Cancel => {
                if current.is_some_and(|s| !s.is_terminal()) {
                    if let Some(d) = self.get(id).await? {
                        delete_partials(&d);
                    }
                    self.transition(id, DownloadState::Cancelled, None).await?;
                }
            }
            // the record is removed by `remove`, reset by `restart`, or left as-is for the
            // next start
            StopReason::Requeue => {
                if matches!(
                    current,
                    Some(DownloadState::Probing | DownloadState::Downloading)
                ) {
                    self.transition(id, DownloadState::Paused, None).await?;
                    self.transition(id, DownloadState::Queued, None).await?;
                }
            }
            StopReason::Remove | StopReason::Shutdown | StopReason::Restart => {}
        }
        Ok(())
    }

    /// A media record from `Probing` to its end state (see the module docs).
    async fn run_media(
        &self,
        mut record: Download,
        secrets: Secrets,
        flags: MediaFlags,
        stop: CancellationToken,
        reason: &Arc<Mutex<StopReason>>,
        limit: &Arc<RateLimiter>,
    ) -> Result<()> {
        let id = record.id;
        let store = &self.inner.store;
        let tool = self.require_media()?;
        let key = record.media_format.clone().unwrap_or_else(|| "best".into());
        let selection = MediaSelection::parse(&key).ok_or_else(|| {
            Error::Core(riptide_core::Error::Invalid(format!(
                "unknown media quality {key:?}"
            )))
        })?;
        let cookies = secrets.cookies.clone();
        // what worked for this record last time is tried first; a fresh record starts without
        // cookies and with yt-dlp's own player clients
        let hint = db(store, move |s| {
            let clients = s
                .setting(&flag_key(id, FLAG_MEDIA_CLIENT))?
                .and_then(|k| PlayerClients::parse(&k));
            let cookies = s.setting(&flag_key(id, FLAG_MEDIA_COOKIES))?.as_deref() == Some("1");
            Ok(clients.map(|clients| Attempt { cookies, clients }))
        })
        .await?;
        let (info, attempt) = tokio::select! {
            () = stop.cancelled() => {
                return self.stopped(id, reason, Vec::new()).await;
            }
            i = tool.probe_resilient(&record.url, cookies.as_deref(), hint) => i?,
        };
        let named = db(store, move |s| {
            Ok(s.setting(&flag_key(id, FLAG_MEDIA_NAMED))?
                .is_some_and(|v| v == "1"))
        })
        .await?;
        if !named {
            if !flags.explicit_name {
                record.filename = media_filename(&info.title, selection);
            }
            if flags.auto_category {
                let category = Category::from_filename(&record.filename);
                let category_id = db(store, move |s| category_row_id(s, category)).await?;
                if category_id.is_some() {
                    record.category_id = category_id;
                }
                if flags.auto_dir {
                    record.dir = self.settings().dir_for(category);
                }
            }
            self.reserve_file_name(&mut record).await?;
        }
        if !info.url.is_empty() {
            record.final_url = Some(Url::parse(&info.url).map_or_else(
                |_| info.url.clone(),
                |u| riptide_net::split_userinfo(&u).0.to_string(),
            ));
        }
        record.size = info
            .choices
            .iter()
            .find(|c| c.key == key)
            .and_then(|c| c.approx_size);
        record.supports_ranges = true;
        record.content_type = Some(
            match selection {
                MediaSelection::AudioMp3 => "audio/mpeg",
                MediaSelection::AudioM4a => "audio/mp4",
                _ => "video/mp4",
            }
            .into(),
        );
        record.speed_limit = limit.limit();
        let to_save = record.clone();
        db(store, move |s| {
            s.update_download(&to_save)?;
            store_attempt(s, id, attempt)?;
            s.set_setting(&flag_key(id, FLAG_MEDIA_NAMED), "1")
        })
        .await?;
        self.emit(Event::Probed {
            id,
            size: record.size,
            supports_ranges: true,
            filename: record.filename.clone(),
            content_type: record.content_type.clone(),
        });
        self.transition(id, DownloadState::Downloading, None)
            .await?;

        let settings = self.settings();
        let rate_limit = match (limit.limit(), settings.global_speed_limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let proxy = (settings.proxy.mode == ProxyMode::Manual)
            .then(|| settings.proxy.url.clone())
            .filter(|u| !u.is_empty());
        let mut job = DownloadJob {
            url: record.url.clone(),
            selection,
            dir: record.dir.clone(),
            stem: media_stem(&record.filename, selection),
            cookies: None,
            rate_limit,
            proxy,
            clients: attempt.clients,
            expected_total: record.size,
        };
        // the download runs the way the probe worked, under the same retry policy: cookies only
        // when the probe needed them, another player client when the site refuses
        let mut ladder = Ladder::new(&record.url, cookies.is_some(), Some(attempt));
        let result = loop {
            let now = ladder.current();
            job.cookies = if now.cookies { cookies.clone() } else { None };
            job.clients = now.clients;
            let (tx, mut rx) = mpsc::channel::<MediaProgress>(64);
            let forward_engine = self.clone();
            let forwarder = tokio::spawn(async move {
                while let Some(p) = rx.recv().await {
                    forward_engine.emit(Event::Progress {
                        id,
                        downloaded: p.downloaded,
                        total: p.total,
                        speed_bps: p.speed_bps,
                        eta_secs: p.eta_secs,
                        connections: 1,
                    });
                }
            });
            let result = tool.download(&job, tx, stop.clone()).await;
            let _ = forwarder.await;
            match result {
                Err(e) => match ladder.next(&e) {
                    Some(next) => {
                        tracing::info!(
                            id = id.0,
                            class = ?e.class(),
                            cookies = next.cookies,
                            clients = next.clients.key(),
                            "media download refused; trying another way"
                        );
                        db(store, move |s| store_attempt(s, id, next)).await?;
                    }
                    None => break Err(e),
                },
                ok => break ok,
            }
        };

        match result? {
            MediaOutcome::Completed { path, size } => {
                let page_url = record.url.clone();
                self.transition(id, DownloadState::Verifying, None).await?;
                let mut done = self.get(id).await?.ok_or(Error::NotFound(id))?;
                if let Some(name) = path.file_name() {
                    done.filename = name.to_string_lossy().into_owned();
                }
                done.size = Some(size);
                let to_save = done.clone();
                db(store, move |s| s.update_download(&to_save)).await?;
                self.emit(Event::Progress {
                    id,
                    downloaded: size,
                    total: Some(size),
                    speed_bps: 0,
                    eta_secs: Some(0),
                    connections: 1,
                });
                self.mark_downloaded(&path, &secrets, Some(page_url)).await;
                self.transition(id, DownloadState::Completed, None).await?;
                self.spawn_record_seen(id);
                self.emit(Event::Completed { id, path });
                self.announce_drained_after(id).await;
                Ok(())
            }
            MediaOutcome::Stopped => self.stopped(id, reason, Vec::new()).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_failed_checkpoint_writes_in_a_row_trip_the_guard_once() {
        let mut f = CheckpointFailures::default();
        for _ in 0..MAX_CHECKPOINT_FAILURES - 1 {
            assert!(!f.record(false));
        }
        // a success in between starts the count over
        assert!(!f.record(true));
        for _ in 0..MAX_CHECKPOINT_FAILURES - 1 {
            assert!(!f.record(false));
        }
        assert!(f.record(false), "the fifth in a row trips");
        assert!(!f.record(false), "and only once");
    }

    #[test]
    fn free_space_is_checked_against_what_is_left_plus_the_margin() {
        let ok = |free: u64| check_free_space(Some(100 * 1024 * 1024), 0, Ok(free));
        assert!(ok(164 * 1024 * 1024).is_ok());
        assert!(matches!(
            ok(164 * 1024 * 1024 - 1),
            Err(Error::DiskFull { .. })
        ));
        // bytes already written do not need room again
        assert!(
            check_free_space(Some(100 * 1024 * 1024), 100 * 1024 * 1024, Ok(DISK_MARGIN)).is_ok()
        );
        // unknown size or an unreadable figure never blocks
        assert!(check_free_space(None, 0, Ok(0)).is_ok());
        let err = std::io::Error::other("no answer");
        assert!(check_free_space(Some(u64::MAX), 0, Err(err)).is_ok());
        // the text keeps the words the UI classifies by, and none of the others
        let msg = Error::DiskFull {
            needed: 1,
            available: 0,
        }
        .to_string()
        .to_lowercase();
        assert!(msg.contains("disk") && msg.contains("space"));
        for other in [
            "checksum",
            "expired",
            "not found",
            "403",
            "declared",
            "content-length",
        ] {
            assert!(!msg.contains(other));
        }
    }

    #[test]
    fn only_referer_and_user_agent_cross_an_origin() {
        let old = Secrets {
            cookies: Some("SID=1".into()),
            headers: vec![
                ("authorization".into(), "Bearer x".into()),
                ("REFERER".into(), "https://a.test/".into()),
                ("User-Agent".into(), "UA".into()),
                ("X-Api-Key".into(), "k".into()),
            ],
            auth: Some(riptide_core::Credentials {
                username: "u".into(),
                password: "p".into(),
            }),
        };
        let kept = without_origin_bound(old);
        assert!(kept.cookies.is_none() && kept.auth.is_none());
        let names: Vec<_> = kept.headers.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["REFERER", "User-Agent"]);
    }

    #[test]
    fn network_failures_are_told_apart_from_the_rest() {
        let status = |code| Error::Net(riptide_net::Error::status(code, None));
        assert!(!is_network_failure(&status(404)));
        assert!(!is_network_failure(&status(503)));
        let io = |kind| Error::Net(riptide_net::Error::Io(std::io::Error::from(kind)));
        assert!(is_network_failure(&io(std::io::ErrorKind::ConnectionReset)));
        assert!(!is_network_failure(&io(
            std::io::ErrorKind::PermissionDenied
        )));
        let exhausted = |last| {
            Error::Net(riptide_net::Error::RetriesExhausted {
                attempts: 5,
                last: Box::new(last),
            })
        };
        assert!(is_network_failure(&exhausted(riptide_net::Error::Io(
            std::io::Error::from(std::io::ErrorKind::TimedOut)
        ))));
        assert!(!is_network_failure(&exhausted(
            riptide_net::Error::ValidatorChanged
        )));
        assert!(!is_network_failure(&Error::Io(std::io::Error::from(
            std::io::ErrorKind::TimedOut
        ))));
        assert!(!is_network_failure(&Error::DiskFull {
            needed: 1,
            available: 0
        }));
        let mismatch = riptide_net::Error::ChecksumMismatch {
            algo: riptide_core::ChecksumAlgo::Sha256,
            expected: "a".into(),
            actual: "b".into(),
        };
        assert!(is_checksum_mismatch(&Error::Net(mismatch)));
        assert!(!is_checksum_mismatch(&status(404)));
    }

    /// An engine over a temp folder with one record parked in `Failed`, ready for the retry
    /// machinery (nothing runs: the record is never queued unless a timer does it).
    async fn engine_with_failed_record() -> (Engine, DownloadId, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(EngineConfig {
            db_path: dir.path().join("t.sqlite"),
            default_dir: dir.path().join("dl"),
            organize_new_install: false,
        })
        .await
        .unwrap();
        let mut request = DownloadRequest::new("http://127.0.0.1:9/f.bin");
        request.start_paused = true;
        let id = engine.add(request).await.unwrap();
        // Paused -> Queued -> Probing -> Failed, written straight to the store
        for to in [
            DownloadState::Queued,
            DownloadState::Probing,
            DownloadState::Failed { retryable: true },
        ] {
            engine.transition(id, to, Some("x".into())).await.unwrap();
        }
        (engine, id, dir)
    }

    async fn state_of(e: &Engine, id: DownloadId) -> DownloadState {
        e.get(id).await.unwrap().unwrap().state
    }

    #[tokio::test]
    async fn at_most_three_retries_and_a_user_action_starts_the_count_over() {
        let (engine, id, _dir) = engine_with_failed_record().await;
        let got: Vec<_> = (0..5).map(|_| engine.reserve_auto_retry(id)).collect();
        assert_eq!(got[..3], AUTO_RETRY_DELAYS.map(Some));
        assert_eq!(got[3..], [None, None]);
        engine.clear_auto_retry(id);
        assert_eq!(engine.reserve_auto_retry(id), Some(AUTO_RETRY_DELAYS[0]));

        // off: never
        engine.clear_auto_retry(id);
        engine
            .set_settings(EngineSettings {
                auto_retry: false,
                ..engine.settings()
            })
            .await
            .unwrap();
        assert_eq!(engine.reserve_auto_retry(id), None);
        engine.shutdown().await;
    }

    #[tokio::test]
    async fn the_timer_requeues_a_failed_download_and_a_user_action_cancels_it() {
        let ms = std::time::Duration::from_millis;
        let (engine, id, _dir) = engine_with_failed_record().await;
        engine.set_test_retry_delays(vec![ms(80); 3]);

        // pause (or any user action) between the failure and the timer: nothing happens
        let delay = engine.reserve_auto_retry(id).unwrap();
        engine.arm_auto_retry(id, delay);
        engine.pause(id).await.unwrap();
        tokio::time::sleep(ms(400)).await;
        assert!(matches!(
            state_of(&engine, id).await,
            DownloadState::Failed { .. }
        ));

        // a user action before the timer is even armed cancels it too
        let delay = engine.reserve_auto_retry(id).unwrap();
        engine.clear_auto_retry(id);
        engine.arm_auto_retry(id, delay);
        tokio::time::sleep(ms(400)).await;
        assert!(matches!(
            state_of(&engine, id).await,
            DownloadState::Failed { .. }
        ));

        // left alone, the timer queues it again
        let mut rx = engine.subscribe();
        let delay = engine.reserve_auto_retry(id).unwrap();
        engine.arm_auto_retry(id, delay);
        let queued = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(Event::StateChanged {
                    to: DownloadState::Queued,
                    ..
                }) = rx.recv().await
                {
                    break;
                }
            }
        })
        .await;
        assert!(queued.is_ok(), "the retry never queued the download");

        // remove cancels a pending timer
        let (other, id2, _dir2) = engine_with_failed_record().await;
        let delay = other.reserve_auto_retry(id2).unwrap();
        other.arm_auto_retry(id2, delay.max(ms(300)));
        other.remove(id2, false).await.unwrap();
        assert!(other.inner.auto_retries.lock().unwrap().is_empty());
        engine.shutdown().await;
        other.shutdown().await;
    }
}
