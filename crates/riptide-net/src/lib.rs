//! HTTP engine for Riptide.
//!
//! M0 ships only the shared client configuration; probing, segmented downloads, resume, retry and
//! auth land in M1 (lane A). Everything here is UI-agnostic (hard rule 9).
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, USER_AGENT};

/// Default `User-Agent`: `Riptide/<version>`.
pub const DEFAULT_USER_AGENT: &str = concat!("Riptide/", env!("CARGO_PKG_VERSION"));

/// Hard cap on redirects followed for one request (hard rule 3).
pub const MAX_REDIRECTS: usize = 20;

/// Time to establish a TCP/TLS connection before giving up.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Idle time on a body stream before the segment is retried.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Errors from building the client. Request-level errors arrive in M1.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `reqwest` refused the configuration (bad proxy, TLS backend, …).
    #[error("cannot build HTTP client: {0}")]
    Build(#[source] reqwest::Error),
}

/// Headers every request carries. `Accept-Encoding: identity` so `Content-Length` and `Range`
/// mean bytes on disk (hard rule 3).
pub fn default_headers(user_agent: Option<&str>) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    h.insert(ACCEPT, HeaderValue::from_static("*/*"));
    let ua = user_agent
        .and_then(|ua| HeaderValue::from_str(ua).ok())
        .unwrap_or_else(|| HeaderValue::from_static(DEFAULT_USER_AGENT));
    h.insert(USER_AGENT, ua);
    h
}

/// Builds the shared `reqwest::Client`: rustls, HTTP/2 allowed, system proxy respected,
/// redirects **not** followed automatically (the engine follows them itself so it can drop
/// `Authorization` / `Cookie` on cross-origin hops and cap the chain at [`MAX_REDIRECTS`]),
/// no automatic decompression, cookies off (the engine sends captured cookies explicitly and only
/// to their origin).
pub fn build_client(user_agent: Option<&str>) -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .default_headers(default_headers(user_agent))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .use_rustls_tls()
        .build()
        .map_err(Error::Build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_headers_force_identity_encoding() {
        let h = default_headers(None);
        assert_eq!(h.get(ACCEPT_ENCODING).unwrap(), "identity");
        assert_eq!(h.get(USER_AGENT).unwrap(), DEFAULT_USER_AGENT);
        assert!(DEFAULT_USER_AGENT.starts_with("Riptide/0."));
    }

    #[test]
    fn browser_user_agent_can_be_mimicked_but_bad_values_fall_back() {
        let h = default_headers(Some("Mozilla/5.0 (Windows NT 10.0) Chrome/130"));
        assert_eq!(
            h.get(USER_AGENT).unwrap(),
            "Mozilla/5.0 (Windows NT 10.0) Chrome/130"
        );
        let h = default_headers(Some("bad\nvalue"));
        assert_eq!(h.get(USER_AGENT).unwrap(), DEFAULT_USER_AGENT);
    }

    #[test]
    fn client_builds() {
        build_client(None).unwrap();
    }
}
