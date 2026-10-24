//! The updater against a local feed: no update, newer stable, prerelease ordering, offline,
//! server error, invalid manifest, bad or corrupted signature, interrupted download and retry,
//! progress events. Real minisign signatures from a throw-away key pair, the real
//! `tauri-plugin-updater` on a mock app, `wiremock` and a raw TCP server as the feed. No GitHub
//! and no installer is ever started (the install itself is covered by
//! `scripts/test-update-e2e.ps1`).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::sync::Mutex;

use base64::Engine as _;
use riptide_desktop_lib::updater::{
    check_endpoint, download_package, download_pending, PendingUpdate, UpdateError,
    UpdateErrorKind, UpdateInfo, PROGRESS_EVENT,
};
use serde_json::json;
use std::io::Cursor;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Listener;
use tauri_plugin_updater::Update;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

struct Keys {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}

impl Keys {
    fn new() -> Keys {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Keys {
            pk: kp.pk,
            sk: kp.sk,
        }
    }
    fn pubkey(&self) -> String {
        b64(&self.pk.to_box().unwrap().to_string())
    }
    fn sign(&self, data: &[u8]) -> String {
        let sig = minisign::sign(
            Some(&self.pk),
            &self.sk,
            Cursor::new(data),
            Some("trusted"),
            Some("untrusted"),
        )
        .unwrap();
        b64(&sig.to_string())
    }
}

fn package(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

/// A mock app running as `current`, with the real updater plugin trusting `keys`.
fn app(current: &str, keys: &Keys) -> tauri::App<MockRuntime> {
    let mut context = mock_context(noop_assets());
    context.package_info_mut().version = semver::Version::parse(current).unwrap();
    context.config_mut().plugins.0.insert(
        "updater".to_string(),
        json!({
            "pubkey": keys.pubkey(),
            "endpoints": ["https://example.invalid/latest.json"],
            "dangerousInsecureTransportProtocol": true,
        }),
    );
    mock_builder()
        .plugin(riptide_desktop_lib::updater::plugin())
        .build(context)
        .unwrap()
}

fn manifest(version: &str, url: &str, signature: &str) -> serde_json::Value {
    json!({
        "version": version,
        "notes": "Faster downloads.",
        "pub_date": "2026-10-01T10:00:00Z",
        "platforms": { "windows-x86_64": { "url": url, "signature": signature } },
        "url": url,
        "signature": signature,
    })
}

async fn serve_manifest(server: &MockServer, body: serde_json::Value) -> url::Url {
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
    url::Url::parse(&format!("{}/latest.json", server.uri())).unwrap()
}

async fn serve_package(server: &MockServer, bytes: &[u8]) -> String {
    Mock::given(method("GET"))
        .and(path("/pkg.exe"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.to_vec()))
        .mount(server)
        .await;
    format!("{}/pkg.exe", server.uri())
}

async fn check(
    current: &str,
    keys: &Keys,
    endpoint: url::Url,
) -> Result<Option<Update>, UpdateError> {
    let app = app(current, keys);
    check_endpoint(app.handle(), endpoint).await
}

/// `unwrap_err` for results holding an `Update` (which has no `Debug`).
fn err_of(result: Result<Option<Update>, UpdateError>) -> UpdateError {
    match result {
        Err(e) => e,
        Ok(_) => panic!("expected an error"),
    }
}

#[tokio::test]
async fn no_update_when_the_feed_has_the_running_version() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    let url = serve_manifest(&server, manifest("0.1.0-beta.2", "http://x/p", "s")).await;
    assert!(check("0.1.0-beta.2", &keys, url).await.unwrap().is_none());
}

#[tokio::test]
async fn no_update_on_204_and_for_an_older_feed() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let url = url::Url::parse(&format!("{}/latest.json", server.uri())).unwrap();
    assert!(check("0.1.0", &keys, url).await.unwrap().is_none());
    let other = MockServer::start().await;
    let url = serve_manifest(&other, manifest("0.0.9", "http://x/p", "s")).await;
    assert!(check("0.1.0", &keys, url).await.unwrap().is_none());
}

#[tokio::test]
async fn a_newer_stable_release_is_offered_with_its_notes() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    let url = serve_manifest(&server, manifest("0.1.0", "http://x/p", "s")).await;
    let update = check("0.1.0-beta.1", &keys, url).await.unwrap().unwrap();
    let info = UpdateInfo::from_update(&update);
    assert_eq!(info.version, "0.1.0");
    assert_eq!(info.current_version, "0.1.0-beta.1");
    assert_eq!(info.notes.as_deref(), Some("Faster downloads."));
    assert!(info.date.is_some());
}

#[tokio::test]
async fn the_static_platforms_shape_is_understood_too() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    let mut body = manifest("0.2.0", "http://x/p", "s");
    body.as_object_mut().unwrap().remove("url");
    body.as_object_mut().unwrap().remove("signature");
    let url = serve_manifest(&server, body).await;
    let result = check("0.1.0", &keys, url).await;
    if cfg!(all(windows, target_arch = "x86_64")) {
        assert_eq!(result.unwrap().unwrap().version, "0.2.0");
    } else {
        assert_eq!(err_of(result).kind, UpdateErrorKind::Manifest);
    }
}

#[tokio::test]
async fn prerelease_ordering_is_alpha_beta_rc_stable() {
    let keys = Keys::new();
    // (running, announced, is an update)
    let cases = [
        ("0.1.0-alpha.4", "0.1.0-beta.1", true),
        ("0.1.0-beta.9", "0.1.0-rc.1", true),
        ("0.1.0-rc.1", "0.1.0", true),
        ("0.1.0-beta.2", "0.1.0-beta.10", true),
        ("0.1.0-beta.1", "0.1.0-beta.1.1", true),
        ("0.1.0-alpha.4", "0.1.1-alpha.1", true),
        ("0.1.0", "0.1.0-rc.1", false),
        ("0.1.0-rc.1", "0.1.0-beta.9", false),
        ("0.1.0-beta.1", "0.1.0-alpha.9", false),
        ("0.1.0-beta.2", "0.1.0-beta.1", false),
        // build metadata never decides
        ("0.1.0-beta.1+e2e", "0.1.0-beta.1", false),
        ("0.1.0-beta.1", "0.1.0-beta.1+other", false),
    ];
    for (running, announced, expected) in cases {
        let server = MockServer::start().await;
        let url = serve_manifest(&server, manifest(announced, "http://x/p", "s")).await;
        let got = check(running, &keys, url).await.unwrap().is_some();
        assert_eq!(got, expected, "{running} -> {announced}");
    }
}

#[tokio::test]
async fn a_dead_network_is_reported_as_offline() {
    let keys = Keys::new();
    // a port nothing listens on
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let url = url::Url::parse(&format!("http://127.0.0.1:{port}/latest.json")).unwrap();
    let e = err_of(check("0.1.0", &keys, url).await);
    assert_eq!(e.kind, UpdateErrorKind::Offline, "{}", e.message);
}

#[tokio::test]
async fn a_server_error_is_reported_as_server() {
    let keys = Keys::new();
    for status in [500u16, 503, 404] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let url = url::Url::parse(&format!("{}/latest.json", server.uri())).unwrap();
        let e = err_of(check("0.1.0", &keys, url).await);
        assert_eq!(
            e.kind,
            UpdateErrorKind::Server,
            "status {status}: {}",
            e.message
        );
    }
}

#[tokio::test]
async fn an_invalid_manifest_is_reported_as_manifest() {
    let keys = Keys::new();
    let bodies: [&[u8]; 4] = [
        b"<html>captive portal</html>",
        b"{not json",
        b"{}",
        br#"{"version":"x.y"}"#,
    ];
    for body in bodies {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&server)
            .await;
        let url = url::Url::parse(&format!("{}/latest.json", server.uri())).unwrap();
        let e = err_of(check("0.1.0", &keys, url).await);
        assert_eq!(e.kind, UpdateErrorKind::Manifest, "{body:?}: {}", e.message);
    }
}

#[tokio::test]
async fn a_package_with_a_wrong_signature_is_refused_and_never_stored() {
    let keys = Keys::new();
    let bytes = package(50_000);
    let stranger = Keys::new();
    let signatures = [
        stranger.sign(&bytes),
        b64("not a minisign signature"),
        "%%%".to_string(),
    ];
    for signature in signatures {
        let server = MockServer::start().await;
        let url = serve_package(&server, &bytes).await;
        let feed = serve_manifest(&server, manifest("0.2.0", &url, &signature)).await;
        let app = app("0.1.0", &keys);
        let update = check_endpoint(app.handle(), feed).await.unwrap().unwrap();
        let pending = PendingUpdate::default();
        pending.set_update(Some(update));
        let e = download_pending(app.handle(), &pending).await.unwrap_err();
        assert_eq!(e.kind, UpdateErrorKind::Signature, "{}", e.message);
        assert!(pending.package().is_none() && !pending.is_downloading());
    }
}

#[tokio::test]
async fn a_corrupted_package_fails_the_signature_check() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    let good = package(50_000);
    let mut bad = good.clone();
    bad[25_000] ^= 0x01; // one flipped bit
    let url = serve_package(&server, &bad).await;
    let feed = serve_manifest(&server, manifest("0.2.0", &url, &keys.sign(&good))).await;
    let app = app("0.1.0", &keys);
    let update = check_endpoint(app.handle(), feed).await.unwrap().unwrap();
    let e = download_package(&update, |_| {}).await.unwrap_err();
    assert_eq!(e.kind, UpdateErrorKind::Signature);
    // the same bytes, checked offline, are refused too (what `install` runs first)
    assert!(update.verify(&bad).is_err());
    assert!(update.verify(&good).is_ok());
}

#[tokio::test]
async fn a_good_package_is_downloaded_verified_and_reported_in_progress_events() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    let bytes = package(300_000);
    let url = serve_package(&server, &bytes).await;
    let feed = serve_manifest(&server, manifest("0.2.0", &url, &keys.sign(&bytes))).await;
    let app = app("0.1.0", &keys);
    let events = std::sync::Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let sink = events.clone();
    app.handle().listen(PROGRESS_EVENT, move |e| {
        sink.lock()
            .unwrap()
            .push(serde_json::from_str(e.payload()).unwrap());
    });
    let update = check_endpoint(app.handle(), feed).await.unwrap().unwrap();
    let pending = PendingUpdate::default();
    pending.set_update(Some(update));
    download_pending(app.handle(), &pending).await.unwrap();
    assert_eq!(pending.package(), Some(bytes.clone()));
    // already downloaded: a second call is a no-op
    download_pending(app.handle(), &pending).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let events = events.lock().unwrap();
    let last = events.last().expect("a progress event");
    assert_eq!(last["downloaded"], 300_000);
    assert_eq!(last["total"], 300_000);
}

/// Answers `Content-Length: total` and then closes after `sent` bytes, once; the next
/// connection gets everything.
async fn flaky_package_server(bytes: Vec<u8>, sent: usize) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut first = true;
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                bytes.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let n = if first { sent } else { bytes.len() };
            first = false;
            let _ = sock.write_all(&bytes[..n]).await;
            let _ = sock.shutdown().await;
        }
    });
    format!("http://{addr}/pkg.exe")
}

#[tokio::test]
async fn an_interrupted_download_keeps_the_update_and_retries_cleanly() {
    let (keys, server) = (Keys::new(), MockServer::start().await);
    let bytes = package(400_000);
    let url = flaky_package_server(bytes.clone(), 100_000).await;
    let feed = serve_manifest(&server, manifest("0.2.0", &url, &keys.sign(&bytes))).await;
    let app = app("0.1.0", &keys);
    let update = check_endpoint(app.handle(), feed).await.unwrap().unwrap();
    let pending = PendingUpdate::default();
    pending.set_update(Some(update));
    let e = download_pending(app.handle(), &pending).await.unwrap_err();
    assert_eq!(e.kind, UpdateErrorKind::Download, "{}", e.message);
    // nothing half-written is kept; the update and the retry are still there
    assert!(pending.package().is_none() && pending.has_update() && !pending.is_downloading());
    download_pending(app.handle(), &pending).await.unwrap();
    assert_eq!(pending.package(), Some(bytes));
}

#[tokio::test]
async fn downloading_without_a_check_says_so() {
    let keys = Keys::new();
    let app = app("0.1.0", &keys);
    let pending = PendingUpdate::default();
    let e = download_pending(app.handle(), &pending).await.unwrap_err();
    assert_eq!(e.kind, UpdateErrorKind::NoUpdate);
}
