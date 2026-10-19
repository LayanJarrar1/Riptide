//! HTTP engine for Riptide: probe a URL, then fetch it over many connections into a
//! preallocated `.rtpart` file with byte-exact resume.
//!
//! * [`probe()`] — `HEAD`/ranged `GET`: size, range support, validators, type, file name.
//! * [`transfer()`] — the multi-connection transfer with work stealing, retries, checkpoints,
//!   restart on a changed file, verification and atomic rename.
//! * [`RequestContext`] — cookies, headers, credentials, referrer, user agent, applied only to
//!   the download's own origin.
//! * [`HostLimiter`] — per-host connection cap shared by every transfer.
//!
//! Everything here is UI-agnostic: the owner of a download drives it, receives
//! [`TransferEvent`]s and persists the [`TransferEvent::Checkpoint`]s.
#![cfg_attr(test, allow(clippy::unwrap_used))]
// `deny`, not `forbid`: one `cfg(windows)` module (`file::sparse`) needs a single Win32 call.
#![deny(unsafe_code)]

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, USER_AGENT};

pub mod context;
pub mod digest;
pub mod disposition;
pub mod error;
pub mod file;
pub mod http;
pub mod limiter;
pub mod net_guard;
pub mod probe;
pub mod proxy;
pub mod rate;
pub mod redirect;
pub mod retry;
pub mod speed;
pub mod transfer;
pub mod verify;

pub use context::RequestContext;
pub use error::{Error, Result};
pub use file::{part_path_for, PartFile, PART_EXTENSION};
pub use limiter::{CapObserver, HostLimiter, HostPermit};
pub use net_guard::{is_private_or_special, NetGuard};
pub use probe::{probe, Probe};
pub use proxy::{ProxyChoice, ProxyUrl};
pub use rate::RateLimiter;
pub use redirect::{parse_http_url, strip_query, Origin, MAX_REDIRECTS};
pub use retry::RetryPolicy;
pub use speed::SpeedMeter;
pub use transfer::{transfer, Outcome, TransferConfig, TransferEvent, TransferPlan};
pub use verify::{checksum_of, verify_file};

/// Default `User-Agent`: `Riptide/<version>`.
pub const DEFAULT_USER_AGENT: &str = concat!("Riptide/", env!("CARGO_PKG_VERSION"));

/// Time to establish a TCP/TLS connection before giving up.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Idle time on a body stream before the segment is retried.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// How the shared client is built.
#[derive(Debug, Clone, Default)]
pub struct ClientOptions {
    /// `User-Agent` override (a browser's, when the link came from the extension).
    pub user_agent: Option<String>,
    /// Skip TLS certificate verification for this client. Never a default; the UI shows a
    /// warning when it is on.
    pub accept_invalid_certs: bool,
    /// Proxy for every request of this client.
    pub proxy: ProxyChoice,
}

/// Headers every request carries. `Accept-Encoding: identity` so `Content-Length` and `Range`
/// mean bytes on disk.
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

/// Builds the shared `reqwest::Client`: rustls, HTTP/2 allowed, proxy per [`ProxyChoice`],
/// redirects **not** followed automatically (the engine follows them itself so it can drop
/// `Authorization` / `Cookie` on cross-origin hops and cap the chain at [`MAX_REDIRECTS`]),
/// no automatic decompression, no cookie jar (the engine sends captured cookies explicitly and
/// only to their origin).
pub fn build_client(options: &ClientOptions) -> Result<reqwest::Client> {
    build_client_inner(options, None)
}

/// Like [`build_client`], with connections limited to public addresses by `guard`'s resolver
/// (see [`NetGuard::dns_resolver`]): for crawls that start from a public host.
pub fn build_client_guarded(options: &ClientOptions, guard: &NetGuard) -> Result<reqwest::Client> {
    build_client_inner(options, Some(guard.dns_resolver()))
}

fn build_client_inner(
    options: &ClientOptions,
    dns: Option<std::sync::Arc<net_guard::GuardedDns>>,
) -> Result<reqwest::Client> {
    let mut builder = options.proxy.apply(reqwest::Client::builder())?;
    if let Some(dns) = dns {
        builder = builder.dns_resolver(dns);
    }
    builder
        .default_headers(default_headers(options.user_agent.as_deref()))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .use_rustls_tls()
        .danger_accept_invalid_certs(options.accept_invalid_certs)
        .build()
        .map_err(Error::from)
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
        build_client(&ClientOptions::default()).unwrap();
        build_client(&ClientOptions {
            user_agent: Some("x".into()),
            accept_invalid_certs: true,
            ..Default::default()
        })
        .unwrap();
    }
}
