//! Live checks of the media adapters against the real services. All `#[ignore]`d: run with
//! `cargo test -p riptide-research --test live_media -- --ignored --test-threads=1 --nocapture`.
//!
//! The Jamendo test needs a free client id in the environment variable `JAMENDO_CLIENT_ID`
//! and prints a note and passes without it.
#![allow(clippy::unwrap_used)]

use riptide_research::sources::{
    blender::Blender, commons::Commons, gutenberg::Gutenberg, internet_archive::InternetArchive,
    jamendo::Jamendo, librivox::LibriVox, openlibrary::OpenLibrary, openverse::Openverse,
};
use riptide_research::{Category, Hit, SearchSource};

async fn check(
    label: &str,
    adapter: impl SearchSource,
    category: Category,
    query: &str,
) -> Vec<Hit> {
    let start = std::time::Instant::now();
    let hits = adapter
        .search_in(category, query, 5)
        .await
        .unwrap_or_else(|e| panic!("{label} failed: {e}"));
    println!(
        "LIVE {label:<22} {} hit(s) in {} ms",
        hits.len(),
        start.elapsed().as_millis()
    );
    if let Some(h) = hits.first() {
        println!(
            "     first: {} | {} | licence: {} | size: {:?} | file: {}",
            h.title,
            h.download_url,
            h.license.as_deref().unwrap_or("-"),
            h.size,
            h.filename.as_deref().unwrap_or("-")
        );
    }
    assert!(!hits.is_empty(), "{label} returned nothing");
    for h in &hits {
        assert!(h.download_url.starts_with("http"), "{h:?}");
        assert!(!h.download_url.contains(' '), "{h:?}");
        assert!(!h.title.is_empty());
    }
    hits
}

#[tokio::test]
#[ignore = "live network"]
async fn gutenberg() {
    let hits = check(
        "Gutenberg",
        Gutenberg::new(),
        Category::Books,
        "pride and prejudice",
    )
    .await;
    assert!(hits.iter().all(|h| h.license.as_deref() != Some("")));
}

#[tokio::test]
#[ignore = "live network"]
async fn open_library() {
    check("Open Library", OpenLibrary::new(), Category::Books, "alice").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn librivox() {
    check(
        "LibriVox",
        LibriVox::new(),
        Category::Books,
        "pride and prejudice",
    )
    .await;
}

/// Openverse's anonymous API is often overloaded; an outage is reported, not a failure.
async fn openverse(category: Category, query: &str) {
    use riptide_research::ResearchError;
    match Openverse::new().search_in(category, query, 5).await {
        Ok(hits) => {
            println!("LIVE Openverse {category:?}: {} hit(s)", hits.len());
            if let Some(h) = hits.first() {
                println!(
                    "     first: {} | {} | licence: {} | size: {:?}",
                    h.title,
                    h.download_url,
                    h.license.as_deref().unwrap_or("-"),
                    h.size
                );
            }
            assert!(!hits.is_empty());
            assert!(hits.iter().all(|h| h.download_url.starts_with("http")));
        }
        Err(
            e @ (ResearchError::Timeout
            | ResearchError::RateLimited { .. }
            | ResearchError::Http { status: 500..=599 }),
        ) => println!("LIVE Openverse {category:?}: SKIPPED, service unavailable: {e}"),
        Err(e) => panic!("Openverse {category:?} failed: {e}"),
    }
}

#[tokio::test]
#[ignore = "live network"]
async fn openverse_audio() {
    openverse(Category::Music, "piano").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn openverse_images() {
    openverse(Category::Images, "cat").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn internet_archive_audio() {
    check(
        "Internet Archive audio",
        InternetArchive::new(),
        Category::Music,
        "jazz",
    )
    .await;
}

#[tokio::test]
#[ignore = "live network"]
async fn internet_archive_video() {
    check(
        "Internet Archive video",
        InternetArchive::new(),
        Category::Video,
        "night",
    )
    .await;
}

#[tokio::test]
#[ignore = "live network"]
async fn commons_audio() {
    check("Commons audio", Commons::new(), Category::Music, "piano").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn commons_video() {
    check("Commons video", Commons::new(), Category::Video, "volcano").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn blender_offline() {
    // the catalogue is built in: no request is made
    check(
        "Blender (offline)",
        Blender::new(),
        Category::Video,
        "sintel",
    )
    .await;
}

#[tokio::test]
#[ignore = "live network"]
async fn jamendo() {
    let Ok(id) = std::env::var("JAMENDO_CLIENT_ID") else {
        println!("LIVE Jamendo              skipped: set JAMENDO_CLIENT_ID to a free client id");
        return;
    };
    check("Jamendo", Jamendo::new(&id), Category::Music, "piano").await;
}
