//! Redirect rules the engine follows by hand: a hard cap, `303` turns into
//! `GET`, and credentials never cross an origin boundary.

use reqwest::Method;
use url::Url;

use crate::error::{Error, Result};

/// Hard cap on redirects per request.
pub const MAX_REDIRECTS: usize = 20;

/// Scheme + host + port triple that decides whether cookies and credentials may be sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    /// `http` or `https`.
    pub scheme: String,
    /// Lower-case host.
    pub host: String,
    /// Effective port.
    pub port: u16,
}

impl Origin {
    /// Origin of a URL (port defaults from the scheme).
    pub fn of(url: &Url) -> Origin {
        Origin {
            scheme: url.scheme().to_ascii_lowercase(),
            host: url.host_str().unwrap_or("").to_ascii_lowercase(),
            port: url.port_or_known_default().unwrap_or(0),
        }
    }

    /// `host:port` as the test server's `/needs-referer` and logs show it.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Whether `a` and `b` are different origins (different scheme, host or port).
pub fn is_cross_origin(a: &Url, b: &Url) -> bool {
    Origin::of(a) != Origin::of(b)
}

/// Whether `status` is a redirect the engine follows.
pub fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Method for the next hop: `303` always becomes `GET` (and drops the body); the others keep
/// the method. `HEAD` stays `HEAD` in every case.
pub fn next_method(status: u16, current: &Method) -> Method {
    if *current == Method::HEAD {
        return Method::HEAD;
    }
    if status == 303 {
        return Method::GET;
    }
    current.clone()
}

/// Resolves a `Location` header against the current URL; only http(s) targets are accepted.
pub fn resolve_location(current: &Url, location: &str) -> Result<Url> {
    let next = current
        .join(location.trim())
        .map_err(|e| Error::InvalidUrl {
            url: location.to_string(),
            reason: e.to_string(),
        })?;
    if next.scheme() != "http" && next.scheme() != "https" {
        return Err(Error::InvalidUrl {
            url: next.scheme().to_string(),
            reason: "redirect target must be http or https".into(),
        });
    }
    Ok(next)
}

/// Parses an http(s) URL for the engine.
pub fn parse_http_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim()).map_err(|e| Error::InvalidUrl {
        url: strip_query(raw),
        reason: e.to_string(),
    })?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(Error::InvalidUrl {
            url: strip_query(raw),
            reason: "only http and https are supported".into(),
        });
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Error::InvalidUrl {
            url: strip_query(raw),
            reason: "missing host".into(),
        });
    }
    Ok(url)
}

/// URL without its query string, for logs and error messages.
pub fn strip_query(raw: &str) -> String {
    match raw.find(['?', '#']) {
        Some(i) => format!("{}?[stripped]", &raw[..i]),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_compare_scheme_host_port() {
        let a = Url::parse("https://Example.com/a").unwrap();
        let b = Url::parse("https://example.com:443/b?x=1").unwrap();
        let c = Url::parse("http://example.com/a").unwrap();
        let d = Url::parse("https://example.com:8443/a").unwrap();
        assert!(!is_cross_origin(&a, &b));
        assert!(is_cross_origin(&a, &c));
        assert!(is_cross_origin(&a, &d));
        assert_eq!(Origin::of(&c).authority(), "example.com:80");
    }

    #[test]
    fn methods_and_locations() {
        assert_eq!(next_method(303, &Method::POST), Method::GET);
        assert_eq!(next_method(307, &Method::POST), Method::POST);
        assert_eq!(next_method(303, &Method::HEAD), Method::HEAD);
        assert_eq!(next_method(302, &Method::GET), Method::GET);
        let cur = Url::parse("https://a.example/dir/file?x=1").unwrap();
        assert_eq!(
            resolve_location(&cur, "/other?y=2").unwrap().as_str(),
            "https://a.example/other?y=2"
        );
        assert_eq!(
            resolve_location(&cur, "next").unwrap().as_str(),
            "https://a.example/dir/next"
        );
        assert_eq!(
            resolve_location(&cur, "http://b.example:81/")
                .unwrap()
                .as_str(),
            "http://b.example:81/"
        );
        assert!(resolve_location(&cur, "ftp://x/").is_err());
        assert!(resolve_location(&cur, "file:///etc/passwd").is_err());
        assert!(is_redirect(308) && !is_redirect(304) && !is_redirect(200));
    }

    #[test]
    fn url_parsing_and_log_stripping() {
        assert!(parse_http_url("https://x.example/f.zip?sig=secret").is_ok());
        assert!(parse_http_url("ftp://x/f").is_err());
        assert!(parse_http_url("not a url").is_err());
        assert!(parse_http_url("https://").is_err());
        assert_eq!(strip_query("https://x/f?token=1"), "https://x/f?[stripped]");
        assert_eq!(strip_query("https://x/f"), "https://x/f");
        match parse_http_url("ftp://h/p?secret=1") {
            Err(Error::InvalidUrl { url, .. }) => assert_eq!(url, "ftp://h/p?[stripped]"),
            other => panic!("{other:?}"),
        }
    }
}
