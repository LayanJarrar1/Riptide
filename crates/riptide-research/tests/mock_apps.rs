//! The game/software/media/web adapters (GitHub games, OpenGameArt, NASA, web search) against a
//! mock server that replays responses recorded from the real services (see
//! `tests/fixtures/apps_*`, recorded 2026-10-20; `apps_brave.json` is hand-built from the
//! documentation, see its `_note`). No network.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_research::http::{Http, Tuning, USER_AGENT};
use riptide_research::sources::websearch::{validate_searxng_base, WebBackend, WebSearch};
use riptide_research::sources::{
    github::Github, nasa::Nasa, opengameart::OpenGameArt, SearchSource,
};
use riptide_research::{Category, HitKind, ResearchError, RightsKind, Source};
use wiremock::matchers::{header, header_regex, method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CANARY: &str = "CANARY-secret-token-9f3a7c1e";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
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

fn html(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/html; charset=utf-8")
        .set_body_string(body)
}

async fn paths_requested(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect()
}

// ---------------------------------------------------------------------------------- GitHub games

#[tokio::test]
async fn github_games_only_offer_repositories_with_an_open_licence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "platformer topic:game"))
        .and(header_regex("user-agent", "^Riptide/"))
        .respond_with(json(fixture("apps_gh_games.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/drwhut/tabletop-club/releases/latest"))
        .respond_with(json(fixture("apps_gh_release_tabletop.json")))
        .mount(&server)
        .await;
    // the release of every other repository is a 404 (no release)
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/.*/releases/latest$"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let hits = Github::with_base(&server.uri(), fast())
        .search_in(Category::Games, "platformer", 10)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits.len() <= 2, "at most two assets per release");
    for h in &hits {
        assert_eq!(h.source, Source::Github);
        assert!(h.download_url.starts_with("https://github.com/drwhut/"));
        assert_eq!(h.license.as_deref(), Some("MIT"));
        assert_eq!(h.rights().kind, RightsKind::OpenSource);
        assert!(h.page_url.as_deref().unwrap().contains("tabletop-club"));
        assert!(h.size.is_some());
        assert!(h.filename.as_deref().unwrap().ends_with(".zip"));
    }
    // repositories without an OSI licence (NOASSERTION / none) are never even looked at
    let asked = paths_requested(&server).await;
    for banned in ["4ian/GDevelop", "leereilly/games", "ares-emulator/ares"] {
        assert!(
            !asked.iter().any(|p| p.contains(banned)),
            "{banned} was queried: {asked:?}"
        );
    }
    // few requests: the search plus at most four releases
    assert!(asked.len() <= 5, "{asked:?}");
}

#[tokio::test]
async fn github_games_fall_back_to_the_games_topic_when_the_first_search_is_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "rare topic:game"))
        .respond_with(json(r#"{"total_count":0,"items":[]}"#.into()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "rare topic:games"))
        .respond_with(json(fixture("apps_gh_games.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/drwhut/tabletop-club/releases/latest"))
        .respond_with(json(fixture("apps_gh_release_tabletop.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/.*/releases/latest$"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let hits = Github::with_base(&server.uri(), fast())
        .search_in(Category::Games, "rare", 3)
        .await
        .unwrap();
    assert!(!hits.is_empty() && hits.len() <= 3);
}

#[tokio::test]
async fn github_games_surface_the_rate_limit_as_a_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "60"))
        .mount(&server)
        .await;
    let err = Github::with_base(&server.uri(), fast())
        .search_in(Category::Games, "platformer", 5)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResearchError::RateLimited { retry_after: Some(d) } if d.as_secs() == 60),
        "{err:?}"
    );
}

#[tokio::test]
async fn github_software_search_keeps_its_plain_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "download manager"))
        .respond_with(json(r#"{"items":[]}"#.into()))
        .expect(1)
        .mount(&server)
        .await;
    let hits = Github::with_base(&server.uri(), fast())
        .search_in(Category::Software, "download manager", 5)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

// ------------------------------------------------------------------------------------ OpenGameArt

/// A search page in the shape of the real one, listing `paths`.
fn oga_search_page(paths: &[&str]) -> String {
    let rows: String = paths
        .iter()
        .map(|p| {
            format!(
                r#"<div class="views-row art-previews-inline"><div class="field-item even" property="dc:title"><span class="art-preview-title"><a href="{p}">Title of {p}</a></span></div></div>"#
            )
        })
        .collect();
    format!(
        r#"<html><head><title>Search Art | OpenGameArt.org</title></head><body><div class="view-content">{rows}</div></body></html>"#
    )
}

/// An art page in the shape of the real one.
fn oga_art_page(licenses: &[&str], files: &[(&str, &str, &str)]) -> String {
    let lic: String = licenses
        .iter()
        .map(|l| format!("<div class='license-icon'><a href='https://example.org/'><div class='license-name'>{l}</div></a></div>"))
        .collect();
    let f: String = files
        .iter()
        .map(|(href, ty, name)| {
            format!(
                r#"<div class="field-item even"><span class="file"><a href="{href}" type="{ty}" data-fid="1" target="_blank" download="{name}">{name}</a> 1 Kb</span></div>"#
            )
        })
        .collect();
    format!(
        r#"<html><head><title>x | OpenGameArt.org</title></head><body>
<div class="field field-name-field-art-licenses field-type-taxonomy-term-reference"><div class="field-items">{lic}</div></div>
<div class="field field-name-field-art-files field-type-file"><div class="field-items">{f}</div></div><ul class="links inline"><li>Log in</li></ul></body></html>"#
    )
}

#[tokio::test]
async fn opengameart_search_then_art_page_gives_direct_files_with_licence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .and(query_param("keys", "tree"))
        .and(header_regex("user-agent", "^Riptide/"))
        .respond_with(html(fixture("apps_oga_search.html")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/content/tiny-16-basic"))
        .respond_with(html(fixture("apps_oga_art.html")))
        .expect(1)
        .mount(&server)
        .await;
    // the two results before it do not answer: they are skipped, not fatal
    Mock::given(method("GET"))
        .and(path_regex(r"^/content/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let hits = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::All, "tree", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2, "{hits:#?}");
    let h = &hits[0];
    assert_eq!(h.source, Source::OpenGameArt);
    assert_eq!(h.kind, HitKind::File);
    assert!(h.title.starts_with("Tiny 16: Basic - "), "{}", h.title);
    assert!(h
        .download_url
        .starts_with("https://opengameart.org/sites/default/files/"));
    assert!(h.size.is_some_and(|s| s > 0));
    assert!(h.filename.is_some());
    assert_eq!(h.license.as_deref(), Some("CC-BY 4.0"));
    assert_eq!(h.rights().kind, RightsKind::Cc);
    assert_eq!(h.authors, vec!["Sharm".to_string()]);
    assert_eq!(h.year, Some(2013));
    assert_eq!(
        h.page_url.as_deref(),
        Some(format!("{}/content/tiny-16-basic", server.uri()).as_str())
    );
    let summary = h.summary.as_deref().unwrap();
    assert!(summary.contains("16x16 tiles"), "{summary}");
    assert!(summary.starts_with("Licences: CC-BY 4.0, CC-BY 3.0, OGA-BY 3.0. "));
    assert!(!summary.contains('<'));
    // only the paths robots.txt allows: the advanced search and art pages
    for p in paths_requested(&server).await {
        assert!(
            p == "/art-search-advanced" || p.starts_with("/content/"),
            "{p}"
        );
    }
}

#[tokio::test]
async fn opengameart_music_uses_the_music_types_and_prefers_the_audio_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .and(query_param("field_art_type_tid[]", "12"))
        .respond_with(html(oga_search_page(&["/content/the-field-of-dreams"])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/content/the-field-of-dreams"))
        .respond_with(html(fixture("apps_oga_music.html")))
        .mount(&server)
        .await;
    let hits = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::Music, "dreams", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].filename.as_deref(), Some("the_field_of_dreams.mp3"));
    assert_eq!(hits[0].size, Some(3_385_581));
    assert_eq!(hits[0].license.as_deref(), Some("CC0"));
    assert_eq!(hits[0].rights().kind, RightsKind::PublicDomain);
    assert_eq!(hits[1].filename.as_deref(), Some("source.7z"));
}

#[tokio::test]
async fn opengameart_games_ask_for_art_types_not_music() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .and(query_param("field_art_type_tid[]", "9"))
        .respond_with(html(oga_search_page(&[])))
        .expect(1)
        .mount(&server)
        .await;
    let hits = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::Games, "castle", 5)
        .await
        .unwrap();
    assert!(hits.is_empty());
    let q = server.received_requests().await.unwrap()[0]
        .url
        .query()
        .unwrap()
        .to_string();
    assert!(!q.contains("tid%5B%5D=12"), "{q}");
}

#[tokio::test]
async fn opengameart_respects_the_limit_and_opens_few_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .respond_with(html(fixture("apps_oga_search.html")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/content/"))
        .respond_with(html(fixture("apps_oga_art.html")))
        .mount(&server)
        .await;
    let hits = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::All, "tree", 1)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    // search + one art page: the limit is reached after the first page
    assert_eq!(paths_requested(&server).await.len(), 2);

    let all = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::All, "tree", 50)
        .await
        .unwrap();
    // never more than three art pages (two files each)
    assert!(all.len() <= 6, "{}", all.len());
}

#[tokio::test]
async fn opengameart_drops_hostile_links_names_and_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .respond_with(html(oga_search_page(&[
            "/content/evil",
            "/content/../../etc/passwd",
            "/content/evil",
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/content/evil"))
        .respond_with(html(oga_art_page(
            &["CC0 <script>x</script>"],
            &[
                ("javascript:alert(1)", "text/plain; length=5", "a.txt"),
                (
                    "https://evil.example/sites/default/files/x.zip",
                    "application/zip; length=5",
                    "b.zip",
                ),
                (
                    "https://opengameart.org/sites/default/files/../../../x.zip",
                    "application/zip; length=5",
                    "c.zip",
                ),
                (
                    "file:///C:/Windows/system.ini",
                    "text/plain; length=5",
                    "d.ini",
                ),
                (
                    "https://opengameart.org/sites/default/files/ok.zip",
                    "application/zip; length=0",
                    "../../evil&#039;s.zip",
                ),
            ],
        )))
        .mount(&server)
        .await;
    let hits = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::All, "evil", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "{hits:#?}");
    let h = &hits[0];
    assert_eq!(
        h.download_url,
        "https://opengameart.org/sites/default/files/ok.zip"
    );
    // a zero length is "unknown", the name has no directory parts and the entity is decoded
    assert_eq!(h.size, None);
    assert_eq!(h.filename.as_deref(), Some(".._.._evil's.zip"));
    assert!(!h.filename.as_deref().unwrap().contains('/'));
    assert_eq!(h.license.as_deref(), Some("CC0 x"));
    let asked = paths_requested(&server).await;
    assert!(!asked.iter().any(|p| p.contains("passwd")), "{asked:?}");
}

#[tokio::test]
async fn opengameart_changed_markup_is_a_parse_error_not_a_panic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .respond_with(html(
            "<html><body>Down for maintenance</body></html>".into(),
        ))
        .mount(&server)
        .await;
    let err = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::All, "tree", 5)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::Parse(_)), "{err:?}");

    // a search page that lists art pages whose markup is unrecognisable
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .respond_with(html(oga_search_page(&["/content/a", "/content/b"])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/content/"))
        .respond_with(html("<html><body>new design</body></html>".into()))
        .mount(&server)
        .await;
    let err = OpenGameArt::with_base(&server.uri(), fast())
        .search_in(Category::All, "tree", 5)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::Parse(_)), "{err:?}");
}

#[tokio::test]
async fn opengameart_errors_are_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art-search-advanced"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let a = OpenGameArt::with_base(&server.uri(), fast());
    assert_eq!(
        a.search_in(Category::All, "tree", 5).await.unwrap_err(),
        ResearchError::Http { status: 503 }
    );
    assert!(matches!(
        a.search_in(Category::All, "  ", 5).await.unwrap_err(),
        ResearchError::Invalid(_)
    ));
}

// ------------------------------------------------------------------------------------------ NASA

fn nasa_fixture(name: &str, server: &MockServer) -> String {
    fixture(name).replace("https://images-assets.nasa.gov", &server.uri())
}

fn hrefs(body: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    v["collection"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["href"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn nasa_images_use_the_original_listed_in_the_search_answer() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "apollo 11"))
        .and(query_param("media_type", "image"))
        .and(header_regex("user-agent", "^Riptide/"))
        .respond_with(json(nasa_fixture("apps_nasa_images.json", &server)))
        .expect(2)
        .mount(&server)
        .await;
    let hits = Nasa::with_base(&server.uri(), fast())
        .search_in(Category::Images, "apollo 11", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 5);
    let h = &hits[0];
    assert_eq!(h.source, Source::Nasa);
    assert_eq!(h.title, "Apollo 11 spacecraft pre-launch");
    assert!(h.download_url.ends_with("/jsc2007e034221~orig.jpg"));
    assert_eq!(h.size, Some(1_402_000));
    assert_eq!(h.year, Some(1969));
    assert_eq!(h.filename.as_deref(), Some("jsc2007e034221_orig.jpg"));
    assert_eq!(
        h.page_url.as_deref(),
        Some("https://images.nasa.gov/details/jsc2007e034221")
    );
    assert_eq!(
        h.license.as_deref(),
        Some("Public domain (NASA media usage guidelines apply)")
    );
    assert_eq!(h.rights().kind, RightsKind::PublicDomain);
    // the other four originals are TIFF: their large JPEG renditions are offered instead
    assert!(hits[1..]
        .iter()
        .all(|h| h.download_url.ends_with("~large.jpg")));
    assert!(hits.iter().all(|h| h.size.is_some()));
    assert!(h
        .summary
        .as_deref()
        .unwrap()
        .contains("Mobile Service Structure"));
    // no collection.json requests were needed
    assert_eq!(paths_requested(&server).await, vec!["/search".to_string()]);

    let few = Nasa::with_base(&server.uri(), fast())
        .search_in(Category::All, "apollo 11", 2)
        .await
        .unwrap();
    assert_eq!(few.len(), 2);
}

#[tokio::test]
async fn nasa_images_fall_back_to_collection_json_and_upgrade_to_https() {
    let server = MockServer::start().await;
    // drop the renditions from the search answer so the collection listing is needed
    let mut v: serde_json::Value =
        serde_json::from_str(&nasa_fixture("apps_nasa_images.json", &server)).unwrap();
    for it in v["collection"]["items"].as_array_mut().unwrap() {
        it.as_object_mut().unwrap().remove("links");
    }
    let hrefs = hrefs(&v.to_string());
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(json(v.to_string()))
        .mount(&server)
        .await;
    let first = url_path(&hrefs[0]);
    Mock::given(method("GET"))
        .and(path(first))
        .respond_with(json(fixture("apps_nasa_coll_image.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/collection\.json$"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let hits = Nasa::with_base(&server.uri(), fast())
        .search_in(Category::Images, "apollo", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "the other items have no listing: skipped");
    assert_eq!(
        hits[0].download_url,
        "https://images-assets.nasa.gov/image/jsc2007e034221/jsc2007e034221~orig.jpg"
    );
    assert_eq!(hits[0].size, None);
}

fn url_path(u: &str) -> String {
    url::Url::parse(u).unwrap().path().to_string()
}

#[tokio::test]
async fn nasa_video_picks_medium_then_original_mp4() {
    let server = MockServer::start().await;
    let body = nasa_fixture("apps_nasa_videos.json", &server);
    let hrefs = hrefs(&body);
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("media_type", "video"))
        .respond_with(json(body))
        .expect(1)
        .mount(&server)
        .await;
    // item 1: the recorded listing (has orig, mobile, small - no medium)
    Mock::given(method("GET"))
        .and(path(url_path(&hrefs[0])))
        .respond_with(json(fixture("apps_nasa_coll_video.json")))
        .mount(&server)
        .await;
    // item 2: a medium rendition exists, and a name with a space
    Mock::given(method("GET"))
        .and(path(url_path(&hrefs[1])))
        .respond_with(json(
            r#"["http://images-assets.nasa.gov/video/a b/a b~orig.mp4","http://images-assets.nasa.gov/video/a b/a b~medium.mp4","http://images-assets.nasa.gov/video/a b/a b.vtt","javascript:alert(1)"]"#
                .into(),
        ))
        .mount(&server)
        .await;
    // item 3: nothing downloadable
    Mock::given(method("GET"))
        .and(path(url_path(&hrefs[2])))
        .respond_with(json(
            r#"["http://images-assets.nasa.gov/video/c/c.vtt"]"#.into(),
        ))
        .mount(&server)
        .await;
    let hits = Nasa::with_base(&server.uri(), fast())
        .search_in(Category::Video, "apollo 11", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2, "{hits:#?}");
    assert!(hits[0].download_url.ends_with("~orig.mp4"));
    assert!(hits[0]
        .download_url
        .starts_with("https://images-assets.nasa.gov/"));
    assert_eq!(
        hits[1].download_url,
        "https://images-assets.nasa.gov/video/a%20b/a%20b~medium.mp4"
    );
    assert_eq!(hits[1].filename.as_deref(), Some("a b_medium.mp4"));
    assert!(hits
        .iter()
        .all(|h| h.license.as_deref().unwrap().contains("Public domain")));
}

#[tokio::test]
async fn nasa_drops_hostile_items_and_maps_errors() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"collection":{"items":[
        {"href":"javascript:alert(1)","data":[{"nasa_id":"ok1","title":"bad link","media_type":"image"}]},
        {"href":format!("{}/image/x/collection.json", server.uri()),"data":[{"nasa_id":"../../etc","title":"bad id","media_type":"image"}]},
        {"href":format!("{}/image/y/collection.json", server.uri()),"data":[{"nasa_id":"y","title":"  <b>Bold</b>\n  title ","description":"<script>x</script>Fine &amp; dandy","date_created":"2001-02-03T00:00:00Z","media_type":"image"}],
         "links":[{"href":"javascript:alert(2)","rel":"canonical","render":"image","size":5}]},
        {"href":format!("{}/image/z/collection.json", server.uri()),"data":[{"nasa_id":"z","title":"","media_type":"image"}]}
    ]}});
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(json(body.to_string()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/image/y/collection.json"))
        .respond_with(json(
            r#"["http://images-assets.nasa.gov/image/y/y~orig.tif","http://images-assets.nasa.gov/image/y/y~large.jpg"]"#.into(),
        ))
        .mount(&server)
        .await;
    let hits = Nasa::with_base(&server.uri(), fast())
        .search_in(Category::Images, "x", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "{hits:#?}");
    assert_eq!(hits[0].title, "Bold title");
    assert_eq!(hits[0].summary.as_deref(), Some("xFine & dandy"));
    assert!(hits[0].download_url.ends_with("y~large.jpg"));

    // empty answer, missing structure, rate limit, server error
    for (status, body, expect) in [
        (200, r#"{"collection":{"items":[]}}"#, None),
        (
            200,
            r#"{"nope":1}"#,
            Some(ResearchError::Parse("no collection.items field".into())),
        ),
        (500, "", Some(ResearchError::Http { status: 500 })),
    ] {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&s)
            .await;
        let r = Nasa::with_base(&s.uri(), fast())
            .search_in(Category::Images, "x", 5)
            .await;
        match expect {
            None => assert!(r.unwrap().is_empty()),
            Some(e) => assert_eq!(r.unwrap_err(), e),
        }
    }
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
        .mount(&s)
        .await;
    assert!(matches!(
        Nasa::with_base(&s.uri(), fast())
            .search_in(Category::Images, "x", 5)
            .await
            .unwrap_err(),
        ResearchError::RateLimited { .. }
    ));
}

// ------------------------------------------------------------------------------------ web search

#[tokio::test]
async fn brave_results_are_pages_with_clean_text_and_the_token_only_in_the_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/res/v1/web/search"))
        .and(query_param("q", "open source download manager"))
        .and(query_param("count", "10"))
        .and(header("x-subscription-token", CANARY))
        .and(header_regex("user-agent", "^Riptide/"))
        .respond_with(json(fixture("apps_brave.json")))
        .expect(1)
        .mount(&server)
        .await;
    let ws = WebSearch::with_brave_base(&server.uri(), CANARY, fast());
    let hits = ws
        .search_in(Category::Web, "open source download manager", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 3, "{hits:#?}");
    assert_eq!(hits[0].title, "Free Download Manager & Accelerator");
    assert_eq!(
        hits[0].summary.as_deref(),
        Some("A download manager that splits files into segments. Free & open.")
    );
    assert_eq!(hits[0].download_url, "https://www.example.org/fdm/");
    assert_eq!(hits[1].download_url, "https://aria2.github.io/");
    assert_eq!(hits[2].title, "untitled.example.net");
    for h in &hits {
        assert_eq!(h.kind, HitKind::Page);
        assert_eq!(h.source, Source::Web);
        assert_eq!(h.license, None);
        assert_eq!(h.page_url.as_deref(), Some(h.download_url.as_str()));
        assert!(h.id.starts_with("web:"));
    }
    // the secret is never in a URL
    for r in server.received_requests().await.unwrap() {
        assert!(!r.url.as_str().contains(CANARY));
    }
}

#[tokio::test]
async fn brave_limit_is_sent_as_count_and_applied() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/res/v1/web/search"))
        .and(query_param("count", "2"))
        .respond_with(json(fixture("apps_brave.json")))
        .expect(1)
        .mount(&server)
        .await;
    let hits = WebSearch::with_brave_base(&server.uri(), CANARY, fast())
        .search_in(Category::Web, "x", 2)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[tokio::test]
async fn brave_secret_never_appears_in_errors_or_debug_output() {
    let backend = WebBackend::Brave {
        api_key: CANARY.into(),
    };
    assert!(!format!("{backend:?}").contains(CANARY));
    assert!(!format!("{backend:#?}").contains(CANARY));
    let server = MockServer::start().await;
    let ws = WebSearch::with_http(backend, &server.uri(), fast());
    assert!(!format!("{ws:?}").contains(CANARY));

    for (status, body) in [(401u16, "invalid CANARY"), (403, ""), (422, ""), (500, "")] {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&s)
            .await;
        let err = WebSearch::with_brave_base(&s.uri(), CANARY, fast())
            .search_in(Category::Web, "x", 5)
            .await
            .unwrap_err();
        assert_eq!(err, ResearchError::Http { status }, "status {status}");
        assert!(!format!("{err} {err:?}").contains(CANARY));
    }
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
        .mount(&s)
        .await;
    let err = WebSearch::with_brave_base(&s.uri(), CANARY, fast())
        .search_in(Category::Web, "x", 5)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::RateLimited { .. }));
    assert!(!format!("{err} {err:?}").contains(CANARY));

    // a token that cannot be a header value is refused before any request, without echoing it
    for bad in [format!("{CANARY}\r\nX-Evil: 1"), String::new(), "  ".into()] {
        let err = WebSearch::with_brave_base(&server.uri(), &bad, fast())
            .search_in(Category::Web, "x", 5)
            .await
            .unwrap_err();
        assert!(matches!(err, ResearchError::Invalid(_)), "{err:?}");
        assert!(!format!("{err} {err:?}").contains(CANARY));
    }
    assert!(server.received_requests().await.unwrap().is_empty());

    // connection failure: the error text is fixed
    let err = WebSearch::with_brave_base("http://127.0.0.1:1", CANARY, fast())
        .search_in(Category::Web, "x", 5)
        .await
        .unwrap_err();
    assert!(!format!("{err} {err:?}").contains(CANARY), "{err:?}");
}

#[tokio::test]
async fn brave_empty_and_unexpected_answers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("q", "nothing"))
        .respond_with(json(
            r#"{"type":"search","query":{"original":"nothing"}}"#.into(),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("q", "weird"))
        .respond_with(json(r#"{"hello":"world"}"#.into()))
        .mount(&server)
        .await;
    let ws = WebSearch::with_brave_base(&server.uri(), CANARY, fast());
    assert!(ws
        .search_in(Category::Web, "nothing", 5)
        .await
        .unwrap()
        .is_empty());
    assert!(matches!(
        ws.search_in(Category::Web, "weird", 5).await.unwrap_err(),
        ResearchError::Parse(_)
    ));
}

#[tokio::test]
async fn searxng_json_results_become_page_hits() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "riptide download manager"))
        .and(query_param("format", "json"))
        .and(header_regex("user-agent", "^Riptide/"))
        .respond_with(json(fixture("apps_searx.json")))
        .expect(2)
        .mount(&server)
        .await;
    let ws = WebSearch::with_searx(&server.uri(), fast());
    let hits = ws
        .search_in(Category::Web, "riptide download manager", 50)
        .await
        .unwrap();
    assert_eq!(hits.len(), 8);
    assert_eq!(hits[0].download_url, "https://munapp.pages.dev/");
    assert_eq!(hits[0].kind, HitKind::Page);
    assert!(hits[0]
        .summary
        .as_deref()
        .unwrap()
        .contains("download manager"));
    assert!(hits.iter().all(|h| h.license.is_none()));
    assert!(hits
        .iter()
        .all(|h| h.download_url.starts_with("http") && !h.title.is_empty()));
    let three = ws
        .search_in(Category::Web, "riptide download manager", 3)
        .await
        .unwrap();
    assert_eq!(three.len(), 3);
    assert_eq!(three[2].id, hits[2].id);
}

#[tokio::test]
async fn searxng_json_disabled_is_a_clear_403_and_html_is_a_parse_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("q", "closed"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("q", "html"))
        .respond_with(html("<html>results page</html>".into()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("q", "limited"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;
    let ws = WebSearch::with_searx(&server.uri(), fast());
    assert_eq!(
        ws.search_in(Category::Web, "closed", 5).await.unwrap_err(),
        ResearchError::Http { status: 403 }
    );
    assert!(matches!(
        ws.search_in(Category::Web, "html", 5).await.unwrap_err(),
        ResearchError::Parse(_)
    ));
    assert!(matches!(
        ws.search_in(Category::Web, "limited", 5).await.unwrap_err(),
        ResearchError::RateLimited { .. }
    ));
}

#[tokio::test]
async fn searxng_hostile_results_are_dropped() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"results":[
        {"url":"javascript:alert(1)","title":"x","content":"x"},
        {"url":"data:text/html,<b>x</b>","title":"x"},
        {"url":"ftp://files.example.org/a","title":"x"},
        {"url":"https://ok.example.org/a#frag","title":"<b>Fine</b>&nbsp;title","content":"<img src=x onerror=1>snippet\n  text"},
        {"url":"https://ok.example.org/a","title":"duplicate"},
        {"url":"https://spa ce.example.org/","title":"x"},
        {"url":"","title":"x"},
        {"title":"no url"},
        {"url":"http://plain.example.org/","title":"A".repeat(500),"content":"B".repeat(900)}
    ]});
    Mock::given(method("GET"))
        .respond_with(json(body.to_string()))
        .mount(&server)
        .await;
    let hits = WebSearch::with_searx(&server.uri(), fast())
        .search_in(Category::Web, "x", 20)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2, "{hits:#?}");
    assert_eq!(hits[0].download_url, "https://ok.example.org/a");
    assert_eq!(hits[0].title, "Fine title");
    assert_eq!(hits[0].summary.as_deref(), Some("snippet text"));
    assert_eq!(hits[1].title.chars().count(), 200);
    assert_eq!(hits[1].summary.as_deref().unwrap().chars().count(), 300);
}

#[test]
fn searxng_base_is_validated_and_normalised() {
    let ok = |s: &str| validate_searxng_base(s).unwrap();
    assert_eq!(
        ok("https://search.example.org"),
        "https://search.example.org"
    );
    assert_eq!(
        ok("https://search.example.org/"),
        "https://search.example.org"
    );
    assert_eq!(
        ok("  HTTPS://Search.Example.org:443/searx/ "),
        "https://search.example.org/searx"
    );
    assert_eq!(
        ok("https://search.example.org:8443/"),
        "https://search.example.org:8443"
    );
    assert_eq!(ok("http://localhost:8080/"), "http://localhost:8080");
    assert_eq!(ok("http://127.0.0.1:8888"), "http://127.0.0.1:8888");
    assert_eq!(ok("http://192.168.1.20/"), "http://192.168.1.20");
    assert_eq!(ok("http://10.0.0.5:8080/s/"), "http://10.0.0.5:8080/s");
    assert_eq!(ok("http://[::1]:8080/"), "http://[::1]:8080");

    for bad in [
        "",
        "not a url",
        "search.example.org",
        "http://search.example.org",
        "http://8.8.8.8/",
        "ftp://search.example.org/",
        "file:///C:/x",
        "javascript:alert(1)",
        "https://user:pass@search.example.org/",
        "https://user@search.example.org/",
        "https://search.example.org/?q=1",
        "https://search.example.org/#x",
        "https://",
    ] {
        let err = validate_searxng_base(bad).unwrap_err();
        assert!(matches!(err, ResearchError::Invalid(_)), "{bad}: {err:?}");
        // the message never repeats the input (it could hold a password)
        if bad.len() > 3 {
            assert!(!err.to_string().contains(bad), "{bad}: {err}");
        }
    }
}

#[tokio::test]
async fn searxng_invalid_base_is_refused_before_any_request() {
    let ws = WebSearch::with_searx("https://user:hunter2@search.example.org/", fast());
    let err = ws.search_in(Category::Web, "x", 5).await.unwrap_err();
    assert!(matches!(err, ResearchError::Invalid(_)));
    assert!(!format!("{err} {err:?}").contains("hunter2"));
}

#[test]
fn user_agent_is_polite() {
    assert!(USER_AGENT.starts_with("Riptide/"));
}
