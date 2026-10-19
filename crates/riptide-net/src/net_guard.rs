//! Keeps requests that untrusted page content steers away from private networks.
//!
//! Two situations are guarded, and nothing else:
//!
//! * the **site grabber**, which follows links found in pages: when the crawl starts from a
//!   public host it must not fetch loopback, link-local, private, CGNAT, multicast or
//!   unspecified addresses (a page could otherwise make this machine request `127.0.0.1`,
//!   `169.254.169.254` or the router). The crawl uses [`NetGuard::blocking`], which checks the
//!   address a name resolves to and, through [`NetGuard::dns_resolver`], also makes the
//!   connection itself use only checked addresses (so DNS rebinding cannot swap them);
//! * a **redirect** from a public host to a private address, on any request: refused with
//!   [`Error::PrivateRedirect`] unless the original URL was private too.
//!
//! A download the user adds directly (a NAS, `localhost`, the test server) is never checked:
//! the user chose that address. A crawl or redirect chain that starts on a private address may
//! go to private addresses as well (the owner crawling their own LAN).
//!
//! The one gap: a redirect target given as a host *name* is resolved here and again by the
//! connection, so a hostile DNS server could answer differently the second time. Crawls close
//! that gap with the guarded resolver; the redirect rule alone cannot (the shared client is
//! also used for the user's own LAN downloads).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use url::{Host, Url};

use crate::error::{Error, Result};

/// `true` for addresses a page must not be able to make us request: loopback, unspecified,
/// private (RFC 1918, ULA), link-local, CGNAT, multicast, broadcast, reserved, benchmarking and
/// documentation ranges, and IPv6 forms that embed such an IPv4 address (IPv4-mapped,
/// IPv4-compatible, NAT64, 6to4); Teredo is refused outright.
pub fn is_private_or_special(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_special_v4(v4),
        IpAddr::V6(v6) => is_special_v6(v6),
    }
}

fn is_special_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_unspecified()           // 0.0.0.0
        || a == 0                 // 0.0.0.0/8 "this network"
        || ip.is_loopback()       // 127.0.0.0/8
        || ip.is_private()        // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local()     // 169.254/16 (cloud metadata lives here)
        || (a == 100 && (64..=127).contains(&b)) // 100.64/10 CGNAT
        || ip.is_broadcast()      // 255.255.255.255
        || ip.is_multicast()      // 224/4
        || a >= 240               // 240/4 reserved
        || (a == 192 && b == 0 && c == 0)        // 192.0.0.0/24 IETF protocol assignments
        || (a == 192 && b == 0 && c == 2)        // 192.0.2.0/24 documentation
        || (a == 198 && (b == 18 || b == 19))    // 198.18/15 benchmarking
        || (a == 198 && b == 51 && c == 100)     // 198.51.100.0/24 documentation
        || (a == 203 && b == 0 && c == 113) // 203.0.113.0/24 documentation
}

fn is_special_v6(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return true;
    }
    let s = ip.segments();
    // IPv4-mapped ::ffff:a.b.c.d and the deprecated IPv4-compatible ::a.b.c.d
    if s[..5] == [0, 0, 0, 0, 0] && (s[5] == 0xffff || s[5] == 0) {
        return is_special_v4(embedded_v4(s[6], s[7]));
    }
    // NAT64 64:ff9b::/96 embeds an IPv4 address; the local-use 64:ff9b:1::/48 is never public
    if s[0] == 0x0064 && s[1] == 0xff9b {
        return s[2] == 1 || (s[2..6] == [0, 0, 0, 0] && is_special_v4(embedded_v4(s[6], s[7])));
    }
    // 6to4 2002::/16 carries the IPv4 address in bits 16..48
    if s[0] == 0x2002 {
        return is_special_v4(embedded_v4(s[1], s[2]));
    }
    (s[0] & 0xfe00) == 0xfc00        // fc00::/7 unique local
        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        || (s[0] & 0xffc0) == 0xfec0 // fec0::/10 site-local (deprecated)
        || (s[0] == 0x2001 && s[1] == 0) // 2001::/32 Teredo
        || (s[0] == 0x2001 && s[1] == 0x0db8) // 2001:db8::/32 documentation
        || (s[0] == 0x0100 && s[1..4] == [0, 0, 0]) // 100::/64 discard-only
}

fn embedded_v4(hi: u16, lo: u16) -> Ipv4Addr {
    let [a, b] = hi.to_be_bytes();
    let [c, d] = lo.to_be_bytes();
    Ipv4Addr::new(a, b, c, d)
}

/// The IP address a URL names directly. The URL parser already folds decimal, hexadecimal and
/// octal IPv4 forms (`2130706433`, `0x7f.1`, `0177.0.0.1`) and bracketed IPv6 into addresses,
/// so none of them slips past a literal check. `None` for a host *name*.
pub fn url_ip(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        Host::Domain(_) => None,
    }
}

/// `localhost` and `*.localhost` always mean this machine, whatever a resolver says.
fn is_localhost_name(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

/// The boxed future of a [`HostResolver`].
pub type ResolveFuture<'a> =
    Pin<Box<dyn Future<Output = std::io::Result<Vec<IpAddr>>> + Send + 'a>>;

/// Turns a host name into addresses. [`SystemResolver`] is the real one; tests inject
/// [`StaticResolver`] to pretend a name is public.
pub trait HostResolver: Send + Sync + fmt::Debug {
    /// The addresses of `host`.
    fn resolve<'a>(&'a self, host: &'a str) -> ResolveFuture<'a>;
}

/// The operating system's resolver.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemResolver;

impl HostResolver for SystemResolver {
    fn resolve<'a>(&'a self, host: &'a str) -> ResolveFuture<'a> {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host, 0)).await?;
            Ok(addrs.map(|a| a.ip()).collect())
        })
    }
}

/// A fixed name table, for tests. Unknown names fail to resolve.
#[derive(Debug, Default, Clone)]
pub struct StaticResolver(pub HashMap<String, Vec<IpAddr>>);

impl StaticResolver {
    /// A table with one entry.
    pub fn with(host: &str, ips: &[IpAddr]) -> Self {
        StaticResolver(HashMap::from([(host.to_ascii_lowercase(), ips.to_vec())]))
    }
}

impl HostResolver for StaticResolver {
    fn resolve<'a>(&'a self, host: &'a str) -> ResolveFuture<'a> {
        Box::pin(async move {
            self.0
                .get(&host.to_ascii_lowercase())
                .cloned()
                .ok_or_else(|| std::io::Error::other("unknown host"))
        })
    }
}

/// What to check, and how names are resolved.
#[derive(Debug, Clone)]
pub struct NetGuard {
    resolver: Arc<dyn HostResolver>,
    block_private: bool,
    allow_private_redirects: bool,
}

impl Default for NetGuard {
    fn default() -> Self {
        NetGuard::off()
    }
}

impl NetGuard {
    /// No blanket blocking (the default for requests the user chose); the redirect rule still
    /// applies.
    pub fn off() -> NetGuard {
        NetGuard {
            resolver: Arc::new(SystemResolver),
            block_private: false,
            allow_private_redirects: false,
        }
    }

    /// Every request, and every redirect hop, must target a public address.
    pub fn blocking() -> NetGuard {
        NetGuard {
            block_private: true,
            ..NetGuard::off()
        }
    }

    /// The user's explicit choice (`allow_private_redirects` in the engine settings) to follow
    /// redirects from a public host to a private address (NAS relays, split-horizon DNS). It
    /// never relaxes a [`NetGuard::blocking`] guard.
    #[must_use]
    pub fn with_allow_private_redirects(mut self, allow: bool) -> NetGuard {
        self.allow_private_redirects = allow;
        self
    }

    /// Uses `resolver` for names (tests).
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn HostResolver>) -> NetGuard {
        self.resolver = resolver;
        self
    }

    /// The guard for a crawl that starts at `start`: [`NetGuard::blocking`] when the start is a
    /// public host, [`NetGuard::off`] when it is itself private (the owner crawling their own
    /// LAN, or the test server).
    pub async fn for_start(start: &Url, resolver: Option<Arc<dyn HostResolver>>) -> NetGuard {
        let mut guard = NetGuard::off();
        if let Some(r) = resolver {
            guard = guard.with_resolver(r);
        }
        if !guard.is_private_start(start).await {
            guard.block_private = true;
        }
        guard
    }

    /// Whether this guard blocks private targets outright.
    pub fn blocks(&self) -> bool {
        self.block_private
    }

    /// Whether `url`'s host is, or resolves to, a private or special address. A name that does
    /// not resolve counts as public (the request will fail on its own).
    pub async fn is_private_host(&self, url: &Url) -> bool {
        if let Some(ip) = url_ip(url) {
            return is_private_or_special(ip);
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        if is_localhost_name(host) {
            return true;
        }
        match self.resolver.resolve(host).await {
            Ok(ips) => ips.into_iter().any(is_private_or_special),
            Err(_) => false,
        }
    }

    /// Whether a chain or crawl starting at `url` counts as private: an IP literal that is
    /// private, `localhost`, or a name whose answers are *all* private. A public name with one
    /// private record is still public (otherwise a hostile DNS zone could switch the guard off).
    pub async fn is_private_start(&self, url: &Url) -> bool {
        if let Some(ip) = url_ip(url) {
            return is_private_or_special(ip);
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        if is_localhost_name(host) {
            return true;
        }
        match self.resolver.resolve(host).await {
            Ok(ips) => !ips.is_empty() && ips.into_iter().all(is_private_or_special),
            Err(_) => false,
        }
    }

    /// Fails with [`Error::PrivateAddress`] when this guard blocks and `url` targets a private
    /// address. A no-op for [`NetGuard::off`].
    pub async fn check(&self, url: &Url) -> Result<()> {
        if self.block_private {
            self.deny_private(url).await
        } else {
            Ok(())
        }
    }

    /// Fails with [`Error::PrivateAddress`] when `url` targets a private address, whatever the
    /// mode.
    pub async fn deny_private(&self, url: &Url) -> Result<()> {
        if self.is_private_host(url).await {
            Err(Error::PrivateAddress)
        } else {
            Ok(())
        }
    }

    /// The redirect rule: following `from` to `next` is refused when `next` is private and the
    /// chain started on a public host (always, for a [`NetGuard::blocking`] guard).
    pub(crate) async fn check_redirect(&self, from: &Url, next: &Url) -> Result<()> {
        if self.allow_private_redirects && !self.block_private {
            return Ok(());
        }
        if self.block_private || !self.is_private_start(from).await {
            self.deny_private(next)
                .await
                .map_err(|_| Error::PrivateRedirect)
        } else {
            Ok(())
        }
    }

    /// A connection-level resolver for `reqwest` that only ever returns public addresses: the
    /// address that was checked is the address that is connected to. Only meaningful for a
    /// blocking guard; combine it with [`NetGuard::check`] for IP-literal URLs, which never
    /// reach DNS. (Through an HTTP proxy the proxy resolves names, so this does not apply.)
    pub fn dns_resolver(&self) -> Arc<GuardedDns> {
        Arc::new(GuardedDns {
            resolver: Arc::clone(&self.resolver),
        })
    }
}

/// The `reqwest` resolver behind [`NetGuard::dns_resolver`].
pub struct GuardedDns {
    resolver: Arc<dyn HostResolver>,
}

impl reqwest::dns::Resolve for GuardedDns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = Arc::clone(&self.resolver);
        Box::pin(async move {
            let host = name.as_str();
            let addrs: Vec<SocketAddr> = if is_localhost_name(host) {
                Vec::new()
            } else {
                resolver
                    .resolve(host)
                    .await?
                    .into_iter()
                    .filter(|ip| !is_private_or_special(*ip))
                    .map(|ip| SocketAddr::new(ip, 0))
                    .collect()
            };
            if addrs.is_empty() {
                return Err("the host resolves only to private network addresses".into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn ipv4_special_ranges_are_blocked_and_public_ones_are_not() {
        for blocked in [
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "127.255.255.254",
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.1",
            "192.168.255.255",
            "169.254.169.254",
            "169.254.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.7",
            "203.0.113.9",
        ] {
            assert!(is_private_or_special(ip(blocked)), "{blocked}");
        }
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.15.255.255",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "192.167.1.1",
            "192.169.0.1",
            "198.17.0.1",
            "198.20.0.1",
            "11.0.0.1",
            "223.255.255.255",
            "169.253.1.1",
            "169.255.1.1",
        ] {
            assert!(!is_private_or_special(ip(public)), "{public}");
        }
    }

    #[test]
    fn ipv6_special_ranges_and_embedded_ipv4_are_blocked() {
        for blocked in [
            "::",
            "::1",
            "fe80::1",
            "febf::1",
            "fc00::1",
            "fd12:3456::1",
            "fec0::1",
            "ff02::1",
            "ff0e::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
            "::ffff:0.0.0.0",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b::a9fe:a9fe",
            "64:ff9b:1::1",
            "2002:7f00:1::1",
            "2002:c0a8:101::1",
            "2001::1",
            "2001:0:4136:e378::1",
            "2001:db8::1",
            "100::1",
        ] {
            assert!(is_private_or_special(ip(blocked)), "{blocked}");
        }
        for public in [
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "2a00:1450:4001::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
            "2001:db9::1",
            "1::1",
        ] {
            assert!(!is_private_or_special(ip(public)), "{public}");
        }
    }

    #[test]
    fn encoded_ip_hosts_are_folded_by_the_url_parser_before_the_check() {
        for (url, expect_private) in [
            ("http://2130706433/", true), // decimal 127.0.0.1
            ("http://0x7f000001/", true), // hex
            ("http://0x7f.0.0.1/", true), // dotted hex
            ("http://0177.0.0.1/", true), // octal
            ("http://127.1/", true),      // short form
            ("http://0/", true),          // 0.0.0.0
            ("http://[::1]/", true),
            ("http://[::ffff:127.0.0.1]/", true),
            ("http://[::ffff:7f00:1]/", true),
            ("http://3232235777/", true), // 192.168.1.1
            ("http://169.254.169.254/latest/meta-data", true),
            ("http://1.1.1.1/", false),
            ("http://[2606:4700:4700::1111]/", false),
            ("http://134744072/", false), // 8.8.8.8 in decimal
        ] {
            match Url::parse(url) {
                Ok(u) => {
                    let ip = url_ip(&u).unwrap_or_else(|| panic!("{url} is not an IP literal"));
                    assert_eq!(is_private_or_special(ip), expect_private, "{url}");
                }
                Err(e) => panic!("{url} should parse: {e}"),
            }
        }
        assert_eq!(url_ip(&Url::parse("http://example.com/").unwrap()), None);
    }

    proptest! {
        #[test]
        fn v4_classification_matches_the_range_table(a in 0u8..=255, b in 0u8..=255, c in 0u8..=255, d in 0u8..=255) {
            let addr = Ipv4Addr::new(a, b, c, d);
            let expected = a == 0 || a == 10 || a == 127 || a >= 224
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 169 && b == 254)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113);
            prop_assert_eq!(is_private_or_special(IpAddr::V4(addr)), expected);
        }

        #[test]
        fn mapped_and_compatible_v6_follow_their_v4(a in 0u8..=255, b in 0u8..=255, c in 0u8..=255, d in 0u8..=255) {
            let v4 = Ipv4Addr::new(a, b, c, d);
            let want = is_private_or_special(IpAddr::V4(v4));
            prop_assert_eq!(is_private_or_special(IpAddr::V6(v4.to_ipv6_mapped())), want);
            let [h, l] = [u16::from_be_bytes([a, b]), u16::from_be_bytes([c, d])];
            let nat64 = Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, h, l);
            prop_assert_eq!(is_private_or_special(IpAddr::V6(nat64)), want);
            let sixtofour = Ipv6Addr::new(0x2002, h, l, 0, 0, 0, 0, 1);
            prop_assert_eq!(is_private_or_special(IpAddr::V6(sixtofour)), want);
        }

        #[test]
        fn nothing_in_a_blocked_v6_prefix_is_public(tail in proptest::array::uniform6(0u16..)) {
            for head in [0xfc00u16, 0xfdff, 0xfe80, 0xfebf, 0xff00, 0xff0e, 0xfec0] {
                let addr = Ipv6Addr::new(head, tail[0], tail[1], tail[2], tail[3], tail[4], tail[5], 1);
                prop_assert!(is_private_or_special(IpAddr::V6(addr)), "{addr}");
            }
        }
    }

    #[tokio::test]
    async fn localhost_names_and_resolved_private_names_count_as_private() {
        let resolver = Arc::new(StaticResolver(HashMap::from([
            ("nas.lan".to_string(), vec![ip("192.168.1.20")]),
            ("cdn.example".to_string(), vec![ip("93.184.216.34")]),
            (
                "mixed.example".to_string(),
                vec![ip("93.184.216.34"), ip("10.0.0.5")],
            ),
        ])));
        let guard = NetGuard::blocking().with_resolver(resolver);
        for (url, private) in [
            ("http://localhost/x", true),
            ("http://LOCALHOST./x", true),
            ("http://app.localhost/x", true),
            ("http://nas.lan/x", true),
            ("http://mixed.example/x", true),
            ("http://cdn.example/x", false),
            ("http://unknown.example/x", false),
        ] {
            let url = Url::parse(url).unwrap();
            assert_eq!(guard.is_private_host(&url).await, private, "{url}");
        }
    }

    #[tokio::test]
    async fn for_start_blocks_only_when_the_start_is_public() {
        let resolver: Arc<dyn HostResolver> = Arc::new(StaticResolver(HashMap::from([
            ("nas.lan".to_string(), vec![ip("192.168.1.20")]),
            ("cdn.example".to_string(), vec![ip("93.184.216.34")]),
        ])));
        let public = Url::parse("https://cdn.example/").unwrap();
        let lan = Url::parse("http://nas.lan/").unwrap();
        let loopback = Url::parse("http://127.0.0.1:8080/").unwrap();
        assert!(NetGuard::for_start(&public, Some(resolver.clone()))
            .await
            .blocks());
        assert!(!NetGuard::for_start(&lan, Some(resolver)).await.blocks());
        assert!(!NetGuard::for_start(&loopback, None).await.blocks());
    }

    #[tokio::test]
    async fn the_redirect_rule_depends_on_where_the_chain_started() {
        let resolver: Arc<dyn HostResolver> = Arc::new(StaticResolver(HashMap::from([(
            "cdn.example".to_string(),
            vec![ip("93.184.216.34")],
        )])));
        let guard = NetGuard::off().with_resolver(resolver);
        let public = Url::parse("https://cdn.example/a").unwrap();
        let private = Url::parse("http://192.168.1.1/admin").unwrap();
        let other_public = Url::parse("https://1.1.1.1/b").unwrap();
        assert!(matches!(
            guard.check_redirect(&public, &private).await,
            Err(Error::PrivateRedirect)
        ));
        assert!(guard.check_redirect(&public, &other_public).await.is_ok());
        // private to private, and private to public, are the user's business
        assert!(guard.check_redirect(&private, &private).await.is_ok());
        assert!(guard.check_redirect(&private, &public).await.is_ok());
        // a blocking guard never lets a chain reach a private address
        let strict = NetGuard::blocking();
        assert!(matches!(
            strict.check_redirect(&private, &private).await,
            Err(Error::PrivateRedirect)
        ));
    }

    #[tokio::test]
    async fn the_guarded_dns_resolver_drops_private_answers() {
        use reqwest::dns::Resolve;
        use std::str::FromStr;
        let table = StaticResolver(HashMap::from([
            (
                "mixed.example".to_string(),
                vec![ip("10.0.0.5"), ip("93.184.216.34")],
            ),
            ("inner.example".to_string(), vec![ip("127.0.0.1")]),
        ]));
        let dns = NetGuard::blocking()
            .with_resolver(Arc::new(table))
            .dns_resolver();
        let name = |s: &str| reqwest::dns::Name::from_str(s).unwrap();
        let got: Vec<IpAddr> = dns
            .resolve(name("mixed.example"))
            .await
            .unwrap()
            .map(|a| a.ip())
            .collect();
        assert_eq!(got, vec![ip("93.184.216.34")]);
        assert!(dns.resolve(name("inner.example")).await.is_err());
        assert!(dns.resolve(name("localhost")).await.is_err());
    }

    #[tokio::test]
    async fn for_start_with_mixed_public_and_private_answers_still_blocks() {
        let resolver: Arc<dyn HostResolver> = Arc::new(StaticResolver(HashMap::from([
            (
                "evil.example".to_string(),
                vec![ip("1.2.3.4"), ip("127.0.0.1")],
            ),
            (
                "lan.example".to_string(),
                vec![ip("192.168.1.2"), ip("10.0.0.3")],
            ),
        ])));
        let evil = Url::parse("http://evil.example/").unwrap();
        let lan = Url::parse("http://lan.example/").unwrap();
        assert!(NetGuard::for_start(&evil, Some(resolver.clone()))
            .await
            .blocks());
        assert!(!NetGuard::for_start(&lan, Some(resolver.clone()))
            .await
            .blocks());
        // a mixed start is public for the redirect rule too
        let guard = NetGuard::off().with_resolver(resolver);
        let private = Url::parse("http://192.168.1.1/").unwrap();
        assert!(matches!(
            guard.check_redirect(&evil, &private).await,
            Err(Error::PrivateRedirect)
        ));
    }

    #[tokio::test]
    async fn allow_private_redirects_lifts_only_the_redirect_rule() {
        let resolver: Arc<dyn HostResolver> = Arc::new(StaticResolver(HashMap::from([(
            "cdn.example".to_string(),
            vec![ip("93.184.216.34")],
        )])));
        let public = Url::parse("https://cdn.example/a").unwrap();
        let private = Url::parse("http://192.168.1.1/admin").unwrap();
        let open = NetGuard::off()
            .with_resolver(resolver.clone())
            .with_allow_private_redirects(true);
        assert!(open.check_redirect(&public, &private).await.is_ok());
        // a blocking (crawl) guard ignores the setting
        let strict = NetGuard::blocking()
            .with_resolver(resolver)
            .with_allow_private_redirects(true);
        assert!(strict.check_redirect(&public, &private).await.is_err());
        let message = Error::PrivateRedirect.to_string();
        assert!(message.contains("allow_private_redirects"), "{message}");
    }
}
