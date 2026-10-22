//! The "already downloaded" index through the engine: a finished download is recognised under
//! URL variants and by content hash, deleting the file flips the match to stale, and the
//! "skip duplicates" option refuses a repeat.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_core::{DownloadId, DownloadRequest, DownloadState};
use riptide_engine::{Engine, EngineConfig, Error, SeenHint, SeenQuery};
use riptide_testserver::TestServer;

fn config(dir: &tempfile::TempDir) -> EngineConfig {
    EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
        organize_new_install: false,
    }
}

async fn wait_completed(engine: &Engine, id: DownloadId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let d = engine.get(id).await.unwrap().unwrap();
        if d.state == DownloadState::Completed {
            return;
        }
        assert!(
            !matches!(d.state, DownloadState::Failed { .. }),
            "download failed"
        );
        assert!(tokio::time::Instant::now() < deadline, "timeout");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The hash arrives a moment after Completed (it is computed off the async runtime).
async fn wait_hashed(engine: &Engine, url: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let q = SeenQuery {
            url: Some(url.into()),
            ..SeenQuery::default()
        };
        if engine.seen_lookup(vec![q]).await.unwrap()[0].is_some() {
            // the sha256 key lands right after the record; give it a beat
            tokio::time::sleep(Duration::from_millis(300)).await;
            return;
        }
        assert!(tokio::time::Instant::now() < deadline, "never indexed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn hint_rows(dir: &tempfile::TempDir) -> Vec<String> {
    let store = riptide_store::Store::open(&config(dir).db_path).unwrap();
    store
        .settings()
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("seen.hint."))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn research_hints_do_not_pile_up_in_the_settings_table() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();
    let hint = || SeenHint {
        title: Some("A private research title".into()),
        doi: Some("10.1234/x".into()),
        ..SeenHint::default()
    };
    // a download that finishes: its hint row is deleted once it is indexed
    let url = server.url("/ok?size=100000");
    let done = engine
        .add_hinted(DownloadRequest::new(url.clone()), hint(), false)
        .await
        .unwrap();
    wait_completed(&engine, done).await;
    wait_hashed(&engine, &url).await;
    // one that is removed before it ever finishes
    let slow = engine
        .add_hinted(
            DownloadRequest {
                start_paused: true,
                ..DownloadRequest::new(server.url("/slow?bps=1000&size=100000"))
            },
            hint(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        hint_rows(&dir).len(),
        1,
        "the unfinished download keeps its hint for now"
    );
    engine.remove(slow, true).await.unwrap();
    assert!(hint_rows(&dir).is_empty(), "{:?}", hint_rows(&dir));

    // and "Clear download memory" removes hints that are still waiting
    let waiting = engine
        .add_hinted(
            DownloadRequest {
                start_paused: true,
                ..DownloadRequest::new(server.url("/ok?size=1000&seed=9"))
            },
            hint(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(hint_rows(&dir).len(), 1);
    engine.seen_forget().await.unwrap();
    assert!(hint_rows(&dir).is_empty());
    let _ = waiting;
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_is_found_across_url_variants_and_by_hash_and_goes_stale_on_delete() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();

    let url = server.url("/ok?size=200000");
    let id = engine
        .add_hinted(
            DownloadRequest::new(url.clone()),
            SeenHint {
                title: Some("A very good paper".into()),
                doi: Some("10.1234/Paper.One".into()),
                ..SeenHint::default()
            },
            false,
        )
        .await
        .unwrap();
    wait_completed(&engine, id).await;
    wait_hashed(&engine, &url).await;
    let d = engine.get(id).await.unwrap().unwrap();
    let path = d.final_path();
    assert!(path.exists());

    // same address, written differently: other query order is irrelevant here, but the
    // tracking parameters and fragment are
    let variant = format!("{url}&utm_source=newsletter&fbclid=abc#section");
    let by_doi = SeenQuery {
        doi: Some("https://doi.org/10.1234/paper.one".into()),
        ..SeenQuery::default()
    };
    let by_url = SeenQuery {
        url: Some(variant),
        ..SeenQuery::default()
    };
    let by_file = SeenQuery {
        filename: Some(d.filename.clone()),
        size: d.size,
        ..SeenQuery::default()
    };
    let unrelated = SeenQuery {
        url: Some(server.url("/ok?size=999")),
        ..SeenQuery::default()
    };
    let hits = engine
        .seen_lookup(vec![by_url, by_doi, by_file, unrelated])
        .await
        .unwrap();
    let m = hits[0].as_ref().expect("url variant must match");
    assert_eq!(m.matched_by, "url");
    assert!(!m.stale);
    assert_eq!(m.title, "A very good paper");
    assert_eq!(hits[1].as_ref().unwrap().matched_by, "doi");
    assert_eq!(hits[2].as_ref().unwrap().matched_by, "file");
    assert!(hits[3].is_none());

    // the same bytes from a different address (and a different file name) are found by hash
    let sha = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(std::fs::read(&path).unwrap()))
    };
    let by_hash = SeenQuery {
        url: Some("https://mirror.example/other-name.bin".into()),
        sha256: Some(sha),
        ..SeenQuery::default()
    };
    assert_eq!(
        engine.seen_lookup(vec![by_hash]).await.unwrap()[0]
            .as_ref()
            .unwrap()
            .matched_by,
        "sha256"
    );

    // delete the file: still remembered, but stale, and the flag is saved
    std::fs::remove_file(&path).unwrap();
    let q = SeenQuery {
        url: Some(url.clone()),
        ..SeenQuery::default()
    };
    let stale = engine.seen_lookup(vec![q.clone()]).await.unwrap();
    assert!(stale[0].as_ref().unwrap().stale);
    std::fs::write(&path, vec![0u8; d.size.unwrap() as usize]).unwrap();
    let back = engine.seen_lookup(vec![q.clone()]).await.unwrap();
    assert!(!back[0].as_ref().unwrap().stale, "file came back");

    // forgetting removes the memory but not the file
    assert_eq!(engine.seen_forget().await.unwrap(), 1);
    assert!(engine.seen_lookup(vec![q]).await.unwrap()[0].is_none());
    assert!(path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn skip_duplicates_refuses_a_repeat_only_when_enabled() {
    let server = TestServer::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(&dir)).await.unwrap();
    let url = server.url("/ok?size=50000");
    let id = engine.add(DownloadRequest::new(url.clone())).await.unwrap();
    wait_completed(&engine, id).await;
    wait_hashed(&engine, &url).await;

    // off by default: a repeat is allowed (the dialog only warns)
    assert!(!engine.skip_duplicates().await);
    let again = engine
        .add(DownloadRequest::new(format!("{url}&utm_campaign=x")))
        .await;
    assert!(again.is_ok());

    engine.set_skip_duplicates(true).await.unwrap();
    let refused = engine
        .add(DownloadRequest::new(format!("{url}&gclid=z#frag")))
        .await;
    assert!(
        matches!(refused, Err(Error::AlreadyDownloaded(_))),
        "{refused:?}"
    );
    let batch = engine
        .add_batch(vec![DownloadRequest::new(url.clone())])
        .await
        .unwrap();
    assert!(batch.added.is_empty());
    assert_eq!(batch.skipped.len(), 1);
}
