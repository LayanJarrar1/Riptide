//! Free disk space for the folder a download is going to.
//!
//! [`free_space`] answers for the nearest existing ancestor of the path (the download folder
//! may not exist yet), so the add dialog and the status bar can warn before a transfer starts.
//! Only the Win32 call is platform code; everything else is portable.

use std::io;
use std::path::{Path, PathBuf};

/// Bytes available to the current user on the volume that holds `path` (or would hold it:
/// the first existing ancestor is queried when `path` does not exist yet).
///
/// Errors: `NotFound` when no ancestor exists (a drive letter that is not mounted),
/// `Unsupported` on platforms without an implementation, or the OS error.
pub fn free_space(path: &Path) -> io::Result<u64> {
    let existing = existing_ancestor(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no part of {} exists", path.display()),
        )
    })?;
    sys::free_space(&existing)
}

/// `path` itself if it exists, else its closest existing ancestor; `None` when nothing on the
/// chain exists (a relative path is resolved against the current directory first).
pub fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    absolute
        .ancestors()
        .find(|p| p.exists())
        .map(Path::to_path_buf)
}

/// Windows: `GetDiskFreeSpaceExW` on the directory. The only `unsafe` in this crate.
#[cfg(windows)]
mod sys {
    #![allow(unsafe_code)]

    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub(super) fn free_space(dir: &Path) -> io::Result<u64> {
        let wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free_to_caller: u64 = 0;
        let mut total: u64 = 0;
        let mut total_free: u64 = 0;
        // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call, and the
        // three out-pointers refer to live `u64`s on this stack frame; the function writes
        // through them only on success and reads nothing else.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_to_caller,
                &mut total,
                &mut total_free,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(free_to_caller)
    }
}

/// Other platforms: not implemented yet (`libc::statvfs` once the crate depends on `libc`).
#[cfg(not(windows))]
mod sys {
    use std::io;
    use std::path::Path;

    pub(super) fn free_space(_dir: &Path) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "free disk space is only implemented on Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_ancestor_walks_up_to_something_that_exists() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("not").join("yet").join("created");
        assert_eq!(existing_ancestor(&deep).as_deref(), Some(dir.path()));
        assert_eq!(existing_ancestor(dir.path()).as_deref(), Some(dir.path()));
        assert!(existing_ancestor(Path::new("relative/x")).is_some());
    }

    #[cfg(windows)]
    #[test]
    fn free_space_answers_for_a_folder_that_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let free = free_space(&dir.path().join("downloads").join("sub")).unwrap();
        assert!(free > 0, "a temp dir has some space");
        assert_eq!(
            free_space(dir.path()).unwrap() / (1 << 30),
            free / (1 << 30),
            "same volume, same answer (to the GiB)"
        );
        // a drive letter that is not mounted has no existing ancestor at all
        if let Some(root) = ('D'..='Z')
            .rev()
            .map(|l| format!("{l}:\\"))
            .find(|r| !Path::new(r).exists())
        {
            let missing = free_space(&Path::new(&root).join("x"));
            assert_eq!(missing.unwrap_err().kind(), io::ErrorKind::NotFound);
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn free_space_is_unsupported_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            free_space(dir.path()).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
