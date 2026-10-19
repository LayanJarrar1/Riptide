//! Per-download request context: referrer, user agent, cookies, extra headers and credentials,
//! applied to every request of that download and **only** to its own origin.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION, COOKIE, REFERER, USER_AGENT};
use reqwest::RequestBuilder;
use riptide_core::{Credentials, Secrets};
use url::Url;

use crate::digest::{basic_authorization, client_nonce, DigestChallenge};
use crate::net_guard::NetGuard;
use crate::redirect::Origin;

/// Which authentication the origin accepted, remembered so later requests send it up front.
#[derive(Debug)]
enum AuthState {
    Basic,
    Digest {
        challenge: DigestChallenge,
        nc: AtomicU32,
    },
}

/// Everything that shapes a download's requests.
#[derive(Debug)]
pub struct RequestContext {
    origin: Origin,
    referrer: Option<HeaderValue>,
    user_agent: Option<HeaderValue>,
    cookies: Option<HeaderValue>,
    headers: Vec<(HeaderName, HeaderValue)>,
    credentials: Option<Credentials>,
    auth: Mutex<Option<AuthState>>,
    guard: NetGuard,
}

impl RequestContext {
    /// A context for `url` with nothing else set. The origin of `url` is the only origin that
    /// will ever receive the cookies, headers and credentials added later.
    pub fn new(url: &Url) -> Self {
        RequestContext {
            origin: Origin::of(url),
            referrer: None,
            user_agent: None,
            cookies: None,
            headers: Vec::new(),
            credentials: None,
            auth: Mutex::new(None),
            guard: NetGuard::off(),
        }
    }

    /// The private-address guard for redirects (and, for crawls, for every hop). The default
    /// only refuses a redirect from a public host to a private address.
    pub fn with_guard(mut self, guard: NetGuard) -> Self {
        self.guard = guard;
        self
    }

    /// The guard applied to redirects.
    pub fn guard(&self) -> &NetGuard {
        &self.guard
    }

    /// `Referer` for every request (page the link was found on). Sent to every origin: it is
    /// not a secret and servers use it for hot-link checks.
    pub fn with_referrer(mut self, referrer: Option<&str>) -> Self {
        self.referrer = referrer.and_then(|r| HeaderValue::from_str(r).ok());
        self
    }

    /// Overrides the client's `User-Agent` (to mimic the browser that captured the link).
    pub fn with_user_agent(mut self, ua: Option<&str>) -> Self {
        self.user_agent = ua.and_then(|u| HeaderValue::from_str(u).ok());
        self
    }

    /// Cookies, headers and credentials. Header names that the engine controls (`Range`,
    /// `Host`, `Accept-Encoding`, `Authorization`, `Cookie`, `Content-Length`) are ignored.
    pub fn with_secrets(mut self, secrets: &Secrets) -> Self {
        self.cookies = secrets
            .cookies
            .as_deref()
            .and_then(|c| HeaderValue::from_str(c).ok());
        self.headers = secrets
            .headers
            .iter()
            .filter_map(|(k, v)| {
                let name = HeaderName::from_bytes(k.trim().as_bytes()).ok()?;
                if is_reserved(&name) {
                    return None;
                }
                let value = HeaderValue::from_str(v.trim()).ok()?;
                Some((name, value))
            })
            .collect();
        self.credentials = secrets.auth.clone();
        self
    }

    /// The origin this context belongs to.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Whether `url` is on the context's origin.
    pub fn same_origin(&self, url: &Url) -> bool {
        Origin::of(url) == self.origin
    }

    /// Credentials, if any.
    pub fn credentials(&self) -> Option<&Credentials> {
        self.credentials.as_ref()
    }

    /// Adds the context's headers to a request for `url`. Cookies, custom headers and
    /// credentials are added only when `url` is on the context's origin.
    pub fn apply(&self, mut req: RequestBuilder, url: &Url, method: &str) -> RequestBuilder {
        if let Some(r) = &self.referrer {
            req = req.header(REFERER, r.clone());
        }
        if let Some(ua) = &self.user_agent {
            req = req.header(USER_AGENT, ua.clone());
        }
        if !self.same_origin(url) {
            return req;
        }
        if let Some(c) = &self.cookies {
            req = req.header(COOKIE, c.clone());
        }
        for (k, v) in &self.headers {
            req = req.header(k.clone(), v.clone());
        }
        if let Some(value) = self.preemptive_authorization(url, method) {
            req = req.header(AUTHORIZATION, value);
        }
        req
    }

    /// `Authorization` for a request on an origin that already challenged us.
    fn preemptive_authorization(&self, url: &Url, method: &str) -> Option<HeaderValue> {
        let creds = self.credentials.as_ref()?;
        let guard = self.auth.lock().ok()?;
        match guard.as_ref()? {
            AuthState::Basic => {
                HeaderValue::from_str(&basic_authorization(&creds.username, &creds.password)).ok()
            }
            AuthState::Digest { challenge, nc } => {
                let n = nc.fetch_add(1, Ordering::SeqCst) + 1;
                let uri = request_uri(url);
                let value = challenge.authorization(
                    &creds.username,
                    &creds.password,
                    method,
                    &uri,
                    n,
                    &client_nonce(),
                );
                HeaderValue::from_str(&value).ok()
            }
        }
    }

    /// Answers a `WWW-Authenticate` challenge once and remembers the scheme for next time.
    /// Returns `None` when there are no credentials or the challenge is unsupported.
    pub fn answer_challenge(
        &self,
        www_authenticate: &str,
        url: &Url,
        method: &str,
    ) -> Option<HeaderValue> {
        let creds = self.credentials.as_ref()?;
        let header = www_authenticate.trim();
        if let Some(challenge) = crate::digest::parse_challenge(header) {
            let uri = request_uri(url);
            let value = challenge.authorization(
                &creds.username,
                &creds.password,
                method,
                &uri,
                1,
                &client_nonce(),
            );
            if let Ok(mut guard) = self.auth.lock() {
                *guard = Some(AuthState::Digest {
                    challenge,
                    nc: AtomicU32::new(1),
                });
            }
            return HeaderValue::from_str(&value).ok();
        }
        if header.len() >= 5 && header[..5].eq_ignore_ascii_case("basic") {
            if let Ok(mut guard) = self.auth.lock() {
                *guard = Some(AuthState::Basic);
            }
            return HeaderValue::from_str(&basic_authorization(&creds.username, &creds.password))
                .ok();
        }
        None
    }

    /// Forgets a remembered scheme (after a 401 with the remembered scheme, e.g. a stale nonce).
    pub fn forget_auth(&self) {
        if let Ok(mut guard) = self.auth.lock() {
            *guard = None;
        }
    }
}

/// Path + query as it goes on the request line, which is what Digest's `uri` must be.
fn request_uri(url: &Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_string(),
    }
}

fn is_reserved(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "range"
            | "host"
            | "accept-encoding"
            | "authorization"
            | "cookie"
            | "content-length"
            | "if-range"
            | "connection"
            | "transfer-encoding"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        let secrets = Secrets {
            cookies: Some("session=abc".into()),
            headers: vec![
                ("X-Token".into(), "t1".into()),
                ("Range".into(), "bytes=0-1".into()),
                ("bad name".into(), "x".into()),
            ],
            auth: Some(Credentials {
                username: "user".into(),
                password: "pass".into(),
            }),
        };
        RequestContext::new(&Url::parse("https://files.example/a/b.zip").unwrap())
            .with_referrer(Some("https://page.example/"))
            .with_user_agent(Some("Mozilla/5.0 Test"))
            .with_secrets(&secrets)
    }

    fn headers_of(ctx: &RequestContext, url: &str) -> reqwest::header::HeaderMap {
        let client = reqwest::Client::new();
        let u = Url::parse(url).unwrap();
        let req = ctx.apply(client.get(u.clone()), &u, "GET").build().unwrap();
        req.headers().clone()
    }

    #[test]
    fn same_origin_gets_everything_cross_origin_only_referrer_and_ua() {
        let c = ctx();
        let same = headers_of(&c, "https://files.example/other/path?x=1");
        assert_eq!(same.get(COOKIE).unwrap(), "session=abc");
        assert_eq!(same.get("x-token").unwrap(), "t1");
        assert!(same.get("range").is_none(), "reserved header dropped");
        assert_eq!(same.get(REFERER).unwrap(), "https://page.example/");
        assert_eq!(same.get(USER_AGENT).unwrap(), "Mozilla/5.0 Test");
        assert!(
            same.get(AUTHORIZATION).is_none(),
            "no preemptive auth before a challenge"
        );

        let cross = headers_of(&c, "https://cdn.other.example/a/b.zip");
        assert!(cross.get(COOKIE).is_none());
        assert!(cross.get("x-token").is_none());
        assert_eq!(cross.get(REFERER).unwrap(), "https://page.example/");
        assert_eq!(cross.get(USER_AGENT).unwrap(), "Mozilla/5.0 Test");

        let port = headers_of(&c, "https://files.example:8443/a/b.zip");
        assert!(
            port.get(COOKIE).is_none(),
            "different port is a different origin"
        );
    }

    #[test]
    fn challenges_are_answered_and_remembered() {
        let c = ctx();
        let u = Url::parse("https://files.example/a/b.zip?q=1").unwrap();
        let basic = c.answer_challenge("Basic realm=\"r\"", &u, "GET").unwrap();
        assert_eq!(basic, "Basic dXNlcjpwYXNz");
        assert_eq!(
            headers_of(&c, "https://files.example/next")
                .get(AUTHORIZATION)
                .unwrap(),
            "Basic dXNlcjpwYXNz"
        );

        let digest = c
            .answer_challenge("Digest realm=\"r\", nonce=\"n1\", qop=\"auth\"", &u, "GET")
            .unwrap();
        let s = digest.to_str().unwrap();
        assert!(s.starts_with("Digest username=\"user\""));
        assert!(s.contains("uri=\"/a/b.zip?q=1\""));
        assert!(s.contains("nc=00000001"));
        let next = headers_of(&c, "https://files.example/next");
        assert!(
            next.get(AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("nc=00000002"),
            "nonce count advances"
        );
        assert!(headers_of(&c, "https://elsewhere.example/")
            .get(AUTHORIZATION)
            .is_none());

        assert!(c.answer_challenge("Negotiate", &u, "GET").is_none());
        c.forget_auth();
        assert!(headers_of(&c, "https://files.example/next")
            .get(AUTHORIZATION)
            .is_none());
        let none = RequestContext::new(&u);
        assert!(
            none.answer_challenge("Basic realm=\"r\"", &u, "GET")
                .is_none(),
            "no credentials, no answer"
        );
    }
}
