//! Looking at and editing the torrent state without starting an engine.
//!
//! The command line uses this for `list`, `pause` and `remove`: none of them should open
//! sockets, bootstrap the DHT or re-hash gigabytes just to print a table. Everything works on
//! the [`Registry`] file and the stored `.torrent` files of a state folder, with the same
//! confinement rules as the engine. Only use it while no engine runs on the same folder.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::registry::{Entry, Registry};
use crate::storage::remove_laid_out_files;
use crate::types::{TorrentSnapshot, TorrentState};

/// File name of the registry inside the state folder.
pub const REGISTRY_FILE: &str = "riptide-torrents.json";

fn open(state_dir: &Path) -> Result<Registry> {
    Registry::open(state_dir.join(REGISTRY_FILE))
}

/// Snapshot of a torrent that is not being run by an engine.
pub fn snapshot(id: &str, entry: &Entry) -> TorrentSnapshot {
    let state = match (&entry.error, entry.completed, entry.user_paused) {
        (Some(error), false, _) => TorrentState::Failed {
            error: error.clone(),
        },
        (_, true, _) => TorrentState::Completed,
        (_, false, true) => TorrentState::Paused,
        (_, false, false) => TorrentState::Checking,
    };
    TorrentSnapshot {
        id: id.to_string(),
        name: entry.name.clone(),
        state,
        dir: entry.dir.clone(),
        total_bytes: entry.selected_bytes,
        downloaded_bytes: if entry.completed {
            entry.selected_bytes
        } else {
            entry.progress_bytes
        },
        uploaded_bytes: 0,
        down_bps: 0,
        up_bps: 0,
        peers_connected: 0,
        peers_seen: 0,
        eta_secs: None,
        file_progress: Vec::new(),
        selected: entry.selected.clone(),
    }
}

/// All torrents recorded in `state_dir`.
pub fn list(state_dir: &Path) -> Result<Vec<TorrentSnapshot>> {
    Ok(open(state_dir)?
        .all()
        .iter()
        .map(|(id, e)| snapshot(id, e))
        .collect())
}

/// Expands a unique prefix (at least 6 hex digits) of an info-hash to the full id.
pub fn resolve_id(state_dir: &Path, prefix: &str) -> Result<String> {
    let prefix = prefix.trim().to_ascii_lowercase();
    if prefix.len() < 6 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::NotFound(prefix));
    }
    let hits: Vec<String> = open(state_dir)?
        .all()
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| id.starts_with(&prefix))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(Error::NotFound(prefix)),
    }
}

/// Marks a torrent paused (or not). A paused torrent is not started by the next engine.
pub fn set_paused(state_dir: &Path, id: &str, paused: bool) -> Result<()> {
    let reg = open(state_dir)?;
    if !reg.update(id, |e| e.user_paused = paused) {
        return Err(Error::NotFound(id.to_string()));
    }
    reg.flush()
}

/// Forgets a torrent; with `delete_files` also removes the files of its planned layout (only
/// those, only below the folder it was added to).
pub fn remove(state_dir: &Path, id: &str, delete_files: bool) -> Result<()> {
    let reg = open(state_dir)?;
    let entry = reg
        .remove(id)
        .ok_or_else(|| Error::NotFound(id.to_string()))?;
    if delete_files {
        remove_laid_out_files(&entry.dir, &entry.deletable());
    }
    reg.flush()?;
    if id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        let _ = std::fs::remove_file(
            state_dir
                .join("torrents")
                .join(format!("{id}.torrent"))
                .as_path(),
        );
    }
    Ok(())
}

/// Forgets every torrent whose id is not in `keep`, deleting the unfinished data of those (a
/// finished torrent keeps its files). Returns the ids that were dropped.
///
/// The desktop app calls it before it starts the engine, with the torrents that still have a row
/// in the list: a registry entry without a row is a torrent the user removed or cancelled while
/// the engine was not running, and must not come back to life.
pub fn retain(state_dir: &Path, keep: &std::collections::HashSet<String>) -> Result<Vec<String>> {
    let reg = open(state_dir)?;
    let mut dropped = Vec::new();
    for (id, entry) in reg.all() {
        if keep.contains(&id) {
            continue;
        }
        reg.remove(&id);
        if !entry.completed {
            remove_laid_out_files(&entry.dir, &entry.deletable());
        }
        if id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            let _ = std::fs::remove_file(state_dir.join("torrents").join(format!("{id}.torrent")));
        }
        dropped.push(id);
    }
    if !dropped.is_empty() {
        reg.flush()?;
    }
    Ok(dropped)
}

/// The folder a state folder keeps stored `.torrent` files in (for diagnostics).
pub fn torrents_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("torrents")
}
