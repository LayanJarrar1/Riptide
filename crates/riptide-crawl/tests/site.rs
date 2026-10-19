//! The crawler against `riptide-testserver`'s `/site/` (in-process, no network).
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use riptide_core::{Category, Credentials, Secrets};
use riptide_crawl::{
    crawl, CrawlEvent, CrawlResult, CrawlSpec, Error, Scope, SkipReason, StopReason,
};
use riptide_net::{build_client, ClientOptions};
use riptide_testserver::{RequestRecord, TestServer};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn client() -> reqwest::Client {
    build_client(&ClientOptions::default()).unwrap()
}

/// A spec for the test site: no delay between requests, everything else default.
fn spec(srv: &TestServer, depth: u32) -> CrawlSpec {
    let mut s = CrawlSpec::new(srv.url("/site/"));
    s.depth = depth;
    s.delay = Duration::ZERO;
    s
}

/// Runs a crawl, collecting every event.
async fn run(spec: CrawlSpec) -> (CrawlResult, Vec<CrawlEvent>) {
    run_with(spec, CancellationToken::new()).await
}

async fn run_with(spec: CrawlSpec, cancel: CancellationToken) -> (CrawlResult, Vec<CrawlEvent>) {
    let (tx, mut rx) = mpsc::channel(8);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        events
    });
    let result = crawl(spec, client(), Some(tx), cancel).await.unwrap();
    (result, collector.await.unwrap())
}

/// File URLs relative to the server's origins, sorted: `/site/files/report.pdf`, or
/// `ALT/site/offsite/file.zip` for the other origin.
fn paths(srv: &TestServer, result: &CrawlResult) -> BTreeSet<String> {
    result
        .files
        .iter()
        .map(|f| {
            f.url
                .strip_prefix(srv.base_url())
                .map(str::to_owned)
                .or_else(|| f.url.strip_prefix(srv.alt_url()).map(|p| format!("ALT{p}")))
                .unwrap_or_else(|| f.url.clone())
        })
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn skipped(events: &[CrawlEvent], suffix: &str) -> Vec<SkipReason> {
    events
        .iter()
        .filter_map(|e| match e {
            CrawlEvent::Skipped { url, reason } if url.ends_with(suffix) => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

fn requests(srv: &TestServer) -> Vec<RequestRecord> {
    srv.state().requests()
}

fn hits(srv: &TestServer, target: &str) -> usize {
    requests(srv)
        .iter()
        .filter(|r| r.target == target && r.method == "GET")
        .count()
}

const DEPTH0_FILES: &[&str] = &[
    "/site/files/report.pdf",
    "/site/files/photo.jpg",
    "/site/download?id=1",
    "ALT/site/offsite/file.zip",
];

// ------------------------------------------------------------------------------------------------
// depth, scope
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn depth_zero_reads_only_the_start_page() {
    let srv = TestServer::spawn().await;
    let (result, events) = run(spec(&srv, 0)).await;
    assert_eq!(paths(&srv, &result), set(DEPTH0_FILES));
    assert_eq!(result.stats.pages_visited, 1);
    assert_eq!(result.stop, StopReason::Finished);
    // no sub-page was requested; only the start page, robots.txt and the two link probes
    for sub in ["/site/docs/", "/site/about.html", "/site/private/"] {
        assert_eq!(hits(&srv, sub), 0, "{sub} must not be fetched at depth 0");
    }
    assert!(skipped(&events, "/site/docs/").contains(&SkipReason::Depth));
    // the last event is Done and agrees with the result
    match events.last() {
        Some(CrawlEvent::Done { stats }) => {
            assert_eq!(stats.files_found, result.files.len() as u64);
            assert_eq!(stats.pages_visited, 1);
        }
        other => panic!("last event: {other:?}"),
    }
    let found = events
        .iter()
        .filter(|e| matches!(e, CrawlEvent::FileFound(_)))
        .count();
    assert_eq!(found, result.files.len());
}

#[tokio::test]
async fn each_extra_level_reveals_the_next_files() {
    let srv = TestServer::spawn().await;
    let mut expected: BTreeSet<String> = set(DEPTH0_FILES);
    let steps: [(u32, &[&str]); 3] = [
        (1, &["/site/files/song.mp3", "/site/files/notes.txt"]),
        (2, &["/site/files/archive.zip"]),
        (3, &["/site/files/clip.mp4", "/site/files/setup.exe"]),
    ];
    for (depth, new_files) in steps {
        expected.extend(new_files.iter().map(|s| (*s).to_string()));
        let (result, _) = run(spec(&srv, depth)).await;
        assert_eq!(paths(&srv, &result), expected, "depth {depth}");
        assert!(
            result.pages.iter().all(|p| p.depth <= depth),
            "depth {depth}: {:?}",
            result.pages
        );
    }
    // depth 4 finds nothing more: depth 3 was the deepest page
    let (result, _) = run(spec(&srv, 9)).await;
    assert_eq!(paths(&srv, &result), expected);
}

#[tokio::test]
async fn loops_and_repeated_links_visit_each_page_once() {
    let srv = TestServer::spawn().await;
    let (result, _) = run(spec(&srv, 9)).await;
    // index, docs/, about.html, chapter1.html, appendix.html (big pages and 404 are skipped)
    assert_eq!(result.stats.pages_visited, 5, "{:?}", result.pages);
    assert!(result.stats.duplicates >= 3, "{:?}", result.stats);
    for page in [
        "/site/",
        "/site/docs/",
        "/site/about.html",
        "/site/docs/chapter1.html",
        "/site/docs/deep/appendix.html",
    ] {
        assert_eq!(hits(&srv, page), 1, "{page}");
    }
    // `about.html#team` and `about.html` are one URL; report.pdf is linked three times, listed once
    assert_eq!(
        result
            .files
            .iter()
            .filter(|f| f.url.ends_with("/report.pdf"))
            .count(),
        1
    );
    // the tree: every page but the start has a parent that was visited
    let visited: BTreeSet<&str> = result.pages.iter().map(|p| p.url.as_str()).collect();
    for p in &result.pages {
        if let Some(parent) = &p.parent {
            assert!(visited.contains(parent.as_str()), "{p:?}");
        } else {
            assert_eq!(p.depth, 0);
        }
    }
}

#[tokio::test]
async fn off_site_pages_are_never_read_but_off_site_files_are_listed() {
    let srv = TestServer::spawn().await;
    let (result, events) = run(spec(&srv, 3)).await;
    let all = paths(&srv, &result);
    assert!(all.contains("ALT/site/offsite/file.zip"));
    assert!(!all.contains("ALT/site/offsite/hidden.zip"), "{all:?}");
    // the off-site page was fetched once (to see what it is) and dropped, nothing else was
    let alt: Vec<String> = requests(&srv)
        .into_iter()
        .filter(|r| r.origin == srv.alt_url())
        .map(|r| format!("{} {}", r.method, r.target))
        .collect();
    assert_eq!(
        alt.iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            &"GET /robots.txt".to_string(),
            &"GET /site/offsite/page".to_string()
        ]),
        "{alt:?}"
    );
    assert!(skipped(&events, "/site/offsite/page").contains(&SkipReason::OffSite));
}

#[tokio::test]
async fn without_off_site_files_nothing_else_is_contacted() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 3);
    s.offsite_files = false;
    let (result, events) = run(s).await;
    assert!(!paths(&srv, &result).iter().any(|p| p.starts_with("ALT")));
    assert!(requests(&srv).iter().all(|r| r.origin == srv.base_url()));
    assert!(skipped(&events, "/site/offsite/file.zip").contains(&SkipReason::OutOfScope));
}

#[tokio::test]
async fn scope_any_walks_into_the_other_origin() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 2);
    s.scope = Scope::Any;
    let (result, _) = run(s).await;
    let all = paths(&srv, &result);
    assert!(all.contains("ALT/site/offsite/hidden.zip"), "{all:?}");
    // domain scope on an IP is a host+port scope: the second port stays out
    let mut s = spec(&srv, 2);
    s.scope = Scope::SameDomain;
    let (result, _) = run(s).await;
    assert!(!paths(&srv, &result).contains("ALT/site/offsite/hidden.zip"));
}

#[tokio::test]
async fn unsupported_schemes_are_reported_and_never_requested() {
    let srv = TestServer::spawn().await;
    let (result, events) = run(spec(&srv, 0)).await;
    let schemes = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                CrawlEvent::Skipped {
                    reason: SkipReason::UnsupportedScheme,
                    ..
                }
            )
        })
        .count();
    assert_eq!(schemes, 2, "mailto: and javascript:");
    assert!(result.stats.skipped >= 2);
    // neither the stylesheet, nor links inside <script> or comments are followed
    for never in [
        "/site/style.css",
        "/site/files/script-only.zip",
        "/site/files/commented.zip",
    ] {
        assert_eq!(hits(&srv, never), 0, "{never}");
        assert!(!paths(&srv, &result).contains(never));
    }
}

#[tokio::test]
async fn a_link_that_is_really_a_file_is_listed_with_its_real_name() {
    let srv = TestServer::spawn().await;
    let (result, _) = run(spec(&srv, 0)).await;
    let export = result
        .files
        .iter()
        .find(|f| f.url.ends_with("/site/download?id=1"))
        .unwrap();
    assert_eq!(export.guessed_name, "export.csv");
    assert_eq!(export.category, Category::Documents);
    assert_eq!(export.size, Some(2000));
    assert_eq!(export.referrer, srv.url("/site/"));
}

// ------------------------------------------------------------------------------------------------
// filters
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn include_and_exclude_globs() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 3);
    s.include = vec!["*.pdf".into(), "**/*.zip".into()];
    let (result, events) = run(s).await;
    assert_eq!(
        paths(&srv, &result),
        set(&[
            "/site/files/report.pdf",
            "/site/files/archive.zip",
            "ALT/site/offsite/file.zip"
        ])
    );
    assert!(skipped(&events, "/site/files/photo.jpg").contains(&SkipReason::NotIncluded));

    // exclude applies to pages too: nothing below docs/ is read, so nothing below it is found
    srv.state().clear_requests();
    let mut s = spec(&srv, 3);
    s.exclude = vec!["/site/docs/**".into(), "*.jpg".into()];
    let (result, events) = run(s).await;
    let all = paths(&srv, &result);
    assert!(!all.contains("/site/files/song.mp3"));
    assert!(!all.contains("/site/files/archive.zip"));
    assert!(!all.contains("/site/files/photo.jpg"));
    assert!(all.contains("/site/files/notes.txt"));
    assert_eq!(hits(&srv, "/site/docs/"), 0);
    assert!(skipped(&events, "/site/docs/").contains(&SkipReason::Excluded));
}

#[tokio::test]
async fn file_type_filter() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 3);
    s.categories = vec![Category::Video];
    let (result, events) = run(s).await;
    assert_eq!(paths(&srv, &result), set(&["/site/files/clip.mp4"]));
    assert!(skipped(&events, "/site/files/report.pdf").contains(&SkipReason::FileType));
    // pages are still walked to reach it
    assert!(result.stats.pages_visited >= 5);

    let mut s = spec(&srv, 3);
    s.categories = vec![Category::Documents, Category::Music];
    let (result, _) = run(s).await;
    assert_eq!(
        paths(&srv, &result),
        set(&[
            "/site/files/report.pdf",
            "/site/files/notes.txt",
            "/site/files/song.mp3",
            "/site/download?id=1"
        ])
    );
}

#[tokio::test]
async fn size_window_uses_probes() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 3);
    s.min_size = Some(100_000);
    let (result, events) = run(s).await;
    assert_eq!(
        paths(&srv, &result),
        set(&[
            "/site/files/report.pdf",
            "/site/files/song.mp3",
            "/site/files/clip.mp4",
            "/site/files/archive.zip",
            "/site/files/setup.exe",
        ])
    );
    assert!(result.files.iter().all(|f| f.size.unwrap() >= 100_000));
    assert!(skipped(&events, "/site/files/notes.txt").contains(&SkipReason::Size));

    let mut s = spec(&srv, 3);
    s.max_size = Some(100_000);
    let (result, _) = run(s).await;
    assert_eq!(
        paths(&srv, &result),
        set(&[
            "/site/files/photo.jpg",
            "/site/files/notes.txt",
            "/site/download?id=1",
            "ALT/site/offsite/file.zip",
        ])
    );
    for f in &result.files {
        assert!(f.size.unwrap() <= 100_000, "{f:?}");
        assert!(f.etag.is_some());
    }
}

// ------------------------------------------------------------------------------------------------
// robots.txt
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn robots_txt_keeps_the_crawler_out_of_private() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 9);
    s.probe_files = true;
    let (result, events) = run(s).await;
    let log = requests(&srv);
    assert!(
        log.iter().all(|r| !r.target.starts_with("/site/private/")),
        "private/ was requested: {log:?}"
    );
    assert!(!paths(&srv, &result).contains("/site/private/secret.pdf"));
    assert_eq!(
        skipped(&events, "/site/private/"),
        vec![SkipReason::Robots],
        "the page"
    );
    assert_eq!(
        skipped(&events, "/site/private/secret.pdf"),
        vec![SkipReason::Robots]
    );
    // fetched once per origin it talks to
    assert_eq!(hits(&srv, "/robots.txt"), 2, "base and alt");
}

#[tokio::test]
async fn robots_can_be_switched_off_by_the_user() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 1);
    s.respect_robots = false;
    let (result, _) = run(s).await;
    assert!(paths(&srv, &result).contains("/site/private/secret.pdf"));
    assert_eq!(hits(&srv, "/robots.txt"), 0);
    assert_eq!(hits(&srv, "/site/private/"), 1);
}

// ------------------------------------------------------------------------------------------------
// caps, errors, time
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn pages_over_the_size_cap_are_skipped_streamed_or_not() {
    let srv = TestServer::spawn().await;
    let (result, events) = run(spec(&srv, 1)).await;
    assert_eq!(
        skipped(&events, "/site/big.html"),
        vec![SkipReason::PageTooLarge],
        "Content-Length announces it"
    );
    assert_eq!(
        skipped(&events, "/site/big-chunked.html"),
        vec![SkipReason::PageTooLarge],
        "no Content-Length: stopped while streaming"
    );
    assert!(!paths(&srv, &result).contains("/site/files/from-big.zip"));
    assert!(result.stats.bytes_read < 1024 * 1024, "{:?}", result.stats);

    let mut s = spec(&srv, 1);
    s.max_page_bytes = 100;
    let (result, events) = run(s).await;
    assert!(result.files.is_empty());
    assert_eq!(result.stats.pages_visited, 0);
    assert!(skipped(&events, "/site/").contains(&SkipReason::PageTooLarge));
}

#[tokio::test]
async fn missing_pages_are_reported_and_the_crawl_goes_on() {
    let srv = TestServer::spawn().await;
    let (result, events) = run(spec(&srv, 1)).await;
    assert_eq!(
        skipped(&events, "/site/missing.html"),
        vec![SkipReason::HttpStatus(404)]
    );
    assert!(result.stats.errors >= 1);
    assert!(paths(&srv, &result).contains("/site/files/song.mp3"));
    assert_eq!(result.stop, StopReason::Finished);
}

#[tokio::test]
async fn page_and_file_caps() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 9);
    s.max_pages = 2;
    let (result, events) = run(s).await;
    assert!(result.page_cap_hit);
    assert!(result.stats.pages_visited <= 2, "{:?}", result.stats);
    assert!(events.iter().any(|e| matches!(
        e,
        CrawlEvent::Skipped {
            reason: SkipReason::PageCap,
            ..
        }
    )));

    let mut s = spec(&srv, 9);
    s.max_files = 3;
    // robots off: which links get the three slots is then decided in document order, not by
    // when a robots refusal comes back
    s.respect_robots = false;
    let (result, events) = run(s).await;
    assert!(result.file_cap_hit);
    assert_eq!(result.files.len(), 3);
    assert!(events.iter().any(|e| matches!(
        e,
        CrawlEvent::Skipped {
            reason: SkipReason::FileCap,
            ..
        }
    )));
}

#[tokio::test]
async fn the_time_budget_ends_a_slow_crawl() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 9);
    // the start page is fetched at once, every later request to the host waits 30 s
    s.delay = Duration::from_secs(30);
    s.time_budget = Duration::from_millis(600);
    let started = Instant::now();
    let (result, events) = run(s).await;
    assert_eq!(result.stop, StopReason::TimeBudget);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(matches!(events.last(), Some(CrawlEvent::Done { .. })));
}

#[tokio::test]
async fn cancellation_stops_promptly_and_still_reports_done() {
    let srv = TestServer::spawn().await;

    // already cancelled: nothing is requested
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (result, events) = run_with(spec(&srv, 3), cancel).await;
    assert_eq!(result.stop, StopReason::Cancelled);
    assert!(requests(&srv).is_empty());
    assert!(matches!(events.last(), Some(CrawlEvent::Done { .. })));

    // cancelled after the first page, while the next requests are held back by the delay
    // (robots.txt takes the first slot, the start page the second, children wait further)
    let mut s = spec(&srv, 9);
    s.delay = Duration::from_millis(400);
    let cancel = CancellationToken::new();
    let (tx, mut rx) = mpsc::channel(8);
    let token = cancel.clone();
    let consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            if matches!(e, CrawlEvent::PageVisited { .. }) {
                token.cancel();
            }
            events.push(e);
        }
        events
    });
    let started = Instant::now();
    let result = crawl(s, client(), Some(tx), cancel).await.unwrap();
    let events = consumer.await.unwrap();
    assert_eq!(result.stop, StopReason::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(result.stats.pages_visited, 1);
    assert!(matches!(events.last(), Some(CrawlEvent::Done { .. })));
}

#[tokio::test]
async fn bad_start_urls_are_refused() {
    for start in ["ftp://example.com/", "mailto:a@b.c", "not a url", ""] {
        let err = crawl(
            CrawlSpec::new(start),
            client(),
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::InvalidStart(_)), "{start:?}: {err}");
    }
    let mut s = CrawlSpec::new("http://127.0.0.1:1/");
    s.concurrency = 0;
    assert!(matches!(
        crawl(s, client(), None, CancellationToken::new()).await,
        Err(Error::InvalidSpec(_))
    ));
}

// ------------------------------------------------------------------------------------------------
// secrets and sync
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn cookies_and_credentials_never_leave_the_start_origin() {
    let srv = TestServer::spawn().await;
    let mut s = spec(&srv, 2);
    s.scope = Scope::Any; // walk into the other origin on purpose
    s.probe_files = true;
    s.secrets = Some(Secrets {
        cookies: Some("session=riptide".into()),
        headers: vec![("X-Token".into(), "secret".into())],
        auth: Some(Credentials {
            username: "u".into(),
            password: "p".into(),
        }),
    });
    let (result, _) = run(s).await;
    assert!(paths(&srv, &result).contains("ALT/site/offsite/hidden.zip"));
    let log = requests(&srv);
    let alt: Vec<_> = log.iter().filter(|r| r.origin == srv.alt_url()).collect();
    assert!(
        alt.len() >= 4,
        "the other origin was really visited: {alt:?}"
    );
    for r in &alt {
        assert!(
            !r.has_cookie && !r.has_authorization,
            "leaked to the other origin: {r:?}"
        );
    }
    let base_pages: Vec<_> = log
        .iter()
        .filter(|r| r.origin == srv.base_url() && r.target.starts_with("/site/"))
        .collect();
    assert!(!base_pages.is_empty());
    assert!(
        base_pages.iter().all(|r| r.has_cookie),
        "the start origin gets the cookie: {base_pages:?}"
    );
    // robots.txt is public and fetched bare, everywhere
    assert!(log
        .iter()
        .filter(|r| r.target == "/robots.txt")
        .all(|r| !r.has_cookie && !r.has_authorization));

    // and the download requests carry the secrets only for the start origin
    let secrets = Secrets {
        cookies: Some("session=riptide".into()),
        ..Secrets::default()
    };
    let reqs = result.download_requests(Some(&secrets), None);
    assert_eq!(reqs.len(), result.files.len());
    for r in &reqs {
        let on_start = r.url.starts_with(srv.base_url());
        assert_eq!(r.secrets.is_some(), on_start, "{}", r.url);
        assert!(r.filename.is_some() && r.referrer.is_some());
    }
}

#[tokio::test]
async fn sync_mode_marks_changed_files() {
    let srv = TestServer::spawn().await;
    let mut first = spec(&srv, 1);
    first.probe_files = true;
    let (before, _) = run(first).await;
    assert!(before.files.iter().all(|f| f.changed.is_none()));
    assert!(before.files.iter().all(|f| f.etag.is_some()));

    let mut known = before.known_files();
    let changed_url = srv.url("/site/files/report.pdf");
    let dropped_url = srv.url("/site/files/notes.txt");
    known
        .iter_mut()
        .find(|k| k.url == changed_url)
        .unwrap()
        .etag = Some("\"old\"".into());
    known.retain(|k| k.url != dropped_url);

    let mut second = spec(&srv, 1);
    second.previous = known;
    let (after, _) = run(second).await;
    for f in &after.files {
        let want = if f.url == changed_url {
            Some(true)
        } else if f.url == dropped_url {
            None
        } else {
            Some(false)
        };
        assert_eq!(f.changed, want, "{}", f.url);
    }
}
