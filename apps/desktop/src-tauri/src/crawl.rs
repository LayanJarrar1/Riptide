//! Site grabber commands: crawl a site for files, stream what is found, add the chosen files.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `start_crawl` | `spec: GrabSpec` | crawl id (the crawl runs in the background) |
//! | `cancel_crawl` | `id` | – (stops it; the files found so far stay addable) |
//! | `discard_crawl` | `id` | – (stops it and forgets its result) |
//! | `add_crawl_files` | `id`, `urls`, `options: GrabAddOptions` | `BatchOutcome` |
//!
//! Progress reaches the webview on the `riptide://crawl` event as [`CrawlBatch`]es: events are
//! gathered for about 100 ms so a fast crawl does not flood the bridge, and the last batch of a
//! crawl carries `end`. The webview never supplies file names, referrers or URLs to download:
//! `add_crawl_files` takes the URLs it ticked and looks each one up in the crawl's own result.
//! The crawl runs through the engine's HTTP client, obeys `robots.txt` (this spec cannot switch
//! that off) and downloads nothing by itself.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use riptide_core::{Category, DownloadRequest, QueueId};
use riptide_crawl::{
    CrawlEvent, CrawlResult, CrawlSpec, Scope, StopReason, DEFAULT_MAX_FILES, DEFAULT_MAX_PAGES,
};
use riptide_engine::{BatchOutcome, Engine};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Webview event carrying [`CrawlBatch`]es.
pub const CRAWL_EVENT: &str = "riptide://crawl";

/// Deepest link depth the dialog may ask for.
pub const MAX_DEPTH: u32 = 5;
/// Most include or exclude patterns in one spec.
const MAX_PATTERNS: usize = 32;
/// Longest single pattern.
const MAX_PATTERN_LEN: usize = 256;
/// Crawl results kept for "add"; the oldest is forgotten past this.
const KEEP_RESULTS: usize = 4;
/// How long events are gathered before they are sent to the webview.
const BATCH_WINDOW: Duration = Duration::from_millis(100);
/// Most events in one batch.
const BATCH_MAX: usize = 256;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// What the site grabber dialog asks for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabSpec {
    /// Start page (http or https).
    pub url: String,
    /// Link depth, `0..=`[`MAX_DEPTH`].
    pub depth: u32,
    /// Which pages are walked into.
    pub scope: Scope,
    /// List files linked from in-scope pages even when they live on another site.
    #[serde(default)]
    pub offsite_files: bool,
    /// File types to list; empty = all.
    #[serde(default)]
    pub categories: Vec<Category>,
    /// Only files matching one of these globs.
    #[serde(default)]
    pub include: Vec<String>,
    /// Skip pages and files matching any of these globs.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Smallest file, bytes.
    #[serde(default)]
    pub min_size: Option<u64>,
    /// Largest file, bytes.
    #[serde(default)]
    pub max_size: Option<u64>,
    /// Pages fetched at most (default and ceiling: the crate's cap).
    #[serde(default)]
    pub max_pages: Option<usize>,
    /// Files listed at most (default and ceiling: the crate's cap).
    #[serde(default)]
    pub max_files: Option<usize>,
}

impl GrabSpec {
    /// Checks the spec and turns it into the crawler's. `robots.txt` is always obeyed and no
    /// secrets are attached: the dialog has neither switch.
    pub fn validate(&self) -> Result<CrawlSpec, String> {
        let url = self.url.trim();
        if !riptide_core::batch::is_http_url(url) {
            return Err("the start address must be an http or https link".into());
        }
        if url::Url::parse(url).is_err() {
            return Err("the start address is not a valid link".into());
        }
        if self.depth > MAX_DEPTH {
            return Err(format!("the depth is at most {MAX_DEPTH}"));
        }
        for list in [&self.include, &self.exclude] {
            if list.len() > MAX_PATTERNS {
                return Err(format!("at most {MAX_PATTERNS} patterns"));
            }
            if list.iter().any(|p| p.len() > MAX_PATTERN_LEN) {
                return Err(format!("a pattern is at most {MAX_PATTERN_LEN} characters"));
            }
        }
        if let (Some(min), Some(max)) = (self.min_size, self.max_size) {
            if min > max {
                return Err("the smallest size is above the largest".into());
            }
        }
        let max_pages = self.max_pages.unwrap_or(DEFAULT_MAX_PAGES);
        let max_files = self.max_files.unwrap_or(DEFAULT_MAX_FILES);
        if !(1..=DEFAULT_MAX_PAGES).contains(&max_pages) {
            return Err(format!("pages must be between 1 and {DEFAULT_MAX_PAGES}"));
        }
        if !(1..=DEFAULT_MAX_FILES).contains(&max_files) {
            return Err(format!("files must be between 1 and {DEFAULT_MAX_FILES}"));
        }
        let clean = |list: &[String]| -> Vec<String> {
            list.iter()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect()
        };
        let mut spec = CrawlSpec::new(url);
        spec.depth = self.depth;
        spec.scope = self.scope;
        spec.offsite_files = self.offsite_files;
        spec.categories = self.categories.clone();
        spec.include = clean(&self.include);
        spec.exclude = clean(&self.exclude);
        spec.min_size = self.min_size;
        spec.max_size = self.max_size;
        spec.max_pages = max_pages;
        spec.max_files = max_files;
        spec.respect_robots = true;
        spec.secrets = None;
        Ok(spec)
    }
}

/// Where the chosen files go.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabAddOptions {
    /// Destination folder; `None` = by category.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Queue; `None` = Main.
    #[serde(default)]
    pub queue_id: Option<i64>,
    /// Add paused ("start later").
    #[serde(default)]
    pub start_paused: bool,
}

/// How a crawl ended, sent with its last batch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlEnd {
    /// Why it stopped (`null` when it failed).
    pub stop: Option<StopReason>,
    /// The page cap cut it short.
    pub page_cap_hit: bool,
    /// The file cap cut it short.
    pub file_cap_hit: bool,
    /// The reason it could not run, if it failed.
    pub error: Option<String>,
}

/// One payload of the `riptide://crawl` event.
#[derive(Debug, Clone, Serialize)]
pub struct CrawlBatch {
    /// The crawl these events belong to.
    pub id: u64,
    /// Events in order. `Done` is sent last, only after the result can be added.
    pub events: Vec<CrawlEvent>,
    /// Present on the last batch.
    pub end: Option<CrawlEnd>,
}

struct Entry {
    cancel: CancellationToken,
    result: Option<CrawlResult>,
}

/// Running and finished crawls, by id.
#[derive(Default)]
pub struct Crawls {
    next: AtomicU64,
    entries: Mutex<HashMap<u64, Entry>>,
}

impl Crawls {
    fn insert(&self) -> (u64, CancellationToken) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = CancellationToken::new();
        if let Ok(mut map) = self.entries.lock() {
            // forget the oldest finished results; a running crawl is never dropped
            while map.len() >= KEEP_RESULTS {
                let oldest = map
                    .iter()
                    .filter(|(_, e)| e.result.is_some())
                    .map(|(k, _)| *k)
                    .min();
                match oldest {
                    Some(k) => map.remove(&k),
                    None => break,
                };
            }
            map.insert(
                id,
                Entry {
                    cancel: cancel.clone(),
                    result: None,
                },
            );
        }
        (id, cancel)
    }

    fn store(&self, id: u64, result: CrawlResult) {
        if let Ok(mut map) = self.entries.lock() {
            if let Some(entry) = map.get_mut(&id) {
                entry.result = Some(result);
            }
        }
    }

    fn cancel(&self, id: u64) {
        if let Ok(map) = self.entries.lock() {
            if let Some(entry) = map.get(&id) {
                entry.cancel.cancel();
            }
        }
    }

    fn discard(&self, id: u64) {
        if let Ok(mut map) = self.entries.lock() {
            if let Some(entry) = map.remove(&id) {
                entry.cancel.cancel();
            }
        }
    }

    /// The crawl's files whose URL is in `urls`, as download requests.
    fn requests_for(
        &self,
        id: u64,
        urls: &[String],
        options: &GrabAddOptions,
    ) -> Result<Vec<DownloadRequest>, String> {
        let map = self.entries.lock().map_err(err)?;
        let result = map
            .get(&id)
            .and_then(|e| e.result.as_ref())
            .ok_or("this crawl is no longer available")?;
        Ok(requests_from(result, urls, options))
    }
}

/// Download requests for the files of `result` named in `urls` (in the crawl's order), with the
/// dialog's destination, queue and start mode applied.
fn requests_from(
    result: &CrawlResult,
    urls: &[String],
    options: &GrabAddOptions,
) -> Vec<DownloadRequest> {
    let wanted: HashSet<&str> = urls.iter().map(String::as_str).collect();
    let mut chosen = result.clone();
    chosen.files.retain(|f| wanted.contains(f.url.as_str()));
    let mut requests = chosen.download_requests(None, options.dir.as_deref());
    for r in &mut requests {
        r.queue_id = options.queue_id.map(QueueId);
        r.start_paused = options.start_paused;
    }
    requests
}

/// Gathers events for [`BATCH_WINDOW`] and emits them; `Done` is held back (the caller sends it
/// once the result is stored). Ends when the crawl drops its sender.
async fn forward(app: AppHandle, id: u64, mut rx: mpsc::Receiver<CrawlEvent>) {
    let mut closed = false;
    while !closed {
        let Some(first) = rx.recv().await else { break };
        let mut buf = vec![first];
        let window = tokio::time::sleep(BATCH_WINDOW);
        tokio::pin!(window);
        while buf.len() < BATCH_MAX {
            tokio::select! {
                biased;
                ev = rx.recv() => match ev {
                    Some(e) => buf.push(e),
                    None => { closed = true; break; }
                },
                () = &mut window => break,
            }
        }
        buf.retain(|e| !matches!(e, CrawlEvent::Done { .. }));
        if buf.is_empty() {
            continue;
        }
        emit(
            &app,
            &CrawlBatch {
                id,
                events: buf,
                end: None,
            },
        );
    }
}

fn emit(app: &AppHandle, batch: &CrawlBatch) {
    if let Err(e) = app.emit(CRAWL_EVENT, batch) {
        tracing::debug!(error = %e, "crawl event not delivered to the webview");
    }
}

/// Starts a crawl in the background; answers its id at once.
#[tauri::command]
pub async fn start_crawl(
    app: AppHandle,
    engine: State<'_, Engine>,
    crawls: State<'_, Arc<Crawls>>,
    spec: GrabSpec,
) -> CmdResult<u64> {
    let spec = spec.validate()?;
    let engine = engine.inner().clone();
    let crawls = Arc::clone(crawls.inner());
    let (id, cancel) = crawls.insert();
    tauri::async_runtime::spawn(async move {
        let (tx, rx) = mpsc::channel(512);
        let forwarder = tauri::async_runtime::spawn(forward(app.clone(), id, rx));
        let outcome = engine.crawl(spec, Some(tx), cancel).await;
        let _ = forwarder.await;
        let batch = match outcome {
            Ok(result) => {
                let done = CrawlEvent::Done {
                    stats: result.stats.clone(),
                };
                let end = CrawlEnd {
                    stop: Some(result.stop),
                    page_cap_hit: result.page_cap_hit,
                    file_cap_hit: result.file_cap_hit,
                    error: None,
                };
                crawls.store(id, result);
                CrawlBatch {
                    id,
                    events: vec![done],
                    end: Some(end),
                }
            }
            Err(e) => CrawlBatch {
                id,
                events: Vec::new(),
                end: Some(CrawlEnd {
                    stop: None,
                    page_cap_hit: false,
                    file_cap_hit: false,
                    error: Some(e.to_string()),
                }),
            },
        };
        emit(&app, &batch);
    });
    Ok(id)
}

/// Stops a running crawl; what it found so far can still be added.
#[tauri::command]
pub fn cancel_crawl(crawls: State<'_, Arc<Crawls>>, id: u64) {
    crawls.cancel(id);
}

/// Stops a crawl and forgets its result (the dialog closed).
#[tauri::command]
pub fn discard_crawl(crawls: State<'_, Arc<Crawls>>, id: u64) {
    crawls.discard(id);
}

/// Adds the ticked files of a finished or cancelled crawl in one transaction.
#[tauri::command]
pub async fn add_crawl_files(
    engine: State<'_, Engine>,
    crawls: State<'_, Arc<Crawls>>,
    id: u64,
    urls: Vec<String>,
    options: GrabAddOptions,
) -> CmdResult<BatchOutcome> {
    let requests = crawls.requests_for(id, &urls, &options)?;
    if requests.is_empty() {
        return Err("none of the chosen files belongs to this crawl".into());
    }
    let outcome = engine.add_batch(requests).await.map_err(err)?;
    if !outcome.added.is_empty() {
        crawls.discard(id);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riptide_crawl::{CrawlStats, FoundFile};

    fn spec(url: &str) -> GrabSpec {
        GrabSpec {
            url: url.into(),
            depth: 1,
            scope: Scope::SameHost,
            offsite_files: true,
            categories: vec![],
            include: vec![],
            exclude: vec![],
            min_size: None,
            max_size: None,
            max_pages: None,
            max_files: None,
        }
    }

    #[test]
    fn accepts_http_and_https_only() {
        assert!(spec("https://example.com/files/").validate().is_ok());
        assert!(spec("  http://example.com ").validate().is_ok());
        for bad in [
            "ftp://example.com/",
            "file:///C:/secret",
            "javascript:alert(1)",
            "example.com",
            "https://",
            "",
        ] {
            assert!(spec(bad).validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn depth_and_caps_stay_within_the_crates_limits() {
        let mut s = spec("https://example.com/");
        s.depth = MAX_DEPTH;
        assert!(s.validate().is_ok());
        s.depth = MAX_DEPTH + 1;
        assert!(s.validate().is_err());
        s.depth = 0;
        s.max_pages = Some(DEFAULT_MAX_PAGES + 1);
        assert!(s.validate().is_err());
        s.max_pages = Some(0);
        assert!(s.validate().is_err());
        s.max_pages = Some(10);
        s.max_files = Some(DEFAULT_MAX_FILES + 1);
        assert!(s.validate().is_err());
        s.max_files = Some(DEFAULT_MAX_FILES);
        let ok = s.validate().unwrap();
        assert_eq!((ok.max_pages, ok.max_files), (10, DEFAULT_MAX_FILES));
    }

    #[test]
    fn sizes_and_patterns_are_checked() {
        let mut s = spec("https://example.com/");
        s.min_size = Some(10);
        s.max_size = Some(5);
        assert!(s.validate().is_err());
        s.max_size = Some(10);
        s.include = vec!["  *.pdf ".into(), " ".into()];
        assert_eq!(s.validate().unwrap().include, ["*.pdf"]);
        s.exclude = vec!["x".repeat(MAX_PATTERN_LEN + 1)];
        assert!(s.validate().is_err());
        s.exclude = vec!["a".into(); MAX_PATTERNS + 1];
        assert!(s.validate().is_err());
    }

    #[test]
    fn robots_are_always_obeyed_and_no_secrets_ride_along() {
        let c = spec("https://example.com/").validate().unwrap();
        assert!(c.respect_robots);
        assert!(c.secrets.is_none());
    }

    #[test]
    fn spec_deserializes_from_the_webview_shape() {
        let s: GrabSpec = serde_json::from_str(
            r#"{"url":"https://a.example/","depth":2,"scope":"same-domain",
                "categories":["documents","images"],"minSize":1024}"#,
        )
        .unwrap();
        assert_eq!(s.scope, Scope::SameDomain);
        assert_eq!(s.categories, [Category::Documents, Category::Images]);
        assert_eq!(s.min_size, Some(1024));
    }

    fn found(url: &str, name: &str) -> FoundFile {
        FoundFile {
            url: url.into(),
            guessed_name: name.into(),
            category: Category::Documents,
            size: None,
            etag: None,
            last_modified: None,
            referrer: "https://a.example/".into(),
            changed: None,
        }
    }

    #[test]
    fn only_the_chosen_files_become_requests_with_the_dialogs_options() {
        let result = CrawlResult {
            start: "https://a.example/".into(),
            files: vec![
                found("https://a.example/1.pdf", "1.pdf"),
                found("https://a.example/2.pdf", "2.pdf"),
            ],
            pages: vec![],
            stats: CrawlStats::default(),
            stop: StopReason::Finished,
            page_cap_hit: false,
            file_cap_hit: false,
        };
        let options = GrabAddOptions {
            dir: Some(PathBuf::from("D:/dl")),
            queue_id: Some(3),
            start_paused: true,
        };
        // a URL the crawl never found is ignored: the webview cannot inject downloads
        let urls = vec![
            "https://a.example/2.pdf".to_owned(),
            "https://evil.example/x.exe".to_owned(),
        ];
        let reqs = requests_from(&result, &urls, &options);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "https://a.example/2.pdf");
        assert_eq!(reqs[0].filename.as_deref(), Some("2.pdf"));
        assert_eq!(reqs[0].queue_id, Some(QueueId(3)));
        assert!(reqs[0].start_paused);
        assert_eq!(reqs[0].dir, Some(PathBuf::from("D:/dl")));
        assert_eq!(reqs[0].referrer.as_deref(), Some("https://a.example/"));
    }
}
