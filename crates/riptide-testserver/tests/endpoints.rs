//! End-to-end tests for every endpoint, driven with `reqwest` against an in-process server.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use futures::StreamExt as _;
use md5::{Digest as _, Md5};
use reqwest::header;
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use riptide_testserver::{
    corrupt_bit_positions, etag, expected_bytes, expected_sha256_hex, TestServer, DEFAULT_SEED,
    LAST_MODIFIED,
};
use sha2::Sha256;
use std::io::Read as _;

fn no_redirects() -> Client {
    Client::builder().redirect(Policy::none()).build().unwrap()
}

fn following() -> Client {
    Client::builder().build().unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn header_str<'a>(resp: &'a reqwest::Response, name: &str) -> Option<&'a str> {
    resp.headers().get(name).and_then(|v| v.to_str().ok())
}

// ------------------------------------------------------------------------------------------------
// /ok
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn ok_full_body_matches_expected_sha256() {
    let srv = TestServer::spawn().await;
    let size = 3 * 1024 * 1024u64;
    let resp = no_redirects()
        .get(srv.url(&format!("/ok?size={size}")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "accept-ranges"), Some("bytes"));
    assert_eq!(header_str(&resp, "content-length"), Some("3145728"));
    assert_eq!(
        header_str(&resp, "content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(
        header_str(&resp, "etag"),
        Some(etag(DEFAULT_SEED, size).as_str())
    );
    assert_eq!(header_str(&resp, "last-modified"), Some(LAST_MODIFIED));
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.len() as u64, size);
    assert_eq!(sha256_hex(&body), expected_sha256_hex(DEFAULT_SEED, size));

    // A different seed gives different bytes with the same shape.
    let other = no_redirects()
        .get(srv.url("/ok?size=4096&seed=42"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(other.as_ref(), expected_bytes(42, 4096).as_slice());
    assert_ne!(
        other.as_ref(),
        expected_bytes(DEFAULT_SEED, 4096).as_slice()
    );
}

#[tokio::test]
async fn ok_range_206_and_content_range() {
    let srv = TestServer::spawn().await;
    let size = 200_000u64;
    let whole = expected_bytes(DEFAULT_SEED, size);
    let c = no_redirects();
    let cases: [(&str, u64, u64); 5] = [
        ("bytes=0-99", 0, 100),
        ("bytes=1000-", 1000, size),
        ("bytes=-500", size - 500, size),
        ("bytes=65530-65600", 65530, 65601),
        ("bytes=199999-999999", 199_999, size),
    ];
    for (range, start, end) in cases {
        let resp = c
            .get(srv.url(&format!("/ok?size={size}")))
            .header(header::RANGE, range)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(
            header_str(&resp, "content-range"),
            Some(format!("bytes {}-{}/{}", start, end - 1, size).as_str()),
            "{range}"
        );
        assert_eq!(
            header_str(&resp, "content-length"),
            Some((end - start).to_string().as_str()),
            "{range}"
        );
        let body = resp.bytes().await.unwrap();
        assert_eq!(
            body.as_ref(),
            &whole[start as usize..end as usize],
            "{range}"
        );
    }
}

#[tokio::test]
async fn ok_416_on_unsatisfiable() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    for range in ["bytes=200000-", "bytes=500000-600000", "bytes=-0"] {
        let resp = c
            .get(srv.url("/ok?size=200000"))
            .header(header::RANGE, range)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(header_str(&resp, "content-range"), Some("bytes */200000"));
        assert!(resp.bytes().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn ok_head_has_length_and_etag() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects()
        .head(srv.url("/ok?size=123456"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "content-length"), Some("123456"));
    assert_eq!(header_str(&resp, "accept-ranges"), Some("bytes"));
    assert_eq!(
        header_str(&resp, "etag"),
        Some(etag(DEFAULT_SEED, 123_456).as_str())
    );
    assert_eq!(header_str(&resp, "last-modified"), Some(LAST_MODIFIED));
    assert!(resp.bytes().await.unwrap().is_empty());
}

#[tokio::test]
async fn ok_if_range_mismatch_returns_200() {
    let srv = TestServer::spawn().await;
    let size = 50_000u64;
    let c = no_redirects();
    let url = srv.url(&format!("/ok?size={size}"));

    let resp = c
        .get(&url)
        .header(header::RANGE, "bytes=100-199")
        .header(header::IF_RANGE, "\"something-else\"")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "content-length"), Some("50000"));
    assert!(resp.headers().get("content-range").is_none());
    assert_eq!(resp.bytes().await.unwrap().len() as u64, size);

    for validator in [etag(DEFAULT_SEED, size), LAST_MODIFIED.to_owned()] {
        let resp = c
            .get(&url)
            .header(header::RANGE, "bytes=100-199")
            .header(header::IF_RANGE, &validator)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT, "{validator}");
        assert_eq!(resp.bytes().await.unwrap().len(), 100);
    }
}

// ------------------------------------------------------------------------------------------------
// /noranges, /nolength, /slow, /drop, /flaky
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn noranges_ignores_range() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects()
        .get(srv.url("/noranges?size=70000"))
        .header(header::RANGE, "bytes=0-9")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("accept-ranges").is_none());
    assert!(resp.headers().get("content-range").is_none());
    assert_eq!(header_str(&resp, "content-length"), Some("70000"));
    let body = resp.bytes().await.unwrap();
    assert_eq!(
        body.as_ref(),
        expected_bytes(DEFAULT_SEED, 70_000).as_slice()
    );
}

#[tokio::test]
async fn nolength_has_no_content_length_and_bytes_match() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects()
        .get(srv.url("/nolength?size=150000"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("content-length").is_none());
    assert_eq!(header_str(&resp, "transfer-encoding"), Some("chunked"));
    assert!(resp.headers().get("accept-ranges").is_none());
    let body = resp.bytes().await.unwrap();
    assert_eq!(
        body.as_ref(),
        expected_bytes(DEFAULT_SEED, 150_000).as_slice()
    );
}

#[tokio::test]
async fn slow_throttles_and_bytes_match() {
    let srv = TestServer::spawn().await;
    let started = Instant::now();
    let resp = no_redirects()
        .get(srv.url("/slow?size=131072&bps=131072"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "accept-ranges"), Some("bytes"));
    let body = resp.bytes().await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(800),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(
        body.as_ref(),
        expected_bytes(DEFAULT_SEED, 131_072).as_slice()
    );
}

#[tokio::test]
async fn drop_closes_after_n_bytes() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects()
        .get(srv.url("/drop?size=1048576&after=100000"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "content-length"), Some("1048576"));
    let mut stream = resp.bytes_stream();
    let mut received = Vec::new();
    let mut errored = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => received.extend_from_slice(&chunk),
            Err(_) => {
                errored = true;
                break;
            }
        }
    }
    assert!(
        errored,
        "stream must end with an error, got {} bytes",
        received.len()
    );
    assert!(received.len() <= 100_000);
    assert_eq!(
        received.as_slice(),
        &expected_bytes(DEFAULT_SEED, 100_000)[..received.len()]
    );
}

#[tokio::test]
async fn flaky_rate_one_always_fails_and_rate_zero_never() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    for _ in 0..8 {
        // Either a 5xx, or (the "reset" variant) a 200 whose body never arrives.
        if let Ok(resp) = c.get(srv.url("/flaky?rate=1&size=4096")).send().await {
            let status = resp.status();
            if status.is_server_error() {
                continue;
            }
            assert_eq!(status, StatusCode::OK);
            assert!(resp.bytes().await.is_err());
        }
    }
    for _ in 0..8 {
        let resp = c
            .get(srv.url("/flaky?rate=0&size=4096"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.bytes().await.unwrap().len(), 4096);
    }
}

// ------------------------------------------------------------------------------------------------
// /retry-after, /limit
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn retry_after_then_ok() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    let url = srv.url("/retry-after?fails=2&size=4096");
    for _ in 0..2 {
        let resp = c.get(&url).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(header_str(&resp, "retry-after"), Some("1"));
    }
    let resp = c.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);
}

#[tokio::test]
async fn limit_returns_429_above_n() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    // 256 MiB bodies cannot drain into the socket buffers, so they stay in flight until dropped.
    let url = srv.url("/limit?conns=2&size=268435456");
    let responses = futures::future::join_all((0..3).map(|_| c.get(&url).send())).await;
    let mut statuses: Vec<u16> = responses
        .iter()
        .map(|r| r.as_ref().unwrap().status().as_u16())
        .collect();
    statuses.sort_unstable();
    assert_eq!(statuses, vec![200, 200, 429]);
    assert_eq!(srv.state().in_flight(), 2);
    drop(responses);

    // Once the clients hang up the slots are released.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let resp = c
            .get(srv.url("/limit?conns=2&size=1024"))
            .send()
            .await
            .unwrap();
        if resp.status() == StatusCode::OK {
            assert_eq!(resp.bytes().await.unwrap().len(), 1024);
            break;
        }
        assert!(Instant::now() < deadline, "slots were never released");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ------------------------------------------------------------------------------------------------
// /redirect, /echo-headers
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn redirect_chain_ends_at_ok_with_requested_code() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();

    let mut url = srv.url("/redirect?n=2&code=307&size=4096");
    for expected_location in [
        "/redirect?n=1&code=307&size=4096",
        "/redirect?n=0&code=307&size=4096",
        "/ok?size=4096",
    ] {
        let resp = c.get(&url).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT, "{url}");
        assert_eq!(header_str(&resp, "location"), Some(expected_location));
        url = srv.url(expected_location);
    }
    let resp = c.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);

    // code=cycle rotates through the five redirect codes.
    let mut seen = Vec::new();
    let mut url = srv.url("/redirect?n=4&code=cycle");
    for _ in 0..5 {
        let resp = c.get(&url).send().await.unwrap();
        seen.push(resp.status().as_u16());
        url = srv.url(header_str(&resp, "location").unwrap());
    }
    seen.sort_unstable();
    assert_eq!(seen, vec![301, 302, 303, 307, 308]);
    assert!(url.ends_with("/ok"));

    // Default code is 302 and a following client lands on /ok with the right bytes.
    let resp = c.get(srv.url("/redirect?n=0")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    let resp = following()
        .get(srv.url("/redirect?n=3&size=8192&seed=9"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.url().path().ends_with("/ok"));
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        expected_bytes(9, 8192).as_slice()
    );
}

#[tokio::test]
async fn redirect_cross_origin_lands_on_alt_url() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();

    let resp = c
        .get(srv.url("/redirect?n=1&cross=1&size=4096"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    let location = header_str(&resp, "location").unwrap().to_owned();
    assert_eq!(location, srv.url_on_alt("/redirect?n=0&code=302&size=4096"));

    // A following client walks the whole chain and ends on the alt origin; /echo-headers shows
    // which origin received the final request.
    let f = following();
    let alt_authority = srv.alt_url().trim_start_matches("http://");
    let resp = f
        .get(srv.url("/redirect?n=2&cross=1&to=echo-headers"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.url().as_str().starts_with(srv.alt_url()),
        "{}",
        resp.url()
    );
    assert_eq!(resp.url().path(), "/echo-headers");
    let echoed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(echoed["host"], alt_authority);

    // /echo-headers reports exactly what arrived: on a single cross-origin hop reqwest drops
    // Authorization and Cookie (its own policy, verified here only as a smoke test of the echo).
    let resp = f
        .get(srv.url("/redirect?n=0&cross=1&to=echo-headers"))
        .header(header::AUTHORIZATION, "Bearer secret")
        .header(header::COOKIE, "session=riptide")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.url().path(), "/echo-headers");
    let echoed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(echoed["host"], alt_authority);
    assert!(echoed.get("authorization").is_none(), "{echoed}");
    assert!(echoed.get("cookie").is_none(), "{echoed}");

    // Same-origin control: the headers survive and the host is the base origin.
    let resp = f
        .get(srv.url("/redirect?n=0&to=echo-headers"))
        .header(header::AUTHORIZATION, "Bearer secret")
        .header(header::COOKIE, "session=riptide")
        .send()
        .await
        .unwrap();
    assert!(resp.url().as_str().starts_with(srv.base_url()));
    let echoed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(echoed["host"], srv.base_url().trim_start_matches("http://"));
    assert_eq!(echoed["authorization"], "Bearer secret");
    assert_eq!(echoed["cookie"], "session=riptide");
}

#[tokio::test]
async fn echo_headers_returns_json() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects()
        .get(srv.url("/echo-headers"))
        .header("x-riptide", "one")
        .header(header::REFERER, "http://example.com/")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(header_str(&resp, "content-type")
        .unwrap()
        .starts_with("application/json"));
    let echoed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(echoed["x-riptide"], "one");
    assert_eq!(echoed["referer"], "http://example.com/");
}

// ------------------------------------------------------------------------------------------------
// /etag-changes, /wrong-length, /expires
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn etag_changes_after_n_requests() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    let size = 20_000u64;
    let url = srv.url(&format!("/etag-changes?after=2&size={size}"));
    let v1 = etag(DEFAULT_SEED, size);

    for _ in 0..2 {
        let resp = c.get(&url).send().await.unwrap();
        assert_eq!(header_str(&resp, "etag"), Some(v1.as_str()));
        assert_eq!(
            resp.bytes().await.unwrap().as_ref(),
            expected_bytes(DEFAULT_SEED, size).as_slice()
        );
    }

    let resp = c.get(&url).send().await.unwrap();
    let v2 = header_str(&resp, "etag").unwrap().to_owned();
    assert!(v2.starts_with("\"v2-"), "{v2}");
    assert_ne!(v2, v1);
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        expected_bytes(DEFAULT_SEED + 1, size).as_slice()
    );

    // Resuming with the old validator yields the whole new body.
    let resp = c
        .get(&url)
        .header(header::RANGE, "bytes=10000-")
        .header(header::IF_RANGE, &v1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "etag"), Some(v2.as_str()));
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        expected_bytes(DEFAULT_SEED + 1, size).as_slice()
    );

    // With the new validator the range is honoured.
    let resp = c
        .get(&url)
        .header(header::RANGE, "bytes=10000-")
        .header(header::IF_RANGE, &v2)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.bytes().await.unwrap().len(), 10_000);
}

#[tokio::test]
async fn wrong_length_declares_delta() {
    let srv = TestServer::spawn().await;
    let size = 100_000u64;

    // Declared longer than the body: the client sees a short body and a closed connection.
    let resp = no_redirects()
        .get(srv.url(&format!("/wrong-length?size={size}&delta=1000")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "content-length"), Some("101000"));
    assert!(resp.bytes().await.is_err());

    // Declared shorter: hyper stops after Content-Length bytes and closes the connection.
    let resp = no_redirects()
        .get(srv.url(&format!("/wrong-length?size={size}&delta=-1000")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "content-length"), Some("99000"));
    assert_eq!(header_str(&resp, "connection"), Some("close"));
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.len(), 99_000);
    assert_eq!(body.as_ref(), &expected_bytes(DEFAULT_SEED, size)[..99_000]);
}

#[tokio::test]
async fn expires_returns_403_after_deadline() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    let url = srv.url("/expires?after=1&size=4096");
    let resp = c.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let resp = c.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ------------------------------------------------------------------------------------------------
// credentials
// ------------------------------------------------------------------------------------------------

fn md5_hex(s: &str) -> String {
    hex::encode(Md5::digest(s.as_bytes()))
}

fn quoted_param<'a>(challenge: &'a str, key: &str) -> &'a str {
    let start = challenge.find(&format!("{key}=\"")).unwrap() + key.len() + 2;
    let end = challenge[start..].find('"').unwrap();
    &challenge[start..start + end]
}

#[tokio::test]
async fn needs_cookie_basic_digest_referer() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();

    // cookie
    let resp = c
        .get(srv.url("/needs-cookie?size=4096"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = c
        .get(srv.url("/needs-cookie?size=4096"))
        .header(header::COOKIE, "other=1; session=riptide")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);

    // referer
    let resp = c
        .get(srv.url("/needs-referer?size=4096"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = c
        .get(srv.url("/needs-referer?size=4096"))
        .header(header::REFERER, "http://example.com/page")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = c
        .get(srv.url("/needs-referer?size=4096"))
        .header(header::REFERER, srv.url("/some/page?x=1"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);

    // basic
    let resp = c
        .get(srv.url("/needs-auth/basic?size=4096"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        header_str(&resp, "www-authenticate"),
        Some("Basic realm=\"riptide\"")
    );
    let resp = c
        .get(srv.url("/needs-auth/basic?size=4096"))
        .basic_auth("user", Some("wrong"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = c
        .get(srv.url("/needs-auth/basic?size=4096"))
        .basic_auth("user", Some("pass"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);

    // digest
    let uri = "/needs-auth/digest?size=4096";
    let resp = c.get(srv.url(uri)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let challenge = header_str(&resp, "www-authenticate").unwrap().to_owned();
    assert!(challenge.starts_with("Digest "), "{challenge}");
    assert!(challenge.contains("realm=\"riptide\""));
    assert!(challenge.contains("qop=\"auth\""));
    assert!(challenge.contains("algorithm=MD5"));
    let nonce = quoted_param(&challenge, "nonce");
    let opaque = quoted_param(&challenge, "opaque");
    let (nc, cnonce) = ("00000001", "0a4f113b");
    let ha2 = md5_hex(&format!("GET:{uri}"));
    let credential = |password: &str| {
        let ha1 = md5_hex(&format!("user:riptide:{password}"));
        let response = md5_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"));
        format!(
            "Digest username=\"user\", realm=\"riptide\", nonce=\"{nonce}\", uri=\"{uri}\", \
             qop=auth, nc={nc}, cnonce=\"{cnonce}\", response=\"{response}\", \
             opaque=\"{opaque}\", algorithm=MD5"
        )
    };
    let resp = c
        .get(srv.url(uri))
        .header(header::AUTHORIZATION, credential("wrong"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = c
        .get(srv.url(uri))
        .header(header::AUTHORIZATION, credential("pass"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().len(), 4096);
}

// ------------------------------------------------------------------------------------------------
// /gzip, /cd/*, /corrupt, /
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn gzip_body_is_gzip_even_with_identity() {
    let srv = TestServer::spawn().await;
    let size = 300_000u64;
    let resp = no_redirects()
        .get(srv.url(&format!("/gzip?size={size}")))
        .header(header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, "content-encoding"), Some("gzip"));
    let declared: usize = header_str(&resp, "content-length")
        .unwrap()
        .parse()
        .unwrap();
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.len(), declared);
    assert_eq!(&body[..2], &[0x1f, 0x8b]);
    let mut inflated = Vec::new();
    GzDecoder::new(body.as_ref())
        .read_to_end(&mut inflated)
        .unwrap();
    assert_eq!(inflated, expected_bytes(DEFAULT_SEED, size));
}

#[tokio::test]
async fn cd_arabic_and_cd_evil_headers() {
    let srv = TestServer::spawn().await;
    let c = no_redirects();
    let resp = c.get(srv.url("/cd/arabic?size=1024")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, "content-disposition"),
        Some("attachment; filename*=UTF-8''%D9%85%D9%84%D9%81%20%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A.zip")
    );
    assert_eq!(resp.bytes().await.unwrap().len(), 1024);

    let resp = c.get(srv.url("/cd/evil?size=1024")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, "content-disposition"),
        Some("attachment; filename=\"../../CON%01.txt. \"; filename*=UTF-8''..%2F..%2FCON%01.txt.%20")
    );
    assert_eq!(resp.bytes().await.unwrap().len(), 1024);
}

#[tokio::test]
async fn corrupt_flips_bits_but_keeps_etag() {
    let srv = TestServer::spawn().await;
    let size = 100_000u64;
    let resp = no_redirects()
        .get(srv.url(&format!("/corrupt?size={size}&flip=3")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, "etag"),
        Some(etag(DEFAULT_SEED, size).as_str())
    );
    assert_eq!(header_str(&resp, "content-length"), Some("100000"));
    let body = resp.bytes().await.unwrap();
    let good = expected_bytes(DEFAULT_SEED, size);
    let flipped: u32 = body
        .iter()
        .zip(&good)
        .map(|(a, b)| (a ^ b).count_ones())
        .sum();
    assert_eq!(flipped, 3);
    for bit in corrupt_bit_positions(DEFAULT_SEED, size, 3) {
        let byte = (bit / 8) as usize;
        assert_eq!(body[byte] ^ good[byte], 1 << (bit % 8), "bit {bit}");
    }
}

#[tokio::test]
async fn index_lists_endpoints() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects().get(srv.url("/")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let text = resp.text().await.unwrap();
    for endpoint in [
        "/ok",
        "/noranges",
        "/drop",
        "/needs-auth/digest",
        "/cd/evil",
    ] {
        assert!(text.contains(endpoint), "{endpoint} missing from index");
    }
    assert!(text.contains(srv.alt_url()));
}

#[tokio::test]
async fn bad_query_parameter_is_400() {
    let srv = TestServer::spawn().await;
    let resp = no_redirects()
        .get(srv.url("/ok?size=ten"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ------------------------------------------------------------------------------------------------
// /batch/<name>
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn batch_items_serve_per_name_bytes_and_404_outside_the_pattern() {
    let srv = TestServer::spawn().await;
    let client = no_redirects();
    let mut sizes = std::collections::HashSet::new();
    for name in [
        "file001.zip",
        "file042.zip",
        "file120.zip",
        "img_a.jpg",
        "img_z.jpg",
    ] {
        let item = riptide_testserver::batch_item(name).unwrap();
        sizes.insert(item.size);
        let resp = client
            .get(srv.url(&format!("/batch/{name}")))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{name}");
        assert_eq!(header_str(&resp, "accept-ranges"), Some("bytes"));
        assert_eq!(
            header_str(&resp, "etag"),
            Some(etag(item.seed, item.size).as_str())
        );
        let body = resp.bytes().await.unwrap();
        assert_eq!(body.len() as u64, item.size, "{name}");
        assert_eq!(
            sha256_hex(&body),
            expected_sha256_hex(item.seed, item.size),
            "{name}"
        );
    }
    assert!(sizes.len() > 1, "items should differ in size");

    // ranges work like /ok
    let item = riptide_testserver::batch_item("file007.zip").unwrap();
    let resp = client
        .get(srv.url("/batch/file007.zip"))
        .header(header::RANGE, "bytes=10-19")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        resp.bytes().await.unwrap().as_ref(),
        &expected_bytes(item.seed, 20)[10..20]
    );

    // size override
    let resp = client
        .get(srv.url("/batch/file007.zip?size=100"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.bytes().await.unwrap().len(), 100);

    // outside the pattern: missing items
    for name in [
        "file000.zip",
        "file121.zip",
        "file1.zip",
        "file0001.zip",
        "img_A.jpg",
        "img_ab.jpg",
        "other.bin",
        "file001.zip.bak",
    ] {
        let resp = client
            .get(srv.url(&format!("/batch/{name}")))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{name}");
    }
    // distinct names, distinct bytes
    assert_ne!(
        riptide_testserver::batch_item("file001.zip").unwrap().seed,
        riptide_testserver::batch_item("file002.zip").unwrap().seed
    );
}
