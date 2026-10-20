//! The media adapters (books, audio, video, images) against a mock server that replays
//! responses recorded from the real services (see `tests/fixtures/media_*`, recorded
//! 2026-10-20). No network.
//!
//! Two fixtures are NOT recordings: `media_openverse_audio.json` (the audio endpoint of
//! Openverse answered 504 all day) and `media_jamendo.json` (Jamendo needs a client id).
//! Both follow the documented response shape and say so in a `_note` field.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_research::http::{Http, Tuning};
use riptide_research::sources::{
    blender::Blender, commons::Commons, gutenberg::Gutenberg, internet_archive::InternetArchive,
    jamendo::Jamendo, librivox::LibriVox, openlibrary::OpenLibrary, openverse::Openverse,
};
use riptide_research::{Category, Hit, ResearchError, RightsKind, SearchSource, Source};
use serde_json::Value;
use wiremock::matchers::{method, path, path_regex, query_param, query_param_contains};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A fixture after `edit` changed it (hostile and odd entries are made this way).
fn edited(name: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(&fixture(name)).unwrap();
    edit(&mut v);
    v.to_string()
}

fn fast() -> Http {
    Http::with_tuning(
        Duration::ZERO,
        Tuning {
            timeout: Duration::from_millis(600),
            max_attempts: 2,
            base_delay: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(2),
        },
    )
}

fn json(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "application/json")
        .set_body_string(body)
}

fn assert_direct(h: &Hit) {
    assert!(
        h.download_url.starts_with("https://") || h.download_url.starts_with("http://"),
        "{h:?}"
    );
    assert!(!h.download_url.contains(' '), "{h:?}");
    assert!(!h.title.is_empty());
    assert!(h.id.starts_with(h.source.id()));
    assert!(!h.title.contains('<'), "{h:?}");
}

// ---------------------------------------------------------------- Gutenberg

#[tokio::test]
async fn gutenberg_prefers_epub_skips_copyrighted_and_non_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/books/"))
        .and(query_param("search", "alice wonderland"))
        .respond_with(json(fixture("media_gutenberg.json")))
        .expect(1)
        .mount(&server)
        .await;
    let hits = Gutenberg::with_base(&server.uri(), fast())
        .search_in(Category::Books, "alice wonderland", 50)
        .await
        .unwrap();
    hits.iter().for_each(assert_direct);
    // 13 results: one still image and two audio books are not books
    assert!(hits.len() >= 8 && hits.len() <= 10, "{}", hits.len());
    assert!(hits.iter().all(|h| h.id != "gutenberg:114"));
    assert!(hits.iter().all(|h| h.id != "gutenberg:19573"));
    let alice = hits.iter().find(|h| h.id == "gutenberg:11").unwrap();
    assert_eq!(alice.title, "Alice's Adventures in Wonderland");
    assert_eq!(alice.authors, vec!["Lewis Carroll".to_string()]);
    assert!(alice.download_url.contains("epub"));
    assert_eq!(alice.filename.as_deref(), Some("pg11.epub"));
    assert_eq!(alice.license.as_deref(), Some("Public domain"));
    assert_eq!(alice.rights().kind, RightsKind::PublicDomain);
    assert_eq!(
        alice.page_url.as_deref(),
        Some("https://www.gutenberg.org/ebooks/11")
    );
    assert!(alice.summary.as_deref().unwrap().chars().count() <= 300);
}

#[tokio::test]
async fn gutenberg_limit_and_hostile_entries() {
    let server = MockServer::start().await;
    let body = edited("media_gutenberg.json", |v| {
        let r = v["results"].as_array_mut().unwrap();
        r[0]["copyright"] = Value::Bool(true); // copyrighted: out
        r[1]["copyright"] = Value::Null; // unknown: in, without a licence
        r[2]["formats"] = serde_json::json!({
            "application/epub+zip": "javascript:alert(1)",
            "text/plain; charset=utf-8": "file:///etc/passwd",
            "text/html": "data:text/html,<script>"
        });
        r[3]["title"] = Value::String("   ".into());
    });
    Mock::given(method("GET"))
        .and(path("/books/"))
        .respond_with(json(body))
        .mount(&server)
        .await;
    let g = Gutenberg::with_base(&server.uri(), fast());
    let hits = g.search_in(Category::Books, "alice", 50).await.unwrap();
    hits.iter().for_each(assert_direct);
    assert!(hits.iter().all(|h| h.id != "gutenberg:11"));
    let unknown = hits.iter().find(|h| h.id == "gutenberg:19033").unwrap();
    assert_eq!(unknown.license, None);
    assert!(hits.iter().all(|h| h.id != "gutenberg:28885"));
    assert_eq!(g.search("alice", 2).await.unwrap().len(), 2);
    // not a book source for music
    assert!(g
        .search_in(Category::Music, "alice", 5)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn gutenberg_errors_are_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/books/"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .mount(&server)
        .await;
    let g = Gutenberg::with_base(&server.uri(), fast());
    assert!(matches!(
        g.search("alice", 5).await,
        Err(ResearchError::RateLimited { .. })
    ));
    assert!(matches!(
        g.search("   ", 5).await,
        Err(ResearchError::Invalid(_))
    ));
    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json("{\"detail\":\"Invalid page.\"}".into()))
        .mount(&bad)
        .await;
    assert!(matches!(
        Gutenberg::with_base(&bad.uri(), fast())
            .search("alice", 5)
            .await,
        Err(ResearchError::Parse(_))
    ));
}

// -------------------------------------------------------------- Open Library

async fn open_library(server: &MockServer) -> OpenLibrary {
    OpenLibrary::with_base(&server.uri(), fast()).with_archive(&server.uri())
}

#[tokio::test]
async fn open_library_resolves_public_works_to_archive_files() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .and(query_param("ebook_access", "public"))
        .and(query_param("has_fulltext", "true"))
        .respond_with(json(fixture("media_openlibrary.json")))
        .expect(1)
        .mount(&server)
        .await;
    // every identifier resolves to the same recorded item
    Mock::given(method("GET"))
        .and(path_regex(r"^/metadata/"))
        .respond_with(json(fixture("media_openlibrary_ia.json")))
        .mount(&server)
        .await;
    let hits = open_library(&server)
        .await
        .search_in(Category::Books, "alice", 10)
        .await
        .unwrap();
    hits.iter().for_each(assert_direct);
    // the three public works; the three borrowable ones are never looked at
    assert!(hits.len() <= 3 && !hits.is_empty(), "{}", hits.len());
    assert!(hits.iter().all(|h| h.download_url.ends_with(".pdf")));
    assert!(hits.iter().all(|h| h.download_url.contains("/download/")));
    let first = &hits[0];
    assert_eq!(first.id, "open-library:OL138052W");
    assert_eq!(first.title, "Alice's Adventures in Wonderland");
    assert_eq!(first.authors, vec!["Lewis Carroll".to_string()]);
    assert_eq!(first.year, Some(1865));
    assert_eq!(first.size, Some(9_960_969));
    assert_eq!(first.license.as_deref(), Some("Public domain"));
    assert_eq!(
        first.page_url.as_deref(),
        Some("https://openlibrary.org/works/OL138052W")
    );
    // at most four metadata lookups per search, and lending-style ids come last
    let reqs = server.received_requests().await.unwrap();
    let lookups: Vec<_> = reqs
        .iter()
        .filter(|r| r.url.path().starts_with("/metadata/"))
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(lookups.len() <= 4, "{lookups:?}");
    assert!(lookups.iter().all(|p| !p.contains("0000")), "{lookups:?}");
}

#[tokio::test]
async fn open_library_drops_restricted_and_lending_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(json(fixture("media_openlibrary.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/metadata/"))
        .respond_with(json(edited("media_openlibrary_ia.json", |v| {
            v["metadata"]["access-restricted-item"] = Value::String("true".into());
        })))
        .mount(&server)
        .await;
    let ol = open_library(&server).await;
    assert!(ol.search("alice", 5).await.unwrap().is_empty());

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(json(fixture("media_openlibrary.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/metadata/"))
        .respond_with(json(edited("media_openlibrary_ia.json", |v| {
            v["metadata"]["collection"] = serde_json::json!(["inlibrary", "americana"]);
        })))
        .mount(&server)
        .await;
    assert!(open_library(&server)
        .await
        .search("alice", 5)
        .await
        .unwrap()
        .is_empty());
    // lookups stay capped even when nothing is found
    let n = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().starts_with("/metadata/"))
        .count();
    assert!(n <= 4, "{n}");
}

#[tokio::test]
async fn open_library_prefers_epub_over_pdf_and_survives_broken_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(json(fixture("media_openlibrary.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/metadata/(.*)carr_14$"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/metadata/"))
        .respond_with(json(edited("media_openlibrary_ia.json", |v| {
            v["files"].as_array_mut().unwrap().push(serde_json::json!({
                "name": "book one.epub", "format": "EPUB", "size": "123", "source": "derivative"
            }));
        })))
        .mount(&server)
        .await;
    let hits = open_library(&server)
        .await
        .search("alice", 1)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(
        hits[0].download_url.ends_with("/book%20one.epub"),
        "{}",
        hits[0].download_url
    );
    assert_eq!(hits[0].size, Some(123));
}

// ----------------------------------------------------------------- LibriVox

#[tokio::test]
async fn librivox_title_prefix_zip_links_and_public_domain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/feed/audiobooks/"))
        .and(query_param("title", "^alice"))
        .and(query_param("format", "json"))
        .respond_with(json(fixture("media_librivox.json")))
        .expect(1)
        .mount(&server)
        .await;
    let hits = LibriVox::with_base(&server.uri(), fast())
        .search_in(Category::Books, "alice", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 3);
    hits.iter().for_each(assert_direct);
    let h = &hits[0];
    assert_eq!(h.title, "Alice's Adventures in Wonderland");
    assert_eq!(
        h.download_url,
        "https://archive.org/compress/alice_in_wonderland_librivox/formats=64KBPS%20MP3&file=/alice_in_wonderland_librivox.zip"
    );
    assert_eq!(
        h.filename.as_deref(),
        Some("alice_in_wonderland_librivox.zip")
    );
    assert_eq!(h.authors, vec!["Lewis Carroll".to_string()]);
    assert_eq!(h.year, Some(1865));
    assert_eq!(h.rights().kind, RightsKind::PublicDomain);
    assert_eq!(
        h.page_url.as_deref(),
        Some("https://librivox.org/alices-adventures-in-wonderland-by-lewis-carroll/")
    );
    // html and line breaks of the description are gone
    assert!(!h.summary.as_deref().unwrap().contains('<'));
    assert!(h.summary.as_deref().unwrap().chars().count() <= 300);
}

#[tokio::test]
async fn librivox_no_match_is_empty_and_a_long_query_retries_with_its_first_word() {
    let server = MockServer::start().await;
    let none = "{\"error\":\"Audiobooks could not be found\"}".to_string();
    Mock::given(method("GET"))
        .and(query_param("title", "^alice in the garden"))
        .respond_with(json(none.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("title", "^alice"))
        .respond_with(json(fixture("media_librivox.json")))
        .expect(1)
        .mount(&server)
        .await;
    let l = LibriVox::with_base(&server.uri(), fast());
    let hits = l.search("alice in the garden", 2).await.unwrap();
    assert_eq!(hits.len(), 2);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json(none))
        .mount(&server)
        .await;
    assert!(LibriVox::with_base(&server.uri(), fast())
        .search("zzz", 5)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn librivox_drops_hostile_links_and_strips_pattern_characters() {
    let server = MockServer::start().await;
    let body = edited("media_librivox.json", |v| {
        let b = v["books"].as_array_mut().unwrap();
        b[0]["url_zip_file"] = Value::String("javascript:alert(1)".into());
        b[1]["url_zip_file"] = Value::String("file:///C:/Windows/win.ini".into());
        b[2]["title"] = Value::String("<b>Bold</b> &amp; Brave".into());
    });
    Mock::given(method("GET"))
        .and(query_param("title", "^alice  "))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("title", "^alice"))
        .respond_with(json(body))
        .mount(&server)
        .await;
    let hits = LibriVox::with_base(&server.uri(), fast())
        .search("alice.*[(", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Bold & Brave");
}

// ------------------------------------------------------------------ Openverse

#[tokio::test]
async fn openverse_images_with_licence_creator_and_landing_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/images/"))
        .and(query_param("q", "cat"))
        .respond_with(json(fixture("media_openverse_images.json")))
        .mount(&server)
        .await;
    let hits = Openverse::with_base(&server.uri(), fast())
        .search_in(Category::Images, "cat", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 3);
    hits.iter().for_each(assert_direct);
    let h = &hits[0];
    assert_eq!(h.title, "Cat Fish 2");
    assert_eq!(h.authors, vec!["admiller".to_string()]);
    assert_eq!(
        h.download_url,
        "https://live.staticflickr.com/3313/3481540500_c846c62863_b.jpg"
    );
    assert_eq!(h.filename.as_deref(), Some("3481540500_c846c62863_b.jpg"));
    assert_eq!(
        h.page_url.as_deref(),
        Some("https://www.flickr.com/photos/32426194@N00/3481540500")
    );
    assert_eq!(
        h.license.as_deref(),
        Some("https://creativecommons.org/licenses/by/2.0/")
    );
    assert_eq!(h.rights().label, "CC BY 2.0");
    assert_eq!(
        Openverse::with_base(&server.uri(), fast())
            .search_in(Category::Images, "cat", 2)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn openverse_audio_licences_and_odd_entries() {
    let server = MockServer::start().await;
    let body = edited("media_openverse_audio.json", |v| {
        let r = v["results"].as_array_mut().unwrap();
        let mut bad = r[0].clone();
        bad["id"] = Value::String("bad-js".into());
        bad["url"] = Value::String("javascript:alert(1)".into());
        let mut mature = r[0].clone();
        mature["id"] = Value::String("mature".into());
        mature["mature"] = Value::Bool(true);
        let mut nourl = r[0].clone();
        nourl["id"] = Value::String("nourl".into());
        nourl["url"] = Value::Null;
        r.extend([bad, mature, nourl]);
    });
    Mock::given(method("GET"))
        .and(path("/v1/audio/"))
        .respond_with(json(body))
        .mount(&server)
        .await;
    let hits = Openverse::with_base(&server.uri(), fast())
        .search_in(Category::Music, "piano", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 3);
    hits.iter().for_each(assert_direct);
    assert_eq!(hits[0].rights().label, "CC BY-SA 3.0");
    assert_eq!(
        hits[0].summary.as_deref(),
        Some("Audio - 3:12 - classical, ambient")
    );
    // html in the title is removed
    assert_eq!(hits[1].title, "Rain on a piano lid");
    assert_eq!(hits[1].rights().kind, RightsKind::PublicDomain);
    assert_eq!(hits[1].size, Some(482_113));
    // no licence URL: the licence code and version are used
    assert_eq!(hits[2].license.as_deref(), Some("Public domain"));
    assert!(hits[2].authors.is_empty());
}

#[tokio::test]
async fn openverse_all_uses_both_indexes_and_survives_one_failing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/images/"))
        .and(query_param("page_size", "2"))
        .respond_with(json(fixture("media_openverse_images.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/audio/"))
        .and(query_param("page_size", "2"))
        .respond_with(json(fixture("media_openverse_audio.json")))
        .expect(1)
        .mount(&server)
        .await;
    let o = Openverse::with_base(&server.uri(), fast());
    let hits = o.search("piano", 4).await.unwrap();
    assert_eq!(hits.len(), 4);
    assert!(hits
        .iter()
        .any(|h| h.download_url.ends_with(".mp3") || h.download_url.contains("jamendo")));
    assert!(hits.iter().any(|h| h.download_url.ends_with(".jpg")));
    // other categories are not served
    assert!(o
        .search_in(Category::Papers, "piano", 4)
        .await
        .unwrap()
        .is_empty());

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/audio/"))
        .respond_with(ResponseTemplate::new(504))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/images/"))
        .respond_with(json(fixture("media_openverse_images.json")))
        .mount(&server)
        .await;
    let hits = Openverse::with_base(&server.uri(), fast())
        .search("piano", 6)
        .await
        .unwrap();
    assert_eq!(hits.len(), 3);
}

#[tokio::test]
async fn openverse_rate_limit_is_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
        .mount(&server)
        .await;
    let err = Openverse::with_base(&server.uri(), fast())
        .search_in(Category::Images, "cat", 5)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResearchError::RateLimited { retry_after: Some(d) } if d.as_secs() == 120),
        "{err:?}"
    );
}

// -------------------------------------------------------------------- Jamendo

#[tokio::test]
async fn jamendo_needs_a_client_id() {
    let server = MockServer::start().await;
    let err = Jamendo::with_base(&server.uri(), "  ", fast())
        .search("piano", 5)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ResearchError::Invalid("Jamendo needs a client id".into())
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn jamendo_only_downloadable_tracks_with_licence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v3.0/tracks/"))
        .and(query_param("client_id", "abc123"))
        .and(query_param("audiodownload_allowed", "true"))
        .and(query_param_contains("search", "piano"))
        .respond_with(json(fixture("media_jamendo.json")))
        .expect(1)
        .mount(&server)
        .await;
    let j = Jamendo::with_base(&server.uri(), " abc123 ", fast());
    let hits = j.search_in(Category::Music, "piano", 10).await.unwrap();
    // the second track does not allow the download
    assert_eq!(hits.len(), 2);
    hits.iter().for_each(assert_direct);
    let h = &hits[0];
    assert_eq!(h.id, "jamendo:1532771");
    assert_eq!(h.title, "Rain & Piano");
    assert_eq!(h.authors, vec!["Calm Composer".to_string()]);
    assert_eq!(
        h.download_url,
        "https://prod-1.storage.jamendo.com/download/track/1532771/mp32/"
    );
    assert_eq!(h.rights().label, "CC BY-NC-SA 3.0");
    assert_eq!(h.year, Some(2019));
    assert_eq!(
        h.page_url.as_deref(),
        Some("https://www.jamendo.com/track/1532771")
    );
    // a name with separators and quotes makes a safe file name
    let name = hits[1].filename.as_deref().unwrap();
    assert!(name.ends_with(".mp3"));
    assert!(!name.contains(['/', '\\', '"', ':', '*', '?']), "{name}");
    // other categories: nothing, and no request for them
    assert!(j
        .search_in(Category::Books, "piano", 5)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn jamendo_failure_status_and_hostile_links() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json(
            "{\"headers\":{\"status\":\"failed\",\"code\":5,\"error_message\":\"Invalid client_id\"},\"results\":[]}"
                .into(),
        ))
        .mount(&server)
        .await;
    let err = Jamendo::with_base(&server.uri(), "nope", fast())
        .search("piano", 5)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::Parse(_)), "{err:?}");
    assert!(!err.to_string().contains("nope"));

    let server = MockServer::start().await;
    let body = edited("media_jamendo.json", |v| {
        v["results"][0]["audiodownload"] = Value::String("javascript:alert(1)".into());
        v["results"][2]["audiodownload"] = Value::String("ftp://x/y.mp3".into());
        v["results"][2]["license_ccurl"] = Value::String("javascript:1".into());
    });
    Mock::given(method("GET"))
        .respond_with(json(body))
        .mount(&server)
        .await;
    let hits = Jamendo::with_base(&server.uri(), "k", fast())
        .search("piano", 5)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

// -------------------------------------------------------------------- Blender

#[tokio::test]
async fn blender_catalogue_matches_words_and_has_sane_links() {
    let b = Blender::new();
    let all = b.search("blender open movie", 50).await.unwrap();
    assert!(all.len() >= 5);
    for h in &all {
        assert!(h.download_url.starts_with("https://download.blender.org/"));
        assert!(!h.download_url.contains(' '));
        let name = h.filename.as_deref().unwrap();
        assert!(
            !name.is_empty() && !name.contains(['/', '\\', ' ']),
            "{name}"
        );
        assert!(h.download_url.ends_with(name));
        assert!(h.size.unwrap() > 10_000_000);
        assert_eq!(h.source, Source::Blender);
        assert_eq!(h.rights().kind, RightsKind::Cc);
        assert!(h.rights().label.starts_with("CC BY"));
        assert!(h.page_url.as_deref().unwrap().starts_with("https://"));
        assert!(h.year.is_some());
    }
    let ids: std::collections::HashSet<_> = all.iter().map(|h| h.id.clone()).collect();
    assert_eq!(ids.len(), all.len());
    let bunny = b.search("BUNNY", 5).await.unwrap();
    assert_eq!(bunny.len(), 1);
    assert!(bunny[0].title.starts_with("Big Buck Bunny"));
    assert_eq!(b.search("sintel 2010", 5).await.unwrap().len(), 1);
    assert!(b.search("nothing like this", 5).await.unwrap().is_empty());
    assert!(b.search("bunny sintel", 5).await.unwrap().is_empty());
    assert_eq!(b.search("blender", 2).await.unwrap().len(), 2);
    // films are video; no hits for other kinds
    assert!(b
        .search_in(Category::Music, "bunny", 5)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        b.search_in(Category::Video, "bunny", 5)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        b.search("  ", 5).await,
        Err(ResearchError::Invalid(_))
    ));
    // the host is replaceable for tests
    let local = Blender::with_base("http://127.0.0.1:9/");
    assert!(local.search("bunny", 1).await.unwrap()[0]
        .download_url
        .starts_with("http://127.0.0.1:9/peach/"));
}

// ----------------------------------------------------------- Internet Archive

async fn ia_server(search: &str, id: &str, meta: String) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/advancedsearch.php"))
        .and(query_param_contains("q", search))
        .respond_with(json(fixture(if search.contains("audio") {
            "media_ia_audio_search.json"
        } else {
            "media_ia_video_search.json"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/metadata/{id}")))
        .respond_with(json(meta))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn internet_archive_music_offers_an_album_as_one_zip() {
    let server = ia_server(
        "mediatype:audio",
        "DWK123",
        fixture("media_ia_audio_meta.json"),
    )
    .await;
    let hits = InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap();
    // the other documents have no metadata mock (404): they are skipped
    assert_eq!(hits.len(), 1);
    let h = &hits[0];
    assert_direct(h);
    assert!(
        h.title.starts_with("Curses From Past Times (LP) ("),
        "{}",
        h.title
    );
    assert!(h.title.ends_with(" tracks)"));
    assert!(
        h.download_url
            .ends_with("/compress/DWK123/formats=VBR%20MP3&file=/DWK123.zip"),
        "{}",
        h.download_url
    );
    assert_eq!(h.filename.as_deref(), Some("DWK123.zip"));
    assert!(h.size.unwrap() > 1_000_000);
    assert_eq!(h.rights().label, "CC BY-NC-ND 3.0");
    assert_eq!(h.authors, vec!["ProleteR".to_string()]);
    assert_eq!(h.year, Some(2012));
    // the search asked for audio under the licence/collection rule
    let reqs = server.received_requests().await.unwrap();
    let q = reqs[0]
        .url
        .query_pairs()
        .find(|(k, _)| k == "q")
        .unwrap()
        .1
        .to_string();
    assert!(q.contains("netlabels") && q.contains("etree"), "{q}");
    assert!(q.contains("-access-restricted-item:true"));
}

#[tokio::test]
async fn internet_archive_music_single_track_is_direct_and_rules_apply() {
    // one VBR MP3 only: a direct file link
    let single = edited("media_ia_audio_meta.json", |v| {
        let files = v["files"].as_array_mut().unwrap();
        let mut seen = false;
        files.retain(|f| {
            if f["format"] == "VBR MP3" {
                let keep = !seen;
                seen = true;
                keep
            } else {
                true
            }
        });
    });
    let server = ia_server("mediatype:audio", "DWK123", single).await;
    let hits = InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Curses From Past Times (LP)");
    assert!(hits[0]
        .download_url
        .ends_with("/download/DWK123/ProleteR_-_01_-_April_Showers.mp3"));
    assert_eq!(hits[0].size, Some(10_764_625));

    // not CC, not in a netlabel/etree collection: out
    let unlicensed = edited("media_ia_audio_meta.json", |v| {
        v["metadata"].as_object_mut().unwrap().remove("licenseurl");
        v["metadata"]["collection"] = serde_json::json!(["opensource_audio", "community"]);
    });
    let server = ia_server("mediatype:audio", "DWK123", unlicensed).await;
    assert!(InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap()
        .is_empty());

    // a netlabel item without a licence URL is fine, but no licence is claimed
    let netlabel = edited("media_ia_audio_meta.json", |v| {
        v["metadata"].as_object_mut().unwrap().remove("licenseurl");
    });
    let server = ia_server("mediatype:audio", "DWK123", netlabel).await;
    let hits = InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].license, None);

    // access-restricted: out
    let restricted = edited("media_ia_audio_meta.json", |v| {
        v["metadata"]["access-restricted-item"] = Value::String("true".into());
    });
    let server = ia_server("mediatype:audio", "DWK123", restricted).await;
    assert!(InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn internet_archive_music_skips_huge_collections() {
    let many = edited("media_ia_audio_meta.json", |v| {
        let files = v["files"].as_array_mut().unwrap();
        for n in 0..70 {
            files.push(serde_json::json!({
                "name": format!("t{n}.mp3"), "format": "VBR MP3", "size": "1000", "source": "original"
            }));
        }
    });
    let server = ia_server("mediatype:audio", "DWK123", many).await;
    let ia = InternetArchive::with_base(&server.uri(), fast());
    assert!(ia
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap()
        .is_empty());
    let big = edited("media_ia_audio_meta.json", |v| {
        for f in v["files"].as_array_mut().unwrap() {
            if f["format"] == "VBR MP3" {
                f["size"] = Value::String("900000000".into());
            }
        }
    });
    let server = ia_server("mediatype:audio", "DWK123", big).await;
    let ia = InternetArchive::with_base(&server.uri(), fast());
    assert!(ia
        .search_in(Category::Music, "jazz", 5)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn internet_archive_video_picks_the_mp4_of_public_domain_films() {
    let server = ia_server(
        "mediatype:movies",
        "FromtheG1954",
        fixture("media_ia_video_meta.json"),
    )
    .await;
    let hits = InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Video, "night", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    let h = &hits[0];
    assert_direct(h);
    assert!(h
        .download_url
        .ends_with("/download/FromtheG1954/FromtheG1954.mp4"));
    assert_eq!(h.filename.as_deref(), Some("FromtheG1954.mp4"));
    assert_eq!(h.size, Some(119_087_429));
    assert_eq!(h.year, Some(1954));
    assert_eq!(h.rights().kind, RightsKind::PublicDomain);
    let reqs = server.received_requests().await.unwrap();
    let q = reqs[0]
        .url
        .query_pairs()
        .find(|(k, _)| k == "q")
        .unwrap()
        .1
        .to_string();
    assert!(q.contains("prelinger") && q.contains("silent_films"), "{q}");
    assert!(!q.contains("classic_tv"));

    // no licence URL: only the public-domain collections qualify
    let no_url = |cols: Value| {
        edited("media_ia_video_meta.json", move |v| {
            v["metadata"].as_object_mut().unwrap().remove("licenseurl");
            v["metadata"]["collection"] = cols;
        })
    };
    for (cols, expect_hit, licence) in [
        (serde_json::json!("prelinger"), true, Some("Public domain")),
        (
            serde_json::json!(["silent_films"]),
            true,
            Some("Public domain"),
        ),
        (serde_json::json!(["feature_films"]), true, None),
        (serde_json::json!(["classic_tv"]), false, None),
        (serde_json::json!(["opensource_movies"]), false, None),
    ] {
        let server = ia_server("mediatype:movies", "FromtheG1954", no_url(cols.clone())).await;
        let hits = InternetArchive::with_base(&server.uri(), fast())
            .search_in(Category::Video, "night", 5)
            .await
            .unwrap();
        assert_eq!(!hits.is_empty(), expect_hit, "{cols}");
        if expect_hit {
            assert_eq!(hits[0].license.as_deref(), licence, "{cols}");
        }
    }

    // no MP4 at all: not offered
    let no_mp4 = edited("media_ia_video_meta.json", |v| {
        v["files"]
            .as_array_mut()
            .unwrap()
            .retain(|f| !f["name"].as_str().unwrap().ends_with(".mp4"));
    });
    let server = ia_server("mediatype:movies", "FromtheG1954", no_mp4).await;
    assert!(InternetArchive::with_base(&server.uri(), fast())
        .search_in(Category::Video, "night", 5)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn internet_archive_books_and_all_keep_the_text_search() {
    for category in [Category::All, Category::Books, Category::Papers] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/advancedsearch.php"))
            .and(query_param_contains("q", "mediatype:texts"))
            .respond_with(json(fixture("ia_search.json")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/metadata/"))
            .respond_with(json(fixture("ia_meta2.json")))
            .mount(&server)
            .await;
        let hits = InternetArchive::with_base(&server.uri(), fast())
            .search_in(category, "relativity", 3)
            .await
            .unwrap();
        assert!(!hits.is_empty(), "{category:?}");
        assert!(hits.iter().all(|h| h.download_url.ends_with(".pdf")));
    }
}

// -------------------------------------------------------------------- Commons

#[tokio::test]
async fn commons_filters_by_file_type_per_category() {
    for (category, term) in [
        (Category::Music, "filetype:audio piano"),
        (Category::Video, "filetype:video piano"),
        (Category::Images, "filetype:bitmap piano"),
        (Category::All, "piano"),
        (Category::Papers, "piano"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .and(query_param("gsrsearch", term))
            .respond_with(json(fixture("media_commons_audio.json")))
            .expect(1)
            .mount(&server)
            .await;
        let hits = Commons::with_base(&server.uri(), fast())
            .search_in(category, "piano", 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 4, "{category:?}");
        hits.iter().for_each(assert_direct);
        assert!(hits.iter().all(|h| h.license.is_some()));
        assert!(hits.iter().any(|h| h.download_url.contains(".ogg")
            || h.download_url.contains(".oga")
            || h.download_url.contains(".mp3")
            || h.download_url.contains(".flac")
            || h.download_url.contains(".wav")));
    }
}
