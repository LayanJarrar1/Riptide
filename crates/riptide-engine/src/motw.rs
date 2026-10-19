//! Mark-of-the-Web: tells Windows that a finished download came from the internet.
//!
//! Browsers write an NTFS alternate data stream `<file>:Zone.Identifier` with
//! `[ZoneTransfer]` / `ZoneId=3`; Explorer, SmartScreen and Office then show their "downloaded
//! from the internet" warnings before an `.exe`, `.msi` or macro document is opened. A download
//! manager that skips it silently removes that protection.
//!
//! Writing is best effort and Windows only (a no-op elsewhere): FAT, exFAT and many network
//! shares have no streams, and that must never fail a download. The URLs in the stream are
//! reduced to scheme, host, port and path: no credentials, query string or fragment (rule 4),
//! http(s) only, no control characters, length capped.

use std::path::Path;

use url::Url;

/// Longest URL kept in the stream.
const MAX_URL_LEN: usize = 1024;

/// A URL fit for `ReferrerUrl` / `HostUrl`, or `None` when it must be left out: only `http` and
/// `https` with a host survive, credentials, query and fragment are removed, anything with a
/// control character or longer than `MAX_URL_LEN` is dropped.
pub fn sanitize_zone_url(raw: &str) -> Option<String> {
    if raw.chars().any(|c| c.is_control()) {
        return None;
    }
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    let text = url.as_str();
    // `url` percent-encodes controls and spaces, but be strict about what is written
    (text.len() <= MAX_URL_LEN && !text.chars().any(|c| c.is_control() || c == ' '))
        .then(|| text.to_string())
}

/// The text of the `Zone.Identifier` stream: zone 3 (Internet) plus whichever URLs survived
/// [`sanitize_zone_url`]. CRLF line ends, as browsers write them.
pub fn zone_identifier_text(referrer: Option<&str>, host_url: Option<&str>) -> String {
    let mut out = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(r) = referrer.and_then(sanitize_zone_url) {
        out.push_str("ReferrerUrl=");
        out.push_str(&r);
        out.push_str("\r\n");
    }
    if let Some(h) = host_url.and_then(sanitize_zone_url) {
        out.push_str("HostUrl=");
        out.push_str(&h);
        out.push_str("\r\n");
    }
    out
}

/// Marks the finished file at `path` as downloaded from the internet. Best effort: returns
/// whether the stream was written; a failure is only logged (without any URL).
#[cfg(windows)]
pub fn mark_from_internet(path: &Path, referrer: Option<&str>, host_url: Option<&str>) -> bool {
    let mut stream = path.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    // UTF-8 without a BOM, which is what browsers write
    match std::fs::write(&stream, zone_identifier_text(referrer, host_url)) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(error = %e, "could not write the Mark-of-the-Web stream");
            false
        }
    }
}

/// Mark-of-the-Web is a Windows (NTFS) feature; nothing to do here.
#[cfg(not(windows))]
pub fn mark_from_internet(_path: &Path, _referrer: Option<&str>, _host_url: Option<&str>) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_lose_credentials_query_and_fragment() {
        assert_eq!(
            sanitize_zone_url("https://user:pw@cdn.example.com:8443/a/b.zip?token=SECRET&x=1#frag")
                .as_deref(),
            Some("https://cdn.example.com:8443/a/b.zip")
        );
        assert_eq!(
            sanitize_zone_url("http://example.com").as_deref(),
            Some("http://example.com/")
        );
    }

    #[test]
    fn only_plain_http_urls_are_kept() {
        for bad in [
            "file:///C:/secret.txt",
            "ftp://example.com/x",
            "javascript:alert(1)",
            "data:text/plain,hi",
            "not a url",
            "",
            "https://example.com/a\r\nZoneId=0",
            "https://example.com/\u{7}",
        ] {
            assert_eq!(sanitize_zone_url(bad), None, "{bad:?}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        assert_eq!(sanitize_zone_url(&long), None, "over the cap");
    }

    #[test]
    fn the_stream_text_is_zone_3_with_clean_urls() {
        let text = zone_identifier_text(
            Some("https://example.com/page?id=7"),
            Some("https://cdn.example.com/f.exe?sig=abc"),
        );
        assert_eq!(
            text,
            "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/page\r\n\
             HostUrl=https://cdn.example.com/f.exe\r\n"
        );
        assert!(!text.contains("sig=") && !text.contains("id=7"));
        // unusable URLs are left out, the zone stays
        assert_eq!(
            zone_identifier_text(Some("file:///x"), None),
            "[ZoneTransfer]\r\nZoneId=3\r\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_stream_is_written_and_the_content_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("setup.exe");
        std::fs::write(&file, b"payload").unwrap();
        let written = mark_from_internet(
            &file,
            Some("https://a.test/p?q=1"),
            Some("https://b.test/f"),
        );
        assert!(written, "the temp folder must be on NTFS");
        let mut stream = file.clone().into_os_string();
        stream.push(":Zone.Identifier");
        let text = std::fs::read_to_string(&stream).unwrap();
        assert!(
            text.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"),
            "{text:?}"
        );
        assert!(text.contains("ReferrerUrl=https://a.test/p\r\n"));
        assert!(text.contains("HostUrl=https://b.test/f\r\n"));
        assert_eq!(std::fs::read(&file).unwrap(), b"payload");
    }
}
