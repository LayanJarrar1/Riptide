//! Live checks of the game, media and web adapters against the real services. All `#[ignore]`d:
//! `cargo test -p riptide-research --test live_apps -- --ignored --test-threads=1 --nocapture`.
//!
//! * Brave needs `BRAVE_API_KEY` (skipped with a note when absent; the key is never printed).
//! * SearXNG uses `SEARXNG_URL` when set, else a public instance that allowed `format=json`
//!   when this test was written (public instances may switch JSON off or rate-limit at any time).
//! * OpenGameArt asks for a ten second crawl delay, so its test takes about half a minute.
#![allow(clippy::unwrap_used)]

use riptide_research::sources::websearch::{validate_searxng_base, WebBackend, WebSearch};
use riptide_research::sources::{github::Github, nasa::Nasa, opengameart::OpenGameArt};
use riptide_research::{Category, Hit, HitKind, SearchSource};

fn report(name: &str, started: std::time::Instant, hits: &[Hit]) {
    let first = hits.first();
    println!(
        "LIVE {name:<14} {} hit(s) in {} ms; first: {} | {} | licence: {}",
        hits.len(),
        started.elapsed().as_millis(),
        first.map(|h| h.title.as_str()).unwrap_or("-"),
        first.map(|h| h.download_url.as_str()).unwrap_or("-"),
        first
            .and_then(|h| h.license.as_deref())
            .unwrap_or("(none reported)"),
    );
    for h in hits {
        println!(
            "     - {} | {} | size {:?} | licence {:?} | badge {:?}",
            h.title,
            h.download_url,
            h.size,
            h.license,
            h.rights().kind
        );
    }
}

fn assert_files(hits: &[Hit]) {
    assert!(!hits.is_empty(), "no hits");
    for h in hits {
        assert_eq!(h.kind, HitKind::File);
        assert!(h.download_url.starts_with("https://"), "{h:?}");
        assert!(!h.title.is_empty());
    }
}

#[tokio::test]
#[ignore = "live network"]
async fn github_games() {
    let t = std::time::Instant::now();
    let hits = Github::new()
        .search_in(Category::Games, "puzzle", 5)
        .await
        .unwrap();
    report("GitHub games", t, &hits);
    assert_files(&hits);
    assert!(hits.iter().all(|h| h.license.is_some()));
}

#[tokio::test]
#[ignore = "live network"]
async fn opengameart() {
    let t = std::time::Instant::now();
    let hits = OpenGameArt::new()
        .search_in(Category::Games, "forest tileset", 4)
        .await
        .unwrap();
    report("OpenGameArt", t, &hits);
    assert_files(&hits);
    assert!(hits.iter().all(|h| h
        .download_url
        .starts_with("https://opengameart.org/sites/default/files/")));
    assert!(hits.iter().all(|h| h.license.is_some()));
}

#[tokio::test]
#[ignore = "live network"]
async fn nasa_images_and_video() {
    let t = std::time::Instant::now();
    let nasa = Nasa::new();
    let images = nasa
        .search_in(Category::Images, "apollo 11", 5)
        .await
        .unwrap();
    report("NASA images", t, &images);
    assert_files(&images);
    let t = std::time::Instant::now();
    let videos = nasa
        .search_in(Category::Video, "apollo 11", 3)
        .await
        .unwrap();
    report("NASA video", t, &videos);
    assert_files(&videos);
    assert!(videos.iter().all(|h| h.download_url.ends_with(".mp4")));
}

#[tokio::test]
#[ignore = "live network"]
async fn web_brave() {
    let Ok(key) = std::env::var("BRAVE_API_KEY") else {
        println!("LIVE Brave          skipped: BRAVE_API_KEY is not set");
        return;
    };
    let t = std::time::Instant::now();
    let hits = WebSearch::new(WebBackend::Brave { api_key: key })
        .search_in(Category::Web, "open source download manager", 5)
        .await
        .unwrap();
    report("Brave", t, &hits);
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.kind == HitKind::Page));
}

#[tokio::test]
#[ignore = "live network"]
async fn web_searxng() {
    let base = std::env::var("SEARXNG_URL").unwrap_or_else(|_| "https://search.lumy.live".into());
    let base = validate_searxng_base(&base).unwrap();
    let t = std::time::Instant::now();
    let hits = WebSearch::new(WebBackend::SearxNg { base_url: base })
        .search_in(Category::Web, "riptide download manager", 5)
        .await
        .unwrap();
    report("SearXNG", t, &hits);
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.kind == HitKind::Page));
}
