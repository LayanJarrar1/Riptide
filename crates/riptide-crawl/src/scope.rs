//! URL normalisation (the de-duplication key) and the scope rules.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

/// Which pages the crawler may walk into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Only the start page's host (same port, `http` and `https` count as one).
    #[default]
    SameHost,
    /// The start page's registrable domain: `www.example.com` and `cdn.example.com` match
    /// `example.com`. The domain is approximated (last two labels, three for `co.uk`-style
    /// suffixes); there is no public-suffix list. IP addresses fall back to [`Scope::SameHost`].
    SameDomain,
    /// Anywhere. Bounded only by depth and the caps.
    Any,
}

impl Scope {
    /// Whether `url` is inside this scope of `start`.
    #[must_use]
    pub fn contains(self, start: &Url, url: &Url) -> bool {
        match self {
            Scope::SameHost => same_host(start, url),
            Scope::SameDomain => same_domain(start, url),
            Scope::Any => true,
        }
    }
}

/// The canonical form two links must share to count as the same resource: http(s) only, no
/// fragment, no user info (a password found in a page is never replayed), no empty `?`.
/// The `url` crate already lower-cases scheme and host, drops default ports and resolves dot
/// segments. Returns `None` for any other scheme or a URL without a host. Idempotent.
#[must_use]
pub fn normalize_url(url: &Url) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.host_str().filter(|h| !h.is_empty())?;
    let mut out = url.clone();
    out.set_fragment(None);
    // `set_username`/`set_password` only fail for cannot-be-a-base URLs, which have no host
    let _ = out.set_username("");
    let _ = out.set_password(None);
    if out.query() == Some("") {
        out.set_query(None);
    }
    Some(out)
}

/// Same host and port; the two default ports (`80` for `http`, `443` for `https`) are treated as
/// equal so an `http` to `https` upgrade stays in scope.
#[must_use]
pub fn same_host(a: &Url, b: &Url) -> bool {
    if a.host_str() != b.host_str() {
        return false;
    }
    a.port_or_known_default() == b.port_or_known_default()
        || (a.port().is_none() && b.port().is_none())
}

/// Same registrable domain (see [`Scope::SameDomain`]).
#[must_use]
pub fn same_domain(a: &Url, b: &Url) -> bool {
    match (a.host(), b.host()) {
        (Some(Host::Domain(x)), Some(Host::Domain(y))) => domain_of(x) == domain_of(y),
        _ => same_host(a, b),
    }
}

/// `www.example.com` → `example.com`; `a.b.example.co.uk` → `example.co.uk`.
#[must_use]
pub fn domain_of(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<IpAddr>().is_ok() {
        return host;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return host;
    }
    let (tld, sld) = (labels[n - 1], labels[n - 2]);
    let take = if tld.len() == 2
        && matches!(
            sld,
            "co" | "com" | "org" | "net" | "gov" | "edu" | "ac" | "or" | "ne" | "go"
        ) {
        3
    } else {
        2
    };
    labels[n - take..].join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn normalisation_drops_fragment_userinfo_and_non_http() {
        let n = normalize_url(&u("HTTP://User:pw@Example.COM:80/a/./b/../c?#frag")).unwrap();
        assert_eq!(n.as_str(), "http://example.com/a/c");
        assert!(normalize_url(&u("mailto:a@b.c")).is_none());
        assert!(normalize_url(&u("javascript:void(0)")).is_none());
        assert!(normalize_url(&u("ftp://example.com/x")).is_none());
        assert!(normalize_url(&u("data:text/plain,hi")).is_none());
        // the query is data: kept, and not reordered
        assert_eq!(
            normalize_url(&u("https://e.com/x?b=1&a=2#z"))
                .unwrap()
                .as_str(),
            "https://e.com/x?b=1&a=2"
        );
    }

    #[test]
    fn scope_rules() {
        let start = u("https://www.example.com/start");
        assert!(Scope::SameHost.contains(&start, &u("http://www.example.com/x")));
        assert!(!Scope::SameHost.contains(&start, &u("https://cdn.example.com/x")));
        assert!(!Scope::SameHost.contains(&start, &u("https://www.example.com:8443/x")));
        assert!(Scope::SameDomain.contains(&start, &u("https://cdn.example.com/x")));
        assert!(!Scope::SameDomain.contains(&start, &u("https://example.org/x")));
        assert!(!Scope::SameDomain.contains(&start, &u("https://notexample.com/x")));
        assert!(Scope::Any.contains(&start, &u("https://example.org/x")));
        // IPs and localhost never widen
        let ip = u("http://127.0.0.1:1000/");
        assert!(Scope::SameDomain.contains(&ip, &u("http://127.0.0.1:1000/x")));
        assert!(!Scope::SameDomain.contains(&ip, &u("http://127.0.0.1:2000/x")));
    }

    #[test]
    fn registrable_domains() {
        assert_eq!(domain_of("example.com"), "example.com");
        assert_eq!(domain_of("a.b.example.com"), "example.com");
        assert_eq!(domain_of("a.example.co.uk"), "example.co.uk");
        assert_eq!(domain_of("localhost"), "localhost");
        assert_eq!(domain_of("EXAMPLE.com."), "example.com");
    }
}
