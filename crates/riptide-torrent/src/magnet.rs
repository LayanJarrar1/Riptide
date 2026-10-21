//! Magnet link validation and log redaction.
//!
//! A magnet link is user- or website-supplied text. [`parse_magnet`] accepts only what we can
//! act on safely: scheme `magnet`, at least one `xt=urn:btih:` with a well-formed info-hash
//! (40 hex digits or 32 base-32 characters), at most [`MAX_TRACKERS`] tracker URLs and only
//! `http`, `https` and `udp` ones. Everything else (`kt`, `x.pe` addresses, `xs`/`as` web seeds,
//! unknown keys) is dropped, never forwarded to the engine.
//!
//! Tracker URLs frequently carry a private passkey in their path or query. They are therefore
//! never logged: [`Magnet::redacted`] prints the info-hash, the display name and the tracker
//! *count* only, and [`redact_url`] strips the query, fragment, credentials and path of any URL
//! that has to appear in a log line.

use url::Url;

use crate::error::{Error, Result};
use crate::limits::{MAX_MAGNET_BYTES, MAX_TRACKERS};
use crate::paths::sanitize_component;

/// A validated magnet link. `Debug` shows the info-hash and the tracker count only: tracker
/// URLs may carry a private passkey and must not end up in a log by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Magnet {
    /// BitTorrent v1 info-hash, 40 lowercase hex digits.
    pub info_hash: String,
    /// Display name (`dn`), sanitised as a file name, if present.
    pub display_name: Option<String>,
    /// Tracker URLs (`tr`), deduplicated and restricted to `http`, `https` and `udp`.
    pub trackers: Vec<Url>,
}

impl std::fmt::Debug for Magnet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Magnet")
            .field("info_hash", &self.info_hash)
            .field("display_name", &self.display_name)
            .field(
                "trackers",
                &format_args!("[{} hidden]", self.trackers.len()),
            )
            .finish()
    }
}

impl Magnet {
    /// A form that is safe to log and to show: no tracker URL, no query string.
    pub fn redacted(&self) -> String {
        format!(
            "magnet:{}{} ({} trackers)",
            self.info_hash,
            self.display_name
                .as_deref()
                .map(|n| format!(" \"{n}\""))
                .unwrap_or_default(),
            self.trackers.len()
        )
    }

    /// Rebuilds a clean magnet URI from the validated parts (what the engine is given):
    /// `xt=urn:btih:<hash>` literally, then the name and each tracker percent-encoded as one
    /// parameter value (so a tracker's own `?uk=` stays inside it).
    pub fn to_uri(&self) -> String {
        let enc =
            |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
        let mut out = format!("magnet:?xt=urn:btih:{}", self.info_hash);
        if let Some(n) = &self.display_name {
            out.push_str("&dn=");
            out.push_str(&enc(n));
        }
        for t in &self.trackers {
            out.push_str("&tr=");
            out.push_str(&enc(t.as_str()));
        }
        out
    }
}

/// Validates `input` and returns the parts we are willing to act on.
pub fn parse_magnet(input: &str) -> Result<Magnet> {
    let input = input.trim();
    if input.len() > MAX_MAGNET_BYTES {
        return Err(Error::InvalidMagnet(format!(
            "longer than {MAX_MAGNET_BYTES} bytes"
        )));
    }
    if input.chars().any(|c| c.is_control()) {
        return Err(Error::InvalidMagnet("contains control characters".into()));
    }
    let url = Url::parse(input).map_err(|_| Error::InvalidMagnet("not a valid URI".into()))?;
    if url.scheme() != "magnet" {
        return Err(Error::InvalidMagnet("scheme must be magnet:".into()));
    }

    let mut info_hash: Option<String> = None;
    let mut display_name = None;
    let mut trackers: Vec<Url> = Vec::new();
    let mut saw_non_btih_xt = false;

    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "xt" => match v.strip_prefix("urn:btih:") {
                Some(h) => {
                    let h = decode_info_hash(h)?;
                    match &info_hash {
                        Some(prev) if *prev != h => {
                            return Err(Error::InvalidMagnet(
                                "several different info-hashes".into(),
                            ))
                        }
                        _ => info_hash = Some(h),
                    }
                }
                None => saw_non_btih_xt = true,
            },
            "dn" if !v.is_empty() => {
                let name = sanitize_component(&v);
                display_name = Some(name);
            }
            "tr" => {
                if trackers.len() >= MAX_TRACKERS {
                    continue;
                }
                if let Ok(t) = Url::parse(&v) {
                    if matches!(t.scheme(), "http" | "https" | "udp")
                        && t.host_str().is_some()
                        && !trackers.contains(&t)
                    {
                        trackers.push(t);
                    }
                }
            }
            _ => {}
        }
    }

    match info_hash {
        Some(info_hash) => Ok(Magnet {
            info_hash,
            display_name,
            trackers,
        }),
        None if saw_non_btih_xt => Err(Error::InvalidMagnet(
            "only BitTorrent v1 (urn:btih:) magnet links are supported".into(),
        )),
        None => Err(Error::InvalidMagnet("no xt=urn:btih: info-hash".into())),
    }
}

/// 40 hex digits, or 32 base-32 characters (RFC 4648, case-insensitive) converted to hex.
fn decode_info_hash(h: &str) -> Result<String> {
    if h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(h.to_ascii_lowercase());
    }
    if h.len() == 32 {
        let mut bits: u64 = 0;
        let mut nbits = 0;
        let mut out = Vec::with_capacity(20);
        for b in h.bytes() {
            let v = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a',
                b'2'..=b'7' => b - b'2' + 26,
                _ => return Err(Error::InvalidMagnet("bad base-32 info-hash".into())),
            };
            bits = (bits << 5) | u64::from(v);
            nbits += 5;
            if nbits >= 8 {
                nbits -= 8;
                out.push(((bits >> nbits) & 0xff) as u8);
            }
        }
        if out.len() == 20 {
            return Ok(out.iter().map(|b| format!("{b:02x}")).collect());
        }
    }
    Err(Error::InvalidMagnet(
        "info-hash must be 40 hex digits or 32 base-32 characters".into(),
    ))
}

/// Reduces a URL to `scheme://host[:port]` for log lines: no credentials, no path (private
/// trackers keep their passkey there), no query, no fragment. Unparseable input becomes
/// `"<invalid url>"`.
pub fn redact_url(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(u) => match u.host_str() {
            Some(h) => match u.port() {
                Some(p) => format!("{}://{h}:{p}", u.scheme()),
                None => format!("{}://{h}", u.scheme()),
            },
            None => format!("{}:", u.scheme()),
        },
        Err(_) => "<invalid url>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";

    #[test]
    fn accepts_hex_and_base32_info_hashes() {
        let m = parse_magnet(&format!("magnet:?xt=urn:btih:{}", H.to_uppercase())).unwrap();
        assert_eq!(m.info_hash, H);
        // base32 of the same 20 bytes
        let b32 = "YEX6DQDLXISUVHOJ6UM3GWVHPQJWPKEK";
        let m2 = parse_magnet(&format!("magnet:?xt=urn:btih:{b32}")).unwrap();
        assert_eq!(m2.info_hash.len(), 40);
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            "http://example.com/x.torrent",
            "magnet:?dn=nohash",
            "magnet:?xt=urn:btih:zzzz",
            "magnet:?xt=urn:btih:abcd",
            "magnet:?xt=urn:sha1:c12fe1c06bba254a9dc9f519b335aa7c1367a88a",
            "magnet:?xt=urn:btmh:1220c12fe1c06bba254a9dc9f519b335aa7c1367a88ac12fe1c06bba254a9dc9",
            "magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335aa7c1367a88a\n&tr=udp://x",
            "",
        ] {
            assert!(parse_magnet(bad).is_err(), "should reject {bad:?}");
        }
        let two = format!("magnet:?xt=urn:btih:{H}&xt=urn:btih:{}", "0".repeat(40));
        assert!(parse_magnet(&two).is_err());
        let huge = format!(
            "magnet:?xt=urn:btih:{H}&dn={}",
            "a".repeat(MAX_MAGNET_BYTES)
        );
        assert!(parse_magnet(&huge).is_err());
    }

    #[test]
    fn only_safe_trackers_survive_and_are_capped() {
        let mut s = format!(
            "magnet:?xt=urn:btih:{H}&tr=ftp://x/y&tr=file:///etc/passwd&tr=javascript:1&tr=udp://tracker.example:80/announce"
        );
        for i in 0..200 {
            s.push_str(&format!("&tr=http://t{i}.example/announce"));
        }
        let m = parse_magnet(&s).unwrap();
        assert_eq!(m.trackers.len(), MAX_TRACKERS);
        assert!(m
            .trackers
            .iter()
            .all(|t| matches!(t.scheme(), "http" | "https" | "udp")));
    }

    #[test]
    fn display_name_is_sanitised() {
        let m = parse_magnet(&format!(
            "magnet:?xt=urn:btih:{H}&dn=..%2F..%2FCON%00%E2%80%AEevil"
        ))
        .unwrap();
        let n = m.display_name.unwrap();
        assert!(!n.contains('/') && !n.contains('\u{202E}'), "{n}");
    }

    #[test]
    fn passkeys_never_reach_the_redacted_forms() {
        let m = parse_magnet(&format!(
            "magnet:?xt=urn:btih:{H}&dn=Movie&tr=https://private.example/SECRETPASSKEY/announce?k=SECRET2"
        ))
        .unwrap();
        let r = m.redacted();
        assert!(
            !r.contains("SECRET") && !r.contains("private.example"),
            "{r}"
        );
        assert!(r.contains(H) && r.contains("1 trackers"));
        assert_eq!(
            redact_url("https://user:pw@private.example:8443/SECRETPASSKEY/announce?k=SECRET2#f"),
            "https://private.example:8443"
        );
        assert_eq!(redact_url("garbage"), "<invalid url>");
    }

    #[test]
    fn rebuilt_uri_round_trips() {
        let m = parse_magnet(&format!(
            "magnet:?xt=urn:btih:{H}&dn=A%20B&tr=udp://t.example:1/announce&kt=ignored&x.pe=1.2.3.4:5"
        ))
        .unwrap();
        let again = parse_magnet(&m.to_uri()).unwrap();
        assert_eq!(m, again);
        assert!(!m.to_uri().contains("x.pe"));
    }
}
