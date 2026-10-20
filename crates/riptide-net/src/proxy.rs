//! Proxy choice for the shared client: the system's, none, or one URL the user typed.
//!
//! *System* is reqwest's platform detection (crate feature `system-proxy`): the `HTTP_PROXY` /
//! `HTTPS_PROXY` / `NO_PROXY` environment variables and, on Windows, the per-user WinINet
//! settings (`HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`:
//! `ProxyEnable`, `ProxyServer`, `ProxyOverride`). A PAC script (`AutoConfigURL`) is not
//! evaluated (PAC is a non-goal); such users pick *Manual*.

use std::fmt;

use url::Url;

use crate::error::{Error, Result};

/// Schemes a manual proxy may use.
pub const PROXY_SCHEMES: [&str; 4] = ["http", "https", "socks5", "socks5h"];

/// A validated proxy URL. `Debug` and `Display` hide the credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyUrl(Url);

impl ProxyUrl {
    /// Parses and validates `raw`: an `http://`, `https://`, `socks5://` or `socks5h://` URL
    /// with a host, optional `user:password@`, and nothing after the port but `/`.
    pub fn parse(raw: &str) -> Result<ProxyUrl> {
        let invalid = |why: &str| Error::InvalidProxy(why.to_string());
        let url = Url::parse(raw.trim()).map_err(|_| invalid("not a URL"))?;
        if !PROXY_SCHEMES.contains(&url.scheme()) {
            return Err(invalid("scheme must be http, https or socks5"));
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(invalid("missing host"));
        }
        if !(url.path().is_empty() || url.path() == "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid("a proxy URL has no path or query"));
        }
        Ok(ProxyUrl(url))
    }

    /// The URL itself (with credentials) for the client builder only.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The URL with `user:password@` replaced, safe for logs and errors.
    pub fn redacted(&self) -> String {
        let mut u = self.0.clone();
        if !u.username().is_empty() || u.password().is_some() {
            let _ = u.set_username("redacted");
            let _ = u.set_password(None);
        }
        u.to_string()
    }
}

impl fmt::Debug for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProxyUrl({})", self.redacted())
    }
}

impl fmt::Display for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

/// Which proxy the client uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProxyChoice {
    /// The platform's proxy settings (see the module docs).
    #[default]
    System,
    /// Direct connections, environment and system settings ignored.
    Direct,
    /// Every request goes through this proxy.
    Manual(ProxyUrl),
}

impl ProxyChoice {
    /// Host names the connector must be able to resolve to reach the proxy itself: the manual
    /// proxy's host, or for the system choice the hosts in `HTTP_PROXY` / `HTTPS_PROXY` /
    /// `ALL_PROXY` and (Windows) the WinINet `ProxyServer` value. Empty for a direct connection.
    pub fn proxy_hosts(&self) -> Vec<String> {
        match self {
            ProxyChoice::Direct => Vec::new(),
            ProxyChoice::Manual(url) => url.0.host_str().map(str::to_owned).into_iter().collect(),
            ProxyChoice::System => system_proxy_hosts(),
        }
    }

    /// Applies the choice to a client builder.
    pub(crate) fn apply(&self, builder: reqwest::ClientBuilder) -> Result<reqwest::ClientBuilder> {
        Ok(match self {
            ProxyChoice::System => builder,
            ProxyChoice::Direct => builder.no_proxy(),
            ProxyChoice::Manual(url) => {
                let proxy = reqwest::Proxy::all(url.as_str())
                    .map_err(|_| Error::InvalidProxy(url.redacted()))?;
                builder.no_proxy().proxy(proxy)
            }
        })
    }
}

/// Hosts out of a proxy setting: `host:port`, `scheme://host:port`, or the WinINet list
/// `http=host:port;https=host2:port`.
pub(crate) fn parse_proxy_hosts(setting: &str) -> Vec<String> {
    setting
        .split([';', ' '])
        .filter_map(|entry| {
            let entry = entry.trim();
            let entry = entry.rsplit_once('=').map_or(entry, |(_, v)| v);
            let entry = entry.split_once("://").map_or(entry, |(_, v)| v);
            let entry = entry.rsplit_once('@').map_or(entry, |(_, v)| v);
            let authority = entry.split(['/', '?']).next().unwrap_or("");
            let host = if let Some(rest) = authority.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                authority.split(':').next().unwrap_or("")
            };
            (!host.is_empty()).then(|| host.to_ascii_lowercase())
        })
        .collect()
}

fn system_proxy_hosts() -> Vec<String> {
    let mut hosts = Vec::new();
    for var in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(v) = std::env::var(var) {
            hosts.extend(parse_proxy_hosts(&v));
        }
    }
    #[cfg(windows)]
    if let Some(v) = wininet::proxy_server() {
        hosts.extend(parse_proxy_hosts(&v));
    }
    hosts.sort();
    hosts.dedup();
    hosts
}

/// The per-user WinINet proxy value, read with one registry call.
#[cfg(windows)]
mod wininet {
    #![allow(unsafe_code)]

    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    pub fn proxy_server() -> Option<String> {
        let subkey: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let value: Vec<u16> = "ProxyServer".encode_utf16().chain(Some(0)).collect();
        let mut buf = vec![0u16; 2048];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: both names are NUL-terminated UTF-16; `buf` is valid for `bytes` bytes and
        // `bytes` is a valid in/out pointer; the type filter limits the result to REG_SZ.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != 0 {
            return None;
        }
        let len = (bytes as usize / 2).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        Some(text.trim_end_matches('\0').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_hosts_are_pulled_out_of_every_setting_format() {
        assert_eq!(parse_proxy_hosts("proxy.corp:8080"), ["proxy.corp"]);
        assert_eq!(
            parse_proxy_hosts("http://user:pw@Proxy.Corp:3128/"),
            ["proxy.corp"]
        );
        assert_eq!(
            parse_proxy_hosts("http=a.corp:80;https=b.corp:443;ftp=c.corp:21"),
            ["a.corp", "b.corp", "c.corp"]
        );
        assert_eq!(parse_proxy_hosts("socks5://[::1]:1080"), ["::1"]);
        assert!(parse_proxy_hosts("").is_empty());
        let manual = ProxyChoice::Manual(ProxyUrl::parse("http://localhost:8888").unwrap());
        assert_eq!(manual.proxy_hosts(), ["localhost"]);
        assert!(ProxyChoice::Direct.proxy_hosts().is_empty());
    }

    #[test]
    fn accepts_the_three_kinds() {
        for ok in [
            "http://127.0.0.1:8080",
            "https://proxy.example:443/",
            "socks5://127.0.0.1:1080",
            "socks5h://user:pa%40ss@proxy.example:1080",
            "  http://proxy.example:3128  ",
        ] {
            ProxyUrl::parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "",
            "proxy.example:8080",
            "ftp://proxy.example",
            "socks4://proxy.example:1080",
            "http://",
            "http://proxy.example:8080/path",
            "http://proxy.example:8080/?q=1",
        ] {
            assert!(ProxyUrl::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn credentials_never_show() {
        let p = ProxyUrl::parse("http://alice:s3cret@proxy.example:3128").unwrap();
        for shown in [format!("{p:?}"), p.to_string(), p.redacted()] {
            assert!(
                !shown.contains("s3cret") && !shown.contains("alice"),
                "{shown}"
            );
            assert!(shown.contains("proxy.example:3128"), "{shown}");
        }
        assert!(p.as_str().contains("s3cret"));
    }

    #[test]
    fn every_choice_builds_a_client() {
        for choice in [
            ProxyChoice::System,
            ProxyChoice::Direct,
            ProxyChoice::Manual(ProxyUrl::parse("socks5://127.0.0.1:1080").unwrap()),
        ] {
            crate::build_client(&crate::ClientOptions {
                proxy: choice,
                ..Default::default()
            })
            .unwrap();
        }
    }
}
