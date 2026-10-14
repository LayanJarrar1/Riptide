//! Where Riptide keeps its files.
//!
//! Installed mode: `%LOCALAPPDATA%\Riptide` on Windows, `$XDG_DATA_HOME/riptide` (or
//! `~/.local/share/riptide`) elsewhere. Portable mode: when a file named `portable.txt` sits next
//! to the executable, everything goes to `data/` next to the executable instead, so the portable
//! zip leaves nothing behind.

use std::path::{Path, PathBuf};

use crate::Error;

/// Name of the marker file that switches on portable mode.
pub const PORTABLE_MARKER: &str = "portable.txt";

/// Resolved data locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Root of all Riptide data (database, logs, settings).
    pub data_dir: PathBuf,
    /// Daily log files.
    pub logs_dir: PathBuf,
    /// The SQLite database.
    pub db_path: PathBuf,
    /// `true` when `portable.txt` was found next to the executable.
    pub portable: bool,
}

impl Paths {
    /// Resolves the paths for the running executable. Does not create anything.
    pub fn resolve() -> Result<Paths, Error> {
        let exe = std::env::current_exe().map_err(|e| Error::Resolve(e.to_string()))?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| Error::Resolve(format!("{} has no parent directory", exe.display())))?;
        let portable = exe_dir.join(PORTABLE_MARKER).is_file();
        Self::compute(exe_dir, portable, dirs::data_local_dir().as_deref())
    }

    /// Pure resolution rule, separated from [`Self::resolve`] so it can be tested without touching
    /// the real executable location.
    ///
    /// * `portable == true` → `<exe_dir>/data`;
    /// * otherwise `<local_data>/Riptide` (`riptide` on non-Windows);
    /// * `local_data == None` and not portable → [`Error::Resolve`].
    pub fn compute(
        exe_dir: &Path,
        portable: bool,
        local_data: Option<&Path>,
    ) -> Result<Paths, Error> {
        let data_dir = if portable {
            exe_dir.join("data")
        } else {
            let base = local_data.ok_or_else(|| {
                Error::Resolve("no per-user data directory (LOCALAPPDATA / XDG_DATA_HOME)".into())
            })?;
            base.join(app_dir_name())
        };
        Ok(Self::in_dir(data_dir, portable))
    }

    /// Paths rooted at an explicit directory (tests, `--data-dir` overrides).
    pub fn in_dir(data_dir: impl Into<PathBuf>, portable: bool) -> Paths {
        let data_dir = data_dir.into();
        Paths {
            logs_dir: data_dir.join("logs"),
            db_path: data_dir.join("riptide.sqlite"),
            data_dir,
            portable,
        }
    }

    /// Creates `data_dir` and `logs_dir` if missing.
    pub fn ensure_dirs(&self) -> Result<(), Error> {
        for dir in [&self.data_dir, &self.logs_dir] {
            std::fs::create_dir_all(dir).map_err(|source| Error::Directory {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

/// `Riptide` on Windows (matches the install folder), lower-case elsewhere (XDG convention).
fn app_dir_name() -> &'static str {
    if cfg!(windows) {
        "Riptide"
    } else {
        "riptide"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_mode_uses_data_next_to_exe() {
        let p = Paths::compute(
            Path::new("/opt/riptide"),
            true,
            Some(Path::new("/home/u/.local/share")),
        )
        .unwrap();
        assert_eq!(p.data_dir, Path::new("/opt/riptide/data"));
        assert_eq!(p.logs_dir, Path::new("/opt/riptide/data/logs"));
        assert_eq!(p.db_path, Path::new("/opt/riptide/data/riptide.sqlite"));
        assert!(p.portable);
    }

    #[test]
    fn installed_mode_uses_local_data_dir() {
        let p = Paths::compute(
            Path::new("/opt/riptide"),
            false,
            Some(Path::new("/home/u/.local/share")),
        )
        .unwrap();
        assert_eq!(
            p.data_dir,
            Path::new("/home/u/.local/share").join(app_dir_name())
        );
        assert!(!p.portable);
        assert!(p.logs_dir.ends_with("logs"));
    }

    #[test]
    fn installed_mode_without_local_data_dir_is_an_error() {
        let err = Paths::compute(Path::new("/opt/riptide"), false, None).unwrap_err();
        assert!(matches!(err, Error::Resolve(_)));
    }

    #[test]
    fn resolve_uses_marker_next_to_exe() {
        // Does not create anything; only checks that the rule is consistent with compute().
        let p = Paths::resolve().unwrap();
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let marker = exe_dir.join(PORTABLE_MARKER).is_file();
        assert_eq!(p.portable, marker);
        if marker {
            assert_eq!(p.data_dir, exe_dir.join("data"));
        } else {
            assert!(p.data_dir.ends_with(app_dir_name()));
        }
    }

    #[test]
    fn ensure_dirs_creates_both() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::in_dir(tmp.path().join("root"), true);
        p.ensure_dirs().unwrap();
        assert!(p.data_dir.is_dir());
        assert!(p.logs_dir.is_dir());
    }
}
