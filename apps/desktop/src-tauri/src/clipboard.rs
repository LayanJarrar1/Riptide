//! Clipboard monitor: while "clipboard monitor" is on, a copied `http(s)` link whose path ends
//! in a known downloadable extension (the category table of `riptide_core::Category`), or a
//! `magnet:?xt=urn:btih:...` link, is offered
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
    if let Some(link) = magnet_link(text) {
        return Some(link);
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

/// A `magnet:` link that names a BitTorrent info-hash (hex or base32); its name is the `dn`
/// (display name) parameter, else the hash.
fn magnet_link(text: &str) -> Option<ClipboardLink> {
    magnet_parts(text).map(|(link, _)| link)
}

/// [`magnet_link`] plus the info-hash it names, lower-cased (what identifies the torrent).
fn magnet_parts(text: &str) -> Option<(ClipboardLink, String)> {
    let (scheme, rest) = text.split_at_checked(8)?;
    if !scheme.eq_ignore_ascii_case("magnet:?") {
        return None;
    }
    let mut hash = None;
    let mut name = None;
    for pair in rest.split('&') {
        // a stray `&&`, a bare `&kt` or an empty value must not hide the link
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        match k.to_ascii_lowercase().as_str() {
            "xt" => {
                let Some(h) = v
                    .get(..9)
                    .filter(|p| p.eq_ignore_ascii_case("urn:btih:"))
                    .and_then(|_| v.get(9..))
                else {
                    continue;
                };
                let hex = h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit());
                let b32 = h.len() == 32
                    && h.bytes()
                        .all(|b| b.is_ascii_alphabetic() || (b'2'..=b'7').contains(&b));
                if hex || b32 {
                    hash = Some(h.to_string());
                }
            }
            "dn" => name = Some(percent_decode(&v.replace('+', " "))),
            _ => {}
        }
    }
    let hash: String = hash?;
    let key = hash.to_ascii_lowercase();
    Some((
        ClipboardLink {
            url: text.to_string(),
            name: name.filter(|n| !n.trim().is_empty()).unwrap_or(hash),
        },
        key,
    ))
}

/// `%XX` decoding of a path segment (invalid escapes stay as they are, bytes are read as UTF-8
/// lossily).
pub(crate) fn percent_decode(s: &str) -> String {
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

/// Most magnet links taken from one block of text.
const MAX_MAGNETS: usize = 50;

/// The magnet links in `text`, one per line (blank lines and other text are skipped, repeats of
/// the same info-hash collapse). Used by "Open magnet from clipboard" and by the Add dialog when
/// several links are pasted at once.
pub fn magnet_lines(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.len() > MAX_URL_LEN || line.chars().any(char::is_whitespace) {
            continue;
        }
        let Some((link, key)) = magnet_parts(line) else {
            continue;
        };
        // one entry per torrent: two different torrents may well share a display name
        if out.len() < MAX_MAGNETS && seen.insert(key) {
            out.push(link.url);
        }
    }
    out
}

/// The magnet links currently on the clipboard (File menu, "Open magnet from clipboard"). The
/// text is only read; nothing is logged.
#[tauri::command]
pub fn clipboard_magnets<R: Runtime>(app: AppHandle<R>) -> Vec<String> {
    app.clipboard()
        .read_text()
        .map(|t| magnet_lines(&t))
        .unwrap_or_default()
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
    fn offers_magnet_links_with_their_display_name() {
        let hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
        let link = downloadable_link(&format!(
            "magnet:?xt=urn:btih:{hash}&dn=Big+Buck%20Bunny&tr=udp%3A%2F%2Fx%3A80"
        ))
        .unwrap();
        assert_eq!(link.name, "Big Buck Bunny");
        assert!(link.url.starts_with("magnet:?xt=urn:btih:"));
        // without a name the hash stands in; the scheme is case-insensitive
        assert_eq!(
            downloadable_link(&format!("MAGNET:?xt=urn:btih:{hash}"))
                .unwrap()
                .name,
            hash
        );
        // not a BitTorrent link, malformed hash, spaces
        assert!(downloadable_link("magnet:?xt=urn:sha1:abc").is_none());
        assert!(downloadable_link("magnet:?xt=urn:btih:short").is_none());
        assert!(downloadable_link("magnet:?dn=only-a-name").is_none());
        assert!(downloadable_link(&format!("magnet:?xt=urn:btih:{hash} extra")).is_none());
    }

    #[test]
    fn offers_a_pasted_magnet_with_an_encoded_tracker_and_an_empty_value() {
        let owner = "magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D";
        let link = downloadable_link(owner).unwrap();
        assert_eq!(link.url, owner);
        assert_eq!(link.name, "428c72ff4aac627830cfacdc8622be0e908c3b65");
        // stray separators and bare keys do not hide it
        let loose = "magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&&kt&dn=&tr=";
        assert_eq!(
            downloadable_link(loose).unwrap().name,
            "428c72ff4aac627830cfacdc8622be0e908c3b65"
        );
    }

    #[test]
    fn magnet_lines_picks_links_one_per_line() {
        let a = "magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D";
        let b = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny";
        let text = format!(
            "notes
{a}

  {b}  
{a}
https://example.com/x.zip
magnet:?xt=urn:sha1:abc
"
        );
        assert_eq!(magnet_lines(&text), [a.to_string(), b.to_string()]);
        assert!(magnet_lines("no links here").is_empty());
        assert!(magnet_lines("").is_empty());
    }

    #[test]
    fn two_torrents_with_the_same_display_name_are_both_offered() {
        let a = "magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&dn=Ubuntu";
        let b = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Ubuntu";
        let again = "magnet:?xt=urn:btih:428C72FF4AAC627830CFACDC8622BE0E908C3B65&dn=Other";
        assert_eq!(
            magnet_lines(&format!("{a}\n{b}\n{again}")),
            [a.to_string(), b.to_string()]
        );
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
