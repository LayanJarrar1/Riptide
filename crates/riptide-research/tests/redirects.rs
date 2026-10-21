//! A redirect must not carry the address it came from in a `Referer` (review round of alpha.5,
//! P3): some sources put a client id in the query string.
#![allow(clippy::unwrap_used, missing_docs)]

use std::time::Duration;

use riptide_research::http::Http;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn a_followed_redirect_carries_no_referer() {
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&target)
        .await;
    let origin = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/final", target.uri())),
        )
        .mount(&origin)
        .await;
    let http = Http::new(Duration::ZERO);
    let url = format!("{}/start?client_id=SECRETID", origin.uri());
    let body = http.text(|| http.get(&url)).await.unwrap();
    assert_eq!(body, "ok");
    let seen = target.received_requests().await.unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        !seen[0].headers.contains_key("referer"),
        "the redirect target saw {:?}",
        seen[0].headers.get("referer")
    );
}
