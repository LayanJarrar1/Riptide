//! Every adapter against a mock server that replays responses recorded from the real services
//! (see `tests/fixtures/`, recorded 2026-10-20). No network.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_research::http::{Http, Tuning, USER_AGENT};
use riptide_research::sources::{
    arxiv::Arxiv, commons::Commons, crossref::Crossref, datagov::DataGov, europepmc::EuropePmc,
    github::Github, internet_archive::InternetArchive, openalex::OpenAlex,
};
use riptide_research::{search_all, Hit, ResearchError, SearchSource, Source};
use wiremock::matchers::{header_regex, method, path, path_regex, query_param_contains};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
            max_attempts: 3,
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
    assert!(!h.title.is_empty());
    assert!(h.id.starts_with(h.source.id()));
}

#[tokio::test]
async fn openalex_returns_only_direct_open_links_with_licence_and_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param_contains("filter", "open_access.is_oa:true"))
        .and(header_regex("user-agent", "^Riptide/"))
        .respond_with(json(fixture("openalex.json")))
        .expect(1)
        .mount(&server)
        .await;
    let hits = OpenAlex::with_base(&server.uri(), fast())
        .search("bittorrent piece selection", 10)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    hits.iter().for_each(assert_direct);
    // a DOI redirect and a bare landing page are not files
    assert!(hits.iter().all(|h| !h.download_url.contains("doi.org/")));
    assert!(hits.iter().all(|h| !h.download_url.contains("urn.kb.se")));
    let springer = hits
        .iter()
        .find(|h| h.download_url.contains("springer"))
        .unwrap();
    assert_eq!(springer.source, Source::OpenAlex);
    assert!(springer.doi.is_some() || springer.year.is_some());
    // best_oa_location.license is null for most works: the licence is shown only when reported
    assert!(hits.iter().all(|h| h.license.as_deref() != Some("")));
}

#[tokio::test]
async fn crossref_keeps_only_open_licence_with_pdf_link() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(json(fixture("crossref.json")))
        .mount(&server)
        .await;
    let hits = Crossref::with_base(&server.uri(), fast())
        .search("bittorrent", 10)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    hits.iter().for_each(assert_direct);
    assert!(hits.iter().all(|h| h
        .license
        .as_deref()
        .unwrap()
        .contains("creativecommons.org")));
    assert!(hits.iter().all(|h| h.doi.is_some()));
    // of the several links of a blog post (markdown, epub, xml, pdf) only the PDF is offered
    let post = hits
        .iter()
        .find(|h| h.doi.as_deref() == Some("10.59350/bqfjn-rgj08"))
        .unwrap();
    assert!(
        post.download_url.ends_with(".pdf/content"),
        "{}",
        post.download_url
    );
    // and a work whose only PDF link is the publisher's closed text-mining endpoint is absent
    assert!(hits
        .iter()
        .all(|h| !h.download_url.contains("api.wiley.com")));
}

#[tokio::test]
async fn arxiv_parses_the_atom_feed_and_sends_an_and_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/query"))
        .and(query_param_contains(
            "search_query",
            "all:bittorrent AND all:piece",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/atom+xml")
                .set_body_string(fixture("arxiv.xml")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let hits = Arxiv::with_base(&server.uri(), fast())
        .search("bittorrent piece selection", 4)
        .await
        .unwrap();
    assert_eq!(hits.len(), 4);
    for h in &hits {
        assert_direct(h);
        assert!(h.download_url.starts_with("https://arxiv.org/pdf/"));
        assert!(h.arxiv.is_some());
        assert!(h.year.is_some());
        assert!(!h.authors.is_empty());
        assert!(h.license.is_none(), "the arXiv API reports no licence");
    }
    assert_eq!(hits[0].arxiv.as_deref(), Some("1705.00548"));
    assert_eq!(
        hits[0].page_url.as_deref(),
        Some("https://arxiv.org/abs/1705.00548")
    );
}

#[tokio::test]
async fn europepmc_open_access_with_pmcid_and_licence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param_contains("query", "OPEN_ACCESS:y"))
        .respond_with(json(fixture("europepmc.json")))
        .mount(&server)
        .await;
    let hits = EuropePmc::with_base(&server.uri(), fast())
        .search("peer-to-peer", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 3);
    for h in &hits {
        assert_direct(h);
        assert!(h.pmcid.as_deref().unwrap().starts_with("PMC"));
        assert!(h.license.is_some());
    }
    assert!(hits.iter().all(|h| h.download_url.starts_with("https://")));
}

#[tokio::test]
async fn internet_archive_two_step_and_drops_restricted_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/advancedsearch.php"))
        .respond_with(json(fixture("ia_search.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/metadata/TheStrandMagazineAnIllustratedMonthly"))
        .respond_with(json(fixture("ia_meta2.json")))
        .mount(&server)
        .await;
    // an item that turned out to be lending-library only
    let restricted = fixture("ia_meta.json").replace(
        "\"identifier\":",
        "\"access-restricted-item\": \"true\", \"identifier\":",
    );
    Mock::given(method("GET"))
        .and(path("/metadata/DnaEm1CapabilitiesOfNuclearWeapons"))
        .respond_with(json(restricted))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex("^/metadata/.*"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let hits = InternetArchive::with_base(&server.uri(), fast())
        .search("history", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    let h = &hits[0];
    assert_direct(h);
    assert!(h
        .download_url
        .contains("/download/TheStrandMagazineAnIllustratedMonthly/"));
    assert!(h.download_url.ends_with(".pdf"));
    assert_eq!(
        h.license.as_deref(),
        Some("http://creativecommons.org/publicdomain/mark/1.0/")
    );
    assert!(h.size.unwrap() > 1_000_000);
    assert!(
        h.title.contains(" - "),
        "several volumes: the title says which: {}",
        h.title
    );
}

#[tokio::test]
async fn commons_files_with_licence_and_size() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .and(query_param_contains("generator", "search"))
        .respond_with(json(fixture("commons.json")))
        .mount(&server)
        .await;
    let hits = Commons::with_base(&server.uri(), fast())
        .search("bittorrent logo", 3)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    for h in &hits {
        assert_direct(h);
        assert!(h.download_url.starts_with("https://upload.wikimedia.org/"));
        assert!(h.license.is_some(), "{h:?}");
        assert!(h.size.is_some());
        assert!(!h.title.starts_with("File:"));
    }
}

#[tokio::test]
async fn github_release_assets_skip_signatures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(header_regex("accept", "github"))
        .respond_with(json(fixture("gh_search.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/qbittorrent/qBittorrent/releases/latest"))
        .respond_with(json(fixture("gh_release.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex("^/repos/.*/releases/latest$"))
        .respond_with(ResponseTemplate::new(404).set_body_string("{\"message\":\"Not Found\"}"))
        .mount(&server)
        .await;
    let hits = Github::with_base(&server.uri(), fast())
        .search("bittorrent client", 6)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    for h in &hits {
        assert_direct(h);
        assert!(h
            .download_url
            .starts_with("https://github.com/qbittorrent/qBittorrent/releases/download/"));
        assert!(!h.download_url.ends_with(".asc"));
        // the repository reports NOASSERTION: no licence is claimed
        assert_eq!(h.license, None);
    }
    // Windows installers first
    assert!(hits[0].filename.as_deref().unwrap().ends_with("setup.exe"));
}

#[tokio::test]
async fn github_rate_limit_is_a_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).insert_header("retry-after", "3600"))
        .mount(&server)
        .await;
    let err = Github::with_base(&server.uri(), fast())
        .search("anything", 3)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::RateLimited { .. }), "{err:?}");
}

#[tokio::test]
async fn datagov_new_catalogue_api_files_only() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param_contains("format", "json"))
        .respond_with(json(fixture("datagov.json")))
        .mount(&server)
        .await;
    let hits = DataGov::with_base(&server.uri(), fast())
        .search("climate", 10)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    for h in &hits {
        assert_direct(h);
        assert!(h.page_url.as_deref().unwrap().contains("/dataset/"));
        assert!(h.title.ends_with(')'), "{}", h.title);
    }
    assert!(hits.len() <= 6, "at most two files per dataset");
}

// ------------------------------------------------------------------ transport behaviour

#[tokio::test]
async fn retries_5xx_and_429_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(json(fixture("openalex.json")))
        .mount(&server)
        .await;
    let hits = OpenAlex::with_base(&server.uri(), fast())
        .search("x", 3)
        .await
        .unwrap();
    assert!(!hits.is_empty());
}

#[tokio::test]
async fn gives_up_with_typed_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(500))
        .expect(3)
        .mount(&server)
        .await;
    let err = OpenAlex::with_base(&server.uri(), fast())
        .search("x", 3)
        .await
        .unwrap_err();
    assert_eq!(err, ResearchError::Http { status: 500 });

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let err = Crossref::with_base(&server.uri(), fast())
        .search("x", 3)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ResearchError::Http { status: 404 },
        "4xx is not retried"
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json("not json at all".into()))
        .mount(&server)
        .await;
    let err = Crossref::with_base(&server.uri(), fast())
        .search("x", 3)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::Parse(_)), "{err:?}");

    let err = Crossref::with_base(&server.uri(), fast())
        .search("   ", 3)
        .await
        .unwrap_err();
    assert!(matches!(err, ResearchError::Invalid(_)));
}

#[tokio::test]
async fn slow_source_times_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json(fixture("openalex.json")).set_delay(Duration::from_secs(3)))
        .mount(&server)
        .await;
    let http = Http::with_tuning(
        Duration::ZERO,
        Tuning {
            timeout: Duration::from_millis(100),
            max_attempts: 2,
            base_delay: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(1),
        },
    );
    let err = OpenAlex::with_base(&server.uri(), http)
        .search("x", 3)
        .await
        .unwrap_err();
    assert_eq!(err, ResearchError::Timeout);
}

#[tokio::test]
async fn requests_are_spaced_by_the_source_interval() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json(fixture("openalex.json")))
        .mount(&server)
        .await;
    let http = Http::with_tuning(Duration::from_millis(250), Tuning::default());
    let oa = OpenAlex::with_base(&server.uri(), http);
    let start = std::time::Instant::now();
    oa.search("a", 3).await.unwrap();
    oa.search("b", 3).await.unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(240),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn the_user_agent_is_polite_and_impersonal() {
    assert!(USER_AGENT.starts_with("Riptide/"));
    assert!(USER_AGENT.contains("github.com/LayanJarrar1/Riptide"));
    assert!(!USER_AGENT.contains('@'), "no e-mail address");
}

#[tokio::test]
async fn search_all_reports_the_failing_source_and_still_returns_the_rest() {
    let ok = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json(fixture("openalex.json")))
        .mount(&ok)
        .await;
    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&bad)
        .await;
    let adapters: Vec<Box<dyn SearchSource>> = vec![
        Box::new(OpenAlex::with_base(&ok.uri(), fast())),
        Box::new(Crossref::with_base(&bad.uri(), fast())),
    ];
    let out = search_all(&adapters, &["a b".into(), "c".into()], 5).await;
    assert!(!out.hits.is_empty());
    assert_eq!(out.failures.len(), 1);
    assert_eq!(out.failures[0].source, Source::Crossref);
    assert_eq!(out.failures[0].code, "http");
    // two phrasings returned the same works: no repeats
    let mut ids: Vec<&str> = out.hits.iter().map(|h| h.id.as_str()).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n);
}
