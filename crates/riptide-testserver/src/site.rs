//! `/site/` — a small crawlable web site for the site-grabber tests, plus `/robots.txt`.
//!
//! The layout (all paths are below `/site/`; `{alt}` is the other listener's origin):
//!
//! ```text
//! /                       depth 0   report.pdf, photo.jpg (img + srcset), links to everything below
//! ├─ docs/                depth 1   song.mp3 (<audio>), chapter1.html
//! │  └─ chapter1.html     depth 2   archive.zip, deep/appendix.html
//! │     └─ deep/appendix.html depth 3   clip.mp4 (<source>), setup.exe, a link back to /site/ (loop)
//! ├─ about.html           depth 1   notes.txt, a link back to /site/ (loop)
//! ├─ private/             disallowed by /robots.txt; private/secret.pdf too
//! ├─ big.html             6 MiB (over the crawler's 5 MiB cap), Content-Length known
//! ├─ big-chunked.html     6 MiB, chunked, no Content-Length
//! ├─ missing.html         404
//! ├─ download?id=1        a "page-looking" link that is really a file: text/csv, attachment export.csv
//! ├─ {alt}/site/offsite/file.zip   an off-site file
//! └─ {alt}/site/offsite/page       an off-site HTML page (links to hidden.zip; must not be crawled)
//! ```
//!
//! The index also carries things that must *not* be followed: a stylesheet, `mailto:` and
//! `javascript:` links, a link inside `<script>` and one inside a comment. File bodies are the
//! deterministic bytes of [`site_file_seed`] and the sizes in [`SITE_FILES`], so tests compare
//! SHA-256 with [`site_file_sha256_hex`].

use std::io;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;

use crate::content::expected_sha256_hex;
use crate::handlers::{plain, serve, Resource, Shaping};
use crate::state::ServerState;

/// Every file the site serves: `(path below /site/, size in bytes)`.
pub const SITE_FILES: &[(&str, u64)] = &[
    ("files/report.pdf", 120_000),
    ("files/song.mp3", 300_000),
    ("files/clip.mp4", 1_500_000),
    ("files/archive.zip", 700_000),
    ("files/photo.jpg", 90_000),
    ("files/notes.txt", 5_000),
    ("files/setup.exe", 400_000),
    ("private/secret.pdf", 30_000),
    ("offsite/file.zip", 50_000),
    ("offsite/hidden.zip", 40_000),
    ("export.csv", 2_000),
];

/// Size of `big.html` and `big-chunked.html`: over the crawler's default 5 MiB page cap.
const BIG_PAGE_BYTES: usize = 6 * 1024 * 1024;

/// What `/robots.txt` says.
const ROBOTS: &str = "# riptide-testserver\nUser-agent: *\nDisallow: /site/private/\n";

/// Seed of the bytes served for `path` (as listed in [`SITE_FILES`]): a hash of the path, so
/// every file differs.
#[must_use]
pub fn site_file_seed(path: &str) -> u64 {
    path.bytes().fold(0xC0FF_EE00_u64, |acc, b| {
        crate::content::splitmix64(acc ^ u64::from(b))
    })
}

fn file_size(path: &str) -> Option<u64> {
    SITE_FILES.iter().find(|(p, _)| *p == path).map(|(_, s)| *s)
}

/// SHA-256 (hex) of the body served for `path`, or `None` for an unknown file.
#[must_use]
pub fn site_file_sha256_hex(path: &str) -> Option<String> {
    file_size(path).map(|size| expected_sha256_hex(site_file_seed(path), size))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "zip" => "application/zip",
        "jpg" => "image/jpeg",
        "txt" => "text/plain",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
}

/// `/robots.txt`
pub(crate) async fn robots() -> Response {
    plain(StatusCode::OK, ROBOTS)
}

fn html(body: String) -> Response {
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

fn big_page(chunked: bool) -> Response {
    let head =
        "<!doctype html><html><body><a href=\"files/from-big.zip\">hidden in a big page</a><!--";
    let tail = "--></body></html>";
    let padding = BIG_PAGE_BYTES - head.len() - tail.len();
    let mut body = String::with_capacity(BIG_PAGE_BYTES);
    body.push_str(head);
    body.push_str(&"x".repeat(padding));
    body.push_str(tail);
    let mut resp = if chunked {
        let chunks: Vec<Result<Bytes, io::Error>> = body
            .as_bytes()
            .chunks(64 * 1024)
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();
        Response::new(Body::from_stream(futures::stream::iter(chunks)))
    } else {
        Response::new(Body::from(body))
    };
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

/// HTML of the page at `rel` (path below `/site/`), if there is one.
fn page_html(rel: &str, alt: &str) -> Option<String> {
    let doc = |title: &str, body: &str| {
        format!(
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{title}</title></head>\n<body>\n{body}\n</body></html>\n"
        )
    };
    Some(match rel {
        "" | "index.html" => format!(
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Riptide test site</title>\n\
             <link rel=\"stylesheet\" href=\"style.css\">\n\
             <link rel=\"alternate\" href=\"files/report.pdf\" type=\"application/pdf\">\n\
             </head>\n<body>\n\
             <a href=\"docs/\">Docs</a> <a href=\"about.html#team\">About</a> <a href=\"about.html\">About again</a>\n\
             <a href=\"files/report.pdf\">Report</a>\n\
             <img src=\"files/photo.jpg\" srcset=\"files/photo.jpg 1x, files/photo.jpg 2x\">\n\
             <a href=\"private/\">Private</a> <a href=\"private/secret.pdf\">Secret</a>\n\
             <a href=\"big.html\">Big</a> <a href=\"big-chunked.html\">Big chunked</a>\n\
             <a href=\"missing.html\">Missing</a>\n\
             <a href=\"download?id=1\">Export</a>\n\
             <a href=\"{alt}/site/offsite/file.zip\">Off-site file</a>\n\
             <a href=\"{alt}/site/offsite/page\">Off-site page</a>\n\
             <a href=\"mailto:someone@example.com\">mail</a> <a href=\"javascript:void(0)\">js</a>\n\
             <script>var x = \"<a href='files/script-only.zip'>no</a>\";</script>\n\
             <!-- <a href=\"files/commented.zip\">no</a> -->\n\
             </body></html>\n"
        ),
        "docs/" => doc(
            "Docs",
            "<a href=\"chapter1.html\">Chapter 1</a> <a href=\"../\">Up</a>\n<audio src=\"../files/song.mp3\"></audio>",
        ),
        "docs/chapter1.html" => doc(
            "Chapter 1",
            "<a href=\"deep/appendix.html\">Appendix</a>\n<a href=\"../files/archive.zip\">Archive</a>",
        ),
        "docs/deep/appendix.html" => doc(
            "Appendix",
            "<video controls><source src=\"../../files/clip.mp4\" type=\"video/mp4\"></video>\n\
             <a href=\"../../files/setup.exe\">Installer</a>\n<a href=\"../../\">Back to the start</a>",
        ),
        "about.html" => doc(
            "About",
            "<a href=\"files/notes.txt\">Notes</a> <a href=\"./\">Home</a>",
        ),
        "private/" => doc("Private", "<a href=\"secret.pdf\">secret</a>"),
        "offsite/page" => doc(
            "Off-site",
            "<a href=\"hidden.zip\">hidden</a> <a href=\"/site/\">site start</a> <a href=\"/site/docs/\">docs</a>",
        ),
        _ => return None,
    })
}

/// Everything below `/site/`.
pub(crate) async fn page(State(st): State<Arc<ServerState>>, req: Request) -> Response {
    let method = req.method().clone();
    let headers: HeaderMap = req.headers().clone();
    let rel = req
        .uri()
        .path()
        .strip_prefix("/site/")
        .unwrap_or("")
        .to_owned();
    let query = req.uri().query().unwrap_or("").to_owned();
    serve_site(&method, &headers, &rel, &query, &st)
}

fn serve_site(
    method: &Method,
    headers: &HeaderMap,
    rel: &str,
    query: &str,
    st: &ServerState,
) -> Response {
    match rel {
        "big.html" => return big_page(false),
        "big-chunked.html" => return big_page(true),
        "download" if query.split('&').any(|kv| kv == "id=1") => {
            return serve_file(method, headers, "export.csv", true);
        }
        _ => {}
    }
    if let Some(body) = page_html(rel, st.alt_origin()) {
        return html(body);
    }
    if file_size(rel).is_some() {
        return serve_file(method, headers, rel, false);
    }
    plain(StatusCode::NOT_FOUND, "not found")
}

fn serve_file(method: &Method, headers: &HeaderMap, path: &str, attachment: bool) -> Response {
    let Some(size) = file_size(path) else {
        return plain(StatusCode::NOT_FOUND, "not found");
    };
    let mut res = Resource::new(site_file_seed(path), size);
    if attachment {
        res.extra.push((
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"export.csv\""),
        ));
    }
    match serve(method, headers, res, Shaping::default()) {
        Ok(mut resp) => {
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(path)),
            );
            resp
        }
        Err(err) => err.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_has_a_distinct_seed_and_a_hash() {
        let mut seeds = std::collections::HashSet::new();
        for (path, _) in SITE_FILES {
            assert!(seeds.insert(site_file_seed(path)), "{path}");
            assert!(site_file_sha256_hex(path).is_some());
        }
        assert!(site_file_sha256_hex("nope").is_none());
    }

    #[test]
    fn pages_link_to_each_other_in_a_loop() {
        let index = page_html("", "http://alt").unwrap();
        assert!(index.contains("http://alt/site/offsite/page"));
        assert!(page_html("docs/deep/appendix.html", "x")
            .unwrap()
            .contains("../../\""));
        assert!(page_html("missing.html", "x").is_none());
    }
}
