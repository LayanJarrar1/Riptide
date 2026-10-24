//! Copy and move with explicit conflicts, progress, cancellation and no silent overwrite.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::names::validate_existing_name;
use crate::{check_path, path_string, Error, Result, PROPERTIES_LIMIT};

/// Copy or move.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferKind {
    /// The sources stay.
    Copy,
    /// The sources go; removed only once their copy is complete.
    Move,
}

/// What to do when the destination already has an item of the same name.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictChoice {
    /// Leave that item alone.
    Skip,
    /// Add it under a free name, `name (1).ext`.
    KeepBoth,
    /// Send the existing item to the Recycle Bin, then put this one in its place.
    Replace,
}

/// An item whose name is already taken in the destination.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// The item being copied or moved.
    pub source: String,
    /// The item already there.
    pub target: String,
    /// Whether the existing item is a folder.
    pub target_is_folder: bool,
}

/// What a transfer would do, before anything happens.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Files to copy (counted inside folders).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub files: u64,
    /// Their bytes.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes: u64,
    /// Bytes that will need new space in the destination: everything for a copy, the sources
    /// on another volume for a move (a move on one volume is a rename).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub needs_space: u64,
    /// Names already taken in the destination; the caller asks the user before going on.
    pub conflicts: Vec<Conflict>,
    /// Sources a move would leave where they are (already in the destination).
    pub unchanged: Vec<String>,
}

/// Progress of a running transfer.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Bytes written so far.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub done_bytes: u64,
    /// Bytes in all (from the plan; a rename counts as instant).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub total_bytes: u64,
    /// The item being worked on.
    pub current: String,
}

/// One finished item.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    /// Where it came from.
    pub from: String,
    /// Where it is now (or where its copy is).
    pub to: String,
}

/// One item that could not be transferred.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failed {
    /// The source item.
    pub path: String,
    /// Why, for the user.
    pub error: String,
}

/// What a transfer did.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferReport {
    /// Items now in the destination.
    pub done: Vec<Moved>,
    /// Items left alone (conflict skipped, or already there).
    pub skipped: Vec<String>,
    /// Items that failed; the others went on.
    pub failed: Vec<Failed>,
    /// The user stopped it; the item in progress was cleaned up.
    pub cancelled: bool,
}

/// Suffix of a file being copied; renamed to its real name once complete.
const PART_SUFFIX: &str = ".riptide-part";
/// Copy buffer.
const CHUNK: usize = 1 << 20;

struct Source {
    path: PathBuf,
    name: String,
}

fn sources_in(sources: &[PathBuf], dest: &Path) -> Result<Vec<Source>> {
    let mut out = Vec::with_capacity(sources.len());
    for s in sources {
        let path = check_path(s)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::BadPath(path_string(&path)))?;
        // an existing item keeps whatever name it has (so it can be moved out of a bad name);
        // only an alternate data stream such as `file.txt:stream` is refused
        validate_existing_name(&name)?;
        if path.symlink_metadata().is_err() {
            return Err(Error::NotFound(name));
        }
        if is_inside(dest, &path) {
            return Err(Error::IntoItself);
        }
        out.push(Source { path, name });
    }
    Ok(out)
}

/// Whether two paths name the same place after resolving links and junctions in their existing
/// parts: case-insensitive on Windows (NTFS is, by default), exact elsewhere.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    same_path_with(a, b, cfg!(windows))
}

fn same_path_with(a: &Path, b: &Path, ignore_case: bool) -> bool {
    let (a, b) = (resolve_lenient(a), resolve_lenient(b));
    if ignore_case {
        fold(&a) == fold(&b)
    } else {
        a == b
    }
}

/// Whether two paths are the same item: like [`same_path`], but the last component is not
/// resolved, so a link and what it points to stay different items (as in [`is_inside`]).
fn same_item(a: &Path, b: &Path) -> bool {
    let resolve = |p: &Path| match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => resolve_lenient(parent).join(name),
        _ => resolve_lenient(p),
    };
    let (a, b) = (resolve(a), resolve(b));
    if cfg!(windows) {
        fold(&a) == fold(&b)
    } else {
        a == b
    }
}

/// The path with every component lower-cased, for case-insensitive comparison.
fn fold(p: &Path) -> PathBuf {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// Canonicalizes the deepest existing ancestor of `path` (so links and junctions are resolved)
/// and appends the part that does not exist yet.
fn resolve_lenient(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(real) = cur.canonicalize() {
            let mut out = real;
            out.extend(rest.iter().rev());
            return out;
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether `inner` is `outer` or lies below it, after resolving links and junctions in the
/// existing parts of both. `outer` itself is not resolved at its last component: a link that is
/// moved or copied is the link, not what it points to.
fn is_inside(inner: &Path, outer: &Path) -> bool {
    is_inside_with(inner, outer, cfg!(windows))
}

fn is_inside_with(inner: &Path, outer: &Path, ignore_case: bool) -> bool {
    let inner = resolve_lenient(inner);
    let outer = match (outer.parent(), outer.file_name()) {
        (Some(parent), Some(name)) => resolve_lenient(parent).join(name),
        _ => resolve_lenient(outer),
    };
    if ignore_case {
        fold(&inner).starts_with(fold(&outer))
    } else {
        inner.starts_with(&outer)
    }
}

fn checked_dest(dest: &Path) -> Result<PathBuf> {
    let dest = check_path(dest)?;
    if !dest.is_dir() {
        return Err(Error::NotFound(path_string(&dest)));
    }
    Ok(dest)
}

/// Counts what `sources` hold and finds the names already taken in `dest`.
pub fn plan(sources: &[PathBuf], dest: &Path, kind: TransferKind) -> Result<Plan> {
    let dest = checked_dest(dest)?;
    let sources = sources_in(sources, &dest)?;
    let mut plan = Plan {
        files: 0,
        bytes: 0,
        needs_space: 0,
        conflicts: Vec::new(),
        unchanged: Vec::new(),
    };
    for s in &sources {
        if kind == TransferKind::Move && s.path.parent().is_some_and(|p| same_path(p, &dest)) {
            plan.unchanged.push(path_string(&s.path));
            continue;
        }
        let (files, bytes) = tree_size(&s.path);
        plan.files += files;
        plan.bytes += bytes;
        if kind == TransferKind::Copy || !same_volume(&s.path, &dest) {
            plan.needs_space += bytes;
        }
        let target = dest.join(&s.name);
        if let Ok(meta) = target.symlink_metadata() {
            plan.conflicts.push(Conflict {
                source: path_string(&s.path),
                target: path_string(&target),
                target_is_folder: meta.is_dir(),
            });
        }
    }
    Ok(plan)
}

/// Copies or moves `sources` into `dest`, resolving every name conflict with `choice`. Items
/// are independent: one that fails is reported and the rest go on. `on_progress` is called
/// after every chunk; the caller throttles what it forwards.
pub fn transfer(
    sources: &[PathBuf],
    dest: &Path,
    kind: TransferKind,
    choice: ConflictChoice,
    on_progress: &mut dyn FnMut(&Progress),
    cancel: &AtomicBool,
) -> Result<TransferReport> {
    let dest = checked_dest(dest)?;
    let sources = sources_in(sources, &dest)?;
    let total_bytes = sources.iter().map(|s| tree_size(&s.path).1).sum();
    let mut ctx = Ctx {
        progress: Progress {
            done_bytes: 0,
            total_bytes,
            current: String::new(),
        },
        on_progress,
        cancel,
        skipped_links: 0,
    };
    let mut report = TransferReport::default();
    for s in &sources {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let from = path_string(&s.path);
        if kind == TransferKind::Move && s.path.parent().is_some_and(|p| same_path(p, &dest)) {
            report.skipped.push(from);
            continue;
        }
        let mut target = dest.join(&s.name);
        if target.symlink_metadata().is_ok() {
            match choice {
                ConflictChoice::Skip => {
                    report.skipped.push(from);
                    continue;
                }
                ConflictChoice::KeepBoth => {
                    target =
                        riptide_core::unique_path(&dest, &s.name, |p| p.symlink_metadata().is_ok());
                }
                // copying an item onto itself can only mean "make a copy"
                ConflictChoice::Replace if same_item(&target, &s.path) => {
                    target =
                        riptide_core::unique_path(&dest, &s.name, |p| p.symlink_metadata().is_ok());
                }
                ConflictChoice::Replace => {
                    if let Err(e) = trash::delete(&target) {
                        report.failed.push(Failed {
                            path: from,
                            error: format!(
                                "the existing item could not be moved to the Recycle Bin: {e}"
                            ),
                        });
                        continue;
                    }
                }
            }
        }
        ctx.progress.current = s.name.clone();
        let outcome = match kind {
            TransferKind::Copy => copy_item(&s.path, &target, &mut ctx),
            TransferKind::Move => move_item(&s.path, &target, &mut ctx),
        };
        match outcome {
            Ok(()) => report.done.push(Moved {
                from,
                to: path_string(&target),
            }),
            Err(Error::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push(Failed {
                path: from,
                error: e.to_string(),
            }),
        }
    }
    Ok(report)
}

struct Ctx<'a> {
    progress: Progress,
    on_progress: &'a mut dyn FnMut(&Progress),
    cancel: &'a AtomicBool,
    /// Links inside copied folders that were left out (they are never followed or copied).
    skipped_links: u64,
}

/// A rename on the same volume; elsewhere a full copy, then the source is removed.
fn move_item(src: &Path, target: &Path, ctx: &mut Ctx<'_>) -> Result<()> {
    let bytes = tree_size(src).1;
    // Known limit: the caller saw `target` free, but another process could create it before this
    // rename; there is no atomic "rename unless exists" in std.
    match std::fs::rename(src, target) {
        Ok(()) => {
            ctx.progress.done_bytes += bytes;
            (ctx.on_progress)(&ctx.progress);
            return Ok(());
        }
        Err(e) if !crosses_volumes(&e) => return Err(e.into()),
        Err(_) => {}
    }
    move_by_copy(src, target, ctx)
}

/// The cross-volume move: copy, then remove the source. A folder holding links cannot be moved
/// this way (links are never copied), so that is found out before a single byte is copied.
fn move_by_copy(src: &Path, target: &Path, ctx: &mut Ctx<'_>) -> Result<()> {
    if contains_link(src) {
        return Err(Error::Io(std::io::Error::other(
            "contains links, which cannot be moved to another drive: nothing was copied and the \
             original was kept",
        )));
    }
    let links_before = ctx.skipped_links;
    copy_item(src, target, ctx)?;
    remove_moved_source(src, ctx.skipped_links > links_before)
}

/// Whether the folder `path` holds a link or junction anywhere below it (never followed). Stops
/// at the first one, or at the walk limit.
fn contains_link(path: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return false;
    }
    let (mut seen, mut stack) = (0u64, vec![path.to_path_buf()]);
    while let Some(dir) = stack.pop() {
        let Ok(items) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            seen += 1;
            if seen > PROPERTIES_LIMIT {
                return false;
            }
            let Ok(kind) = item.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                return true;
            }
            if kind.is_dir() {
                stack.push(item.path());
            }
        }
    }
    false
}

/// Removes the source of a cross-volume move once its copy is complete. Links inside it were not
/// copied, so a source that held any is kept rather than deleted with them.
fn remove_moved_source(src: &Path, had_links: bool) -> Result<()> {
    if had_links {
        return Err(Error::Io(std::io::Error::other(
            "contains links that were not moved: everything else was copied and the original \
             was kept",
        )));
    }
    let removed = if src.symlink_metadata()?.is_dir() {
        std::fs::remove_dir_all(src)
    } else {
        std::fs::remove_file(src)
    };
    removed.map_err(|e| {
        Error::Io(std::io::Error::new(
            e.kind(),
            format!("copied, but the original could not be removed: {e}"),
        ))
    })
}

/// Copies a file or a folder tree to `target` (which does not exist). On any failure or a
/// cancel, whatever was created for this item is removed again.
fn copy_item(src: &Path, target: &Path, ctx: &mut Ctx<'_>) -> Result<()> {
    let result = copy_tree(src, target, ctx);
    if result.is_err() {
        if target.is_dir() {
            let _ = std::fs::remove_dir_all(target);
        }
        let _ = std::fs::remove_file(part_path(target));
    }
    result
}

fn copy_tree(src: &Path, target: &Path, ctx: &mut Ctx<'_>) -> Result<()> {
    let meta = src.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        return Err(Error::Io(std::io::Error::other("links are not copied")));
    }
    if meta.is_dir() {
        std::fs::create_dir(target)?;
        for item in std::fs::read_dir(src)? {
            let item = item?;
            // links inside a folder are left out rather than followed
            if item.file_type()?.is_symlink() {
                ctx.skipped_links += 1;
                continue;
            }
            copy_tree(&item.path(), &target.join(item.file_name()), ctx)?;
        }
        return Ok(());
    }
    copy_file(src, target, &meta, ctx)
}

/// Copies through `<name>.riptide-part`, so an interrupted copy never leaves a file that looks
/// complete under the real name.
fn copy_file(src: &Path, target: &Path, meta: &std::fs::Metadata, ctx: &mut Ctx<'_>) -> Result<()> {
    let part = part_path(target);
    let mut input = std::fs::File::open(src)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if ctx.cancel.load(Ordering::Relaxed) {
            drop(output);
            let _ = std::fs::remove_file(&part);
            return Err(Error::Cancelled);
        }
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        ctx.progress.done_bytes += n as u64;
        (ctx.on_progress)(&ctx.progress);
    }
    output.flush()?;
    output.sync_all()?;
    if let Ok(modified) = meta.modified() {
        let _ = output.set_modified(modified);
    }
    drop(output);
    if target.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Exists(path_string(target)));
    }
    std::fs::rename(&part, target)?;
    if meta.permissions().readonly() {
        let mut p = std::fs::metadata(target)?.permissions();
        p.set_readonly(true);
        let _ = std::fs::set_permissions(target, p);
    }
    Ok(())
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(PART_SUFFIX);
    target.with_file_name(name)
}

/// Files and bytes under `path`, without following links, stopping at the walk limit.
fn tree_size(path: &Path) -> (u64, u64) {
    let Ok(meta) = path.symlink_metadata() else {
        return (0, 0);
    };
    if meta.is_file() {
        return (1, meta.len());
    }
    if !meta.is_dir() {
        return (0, 0);
    }
    let (mut files, mut bytes, mut seen) = (0u64, 0u64, 0u64);
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(items) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            seen += 1;
            if seen > PROPERTIES_LIMIT {
                return (files, bytes);
            }
            let Ok(m) = item.path().symlink_metadata() else {
                continue;
            };
            if m.is_dir() {
                stack.push(item.path());
            } else if m.is_file() {
                files += 1;
                bytes += m.len();
            }
        }
    }
    (files, bytes)
}

/// Whether a rename failed only because source and target are on different volumes.
fn crosses_volumes(e: &std::io::Error) -> bool {
    // EXDEV on Unix, ERROR_NOT_SAME_DEVICE on Windows
    e.kind() == std::io::ErrorKind::CrossesDevices
        || (cfg!(unix) && e.raw_os_error() == Some(18))
        || (cfg!(windows) && e.raw_os_error() == Some(17))
}

#[cfg(unix)]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (a.symlink_metadata(), b.metadata()) {
        (Ok(x), Ok(y)) => x.dev() == y.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::path::Component;
    let root = |p: &Path| match p.components().next() {
        Some(Component::Prefix(prefix)) => Some(prefix.as_os_str().to_ascii_lowercase()),
        _ => None,
    };
    root(a).is_some() && root(a) == root(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct T {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    fn setup() -> T {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let src = root.join("src");
        std::fs::create_dir_all(src.join("album").join("disc 2")).unwrap();
        std::fs::write(src.join("a.txt"), b"alpha").unwrap();
        std::fs::write(src.join("album").join("01.mp3"), vec![1u8; 3000]).unwrap();
        std::fs::write(
            src.join("album").join("disc 2").join("02.mp3"),
            vec![2u8; 2_500_000],
        )
        .unwrap();
        std::fs::create_dir(root.join("dest")).unwrap();
        T { _dir: dir, root }
    }

    fn run(t: &T, items: &[&str], kind: TransferKind, choice: ConflictChoice) -> TransferReport {
        let sources: Vec<PathBuf> = items.iter().map(|i| t.root.join("src").join(i)).collect();
        let mut calls = 0;
        let report = transfer(
            &sources,
            &t.root.join("dest"),
            kind,
            choice,
            &mut |_| calls += 1,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(calls > 0 || report.done.is_empty());
        report
    }

    #[test]
    fn copies_files_and_trees_byte_exact_and_keeps_the_sources() {
        let t = setup();
        let p = plan(
            &[t.root.join("src/a.txt"), t.root.join("src/album")],
            &t.root.join("dest"),
            TransferKind::Copy,
        )
        .unwrap();
        assert_eq!((p.files, p.bytes), (3, 5 + 3000 + 2_500_000));
        assert_eq!(p.needs_space, p.bytes);
        assert!(p.conflicts.is_empty());

        let r = run(
            &t,
            &["a.txt", "album"],
            TransferKind::Copy,
            ConflictChoice::KeepBoth,
        );
        assert_eq!(r.done.len(), 2);
        assert!(r.failed.is_empty());
        assert_eq!(
            std::fs::read(t.root.join("dest/album/disc 2/02.mp3")).unwrap(),
            vec![2u8; 2_500_000]
        );
        assert!(t.root.join("src/album/disc 2/02.mp3").exists());
        // no temporary file is left behind
        assert!(!t
            .root
            .join("dest/album/disc 2/02.mp3.riptide-part")
            .exists());
    }

    #[test]
    fn conflicts_are_reported_first_and_never_overwritten_silently() {
        let t = setup();
        std::fs::write(t.root.join("dest/a.txt"), b"keep me").unwrap();
        let p = plan(
            &[t.root.join("src/a.txt")],
            &t.root.join("dest"),
            TransferKind::Copy,
        )
        .unwrap();
        assert_eq!(p.conflicts.len(), 1);
        assert!(!p.conflicts[0].target_is_folder);

        let skipped = run(&t, &["a.txt"], TransferKind::Copy, ConflictChoice::Skip);
        assert_eq!(skipped.skipped.len(), 1);
        assert_eq!(
            std::fs::read(t.root.join("dest/a.txt")).unwrap(),
            b"keep me"
        );

        let both = run(&t, &["a.txt"], TransferKind::Copy, ConflictChoice::KeepBoth);
        assert!(both.done[0].to.ends_with("a (1).txt"));
        assert_eq!(
            std::fs::read(t.root.join("dest/a.txt")).unwrap(),
            b"keep me"
        );
        assert_eq!(
            std::fs::read(t.root.join("dest/a (1).txt")).unwrap(),
            b"alpha"
        );
    }

    #[test]
    fn moves_on_one_volume_by_rename_and_leaves_items_already_there() {
        let t = setup();
        let r = run(&t, &["album"], TransferKind::Move, ConflictChoice::KeepBoth);
        assert_eq!(r.done.len(), 1);
        assert!(!t.root.join("src/album").exists());
        assert!(t.root.join("dest/album/01.mp3").exists());

        let again = transfer(
            &[t.root.join("dest/album")],
            &t.root.join("dest"),
            TransferKind::Move,
            ConflictChoice::KeepBoth,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(again.skipped.len(), 1);
        let p = plan(
            &[t.root.join("dest/album")],
            &t.root.join("dest"),
            TransferKind::Move,
        )
        .unwrap();
        assert_eq!(p.unchanged.len(), 1);
        assert_eq!(p.needs_space, 0);
    }

    #[test]
    fn a_folder_cannot_go_into_itself() {
        let t = setup();
        let inner = t.root.join("src/album/disc 2");
        assert!(matches!(
            plan(&[t.root.join("src/album")], &inner, TransferKind::Copy),
            Err(Error::IntoItself)
        ));
        assert!(matches!(
            transfer(
                &[t.root.join("src/album")],
                &inner,
                TransferKind::Move,
                ConflictChoice::Skip,
                &mut |_| {},
                &AtomicBool::new(false)
            ),
            Err(Error::IntoItself)
        ));
    }

    #[test]
    fn into_itself_is_judged_on_resolved_paths_and_case() {
        let t = setup();
        let album = t.root.join("src/album");
        // a destination that does not exist yet, below the source
        let deep = album.join("disc 2").join("new").join("deeper");
        assert!(is_inside_with(&deep, &album, false));
        assert!(!is_inside_with(&t.root.join("dest"), &album, false));
        // a sibling that merely shares the prefix is not inside
        std::fs::create_dir(t.root.join("src/album2")).unwrap();
        assert!(!is_inside_with(&t.root.join("src/album2"), &album, false));
        // case differences only count when asked for
        let shouting = t.root.join("SRC").join("ALBUM").join("disc 2");
        assert!(is_inside_with(&shouting, &album, true));
        assert!(same_path_with(&album, &t.root.join("SRC/Album"), true));
        assert!(!same_path_with(&album, &t.root.join("src/other"), true));
        // (a Windows file system resolves the spelling to the real case, so only elsewhere)
        #[cfg(not(windows))]
        assert!(!same_path_with(&album, &t.root.join("SRC/Album"), false));
    }

    /// A directory link (symlink; junction on Windows). False when the platform refuses to make one.
    fn make_dir_link(link: &Path, target: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    }

    fn is_link(p: &Path) -> bool {
        p.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    #[test]
    fn replace_through_a_directory_alias_never_trashes_the_source() {
        let t = setup();
        let jump = t.root.join("jump");
        if !make_dir_link(&jump, &t.root.join("src")) {
            return; // links cannot be created here
        }
        assert!(is_link(&jump), "the link was created, so the test must run");
        assert!(same_path(&jump, &t.root.join("src")));
        assert!(same_item(&jump.join("a.txt"), &t.root.join("src/a.txt")));
        assert!(!same_item(&jump, &t.root.join("src")));
        for kind in [TransferKind::Copy, TransferKind::Move] {
            let r = transfer(
                &[t.root.join("src/a.txt")],
                &jump,
                kind,
                ConflictChoice::Replace,
                &mut |_| {},
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(r.failed.is_empty(), "{:?}", r.failed);
            assert_eq!(std::fs::read(t.root.join("src/a.txt")).unwrap(), b"alpha");
        }
        let p = plan(&[t.root.join("src/a.txt")], &jump, TransferKind::Move).unwrap();
        assert_eq!(
            p.unchanged.len(),
            1,
            "a move into its own folder is a no-op"
        );
    }

    #[test]
    fn a_folder_with_links_is_refused_before_anything_is_copied() {
        let t = setup();
        let album = t.root.join("src/album");
        assert!(!contains_link(&album));
        let link = album.join("disc 2").join("ln");
        if !make_dir_link(&link, &t.root.join("src")) {
            return; // links cannot be created here
        }
        assert!(is_link(&link), "the link was created, so the test must run");
        assert!(contains_link(&album));
        let target = t.root.join("dest/album");
        let mut ctx = Ctx {
            progress: Progress {
                done_bytes: 0,
                total_bytes: 0,
                current: String::new(),
            },
            on_progress: &mut |_| {},
            cancel: &AtomicBool::new(false),
            skipped_links: 0,
        };
        let err = move_by_copy(&album, &target, &mut ctx).unwrap_err();
        assert!(err.to_string().contains("nothing was copied"), "{err}");
        assert_eq!(ctx.progress.done_bytes, 0, "no byte was copied");
        assert!(!target.exists());
        assert!(album.join("01.mp3").exists());
    }

    #[test]
    fn existing_source_names_only_refuse_streams() {
        assert!(validate_existing_name("aux").is_ok());
        assert!(validate_existing_name("report.").is_ok());
        assert!(validate_existing_name("a\u{200F}b\u{202E}").is_ok());
        assert!(validate_existing_name("file.txt:stream").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_the_source_tree_counts_as_into_itself() {
        let t = setup();
        let link = t.root.join("dest/jump");
        std::os::unix::fs::symlink(t.root.join("src/album"), &link).unwrap();
        assert!(matches!(
            plan(&[t.root.join("src/album")], &link, TransferKind::Copy),
            Err(Error::IntoItself)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_into_the_source_tree_counts_as_into_itself() {
        let t = setup();
        let link = t.root.join("dest/jump");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(t.root.join("src/album"))
            .output()
            .unwrap();
        if !made.status.success() {
            return;
        }
        assert!(matches!(
            plan(&[t.root.join("src/album")], &link, TransferKind::Copy),
            Err(Error::IntoItself)
        ));
    }

    #[test]
    fn sources_with_stream_or_unsafe_names_are_refused() {
        let t = setup();
        for name in ["a.txt:stream", "x.txt:Zone.Identifier"] {
            let r = plan(
                &[t.root.join("src").join(name)],
                &t.root.join("dest"),
                TransferKind::Copy,
            );
            assert!(matches!(r, Err(Error::BadName(_))), "{name:?}");
        }
    }

    #[test]
    fn a_move_never_deletes_a_source_whose_links_were_not_copied() {
        let t = setup();
        let src = t.root.join("src/album");
        assert!(remove_moved_source(&src, true).is_err());
        assert!(src.join("01.mp3").exists());
        remove_moved_source(&src, false).unwrap();
        assert!(!src.exists());
    }

    #[cfg(unix)]
    #[test]
    fn copying_a_folder_counts_the_links_it_leaves_out() {
        let t = setup();
        std::os::unix::fs::symlink("a.txt", t.root.join("src/album/ln")).unwrap();
        let mut ctx = Ctx {
            progress: Progress {
                done_bytes: 0,
                total_bytes: 0,
                current: String::new(),
            },
            on_progress: &mut |_| {},
            cancel: &AtomicBool::new(false),
            skipped_links: 0,
        };
        copy_item(
            &t.root.join("src/album"),
            &t.root.join("dest/album"),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(ctx.skipped_links, 1);
    }

    #[test]
    fn a_cancelled_copy_leaves_no_partial_item() {
        let t = setup();
        let stop = AtomicBool::new(false);
        let mut chunks = 0;
        let r = transfer(
            &[t.root.join("src/album")],
            &t.root.join("dest"),
            TransferKind::Copy,
            ConflictChoice::KeepBoth,
            &mut |_| {
                chunks += 1;
                if chunks == 2 {
                    stop.store(true, Ordering::Relaxed);
                }
            },
            &stop,
        )
        .unwrap();
        assert!(r.cancelled);
        assert!(r.done.is_empty());
        assert!(
            !t.root.join("dest/album").exists(),
            "the half-copied folder is removed"
        );
        assert!(t.root.join("src/album/disc 2/02.mp3").exists());
    }

    #[test]
    fn copying_onto_itself_makes_a_copy_and_missing_sources_are_refused() {
        let t = setup();
        let r = transfer(
            &[t.root.join("src/a.txt")],
            &t.root.join("src"),
            TransferKind::Copy,
            ConflictChoice::Replace,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(r.done[0].to.ends_with("a (1).txt"));
        assert_eq!(std::fs::read(t.root.join("src/a.txt")).unwrap(), b"alpha");
        assert!(matches!(
            plan(
                &[t.root.join("src/nope")],
                &t.root.join("dest"),
                TransferKind::Copy
            ),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            plan(
                &[t.root.join("src/a.txt")],
                &t.root.join("not-a-folder"),
                TransferKind::Copy
            ),
            Err(Error::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_fails_alone_and_the_rest_goes_on() {
        use std::os::unix::fs::PermissionsExt;
        let t = setup();
        let locked = t.root.join("src/locked.bin");
        std::fs::write(&locked, b"secret").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        // root reads anything: the case cannot be produced there
        if std::fs::read(&locked).is_ok() {
            return;
        }
        let r = run(
            &t,
            &["locked.bin", "a.txt"],
            TransferKind::Copy,
            ConflictChoice::KeepBoth,
        );
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.done.len(), 1);
        assert!(!t.root.join("dest/locked.bin").exists());
        assert!(!t.root.join("dest/locked.bin.riptide-part").exists());
    }

    #[test]
    fn crossing_volumes_is_recognised_from_the_os_error() {
        assert!(crosses_volumes(&std::io::Error::from(
            std::io::ErrorKind::CrossesDevices
        )));
        assert!(!crosses_volumes(&std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )));
    }
}
