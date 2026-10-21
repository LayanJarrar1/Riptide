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
    /// Files (relative to `dir`) that this torrent's storage created itself. `None` only for an
    /// entry written by a build that did not record it: then every file of the layout counts.
    /// Removing a torrent deletes only these (and, for an adopted torrent, the files the user
    /// declared to be its data), never a name that merely sits in the layout of a torrent that
    /// never opened its files.
    #[serde(default)]
    pub created: Option<Vec<PathBuf>>,
}

impl Entry {
    /// The files that removing this torrent may delete; see [`Entry::created`].
    pub fn deletable(&self) -> Vec<PathBuf> {
        let layout = self.files.clone().unwrap_or_default();
        match &self.created {
            None => layout,
            Some(created) => {
                let mut out = created.clone();
                if self.adopt {
                    out.extend(layout);
                }
                out
            }
        }
    }
}

/// Lower-cased, separator-normalised key of a relative path.
fn path_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// A folder as the file system sees it, as a comparison key: links and junctions resolved, short
/// (8.3) names expanded, case folded. A folder that does not exist yet is resolved through its
/// deepest existing ancestor. Two spellings of one folder give one key.
pub fn normalise_dir(p: &Path) -> String {
    let mut existing = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (
            existing.file_name().map(|n| n.to_owned()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut full = std::fs::canonicalize(&existing).unwrap_or(existing);
    full.extend(rest.iter().rev());
    let key = path_key(&full);
    key.strip_prefix("//?/").map(str::to_string).unwrap_or(key)
}

/// What the other torrents have laid out in one folder; see [`Registry::claim_set`].
#[derive(Debug, Default, Clone)]
pub struct ClaimSet {
    files: std::collections::HashSet<String>,
    folders: std::collections::HashSet<String>,
}

impl ClaimSet {
    /// Whether `rel` is a file another torrent has laid out, or a folder one of them has files in.
    pub fn claims(&self, rel: &Path) -> bool {
        let k = path_key(rel);
        self.files.contains(&k) || self.folders.contains(&k)
    }
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

    /// The files every other torrent (any entry except `except`, in any state) has laid out in
    /// `dir`, compared by the folder itself and not by how its path is spelled. Build it once per
    /// planning run, not per file.
    pub fn claim_set(&self, dir: &Path, except: Option<&str>) -> ClaimSet {
        let want = normalise_dir(dir);
        let mut set = ClaimSet::default();
        for (hash, e) in self.lock().torrents.iter() {
            if Some(hash.as_str()) == except || normalise_dir(&e.dir) != want {
                continue;
            }
            for f in e.files.iter().flatten() {
                let k = path_key(f);
                let mut folder = String::new();
                for part in k
                    .split('/')
                    .collect::<Vec<_>>()
                    .split_last()
                    .map_or(&[][..], |(_, d)| d)
                {
                    if !folder.is_empty() {
                        folder.push('/');
                    }
                    folder.push_str(part);
                    set.folders.insert(folder.clone());
                }
                set.files.insert(k);
            }
        }
        set
    }

    /// Whether another torrent has laid out `rel` below `dir` (or has files in it as a folder).
    pub fn claims(&self, dir: &Path, rel: &Path, except: Option<&str>) -> bool {
        self.claim_set(dir, except).claims(rel)
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
            created: Some(Vec::new()),
        }
    }

    #[test]
    fn claims_see_every_layout_case_insensitively_and_skip_the_asker() {
        let d = tempfile::tempdir().unwrap();
        let r = Registry::in_memory();
        let mut e = entry(d.path());
        e.files = Some(vec![PathBuf::from("Set/Same.bin")]);
        r.insert("aa", e);
        let p = Path::new("set/same.BIN");
        assert!(r.claims(d.path(), p, None));
        assert!(
            !r.claims(d.path(), p, Some("aa")),
            "own layout is not a claim"
        );
        assert!(!r.claims(&d.path().join("other"), p, None), "other folder");
        assert!(
            r.claims(d.path(), Path::new("set"), None),
            "a file needs that folder"
        );
        assert!(!r.claims(d.path(), Path::new("set/other.bin"), None));
    }

    #[test]
    fn the_same_folder_spelled_differently_is_the_same_folder() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("Out");
        std::fs::create_dir_all(out.join("sub")).unwrap();
        let r = Registry::in_memory();
        let mut e = entry(&out);
        e.files = Some(vec![PathBuf::from("a.bin")]);
        r.insert("aa", e);
        let p = Path::new("a.bin");
        assert!(r.claims(&out, p, None));
        // another spelling: different case, a `..` detour, a trailing separator
        assert!(r.claims(&d.path().join("OUT").join("sub").join(".."), p, None));
        assert!(r.claims(&d.path().join("out").join(""), p, None));
        assert!(!r.claims(&out.join("sub"), p, None), "a different folder");
    }

    #[test]
    fn deletable_is_what_was_created_plus_adopted_data() {
        let d = tempfile::tempdir().unwrap();
        let mut e = entry(d.path());
        e.files = Some(vec![PathBuf::from("a"), PathBuf::from("b")]);
        e.created = Some(vec![PathBuf::from("a")]);
        assert_eq!(e.deletable(), vec![PathBuf::from("a")]);
        e.adopt = true;
        assert_eq!(e.deletable().len(), 3);
        e.adopt = false;
        e.created = None;
        assert_eq!(e.deletable().len(), 2, "legacy entries keep the old rule");
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
