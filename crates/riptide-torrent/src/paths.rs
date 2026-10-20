//! Turning the file list of a torrent into safe relative paths.
//!
//! The names inside a torrent come from whoever made it. [`plan_layout`] gives every file a
//! relative path that is safe on Windows, macOS and Linux, using the same rules as
//! [`riptide_core::filename::sanitize_filename`] on every path component, plus:
//!
//! * `.` / `..` / empty components are dropped, so `../../x` can only ever mean `x`;
//! * bidirectional override characters (U+202A..U+202E, U+2066..U+2069) are removed, so a name
//!   cannot display backwards to fake an extension;
//! * names that collide after sanitising (case-insensitive, NFC) get a ` (n)` suffix instead of
//!   silently overwriting each other, and a file never shares a path with a directory;
//! * depth and total length are bounded ([`crate::limits`]).
//!
//! [`confine`] is the last line of defence: it refuses any relative path that is not made of plain
//! components, so even a bug upstream cannot turn into a write outside the chosen folder.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use riptide_core::filename::{sanitize_filename, split_extension};
use unicode_normalization::UnicodeNormalization;

use crate::error::{Error, Result};
use crate::limits::{MAX_PATH_DEPTH, MAX_RELATIVE_PATH_BYTES};

/// One file of a torrent as the metainfo describes it (names not yet trusted).
#[derive(Debug, Clone)]
pub struct RawFile {
    /// Path components exactly as decoded from the torrent.
    pub components: Vec<String>,
    /// Length in bytes.
    pub len: u64,
    /// BEP 47 padding file: never created on disk.
    pub padding: bool,
}

fn is_bidi_override(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Sanitises one path component: bidi overrides removed, then
/// [`sanitize_filename`] (control characters, reserved characters and device names, trailing
/// dots and spaces, NFC, 200 bytes).
pub fn sanitize_component(raw: &str) -> String {
    let stripped: String = raw.chars().filter(|c| !is_bidi_override(*c)).collect();
    sanitize_filename(&stripped)
}

/// Splits a decoded path component on both separators and drops `""`, `.` and `..`.
fn split_component(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(['/', '\\'])
        .filter(|p| !p.trim().is_empty() && *p != "." && *p != "..")
}

fn key(components: &[String]) -> String {
    components
        .join("/")
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

fn with_suffix(name: &str, n: u32) -> String {
    let (stem, ext) = split_extension(name);
    let candidate = match ext {
        Some(e) => format!("{stem} ({n}).{e}"),
        None => format!("{stem} ({n})"),
    };
    // re-sanitise: the suffix may push a name over the byte cap
    sanitize_filename(&candidate)
}

/// Plans the on-disk layout for `files`.
///
/// `torrent_name` is the `name` field of the torrent: for a torrent with two or more files it
/// becomes the (sanitised) top-level folder, for a single file the file's own path is used.
/// `exists` tells whether a relative path is already taken on disk (pass `&|_| false` when the
/// target is known to be empty or when adopting existing data).
///
/// The result has one relative path per input file, in the same order.
pub fn plan_layout(
    torrent_name: &str,
    files: &[RawFile],
    exists: &dyn Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>> {
    let top = if files.len() >= 2 {
        Some(sanitize_component(torrent_name))
    } else {
        None
    };

    let mut taken_files: HashSet<String> = HashSet::new();
    let mut taken_dirs: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(files.len());

    for (index, file) in files.iter().enumerate() {
        let mut comps: Vec<String> = Vec::new();
        if let Some(t) = &top {
            comps.push(t.clone());
        }
        for raw in &file.components {
            for part in split_component(raw) {
                comps.push(sanitize_component(part));
            }
        }
        if comps.is_empty() || (top.is_some() && comps.len() == 1) {
            comps.push(format!("file-{index}"));
        }
        if comps.len() > MAX_PATH_DEPTH {
            return Err(Error::InvalidTorrent(format!(
                "file {index} is nested {} folders deep (limit {MAX_PATH_DEPTH})",
                comps.len()
            )));
        }

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            if attempt > 10_000 {
                return Err(Error::InvalidTorrent(
                    "too many files with the same name".into(),
                ));
            }
            // a file already sits where we need a folder: rename that folder component
            let clash_dir =
                (0..comps.len() - 1).find(|&i| taken_files.contains(&key(&comps[..=i])));
            if let Some(i) = clash_dir {
                comps[i] = with_suffix(&comps[i], attempt);
                continue;
            }
            let k = key(&comps);
            let rel: PathBuf = comps.iter().collect();
            if taken_files.contains(&k)
                || taken_dirs.contains(&k)
                || (!file.padding && exists(&rel))
            {
                let last = comps.len() - 1;
                comps[last] = with_suffix(&comps[last], attempt);
                continue;
            }
            break;
        }

        let rel_len: usize = comps.iter().map(|c| c.len() + 1).sum();
        if rel_len > MAX_RELATIVE_PATH_BYTES {
            return Err(Error::InvalidTorrent(format!(
                "the path of file {index} is {rel_len} bytes long (limit {MAX_RELATIVE_PATH_BYTES})"
            )));
        }

        taken_files.insert(key(&comps));
        for i in 0..comps.len() - 1 {
            taken_dirs.insert(key(&comps[..=i]));
        }
        out.push(comps.iter().collect());
    }
    Ok(out)
}

/// Joins `rel` onto `root`, refusing anything that is not a plain relative path.
///
/// Absolute paths, drive prefixes, `..` and `.` components all fail with [`Error::PathEscape`].
/// The check is lexical; [`ensure_no_symlink_escape`] covers links that already exist on disk.
pub fn confine(root: &Path, rel: &Path) -> Result<PathBuf> {
    if rel.as_os_str().is_empty() {
        return Err(Error::PathEscape(rel.to_path_buf()));
    }
    for c in rel.components() {
        if !matches!(c, Component::Normal(_)) {
            return Err(Error::PathEscape(rel.to_path_buf()));
        }
    }
    Ok(root.join(rel))
}

/// Checks that the already existing part of `path` really lives under `root` once symbolic
/// links and junctions are resolved. `path` itself may not exist yet.
pub fn ensure_no_symlink_escape(root: &Path, path: &Path) -> Result<()> {
    let real_root = std::fs::canonicalize(root)?;
    let mut probe = path.to_path_buf();
    // walk up to the deepest ancestor that exists
    while !probe.exists() {
        if !probe.pop() {
            return Err(Error::PathEscape(path.to_path_buf()));
        }
    }
    let real = std::fs::canonicalize(&probe)?;
    if real.starts_with(&real_root) {
        Ok(())
    } else {
        Err(Error::PathEscape(path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(parts: &[&str]) -> RawFile {
        RawFile {
            components: parts.iter().map(|s| s.to_string()).collect(),
            len: 1,
            padding: false,
        }
    }

    fn plan(name: &str, files: &[RawFile]) -> Vec<PathBuf> {
        plan_layout(name, files, &|_| false).unwrap()
    }

    fn assert_safe(p: &Path) {
        assert!(confine(Path::new("root"), p).is_ok(), "unsafe: {p:?}");
        for c in p.components() {
            let s = c.as_os_str().to_string_lossy();
            assert!(!s.ends_with('.') && !s.ends_with(' '), "trailing dot: {s}");
            assert!(s.len() <= 200);
            assert!(!s.chars().any(|c| c.is_control()));
        }
    }

    #[test]
    fn traversal_is_neutralised() {
        let p = plan(
            "t",
            &[
                raw(&["..", "..", "evil.txt"]),
                raw(&["a", "..", "b", "x"]),
                raw(&["/etc/passwd"]),
                raw(&["C:\\Windows\\System32\\x.dll"]),
            ],
        );
        assert_eq!(p[0], Path::new("t/evil.txt"));
        assert_eq!(p[1], Path::new("t/a/b/x"));
        assert_eq!(p[2], Path::new("t/etc/passwd"));
        assert_eq!(p[3], Path::new("t/C/Windows/System32/x.dll"));
        p.iter().for_each(|x| assert_safe(x));
    }

    #[test]
    fn windows_reserved_names_and_trailing_dots() {
        let p = plan(
            "pack",
            &[
                raw(&["CON"]),
                raw(&["nul.txt"]),
                raw(&["Lpt1", "file."]),
                raw(&["x. . "]),
            ],
        );
        assert_eq!(p[0], Path::new("pack/_CON"));
        assert_eq!(p[1], Path::new("pack/_nul.txt"));
        assert_eq!(p[2], Path::new("pack/_Lpt1/file"));
        assert_eq!(p[3], Path::new("pack/x"));
    }

    #[test]
    fn control_and_bidi_characters_are_removed() {
        let p = plan(
            "t",
            &[
                raw(&["photo\u{202E}gpj.exe"]),
                raw(&["a\u{0}b\u{7}c\n.txt"]),
            ],
        );
        assert_eq!(p[0], Path::new("t/photogpj.exe"));
        assert_eq!(p[1], Path::new("t/abc.txt"));
    }

    #[test]
    fn long_names_are_capped() {
        let long = "ا".repeat(400);
        let p = plan("t", &[raw(&[&format!("{long}.mkv")])]);
        let name = p[0].file_name().unwrap().to_string_lossy().to_string();
        assert!(name.len() <= 200 && name.ends_with(".mkv"));
    }

    #[test]
    fn duplicates_after_sanitising_are_renamed_not_merged() {
        let p = plan(
            "t",
            &[
                raw(&["a.txt"]),
                raw(&["A.TXT"]),
                raw(&["a.txt."]),
                raw(&["x", "..", "a.txt"]),
            ],
        );
        let set: HashSet<_> = p
            .iter()
            .map(|x| x.to_string_lossy().to_lowercase())
            .collect();
        assert_eq!(set.len(), 4, "{p:?}");
    }

    #[test]
    fn file_and_folder_with_same_name_do_not_clash() {
        let p = plan("t", &[raw(&["a"]), raw(&["a", "b.txt"])]);
        assert_ne!(p[0], p[1].parent().unwrap().to_path_buf());
        let q = plan("t", &[raw(&["a", "b.txt"]), raw(&["a"])]);
        assert_ne!(q[1], q[0].parent().unwrap().to_path_buf());
    }

    #[test]
    fn existing_files_on_disk_are_not_overwritten() {
        let p = plan_layout("t", &[raw(&["a.txt"])], &|r| r == Path::new("a.txt")).unwrap();
        assert_eq!(p[0], Path::new("a (1).txt"));
    }

    #[test]
    fn single_file_uses_its_own_name_multi_file_gets_a_folder() {
        assert_eq!(
            plan("ignored", &[raw(&["movie.mkv"])])[0],
            Path::new("movie.mkv")
        );
        let m = plan("Show ../S1", &[raw(&["a"]), raw(&["b"])]);
        assert_eq!(m[0], Path::new("S1/a"));
    }

    #[test]
    fn absurd_depth_and_length_are_rejected() {
        let deep: Vec<&str> = vec!["d"; 40];
        assert!(plan_layout("t", &[raw(&deep)], &|_| false).is_err());
        let many: Vec<String> = (0..20).map(|_| "x".repeat(150)).collect();
        let f = RawFile {
            components: many,
            len: 1,
            padding: false,
        };
        assert!(plan_layout("t", &[f], &|_| false).is_err());
    }

    #[test]
    fn confine_rejects_anything_but_plain_components() {
        let root = Path::new("root");
        assert!(confine(root, Path::new("../x")).is_err());
        assert!(confine(root, Path::new("a/../../x")).is_err());
        assert!(confine(root, Path::new("/abs")).is_err());
        assert!(confine(root, Path::new("")).is_err());
        #[cfg(windows)]
        {
            assert!(confine(root, Path::new("C:\\x")).is_err());
            assert!(confine(root, Path::new("C:x")).is_err());
            assert!(confine(root, Path::new("\\\\server\\share\\x")).is_err());
        }
        assert!(confine(root, Path::new("a/b")).is_ok());
    }

    proptest::proptest! {
        #[test]
        fn any_input_yields_confined_unique_paths(
            parts in proptest::collection::vec(proptest::collection::vec(".{0,40}", 1..5), 1..6)
        ) {
            let files: Vec<RawFile> = parts
                .into_iter()
                .map(|c| RawFile { components: c, len: 1, padding: false })
                .collect();
            if let Ok(plan) = plan_layout("name", &files, &|_| false) {
                proptest::prop_assert_eq!(plan.len(), files.len());
                let mut seen = HashSet::new();
                for p in &plan {
                    proptest::prop_assert!(confine(Path::new("root"), p).is_ok());
                    proptest::prop_assert!(seen.insert(p.to_string_lossy().to_lowercase()));
                }
            }
        }
    }
}
