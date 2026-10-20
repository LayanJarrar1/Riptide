//! Riptide's own record of every torrent: where it lives, what was selected, how far it got.
//!
//! librqbit persists the torrents themselves (metadata, pause state); this small JSON file holds
//! what only Riptide knows: the folder the user chose, the planned (sanitised) file layout, the
//! file selection and the completion flag. It is written atomically (temp file + rename) and is
//! the reason a restart after a crash finds the same files at the same paths.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Persisted facts about one torrent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// The folder chosen by the user; every file lives below it.
    pub dir: PathBuf,
    /// Planned relative path of every file (set when the metadata is known).
    #[serde(default)]
    pub files: Option<Vec<PathBuf>>,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Size of the whole torrent.
    #[serde(default)]
    pub total_bytes: u64,
    /// Bytes selected for download.
    #[serde(default)]
    pub selected_bytes: u64,
    /// Selected file indices (`None` = everything).
    #[serde(default)]
    pub selected: Option<Vec<usize>>,
    /// Download files one after the other, in order.
    #[serde(default)]
    pub sequential: bool,
    /// Existing files in the folder are the data of this torrent: verify and reuse them.
    #[serde(default)]
    pub adopt: bool,
    /// The user paused it; it stays paused across restarts.
    #[serde(default)]
    pub user_paused: bool,
    /// Every selected piece was verified.
    #[serde(default)]
    pub completed: bool,
    /// Seeding has reached its limit (or was never wanted).
    #[serde(default)]
    pub seeding_done: bool,
    /// Unix time the torrent was added.
    #[serde(default)]
    pub added_at: u64,
    /// Unix time all selected pieces were verified.
    #[serde(default)]
    pub finished_at: Option<u64>,
    /// Bytes downloaded and verified at the last flush (for offline listings).
    #[serde(default)]
    pub progress_bytes: u64,
    /// Last error, if any.
    #[serde(default)]
    pub error: Option<String>,
    /// Peers the user supplied (`ip:port`); tried again after a restart.
    #[serde(default)]
    pub peers: Vec<std::net::SocketAddr>,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    torrents: BTreeMap<String, Entry>,
}

/// The registry; cheap to share behind an `Arc`.
pub struct Registry {
    path: Option<PathBuf>,
    inner: Mutex<File>,
}

impl Registry {
    /// Loads the registry stored at `path` (a missing file means an empty registry; a corrupt
    /// one is an error rather than silently forgetting the user's torrents).
    pub fn open(path: PathBuf) -> Result<Self> {
        let file = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b)
                .map_err(|e| Error::State(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path: Some(path),
            inner: Mutex::new(file),
        })
    }

    /// A registry that is never written to disk (tests, previews).
    pub fn in_memory() -> Self {
        Self {
            path: None,
            inner: Mutex::new(File::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, File> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Entry for an info-hash.
    pub fn get(&self, hash: &str) -> Option<Entry> {
        self.lock().torrents.get(hash).cloned()
    }

    /// All entries.
    pub fn all(&self) -> Vec<(String, Entry)> {
        self.lock()
            .torrents
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Inserts or replaces an entry (not yet flushed).
    pub fn insert(&self, hash: &str, entry: Entry) {
        self.lock().torrents.insert(hash.to_string(), entry);
    }

    /// Removes an entry (not yet flushed).
    pub fn remove(&self, hash: &str) -> Option<Entry> {
        self.lock().torrents.remove(hash)
    }

    /// Records the planned file layout.
    pub fn set_layout(&self, hash: &str, files: Vec<PathBuf>) {
        if let Some(e) = self.lock().torrents.get_mut(hash) {
            e.files = Some(files);
        }
    }

    /// Changes an entry in place; returns whether it exists.
    pub fn update(&self, hash: &str, f: impl FnOnce(&mut Entry)) -> bool {
        match self.lock().torrents.get_mut(hash) {
            Some(e) => {
                f(e);
                true
            }
            None => false,
        }
    }

    /// Writes the registry atomically.
    pub fn flush(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let data = {
            let g = self.lock();
            serde_json::to_vec_pretty(&*g).map_err(|e| Error::State(e.to_string()))?
        };
        write_atomic(path, &data)
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: &Path) -> Entry {
        Entry {
            dir: dir.to_path_buf(),
            files: None,
            name: "n".into(),
            total_bytes: 10,
            selected_bytes: 10,
            selected: None,
            sequential: false,
            adopt: false,
            user_paused: false,
            completed: false,
            seeding_done: false,
            added_at: 1,
            finished_at: None,
            progress_bytes: 0,
            error: None,
            peers: Vec::new(),
        }
    }

    #[test]
    fn round_trips_through_disk() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.json");
        let r = Registry::open(p.clone()).unwrap();
        r.insert("ab", entry(d.path()));
        r.set_layout("ab", vec![PathBuf::from("x/y")]);
        r.flush().unwrap();
        let r2 = Registry::open(p).unwrap();
        assert_eq!(
            r2.get("ab").unwrap().files.unwrap(),
            vec![PathBuf::from("x/y")]
        );
    }

    #[test]
    fn corrupt_file_is_an_error_not_an_empty_registry() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.json");
        std::fs::write(&p, b"{not json").unwrap();
        assert!(Registry::open(p).is_err());
    }
}
