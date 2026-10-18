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
#![cfg_attr(test, allow(clippy::unwrap_used))]
// `deny`, not `forbid`: `disk::sys` on Windows is one documented Win32 call.
#![deny(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use riptide_core::{
    Category, Download, DownloadId, DownloadKind, DownloadRequest, DownloadState, Event, MediaInfo,
    MediaSelection, Secrets, Segment,
};
use riptide_media::{DownloadJob, MediaOutcome, MediaProgress, YtDlp};
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

pub use disk::free_space;
pub use riptide_net::parse_http_url;

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
    /// Disk.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A background task died.
    #[error("internal: {0}")]
    Internal(String),
}

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
    /// Downloads running at once (the Main queue's limit; per-queue limits come later).
    pub max_active: u32,
    /// `User-Agent` for requests that do not carry a browser's.
    pub user_agent: Option<String>,
    /// Folder per category key (`"video"`, `"documents"`, … — see
    /// [`riptide_core::Category::key`]). A missing or empty entry means `default_dir`.
    /// Only absolute paths under known keys survive [`Engine::set_settings`].
    #[serde(default)]
    pub category_dirs: BTreeMap<String, String>,
    /// Cap in bytes per second across all downloads together; `None` = unlimited. Applies to
    /// running downloads within a second.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub global_speed_limit: Option<u64>,
    /// Which proxy new connections use.
    #[serde(default)]
    pub proxy: ProxySettings,
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
    /// folder if one is set, else `default_dir`.
    pub fn dir_for(&self, category: Category) -> PathBuf {
        self.category_dirs
            .get(category.key())
            .filter(|d| !d.trim().is_empty())
            .map(|d| PathBuf::from(d.trim()))
            .unwrap_or_else(|| PathBuf::from(&self.default_dir))
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
                (Category::from_key(&key).is_some()
                    && !dir.is_empty()
                    && Path::new(dir).is_absolute())
                .then(|| (key, dir.to_owned()))
            })
            .collect();
        // 0 means unlimited, as everywhere else
        self.global_speed_limit = self.global_speed_limit.filter(|&l| l > 0);
        self.proxy.url = self.proxy.url.trim().to_string();
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
            global_speed_limit: None,
            proxy: ProxySettings::default(),
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
/// Every per-download flag, for cleanup.
const ALL_FLAGS: [&str; 4] = [
    FLAG_EXPLICIT_NAME,
    FLAG_AUTO_CATEGORY,
    FLAG_AUTO_DIR,
    FLAG_MEDIA_NAMED,
];

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
    }
}

struct Running {
    stop: CancellationToken,
    reason: Arc<Mutex<StopReason>>,
    task: JoinHandle<()>,
    /// The download's own bandwidth bucket (its `speed_limit`), shared by its connections.
    limit: Arc<RateLimiter>,
}

struct Inner {
    store: Arc<Mutex<Store>>,
    /// Rebuilt when the proxy or user agent changes; running transfers keep the old one.
    client: RwLock<reqwest::Client>,
    limiter: RwLock<Arc<HostLimiter>>,
    /// [`EngineSettings::global_speed_limit`], shared by every connection of every download.
    global_limit: Arc<RateLimiter>,
    events: broadcast::Sender<Event>,
    running: Mutex<HashMap<DownloadId, Running>>,
    wake: Notify,
    settings: RwLock<EngineSettings>,
    shutting_down: AtomicBool,
    /// The bundled yt-dlp (and ffmpeg) for media records; `None` until the app sets it.
    media: RwLock<Option<YtDlp>>,
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

/// Validates `request` and builds the record [`Engine::add`] inserts.
fn prepare_add(request: &DownloadRequest, settings: &EngineSettings) -> Result<Prepared> {
    let url = riptide_net::parse_http_url(&request.url)?;
    let media_selection = match request.kind {
        DownloadKind::Http => None,
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
        request.url.clone(),
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
    d.queue_id = request.queue_id.unwrap_or(riptide_core::QueueId::MAIN);
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
    let mut parsed = Url::parse(url).ok()?;
    parsed.set_fragment(None);
    Some(parsed.into())
}

impl Engine {
    /// Opens the store, loads settings, re-queues downloads a crash left active, and starts
    /// the scheduler.
    pub async fn open(config: EngineConfig) -> Result<Engine> {
        let db_path = config.db_path.clone();
        let store = tokio::task::spawn_blocking(move || Store::open(&db_path))
            .await
            .map_err(|e| Error::Internal(format!("store open task: {e}")))??;
        let store = Arc::new(Mutex::new(store));

        let default_dir = config.default_dir.clone();
        let settings = db(&store, move |s| {
            let loaded = s
                .setting(EngineSettings::KEY)?
                .and_then(|raw| serde_json::from_str::<EngineSettings>(&raw).ok())
                .unwrap_or_else(|| EngineSettings::with_default_dir(&default_dir));
            let mut loaded = loaded.sanitized();
            if loaded.default_dir.is_empty() {
                loaded.default_dir = default_dir.to_string_lossy().into_owned();
            }
            Ok(loaded)
        })
        .await?;

        // crash recovery: anything still "active" belongs back in the queue
        db(&store, |s| {
            for d in s.list_downloads()? {
                match d.state {
                    DownloadState::Probing | DownloadState::Downloading => {
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
        })
        .await?;

        let client = build_client(&settings.client_options())?;
        let (events, _) = broadcast::channel(2048);
        let inner = Arc::new(Inner {
            store,
            client: RwLock::new(client),
            limiter: RwLock::new(HostLimiter::new(settings.per_host as usize)),
            global_limit: Arc::new(RateLimiter::new(settings.global_speed_limit)),
            events,
            running: Mutex::new(HashMap::new()),
            wake: Notify::new(),
            settings: RwLock::new(settings),
            shutting_down: AtomicBool::new(false),
            media: RwLock::new(None),
        });
        let engine = Engine { inner };
        tokio::spawn(engine.clone().scheduler());
        Ok(engine)
    }

    /// Sets (or clears) the yt-dlp that runs media records. Applies from the next start.
    pub fn set_media_tools(&self, tools: Option<YtDlp>) {
        if let Ok(mut m) = self.inner.media.write() {
            *m = tools;
        }
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
        Ok(self.require_media()?.probe(url, cookies).await?)
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
        let settings = settings.sanitized();
        let previous = self.settings();
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
            *l = HostLimiter::new(settings.per_host as usize);
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
        if let Ok(mut s) = self.inner.settings.write() {
            *s = settings;
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
        let mut ctx = RequestContext::new(&parsed).with_referrer(referrer);
        if let Some(s) = secrets {
            ctx = ctx.with_secrets(s);
        }
        let p = probe(&self.client(), url, &ctx).await?;
        Ok(ProbeInfo {
            final_url: p.final_url.to_string(),
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
        for request in &requests {
            let skip = |reason, existing| BatchSkip {
                url: request.url.clone(),
                reason,
                existing,
            };
            if riptide_net::parse_http_url(&request.url).is_err() {
                skipped.push(skip(BatchSkipReason::Invalid, None));
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
    fn client(&self) -> reqwest::Client {
        self.inner
            .client
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    /// Pauses a queued or running download.
    pub async fn pause(&self, id: DownloadId) -> Result<()> {
        if self.stop_running(id, StopReason::Pause) {
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
        let d = self.get(id).await?.ok_or(Error::NotFound(id))?;
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
        if self.stop_running(id, StopReason::Cancel) {
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
        if let Some(running) = self.take_running(id, StopReason::Remove) {
            running.stop.cancel();
            let _ = running.task.await;
        }
        let d = self.get(id).await?.ok_or(Error::NotFound(id))?;
        delete_partials(&d);
        if delete_file && d.state == DownloadState::Completed {
            let _ = std::fs::remove_file(d.final_path());
        }
        db(&self.inner.store, move |s| {
            s.remove_download(id)?;
            for flag in ALL_FLAGS {
                s.set_setting(&flag_key(id, flag), "")?;
            }
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
        let mut wanted = url.clone();
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
        if let Some(running) = self.take_running(id, StopReason::Restart) {
            running.stop.cancel();
            let _ = running.task.await;
        }
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
    /// `User-Agent` header is carried over when the new set has none); `referrer` replaces the
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
        riptide_net::parse_http_url(url)?;
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
        let was_running = match self.take_running(id, StopReason::Pause) {
            Some(running) => {
                running.stop.cancel();
                let _ = running.task.await;
                true
            }
            None => false,
        };

        let mut fresh = self.get(id).await?.ok_or(Error::NotFound(id))?;
        fresh.url = url.to_string();
        fresh.final_url = None;
        fresh.error = None;
        let referrer = referrer.map(str::to_string);
        db(&self.inner.store, move |s| {
            let changed = secrets.is_some() || referrer.is_some();
            let old = s.secrets(id)?.unwrap_or_default();
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
        for d in self.list().await? {
            if matches!(
                d.state,
                DownloadState::Queued | DownloadState::Probing | DownloadState::Downloading
            ) {
                self.pause(d.id).await?;
            }
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
        self.inner.wake.notify_one();
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

    fn take_running(&self, id: DownloadId, reason: StopReason) -> Option<Running> {
        let mut map = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
        let r = map.remove(&id)?;
        if let Ok(mut current) = r.reason.lock() {
            *current = reason;
        }
        Some(r)
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
        let running_ids: Vec<DownloadId> = self.running_ids();
        if running_ids.len() >= max_active {
            return Ok(false);
        }
        let mut queued = db(&self.inner.store, |s| {
            s.list_downloads_in_state(DownloadState::Queued)
        })
        .await?;
        queued.retain(|d| !running_ids.contains(&d.id));
        queued.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.added_at.cmp(&b.added_at))
        });
        let Some(next) = queued.into_iter().next() else {
            return Ok(false);
        };

        let stop = CancellationToken::new();
        let reason = Arc::new(Mutex::new(StopReason::Pause));
        let engine = self.clone();
        let id = next.id;
        let task_stop = stop.clone();
        let task_reason = Arc::clone(&reason);
        let limit = Arc::new(RateLimiter::new(next.speed_limit));
        let task_limit = Arc::clone(&limit);
        // the map lock is held across the spawn so the task's own `remove` cannot run first
        let mut running = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
        let task = tokio::spawn(async move {
            engine
                .run_download(id, task_stop, task_reason, task_limit)
                .await;
        });
        running.insert(
            id,
            Running {
                stop,
                reason,
                task,
                limit,
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
        let outcome = self
            .run_download_inner(id, stop.clone(), &reason, &limit)
            .await;
        if let Err(e) = outcome {
            let retryable = match &e {
                Error::Net(n) => n.is_retryable(),
                Error::Media(m) => m.is_retryable(),
                _ => false,
            };
            tracing::warn!(id = id.0, error = %e, retryable, "download failed");
            let _ = self
                .transition(id, DownloadState::Failed { retryable }, Some(e.to_string()))
                .await;
        }
        self.inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        self.inner.wake.notify_one();
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
        let ctx = Arc::new(RequestContext::new(&url).with_secrets(&secrets));
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

        record.final_url = Some(probed.final_url.to_string());
        record.size = probed.size;
        record.supports_ranges = probed.supports_ranges;
        record.etag = probed.etag.clone();
        record.last_modified = probed.last_modified.clone();
        record.content_type = probed.content_type.clone();
        if !explicit_name && segments.is_empty() {
            record.filename = probed.filename.clone();
            // the server's name decides the category; a category-chosen folder follows it
            if auto_category {
                let category = Category::from_filename(&record.filename);
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
            let unique = riptide_core::unique_path(&record.dir, &record.filename, |p| p.exists());
            if let Some(name) = unique.file_name() {
                record.filename = name.to_string_lossy().into_owned();
            }
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
        let forwarder = tokio::spawn(async move {
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
                        let _ =
                            db(&forward_store, move |s| s.replace_segments(id, &segments)).await;
                    }
                    TransferEvent::Verifying => {
                        let _ = forward_engine
                            .transition(id, DownloadState::Verifying, None)
                            .await;
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

        let result = transfer(client, ctx, plan, cfg, tx, stop.clone(), limiter).await;
        let _ = forwarder.await;

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
                self.transition(id, DownloadState::Completed, None).await?;
                self.emit(Event::Completed { id, path });
                Ok(())
            }
            Ok(Outcome::Stopped { segments }) => self.stopped(id, reason, segments).await,
            Err(e) => Err(e.into()),
        }
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
        let info = tokio::select! {
            () = stop.cancelled() => {
                return self.stopped(id, reason, Vec::new()).await;
            }
            i = tool.probe(&record.url, cookies.as_deref()) => i?,
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
            let unique = riptide_core::unique_path(&record.dir, &record.filename, |p| p.exists());
            if let Some(name) = unique.file_name() {
                record.filename = name.to_string_lossy().into_owned();
            }
        }
        if !info.url.is_empty() {
            record.final_url = Some(info.url.clone());
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
        let job = DownloadJob {
            url: record.url.clone(),
            selection,
            dir: record.dir.clone(),
            stem: media_stem(&record.filename, selection),
            cookies,
            rate_limit,
            proxy,
            expected_total: record.size,
        };
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

        match result? {
            MediaOutcome::Completed { path, size } => {
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
                self.transition(id, DownloadState::Completed, None).await?;
                self.emit(Event::Completed { id, path });
                Ok(())
            }
            MediaOutcome::Stopped => self.stopped(id, reason, Vec::new()).await,
        }
    }
}
