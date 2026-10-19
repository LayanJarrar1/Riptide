//! The crawl request, its events and its result.

use std::path::Path;
use std::time::Duration;

use riptide_core::{Category, DownloadRequest, Secrets};
use riptide_net::Origin;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::scope::Scope;

/// Default cap on pages fetched.
pub const DEFAULT_MAX_PAGES: usize = 500;
/// Default cap on files listed.
pub const DEFAULT_MAX_FILES: usize = 5_000;
/// Default cap on the size of one HTML page.
pub const DEFAULT_MAX_PAGE_BYTES: u64 = 5 * 1024 * 1024;
/// Links read from one page at most; the rest are ignored.
pub const MAX_LINKS_PER_PAGE: usize = 20_000;

/// A file the crawl found earlier, for sync mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownFile {
    /// Its URL (as in [`FoundFile::url`]).
    pub url: String,
    /// The `ETag` seen then.
    pub etag: Option<String>,
    /// The `Last-Modified` seen then.
    pub last_modified: Option<String>,
}

impl From<&FoundFile> for KnownFile {
    fn from(f: &FoundFile) -> Self {
        KnownFile {
            url: f.url.clone(),
            etag: f.etag.clone(),
            last_modified: f.last_modified.clone(),
        }
    }
}

/// What to crawl and how far. `Debug` never prints cookies or credentials (`Secrets` redacts).
#[derive(Debug, Clone)]
pub struct CrawlSpec {
    /// Start page.
    pub start: String,
    /// Link depth: `0` reads only the start page, `1` also the pages it links to, and so on.
    pub depth: u32,
    /// Which pages may be walked into.
    pub scope: Scope,
    /// List files linked from in-scope pages even when they live elsewhere (a CDN, another
    /// site), and look one step into off-site links of unknown type to see whether they are
    /// files. Off-site pages themselves are never read.
    pub offsite_files: bool,
    /// Glob patterns (see [`crate::glob::Glob`]); when not empty, only files matching one are
    /// listed. Pages are not filtered by this.
    pub include: Vec<String>,
    /// Glob patterns; pages and files matching any are skipped.
    pub exclude: Vec<String>,
    /// File types to list; empty means all.
    pub categories: Vec<Category>,
    /// Skip files smaller than this (needs a size, so files are probed).
    pub min_size: Option<u64>,
    /// Skip files larger than this (needs a size, so files are probed).
    pub max_size: Option<u64>,
    /// `HEAD`-probe every listed file for its size, validators and `Content-Disposition` name.
    /// Implied by `min_size`, `max_size` and `previous`.
    pub probe_files: bool,
    /// Fetch links of unknown type (`/download?id=3`) that are off-site or beyond the depth just
    /// far enough to see whether they are files. Turn off to save requests.
    pub resolve_unknown_links: bool,
    /// Pages fetched at most (including the probes that resolve unknown links).
    pub max_pages: usize,
    /// Files listed at most.
    pub max_files: usize,
    /// Largest HTML page that is read; a bigger one is skipped.
    pub max_page_bytes: u64,
    /// Wall-clock budget for the whole crawl.
    pub time_budget: Duration,
    /// Timeout of one request.
    pub request_timeout: Duration,
    /// Requests in flight in total.
    pub concurrency: usize,
    /// Requests in flight per host (politeness).
    pub per_host: usize,
    /// Minimum gap between two requests to one host; a larger `Crawl-delay` wins.
    pub delay: Duration,
    /// Obey `robots.txt`. Turning it off is the user's explicit choice for their own sites.
    pub respect_robots: bool,
    /// Cookies / headers / credentials. Sent to the start page's origin **only**.
    pub secrets: Option<Secrets>,
    /// A previous result for sync mode: matching files get [`FoundFile::changed`].
    pub previous: Vec<KnownFile>,
}

impl CrawlSpec {
    /// A spec for `start` with the documented defaults: depth 1, same host, off-site files,
    /// 500 pages, 5 000 files, 5 MiB per page, 10 minutes, 4 requests in flight, 2 per host,
    /// 100 ms between requests to a host, `robots.txt` obeyed.
    pub fn new(start: impl Into<String>) -> CrawlSpec {
        CrawlSpec {
            start: start.into(),
            depth: 1,
            scope: Scope::SameHost,
            offsite_files: true,
            include: Vec::new(),
            exclude: Vec::new(),
            categories: Vec::new(),
            min_size: None,
            max_size: None,
            probe_files: false,
            resolve_unknown_links: true,
            max_pages: DEFAULT_MAX_PAGES,
            max_files: DEFAULT_MAX_FILES,
            max_page_bytes: DEFAULT_MAX_PAGE_BYTES,
            time_budget: Duration::from_secs(10 * 60),
            request_timeout: Duration::from_secs(30),
            concurrency: 4,
            per_host: 2,
            delay: Duration::from_millis(100),
            respect_robots: true,
            secrets: None,
            previous: Vec::new(),
        }
    }
}

/// A file the crawl found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundFile {
    /// Normalised URL (no fragment, no user info), to be handed to the engine as is.
    pub url: String,
    /// File name to save under: `Content-Disposition` if the file was probed, else the URL's
    /// last segment; already sanitised.
    pub guessed_name: String,
    /// By the name's extension (or the content type for names without one).
    pub category: Category,
    /// Bytes, when probed or announced by the server.
    pub size: Option<u64>,
    /// `ETag`, when probed.
    pub etag: Option<String>,
    /// `Last-Modified`, when probed.
    pub last_modified: Option<String>,
    /// The page the link was found on; becomes the download's `Referer`.
    pub referrer: String,
    /// Sync mode: `Some(true)` if the file differs from [`CrawlSpec::previous`], `Some(false)` if
    /// it matches, `None` if it was not in `previous` (or there was no previous result).
    pub changed: Option<bool>,
}

/// Why a link was not followed or a file not listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
pub enum SkipReason {
    /// `mailto:`, `javascript:`, `data:`, `ftp:`, ... only http(s) is followed.
    UnsupportedScheme,
    /// Outside the scope and not a file that may be listed.
    OutOfScope,
    /// An off-site page: never read.
    OffSite,
    /// More link hops than the depth allows.
    Depth,
    /// Matches an `exclude` pattern.
    Excluded,
    /// Matches no `include` pattern.
    NotIncluded,
    /// File type not selected.
    FileType,
    /// Outside the size window.
    Size,
    /// Disallowed by `robots.txt`.
    Robots,
    /// The page cap was reached.
    PageCap,
    /// The file cap was reached.
    FileCap,
    /// The HTML page is bigger than the page cap.
    PageTooLarge,
    /// The server answered this status.
    HttpStatus(u16),
    /// A network or protocol failure, or an unusable page.
    Error(String),
}

/// Counters of one crawl.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlStats {
    /// Pages fetched and read.
    pub pages_visited: u64,
    /// Files listed.
    pub files_found: u64,
    /// Links not followed or files not listed ([`CrawlEvent::Skipped`] events).
    pub skipped: u64,
    /// Links already seen (loops and repeats), not counted in `skipped`.
    pub duplicates: u64,
    /// Requests that failed or were answered with an error status (a subset of `skipped`).
    pub errors: u64,
    /// HTML bytes read.
    pub bytes_read: u64,
    /// Wall-clock milliseconds.
    pub elapsed_ms: u64,
}

/// Why the crawl ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    /// Everything reachable within the limits was visited.
    Finished,
    /// The caller cancelled.
    Cancelled,
    /// The time budget ran out.
    TimeBudget,
}

/// One page the crawl read, for the preview tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisitedPage {
    /// Normalised URL as requested.
    pub url: String,
    /// Link hops from the start page.
    pub depth: u32,
    /// The page that linked here (`None` for the start page).
    pub parent: Option<String>,
}

/// A progress event. The last one is always [`CrawlEvent::Done`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CrawlEvent {
    /// A page was fetched and its links read.
    PageVisited {
        /// The page.
        url: String,
        /// Link hops from the start page.
        depth: u32,
        /// Links found on it.
        links: usize,
    },
    /// A file was found.
    FileFound(FoundFile),
    /// A link was not followed or a file not listed.
    Skipped {
        /// The link.
        url: String,
        /// Why.
        reason: SkipReason,
    },
    /// The crawl ended.
    Done {
        /// Final counters.
        stats: CrawlStats,
    },
}

/// What a crawl produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlResult {
    /// The normalised start URL.
    pub start: String,
    /// Every file found, in discovery order, de-duplicated.
    pub files: Vec<FoundFile>,
    /// Every page read (the preview tree is `parent` links).
    pub pages: Vec<VisitedPage>,
    /// Counters.
    pub stats: CrawlStats,
    /// Why it ended.
    pub stop: StopReason,
    /// The page cap cut the crawl short.
    pub page_cap_hit: bool,
    /// The file cap cut the listing short.
    pub file_cap_hit: bool,
}

impl CrawlResult {
    /// One [`DownloadRequest`] per file (optionally only those in `only`, by URL), ready for
    /// the engine's `add`: the guessed name, the page as `Referer`, and `dir` if given.
    /// `secrets` (the crawl's own) are attached only to files on the **start origin**, so a
    /// cookie that opened the site never travels to a CDN.
    #[must_use]
    pub fn download_requests(
        &self,
        secrets: Option<&Secrets>,
        dir: Option<&Path>,
    ) -> Vec<DownloadRequest> {
        let start_origin = Url::parse(&self.start).ok().map(|u| Origin::of(&u));
        self.files
            .iter()
            .map(|f| {
                let mut req = DownloadRequest::new(f.url.clone());
                req.filename = Some(f.guessed_name.clone());
                req.dir = dir.map(Path::to_path_buf);
                req.referrer = Some(f.referrer.clone()).filter(|r| !r.is_empty());
                let same_origin = Url::parse(&f.url)
                    .ok()
                    .is_some_and(|u| Some(Origin::of(&u)) == start_origin);
                if same_origin {
                    req.secrets = secrets.filter(|s| !s.is_empty()).cloned();
                }
                req
            })
            .collect()
    }

    /// The files as sync-mode input for the next crawl.
    #[must_use]
    pub fn known_files(&self) -> Vec<KnownFile> {
        self.files.iter().map(KnownFile::from).collect()
    }
}

/// Why a crawl could not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The start URL is not a usable http(s) URL.
    #[error("invalid start URL: {0}")]
    InvalidStart(String),
    /// A glob or limit is unusable.
    #[error("invalid crawl setting: {0}")]
    InvalidSpec(String),
}
