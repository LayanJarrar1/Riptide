//! The site grabber through the engine: crawl `/site/`, add every found file, download them all
//! and compare SHA-256 with the test server's deterministic bytes.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use riptide_core::{ChecksumAlgo, DownloadId, DownloadState, Event};
use riptide_crawl::{CrawlEvent, CrawlSpec, StopReason};
use riptide_engine::{Engine, EngineConfig, Error};
use riptide_net::checksum_of;
use riptide_testserver::{site_file_sha256_hex, TestServer};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The path of a crawled URL below `/site/`, as `riptide_testserver::SITE_FILES` lists it.
fn site_path(url: &str) -> String {
    let rest = url.split_once("/site/").unwrap().1;
    if rest.starts_with("download") {
        "export.csv".to_string()
    } else {
        rest.to_string()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn crawl_site_add_every_file_and_the_bytes_match() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig {
        db_path: tmp.path().join("riptide.sqlite"),
        default_dir: tmp.path().join("downloads"),
        organize_new_install: false,
    })
    .await
    .unwrap();

    // 1. crawl: the engine's client, events on a channel
    let mut spec = CrawlSpec::new(server.url("/site/"));
    spec.depth = 3;
    spec.delay = Duration::ZERO;
    let (tx, mut rx) = mpsc::channel(16);
    let drain = tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Some(e) = rx.recv().await {
            seen.push(e);
        }
        seen
    });
    let result = engine
        .crawl(spec, Some(tx), CancellationToken::new())
        .await
        .unwrap();
    let events = drain.await.unwrap();
    assert_eq!(result.stop, StopReason::Finished);
    assert!(matches!(events.last(), Some(CrawlEvent::Done { .. })));
    assert_eq!(result.files.len(), 9, "{:#?}", result.files);
    assert!(
        engine.list().await.unwrap().is_empty(),
        "crawling adds nothing by itself"
    );

    // 2. add every file
    let dest = tmp.path().join("grabbed");
    let mut done = engine.subscribe();
    let mut ids = BTreeMap::new();
    for (file, request) in result
        .files
        .iter()
        .zip(result.download_requests(None, Some(&dest)))
    {
        assert_eq!(request.url, file.url);
        let id = engine.add(request).await.unwrap();
        ids.insert(id, file.url.clone());
    }

    // 3. wait until all complete
    let mut completed: BTreeSet<DownloadId> = BTreeSet::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while completed.len() < ids.len() {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, done.recv()).await {
            Ok(Ok(Event::Completed { id, .. })) => {
                completed.insert(id);
            }
            Ok(Ok(Event::Failed { id, error, .. })) => panic!("download {id:?} failed: {error}"),
            Ok(_) => {}
            Err(_) => panic!("timed out; completed {}/{}", completed.len(), ids.len()),
        }
    }

    // 4. byte-exact: SHA-256 of every file equals what the server derives for that path
    for (id, url) in &ids {
        let record = engine.get(*id).await.unwrap().unwrap();
        assert_eq!(record.state, DownloadState::Completed, "{url}");
        assert_eq!(record.dir, dest, "saved in the chosen folder: {url}");
        let path = dest.join(&record.filename);
        let expected = site_file_sha256_hex(&site_path(url)).unwrap();
        assert_eq!(
            checksum_of(path.clone(), ChecksumAlgo::Sha256)
                .await
                .unwrap(),
            expected,
            "{url} -> {}",
            record.filename
        );
    }
    let names: BTreeSet<String> = std::fs::read_dir(&dest)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 9, "no leftovers, no .rtpart: {names:?}");
    assert!(names.iter().all(|n| !n.ends_with(".rtpart")), "{names:?}");
    assert!(names.contains("export.csv") && names.contains("file.zip"));

    // the Referer of each download is the page it was found on
    let secrets_free = result.download_requests(None, None);
    assert!(secrets_free
        .iter()
        .all(|r| r.secrets.is_none() && r.referrer.is_some()));
    engine.shutdown().await;
}

#[tokio::test]
async fn crawl_errors_and_cancellation_through_the_engine() {
    let server = TestServer::spawn().await;
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig {
        db_path: tmp.path().join("riptide.sqlite"),
        default_dir: tmp.path().join("downloads"),
        organize_new_install: false,
    })
    .await
    .unwrap();

    let err = engine
        .crawl(
            CrawlSpec::new("file:///etc/passwd"),
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Crawl(_)), "{err}");

    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = engine
        .crawl(CrawlSpec::new(server.url("/site/")), None, cancel)
        .await
        .unwrap();
    assert_eq!(result.stop, StopReason::Cancelled);
    assert!(result.files.is_empty());
    engine.shutdown().await;
}
