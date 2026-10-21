//! The disk back end handed to librqbit: it never uses a path the torrent chose.
//!
//! librqbit's stock file storage joins the paths of the `.torrent` onto the output folder. We
//! replace it with [`SafeStorageFactory`]: the path of every file comes from the layout planned
//! by [`crate::paths::plan_layout`] and stored in the [`Registry`], is confined to the chosen
//! folder ([`confine`], symlink/junction check), and the file names of the torrent are never
//! looked at again.

use std::fs::{File, OpenOptions};
use std::io::IoSlice;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context};
use librqbit::storage::{StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

use crate::paths::{confine, ensure_no_symlink_escape, plan_layout, RawFile};
use crate::registry::{Entry, Registry};

/// Marks a file as sparse (NTFS), so bytes written far into a file that is never completed do not
/// reserve the whole length on disk.
///
/// A torrent's pieces do not respect file boundaries: to verify the piece that holds a file you
/// asked for, the bytes of its neighbours in that piece must be written too. Without the sparse
/// flag, writing a few kilobytes at the end of an unselected 263 MB file allocates all 263 MB
/// (found while testing with a real torrent). librqbit's own storage does the same marking.
mod sparse {
    #[cfg(windows)]
    #[allow(unsafe_code)]
    pub fn mark(file: &std::fs::File) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;

        use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
        use windows_sys::Win32::System::IO::DeviceIoControl;

        let mut returned: u32 = 0;
        // SAFETY: the handle is valid for the lifetime of `file`; FSCTL_SET_SPARSE takes no input
        // buffer (the default, "set sparse", applies) and no output buffer, so the null pointers
        // with zero sizes are what the API documents; `returned` outlives the call.
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle() as _,
                FSCTL_SET_SPARSE,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Other platforms: files are sparse by default.
    #[cfg(not(windows))]
    pub fn mark(_file: &std::fs::File) -> std::io::Result<()> {
        Ok(())
    }
}

/// Positional file I/O (the only platform-specific code of the crate).
mod posio {
    use std::fs::File;
    use std::io;

    #[cfg(unix)]
    pub fn write_all_at(f: &File, buf: &[u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::write_all_at(f, buf, offset)
    }

    #[cfg(unix)]
    pub fn read_exact_at(f: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(f, buf, offset)
    }

    #[cfg(windows)]
    pub fn write_all_at(f: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let n = f.seek_write(buf, offset)?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            buf = &buf[n..];
            offset += n as u64;
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn read_exact_at(f: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let n = f.seek_read(buf, offset)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            offset += n as u64;
            buf = &mut buf[n..];
        }
        Ok(())
    }
}

enum Slot {
    /// BEP 47 padding: reads are zeros, writes are dropped.
    Padding,
    Open {
        file: File,
        path: PathBuf,
    },
}

/// Per-torrent storage; see the module documentation.
pub struct SafeStorage {
    root: PathBuf,
    /// Absolute path of every file, confined to `root`.
    paths: Vec<Option<PathBuf>>,
    /// The same files, relative to `root` (what the registry records).
    rels: Vec<Option<PathBuf>>,
    registry: Arc<Registry>,
    hash: String,
    slots: Mutex<Vec<Slot>>,
}

/// Creates `dir` (below `root`) one folder at a time and checks, before each step, that the part
/// that already exists does not lead out of `root` through a link or junction. A torrent that
/// aims at `pack/sub/deep/a.bin` with `pack` being a junction therefore creates nothing outside.
fn create_dirs_confined(root: &Path, dir: &Path) -> anyhow::Result<()> {
    let rel = dir
        .strip_prefix(root)
        .context("folder is outside the download folder")?;
    let mut cur = root.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        ensure_no_symlink_escape(root, &cur)?;
        if !cur.exists() {
            std::fs::create_dir(&cur)?;
            ensure_no_symlink_escape(root, &cur)?;
        }
    }
    Ok(())
}

impl SafeStorage {
    fn with_slot<R>(
        &self,
        file_id: usize,
        f: impl FnOnce(&Slot) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        let g = self.slots.lock().map_err(|_| anyhow!("storage poisoned"))?;
        f(g.get(file_id)
            .context("storage is closed or no such file")?)
    }
}

impl SafeStorage {
    /// Opens (creating what is missing) every file; `created` collects the ones this call
    /// brought into being, also when it fails partway.
    fn open_all(&self, slots: &mut Vec<Slot>, created: &mut Vec<PathBuf>) -> anyhow::Result<()> {
        for (p, rel) in self.paths.iter().zip(&self.rels) {
            match p {
                None => slots.push(Slot::Padding),
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        create_dirs_confined(&self.root, parent)?;
                    }
                    ensure_no_symlink_escape(&self.root, path)?;
                    // `create_new` first: only a file this call brings into being is recorded as
                    // created by the torrent (and so may be deleted with it)
                    let mut opts = OpenOptions::new();
                    opts.read(true).write(true);
                    let file = match opts.clone().create_new(true).open(path) {
                        Ok(f) => {
                            if let Some(rel) = rel {
                                created.push(rel.clone());
                            }
                            f
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => opts
                            .open(path)
                            .with_context(|| format!("cannot open {}", path.display()))?,
                        Err(e) => {
                            return Err(e)
                                .with_context(|| format!("cannot open {}", path.display()))
                        }
                    };
                    ensure_no_symlink_escape(&self.root, path)?;
                    // best effort: a file system without sparse files (FAT, some shares) is fine
                    let _ = sparse::mark(&file);
                    slots.push(Slot::Open {
                        file,
                        path: path.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Records files this torrent created (flushed at once, before any byte is written, so a
    /// crash cannot lose the record).
    fn record_created(&self, created: Vec<PathBuf>) -> anyhow::Result<()> {
        if created.is_empty() {
            return Ok(());
        }
        self.registry.update(&self.hash, |e| {
            let list = e.created.get_or_insert_with(Vec::new);
            for c in created {
                if !list.contains(&c) {
                    list.push(c);
                }
            }
        });
        self.registry.flush().map_err(|e| anyhow!("{e}"))
    }
}

impl TorrentStorage for SafeStorage {
    fn init(
        &mut self,
        _shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        let mut slots = Vec::with_capacity(self.paths.len());
        let mut created: Vec<PathBuf> = Vec::new();
        let opened = self.open_all(&mut slots, &mut created);
        // what was created before a failure is recorded too, so removing the torrent (or the
        // rollback of a failed add) deletes exactly that
        let recorded = self.record_created(created);
        opened?;
        recorded?;
        *self.slots.lock().map_err(|_| anyhow!("storage poisoned"))? = slots;
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.with_slot(file_id, |s| match s {
            Slot::Padding => {
                buf.fill(0);
                Ok(())
            }
            Slot::Open { file, .. } => Ok(posio::read_exact_at(file, buf, offset)?),
        })
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        self.with_slot(file_id, |s| match s {
            Slot::Padding => Ok(()),
            Slot::Open { file, .. } => Ok(posio::write_all_at(file, buf, offset)?),
        })
    }

    fn pwrite_all_vectored(
        &self,
        file_id: usize,
        offset: u64,
        bufs: [IoSlice<'_>; 2],
    ) -> anyhow::Result<usize> {
        let mut offset = offset;
        let mut size = 0;
        for b in bufs {
            self.pwrite_all(file_id, offset, &b)?;
            offset += b.len() as u64;
            size += b.len();
        }
        Ok(size)
    }

    fn remove_file(&self, file_id: usize, _filename: &Path) -> anyhow::Result<()> {
        // the name librqbit passes comes from the torrent; we use our own planned path instead
        let path = self
            .paths
            .get(file_id)
            .and_then(|p| p.as_ref())
            .context("no such file")?;
        // only files this torrent created (or was told are its own data) may be deleted
        let allowed = match (self.rels.get(file_id), self.registry.get(&self.hash)) {
            (Some(Some(rel)), Some(entry)) => entry.deletable().contains(rel),
            _ => false,
        };
        if !allowed {
            return Ok(());
        }
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        // tidy up emptied folders, never the chosen folder itself
        let mut dir = path.parent();
        while let Some(d) = dir {
            if d == self.root || !d.starts_with(&self.root) {
                break;
            }
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        Ok(())
    }

    fn remove_directory_if_empty(&self, _path: &Path) -> anyhow::Result<()> {
        // folders are removed by `remove_file` using our own layout
        Ok(())
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        self.with_slot(file_id, |s| match s {
            Slot::Padding => Ok(()),
            Slot::Open { file, .. } => Ok(file.set_len(length)?),
        })
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let taken =
            std::mem::take(&mut *self.slots.lock().map_err(|_| anyhow!("storage poisoned"))?);
        Ok(Box::new(SafeStorage {
            root: self.root.clone(),
            paths: self.paths.clone(),
            rels: self.rels.clone(),
            registry: self.registry.clone(),
            hash: self.hash.clone(),
            slots: Mutex::new(taken),
        }))
    }
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slot::Padding => f.write_str("Padding"),
            Slot::Open { path, .. } => write!(f, "Open({})", path.display()),
        }
    }
}

/// Factory shared by every torrent of a session; looks the folder and the layout of a torrent
/// up in the [`Registry`] by info-hash.
#[derive(Clone)]
pub struct SafeStorageFactory {
    registry: Arc<Registry>,
}

impl SafeStorageFactory {
    /// Creates the factory.
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }

    /// Boxed for `SessionOptions::default_storage_factory`.
    pub fn boxed(self) -> librqbit::storage::BoxStorageFactory {
        StorageFactoryExt::boxed(self)
    }
}

impl StorageFactory for SafeStorageFactory {
    type Storage = SafeStorage;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<SafeStorage> {
        let hash = shared.info_hash.as_string();
        let entry = self
            .registry
            .get(&hash)
            .ok_or_else(|| anyhow!("torrent {hash} has no registered download folder"))?;
        let root = entry.dir.clone();

        // the layout is planned once and then kept, so resuming finds the same files
        let layout: Vec<PathBuf> = match entry.files {
            Some(files) if files.len() == metadata.file_infos.len() => files,
            _ => {
                let name = metadata
                    .info
                    .name()
                    .map(|n| n.into_owned())
                    .unwrap_or_default();
                let raw: Vec<RawFile> = metadata
                    .info
                    .iter_file_details()
                    .zip(metadata.file_infos.iter())
                    .map(|(fd, fi)| RawFile {
                        components: fd
                            .filename
                            .iter_components()
                            .map(|c| c.into_owned())
                            .collect(),
                        len: fi.len,
                        padding: fi.attrs.padding,
                    })
                    .collect();
                let adopt = entry.adopt;
                let r = root.clone();
                let claims = self.registry.claim_set(&r, Some(&hash));
                let planned = plan_layout(&name, &raw, &move |rel| {
                    claims.claims(rel) || (!adopt && name_taken(&r, rel))
                })
                .map_err(|e| anyhow!("{e}"))?;
                self.registry.set_layout(&hash, planned.clone());
                planned
            }
        };

        let mut paths = Vec::with_capacity(layout.len());
        let mut rels = Vec::with_capacity(layout.len());
        for (rel, fi) in layout.iter().zip(metadata.file_infos.iter()) {
            if fi.attrs.padding {
                paths.push(None);
                rels.push(None);
            } else {
                let abs = confine(&root, rel).map_err(|e| anyhow!("{e}"))?;
                paths.push(Some(abs));
                rels.push(Some(rel.clone()));
            }
        }
        if paths.len() != metadata.file_infos.len() {
            bail!("layout does not match the torrent");
        }
        std::fs::create_dir_all(&root)?;
        Ok(SafeStorage {
            root,
            paths,
            rels,
            registry: self.registry.clone(),
            hash,
            slots: Default::default(),
        })
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

/// The files of `entry` that are only placeholders: not selected, created by this torrent, and
/// holding at most the bytes of the boundary pieces they share with their neighbours (two pieces)
/// and not complete, as of the last time progress was recorded. A file with more verified data than
/// that, or a complete one (the user narrowed the selection after downloading it), is never listed; neither is a file the torrent
/// did not create, or anything when the progress is not known.
pub fn placeholders(entry: &Entry) -> Vec<PathBuf> {
    let (Some(files), Some(created), Some(progress), Some(piece), Some(lens)) = (
        entry.files.as_ref(),
        entry.created.as_ref(),
        entry.file_progress.as_ref(),
        entry.piece_length,
        entry.file_lens.as_ref(),
    ) else {
        return Vec::new();
    };
    let Some(selected) = entry.selected.as_ref() else {
        return Vec::new(); // everything is wanted
    };
    if progress.len() != files.len() || lens.len() != files.len() {
        return Vec::new();
    }
    files
        .iter()
        .enumerate()
        .filter(|(i, rel)| {
            !selected.contains(i)
                && created.contains(rel)
                && progress[*i] < lens[*i] // a complete file is real data
                && progress[*i] <= 2 * u64::from(piece)
        })
        .map(|(_, rel)| rel.clone())
        .collect()
}

/// Deletes [`placeholders`] (and folders left empty); returns the ones that are gone. Only call
/// it while no library session holds the files open.
pub fn remove_placeholders(entry: &Entry) -> Vec<PathBuf> {
    let mut gone = Vec::new();
    for rel in placeholders(entry) {
        let Ok(abs) = confine(&entry.dir, &rel) else {
            continue;
        };
        if ensure_no_symlink_escape(&entry.dir, &abs).is_err() {
            continue;
        }
        match std::fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => continue,
        }
        let mut dir = abs.parent();
        while let Some(d) = dir {
            if d == entry.dir || !d.starts_with(&entry.dir) || std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        gone.push(rel);
    }
    gone
}

/// Whether `rel` below `dir` is taken on disk: the file itself, or a partial download that would
/// be renamed to it.
pub fn name_taken(dir: &Path, rel: &Path) -> bool {
    let p = dir.join(rel);
    if p.exists() {
        return true;
    }
    let mut part = p.into_os_string();
    part.push(".rtpart");
    PathBuf::from(part).exists()
}

/// Deletes the given files (relative to `root`) and folders left empty, without going through
/// the library. Pass [`crate::registry::Entry::deletable`]: files the torrent did not create are
/// never in it. Paths that fail the confinement checks are skipped.
pub fn remove_laid_out_files(root: &Path, layout: &[PathBuf]) {
    for rel in layout {
        let Ok(abs) = confine(root, rel) else {
            continue;
        };
        if ensure_no_symlink_escape(root, &abs).is_err() {
            continue;
        }
        let _ = std::fs::remove_file(&abs);
        let mut dir = abs.parent();
        while let Some(d) = dir {
            if d == root || !d.starts_with(root) || std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
}
