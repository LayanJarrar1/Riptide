//! The file manager's commands: thin wrappers over `riptide-fs`, run off the async runtime.
//!
//! These are the only commands through which the webview reaches the file system beyond the
//! downloads it manages. Every path is checked by `riptide_fs::check_path` (absolute, no device
//! namespace, no `..`), every typed name by `riptide_fs::validate_name`; a delete goes to the
//! Recycle Bin and a transfer never overwrites without the user's explicit "replace", which
//! itself sends the old item to the Recycle Bin.
//!
//! | command | does |
//! |---|---|
//! | `fs_places` | Downloads, the category root, the profile folders and the drives |
//! | `fs_capabilities` | whether Recycle Bin items can be listed and restored here |
//! | `fs_list` / `fs_entry` | a folder's contents / one item |
//! | `fs_create_folder` / `fs_rename` | new folder / rename in place |
//! | `fs_trash` / `fs_trashed` / `fs_restore` | Recycle Bin |
//! | `fs_properties` / `fs_search` | size and counts / search under a folder |
//! | `fs_plan` / `fs_transfer` / `fs_cancel` | copy or move: conflicts and space first, then the transfer with `riptide://fs-progress` events |

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riptide_engine::Engine;
use riptide_fs::{
    ConflictChoice, Entry, Listing, Plan, Progress, Properties, TransferKind, TransferReport,
    TrashedItem,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// Progress of a running transfer, at most ten per second per transfer.
pub const FS_PROGRESS_EVENT: &str = "riptide://fs-progress";

/// Most results a search returns.
const SEARCH_LIMIT: usize = 500;
/// Gap between two progress events of one transfer.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Runs a blocking file operation on the blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> riptide_fs::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(err)?
        .map_err(err)
}

/// Running transfers and searches by the id the UI chose, for `fs_cancel`.
#[derive(Default)]
pub struct FileOps {
    running: Mutex<HashMap<u64, Arc<AtomicBool>>>,
}

impl FileOps {
    fn start(&self, id: u64) -> CmdResult<Arc<AtomicBool>> {
        let mut map = self.running.lock().map_err(err)?;
        if map.contains_key(&id) {
            return Err("an operation with this id is already running".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        map.insert(id, Arc::clone(&flag));
        Ok(flag)
    }

    fn finish(&self, id: u64) {
        if let Ok(mut map) = self.running.lock() {
            map.remove(&id);
        }
    }
}

/// A place in the sidebar.
#[derive(Debug, Clone, Serialize)]
pub struct Place {
    /// `downloads`, `organized`, `home`, `desktop`, `documents`, `pictures`, `music`, `videos`
    /// or `drive`.
    pub key: &'static str,
    /// Shown as is for drives (`C:\`); the UI translates the others.
    pub label: String,
    /// The folder.
    pub path: String,
}

/// Where the user can start browsing. Folders that do not exist are left out.
#[tauri::command]
pub async fn fs_places(engine: State<'_, Engine>) -> CmdResult<Vec<Place>> {
    let settings = engine.settings();
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = Vec::new();
        let mut push = |key: &'static str, path: Option<PathBuf>| {
            if let Some(p) = path.filter(|p| p.is_dir()) {
                let path = p.to_string_lossy().into_owned();
                if !out.iter().any(|x: &Place| x.path == path) {
                    out.push(Place {
                        key,
                        label: path.clone(),
                        path,
                    });
                }
            }
        };
        push("downloads", Some(PathBuf::from(&settings.default_dir)));
        if settings.organize {
            push("organized", Some(settings.organized_root()));
        }
        push("home", dirs::home_dir());
        push("desktop", dirs::desktop_dir());
        push("documents", dirs::document_dir());
        push("pictures", dirs::picture_dir());
        push("music", dirs::audio_dir());
        push("videos", dirs::video_dir());
        for root in drives() {
            push("drive", Some(root));
        }
        out
    })
    .await
    .map_err(err)
}

#[cfg(windows)]
fn drives() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|l| PathBuf::from(format!("{}:\\", l as char)))
        .filter(|p| p.is_dir())
        .collect()
}

/// macOS: the start-up disk and every mounted volume (external drives, disk images, shares).
#[cfg(target_os = "macos")]
fn drives() -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from("/")];
    if let Ok(items) = std::fs::read_dir("/Volumes") {
        let mut volumes: Vec<PathBuf> = items
            .flatten()
            .map(|e| e.path())
            // the start-up disk also appears here as a link to `/`
            .filter(|p| p.is_dir() && std::fs::read_link(p).is_err())
            .collect();
        volumes.sort();
        out.extend(volumes);
    }
    out
}

#[cfg(not(any(windows, target_os = "macos")))]
fn drives() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

/// What the file manager can do on this system.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCapabilities {
    /// Recycle Bin items can be listed and restored (not on macOS, where Finder does it).
    pub can_restore: bool,
}

/// What the file manager can do here.
#[tauri::command]
pub fn fs_capabilities() -> FsCapabilities {
    FsCapabilities {
        can_restore: riptide_fs::CAN_RESTORE,
    }
}

/// A folder's contents.
#[tauri::command]
pub async fn fs_list(path: String) -> CmdResult<Listing> {
    blocking(move || riptide_fs::list(&PathBuf::from(path))).await
}

/// One item.
#[tauri::command]
pub async fn fs_entry(path: String) -> CmdResult<Entry> {
    blocking(move || riptide_fs::entry(&PathBuf::from(path))).await
}

/// A new, empty folder.
#[tauri::command]
pub async fn fs_create_folder(parent: String, name: String) -> CmdResult<Entry> {
    blocking(move || riptide_fs::create_folder(&PathBuf::from(parent), &name)).await
}

/// Renames in place.
#[tauri::command]
pub async fn fs_rename(path: String, name: String) -> CmdResult<Entry> {
    blocking(move || riptide_fs::rename(&PathBuf::from(path), &name)).await
}

/// To the Recycle Bin.
#[tauri::command]
pub async fn fs_trash(paths: Vec<String>) -> CmdResult<()> {
    blocking(move || {
        riptide_fs::delete_to_trash(&paths.into_iter().map(PathBuf::from).collect::<Vec<_>>())
    })
    .await
}

/// What the Recycle Bin holds from `parent` (everything without one).
#[tauri::command]
pub async fn fs_trashed(parent: Option<String>) -> CmdResult<Vec<TrashedItem>> {
    blocking(move || riptide_fs::trashed_items(parent.map(PathBuf::from).as_deref())).await
}

/// Back from the Recycle Bin.
#[tauri::command]
pub async fn fs_restore(ids: Vec<String>) -> CmdResult<()> {
    blocking(move || riptide_fs::restore_from_trash(&ids)).await
}

/// Size and contents of an item; cancellable with `fs_cancel(id)`.
#[tauri::command]
pub async fn fs_properties(
    ops: State<'_, Arc<FileOps>>,
    id: u64,
    path: String,
) -> CmdResult<Properties> {
    let ops = Arc::clone(ops.inner());
    let cancel = ops.start(id)?;
    let out = blocking(move || riptide_fs::properties(&PathBuf::from(path), &cancel)).await;
    ops.finish(id);
    out
}

/// Items under `dir` whose name contains `query`; cancellable with `fs_cancel(id)`.
#[tauri::command]
pub async fn fs_search(
    ops: State<'_, Arc<FileOps>>,
    id: u64,
    dir: String,
    query: String,
) -> CmdResult<Vec<Entry>> {
    let ops = Arc::clone(ops.inner());
    let cancel = ops.start(id)?;
    let out =
        blocking(move || riptide_fs::search(&PathBuf::from(dir), &query, SEARCH_LIMIT, &cancel))
            .await;
    ops.finish(id);
    out
}

/// What a copy or move would do, plus the free space of the destination, so the UI can ask
/// about conflicts and warn about space before anything is written.
#[derive(Debug, Clone, Serialize)]
pub struct TransferPlan {
    #[serde(flatten)]
    plan: Plan,
    /// Free bytes on the destination volume, `None` if unknown.
    free: Option<u64>,
}

/// Plans a copy or move.
#[tauri::command]
pub async fn fs_plan(
    sources: Vec<String>,
    dest: String,
    kind: TransferKind,
) -> CmdResult<TransferPlan> {
    tauri::async_runtime::spawn_blocking(move || {
        let dest = PathBuf::from(dest);
        let sources: Vec<PathBuf> = sources.into_iter().map(PathBuf::from).collect();
        let plan = riptide_fs::plan(&sources, &dest, kind).map_err(err)?;
        let free = riptide_engine::free_space(&dest).ok();
        Ok(TransferPlan { plan, free })
    })
    .await
    .map_err(err)?
}

/// Progress event payload.
#[derive(Debug, Clone, Serialize)]
struct ProgressEvent<'a> {
    id: u64,
    #[serde(flatten)]
    progress: &'a Progress,
}

/// Copies or moves; progress arrives as `riptide://fs-progress` events, the report as the
/// result. Refused when the destination lacks the space the plan needs.
#[tauri::command]
pub async fn fs_transfer(
    app: AppHandle,
    ops: State<'_, Arc<FileOps>>,
    id: u64,
    sources: Vec<String>,
    dest: String,
    kind: TransferKind,
    choice: ConflictChoice,
) -> CmdResult<TransferReport> {
    let ops = Arc::clone(ops.inner());
    let cancel = ops.start(id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let dest = PathBuf::from(dest);
        let sources: Vec<PathBuf> = sources.into_iter().map(PathBuf::from).collect();
        let plan = riptide_fs::plan(&sources, &dest, kind).map_err(err)?;
        if let Ok(free) = riptide_engine::free_space(&dest) {
            if plan.needs_space > free {
                return Err(format!(
                    "not enough space: {} bytes needed, {} bytes free",
                    plan.needs_space, free
                ));
            }
        }
        let mut last = Instant::now() - PROGRESS_EVERY;
        let mut emit = |p: &Progress| {
            if last.elapsed() >= PROGRESS_EVERY || p.done_bytes >= p.total_bytes {
                last = Instant::now();
                if let Err(e) = app.emit(FS_PROGRESS_EVENT, ProgressEvent { id, progress: p }) {
                    tracing::debug!(error = %e, "fs progress not delivered");
                }
            }
        };
        riptide_fs::transfer(&sources, &dest, kind, choice, &mut emit, &cancel).map_err(err)
    })
    .await
    .map_err(err);
    ops.finish(id);
    result?
}

/// Stops a running transfer, properties count or search. Unknown ids are ignored (it ended).
#[tauri::command]
pub fn fs_cancel(ops: State<'_, Arc<FileOps>>, id: u64) {
    if let Ok(map) = ops.running.lock() {
        if let Some(flag) = map.get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_runs_once_at_a_time_and_is_free_again_when_finished() {
        let ops = FileOps::default();
        let flag = ops.start(7).unwrap();
        assert!(ops.start(7).is_err());
        assert!(!flag.load(Ordering::Relaxed));
        ops.finish(7);
        assert!(ops.start(7).is_ok());
    }
}
