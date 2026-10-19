//! A crawl that starts on a public host must not fetch or list links to private addresses; a
//! crawl that starts on a private address (the loopback test server) may. The "public host"
//! is simulated: an injected resolver says `public.test` is 93.184.216.34 while the client
//! really connects to the test server.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use riptide_crawl::{crawl, crawl_with_guard, CrawlEvent, CrawlSpec, SkipReason};
use riptide_net::net_guard::StaticResolver;
use riptide_net::{build_client, ClientOptions, NetGuard};
use riptide_testserver::TestServer;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use url::Url;

fn public_guard() -> NetGuard {
    let table = StaticResolver(HashMap::from([(
        "public.test".to_string(),
        vec!["93.184.216.34".parse::<IpAddr>().unwrap()],
    )]));
    NetGuard::blocking().with_resolver(Arc::new(table))
}

async fn run(
    spec: CrawlSpec,
    client: reqwest::Client,
    guard: Option<NetGuard>,
) -> (riptide_crawl::CrawlResult, Vec<CrawlEvent>) {
    let (tx, mut rx) = mpsc::channel(8);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        events
    });
    let cancel = CancellationToken::new();
    let result = match guard {
        Some(g) => crawl_with_guard(spec, client, Some(tx), cancel, g).await,
        None => crawl(spec, client, Some(tx), cancel).await,
    }
    .unwrap();
    (result, collector.await.unwrap())
}

#[tokio::test]
async fn a_public_start_never_touches_the_private_off_site_link() {
    let server = TestServer::spawn().await;
    let port = Url::parse(server.base_url()).unwrap().port().unwrap();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve("public.test", SocketAddr::from(([127, 0, 0, 1], port)))
        .build()
        .unwrap();
    let mut spec = CrawlSpec::new(format!("http://public.test:{port}/site/"));
    spec.depth = 2;
    spec.delay = Duration::ZERO;
    let (result, events) = run(spec, client, Some(public_guard())).await;

    // the site itself is crawled
    assert!(
        result
            .files
            .iter()
            .any(|f| f.url.ends_with("/files/report.pdf")),
        "{:?}",
        result.files.iter().map(|f| &f.url).collect::<Vec<_>>()
    );
    // the link to the other (loopback) origin is neither listed nor requested
    assert!(
        result
            .files
            .iter()
            .all(|f| !f.url.starts_with(server.alt_url())),
        "private off-site file listed"
    );
    let refused: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CrawlEvent::Skipped { url, reason } if url.starts_with(server.alt_url()) => {
                Some(reason.clone())
            }
            _ => None,
        })
        .collect();
    assert!(
        !refused.is_empty(),
        "the off-site link should be reported as skipped"
    );
    assert!(
        refused
            .iter()
            .all(|r| matches!(r, SkipReason::Error(m) if m.contains("private network address"))),
        "{refused:?}"
    );
    let on_alt = server
        .state()
        .requests()
        .iter()
        .filter(|r| r.origin == server.alt_url())
        .count();
    assert_eq!(on_alt, 0, "nothing was requested from the private origin");
}

#[tokio::test]
async fn a_private_start_may_crawl_private_targets() {
    // the loopback test server is a private start: the owner crawling their own machine
    let server = TestServer::spawn().await;
    let mut spec = CrawlSpec::new(server.url("/site/"));
    spec.depth = 2;
    spec.delay = Duration::ZERO;
    let client = build_client(&ClientOptions::default()).unwrap();
    let (result, _events) = run(spec, client, None).await;
    assert!(
        result
            .files
            .iter()
            .any(|f| f.url.starts_with(server.alt_url())),
        "the off-site file on the other loopback origin is listed"
    );
}

#[tokio::test]
async fn a_blocking_guard_refuses_a_private_start_page() {
    let server = TestServer::spawn().await;
    let mut spec = CrawlSpec::new(server.url("/site/"));
    spec.delay = Duration::ZERO;
    let client = build_client(&ClientOptions::default()).unwrap();
    let (result, events) = run(spec, client, Some(NetGuard::blocking())).await;
    assert!(result.files.is_empty());
    assert!(events.iter().any(|e| matches!(
        e,
        CrawlEvent::Skipped { reason: SkipReason::Error(m), .. } if m.contains("private network address")
    )));
    assert!(
        server.state().requests().is_empty(),
        "no request reached the server"
    );
}
