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
use crate::registry::Registry;

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
    slots: Mutex<Vec<Slot>>,
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

impl TorrentStorage for SafeStorage {
    fn init(
        &mut self,
        _shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        let mut slots = Vec::with_capacity(self.paths.len());
        for p in &self.paths {
            match p {
                None => slots.push(Slot::Padding),
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    // after creating folders, make sure no pre-existing link leads out
                    ensure_no_symlink_escape(&self.root, path)?;
                    let file = OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .read(true)
                        .write(true)
                        .open(path)
                        .with_context(|| format!("cannot open {}", path.display()))?;
                    ensure_no_symlink_escape(&self.root, path)?;
                    slots.push(Slot::Open {
                        file,
                        path: path.clone(),
                    });
                }
            }
        }
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
                let planned = plan_layout(&name, &raw, &move |rel| !adopt && r.join(rel).exists())
                    .map_err(|e| anyhow!("{e}"))?;
                self.registry.set_layout(&hash, planned.clone());
                planned
            }
        };

        let mut paths = Vec::with_capacity(layout.len());
        for (rel, fi) in layout.iter().zip(metadata.file_infos.iter()) {
            if fi.attrs.padding {
                paths.push(None);
            } else {
                let abs = confine(&root, rel).map_err(|e| anyhow!("{e}"))?;
                paths.push(Some(abs));
            }
        }
        if paths.len() != metadata.file_infos.len() {
            bail!("layout does not match the torrent");
        }
        std::fs::create_dir_all(&root)?;
        Ok(SafeStorage {
            root,
            paths,
            slots: Default::default(),
        })
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

/// Deletes the files of a planned layout below `root` (and folders left empty), without going
/// through the library. Paths that fail the confinement checks are skipped.
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
