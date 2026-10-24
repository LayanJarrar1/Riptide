//! The `.rtpart` file: preallocated to the final size, written in place at segment offsets,
//! renamed atomically when complete.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Extension of a partial file.
pub const PART_EXTENSION: &str = "rtpart";

/// `<final>.rtpart` next to the final path.
pub fn part_path_for(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(PART_EXTENSION);
    final_path.with_file_name(name)
}

/// An open partial file with positioned writes. Cheap to share: every method takes `&self`,
/// and positioned writes do not touch the file cursor, so several segments may write at once
/// through one handle.
#[derive(Debug)]
pub struct PartFile {
    file: File,
    path: PathBuf,
}

impl PartFile {
    /// Opens or creates `path`. With a known `size` the file is extended (or truncated) to it so
    /// the space is reserved up front and every segment can write at its true offset; with an
    /// unknown size the file grows as the stream arrives.
    pub fn open(path: &Path, size: Option<u64>) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if let Some(size) = size {
            if file.metadata()?.len() != size {
                // On NTFS a plain set_len only moves the end of file; the first write at a high
                // offset then zero-fills everything before it synchronously, which turns an
                // 8-connection download into eight serialised multi-hundred-MB writes. A sparse
                // file skips that fill; every range is written by the time the file is done.
                sparse::mark_sparse(&file);
                file.set_len(size)?;
            }
        }
        Ok(PartFile {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes all of `data` at `offset`.
    pub fn write_at(&self, mut offset: u64, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = positioned_write(&self.file, data, offset)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "write returned 0 bytes",
                ));
            }
            offset += n as u64;
            data = &data[n..];
        }
        Ok(())
    }

    /// Reads up to `buf.len()` bytes at `offset` (verification reads the file back).
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        positioned_read(&self.file, buf, offset)
    }

    /// Current length on disk.
    pub fn len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// `true` for an empty file.
    pub fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Cuts the file to `len` bytes (restart from zero keeps the allocation by re-extending).
    pub fn truncate(&self, len: u64) -> io::Result<()> {
        self.file.set_len(len)
    }

    /// Flushes data to the device (called before every checkpoint).
    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_data()
    }

    /// Syncs and renames the part file to `final_path`. Refuses to overwrite an existing file
    /// unless `overwrite` is set.
    pub fn finish(self, final_path: &Path, overwrite: bool) -> io::Result<()> {
        self.file.sync_all()?;
        drop(self.file);
        if final_path.exists() && !overwrite {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                final_path.display().to_string(),
            ));
        }
        if overwrite && final_path.exists() {
            std::fs::remove_file(final_path)?;
        }
        rename_with_retry(
            &self.path,
            final_path,
            RENAME_ATTEMPTS,
            RENAME_DELAY,
            |from, to| std::fs::rename(from, to),
        )
    }
}

/// Tries of the final rename and the pause between them (about two seconds in all).
const RENAME_ATTEMPTS: u32 = 10;
const RENAME_DELAY: Duration = Duration::from_millis(200);

/// Windows: an antivirus scanner or indexer holding the fresh file makes the rename fail with
/// access denied (5) or a sharing / lock violation (32, 33) for a moment. Nothing else is
/// transient, and elsewhere nothing is.
fn is_transient_rename_error(e: &io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

/// Runs `rename(from, to)`, retrying up to `attempts` times, `delay` apart, while the error is
/// [`is_transient_rename_error`]. Any other error (such as an existing target) is returned at once.
fn rename_with_retry(
    from: &Path,
    to: &Path,
    attempts: u32,
    delay: Duration,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match rename(from, to) {
            Err(e) if attempt < attempts && is_transient_rename_error(&e) => {
                tracing::debug!(attempt, error = %e, "final rename blocked, retrying");
                std::thread::sleep(delay);
                attempt += 1;
            }
            other => return other,
        }
    }
}

#[cfg(windows)]
fn positioned_write(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(data, offset)
}

#[cfg(unix)]
fn positioned_write(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(data, offset)
}

#[cfg(windows)]
fn positioned_read(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buf, offset)
}

#[cfg(unix)]
fn positioned_read(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buf, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preallocates_writes_out_of_order_and_renames() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("file.bin");
        let part = part_path_for(&final_path);
        assert_eq!(part.file_name().unwrap(), "file.bin.rtpart");

        let f = PartFile::open(&part, Some(10)).unwrap();
        assert_eq!(f.len().unwrap(), 10);
        f.write_at(6, b"6789").unwrap();
        f.write_at(0, b"012345").unwrap();
        let mut buf = [0u8; 10];
        assert_eq!(f.read_at(0, &mut buf).unwrap(), 10);
        assert_eq!(&buf, b"0123456789");
        f.sync().unwrap();
        f.finish(&final_path, false).unwrap();
        assert_eq!(std::fs::read(&final_path).unwrap(), b"0123456789");
        assert!(!part.exists());
    }

    #[test]
    fn reopen_keeps_bytes_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("x");
        let part = part_path_for(&final_path);
        PartFile::open(&part, Some(4))
            .unwrap()
            .write_at(0, b"abcd")
            .unwrap();
        let again = PartFile::open(&part, Some(4)).unwrap();
        let mut buf = [0u8; 4];
        again.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf, b"abcd");
        std::fs::write(&final_path, b"old").unwrap();
        let err = again.finish(&final_path, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let f = PartFile::open(&part, Some(4)).unwrap();
        f.finish(&final_path, true).unwrap();
        assert_eq!(std::fs::read(&final_path).unwrap(), b"abcd");
    }

    #[cfg(windows)]
    #[test]
    fn rename_retries_a_sharing_violation_then_succeeds() {
        for code in [5, 32, 33] {
            let mut calls = 0;
            let r = rename_with_retry(
                Path::new("a"),
                Path::new("b"),
                10,
                Duration::from_millis(1),
                |_, _| {
                    calls += 1;
                    if calls < 4 {
                        Err(io::Error::from_raw_os_error(code))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(r.is_ok(), "code {code}");
            assert_eq!(calls, 4);
        }
    }

    #[cfg(windows)]
    #[test]
    fn rename_gives_up_after_the_attempts() {
        let mut calls = 0;
        let r = rename_with_retry(
            Path::new("a"),
            Path::new("b"),
            5,
            Duration::from_millis(1),
            |_, _| {
                calls += 1;
                Err(io::Error::from_raw_os_error(32))
            },
        );
        assert!(r.is_err());
        assert_eq!(calls, 5);
    }

    #[test]
    fn rename_does_not_retry_other_errors() {
        let mut calls = 0;
        let r = rename_with_retry(
            Path::new("a"),
            Path::new("b"),
            10,
            Duration::from_millis(1),
            |_, _| {
                calls += 1;
                Err(io::Error::new(io::ErrorKind::AlreadyExists, "exists"))
            },
        );
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(calls, 1);
    }

    #[test]
    fn unknown_size_grows_and_truncate_resets() {
        let dir = tempfile::tempdir().unwrap();
        let f = PartFile::open(&dir.path().join("s.rtpart"), None).unwrap();
        assert!(f.is_empty().unwrap());
        f.write_at(0, &[1u8; 1000]).unwrap();
        f.write_at(1000, &[2u8; 24]).unwrap();
        assert_eq!(f.len().unwrap(), 1024);
        f.truncate(0).unwrap();
        assert!(f.is_empty().unwrap());
    }
}

/// Windows: mark the part file sparse so preallocation does not trigger NTFS zero-filling on
/// the first out-of-order write (see `PartFile::open`). Elsewhere a no-op: ext4/APFS/XFS extend
/// files without filling.
#[cfg(windows)]
mod sparse {
    #![allow(unsafe_code)]

    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    /// Best effort: a failure (FAT32, exFAT, network shares) only costs the zero-fill time.
    pub fn mark_sparse(file: &std::fs::File) {
        let mut returned: u32 = 0;
        // SAFETY: the handle comes from an open `File` and stays valid for the call; the
        // control code takes no input or output buffer, and `returned` is a valid out-pointer.
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
            tracing::debug!(
                error = %std::io::Error::last_os_error(),
                "could not mark the part file sparse; writes at high offsets will zero-fill"
            );
        }
    }
}

#[cfg(not(windows))]
mod sparse {
    pub fn mark_sparse(_file: &std::fs::File) {}
}
