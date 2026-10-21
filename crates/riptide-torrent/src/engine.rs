//! [`TorrentEngine`]: the API the CLI and the desktop app use to run torrents.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, Session,
    SessionOptions, TorrentStatsState,
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::limits::METADATA_TIMEOUT_SECS;
use crate::magnet::parse_magnet;
use crate::meta::{
    info_hash_of_bytes, inspect_torrent, preview_from_info, read_torrent_file, TorrentPreview,
};
use crate::registry::{Entry, Registry};
use crate::storage::SafeStorageFactory;
use crate::types::{
    AddOptions, SeedingPolicy, TorrentEvent, TorrentSettings, TorrentSnapshot, TorrentSource,
    TorrentState,
};

const TICK: Duration = Duration::from_millis(500);
/// Most peer addresses remembered per torrent.
const MAX_STORED_PEERS: usize = 64;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn hash_of(id: &str) -> Result<Id20> {
    if id.len() != 40 {
        return Err(Error::NotFound(id.to_string()));
    }
    Id20::from_str(id).map_err(|_| Error::NotFound(id.to_string()))
}

/// Runs BitTorrent downloads. Create one with [`TorrentEngine::start`].
pub struct TorrentEngine {
    session: Arc<Session>,
    registry: Arc<Registry>,
    settings: Mutex<TorrentSettings>,
    events: broadcast::Sender<TorrentEvent>,
    cancel: CancellationToken,
    monitor: Mutex<Option<JoinHandle<()>>>,
    /// Torrents for which a failure event was already sent.
    failed: Mutex<HashMap<String, String>>,
    /// Files already reported through [`TorrentEvent::FilesCompleted`], per torrent.
    done_files: Mutex<HashMap<String, std::collections::HashSet<usize>>>,
    /// Serialises `add`: two torrents added together must not both plan the same free name.
    add_lock: tokio::sync::Mutex<()>,
}

impl TorrentEngine {
    /// Opens (or creates) the state under `settings.state_dir`, restores the torrents of the
    /// previous run and starts the progress monitor.
    ///
    /// Torrents that were completed (with seeding off) or paused by the user stay stopped.
    pub async fn start(settings: TorrentSettings) -> Result<Arc<Self>> {
        validate_settings(&settings)?;
        std::fs::create_dir_all(&settings.state_dir)?;
        std::fs::create_dir_all(&settings.default_dir)?;
        let registry = Arc::new(Registry::open(
            settings.state_dir.join(crate::offline::REGISTRY_FILE),
        )?);

        let opts = session_options(&settings, registry.clone());
        let session = Session::new_with_opts(settings.default_dir.clone(), opts)
            .await
            .map_err(|e| Error::Engine(format!("{e:#}")))?;

        let (events, _) = broadcast::channel(512);
        let engine = Arc::new(Self {
            session,
            registry,
            settings: Mutex::new(settings),
            events,
            cancel: CancellationToken::new(),
            monitor: Mutex::new(None),
            failed: Mutex::new(HashMap::new()),
            add_lock: tokio::sync::Mutex::new(()),
            done_files: Mutex::new(HashMap::new()),
        });
        engine.restore().await;
        let weak = Arc::downgrade(&engine);
        let cancel = engine.cancel.clone();
        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tick.tick() => {}
                }
                match weak.upgrade() {
                    Some(e) => e.tick().await,
                    None => break,
                }
            }
        });
        *lock(&engine.monitor) = Some(handle);
        Ok(engine)
    }

    /// Subscribes to engine events.
    pub fn subscribe(&self) -> broadcast::Receiver<TorrentEvent> {
        self.events.subscribe()
    }

    /// Current settings.
    pub fn settings(&self) -> TorrentSettings {
        lock(&self.settings).clone()
    }

    /// Address the engine listens on for peers, if it listens.
    pub fn listen_addr(&self) -> Option<std::net::SocketAddr> {
        self.session.listen_addr()
    }

    fn emit(&self, e: TorrentEvent) {
        // no subscriber is not an error
        let _ = self.events.send(e);
    }

    /// Resolves a source to the files it would write, without starting anything.
    ///
    /// For a magnet link this contacts DHT / trackers / `opts.peers` until the metadata
    /// arrives (at most [`METADATA_TIMEOUT_SECS`]). Pass the result's
    /// [`TorrentPreview::torrent_bytes`] to [`TorrentSource::Bytes`] to add the torrent without
    /// fetching the metadata again.
    pub async fn preview(
        &self,
        source: &TorrentSource,
        opts: &AddOptions,
    ) -> Result<TorrentPreview> {
        let dir = self.target_dir(opts)?;
        let adopt = opts.adopt_existing;
        // the torrent's own entry (when it is already known) is not a claim against itself
        let own = match source {
            TorrentSource::Bytes(b) => info_hash_of_bytes(b),
            TorrentSource::File(p) => read_torrent_file(p)
                .ok()
                .and_then(|b| info_hash_of_bytes(&b)),
            TorrentSource::Magnet(m) => parse_magnet(m).ok().map(|m| m.info_hash),
        };
        let claims = self.registry.claim_set(&dir, own.as_deref());
        // folder and file name of each reserved path, folder compared as the file system sees it
        let dir_key = crate::registry::normalise_dir(&dir);
        let reserved: std::collections::HashSet<String> = opts
            .reserved
            .iter()
            .filter_map(|p| {
                let name = p.file_name()?.to_string_lossy().to_lowercase();
                let parent = crate::registry::normalise_dir(p.parent()?);
                Some(format!("{parent}/{name}"))
            })
            .collect();
        let exists = move |rel: &Path| {
            if claims.claims(rel) {
                return true;
            }
            let full = format!(
                "{dir_key}/{}",
                rel.to_string_lossy().replace('\\', "/").to_lowercase()
            );
            if reserved.contains(&full)
                || reserved.contains(full.strip_suffix(".rtpart").unwrap_or("\0"))
            {
                return true;
            }
            !adopt && crate::storage::name_taken(&dir, rel)
        };
        match source {
            TorrentSource::Bytes(b) => inspect_torrent(b, &exists),
            TorrentSource::File(p) => inspect_torrent(&read_torrent_file(p)?, &exists),
            TorrentSource::Magnet(m) => {
                let magnet = parse_magnet(m)?;
                tracing::info!(target: "riptide_torrent", magnet = %magnet.redacted(), "resolving magnet link");
                let add = AddTorrentOptions {
                    list_only: true,
                    initial_peers: (!opts.peers.is_empty()).then(|| opts.peers.clone()),
                    ..Default::default()
                };
                let fut = self
                    .session
                    .add_torrent(AddTorrent::Url(magnet.to_uri().into()), Some(add));
                let resp = tokio::time::timeout(Duration::from_secs(METADATA_TIMEOUT_SECS), fut)
                    .await
                    .map_err(|_| Error::MetadataTimeout)?
                    .map_err(|e| Error::Engine(format!("{e:#}")))?;
                match resp {
                    AddTorrentResponse::ListOnly(l) => {
                        let bytes = l.torrent_bytes.to_vec();
                        // the byte-level checks run on what the peers delivered, too
                        crate::meta::check_bencode_shape(&bytes)?;
                        preview_from_info(l.info_hash.as_string(), &l.info, bytes, &exists)
                    }
                    _ => Err(Error::Engine("unexpected response to a listing".into())),
                }
            }
        }
    }

    fn target_dir(&self, opts: &AddOptions) -> Result<PathBuf> {
        let dir = match &opts.dir {
            Some(d) => d.clone(),
            None => lock(&self.settings).default_dir.clone(),
        };
        std::path::absolute(&dir).map_err(Error::from)
    }

    /// Adds a torrent. Nothing is written outside the chosen folder, whatever the torrent says.
    ///
    /// Returns the id (info-hash). Adding a torrent that is already known returns its id and
    /// changes nothing. With [`AddOptions::start_paused`] the torrent is only recorded: it
    /// touches neither the network nor the disk until [`TorrentEngine::resume`].
    pub async fn add(&self, source: TorrentSource, opts: AddOptions) -> Result<String> {
        let _adding = self.add_lock.lock().await;
        let preview = self.preview(&source, &opts).await?;
        let id = preview.info_hash.clone();
        if self.registry.get(&id).is_some() {
            return Ok(id);
        }
        let dir = self.target_dir(&opts)?;
        if dir.exists() && !dir.is_dir() {
            return Err(Error::InvalidSettings(format!(
                "{} is not a folder",
                dir.display()
            )));
        }
        std::fs::create_dir_all(&dir)?;

        let layout: Vec<PathBuf> = match &opts.layout {
            Some(l) => {
                if l.len() != preview.files.len()
                    || l.iter().any(|p| crate::paths::confine(&dir, p).is_err())
                {
                    return Err(Error::InvalidSelection(
                        "the planned layout does not fit the torrent".into(),
                    ));
                }
                l.clone()
            }
            None => preview.files.iter().map(|f| f.path.clone()).collect(),
        };
        let selected = normalise_selection(&preview, opts.only_files.as_deref())?;
        let selected_bytes = selected
            .as_ref()
            .map(|s| s.iter().map(|i| preview.files[*i].len).sum())
            .unwrap_or(preview.total_bytes);

        self.save_torrent_file(&id, &preview.torrent_bytes)?;
        self.registry.insert(
            &id,
            Entry {
                dir: dir.clone(),
                files: Some(layout),
                name: preview.name.clone(),
                total_bytes: preview.total_bytes,
                selected_bytes,
                selected,
                sequential: opts.sequential,
                adopt: opts.adopt_existing,
                user_paused: opts.start_paused,
                completed: false,
                seeding_done: false,
                added_at: now_secs(),
                finished_at: None,
                progress_bytes: 0,
                error: None,
                peers: opts.peers.iter().copied().take(MAX_STORED_PEERS).collect(),
                created: Some(Vec::new()),
                piece_length: Some(preview.piece_length),
                file_lens: Some(preview.files.iter().map(|f| f.len).collect()),
                file_progress: None,
            },
        );
        self.registry.flush()?;

        let result = if opts.start_paused {
            Ok(())
        } else {
            self.ensure_in_session(&id).await
        };
        match result {
            Ok(()) => {
                tracing::info!(target: "riptide_torrent", id = %id, name = %preview.name, files = preview.files.len(), "torrent added");
                self.emit(TorrentEvent::Added { id: id.clone() });
                Ok(id)
            }
            Err(e) => {
                // files this attempt created before it failed go with it
                if let Some(entry) = self.registry.get(&id) {
                    crate::storage::remove_laid_out_files(&entry.dir, &entry.deletable());
                }
                self.registry.remove(&id);
                let _ = self.registry.flush();
                if let Ok(p) = self.torrent_file(&id) {
                    let _ = std::fs::remove_file(p);
                }
                Err(e)
            }
        }
    }

    fn torrent_file(&self, id: &str) -> Result<PathBuf> {
        hash_of(id)?; // the id becomes part of a file name: only 40 hex digits get through
        Ok(lock(&self.settings)
            .state_dir
            .join("torrents")
            .join(format!("{id}.torrent")))
    }

    fn save_torrent_file(&self, id: &str, bytes: &[u8]) -> Result<()> {
        let path = self.torrent_file(id)?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension("torrent.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Makes sure the library is running this torrent (it is not, for example, after a restart
    /// while the torrent is paused or completed with seeding off).
    async fn ensure_in_session(&self, id: &str) -> Result<()> {
        if self.handle(id).is_ok() {
            return Ok(());
        }
        let entry = self
            .registry
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        let bytes = std::fs::read(self.torrent_file(id)?)?;
        // the stored file is vetted again: it lives in a user-writable folder
        let _ = inspect_torrent(&bytes, &|_| false)?;
        self.session_add(bytes, &entry).await
    }

    /// Hands a torrent to the library with the file set that should be active right now.
    async fn session_add(&self, bytes: Vec<u8>, entry: &Entry) -> Result<()> {
        let only_files = match (&entry.selected, entry.sequential) {
            (Some(s), true) => s.first().map(|i| vec![*i]),
            (None, true) => Some(vec![0]),
            (s, false) => s.clone(),
        };
        let add = AddTorrentOptions {
            only_files,
            overwrite: true,
            output_folder: Some(entry.dir.to_string_lossy().into_owned()),
            initial_peers: (!entry.peers.is_empty()).then(|| entry.peers.clone()),
            ..Default::default()
        };
        match self
            .session
            .add_torrent(AddTorrent::from_bytes(bytes), Some(add))
            .await
        {
            Ok(AddTorrentResponse::Added(..)) | Ok(AddTorrentResponse::AlreadyManaged(..)) => {
                Ok(())
            }
            Ok(AddTorrentResponse::ListOnly(_)) => {
                Err(Error::Engine("unexpected listing response".into()))
            }
            Err(e) => Err(Error::Engine(format!("{e:#}"))),
        }
    }

    /// Forgets the in-memory state of a torrent and re-hashes every piece on disk from scratch.
    ///
    /// Use it after the files may have been touched outside Riptide. A torrent whose data no
    /// longer matches goes back to "incomplete" and, if peers are available, repairs itself;
    /// it is reported as completed again only after every selected piece verified.
    pub async fn verify(&self, id: &str) -> Result<()> {
        if self.registry.get(id).is_none() {
            return Err(Error::NotFound(id.to_string()));
        }
        if self.handle(id).is_ok() {
            self.session
                .delete(TorrentIdOrHash::Hash(hash_of(id)?), false)
                .await
                .map_err(|e| Error::Engine(format!("{e:#}")))?;
        }
        self.registry.update(id, |e| {
            e.completed = false;
            e.seeding_done = false;
            e.user_paused = false;
            e.finished_at = None;
            e.progress_bytes = 0;
            e.error = None;
        });
        self.registry.flush()?;
        lock(&self.failed).remove(id);
        lock(&self.done_files).remove(id);
        self.ensure_in_session(id).await
    }

    fn handle(&self, id: &str) -> Result<Arc<librqbit::ManagedTorrent>> {
        let h = hash_of(id)?;
        self.session
            .get(TorrentIdOrHash::Hash(h))
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    /// Pauses a torrent; it stays paused across restarts.
    pub async fn pause(&self, id: &str) -> Result<()> {
        if self.registry.get(id).is_none() {
            return Err(Error::NotFound(id.to_string()));
        }
        self.registry.update(id, |e| e.user_paused = true);
        self.registry.flush()?;
        if let Ok(h) = self.handle(id) {
            if !matches!(h.stats().state, TorrentStatsState::Paused) {
                self.session
                    .pause(&h)
                    .await
                    .map_err(|e| Error::Engine(format!("{e:#}")))?;
            }
        }
        self.emit(TorrentEvent::Paused { id: id.to_string() });
        Ok(())
    }

    /// Resumes a paused torrent. A completed torrent only goes back on the network if seeding
    /// is enabled.
    pub async fn resume(&self, id: &str) -> Result<()> {
        let seeding = lock(&self.settings).seeding;
        let entry = self
            .registry
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        self.registry.update(id, |e| {
            e.user_paused = false;
            e.error = None;
            if e.completed && seeding.enabled {
                e.seeding_done = false;
                e.finished_at = Some(now_secs());
            }
        });
        self.registry.flush()?;
        if entry.completed && !seeding.enabled {
            return Ok(());
        }
        lock(&self.failed).remove(id);
        match self.handle(id) {
            Ok(h) => {
                if matches!(h.stats().state, TorrentStatsState::Paused) || h.is_paused() {
                    self.session
                        .unpause(&h)
                        .await
                        .map_err(|e| Error::Engine(format!("{e:#}")))?;
                }
            }
            Err(_) => self.ensure_in_session(id).await?,
        }
        self.emit(TorrentEvent::Resumed { id: id.to_string() });
        Ok(())
    }

    /// Removes a torrent, optionally deleting the files it wrote (only the files of this
    /// torrent's own layout, never anything else in the folder).
    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<()> {
        let h = hash_of(id)?;
        let entry = self
            .registry
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        if self.handle(id).is_ok() {
            if let Err(e) = self
                .session
                .delete(TorrentIdOrHash::Hash(h), delete_files)
                .await
            {
                tracing::warn!(target: "riptide_torrent", id, "delete: {e:#}");
            }
        } else if delete_files {
            crate::storage::remove_laid_out_files(&entry.dir, &entry.deletable());
        }
        self.registry.remove(id);
        self.registry.flush()?;
        let _ = std::fs::remove_file(self.torrent_file(id)?);
        lock(&self.failed).remove(id);
        lock(&self.done_files).remove(id);
        self.emit(TorrentEvent::Removed { id: id.to_string() });
        Ok(())
    }

    /// Changes which files are downloaded (indices from the preview).
    pub async fn select_files(&self, id: &str, files: &[usize]) -> Result<()> {
        let entry = self
            .registry
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        let layout_len = entry.files.as_ref().map(Vec::len).unwrap_or(0);
        if files.is_empty() || files.iter().any(|i| *i >= layout_len) {
            return Err(Error::InvalidSelection("index out of range".into()));
        }
        self.ensure_in_session(id).await?;
        let h = self.handle(id)?;
        // the library refuses a change while it is still checking the files on disk
        for _ in 0..1200 {
            if !matches!(h.stats().state, TorrentStatsState::Initializing { .. }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut sel: Vec<usize> = files.to_vec();
        sel.sort_unstable();
        sel.dedup();
        let lens: Vec<u64> = h
            .with_metadata(|m| m.file_infos.iter().map(|f| f.len).collect())
            .map_err(|e| Error::Engine(format!("{e:#}")))?;
        let selected_bytes: u64 = sel.iter().map(|i| lens[*i]).sum();
        self.registry.update(id, |e| {
            e.selected = Some(sel.clone());
            e.selected_bytes = selected_bytes;
            e.completed = false;
            e.seeding_done = false;
        });
        let set = if entry.sequential {
            std::iter::once(sel[0]).collect()
        } else {
            sel.iter().copied().collect()
        };
        self.session
            .update_only_files(&h, &set)
            .await
            .map_err(|e| Error::Engine(format!("{e:#}")))?;
        if entry.user_paused {
            let _ = self.session.pause(&h).await;
        }
        self.registry.flush()
    }

    /// Sets the upload cap in bytes per second (`None` = unlimited).
    pub fn set_upload_limit(&self, bps: Option<u32>) {
        self.session
            .ratelimits
            .set_upload_bps(bps.and_then(NonZeroU32::new));
        lock(&self.settings).upload_limit_bps = bps;
    }

    /// Sets the download cap in bytes per second (`None` = unlimited).
    pub fn set_download_limit(&self, bps: Option<u32>) {
        self.session
            .ratelimits
            .set_download_bps(bps.and_then(NonZeroU32::new));
        lock(&self.settings).download_limit_bps = bps;
    }

    /// Changes the seeding policy. Turning seeding off stops torrents that are seeding.
    pub fn set_seeding(&self, policy: SeedingPolicy) -> Result<()> {
        validate_seeding(&policy)?;
        lock(&self.settings).seeding = policy;
        Ok(())
    }

    /// All torrents.
    pub fn list(&self) -> Vec<TorrentSnapshot> {
        self.registry
            .all()
            .into_iter()
            .filter_map(|(id, entry)| self.snapshot_of(&id, &entry))
            .collect()
    }

    /// One torrent.
    pub fn snapshot(&self, id: &str) -> Result<TorrentSnapshot> {
        let entry = self
            .registry
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        self.snapshot_of(id, &entry)
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn snapshot_of(&self, id: &str, entry: &Entry) -> Option<TorrentSnapshot> {
        let Ok(h) = self.handle(id) else {
            return Some(crate::offline::snapshot(id, entry));
        };
        let st = h.stats();
        let live = st.live.as_ref();
        let paused_by_user = entry.user_paused;
        let state = match st.state {
            TorrentStatsState::Error => TorrentState::Failed {
                error: st.error.clone().unwrap_or_else(|| "unknown error".into()),
            },
            _ if entry.completed && matches!(st.state, TorrentStatsState::Live) => {
                TorrentState::Seeding
            }
            _ if entry.completed => TorrentState::Completed,
            _ if paused_by_user => TorrentState::Paused,
            TorrentStatsState::Paused => TorrentState::Paused,
            TorrentStatsState::Initializing { paused: true } => TorrentState::Paused,
            TorrentStatsState::Initializing { .. } => TorrentState::Checking,
            TorrentStatsState::Live => TorrentState::Downloading,
        };
        let down_bps = live.map(|l| l.download_speed.as_bytes()).unwrap_or(0);
        let up_bps = live.map(|l| l.upload_speed.as_bytes()).unwrap_or(0);
        let (connected, seen) = live
            .map(|l| (l.snapshot.peer_stats.live, l.snapshot.peer_stats.seen))
            .unwrap_or((0, 0));
        let total = entry.selected_bytes;
        let downloaded = if entry.completed {
            total
        } else {
            match st.state {
                TorrentStatsState::Initializing { .. } | TorrentStatsState::Error => {
                    entry.progress_bytes
                }
                _ => st.progress_bytes.min(total),
            }
        };
        let eta = (down_bps > 0 && total > downloaded).then(|| (total - downloaded) / down_bps);
        Some(TorrentSnapshot {
            id: id.to_string(),
            name: entry.name.clone(),
            state,
            dir: entry.dir.clone(),
            total_bytes: total,
            downloaded_bytes: downloaded,
            uploaded_bytes: st.uploaded_bytes,
            down_bps,
            up_bps,
            peers_connected: connected,
            peers_seen: seen,
            eta_secs: eta,
            file_progress: st.file_progress.clone(),
            selected: entry.selected.clone(),
        })
    }

    /// Re-adds what should be running after a start: unfinished torrents the user did not
    /// pause, and finished ones only while seeding is on and not yet done. Everything else
    /// stays out of the library (no network, no disk reads) until the user resumes it.
    async fn restore(&self) {
        let seeding = lock(&self.settings).seeding.enabled;
        for (id, entry) in self.registry.all() {
            let wanted =
                !entry.user_paused && (!entry.completed || (seeding && !entry.seeding_done));
            if !wanted {
                continue;
            }
            if let Err(e) = self.ensure_in_session(&id).await {
                tracing::warn!(target: "riptide_torrent", id = %id, "cannot restore torrent: {e}");
                let msg = e.to_string();
                self.registry.update(&id, |x| x.error = Some(msg.clone()));
                lock(&self.failed).insert(id.clone(), msg.clone());
                self.emit(TorrentEvent::Failed { id, error: msg });
            }
        }
        let _ = self.registry.flush();
    }

    async fn tick(&self) {
        let settings = self.settings();
        let mut dirty = false;
        for (id, entry) in self.registry.all() {
            let Ok(h) = self.handle(&id) else { continue };
            let st = h.stats();

            // failures
            if matches!(st.state, TorrentStatsState::Error) {
                let msg = st.error.clone().unwrap_or_else(|| "unknown error".into());
                let first = lock(&self.failed).insert(id.clone(), msg.clone()).is_none();
                if first {
                    self.registry.update(&id, |e| e.error = Some(msg.clone()));
                    dirty = true;
                    self.emit(TorrentEvent::Failed {
                        id: id.clone(),
                        error: msg,
                    });
                }
                continue;
            }

            // sequential: keep exactly the files up to the first unfinished one selected
            if entry.sequential && !entry.completed {
                if let Some(next) = self.sequential_step(&h, &entry, &st.file_progress) {
                    let _ = self.session.update_only_files(&h, &next).await;
                }
            }

            // files that are complete while the torrent as a whole is not
            if !entry.completed {
                self.report_finished_files(&id, &h, &st.file_progress);
            }

            // did the selected data just become complete (every piece hash-verified)?
            let done = !entry.completed && self.selection_done(&h, &entry, &st);
            if done {
                self.registry.update(&id, |e| {
                    e.completed = true;
                    e.finished_at = Some(now_secs());
                    e.progress_bytes = e.selected_bytes;
                    e.error = None;
                });
                dirty = true;
                self.merge_file_progress(&id, &st.file_progress);
                tracing::info!(target: "riptide_torrent", id = %id, "torrent completed and verified");
                self.emit(TorrentEvent::Completed { id: id.clone() });
            }
            let entry = self.registry.get(&id).unwrap_or(entry);

            // seeding policy
            if entry.completed && !entry.seeding_done && matches!(st.state, TorrentStatsState::Live)
            {
                let stop_reason = if !settings.seeding.enabled {
                    Some(false)
                } else if seeding_limit_hit(&settings.seeding, &entry, st.uploaded_bytes) {
                    Some(true)
                } else {
                    None
                };
                if let Some(announce) = stop_reason {
                    let _ = self.session.pause(&h).await;
                    self.registry.update(&id, |e| e.seeding_done = true);
                    dirty = true;
                    if announce {
                        self.emit(TorrentEvent::SeedingStopped { id: id.clone() });
                    }
                }
            }

            // progress bookkeeping and event
            if let Some(snap) = self.snapshot_of(&id, &entry) {
                if snap.downloaded_bytes != entry.progress_bytes && !entry.completed {
                    self.registry
                        .update(&id, |e| e.progress_bytes = snap.downloaded_bytes);
                    dirty = true;
                }
                let active = !matches!(
                    snap.state,
                    TorrentState::Paused | TorrentState::Completed | TorrentState::Failed { .. }
                );
                if active {
                    self.emit(TorrentEvent::Progress(snap));
                }
            }
        }
        if dirty {
            if let Err(e) = self.registry.flush() {
                tracing::warn!(target: "riptide_torrent", "cannot save torrent state: {e}");
            }
        }
    }

    fn report_finished_files(&self, id: &str, h: &Arc<librqbit::ManagedTorrent>, progress: &[u64]) {
        if progress.is_empty() {
            return;
        }
        let Ok(lens) =
            h.with_metadata(|m| m.file_infos.iter().map(|f| f.len).collect::<Vec<u64>>())
        else {
            return;
        };
        if lens.len() != progress.len() {
            return;
        }
        let fresh: Vec<usize> = {
            let mut all = lock(&self.done_files);
            let seen = all.entry(id.to_string()).or_default();
            (0..lens.len())
                .filter(|i| lens[*i] > 0 && progress[*i] >= lens[*i] && seen.insert(*i))
                .collect()
        };
        if !fresh.is_empty() {
            self.emit(TorrentEvent::FilesCompleted {
                id: id.to_string(),
                indices: fresh,
            });
        }
    }

    fn selection_done(
        &self,
        h: &Arc<librqbit::ManagedTorrent>,
        entry: &Entry,
        st: &librqbit::TorrentStats,
    ) -> bool {
        if !matches!(
            st.state,
            TorrentStatsState::Live | TorrentStatsState::Paused
        ) {
            return false;
        }
        if !entry.sequential {
            return st.finished;
        }
        let lens: Vec<u64> = h
            .with_metadata(|m| m.file_infos.iter().map(|f| f.len).collect())
            .unwrap_or_default();
        if lens.is_empty() || st.file_progress.len() != lens.len() {
            return false;
        }
        selected_indices(entry, lens.len())
            .iter()
            .all(|i| st.file_progress[*i] >= lens[*i])
    }

    /// For a sequential torrent: the file set that should be active now, if it differs from the
    /// current one.
    fn sequential_step(
        &self,
        h: &Arc<librqbit::ManagedTorrent>,
        entry: &Entry,
        progress: &[u64],
    ) -> Option<std::collections::HashSet<usize>> {
        let lens: Vec<u64> = h
            .with_metadata(|m| m.file_infos.iter().map(|f| f.len).collect())
            .ok()?;
        if progress.len() != lens.len() {
            return None;
        }
        let sel = selected_indices(entry, lens.len());
        let first_open = sel.iter().copied().find(|i| progress[*i] < lens[*i])?;
        let want: std::collections::HashSet<usize> = [first_open].into_iter().collect();
        let current: std::collections::HashSet<usize> = h
            .only_files()
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();
        (current != want).then_some(want)
    }

    /// Stops the engine: pauses every torrent, saves state, stops the session.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let handle = lock(&self.monitor).take();
        if let Some(h) = handle {
            let _ = h.await;
        }
        self.record_file_progress();
        let _ = self.registry.flush();
        self.session.stop().await;
        // the session no longer holds any file open: placeholders can go
        self.drop_placeholders();
        let _ = self.registry.flush();
    }

    /// Remembers the verified bytes per file of every torrent the library can answer for now
    /// (not while it is still checking the files: those numbers are not final).
    fn record_file_progress(&self) {
        for (id, entry) in self.registry.all() {
            let Ok(h) = self.handle(&id) else { continue };
            let st = h.stats();
            if !matches!(
                st.state,
                TorrentStatsState::Live | TorrentStatsState::Paused
            ) {
                continue;
            }
            let n = entry.files.as_ref().map_or(0, Vec::len);
            if n > 0 && st.file_progress.len() == n {
                self.merge_file_progress(&id, &st.file_progress);
            }
        }
    }

    /// Keeps, per file, the most verified bytes ever seen. The library reports nothing for files
    /// that are not selected, so a later, narrower selection must not make a file that was
    /// downloaded earlier look empty (it would be taken for a placeholder).
    fn merge_file_progress(&self, id: &str, now: &[u64]) {
        self.registry.update(id, |e| {
            let merged: Vec<u64> = match &e.file_progress {
                Some(old) if old.len() == now.len() => {
                    old.iter().zip(now).map(|(a, b)| *a.max(b)).collect()
                }
                _ => now.to_vec(),
            };
            e.file_progress = Some(merged);
        });
    }

    /// Deletes the files of unselected content that hold nothing but the bytes of a boundary
    /// piece (see [`crate::storage::placeholders`]) and forgets that this torrent created them.
    fn drop_placeholders(&self) {
        for (id, entry) in self.registry.all() {
            let gone = crate::storage::remove_placeholders(&entry);
            if !gone.is_empty() {
                self.registry.update(&id, |e| {
                    if let Some(c) = &mut e.created {
                        c.retain(|p| !gone.contains(p));
                    }
                });
            }
        }
    }
}

fn selected_indices(entry: &Entry, n: usize) -> Vec<usize> {
    match &entry.selected {
        Some(s) => s.iter().copied().filter(|i| *i < n).collect(),
        None => (0..n).collect(),
    }
}

fn seeding_limit_hit(p: &SeedingPolicy, e: &Entry, uploaded: u64) -> bool {
    let ratio_hit = p.max_ratio.is_some_and(|r| {
        e.selected_bytes > 0 && (uploaded as f64) >= f64::from(r) * e.selected_bytes as f64
    });
    let time_hit = p.max_minutes.is_some_and(|m| {
        e.finished_at
            .is_some_and(|f| now_secs().saturating_sub(f) >= u64::from(m) * 60)
    });
    ratio_hit || time_hit
}

/// Validates a file selection against the preview; returns it sorted and deduplicated.
pub(crate) fn normalise_selection(
    preview: &TorrentPreview,
    only: Option<&[usize]>,
) -> Result<Option<Vec<usize>>> {
    let Some(only) = only else {
        return Ok(None);
    };
    if only.is_empty() {
        return Err(Error::InvalidSelection("no file selected".into()));
    }
    let mut v = only.to_vec();
    v.sort_unstable();
    v.dedup();
    for i in &v {
        match preview.files.get(*i) {
            None => {
                return Err(Error::InvalidSelection(format!(
                    "file {i} does not exist (the torrent has {} files)",
                    preview.files.len()
                )))
            }
            Some(f) if f.padding => {
                return Err(Error::InvalidSelection(format!("file {i} is padding")))
            }
            _ => {}
        }
    }
    Ok(Some(v))
}

fn validate_seeding(p: &SeedingPolicy) -> Result<()> {
    if let Some(r) = p.max_ratio {
        if !r.is_finite() || r <= 0.0 {
            return Err(Error::InvalidSettings(
                "seeding ratio must be positive".into(),
            ));
        }
    }
    if p.max_minutes == Some(0) {
        return Err(Error::InvalidSettings(
            "seeding time must be at least one minute".into(),
        ));
    }
    Ok(())
}

fn validate_settings(s: &TorrentSettings) -> Result<()> {
    validate_seeding(&s.seeding)?;
    if s.peer_limit == Some(0) {
        return Err(Error::InvalidSettings(
            "peer limit must be at least 1".into(),
        ));
    }
    Ok(())
}

fn session_options(s: &TorrentSettings, registry: Arc<Registry>) -> SessionOptions {
    let mut o = SessionOptions {
        fastresume: false,
        disable_trackers: !s.trackers,
        disable_local_service_discovery: !s.local_discovery,
        peer_limit: s.peer_limit,
        ipv4_only: s.listen_ip.is_ipv4(),
        listen: Some(ListenerOptions {
            listen_addr: std::net::SocketAddr::new(s.listen_ip, s.listen_port),
            enable_upnp_port_forwarding: s.upnp,
            ipv4_only: s.listen_ip.is_ipv4(),
            ..Default::default()
        }),
        default_storage_factory: Some(SafeStorageFactory::new(registry).boxed()),
        ..Default::default()
    };
    o.ratelimits.upload_bps = s.upload_limit_bps.and_then(NonZeroU32::new);
    o.ratelimits.download_bps = s.download_limit_bps.and_then(NonZeroU32::new);
    o.dht = s.dht.then(|| DhtSessionConfig {
        persistence: Some(DhtPersistenceConfig {
            config_filename: Some(s.state_dir.join("dht.json")),
            ..Default::default()
        }),
        ..Default::default()
    });
    o
}
