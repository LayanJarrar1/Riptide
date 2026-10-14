//! Learn what a URL is before downloading it: size, range support, validators, type, name.

use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::{Client, Method, Response};
use riptide_core::sanitize_filename;
use url::Url;

use crate::context::RequestContext;
use crate::disposition::filename_from_disposition;
use crate::error::Result;
use crate::http::{ensure_success, send};
use crate::redirect::parse_http_url;

/// What the probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// URL after redirects; segments are fetched from here.
    pub final_url: Url,
    /// Total size in bytes, `None` for chunked responses without `Content-Length`.
    pub size: Option<u64>,
    /// Whether the server honours `Range` (`Accept-Ranges: bytes` or a `206` answer).
    pub supports_ranges: bool,
    /// Strong or weak `ETag`, as sent.
    pub etag: Option<String>,
    /// `Last-Modified`, as sent.
    pub last_modified: Option<String>,
    /// `Content-Type`.
    pub content_type: Option<String>,
    /// `Content-Encoding` when the server compressed despite `Accept-Encoding: identity`; the
    /// bytes on disk are then the encoded bytes and `size` is their length.
    pub content_encoding: Option<String>,
    /// Sanitised file name: `Content-Disposition` → URL path → `download`, with an extension
    /// guessed from the content type when the name has none.
    pub filename: String,
    /// The name before sanitising (for the UI to show what the server said), if any.
    pub suggested_name: Option<String>,
}

impl Probe {
    /// The validator a resume sends in `If-Range`: the ETag if present, else `Last-Modified`.
    pub fn validator(&self) -> Option<&str> {
        self.etag.as_deref().or(self.last_modified.as_deref())
    }

    /// Whether a partial file can be resumed in place (ranges + known size + a validator).
    pub fn resumable(&self) -> bool {
        self.supports_ranges && self.size.is_some() && self.validator().is_some()
    }
}

/// Probes `url`: `HEAD` first; if the server refuses `HEAD` or hides the length, a `GET` with
/// `Range: bytes=0-0` whose body is dropped unread. Never downloads the file. A transient
/// answer (`5xx`, `429`, `408`) is retried twice, honouring `Retry-After` up to five seconds.
pub async fn probe(client: &Client, url: &str, ctx: &RequestContext) -> Result<Probe> {
    let url = parse_http_url(url)?;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match probe_once(client, &url, ctx).await {
            Err(e) if e.is_retryable() && attempt < 3 => {
                let wait = std::time::Duration::from_secs(e.retry_after_secs().unwrap_or(1).min(5));
                tracing::debug!(attempt, error = %e, ?wait, "probe retry");
                tokio::time::sleep(wait).await;
            }
            other => return other,
        }
    }
}

async fn probe_once(client: &Client, url: &Url, ctx: &RequestContext) -> Result<Probe> {
    let head = send(client, ctx, Method::HEAD, url.clone(), |r| r).await;
    if let Ok(sent) = head {
        let status = sent.response.status().as_u16();
        if sent.response.status().is_success()
            && sent.response.headers().contains_key(CONTENT_LENGTH)
        {
            return Ok(from_head(sent.response, sent.url));
        }
        // 405/501 (no HEAD), 403 (HEAD-only auth quirks), or no length: fall through to GET
        tracing::debug!(status, "HEAD inconclusive, probing with a ranged GET");
    }

    let sent = send(client, ctx, Method::GET, url.clone(), |r| {
        r.header(RANGE, "bytes=0-0")
    })
    .await?;
    ensure_success(&sent.response)?;
    let status = sent.response.status().as_u16();
    let headers = sent.response.headers().clone();
    let final_url = sent.url;
    drop(sent.response); // never read the body

    let (size, supports_ranges) = if status == 206 {
        let total = headers
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total);
        (total, true)
    } else {
        let len = headers
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        let accept = headers
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("bytes"));
        (len, accept)
    };
    Ok(build(final_url, &headers, size, supports_ranges))
}

fn from_head(response: Response, final_url: Url) -> Probe {
    let headers = response.headers().clone();
    let size = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let supports_ranges = headers
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("bytes"));
    build(final_url, &headers, size, supports_ranges)
}

fn build(
    final_url: Url,
    headers: &reqwest::header::HeaderMap,
    size: Option<u64>,
    supports_ranges: bool,
) -> Probe {
    let text = |name: &reqwest::header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let content_type = text(&CONTENT_TYPE).map(|t| {
        t.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    });
    let content_encoding = text(&CONTENT_ENCODING).filter(|e| !e.eq_ignore_ascii_case("identity"));
    let suggested_name = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
        .and_then(filename_from_disposition);
    let raw = suggested_name
        .clone()
        .unwrap_or_else(|| filename_from_url(&final_url));
    let mut filename = sanitize_filename(&raw);
    if riptide_core::split_extension(&filename).1.is_none() {
        if let Some(ext) = content_type.as_deref().and_then(extension_for) {
            filename = sanitize_filename(&format!("{filename}.{ext}"));
        }
    }
    Probe {
        final_url,
        size,
        supports_ranges,
        etag: text(&ETAG),
        last_modified: text(&LAST_MODIFIED),
        content_type,
        content_encoding,
        filename,
        suggested_name,
    }
}

/// Last non-empty path segment, percent-decoded.
pub fn filename_from_url(url: &Url) -> String {
    let last = url
        .path_segments()
        .and_then(|mut s| s.next_back().filter(|p| !p.is_empty()).map(str::to_owned))
        .unwrap_or_default();
    let decoded = percent_encoding::percent_decode_str(&last)
        .decode_utf8_lossy()
        .into_owned();
    if decoded.trim().is_empty() {
        riptide_core::filename::FALLBACK_FILENAME.to_string()
    } else {
        decoded
    }
}

/// `bytes 0-0/12345` → `12345`; `bytes 0-0/*` → `None`.
pub fn parse_content_range_total(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let total = rest.rsplit('/').next()?.trim();
    total.parse().ok()
}

/// `bytes 100-199/12345` → `(100, 199, Some(12345))`.
pub fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let total = total.trim().parse().ok();
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?, total))
}

/// A file extension for common content types, used only when the name has none.
fn extension_for(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/pdf" => "pdf",
        "application/x-7z-compressed" => "7z",
        "application/x-rar-compressed" | "application/vnd.rar" => "rar",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-tar" => "tar",
        "application/x-iso9660-image" => "iso",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-msi" => "msi",
        "video/mp4" => "mp4",
        "video/x-matroska" => "mkv",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/flac" | "audio/x-flac" => "flac",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "text/plain" => "txt",
        "application/json" => "json",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_parsing() {
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(parse_content_range_total("bytes 0-0/*"), None);
        assert_eq!(
            parse_content_range("bytes 100-199/12345"),
            Some((100, 199, Some(12345)))
        );
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, 9, None)));
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }

    #[test]
    fn url_names() {
        let u = Url::parse("https://h/a/b/%D9%85%D9%84%D9%81.zip?x=1").unwrap();
        assert_eq!(filename_from_url(&u), "ملف.zip");
        assert_eq!(
            filename_from_url(&Url::parse("https://h/").unwrap()),
            "download"
        );
        assert_eq!(
            filename_from_url(&Url::parse("https://h/dir/").unwrap()),
            "download"
        );
        assert_eq!(extension_for("application/pdf"), Some("pdf"));
        assert_eq!(extension_for("application/octet-stream"), None);
    }
}
