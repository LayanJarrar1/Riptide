//! Clipboard monitor: while "clipboard monitor" is on, a copied `http(s)` link whose path ends
//! in a known downloadable extension (the category table of `riptide_core::Category`) is offered
//! to the webview as [`CLIPBOARD_LINK_EVENT`]; the webview asks the user (it never starts a
//! download by itself) and skips links already in the list. The same URL is offered once.
//!
//! The clipboard is polled once a second on its own thread, and only while the setting is on;
//! the text is only read, never logged.

use std::collections::HashSet;
use std::time::Duration;

use riptide_core::Category;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::shell;

/// Tauri event carrying a [`ClipboardLink`].
pub const CLIPBOARD_LINK_EVENT: &str = "riptide://clipboard-link";

/// Longest clipboard text considered a link.
const MAX_URL_LEN: usize = 4096;

/// How many offered URLs are remembered (so "the same URL twice" is ignored).
const SEEN_CAP: usize = 512;

/// A downloadable link found on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClipboardLink {
    /// The link, as copied (trimmed).
    pub url: String,
    /// The last path segment, percent-decoded: what the prompt names.
    pub name: String,
}

/// Whether `text` is one `http(s)` URL whose last path segment has a known downloadable
/// extension; returns the link and its file name.
pub fn downloadable_link(text: &str) -> Option<ClipboardLink> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_URL_LEN || text.chars().any(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return None;
    }
    let last = url.path_segments()?.next_back()?;
    let name = percent_decode(last);
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    let ext = ext.to_ascii_lowercase();
    if !Category::ALL
        .iter()
        .any(|c| c.extensions().contains(&ext.as_str()))
    {
        return None;
    }
    Some(ClipboardLink {
        url: text.to_string(),
        name,
    })
}

/// `%XX` decoding of a path segment (invalid escapes stay as they are, bytes are read as UTF-8
/// lossily).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok());
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Remembers what was already seen on the clipboard and decides what to offer.
#[derive(Debug, Default)]
pub struct Watcher {
    last_text: Option<String>,
    offered: HashSet<String>,
}

impl Watcher {
    /// Takes the current clipboard text; returns a link to offer, at most once per URL and
    /// only when the text changed since the last look.
    pub fn observe(&mut self, text: &str) -> Option<ClipboardLink> {
        if self.last_text.as_deref() == Some(text) {
            return None;
        }
        self.last_text = Some(text.to_string());
        let link = downloadable_link(text)?;
        if self.offered.contains(&link.url) {
            return None;
        }
        if self.offered.len() >= SEEN_CAP {
            self.offered.clear();
        }
        self.offered.insert(link.url.clone());
        Some(link)
    }

    /// Records the current text without offering it (what was on the clipboard before the
    /// monitor was turned on is not news).
    pub fn prime(&mut self, text: Option<String>) {
        self.last_text = text;
    }
}

/// Starts the polling thread.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("riptide-clipboard".into())
        .spawn(move || {
            let mut watcher = Watcher::default();
            let mut was_on = false;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let on = shell::prefs(&app).clipboard_monitor;
                if !on {
                    was_on = false;
                    continue;
                }
                let text = app.clipboard().read_text().ok();
                if !was_on {
                    was_on = true;
                    watcher.prime(text);
                    continue;
                }
                let Some(text) = text else { continue };
                if let Some(link) = watcher.observe(&text) {
                    if let Err(e) = app.emit(CLIPBOARD_LINK_EVENT, &link) {
                        tracing::debug!(error = %e, "clipboard link not delivered");
                    }
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "clipboard monitor not started");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_links_with_known_extensions() {
        let link = downloadable_link("  https://example.com/files/Setup.EXE?x=1#y \n").unwrap();
        assert_eq!(link.url, "https://example.com/files/Setup.EXE?x=1#y");
        assert_eq!(link.name, "Setup.EXE");
        assert!(downloadable_link("http://host:8080/a/b/movie.mkv").is_some());
        assert!(downloadable_link("https://h/archive.zip").is_some());
    }

    #[test]
    fn decodes_the_name() {
        let link =
            downloadable_link("https://example.com/%D8%AA%D9%82%D8%B1%D9%8A%D8%B1%20final.pdf")
                .unwrap();
        assert_eq!(link.name, "تقرير final.pdf");
    }

    #[test]
    fn rejects_everything_else() {
        for text in [
            "",
            "hello world",
            "https://example.com/",
            "https://example.com/page.html",
            "https://example.com/page",
            "https://example.com/.zip",
            "ftp://example.com/file.zip",
            "file:///C:/file.zip",
            "javascript:alert(1)//x.zip",
            "https://example.com/a.zip https://example.com/b.zip",
            "https://example.com/a.zip\nhttps://example.com/b.zip",
        ] {
            assert!(downloadable_link(text).is_none(), "{text:?}");
        }
        let long = format!("https://example.com/{}.zip", "a".repeat(MAX_URL_LEN));
        assert!(downloadable_link(&long).is_none());
    }

    #[test]
    fn watcher_offers_each_url_once_and_skips_primed_text() {
        let mut w = Watcher::default();
        w.prime(Some("https://example.com/old.zip".into()));
        assert!(w.observe("https://example.com/old.zip").is_none(), "primed");
        assert!(w.observe("https://example.com/a.zip").is_some());
        assert!(
            w.observe("https://example.com/a.zip").is_none(),
            "unchanged"
        );
        assert!(w.observe("something else").is_none());
        assert!(
            w.observe("https://example.com/a.zip").is_none(),
            "already offered"
        );
        assert!(w.observe("https://example.com/b.iso").is_some());
    }

    #[test]
    fn percent_decode_keeps_bad_escapes() {
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("%41%42"), "AB");
    }
}
