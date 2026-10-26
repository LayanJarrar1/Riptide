//! BitTorrent records in the engine.
//!
//! A torrent is a [`DownloadKind::Torrent`] row in the same list as every other download, so the
//! list, the filters, the notifications and the events work unchanged. What differs is who moves
//! the bytes: not the segment scheduler (it never starts a torrent row) but
//! [`riptide_torrent::TorrentEngine`], which owns the peers, the piece hashing and the files.
//! This module is the bridge between the two:
//!
//! * **Adding** is two steps, so the user can pick files before anything is written:
//!   [`Engine::torrent_preview`] resolves a magnet link or reads a `.torrent` file and answers the
//!   file list plus a token; [`Engine::add_torrent`] takes the token and the selection, inserts
//!   the row (state `Probing`, never `Queued`) and starts the torrent.
//! * **State.** Every torrent event (and the snapshot taken at start-up) is turned into legal
//!   [`DownloadState`] transitions by walking the transition table (`Downloading` -> `Verifying`
//!   -> `Completed`; `Paused` -> `Queued` -> `Probing` -> `Downloading`; ...). A torrent that is
//!   seeding by the user's choice is `Completed` with the `seeding` flag set.
//! * **Events.** [`Event::Progress`] and [`Event::TorrentStats`] come out at most twice a second
//!   per torrent (the torrent engine's own tick), far below the 10 per second budget.
//! * **Persistence.** The torrent engine keeps its own crash-safe registry; this module mirrors
//!   what the list needs into the `torrents` table (at most once a second), so the list is right
//!   before the torrent engine has re-checked a single piece after a restart or `kill -9`.
//! * **Hostile names.** File names come from [`riptide_torrent::paths`] (the same rules as HTTP
//!   downloads, plus confinement to the chosen folder); this module only joins them to the
//!   folder and never builds a path from anything else.
//!
//! Torrents bypass the queues' concurrency limits (they have their own peer budget) and are not
//! affected by the per-download speed limit; the upload cap and the seeding rules are in
//! [`TorrentOptions`].

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riptide_core::{
    Category, Download, DownloadId, DownloadKind, DownloadState, Event, TorrentAdd,
    TorrentFileInfo, TorrentInfo, TorrentLive, TorrentPreviewFile, TorrentPreviewInfo,
    TorrentSourceRequest,
};
use riptide_torrent::{
    AddOptions, SeedingPolicy, TorrentEngine, TorrentEvent, TorrentPreview, TorrentSettings,
    TorrentSnapshot, TorrentSource, TorrentState,
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::{db, now_millis, Engine, Error, Result};

/// Previews kept for the add dialog.
const MAX_PREVIEWS: usize = 8;
/// How long a preview stays valid.
const PREVIEW_TTL: Duration = Duration::from_secs(30 * 60);
/// The mirror in the `torrents` table is written at most this often per torrent.
const PERSIST_EVERY: Duration = Duration::from_secs(1);

/// Settings -> Torrents. Part of [`crate::EngineSettings`].
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentOptions {
    /// Keep uploading after a download is complete. **Off by default**: a finished torrent stops
    /// all network activity.
    pub seeding: bool,
    /// Stop seeding at this upload ratio, in percent (`100` = 1.0; `0` = no ratio limit).
    pub seed_ratio_percent: u32,
    /// Stop seeding after this many minutes (`0` = no time limit).
    pub seed_minutes: u32,
    /// Upload cap in bytes per second, also while downloading (`None` = unlimited).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub upload_limit: Option<u64>,
    /// Find peers through the mainline DHT (ignored for private torrents). Applies after a restart.
    pub dht: bool,
    /// TCP port for incoming peers (`0` = pick one). Applies after a restart.
    pub listen_port: u16,
}

impl Default for TorrentOptions {
    fn default() -> Self {
        TorrentOptions {
            seeding: false,
            seed_ratio_percent: 100,
            seed_minutes: 24 * 60,
            upload_limit: None,
            dht: true,
            listen_port: 0,
        }
    }
}

impl TorrentOptions {
    /// Keeps only values the torrent engine accepts.
    pub(crate) fn sanitized(mut self) -> Self {
        self.upload_limit = self.upload_limit.filter(|l| *l > 0);
        self.seed_ratio_percent = self.seed_ratio_percent.min(100_000);
        self.seed_minutes = self.seed_minutes.min(60 * 24 * 365);
        self
    }

    fn policy(&self) -> SeedingPolicy {
        SeedingPolicy {
            enabled: self.seeding,
            max_ratio: (self.seed_ratio_percent > 0)
                .then(|| self.seed_ratio_percent as f32 / 100.0),
            max_minutes: (self.seed_minutes > 0).then_some(self.seed_minutes),
        }
    }

    fn upload_bps(&self) -> Option<u32> {
        self.upload_limit
            .map(|l| u32::try_from(l).unwrap_or(u32::MAX))
    }
}

/// What [`Engine::torrent_web_seed_add`] did.
#[derive(Debug, Clone, Default)]
pub struct WebSeedAdded {
    /// The new downloads.
    pub added: Vec<DownloadId>,
    /// Files not added: an address that is not http(s), or one that targets a private network.
    pub refused: usize,
}

/// A preview waiting for the user's choice.
pub(crate) struct Pending {
    token: String,
    preview: TorrentPreview,
    made: Instant,
}

/// A last look at the torrent engine's settings, kept so a restart applies it again.
pub(crate) type Tweak = Arc<dyn Fn(&mut TorrentSettings) + Send + Sync>;

/// How long nothing needs the torrent engine before it is stopped: the listening port, the DHT
/// node and every peer connection are closed, as the privacy promise says a finished torrent does.
pub(crate) const DEFAULT_IDLE_GRACE: Duration = Duration::from_secs(5);

/// Keeps the torrent engine from being stopped while a command is using it.
pub(crate) struct BusyGuard(Arc<TorrentRuntime>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The running torrent engine plus the bookkeeping that ties it to download rows.
pub(crate) struct TorrentRuntime {
    pub(crate) engine: Arc<TorrentEngine>,
    /// info-hash -> download row.
    ids: Mutex<HashMap<String, DownloadId>>,
    /// Commands currently using the torrent engine; it is not stopped while any does.
    busy: std::sync::atomic::AtomicUsize,
    /// Serialises adding, so two torrents added together cannot both plan the same free name.
    adding: tokio::sync::Mutex<()>,
    /// Serialises state walking, so a command and the event bridge never interleave.
    sync: tokio::sync::Mutex<()>,
    /// Last write of the `torrents` mirror per row.
    persisted: Mutex<HashMap<DownloadId, Instant>>,
    /// Lifetime upload total stored when this run first saw the torrent; the torrent engine counts
    /// from zero at every start.
    base_uploaded: Mutex<HashMap<String, u64>>,
    bridge: Mutex<Option<JoinHandle<()>>>,
    /// Stops the engine when nothing needs it.
    idle: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl TorrentRuntime {
    fn id_of(&self, hash: &str) -> Option<DownloadId> {
        lock(&self.ids).get(hash).copied()
    }
}

/// The states to pass through to get from `from` to `to` along legal arrows (excluding `from`),
/// shortest first; `None` when `to` is not reachable.
pub(crate) fn walk(from: DownloadState, to: DownloadState) -> Option<Vec<DownloadState>> {
    if from == to {
        return Some(Vec::new());
    }
    let mut prev: HashMap<DownloadState, DownloadState> = HashMap::new();
    let mut queue = VecDeque::from([from]);
    while let Some(s) = queue.pop_front() {
        for n in s.successors() {
            if n == DownloadState::Cancelled || n == from || prev.contains_key(&n) {
                continue;
            }
            prev.insert(n, s);
            if n == to {
                let mut path = vec![n];
                let mut cur = n;
                while let Some(p) = prev.get(&cur) {
                    if *p == from {
                        break;
                    }
                    path.push(*p);
                    cur = *p;
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(n);
        }
    }
    None
}

/// What the row's state should be for this snapshot; `None` = leave it.
fn target_state(snap: &TorrentSnapshot, current: DownloadState) -> Option<DownloadState> {
    match &snap.state {
        // checking pieces on disk after a start: keep whatever the row shows, except a row that
        // was only just created or woken
        TorrentState::Checking => matches!(current, DownloadState::Queued | DownloadState::Paused)
            .then_some(DownloadState::Probing),
        TorrentState::Downloading => Some(DownloadState::Downloading),
        TorrentState::Seeding | TorrentState::Completed => Some(DownloadState::Completed),
        TorrentState::Paused => Some(DownloadState::Paused),
        TorrentState::Failed { .. } => Some(DownloadState::Failed { retryable: true }),
    }
}

fn is_failed(s: DownloadState) -> bool {
    matches!(s, DownloadState::Failed { .. })
}

fn files_of(preview: &TorrentPreview) -> Vec<TorrentFileInfo> {
    preview
        .files
        .iter()
        .filter(|f| !f.padding)
        .map(|f| TorrentFileInfo {
            index: u32::try_from(f.index).unwrap_or(u32::MAX),
            path: f.path.to_string_lossy().replace('\\', "/"),
            len: f.len,
        })
        .collect()
}

/// The name of what the torrent leaves in the folder: its single file, or the folder all files
/// share. Derived from the planned (sanitised, collision-free) layout, not from the metadata.
fn root_name(files: &[TorrentFileInfo], fallback: &str) -> String {
    let first = files.first().map(|f| f.path.as_str()).unwrap_or(fallback);
    if files.len() == 1 {
        return first.to_string();
    }
    let top = first.split('/').next().unwrap_or(first);
    if files
        .iter()
        .all(|f| f.path.split('/').next() == Some(top) && f.path.contains('/'))
    {
        top.to_string()
    } else {
        fallback.to_string()
    }
}

/// The canonical link kept in the list: the info-hash and nothing else. Trackers in a magnet
/// link can carry a private passkey, so they are never stored.
fn canonical_url(hash: &str) -> String {
    format!("magnet:?xt=urn:btih:{hash}")
}

fn to_selection(only: &Option<Vec<u32>>) -> Option<Vec<usize>> {
    only.as_ref()
        .map(|v| v.iter().map(|i| *i as usize).collect())
}

impl Engine {
    pub(crate) fn torrent_rt(&self) -> Option<Arc<TorrentRuntime>> {
        self.inner
            .torrents
            .read()
            .ok()
            .and_then(|t| t.as_ref().map(Arc::clone))
    }

    /// The torrent engine, started if it is not running, held (not stopped for being idle) until
    /// the guard is dropped.
    async fn acquire_torrents(&self) -> Result<(Arc<TorrentRuntime>, BusyGuard)> {
        loop {
            {
                let _l = self.inner.torrents_starting.lock().await;
                if let Some(rt) = self.torrent_rt() {
                    rt.busy.fetch_add(1, Ordering::SeqCst);
                    return Ok((Arc::clone(&rt), BusyGuard(rt)));
                }
            }
            self.start_torrents(None).await?;
        }
    }

    /// Like [`Engine::acquire_torrents`] but never starts the engine: `None` when it is off.
    async fn try_acquire_torrents(&self) -> Option<(Arc<TorrentRuntime>, BusyGuard)> {
        let _l = self.inner.torrents_starting.lock().await;
        let rt = self.torrent_rt()?;
        rt.busy.fetch_add(1, Ordering::SeqCst);
        Some((Arc::clone(&rt), BusyGuard(rt)))
    }

    /// How long nothing needs the torrent engine before it stops (5 seconds by default).
    pub fn set_torrent_idle_grace(&self, grace: Duration) {
        self.inner.torrent_idle_grace_ms.store(
            u64::try_from(grace.as_millis()).unwrap_or(u64::MAX),
            Ordering::SeqCst,
        );
    }

    /// Names other downloads (not torrents: the torrent engine knows its own) have claimed in
    /// their folders, whether or not a file exists yet; a torrent plans around them.
    async fn http_name_claims(&self) -> Result<Vec<PathBuf>> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter(|d| {
                d.kind != DownloadKind::Torrent
                    && !matches!(d.state, DownloadState::Completed | DownloadState::Cancelled)
            })
            .map(|d| d.dir.join(&d.filename))
            .collect())
    }

    /// Runs `f` on the stored torrent state while no torrent engine is running (`None` when one
    /// is: the caller then goes through it). Holds the start-up lock, so the engine cannot start
    /// halfway through and read a registry that is being edited.
    async fn with_torrents_off<T: Send + 'static>(
        &self,
        f: impl FnOnce(PathBuf) -> riptide_torrent::Result<T> + Send + 'static,
    ) -> Result<Option<T>> {
        let _starting = self.inner.torrents_starting.lock().await;
        if self.torrents_enabled() {
            return Ok(None);
        }
        let dir = self.inner.data_dir.join("torrents");
        let out = tokio::task::spawn_blocking(move || f(dir))
            .await
            .map_err(|e| Error::Internal(format!("torrent state task: {e}")))?;
        match out {
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(e.into()),
        }
    }

    /// Starts the torrent engine at launch only if a torrent needs the network right now: one
    /// that is downloading, queued or being checked, or a finished one that is seeding by the
    /// user choice. Returns whether it is running. Paused, finished and failed torrents leave
    /// no listening port and no DHT node behind; the engine starts the moment one is resumed,
    /// verified or added.
    pub async fn enable_torrents_if_active(&self) -> Result<bool> {
        self.enable_torrents_if_active_with(|_| {}).await
    }

    /// Like [`Engine::enable_torrents_if_active`] with a last look at the torrent engine settings.
    pub async fn enable_torrents_if_active_with(
        &self,
        tweak: impl Fn(&mut TorrentSettings) + Send + Sync + 'static,
    ) -> Result<bool> {
        if self.torrents_enabled() {
            return Ok(true);
        }
        let seeding_allowed = self.settings().torrent.seeding;
        let rows = db(&self.inner.store, |s| s.list_torrents()).await?;
        let mut needed = false;
        for t in &rows {
            let Some(d) = self.get(t.download_id).await? else {
                continue;
            };
            needed |= match d.state {
                DownloadState::Queued
                | DownloadState::Probing
                | DownloadState::Downloading
                | DownloadState::Verifying => true,
                DownloadState::Completed => seeding_allowed && t.seeding,
                _ => false,
            };
        }
        if needed {
            self.enable_torrents_with(tweak).await?;
        }
        Ok(needed)
    }

    /// `true` once [`Engine::enable_torrents`] has run.
    pub fn torrents_enabled(&self) -> bool {
        self.torrent_rt().is_some()
    }

    /// Starts the BitTorrent engine (state under `<data dir>/torrents`), resumes the torrents of
    /// the previous run and begins mirroring their progress into the list. Safe to call twice.
    pub async fn enable_torrents(&self) -> Result<()> {
        self.start_torrents(None).await
    }

    /// Like [`Engine::enable_torrents`] with a last look at the torrent engine's settings (tests
    /// turn DHT, trackers and LAN discovery off and listen on loopback).
    pub async fn enable_torrents_with(
        &self,
        tweak: impl Fn(&mut TorrentSettings) + Send + Sync + 'static,
    ) -> Result<()> {
        self.start_torrents(Some(Arc::new(tweak))).await
    }

    async fn start_torrents(&self, tweak: Option<Tweak>) -> Result<()> {
        let _starting = self.inner.torrents_starting.lock().await;
        if self.torrents_enabled() {
            return Ok(());
        }
        // a restart (after the engine stopped for being idle) applies the same last look again
        let tweak = {
            let mut slot = lock(&self.inner.torrent_tweak);
            if tweak.is_some() {
                *slot = tweak;
            }
            slot.clone()
        };
        let settings = self.settings();
        let mut ts = TorrentSettings::new(
            self.inner.data_dir.join("torrents"),
            PathBuf::from(&settings.default_dir),
        );
        let o = settings.torrent.clone().sanitized();
        ts.seeding = o.policy();
        ts.upload_limit_bps = o.upload_bps();
        ts.dht = o.dht;
        ts.listen_port = o.listen_port;
        if let Some(t) = &tweak {
            t(&mut ts);
        }

        // A registry entry whose row is gone (removed or cancelled while this engine was off) is
        // a ghost: it must not be restored, or it would download and upload with no row to show
        // for it. Dropped before the engine opens the registry.
        let rows = db(&self.inner.store, |s| s.list_torrents()).await?;
        let mut keep = std::collections::HashSet::new();
        for t in &rows {
            if self
                .get(t.download_id)
                .await?
                .is_some_and(|d| d.state != DownloadState::Cancelled)
            {
                keep.insert(t.info_hash.clone());
            }
        }
        let state_dir = ts.state_dir.clone();
        let dropped = tokio::task::spawn_blocking(move || {
            riptide_torrent::offline::retain(&state_dir, &keep)
        })
        .await
        .map_err(|e| Error::Internal(format!("torrent state task: {e}")))??;
        if !dropped.is_empty() {
            tracing::warn!(
                count = dropped.len(),
                "dropped torrents that had no row in the list"
            );
        }

        let engine = TorrentEngine::start(ts).await?;

        let rt = Arc::new(TorrentRuntime {
            engine,
            ids: Mutex::new(
                rows.iter()
                    .map(|t| (t.info_hash.clone(), t.download_id))
                    .collect(),
            ),
            busy: std::sync::atomic::AtomicUsize::new(0),
            adding: tokio::sync::Mutex::new(()),
            sync: tokio::sync::Mutex::new(()),
            persisted: Mutex::new(HashMap::new()),
            base_uploaded: Mutex::new(
                rows.iter()
                    .map(|t| (t.info_hash.clone(), t.uploaded))
                    .collect(),
            ),
            bridge: Mutex::new(None),
            idle: Mutex::new(None),
        });
        if let Ok(mut slot) = self.inner.torrents.write() {
            *slot = Some(Arc::clone(&rt));
        }

        // rows whose torrent state vanished (state folder deleted) cannot run any more
        for t in &rows {
            let Some(d) = self.get(t.download_id).await? else {
                continue;
            };
            if d.state.is_terminal() {
                continue;
            }
            match rt.engine.snapshot(&t.info_hash) {
                Ok(snap) => self.apply_snapshot(&rt, snap, false).await,
                Err(_) => {
                    let _ = self
                        .advance(
                            &rt,
                            t.download_id,
                            DownloadState::Failed { retryable: false },
                            Some("the torrent's state is missing; add it again".into()),
                        )
                        .await;
                }
            }
        }

        // subscribe before the first event can be missed, then run until shutdown
        let mut events = rt.engine.subscribe();
        let engine = self.clone();
        let bridge_rt = Arc::clone(&rt);
        let handle = tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(ev) => engine.on_torrent_event(&bridge_rt, ev).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        for snap in bridge_rt.engine.list() {
                            engine.apply_snapshot(&bridge_rt, snap, true).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        *lock(&rt.bridge) = Some(handle);
        let idle_engine = self.clone();
        let idle_rt = Arc::clone(&rt);
        *lock(&rt.idle) = Some(tokio::spawn(async move {
            idle_engine.watch_idle(idle_rt).await;
        }));
        tracing::info!(torrents = rows.len(), "torrent engine started");
        Ok(())
    }

    /// Whether any torrent needs the network now: checking, downloading, or seeding.
    fn torrents_need_network(rt: &TorrentRuntime) -> bool {
        rt.busy.load(Ordering::SeqCst) > 0
            || rt.engine.list().iter().any(|s| {
                matches!(
                    s.state,
                    TorrentState::Checking | TorrentState::Downloading | TorrentState::Seeding
                )
            })
    }

    /// Stops the torrent engine once nothing has needed it for the grace period.
    async fn watch_idle(&self, rt: Arc<TorrentRuntime>) {
        let mut idle_since: Option<Instant> = None;
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let current = self.torrent_rt();
            if !current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &rt)) {
                return; // stopped or replaced by someone else
            }
            if Self::torrents_need_network(&rt) {
                idle_since = None;
                continue;
            }
            let grace =
                Duration::from_millis(self.inner.torrent_idle_grace_ms.load(Ordering::SeqCst));
            let since = *idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= grace {
                if self.stop_if_idle(&rt).await {
                    return;
                }
                idle_since = None;
            }
        }
    }

    /// Stops the engine if it is still the running one, still idle and unused; true when stopped.
    /// Everything that starts, uses or stops it takes the same lock, so a command that arrives
    /// now either finds it running (and keeps it) or starts it again afterwards.
    async fn stop_if_idle(&self, rt: &Arc<TorrentRuntime>) -> bool {
        let _l = self.inner.torrents_starting.lock().await;
        let is_current = self.torrent_rt().is_some_and(|c| Arc::ptr_eq(&c, rt));
        if !is_current
            || Self::torrents_need_network(rt)
            || self.inner.shutting_down.load(Ordering::SeqCst)
        {
            return false;
        }
        // the rows and the lifetime upload total are written before the session goes away
        lock(&rt.persisted).clear();
        for snap in rt.engine.list() {
            self.apply_snapshot(rt, snap, true).await;
        }
        if let Ok(mut slot) = self.inner.torrents.write() {
            *slot = None;
        }
        let bridge = lock(&rt.bridge).take();
        rt.engine.shutdown().await;
        if let Some(h) = bridge {
            h.abort();
        }
        tracing::info!("torrent engine stopped: nothing needs the network");
        true
    }

    /// The settings the torrent engine runs with right now (`None` until torrents are enabled).
    pub fn torrent_settings(&self) -> Option<TorrentSettings> {
        self.torrent_rt().map(|rt| rt.engine.settings())
    }

    /// Pushes Settings -> Torrents into the running torrent engine (seeding rules and the upload
    /// cap apply at once; DHT and the port at the next start).
    pub(crate) fn apply_torrent_options(&self, o: &TorrentOptions) {
        let Some(rt) = self.torrent_rt() else { return };
        let o = o.clone().sanitized();
        if let Err(e) = rt.engine.set_seeding(o.policy()) {
            tracing::warn!(error = %e, "seeding settings refused");
        }
        rt.engine.set_upload_limit(o.upload_bps());
    }

    /// Stops the torrent engine (called by [`Engine::shutdown`]).
    pub(crate) async fn shutdown_torrents(&self) {
        let _l = self.inner.torrents_starting.lock().await;
        let rt = self.inner.torrents.write().ok().and_then(|mut t| t.take());
        if let Some(rt) = rt {
            if let Some(h) = lock(&rt.idle).take() {
                h.abort();
            }
            let handle = lock(&rt.bridge).take();
            rt.engine.shutdown().await;
            if let Some(h) = handle {
                h.abort();
            }
        }
    }

    /// Resolves a magnet link or `.torrent` file into the list of files it would write, without
    /// adding anything. A magnet link contacts peers until the metadata arrives (two minutes at
    /// most).
    pub async fn torrent_preview(
        &self,
        source: TorrentSourceRequest,
        dir: Option<PathBuf>,
    ) -> Result<TorrentPreviewInfo> {
        let (rt, _busy) = self.acquire_torrents().await?;
        let dir = dir.unwrap_or_else(|| self.settings().torrent_dir());
        let opts = AddOptions {
            dir: Some(dir.clone()),
            reserved: self.http_name_claims().await?,
            ..Default::default()
        };
        let source = match source {
            TorrentSourceRequest::Magnet { uri } => TorrentSource::Magnet(uri),
            TorrentSourceRequest::File { path } => TorrentSource::File(path),
        };
        let mut preview = rt.engine.preview(&source, &opts).await?;
        let existing = {
            let hash = preview.info_hash.clone();
            db(&self.inner.store, move |s| {
                Ok(match s.torrent_by_hash(&hash)? {
                    Some(t) => s
                        .get_download(t.download_id)?
                        .filter(|d| d.state != DownloadState::Cancelled)
                        .map(|d| d.id),
                    None => None,
                })
            })
            .await?
        };
        // a torrent that is already in the list is shown under its real names: the planner would
        // otherwise rename every file (`name (1).ext`) because its own files are in the folder
        if existing.is_some() && matches!(source, TorrentSource::File(_)) {
            let adopt = AddOptions {
                adopt_existing: true,
                ..opts.clone()
            };
            if let Ok(again) = rt.engine.preview(&source, &adopt).await {
                preview = again;
            }
        }
        // one pass of stat calls, off the async runtime
        let on_disk: Vec<bool> = {
            let paths: Vec<PathBuf> = preview.files.iter().map(|f| dir.join(&f.path)).collect();
            tokio::task::spawn_blocking(move || paths.iter().map(|p| p.exists()).collect())
                .await
                .map_err(|e| Error::Internal(format!("preview task: {e}")))?
        };
        let files = preview
            .files
            .iter()
            .zip(on_disk)
            .filter(|(f, _)| !f.padding)
            .map(|(f, exists)| TorrentPreviewFile {
                index: u32::try_from(f.index).unwrap_or(u32::MAX),
                path: f.path.to_string_lossy().replace('\\', "/"),
                len: f.len,
                exists,
            })
            .collect();
        let token = format!(
            "{}-{}",
            preview.info_hash,
            self.inner.torrent_next_token.fetch_add(1, Ordering::SeqCst)
        );
        let info = TorrentPreviewInfo {
            token: token.clone(),
            info_hash: preview.info_hash.clone(),
            name: preview.name.clone(),
            files,
            total_bytes: preview.total_bytes,
            private: preview.private,
            existing,
            web_seeds: preview.web_seeds.clone(),
        };
        let mut pend = lock(&self.inner.torrent_previews);
        pend.retain(|p| p.made.elapsed() < PREVIEW_TTL);
        pend.push_back(Pending {
            token,
            preview,
            made: Instant::now(),
        });
        while pend.len() > MAX_PREVIEWS {
            pend.pop_front();
        }
        Ok(info)
    }

    /// Adds the torrent behind a [`Engine::torrent_preview`] token with the user's file choice.
    /// The row is listed at once (state `Probing`, or `Paused` for `start_paused`); the torrent
    /// engine then checks anything already in the folder and downloads the rest.
    pub async fn add_torrent(&self, add: TorrentAdd) -> Result<DownloadId> {
        let (rt, _busy) = self.acquire_torrents().await?;
        let _adding = rt.adding.lock().await;
        let preview = {
            let mut pend = lock(&self.inner.torrent_previews);
            let at = pend
                .iter()
                .position(|p| p.token == add.token && p.made.elapsed() < PREVIEW_TTL);
            match at {
                Some(i) => pend.remove(i).map(|p| p.preview),
                None => None,
            }
        }
        .ok_or_else(|| {
            Error::Core(riptide_core::Error::Invalid(
                "the torrent preview has expired; open the torrent again".into(),
            ))
        })?;

        let settings = self.settings();
        let dir = add.dir.clone().unwrap_or_else(|| settings.torrent_dir());
        let opts = AddOptions {
            dir: Some(dir.clone()),
            only_files: to_selection(&add.only_files),
            sequential: add.sequential,
            start_paused: add.start_paused,
            peers: add.peers.clone(),
            reserved: self.http_name_claims().await?,
            ..Default::default()
        };
        // the layout for the chosen folder (collisions are resolved against it)
        let source = TorrentSource::Bytes(preview.torrent_bytes.clone());
        let planned = rt.engine.preview(&source, &opts).await?;
        // this one plan is what the row records and what the torrent engine writes
        let opts = AddOptions {
            layout: Some(planned.files.iter().map(|f| f.path.clone()).collect()),
            ..opts
        };
        let files = files_of(&planned);
        let selected: Option<Vec<u32>> = add.only_files.clone().map(|mut v| {
            v.sort_unstable();
            v.dedup();
            v
        });
        let hash = planned.info_hash.clone();

        let mut d = Download::new(
            DownloadId(0),
            canonical_url(&hash),
            root_name(&files, &planned.name),
            absolute(&dir),
            now_millis(),
        );
        d.kind = DownloadKind::Torrent;
        d.state = if add.start_paused {
            DownloadState::Paused
        } else {
            DownloadState::Probing
        };
        d.queue_id = add.queue_id.unwrap_or(riptide_core::QueueId::MAIN);
        d.connections = 1;
        let info = TorrentInfo {
            download_id: DownloadId(0),
            info_hash: hash.clone(),
            name: planned.name.clone(),
            files,
            selected,
            sequential: add.sequential,
            seeding: false,
            uploaded: 0,
            downloaded: 0,
            peers: 0,
        };
        d.size = Some(info.selected_bytes());

        let row = (d, info);
        let id = db(&self.inner.store, move |s| {
            let (mut d, mut info) = row;
            if let Some(old) = s.torrent_by_hash(&info.info_hash)? {
                let live = s
                    .get_download(old.download_id)?
                    .is_some_and(|x| x.state != DownloadState::Cancelled);
                if live {
                    return Err(riptide_store::Error::Corrupt(
                        "this torrent is already in the list".into(),
                    ));
                }
                // a cancelled torrent makes room for its info-hash
                s.remove_download(old.download_id)?;
            }
            d.category_id = crate::category_row_id(s, Category::Other)?;
            let id = s.insert_download(&d)?;
            info.download_id = id;
            s.insert_torrent(&info)?;
            Ok(id)
        })
        .await
        .map_err(|e| match e {
            Error::Store(riptide_store::Error::Corrupt(m)) => {
                Error::Core(riptide_core::Error::Invalid(m))
            }
            other => other,
        })?;
        lock(&rt.ids).insert(hash.clone(), id);
        lock(&rt.base_uploaded).insert(hash.clone(), 0);

        if let Err(e) = rt.engine.add(source, opts).await {
            lock(&rt.ids).remove(&hash);
            let _ = db(&self.inner.store, move |s| s.remove_download(id)).await;
            return Err(e.into());
        }
        self.emit(Event::Added { id });
        // a torrent that was added with files already in place may already be done
        if let Ok(snap) = rt.engine.snapshot(&hash) {
            self.apply_snapshot(&rt, snap, true).await;
        }
        Ok(id)
    }

    /// Downloads the chosen files of a previewed torrent from its first web seed (BEP 19) as
    /// ordinary downloads, into the same folder layout. The addresses are built from the names
    /// the `.torrent` spells, percent-encoded; each is checked against the private-network rule
    /// first (a torrent file is untrusted content and must not aim a click at the LAN or at
    /// localhost), and redirects are held to the usual rule. The files are not checked against the
    /// torrent's piece hashes.
    pub async fn torrent_web_seed_add(
        &self,
        token: &str,
        only_files: Option<Vec<u32>>,
        dir: Option<PathBuf>,
        start_paused: bool,
    ) -> Result<WebSeedAdded> {
        let preview = {
            let pend = lock(&self.inner.torrent_previews);
            pend.iter()
                .find(|p| p.token == token && p.made.elapsed() < PREVIEW_TTL)
                .map(|p| p.preview.clone())
        }
        .ok_or_else(|| {
            Error::Core(riptide_core::Error::Invalid(
                "the torrent preview has expired; open the torrent again".into(),
            ))
        })?;
        let Some(seed) = preview.web_seeds.first() else {
            return Err(Error::Core(riptide_core::Error::Invalid(
                "this torrent lists no web seed".into(),
            )));
        };
        let (name, originals) = riptide_torrent::meta::original_layout(&preview.torrent_bytes)?;
        let multi = preview.files.len() > 1;
        let dir = dir.unwrap_or_else(|| self.settings().torrent_dir());
        let mut out = WebSeedAdded::default();
        for (i, f) in preview.files.iter().enumerate() {
            let chosen = only_files
                .as_ref()
                .is_none_or(|o| o.contains(&u32::try_from(f.index).unwrap_or(u32::MAX)));
            if f.padding || !chosen {
                continue;
            }
            let comps = originals.get(i).map(Vec::as_slice).unwrap_or_default();
            let Some(url) = riptide_torrent::meta::web_seed_url(seed, &name, comps, multi) else {
                out.refused += 1;
                continue;
            };
            if self.is_private_target(&url).await {
                out.refused += 1;
                continue;
            }
            let file_name = f.path.file_name().map(|n| n.to_string_lossy().into_owned());
            let folder = f.path.parent().map(|p| dir.join(p)).unwrap_or(dir.clone());
            let request = riptide_core::DownloadRequest {
                filename: file_name,
                dir: Some(folder),
                start_paused,
                ..riptide_core::DownloadRequest::new(url)
            };
            out.added.push(self.add_checked(request, true).await?);
        }
        if out.added.is_empty() && out.refused > 0 {
            return Err(Error::Net(riptide_net::Error::PrivateAddress));
        }
        Ok(out)
    }

    /// Every torrent's stored facts.
    pub async fn torrents(&self) -> Result<Vec<TorrentInfo>> {
        db(&self.inner.store, |s| s.list_torrents()).await
    }

    /// Stored facts plus live numbers of one torrent (the details drawer polls this).
    pub async fn torrent_live(&self, id: DownloadId) -> Result<TorrentLive> {
        let info = db(&self.inner.store, move |s| s.get_torrent(id))
            .await?
            .ok_or(Error::NotFound(id))?;
        let snap = self
            .torrent_rt()
            .and_then(|rt| rt.engine.snapshot(&info.info_hash).ok());
        let uploaded = info.uploaded;
        let ratio = info.ratio();
        Ok(match snap {
            Some(s) => TorrentLive {
                file_progress: s.file_progress,
                peers: s.peers_connected,
                peers_seen: s.peers_seen,
                down_bps: s.down_bps,
                up_bps: s.up_bps,
                ratio: {
                    let size = info.selected_bytes();
                    let total = uploaded.max(self.live_uploaded(&info, s.uploaded_bytes));
                    if size == 0 {
                        ratio
                    } else {
                        (total as f64 / size as f64) as f32
                    }
                },
                info,
            },
            None => TorrentLive {
                file_progress: self.stored_file_progress(&info, id).await,
                info,
                peers: 0,
                peers_seen: 0,
                down_bps: 0,
                up_bps: 0,
                ratio,
            },
        })
    }

    /// Per-file progress of a torrent the torrent engine is not running (stopped for being idle,
    /// or not started): from the stored state, and for a finished torrent from its row, so a
    /// completed file shows 100% and not 0%.
    async fn stored_file_progress(&self, info: &TorrentInfo, id: DownloadId) -> Vec<u64> {
        let dir = self.inner.data_dir.join("torrents");
        let hash = info.info_hash.clone();
        let stored = tokio::task::spawn_blocking(move || {
            riptide_torrent::offline::snapshot_for(&dir, &hash)
                .map(|s| s.file_progress)
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        if !stored.is_empty() {
            return stored;
        }
        let done = self
            .get(id)
            .await
            .ok()
            .flatten()
            .is_some_and(|d| d.state == DownloadState::Completed);
        if !done {
            return Vec::new();
        }
        let n = info
            .files
            .iter()
            .map(|f| f.index as usize + 1)
            .max()
            .unwrap_or(0);
        let mut out = vec![0u64; n];
        for f in &info.files {
            if info.selected.as_ref().is_none_or(|s| s.contains(&f.index)) {
                out[f.index as usize] = f.len;
            }
        }
        out
    }

    fn live_uploaded(&self, info: &TorrentInfo, session: u64) -> u64 {
        let base = self
            .torrent_rt()
            .map(|rt| {
                lock(&rt.base_uploaded)
                    .get(&info.info_hash)
                    .copied()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        base + session
    }

    /// Re-hashes every piece on disk (use after the files may have been changed outside Riptide).
    /// A torrent that no longer verifies goes back to `Downloading` and repairs itself from
    /// peers; it is `Completed` again only after every selected piece verified.
    pub async fn verify_torrent(&self, id: DownloadId) -> Result<()> {
        let (rt, _busy) = self.acquire_torrents().await?;
        let hash = self.torrent_hash(id).await?;
        rt.engine.verify(&hash).await?;
        if let Ok(snap) = rt.engine.snapshot(&hash) {
            self.apply_snapshot(&rt, snap, true).await;
        }
        Ok(())
    }

    async fn torrent_hash(&self, id: DownloadId) -> Result<String> {
        db(&self.inner.store, move |s| s.get_torrent(id))
            .await?
            .map(|t| t.info_hash)
            .ok_or(Error::NotFound(id))
    }

    pub(crate) async fn torrent_pause(&self, id: DownloadId) -> Result<()> {
        let hash = self.torrent_hash(id).await?;
        let Some((rt, _busy)) = self.try_acquire_torrents().await else {
            // no engine running: record the pause in the stored state, so the next engine keeps
            // the torrent stopped, and show it in the list
            let h = hash.clone();
            let paused = self
                .with_torrents_off(move |dir| {
                    match riptide_torrent::offline::set_paused(&dir, &h, true) {
                        Err(riptide_torrent::Error::NotFound(_)) => Ok(()),
                        other => other,
                    }
                })
                .await?;
            if paused.is_some() {
                if let Some(d) = self.get(id).await? {
                    if matches!(
                        d.state,
                        DownloadState::Probing | DownloadState::Downloading | DownloadState::Queued
                    ) {
                        self.transition(id, DownloadState::Paused, None).await?;
                    }
                }
                return Ok(());
            }
            // the engine started meanwhile: go through it
            return Box::pin(self.torrent_pause(id)).await;
        };
        rt.engine.pause(&hash).await?;
        // a finished or failed torrent stays as it is; an active one becomes `Paused`
        if let Some(d) = self.get(id).await? {
            if matches!(
                d.state,
                DownloadState::Probing | DownloadState::Downloading | DownloadState::Queued
            ) {
                self.advance(&rt, id, DownloadState::Paused, None).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn torrent_resume(&self, id: DownloadId) -> Result<()> {
        // the engine is started on demand: a launch with nothing to download leaves it off
        let (rt, _busy) = self.acquire_torrents().await?;
        let hash = self.torrent_hash(id).await?;
        rt.engine.resume(&hash).await?;
        if let Some(d) = self.get(id).await? {
            if matches!(
                d.state,
                DownloadState::Paused | DownloadState::Failed { .. }
            ) {
                self.advance(&rt, id, DownloadState::Probing, None).await?;
            }
        }
        if let Ok(snap) = rt.engine.snapshot(&hash) {
            self.apply_snapshot(&rt, snap, true).await;
        }
        Ok(())
    }

    /// Cancel: the torrent stops and what it wrote so far is deleted; the row stays `Cancelled`.
    pub(crate) async fn torrent_cancel(&self, d: &Download) -> Result<()> {
        if d.state.is_terminal() {
            return Ok(());
        }
        if let Some(info) = db(&self.inner.store, {
            let id = d.id;
            move |s| s.get_torrent(id)
        })
        .await?
        {
            self.drop_torrent(&info.info_hash, true).await?;
        }
        self.transition(d.id, DownloadState::Cancelled, None)
            .await?;
        Ok(())
    }

    /// Remove: the torrent stops and leaves the engine. Unfinished data always goes; finished
    /// files go only with `delete_file`.
    pub(crate) async fn torrent_remove(&self, d: &Download, delete_file: bool) -> Result<()> {
        let id = d.id;
        let Some(info) = db(&self.inner.store, move |s| s.get_torrent(id)).await? else {
            return Ok(());
        };
        let delete = delete_file || d.state != DownloadState::Completed;
        self.drop_torrent(&info.info_hash, delete).await
    }

    /// Takes a torrent out of the torrent engine for good. Without a running engine the stored
    /// state is edited directly: leaving the entry would bring the torrent back at the next
    /// start, downloading and uploading with no row in the list.
    async fn drop_torrent(&self, hash: &str, delete_files: bool) -> Result<()> {
        loop {
            if let Some((rt, _busy)) = self.try_acquire_torrents().await {
                lock(&rt.ids).remove(hash);
                return match rt.engine.remove(hash, delete_files).await {
                    Ok(()) | Err(riptide_torrent::Error::NotFound(_)) => Ok(()),
                    Err(e) => Err(e.into()),
                };
            }
            let h = hash.to_string();
            let done = self
                .with_torrents_off(move |dir| {
                    match riptide_torrent::offline::remove(&dir, &h, delete_files) {
                        Err(riptide_torrent::Error::NotFound(_)) => Ok(()),
                        other => other,
                    }
                })
                .await?;
            if done.is_some() {
                return Ok(());
            }
            // the engine started between the two looks: go through it
        }
    }

    async fn on_torrent_event(&self, rt: &Arc<TorrentRuntime>, ev: TorrentEvent) {
        match ev {
            TorrentEvent::Progress(snap) => self.apply_snapshot(rt, snap, true).await,
            TorrentEvent::Completed { id }
            | TorrentEvent::Paused { id }
            | TorrentEvent::Resumed { id }
            | TorrentEvent::SeedingStopped { id }
            | TorrentEvent::Failed { id, .. } => {
                if let Ok(snap) = rt.engine.snapshot(&id) {
                    self.apply_snapshot(rt, snap, true).await;
                }
            }
            TorrentEvent::FilesCompleted { id, indices } => {
                self.mark_torrent_files(rt, &id, &indices).await;
            }
            TorrentEvent::Added { .. } | TorrentEvent::Removed { .. } => {}
        }
    }

    /// Mark-of-the-Web on files that just finished, while the rest of the torrent still runs: a
    /// file written under its final name must not be openable without SmartScreen until the
    /// whole torrent is done.
    async fn mark_torrent_files(&self, rt: &Arc<TorrentRuntime>, hash: &str, indices: &[usize]) {
        if !self.settings().mark_of_the_web {
            return;
        }
        let Some(id) = rt.id_of(hash) else { return };
        let (Ok(Some(d)), Ok(Some(info))) = (
            self.get(id).await,
            db(&self.inner.store, move |s| s.get_torrent(id)).await,
        ) else {
            return;
        };
        let paths: Vec<PathBuf> = info
            .files
            .iter()
            .filter(|f| indices.contains(&(f.index as usize)))
            .map(|f| d.dir.join(&f.path))
            .collect();
        let _ = tokio::task::spawn_blocking(move || {
            for p in paths {
                if p.is_file() {
                    crate::motw::mark_from_internet(&p, None, None);
                }
            }
        })
        .await;
    }

    /// Brings the row, the mirror and the UI in line with a snapshot.
    async fn apply_snapshot(&self, rt: &Arc<TorrentRuntime>, snap: TorrentSnapshot, emit: bool) {
        let Some(id) = rt.id_of(&snap.id) else { return };
        let Ok(Some(d)) = self.get(id).await else {
            return;
        };
        if d.state == DownloadState::Cancelled {
            return;
        }
        let before = d.state;
        let seeding = matches!(snap.state, TorrentState::Seeding);
        let error = match &snap.state {
            TorrentState::Failed { error } => Some(error.clone()),
            _ => None,
        };
        let want = target_state(&snap, before)
            .filter(|to| *to != before && !(is_failed(before) && is_failed(*to)));

        let base = lock(&rt.base_uploaded).get(&snap.id).copied().unwrap_or(0);
        let uploaded = base + snap.uploaded_bytes;
        let size = snap.total_bytes;
        let ratio_milli = uploaded
            .saturating_mul(1000)
            .checked_div(size)
            .map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));

        // The mirror the list reads is written at most once a second, and always before a state
        // change: the UI re-reads the record when the state changes (a pause would otherwise show
        // the number from a second ago, which at 20 MB/s is 20 MB short).
        let due = {
            let mut p = lock(&rt.persisted);
            let due = want.is_some() || p.get(&id).is_none_or(|t| t.elapsed() >= PERSIST_EVERY);
            if due {
                p.insert(id, Instant::now());
            }
            due
        };
        if due {
            let (down, peers) = (snap.downloaded_bytes, snap.peers_connected);
            let _ = db(&self.inner.store, move |s| {
                s.update_torrent_stats(id, seeding, uploaded, down, peers)
            })
            .await;
        }

        if let Some(to) = want {
            if let Err(e) = self.advance(rt, id, to, error).await {
                tracing::warn!(id = id.0, error = %e, "torrent state change refused");
            }
        }
        let now = self.get(id).await.ok().flatten().map(|x| x.state);
        let finished_now =
            before != DownloadState::Completed && now == Some(DownloadState::Completed);

        if finished_now {
            self.finish_torrent(id).await;
        }
        // a finished torrent being re-checked reports 0 bytes until the check ends: not worth showing
        let rechecking =
            before == DownloadState::Completed && matches!(snap.state, TorrentState::Checking);
        if emit && !rechecking && !matches!(snap.state, TorrentState::Paused) {
            self.emit(Event::Progress {
                id,
                downloaded: snap.downloaded_bytes,
                total: Some(snap.total_bytes),
                speed_bps: snap.down_bps,
                eta_secs: snap.eta_secs,
                connections: snap.peers_connected,
            });
            self.emit(Event::TorrentStats {
                id,
                peers: snap.peers_connected,
                peers_seen: snap.peers_seen,
                up_bps: snap.up_bps,
                uploaded,
                ratio_milli,
                seeding,
            });
        }
    }

    /// Mark-of-the-Web on every finished file, then the `Completed` event.
    async fn finish_torrent(&self, id: DownloadId) {
        let (Ok(Some(d)), Ok(Some(info))) = (
            self.get(id).await,
            db(&self.inner.store, move |s| s.get_torrent(id)).await,
        ) else {
            return;
        };
        if self.settings().mark_of_the_web {
            let paths: Vec<PathBuf> = info
                .files
                .iter()
                .filter(|f| info.selected.as_ref().is_none_or(|s| s.contains(&f.index)))
                .map(|f| d.dir.join(&f.path))
                .collect();
            let _ = tokio::task::spawn_blocking(move || {
                for p in paths {
                    if p.is_file() {
                        crate::motw::mark_from_internet(&p, None, None);
                    }
                }
            })
            .await;
        }
        self.emit(Event::Completed {
            id,
            path: d.final_path(),
        });
    }

    /// Moves a row to `to` along legal transitions. A completed row that stops being complete
    /// (a verify found damage) is reopened in place, the one exception to "`Completed` never
    /// changes".
    async fn advance(
        &self,
        rt: &Arc<TorrentRuntime>,
        id: DownloadId,
        to: DownloadState,
        error: Option<String>,
    ) -> Result<()> {
        let _guard = rt.sync.lock().await;
        let Some(d) = self.get(id).await? else {
            return Ok(());
        };
        let from = d.state;
        if from == to
            || (matches!(from, DownloadState::Failed { .. })
                && matches!(to, DownloadState::Failed { .. }))
        {
            return Ok(());
        }
        if from == DownloadState::Completed {
            if matches!(to, DownloadState::Downloading | DownloadState::Probing) {
                db(&self.inner.store, move |s| s.reopen_completed(id)).await?;
                self.emit(Event::StateChanged {
                    id,
                    from,
                    to: DownloadState::Downloading,
                });
            }
            return Ok(());
        }
        if from.is_terminal() {
            return Ok(());
        }
        let Some(path) = walk(from, to) else {
            return Ok(());
        };
        for step in path {
            let err = matches!(step, DownloadState::Failed { .. })
                .then(|| error.clone())
                .flatten();
            self.transition(id, step, err).await?;
        }
        Ok(())
    }
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_uses_only_legal_arrows() {
        use DownloadState::*;
        for from in DownloadState::ALL {
            for to in DownloadState::ALL {
                if to == Cancelled {
                    continue;
                }
                if let Some(path) = walk(from, to) {
                    let mut cur = from;
                    for step in path {
                        assert!(cur.can_transition_to(step), "{cur:?} -> {step:?}");
                        cur = step;
                    }
                    assert_eq!(cur, to);
                }
            }
        }
        assert_eq!(
            walk(Downloading, Completed),
            Some(vec![Verifying, Completed])
        );
        assert_eq!(
            walk(Paused, Probing),
            Some(vec![Queued, Probing]),
            "resume goes through Queued"
        );
        assert_eq!(walk(Probing, Downloading), Some(vec![Downloading]));
        assert_eq!(walk(Completed, Downloading), None, "terminal");
    }

    #[test]
    fn root_name_is_the_shared_folder_or_the_single_file() {
        let f = |p: &str| TorrentFileInfo {
            index: 0,
            path: p.into(),
            len: 1,
        };
        assert_eq!(root_name(&[f("movie.mkv")], "x"), "movie.mkv");
        assert_eq!(root_name(&[f("set/a"), f("set/sub/b")], "x"), "set");
        assert_eq!(root_name(&[f("a"), f("b")], "fallback"), "fallback");
    }

    #[test]
    fn canonical_url_keeps_no_trackers() {
        let h = "ab".repeat(20);
        assert_eq!(canonical_url(&h), format!("magnet:?xt=urn:btih:{h}"));
    }

    #[test]
    fn options_map_to_the_seeding_policy() {
        let mut o = TorrentOptions::default();
        assert!(!o.policy().enabled, "seeding is off by default");
        o.seeding = true;
        o.seed_ratio_percent = 250;
        o.seed_minutes = 0;
        let p = o.policy();
        assert_eq!(p.max_ratio, Some(2.5));
        assert_eq!(p.max_minutes, None);
        o.upload_limit = Some(u64::MAX);
        assert_eq!(o.upload_bps(), Some(u32::MAX));
    }
}
