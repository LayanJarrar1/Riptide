//! File operations behind Riptide's file manager: list a folder, create and rename, copy and
//! move with explicit conflict handling, send to and restore from the Recycle Bin, properties
//! and search. Nothing here knows about a UI; the desktop app wraps these in Tauri commands and
//! runs them off the async runtime.
//!
//! # Safety rules
//!
//! * **Paths** come from the user's own navigation, but are still checked: they must be
//!   absolute, may not use the Windows device namespace (`\\.\`, `\\?\GLOBALROOT`) and lose any
//!   `.` / `..` components before use ([`check_path`]).
//! * **Names** typed by the user (new folder, rename) are single path components that Windows
//!   accepts: no separators, no `<>:"|?*`, no control characters, no reserved device names, no
//!   trailing dot or space ([`validate_name`]).
//! * **Nothing is overwritten silently.** A transfer reports conflicts first ([`plan`]); the
//!   caller picks skip, keep both (`name (1).ext`) or replace, and a replaced item goes to the
//!   Recycle Bin, so it can be restored.
//! * **A move never loses the original.** Same volume: one rename. Across volumes: the item is
//!   copied in full (files through a temporary name), and only then is the source removed.
//! * **Delete** means the Recycle Bin ([`delete_to_trash`]); there is no permanent delete here.
//!
//! # Known limits
//!
//! * There is no allowed-roots restriction, by design: this is a file manager, and the user may
//!   browse and change any folder their account can. The trust boundary is the Tauri command
//!   layer, which only the app's own webview can call.
//! * Checks and actions are not atomic (time-of-check to time-of-use). Between "the name is
//!   free" and the rename, another process can create that name; [`rename`] and a same-volume
//!   move then rely on what the OS does with the race (a file may be replaced, a folder not).

#![cfg_attr(test, allow(clippy::unwrap_used))]

mod names;
mod transfer;

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

pub use names::validate_name;
pub use transfer::{
    plan, transfer, Conflict, ConflictChoice, Plan, Progress, TransferKind, TransferReport,
};

/// Errors of a file operation, phrased for the user (the UI shows `to_string()`).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The path is relative, uses the device namespace or is otherwise unusable.
    #[error("not a usable path: {0}")]
    BadPath(String),
    /// A name the file system would refuse or misread.
    #[error("{0}")]
    BadName(String),
    /// Something with that name is already there.
    #[error("\"{0}\" already exists")]
    Exists(String),
    /// The item is not there (any more).
    #[error("\"{0}\" was not found")]
    NotFound(String),
    /// A folder cannot go into itself or one of its own subfolders.
    #[error("a folder cannot be copied or moved into itself")]
    IntoItself,
    /// The operation was cancelled; finished items stay, the unfinished one is cleaned up.
    #[error("cancelled")]
    Cancelled,
    /// The Recycle Bin refused or is unavailable.
    #[error("Recycle Bin: {0}")]
    Trash(String),
    /// Any other file-system error (permissions, locked file, disk full, …).
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What a directory entry is.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// A symbolic link or junction; shown, never followed by copy or move.
    Link,
}

/// One item in a folder.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// File or folder name.
    pub name: String,
    /// Full path.
    pub path: String,
    /// File, folder or link.
    pub kind: EntryKind,
    /// Size in bytes (files only).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size: Option<u64>,
    /// Last modification, Unix milliseconds.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub modified: Option<i64>,
    /// Creation, Unix milliseconds, where the file system records it.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub created: Option<i64>,
    /// Hidden (Windows attribute, or a leading dot elsewhere).
    pub hidden: bool,
    /// Read-only.
    pub readonly: bool,
}

/// A folder's contents.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// The folder, normalised.
    pub path: String,
    /// Its parent, `None` at a drive or file-system root.
    pub parent: Option<String>,
    /// Folders first, then files, by name; the UI re-sorts as the user asks.
    pub entries: Vec<Entry>,
    /// Entries that could not be read (permissions) and are left out.
    pub skipped: u32,
}

/// Size and contents of an item, for the Properties panel.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Properties {
    /// The item itself.
    pub entry: Entry,
    /// Files inside (folders only).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub files: Option<u64>,
    /// Folders inside (folders only).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub folders: Option<u64>,
    /// Total bytes of the files inside, or the file's own size.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub total_size: u64,
    /// The count stopped at [`PROPERTIES_LIMIT`] items; the numbers are a lower bound.
    pub truncated: bool,
}

/// An item in the Recycle Bin that came from Riptide's file manager or anywhere else.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashedItem {
    /// Opaque id to pass back to [`restore_from_trash`].
    pub id: String,
    /// Name it had.
    pub name: String,
    /// Folder it was deleted from.
    pub original_parent: String,
    /// When it was deleted, Unix milliseconds.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub deleted_at: i64,
}

/// Most items a properties count or a search walks before it stops.
pub const PROPERTIES_LIMIT: u64 = 200_000;

/// An absolute, normalised path: no `.` or `..` left, no device namespace. `..` above the root
/// is refused rather than clamped.
pub fn check_path(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    if !path.is_absolute() || text.is_empty() {
        return Err(Error::BadPath(text.into_owned()));
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with(r"\\.\")
        || lower.starts_with(r"\\?\globalroot")
        || lower.starts_with("//./")
    {
        return Err(Error::BadPath(text.into_owned()));
    }
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            // `..` above the root has nothing to remove: refused, not clamped
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Error::BadPath(text.into_owned()));
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if !out.is_absolute() {
        return Err(Error::BadPath(text.into_owned()));
    }
    Ok(out)
}

/// The contents of `dir`.
pub fn list(dir: &Path) -> Result<Listing> {
    let dir = check_path(dir)?;
    let mut entries = Vec::new();
    let mut skipped = 0;
    for item in std::fs::read_dir(&dir)? {
        match item.and_then(|e| entry_of(&e.path())) {
            Ok(e) => entries.push(e),
            Err(_) => skipped += 1,
        }
    }
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Folder)
            .cmp(&(b.kind != EntryKind::Folder))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Listing {
        path: path_string(&dir),
        parent: dir.parent().map(path_string),
        entries,
        skipped,
    })
}

/// One item's details, without following a link.
pub fn entry(path: &Path) -> Result<Entry> {
    let path = check_path(path)?;
    entry_of(&path).map_err(|e| not_found_or(e, &path))
}

/// Creates `name` inside `parent`; an existing item of that name is an error, never reused.
pub fn create_folder(parent: &Path, name: &str) -> Result<Entry> {
    validate_name(name)?;
    let parent = check_path(parent)?;
    let target = parent.join(name);
    if target.symlink_metadata().is_ok() {
        return Err(Error::Exists(name.to_owned()));
    }
    std::fs::create_dir(&target)?;
    Ok(entry_of(&target)?)
}

/// Renames an item in place. A name that only differs in case is allowed (Windows treats it as
/// the same file); any other existing item of that name is an error.
pub fn rename(path: &Path, new_name: &str) -> Result<Entry> {
    validate_name(new_name)?;
    let path = check_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::BadPath(path_string(&path)))?;
    let old_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // the item being renamed gets the same scrutiny as a typed name (`file.txt:stream` is an
    // NTFS alternate data stream, not a file name)
    validate_name(&old_name)?;
    if path.symlink_metadata().is_err() {
        return Err(Error::NotFound(old_name));
    }
    let target = parent.join(new_name);
    if old_name == new_name {
        return Ok(entry_of(&path)?);
    }
    let case_only = old_name.to_lowercase() == new_name.to_lowercase();
    if !case_only && target.symlink_metadata().is_ok() {
        return Err(Error::Exists(new_name.to_owned()));
    }
    // Known limit: another process can create `target` between the check above and this rename.
    std::fs::rename(&path, &target)?;
    Ok(entry_of(&target)?)
}

/// Sends items to the Recycle Bin (the platform trash elsewhere). All or nothing per call where
/// the platform allows; a missing item is an error before anything moves.
pub fn delete_to_trash(paths: &[PathBuf]) -> Result<()> {
    let mut checked = Vec::with_capacity(paths.len());
    for p in paths {
        let p = check_path(p)?;
        if p.symlink_metadata().is_err() {
            return Err(Error::NotFound(path_string(&p)));
        }
        if p.parent().is_none() {
            return Err(Error::BadPath(path_string(&p)));
        }
        checked.push(p);
    }
    trash::delete_all(&checked).map_err(|e| Error::Trash(e.to_string()))
}

/// What the Recycle Bin holds that was deleted from `parent` (or everything, without a parent),
/// newest first. Empty where the platform cannot list it.
pub fn trashed_items(parent: Option<&Path>) -> Result<Vec<TrashedItem>> {
    let parent = parent.map(check_path).transpose()?;
    let mut out: Vec<TrashedItem> = list_trash()?
        .into_iter()
        .filter(|i| {
            parent
                .as_ref()
                .is_none_or(|p| Path::new(&i.original_parent) == p.as_path())
        })
        .collect();
    out.sort_by_key(|i| std::cmp::Reverse(i.deleted_at));
    Ok(out)
}

/// Puts items back where they were deleted from. An item whose original place is taken again
/// is not restored (the platform refuses); the error names it.
pub fn restore_from_trash(ids: &[String]) -> Result<()> {
    restore_trash(ids)
}

/// Size and contents, counted without following links and stopping at [`PROPERTIES_LIMIT`].
pub fn properties(path: &Path, cancel: &AtomicBool) -> Result<Properties> {
    let entry = entry(path)?;
    if entry.kind != EntryKind::Folder {
        return Ok(Properties {
            total_size: entry.size.unwrap_or(0),
            entry,
            files: None,
            folders: None,
            truncated: false,
        });
    }
    let (mut files, mut folders, mut total, mut seen) = (0u64, 0u64, 0u64, 0u64);
    let mut stack = vec![PathBuf::from(&entry.path)];
    let mut truncated = false;
    'walk: while let Some(dir) = stack.pop() {
        let Ok(items) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            seen += 1;
            if seen > PROPERTIES_LIMIT {
                truncated = true;
                break 'walk;
            }
            let Ok(meta) = item.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                folders += 1;
                stack.push(item.path());
            } else if meta.is_file() {
                files += 1;
                total += meta.len();
            }
        }
    }
    Ok(Properties {
        entry,
        files: Some(files),
        folders: Some(folders),
        total_size: total,
        truncated,
    })
}

/// Items under `dir` whose name contains `query` (case-insensitive), breadth first, at most
/// `limit` results; links are listed but not entered.
pub fn search(dir: &Path, query: &str, limit: usize, cancel: &AtomicBool) -> Result<Vec<Entry>> {
    let dir = check_path(dir)?;
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([dir]);
    let mut seen = 0u64;
    while let Some(dir) = queue.pop_front() {
        let Ok(items) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            seen += 1;
            if seen > PROPERTIES_LIMIT {
                return Ok(out);
            }
            let path = item.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                queue.push_back(path.clone());
            }
            if item
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains(&needle)
            {
                if let Ok(e) = entry_of(&path) {
                    out.push(e);
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn not_found_or(e: std::io::Error, path: &Path) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::NotFound(path_string(path))
    } else {
        Error::Io(e)
    }
}

fn millis(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let d = t.ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

pub(crate) fn entry_of(path: &Path) -> std::io::Result<Entry> {
    let meta = path.symlink_metadata()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_string(path));
    let kind = if meta.file_type().is_symlink() || is_reparse_point(&meta) {
        EntryKind::Link
    } else if meta.is_dir() {
        EntryKind::Folder
    } else {
        EntryKind::File
    };
    Ok(Entry {
        hidden: is_hidden(&meta, &name),
        readonly: meta.permissions().readonly(),
        size: (kind == EntryKind::File).then_some(meta.len()),
        modified: millis(meta.modified()),
        created: millis(meta.created()),
        path: path_string(path),
        name,
        kind,
    })
}

#[cfg(windows)]
fn is_hidden(meta: &std::fs::Metadata, _name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn is_hidden(_meta: &std::fs::Metadata, name: &str) -> bool {
    name.starts_with('.')
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn list_trash() -> Result<Vec<TrashedItem>> {
    let items = trash::os_limited::list().map_err(|e| Error::Trash(e.to_string()))?;
    Ok(items
        .into_iter()
        .map(|i| TrashedItem {
            id: i.id.to_string_lossy().into_owned(),
            name: i.name.to_string_lossy().into_owned(),
            original_parent: path_string(&i.original_parent),
            deleted_at: i.time_deleted.saturating_mul(1000),
        })
        .collect())
}

#[cfg(not(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
fn list_trash() -> Result<Vec<TrashedItem>> {
    Ok(Vec::new())
}

#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn restore_trash(ids: &[String]) -> Result<()> {
    let wanted: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let items: Vec<trash::TrashItem> = trash::os_limited::list()
        .map_err(|e| Error::Trash(e.to_string()))?
        .into_iter()
        .filter(|i| wanted.contains(i.id.to_string_lossy().as_ref()))
        .collect();
    if items.len() != wanted.len() {
        return Err(Error::Trash(
            "some items are no longer in the Recycle Bin".into(),
        ));
    }
    trash::os_limited::restore_all(items).map_err(|e| Error::Trash(e.to_string()))
}

#[cfg(not(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
fn restore_trash(_ids: &[String]) -> Result<()> {
    Err(Error::Trash(
        "restoring is not supported on this system".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn paths_must_be_absolute_and_lose_dot_segments() {
        assert!(check_path(Path::new("relative/x")).is_err());
        assert!(check_path(Path::new("")).is_err());
        let t = tmp();
        let p = t.path().join("a").join("..").join("b").join(".");
        assert_eq!(check_path(&p).unwrap(), t.path().join("b"));
        #[cfg(windows)]
        {
            assert!(check_path(Path::new(r"\\.\PhysicalDrive0")).is_err());
            assert!(check_path(Path::new(r"\\?\GLOBALROOT\Device\X")).is_err());
        }
    }

    #[test]
    fn lists_folders_first_and_counts_what_it_could_not_read() {
        let t = tmp();
        std::fs::write(t.path().join("b.txt"), b"hello").unwrap();
        std::fs::write(t.path().join("A.txt"), b"x").unwrap();
        std::fs::create_dir(t.path().join("zeta")).unwrap();
        let l = list(t.path()).unwrap();
        let names: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zeta", "A.txt", "b.txt"]);
        assert_eq!(l.entries[0].kind, EntryKind::Folder);
        assert_eq!(l.entries[2].size, Some(5));
        assert_eq!(
            l.parent.as_deref(),
            t.path().parent().map(path_string).as_deref()
        );
        assert_eq!(l.skipped, 0);
    }

    #[test]
    fn creates_and_renames_without_clobbering() {
        let t = tmp();
        let f = create_folder(t.path(), "Projects").unwrap();
        assert_eq!(f.kind, EntryKind::Folder);
        assert!(matches!(
            create_folder(t.path(), "Projects"),
            Err(Error::Exists(_))
        ));
        assert!(matches!(
            create_folder(t.path(), "a/b"),
            Err(Error::BadName(_))
        ));

        std::fs::write(t.path().join("note.txt"), b"1").unwrap();
        assert!(matches!(
            rename(&t.path().join("note.txt"), "Projects"),
            Err(Error::Exists(_))
        ));
        let r = rename(&t.path().join("note.txt"), "Note.txt").unwrap();
        assert_eq!(r.name, "Note.txt");
        assert!(matches!(
            rename(&t.path().join("missing"), "x"),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            rename(&t.path().join("Note.txt"), "..\\escape"),
            Err(Error::BadName(_))
        ));
    }

    /// Uses the real Recycle Bin (the freedesktop trash on Linux): what goes in comes back.
    #[test]
    fn delete_goes_to_the_recycle_bin_and_restore_brings_it_back() {
        let t = tmp();
        let parent = t.path().join("riptide-trash-test");
        std::fs::create_dir(&parent).unwrap();
        let file = parent.join("old report.txt");
        std::fs::write(&file, b"keep").unwrap();
        assert!(matches!(
            delete_to_trash(&[parent.join("missing")]),
            Err(Error::NotFound(_))
        ));
        if delete_to_trash(std::slice::from_ref(&file)).is_err() {
            // no usable trash on this machine (sandboxed CI): nothing more to check here
            return;
        }
        assert!(!file.exists());
        let items = trashed_items(Some(&parent)).unwrap();
        if items.is_empty() {
            return; // the platform cannot list its trash
        }
        assert_eq!(items[0].name, "old report.txt");
        restore_from_trash(&[items[0].id.clone()]).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");
        assert!(trashed_items(Some(&parent)).unwrap().is_empty());
    }

    #[test]
    fn properties_count_a_tree_and_search_finds_by_name() {
        let t = tmp();
        let d = t.path().join("docs");
        std::fs::create_dir_all(d.join("2026").join("q3")).unwrap();
        std::fs::write(d.join("report.pdf"), vec![0u8; 100]).unwrap();
        std::fs::write(d.join("2026").join("Report-final.PDF"), vec![0u8; 50]).unwrap();
        std::fs::write(d.join("2026").join("q3").join("التقرير.pdf"), vec![0u8; 7]).unwrap();
        let stop = AtomicBool::new(false);
        let p = properties(&d, &stop).unwrap();
        assert_eq!((p.files, p.folders, p.total_size), (Some(3), Some(2), 157));
        assert!(!p.truncated);

        let found = search(&d, "report", 10, &stop).unwrap();
        assert_eq!(found.len(), 2);
        let arabic = search(&d, "التقرير", 10, &stop).unwrap();
        assert_eq!(arabic.len(), 1);
        assert_eq!(
            search(&d, "pdf", 1, &stop).unwrap().len(),
            1,
            "stops at the limit"
        );
        stop.store(true, Ordering::Relaxed);
        assert!(matches!(
            search(&d, "pdf", 10, &stop),
            Err(Error::Cancelled)
        ));
    }
}
