//! A redirect from a public host to a private address is refused; the same chain started on a
//! private address (the loopback test server) is followed. The "public host" is simulated with
//! an injected resolver (`public.test` is said to be 93.184.216.34) while the client really
//! connects to the test server.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use riptide_net::net_guard::StaticResolver;
use riptide_net::{Error, NetGuard, RequestContext};
use riptide_testserver::TestServer;
use url::Url;

fn client_for(server: &TestServer) -> reqwest::Client {
    let base = Url::parse(server.base_url()).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], base.port().unwrap()));
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve("public.test", addr)
        .build()
        .unwrap()
}

fn pretend_public() -> NetGuard {
    let table = StaticResolver(HashMap::from([(
        "public.test".to_string(),
        vec!["93.184.216.34".parse::<IpAddr>().unwrap()],
    )]));
    NetGuard::off().with_resolver(Arc::new(table))
}

async fn get(
    client: &reqwest::Client,
    ctx: &RequestContext,
    url: &str,
) -> riptide_net::Result<u16> {
    let sent = riptide_net::http::send(
        client,
        ctx,
        reqwest::Method::GET,
        Url::parse(url).unwrap(),
        |r| r,
    )
    .await?;
    Ok(sent.response.status().as_u16())
}

#[tokio::test]
async fn public_to_private_redirect_is_refused_with_a_clear_error() {
    let server = TestServer::spawn().await;
    let client = client_for(&server);
    let port = Url::parse(server.base_url()).unwrap().port().unwrap();
    let start = format!("http://public.test:{port}/redirect?cross=1&to=ok");
    let ctx = RequestContext::new(&Url::parse(&start).unwrap()).with_guard(pretend_public());
    let err = get(&client, &ctx, &start).await.unwrap_err();
    assert!(matches!(err, Error::PrivateRedirect), "{err:?}");
    assert_eq!(err.to_string(), "redirected to a private network address");
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn public_to_public_redirects_still_work() {
    let server = TestServer::spawn().await;
    let client = client_for(&server);
    let port = Url::parse(server.base_url()).unwrap().port().unwrap();
    // same-host hops stay on public.test
    let start = format!("http://public.test:{port}/redirect?n=3&code=cycle&size=1000");
    let ctx = RequestContext::new(&Url::parse(&start).unwrap()).with_guard(pretend_public());
    assert_eq!(get(&client, &ctx, &start).await.unwrap(), 200);
}

#[tokio::test]
async fn a_chain_that_starts_on_a_private_address_may_go_to_private_addresses() {
    let server = TestServer::spawn().await;
    let client = client_for(&server);
    // the test server is on loopback: user-added LAN/localhost downloads keep working,
    // including a cross-origin hop to the second loopback listener
    let start = server.url("/redirect?cross=1&to=ok");
    let ctx = RequestContext::new(&Url::parse(&start).unwrap());
    assert_eq!(get(&client, &ctx, &start).await.unwrap(), 200);
}

#[tokio::test]
async fn a_blocking_guard_refuses_private_hops_even_from_a_private_start() {
    let server = TestServer::spawn().await;
    let client = client_for(&server);
    let start = server.url("/redirect?cross=1&to=ok");
    let ctx = RequestContext::new(&Url::parse(&start).unwrap()).with_guard(NetGuard::blocking());
    let err = get(&client, &ctx, &start).await.unwrap_err();
    assert!(matches!(err, Error::PrivateRedirect), "{err:?}");
}
