//! General web search through the backend the user chose. Results are web pages
//! ([`HitKind::Page`]), not files: the app offers "Find files on this page" for them.
//!
//! * **Brave Search API** (`GET /res/v1/web/search`): needs the user's own subscription token,
//!   sent in the `X-Subscription-Token` header only. The token is never put in a URL, a log line
//!   or an error, and the `Debug` output of [`WebBackend`] redacts it.
//! * **SearXNG** (`GET {base}/search?q=...&format=json`): a self-hosted or public instance. Most
//!   public instances switch the JSON format off and answer `403`; that surfaces as
//!   [`ResearchError::Http`] with `status: 403` (the user should use their own instance, or one
//!   that lists JSON as enabled). The base URL is checked by [`validate_searxng_base`].
//!
//! Everything in an answer is untrusted: only `http`/`https` result links are kept, titles and
//! snippets are stripped of markup and cut to length, and repeated links are dropped.

use std::fmt;

use async_trait::async_trait;
use serde_json::Value;
use url::{Host, Url};

use super::{check_query, clamp, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{jstr, strip_html, tidy, Category, Hit, HitKind, Source};

/// Where web results come from.
#[derive(Clone, PartialEq, Eq)]
pub enum WebBackend {
    /// Brave Search API with the user's subscription token.
    Brave {
        /// The API token (never logged, never part of an error or of `Debug` output).
        api_key: String,
    },
    /// A SearXNG instance (validated base URL).
    SearxNg {
        /// Base URL of the instance.
        base_url: String,
    },
}

impl fmt::Debug for WebBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebBackend::Brave { .. } => f
                .debug_struct("Brave")
                .field("api_key", &"<redacted>")
                .finish(),
            WebBackend::SearxNg { base_url } => f
                .debug_struct("SearxNg")
                .field("base_url", base_url)
                .finish(),
        }
    }
}

/// Web search adapter.
pub struct WebSearch {
    backend: WebBackend,
    brave_base: String,
    http: Http,
}

impl fmt::Debug for WebSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSearch")
            .field("backend", &self.backend)
            .finish()
    }
}

impl WebSearch {
    /// Builds the adapter for `backend`, talking to the real Brave API when that is chosen.
    pub fn new(backend: WebBackend) -> Self {
        Self::with_http(
            backend,
            "https://api.search.brave.com",
            Http::without_redirects(super::pacing(Source::Web)),
        )
    }

    /// Like [`WebSearch::new`] with an explicit Brave API base and client (tests).
    pub fn with_http(backend: WebBackend, brave_base: &str, http: Http) -> Self {
        WebSearch {
            backend,
            brave_base: brave_base.trim_end_matches('/').into(),
            http,
        }
    }

    /// Brave Search at `base` with `api_key` (tests).
    pub fn with_brave_base(base: &str, api_key: &str, http: Http) -> Self {
        Self::with_http(
            WebBackend::Brave {
                api_key: api_key.to_string(),
            },
            base,
            http,
        )
    }

    /// A SearXNG instance at `base_url` (not validated here: [`WebSearch::search_in`] validates
    /// it before every request).
    pub fn with_searx(base_url: &str, http: Http) -> Self {
        Self::with_http(
            WebBackend::SearxNg {
                base_url: base_url.to_string(),
            },
            "https://api.search.brave.com",
            http,
        )
    }

    async fn brave(&self, q: &str, wanted: usize, key: &str) -> Result<Vec<Hit>> {
        let key = key.trim();
        if key.is_empty() {
            return Err(ResearchError::Invalid(
                "the Brave Search token is empty".into(),
            ));
        }
        if !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
            return Err(ResearchError::Invalid(
                "the Brave Search token has characters a token cannot have".into(),
            ));
        }
        let url = format!(
            "{}/res/v1/web/search?q={}&count={}",
            self.brave_base,
            enc(q),
            wanted.min(20)
        );
        let body = self
            .http
            .json(|| {
                self.http
                    .get(&url)
                    .header("Accept", "application/json")
                    .header("X-Subscription-Token", key)
            })
            .await?;
        let results = body
            .pointer("/web/results")
            .and_then(Value::as_array)
            // a query without any hit has no `web` object at all
            .or_else(|| {
                body.get("type")
                    .and_then(Value::as_str)
                    .filter(|t| *t == "search")
                    .map(|_| &EMPTY)
            })
            .ok_or_else(|| ResearchError::Parse("no web.results field".into()))?;
        Ok(collect(results, "description", wanted))
    }

    async fn searx(&self, q: &str, wanted: usize, base: &str) -> Result<Vec<Hit>> {
        let base = validate_searxng_base(base)?;
        let url = format!("{base}/search?q={}&format=json", enc(q));
        let body = self
            .http
            .json(|| self.http.get(&url).header("Accept", "application/json"))
            .await?;
        let results = body
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(|| ResearchError::Parse("no results field".into()))?;
        Ok(collect(results, "content", wanted))
    }
}

static EMPTY: Vec<Value> = Vec::new();

/// Checks and normalises the base URL of a SearXNG instance: it must parse, use `https` (or
/// `http` for a loopback or private-network host), have a host, carry no credentials, no query
/// and no fragment. Returns it without a trailing slash. The error never repeats the input
/// (it could hold a password).
pub fn validate_searxng_base(raw: &str) -> Result<String> {
    let bad = |why: &str| ResearchError::Invalid(format!("SearXNG address: {why}"));
    let url = Url::parse(raw.trim()).map_err(|_| bad("not a valid URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("must not contain a user name or password"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(bad("must not contain a query or fragment"));
    }
    let Some(host) = url.host() else {
        return Err(bad("has no host"));
    };
    match url.scheme() {
        "https" => {}
        "http" => {
            let local = match host {
                Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
                Host::Ipv4(ip) => ip.is_loopback() || ip.is_private(),
                Host::Ipv6(ip) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00,
            };
            if !local {
                return Err(bad("plain http is only allowed for a local network host"));
            }
        }
        _ => return Err(bad("must start with https://")),
    }
    // `Url` drops the default port and lowercases the host
    let s = url.as_str().trim_end_matches('/');
    Ok(s.to_string())
}

/// Turns a backend's result array into page hits.
fn collect(results: &[Value], snippet_key: &str, wanted: usize) -> Vec<Hit> {
    let mut seen = std::collections::HashSet::new();
    let mut hits = Vec::new();
    for r in results {
        if hits.len() >= wanted {
            break;
        }
        let Some(raw) = r.get("url").and_then(jstr) else {
            continue;
        };
        let Some(url) = clean_result_url(raw) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let title = tidy(&decode(r.get("title").and_then(jstr).unwrap_or("")), 200);
        let title = if title.is_empty() {
            Url::parse(&url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| url.clone())
        } else {
            title
        };
        let mut h = Hit::new(Source::Web, &url, &title, &url);
        h.kind = HitKind::Page;
        h.page_url = Some(url.clone());
        h.summary = r
            .get(snippet_key)
            .and_then(jstr)
            .map(|s| tidy(&decode(s), 300))
            .filter(|s| !s.is_empty());
        hits.push(h);
    }
    hits
}

fn decode(s: &str) -> String {
    strip_html(s).replace("&#x27;", "'").replace("&#039;", "'")
}

/// `http`/`https` links with a host and no control characters, without fragment.
fn clean_result_url(raw: &str) -> Option<String> {
    if raw.chars().any(|c| c.is_control() || c.is_whitespace()) || raw.len() > 2000 {
        return None;
    }
    let mut url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

#[async_trait]
impl SearchSource for WebSearch {
    fn source(&self) -> Source {
        Source::Web
    }

    fn max_queries(&self) -> usize {
        1
    }

    async fn search_in(&self, _category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        match &self.backend {
            WebBackend::Brave { api_key } => self.brave(&q, wanted, api_key).await,
            WebBackend::SearxNg { base_url } => self.searx(&q, wanted, base_url).await,
        }
    }
}
