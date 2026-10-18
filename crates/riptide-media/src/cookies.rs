//! The browser's cookies for a page, handed to yt-dlp as a temporary Netscape cookie file.
//!
//! The file is created by `tempfile` in the user's temporary folder: on Unix with mode `0600`,
//! on Windows inside the user profile, whose ACL admits only the user (plus SYSTEM and
//! administrators). It is deleted when the [`CookieFile`] is dropped, that is when the yt-dlp run
//! that needed it has ended. Its contents are never logged; `Debug` shows only the path.

use std::io::Write;
use std::path::Path;

use url::Url;

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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut file = tempfile::Builder::new()
            .prefix("riptide-cookies-")
            .suffix(".txt")
            .tempfile()?;
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
}
