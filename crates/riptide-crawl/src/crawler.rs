//! The crawl loop. One coordinator owns all mutable state (the seen set, the frontier, the
//! counters, the result); the requests run as futures polled by that same task, so there are no
//! locks around crawl state and cancelling is dropping the in-flight futures.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_DISPOSITION, CONTENT_ENCODING};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, LAST_MODIFIED, REFERER};
use reqwest::Method;
use riptide_core::{sanitize_filename, Category};
use riptide_net::http::send;
use riptide_net::probe::filename_from_url;
use riptide_net::redirect::strip_query;
use riptide_net::{parse_http_url, NetGuard, Origin, RequestContext};
use tokio::sync::{mpsc, OnceCell};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::gate::HostGate;
use crate::glob::Glob;
use crate::html::{extract_links, Link, LinkKind};
use crate::robots::{Robots, AGENT, MAX_ROBOTS_BYTES};
use crate::scope::normalize_url;
use crate::types::{
    CrawlEvent, CrawlResult, CrawlSpec, CrawlStats, Error, FoundFile, KnownFile, SkipReason,
    StopReason, VisitedPage, MAX_LINKS_PER_PAGE,
};

/// Crawls `spec.start` and returns what it found; see the crate docs for the rules.
///
/// Progress goes to `events` (when given) as it happens, ending with [`CrawlEvent::Done`]; a
/// slow receiver slows the crawl down rather than losing events, and a dropped one is ignored.
/// `cancel` stops the crawl promptly: requests in flight are abandoned and the partial result is
/// returned with [`StopReason::Cancelled`]. `client` is the shared client built by
/// [`riptide_net::build_client`] (no automatic redirects, `Accept-Encoding: identity`).
///
/// # Errors
/// Only for a start URL that is not http(s) or a spec that cannot work; everything that goes
/// wrong during the crawl is a [`CrawlEvent::Skipped`] instead.
///
/// A crawl that starts on a public host never fetches (or lists) links that point to loopback,
/// private, link-local or otherwise special addresses, nor follows redirects there: a page must
/// not be able to aim this machine at its own network. A crawl that starts on a private
/// address (own LAN, `localhost`) may go to private addresses. See
/// [`riptide_net::net_guard`]; [`crawl_with_guard`] takes the guard explicitly.
pub async fn crawl(
    spec: CrawlSpec,
    client: reqwest::Client,
    events: Option<mpsc::Sender<CrawlEvent>>,
    cancel: CancellationToken,
) -> Result<CrawlResult, Error> {
    let start = parse_http_url(&spec.start).map_err(|e| Error::InvalidStart(e.to_string()))?;
    let guard = NetGuard::for_start(&start, None).await;
    crawl_with_guard(spec, client, events, cancel, guard).await
}

/// [`crawl`] with an explicit [`NetGuard`] (the engine passes a client whose DNS resolver is the
/// guard's, so the checked address is the one connected to; tests inject a resolver).
///
/// # Errors
/// As [`crawl`].
pub async fn crawl_with_guard(
    spec: CrawlSpec,
    client: reqwest::Client,
    events: Option<mpsc::Sender<CrawlEvent>>,
    cancel: CancellationToken,
    guard: NetGuard,
) -> Result<CrawlResult, Error> {
    let start = parse_http_url(&spec.start).map_err(|e| Error::InvalidStart(e.to_string()))?;
    let start = normalize_url(&start).ok_or_else(|| Error::InvalidStart(spec.start.clone()))?;
    if spec.concurrency == 0 || spec.per_host == 0 || spec.max_pages == 0 {
        return Err(Error::InvalidSpec(
            "concurrency, per_host and max_pages must be at least 1".into(),
        ));
    }
    let shared = Shared::new(spec, start.clone(), client, guard);
    Ok(run(&shared, start, events, cancel).await)
}

// ---------------------------------------------------------------------------------------------
// Shared, read-mostly state used by the request futures
// ---------------------------------------------------------------------------------------------

struct Shared {
    spec: CrawlSpec,
    start: Url,
    client: reqwest::Client,
    /// Private-address policy for every request of this crawl.
    guard: NetGuard,
    /// Carries the secrets; applies them to the start origin only.
    ctx: RequestContext,
    include: Vec<Glob>,
    exclude: Vec<Glob>,
    previous: HashMap<String, KnownFile>,
    probe_files: bool,
    gates: Mutex<HashMap<Origin, Arc<HostGate>>>,
    robots: Mutex<HashMap<Origin, Arc<OnceCell<Arc<Robots>>>>>,
}

impl Shared {
    fn new(spec: CrawlSpec, start: Url, client: reqwest::Client, guard: NetGuard) -> Shared {
        let mut ctx = RequestContext::new(&start).with_guard(guard.clone());
        if let Some(secrets) = &spec.secrets {
            ctx = ctx.with_secrets(secrets);
        }
        Shared {
            include: spec.include.iter().map(|p| Glob::new(p)).collect(),
            exclude: spec.exclude.iter().map(|p| Glob::new(p)).collect(),
            previous: spec
                .previous
                .iter()
                .map(|k| (k.url.clone(), k.clone()))
                .collect(),
            probe_files: spec.probe_files
                || spec.min_size.is_some()
                || spec.max_size.is_some()
                || !spec.previous.is_empty(),
            gates: Mutex::new(HashMap::new()),
            robots: Mutex::new(HashMap::new()),
            spec,
            start,
            client,
            guard,
            ctx,
        }
    }

    fn gate(&self, url: &Url) -> Arc<HostGate> {
        let mut gates = self.gates.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(
            gates
                .entry(Origin::of(url))
                .or_insert_with(|| Arc::new(HostGate::new(self.spec.per_host, self.spec.delay))),
        )
    }

    /// Whether `robots.txt` lets Riptide fetch `url` (always true when robots are off).
    async fn allowed(&self, url: &Url) -> bool {
        if !self.spec.respect_robots {
            return true;
        }
        let cell = {
            let mut map = self.robots.lock().unwrap_or_else(|p| p.into_inner());
            Arc::clone(map.entry(Origin::of(url)).or_default())
        };
        let robots = cell
            .get_or_init(|| async { Arc::new(self.fetch_robots(url).await) })
            .await;
        let mut target = url.path().to_string();
        if let Some(q) = url.query() {
            target.push('?');
            target.push_str(q);
        }
        robots.is_allowed(&target)
    }

    async fn fetch_robots(&self, any_url_of_origin: &Url) -> Robots {
        let Ok(robots_url) = any_url_of_origin.join("/robots.txt") else {
            return Robots::deny_all();
        };
        let gate = self.gate(&robots_url);
        // no secrets for robots.txt: it is public by definition
        let ctx = RequestContext::new(&robots_url).with_guard(self.guard.clone());
        let fetched = tokio::time::timeout(self.spec.request_timeout, async {
            let _slot = gate.enter().await;
            let mut sent = send(&self.client, &ctx, Method::GET, robots_url.clone(), |r| r)
                .await
                .map_err(|e| e.to_string())?;
            let status = sent.response.status().as_u16();
            let mut body = Vec::new();
            if (200..300).contains(&status) {
                while let Some(chunk) = sent.response.chunk().await.map_err(|e| e.to_string())? {
                    body.extend_from_slice(&chunk);
                    if body.len() >= MAX_ROBOTS_BYTES {
                        body.truncate(MAX_ROBOTS_BYTES);
                        break;
                    }
                }
            }
            Ok::<_, String>((status, body))
        })
        .await;
        let robots = match fetched {
            Ok(Ok((status, body))) if (200..300).contains(&status) => {
                Robots::parse(&String::from_utf8_lossy(&body), AGENT)
            }
            // no robots.txt (or one we may not read): no rules
            Ok(Ok((status, _))) if (400..500).contains(&status) => Robots::allow_all(),
            // unreachable, 5xx, redirect loop: assume everything is off limits (RFC 9309)
            _ => Robots::deny_all(),
        };
        if let Some(delay) = robots.crawl_delay() {
            gate.raise_delay(delay);
        }
        tracing::debug!(
            origin = %strip_query(robots_url.as_str()),
            crawl_delay = ?robots.crawl_delay(),
            "robots.txt loaded"
        );
        robots
    }

    /// Runs one unit of work to its outcome. Never fails: a failure is an outcome.
    async fn run_work(&self, work: Work) -> Outcome {
        // before anything is requested (robots.txt included): a link to a private address
        // found by a crawl that started on a public host is neither fetched nor listed
        if let Err(e) = self.guard.check(&work.url).await {
            return Outcome::Skipped {
                work,
                reason: skip_for_net_error(e),
            };
        }
        if !self.allowed(&work.url).await {
            return Outcome::Skipped {
                work,
                reason: SkipReason::Robots,
            };
        }
        // waiting for the host's turn is not part of the request timeout; a file link that is
        // only listed (no probe) makes no request and needs no turn
        let _slot = if matches!(work.kind, WorkKind::File) && !self.probe_files {
            None
        } else {
            Some(self.gate(&work.url).enter().await)
        };
        let result = tokio::time::timeout(self.spec.request_timeout, async {
            match work.kind {
                WorkKind::File => self.file_work(&work).await,
                WorkKind::Page | WorkKind::Resolve(_) => self.page_work(&work).await,
            }
        })
        .await;
        match result {
            Ok(Ok(done)) => done.into_outcome(work),
            Ok(Err(reason)) => Outcome::Skipped { work, reason },
            Err(_elapsed) => Outcome::Skipped {
                work,
                reason: SkipReason::Error("timed out".into()),
            },
        }
    }

    /// A file link: nothing to fetch unless sizes / validators are wanted.
    async fn file_work(&self, work: &Work) -> Result<Done, SkipReason> {
        let referrer = work
            .parent
            .as_ref()
            .map_or_else(String::new, Url::to_string);
        let key = work.url.as_str();
        if !self.probe_files {
            let name = sanitize_filename(&filename_from_url(&work.url));
            return Ok(Done::File(FoundFile {
                url: key.to_string(),
                category: Category::from_filename(&name),
                guessed_name: name,
                size: None,
                etag: None,
                last_modified: None,
                referrer,
                changed: None,
            }));
        }
        let probe = riptide_net::probe(&self.client, key, &self.ctx)
            .await
            .map_err(skip_for_net_error)?;
        let mut category = Category::from_filename(&probe.filename);
        if category == Category::Other {
            category = category_from_mime(probe.content_type.as_deref());
        }
        let changed = self.changed(key, probe.etag.as_deref(), probe.last_modified.as_deref());
        Ok(Done::File(FoundFile {
            url: key.to_string(),
            guessed_name: probe.filename,
            category,
            size: probe.size,
            etag: probe.etag,
            last_modified: probe.last_modified,
            referrer,
            changed,
        }))
    }

    /// A link that may be a page: `GET` it; HTML is read (when `Page`), anything else is a file.
    async fn page_work(&self, work: &Work) -> Result<Done, SkipReason> {
        let referer = referer_for(work.parent.as_ref(), &work.url);
        let mut sent = send(
            &self.client,
            &self.ctx,
            Method::GET,
            work.url.clone(),
            |r| {
                let r = r.header(ACCEPT, "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5");
                match &referer {
                    Some(v) => r.header(REFERER, v.clone()),
                    None => r,
                }
            },
        )
        .await
        .map_err(skip_for_net_error)?;
        if !sent.response.status().is_success() {
            return Err(SkipReason::HttpStatus(sent.response.status().as_u16()));
        }
        let headers = sent.response.headers().clone();
        let final_url = normalize_url(&sent.url).unwrap_or_else(|| sent.url.clone());
        let mime = headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|t| {
                t.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            });
        let is_html = mime
            .as_deref()
            .is_none_or(|m| m.is_empty() || m == "text/html" || m == "application/xhtml+xml");

        if !is_html {
            // it is a file after all (`/download?id=3`)
            let (name, category) = file_name_and_category(&headers, &final_url, mime.as_deref());
            let size = header_u64(&headers, CONTENT_LENGTH);
            let (etag, last_modified) = (
                header_string(&headers, ETAG),
                header_string(&headers, LAST_MODIFIED),
            );
            let changed =
                self.changed(work.url.as_str(), etag.as_deref(), last_modified.as_deref());
            return Ok(Done::File(FoundFile {
                url: work.url.to_string(),
                guessed_name: name,
                category,
                size,
                etag,
                last_modified,
                referrer: work
                    .parent
                    .as_ref()
                    .map_or_else(String::new, Url::to_string),
                changed,
            }));
        }
        if let WorkKind::Resolve(reason) = &work.kind {
            // an HTML page we only wanted to classify: drop the body unread
            return Err(reason.clone());
        }
        if headers
            .get(CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|e| !e.trim().eq_ignore_ascii_case("identity"))
        {
            return Err(SkipReason::Error("unsupported Content-Encoding".into()));
        }
        let cap = self.spec.max_page_bytes;
        if header_u64(&headers, CONTENT_LENGTH).is_some_and(|len| len > cap) {
            return Err(SkipReason::PageTooLarge);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = sent
            .response
            .chunk()
            .await
            .map_err(|e| SkipReason::Error(redacted(&e.to_string())))?
        {
            if body.len() as u64 + chunk.len() as u64 > cap {
                return Err(SkipReason::PageTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let bytes = body.len() as u64;
        let page_url = final_url.clone();
        let links = tokio::task::spawn_blocking(move || {
            let text = String::from_utf8_lossy(&body);
            extract_links(&text, &page_url, MAX_LINKS_PER_PAGE)
        })
        .await
        .map_err(|e| SkipReason::Error(format!("link extraction failed: {e}")))?;
        Ok(Done::Page {
            final_url,
            links,
            bytes,
        })
    }

    fn changed(&self, url: &str, etag: Option<&str>, last_modified: Option<&str>) -> Option<bool> {
        let prev = self.previous.get(url)?;
        Some(
            match (&prev.etag, etag, &prev.last_modified, last_modified) {
                (Some(a), Some(b), _, _) => a != b,
                (_, _, Some(a), Some(b)) => a != b,
                // no validator to compare: assume it changed
                _ => true,
            },
        )
    }

    /// Why `file` must not be listed, if so.
    fn reject_file(&self, file: &FoundFile, url: &Url) -> Option<SkipReason> {
        if !self.include.is_empty() && !self.include.iter().any(|g| g.matches_url(url)) {
            return Some(SkipReason::NotIncluded);
        }
        if !self.spec.categories.is_empty() && !self.spec.categories.contains(&file.category) {
            return Some(SkipReason::FileType);
        }
        if let Some(size) = file.size {
            if self.spec.min_size.is_some_and(|min| size < min)
                || self.spec.max_size.is_some_and(|max| size > max)
            {
                return Some(SkipReason::Size);
            }
        }
        None
    }
}

fn skip_for_net_error(e: riptide_net::Error) -> SkipReason {
    match e {
        riptide_net::Error::Status { status, .. } => SkipReason::HttpStatus(status),
        other => SkipReason::Error(redacted(&other.to_string())),
    }
}

/// Error texts can embed URLs with tokens in their query strings.
fn redacted(text: &str) -> String {
    text.split_whitespace()
        .map(|w| {
            if w.contains("://") {
                strip_query(w)
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn header_string(h: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
}

fn header_u64(h: &HeaderMap, name: reqwest::header::HeaderName) -> Option<u64> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
}

fn file_name_and_category(
    headers: &HeaderMap,
    url: &Url,
    mime: Option<&str>,
) -> (String, Category) {
    let raw = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
        .and_then(riptide_net::disposition::filename_from_disposition)
        .unwrap_or_else(|| filename_from_url(url));
    let name = sanitize_filename(&raw);
    let mut category = Category::from_filename(&name);
    if category == Category::Other {
        category = category_from_mime(mime);
    }
    (name, category)
}

fn category_from_mime(mime: Option<&str>) -> Category {
    match mime.unwrap_or("") {
        m if m.starts_with("video/") => Category::Video,
        m if m.starts_with("audio/") => Category::Music,
        m if m.starts_with("image/") => Category::Images,
        "application/pdf" | "text/plain" | "text/csv" | "application/msword" => Category::Documents,
        "application/zip" | "application/x-7z-compressed" | "application/gzip" => {
            Category::Compressed
        }
        _ => Category::Other,
    }
}

/// `Referer` for a request found on `parent`: the full page on the same origin, only the origin
/// (like a browser's `strict-origin-when-cross-origin`) elsewhere.
fn referer_for(parent: Option<&Url>, target: &Url) -> Option<HeaderValue> {
    let parent = parent?;
    let value = if Origin::of(parent) == Origin::of(target) {
        parent.as_str().to_string()
    } else {
        format!("{}/", parent.origin().ascii_serialization())
    };
    HeaderValue::from_str(&value).ok()
}

// ---------------------------------------------------------------------------------------------
// Work and outcomes
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum WorkKind {
    /// Fetch, read, follow its links.
    Page,
    /// A link of unknown type that may not be read as a page (off-site, or beyond the depth):
    /// fetch only to see whether it is a file; HTML is dropped with this reason.
    Resolve(SkipReason),
    /// A file link.
    File,
}

#[derive(Debug, Clone)]
struct Work {
    url: Url,
    depth: u32,
    parent: Option<Url>,
    kind: WorkKind,
}

enum Done {
    Page {
        final_url: Url,
        links: Vec<Link>,
        bytes: u64,
    },
    File(FoundFile),
}

impl Done {
    fn into_outcome(self, work: Work) -> Outcome {
        match self {
            Done::Page {
                final_url,
                links,
                bytes,
            } => Outcome::Page {
                work,
                final_url,
                links,
                bytes,
            },
            Done::File(file) => Outcome::File { work, file },
        }
    }
}

enum Outcome {
    Page {
        work: Work,
        final_url: Url,
        links: Vec<Link>,
        bytes: u64,
    },
    File {
        work: Work,
        file: FoundFile,
    },
    Skipped {
        work: Work,
        reason: SkipReason,
    },
}

// ---------------------------------------------------------------------------------------------
// The coordinator
// ---------------------------------------------------------------------------------------------

struct State {
    seen: HashSet<String>,
    frontier: VecDeque<Work>,
    pages_reserved: usize,
    files_reserved: usize,
    stats: CrawlStats,
    files: Vec<FoundFile>,
    pages: Vec<VisitedPage>,
    page_cap_hit: bool,
    file_cap_hit: bool,
    events: Option<mpsc::Sender<CrawlEvent>>,
    cancel: CancellationToken,
}

impl State {
    async fn emit(&self, event: CrawlEvent) {
        if let Some(tx) = &self.events {
            tokio::select! {
                _ = tx.send(event) => {}
                () = self.cancel.cancelled() => {}
            }
        }
    }

    async fn skip(&mut self, url: &Url, reason: SkipReason) {
        self.stats.skipped += 1;
        if matches!(reason, SkipReason::HttpStatus(_) | SkipReason::Error(_)) {
            self.stats.errors += 1;
        }
        tracing::debug!(url = %strip_query(url.as_str()), ?reason, "crawl: skipped");
        self.emit(CrawlEvent::Skipped {
            url: url.to_string(),
            reason,
        })
        .await;
    }
}

async fn run(
    shared: &Shared,
    start: Url,
    events: Option<mpsc::Sender<CrawlEvent>>,
    cancel: CancellationToken,
) -> CrawlResult {
    let started = Instant::now();
    let deadline = started + shared.spec.time_budget;
    let mut st = State {
        seen: HashSet::new(),
        frontier: VecDeque::new(),
        pages_reserved: 1,
        files_reserved: 0,
        stats: CrawlStats::default(),
        files: Vec::new(),
        pages: Vec::new(),
        page_cap_hit: false,
        file_cap_hit: false,
        events,
        cancel: cancel.clone(),
    };
    st.seen.insert(start.as_str().to_owned());
    st.frontier.push_back(Work {
        url: start,
        depth: 0,
        parent: None,
        kind: WorkKind::Page,
    });

    let mut inflight: FuturesUnordered<BoxFuture<'_, Outcome>> = FuturesUnordered::new();
    let stop = loop {
        while inflight.len() < shared.spec.concurrency {
            let Some(work) = st.frontier.pop_front() else {
                break;
            };
            inflight.push(Box::pin(shared.run_work(work)));
        }
        if inflight.is_empty() {
            break StopReason::Finished;
        }
        tokio::select! {
            biased;
            () = cancel.cancelled() => break StopReason::Cancelled,
            () = tokio::time::sleep_until(deadline) => break StopReason::TimeBudget,
            Some(outcome) = inflight.next() => handle(shared, &mut st, outcome).await,
        }
    };
    // abandon whatever was still in flight
    drop(inflight);

    st.stats.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if let Some(tx) = &st.events {
        let done = CrawlEvent::Done {
            stats: st.stats.clone(),
        };
        // the receiver may be busy or gone; never hang on the last event
        if let Err(mpsc::error::TrySendError::Full(done)) = tx.try_send(done) {
            let _ = tokio::time::timeout(Duration::from_secs(1), tx.send(done)).await;
        }
    }
    CrawlResult {
        start: shared.start.to_string(),
        files: st.files,
        pages: st.pages,
        stats: st.stats,
        stop,
        page_cap_hit: st.page_cap_hit,
        file_cap_hit: st.file_cap_hit,
    }
}

async fn handle(shared: &Shared, st: &mut State, outcome: Outcome) {
    match outcome {
        Outcome::Skipped { work, reason } => {
            if matches!(work.kind, WorkKind::File) {
                st.files_reserved = st.files_reserved.saturating_sub(1);
            }
            st.skip(&work.url, reason).await;
        }
        Outcome::File { work, file } => {
            let counted_already = matches!(work.kind, WorkKind::File);
            let url = Url::parse(&file.url).unwrap_or_else(|_| work.url.clone());
            if let Some(reason) = shared.reject_file(&file, &url) {
                if counted_already {
                    st.files_reserved = st.files_reserved.saturating_sub(1);
                }
                st.skip(&work.url, reason).await;
                return;
            }
            if !counted_already {
                // a link that turned out to be a file: it takes a file slot only now
                if st.files_reserved >= shared.spec.max_files {
                    st.file_cap_hit = true;
                    st.skip(&work.url, SkipReason::FileCap).await;
                    return;
                }
                st.files_reserved += 1;
            }
            st.stats.files_found += 1;
            st.files.push(file.clone());
            st.emit(CrawlEvent::FileFound(file)).await;
        }
        Outcome::Page {
            work,
            final_url,
            links,
            bytes,
        } => {
            if final_url != work.url {
                // redirected: the target counts once, and must still be in scope
                if !st.seen.insert(final_url.as_str().to_owned()) {
                    st.stats.duplicates += 1;
                    return;
                }
                if work.depth > 0 && !shared.spec.scope.contains(&shared.start, &final_url) {
                    st.skip(&final_url, SkipReason::OutOfScope).await;
                    return;
                }
            }
            st.stats.pages_visited += 1;
            st.stats.bytes_read += bytes;
            st.pages.push(VisitedPage {
                url: work.url.to_string(),
                depth: work.depth,
                parent: work.parent.as_ref().map(Url::to_string),
            });
            st.emit(CrawlEvent::PageVisited {
                url: work.url.to_string(),
                depth: work.depth,
                links: links.len(),
            })
            .await;
            for link in links {
                consider(shared, st, &final_url, work.depth, link).await;
            }
        }
    }
}

/// Decides what to do with one link found on `page` (at `page_depth`).
async fn consider(shared: &Shared, st: &mut State, page: &Url, page_depth: u32, link: Link) {
    let spec = &shared.spec;
    let Some(url) = normalize_url(&link.url) else {
        st.skip(&link.url, SkipReason::UnsupportedScheme).await;
        return;
    };
    if !st.seen.insert(url.as_str().to_owned()) {
        st.stats.duplicates += 1;
        return;
    }
    if shared.exclude.iter().any(|g| g.matches_url(&url)) {
        st.skip(&url, SkipReason::Excluded).await;
        return;
    }
    let in_scope = spec.scope.contains(&shared.start, &url);
    let child_depth = page_depth + 1;
    let work = |kind: WorkKind, depth: u32| Work {
        url: url.clone(),
        depth,
        parent: Some(page.clone()),
        kind,
    };

    match link.kind {
        LinkKind::File => {
            if !in_scope && !spec.offsite_files {
                st.skip(&url, SkipReason::OutOfScope).await;
                return;
            }
            if !shared.include.is_empty() && !shared.include.iter().any(|g| g.matches_url(&url)) {
                st.skip(&url, SkipReason::NotIncluded).await;
                return;
            }
            let guess = Category::from_filename(&filename_from_url(&url));
            if !spec.categories.is_empty() && !spec.categories.contains(&guess) {
                // the real name (Content-Disposition) could still match, but only a probe
                // would know; without probing the extension is what the user selected on
                if !shared.probe_files {
                    st.skip(&url, SkipReason::FileType).await;
                    return;
                }
            }
            if st.files_reserved >= spec.max_files {
                st.file_cap_hit = true;
                st.skip(&url, SkipReason::FileCap).await;
                return;
            }
            st.files_reserved += 1;
            st.frontier.push_back(work(WorkKind::File, child_depth));
        }
        LinkKind::Page | LinkKind::Maybe => {
            let maybe = link.kind == LinkKind::Maybe;
            let kind = if in_scope && child_depth <= spec.depth {
                WorkKind::Page
            } else if !in_scope && !spec.offsite_files {
                st.skip(&url, SkipReason::OutOfScope).await;
                return;
            } else if maybe && spec.resolve_unknown_links {
                WorkKind::Resolve(if in_scope {
                    SkipReason::Depth
                } else {
                    SkipReason::OffSite
                })
            } else {
                let reason = if in_scope {
                    SkipReason::Depth
                } else {
                    SkipReason::OffSite
                };
                st.skip(&url, reason).await;
                return;
            };
            if st.pages_reserved >= spec.max_pages {
                st.page_cap_hit = true;
                st.skip(&url, SkipReason::PageCap).await;
                return;
            }
            st.pages_reserved += 1;
            st.frontier.push_back(work(kind, child_depth));
        }
    }
}
