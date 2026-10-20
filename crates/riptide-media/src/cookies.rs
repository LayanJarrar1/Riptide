//! The browser's cookies for a page, handed to yt-dlp as a temporary Netscape cookie file.
//!
//! The file is created by `tempfile`, in the app's own `tmp` folder when the caller gives one
//! ([`CookieFile::write_in`]) and otherwise in the user's temporary folder: on Unix with mode
//! `0600`, on Windows inside the user profile, whose ACL admits only the user (plus SYSTEM and
//! administrators). It is deleted when the [`CookieFile`] is dropped, that is when the yt-dlp run
//! that needed it has ended. A hard kill skips the drop, so the app calls
//! [`sweep_stale_cookie_files`] on its folder at start-up. Its contents are never logged; `Debug`
//! shows only the path.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use url::Url;

const FILE_PREFIX: &str = "riptide-cookies-";
const FILE_SUFFIX: &str = ".txt";

/// How old a leftover cookie file must be before the start-up sweep removes it, so a file that
/// a running process has just written is never taken away.
pub const STALE_AFTER: Duration = Duration::from_secs(60);

/// Deletes `riptide-cookies-*.txt` files older than `min_age` from `dir` (leftovers of a process
/// that was killed before it could clean up). Other files are never touched; a missing folder is
/// fine. Returns how many were removed. Call it once at start-up, before any media job.
pub fn sweep_stale_cookie_files(dir: &Path, min_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX)) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let age = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .unwrap_or(Duration::MAX);
        if age >= min_age && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// A temporary cookie file. Deleted on drop.
pub struct CookieFile {
    file: tempfile::NamedTempFile,
}

impl std::fmt::Debug for CookieFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieFile")
            .field("path", &self.file.path())
            .finish_non_exhaustive()
    }
}

/// Cookie domain for a page: `www.` is dropped and a leading dot makes the cookie apply to
/// subdomains too (`m.youtube.com`, `music.youtube.com`).
fn cookie_domain(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let base = host.strip_prefix("www.").unwrap_or(&host);
    format!(".{base}")
}

fn clean(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_control())
}

/// Renders a `Cookie:` header value (`a=1; b=2`) captured for `page` as Netscape cookie-file
/// text. Pairs with control characters (tabs, newlines) are dropped: they could forge extra
/// lines. Expiry is one day from `now_secs` so yt-dlp does not discard them as session cookies.
pub fn netscape_text(page: &Url, cookie_header: &str, now_secs: u64) -> String {
    let host = page.host_str().unwrap_or_default();
    let domain = cookie_domain(host);
    let secure = if page.scheme() == "https" {
        "TRUE"
    } else {
        "FALSE"
    };
    let expiry = now_secs + 86_400;
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for pair in cookie_header.split(';') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if !clean(name) || value.chars().any(|c| c.is_control()) {
            continue;
        }
        out.push_str(&format!(
            "{domain}\tTRUE\t/\t{secure}\t{expiry}\t{name}\t{value}\n"
        ));
    }
    out
}

impl CookieFile {
    /// Writes the cookies for `page` to a new temporary file.
    pub fn write(page: &Url, cookie_header: &str) -> std::io::Result<CookieFile> {
        Self::write_in(None, page, cookie_header)
    }

    /// Like [`CookieFile::write`], in `dir` (created if missing) when given. The name carries
    /// the process id and a random part.
    pub fn write_in(
        dir: Option<&Path>,
        page: &Url,
        cookie_header: &str,
    ) -> std::io::Result<CookieFile> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prefix = format!("{FILE_PREFIX}{}-", std::process::id());
        let mut builder = tempfile::Builder::new();
        builder.prefix(&prefix).suffix(FILE_SUFFIX);
        let mut file = match dir {
            Some(dir) => {
                std::fs::create_dir_all(dir)?;
                builder.tempfile_in(dir)?
            }
            None => builder.tempfile()?,
        };
        file.write_all(netscape_text(page, cookie_header, now).as_bytes())?;
        file.flush()?;
        Ok(CookieFile { file })
    }

    /// Where the file is (passed to `--cookies`).
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_becomes_netscape_lines_for_the_page_domain() {
        let page = Url::parse("https://www.youtube.com/watch?v=x").unwrap();
        let text = netscape_text(
            &page,
            "SID=abc; PREF=f6=40000000; bad\tname=1; x=a\nb",
            1_000,
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "# Netscape HTTP Cookie File");
        assert_eq!(lines[1], ".youtube.com\tTRUE\t/\tTRUE\t87400\tSID\tabc");
        assert_eq!(
            lines[2],
            ".youtube.com\tTRUE\t/\tTRUE\t87400\tPREF\tf6=40000000"
        );
        assert_eq!(
            lines.len(),
            3,
            "pairs with control characters are dropped: {text:?}"
        );
        let http = Url::parse("http://vimeo.com/1").unwrap();
        assert!(netscape_text(&http, "a=1", 0).contains(".vimeo.com\tTRUE\t/\tFALSE\t86400\ta\t1"));
    }

    #[test]
    fn the_file_is_deleted_on_drop_and_debug_hides_values() {
        let page = Url::parse("https://youtube.com/").unwrap();
        let file = CookieFile::write(&page, "SID=topsecret").unwrap();
        let path = file.path().to_path_buf();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("topsecret"));
        assert!(!format!("{file:?}").contains("topsecret"));
        drop(file);
        assert!(!path.exists());
    }

    #[test]
    fn a_leftover_from_a_killed_run_is_swept_and_other_files_stay() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("riptide-cookies-4242-abc.txt");
        let other = dir.path().join("notes.txt");
        std::fs::write(&old, "SID=topsecret").unwrap();
        std::fs::write(&other, "keep").unwrap();
        // a fresh file survives a one-minute sweep ...
        assert_eq!(sweep_stale_cookie_files(dir.path(), STALE_AFTER), 0);
        assert!(old.exists());
        // ... and an old one does not
        assert_eq!(sweep_stale_cookie_files(dir.path(), Duration::ZERO), 1);
        assert!(!old.exists());
        assert!(other.exists());
        assert_eq!(
            sweep_stale_cookie_files(&dir.path().join("missing"), Duration::ZERO),
            0
        );
    }

    #[test]
    fn files_go_to_the_given_folder_with_the_pid_in_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let page = Url::parse("https://youtube.com/").unwrap();
        let sub = dir.path().join("tmp");
        let file = CookieFile::write_in(Some(&sub), &page, "SID=x").unwrap();
        assert_eq!(file.path().parent(), Some(sub.as_path()));
        let name = file
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(name.starts_with(&format!("riptide-cookies-{}-", std::process::id())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "group/other have no access: {mode:o}");
        }
    }
}
