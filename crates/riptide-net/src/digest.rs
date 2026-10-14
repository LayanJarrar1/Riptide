//! HTTP Digest authentication (RFC 7616, MD5 and MD5-sess, `qop=auth`), plus Basic.
//!
//! Only what download servers actually use: MD5 (SHA-256 digest challenges are rare and the
//! fallback is a clear "authentication failed", never a silent downgrade).

use base64::Engine as _;
use md5::{Digest as _, Md5};

/// A parsed `WWW-Authenticate: Digest …` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestChallenge {
    /// `realm`.
    pub realm: String,
    /// `nonce`.
    pub nonce: String,
    /// `opaque`, echoed back if present.
    pub opaque: Option<String>,
    /// `algorithm` as sent (`MD5`, `MD5-sess`); `None` means MD5.
    pub algorithm: Option<String>,
    /// Whether `qop="auth"` was offered.
    pub qop_auth: bool,
    /// `stale=true` means the nonce expired and the same credentials may be retried.
    pub stale: bool,
}

/// Parses the value of a `WWW-Authenticate` header. Returns `None` unless it is a Digest
/// challenge the engine can answer (MD5 family).
pub fn parse_challenge(header: &str) -> Option<DigestChallenge> {
    let rest = header.trim().strip_prefix("Digest")?.trim_start();
    let mut realm = None;
    let mut nonce = None;
    let mut opaque = None;
    let mut algorithm = None;
    let mut qop_auth = false;
    let mut stale = false;
    for (k, v) in parse_params(rest) {
        match k.to_ascii_lowercase().as_str() {
            "realm" => realm = Some(v),
            "nonce" => nonce = Some(v),
            "opaque" => opaque = Some(v),
            "algorithm" => algorithm = Some(v),
            "qop" => qop_auth = v.split(',').any(|q| q.trim().eq_ignore_ascii_case("auth")),
            "stale" => stale = v.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    let algorithm_ok = algorithm
        .as_deref()
        .is_none_or(|a| a.eq_ignore_ascii_case("MD5") || a.eq_ignore_ascii_case("MD5-sess"));
    if !algorithm_ok {
        return None;
    }
    Some(DigestChallenge {
        realm: realm?,
        nonce: nonce?,
        opaque,
        algorithm,
        qop_auth,
        stale,
    })
}

/// `key=value, key="quoted, value"` pairs.
fn parse_params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        if rest.is_empty() {
            break;
        }
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_string();
        rest = &rest[eq + 1..];
        let value;
        if let Some(stripped) = rest.strip_prefix('"') {
            let mut end = 0;
            let bytes = stripped.as_bytes();
            let mut escaped = false;
            while end < bytes.len() {
                if escaped {
                    escaped = false;
                } else if bytes[end] == b'\\' {
                    escaped = true;
                } else if bytes[end] == b'"' {
                    break;
                }
                end += 1;
            }
            value = stripped[..end].replace("\\\"", "\"");
            rest = &stripped[(end + 1).min(stripped.len())..];
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].trim().to_string();
            rest = &rest[end..];
        }
        out.push((key, value));
    }
    out
}

fn md5_hex(input: &str) -> String {
    hex::encode(Md5::digest(input.as_bytes()))
}

impl DigestChallenge {
    /// Builds the `Authorization: Digest …` header value.
    ///
    /// `nc` is the request counter for this nonce (starting at 1) and `cnonce` the client nonce;
    /// both are caller-supplied so the computation is deterministic and testable.
    pub fn authorization(
        &self,
        username: &str,
        password: &str,
        method: &str,
        uri: &str,
        nc: u32,
        cnonce: &str,
    ) -> String {
        let ha1_plain = md5_hex(&format!("{username}:{}:{password}", self.realm));
        let ha1 = if self
            .algorithm
            .as_deref()
            .is_some_and(|a| a.eq_ignore_ascii_case("MD5-sess"))
        {
            md5_hex(&format!("{ha1_plain}:{}:{cnonce}", self.nonce))
        } else {
            ha1_plain
        };
        let ha2 = md5_hex(&format!("{method}:{uri}"));
        let nc_hex = format!("{nc:08x}");
        let response = if self.qop_auth {
            md5_hex(&format!(
                "{ha1}:{}:{nc_hex}:{cnonce}:auth:{ha2}",
                self.nonce
            ))
        } else {
            md5_hex(&format!("{ha1}:{}:{ha2}", self.nonce))
        };
        let mut out = format!(
            "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\"",
            escape(username),
            escape(&self.realm),
            escape(&self.nonce),
            uri,
            response
        );
        if let Some(alg) = &self.algorithm {
            out.push_str(&format!(", algorithm={alg}"));
        }
        if self.qop_auth {
            out.push_str(&format!(", qop=auth, nc={nc_hex}, cnonce=\"{cnonce}\""));
        }
        if let Some(op) = &self.opaque {
            out.push_str(&format!(", opaque=\"{}\"", escape(op)));
        }
        out
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `Authorization: Basic …` value.
pub fn basic_authorization(username: &str, password: &str) -> String {
    let raw = format!("{username}:{password}");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    )
}

/// A fresh client nonce (16 random hex chars).
pub fn client_nonce() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_CHALLENGE: &str = "Digest realm=\"testrealm@host.com\", qop=\"auth,auth-int\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"";

    #[test]
    fn parses_the_rfc_example() {
        let c = parse_challenge(RFC_CHALLENGE).unwrap();
        assert_eq!(c.realm, "testrealm@host.com");
        assert_eq!(c.nonce, "dcd98b7102dd2f0e8b11d0f600bfb0c093");
        assert_eq!(
            c.opaque.as_deref(),
            Some("5ccc069c403ebaf9f0171e9517f40e41")
        );
        assert!(c.qop_auth);
        assert!(!c.stale);
    }

    #[test]
    fn computes_the_rfc_2617_response() {
        let c = parse_challenge(RFC_CHALLENGE).unwrap();
        let auth = c.authorization(
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            1,
            "0a4f113b",
        );
        assert!(
            auth.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "{auth}"
        );
        assert!(auth.contains("nc=00000001"));
        assert!(auth.contains("cnonce=\"0a4f113b\""));
        assert!(auth.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));
        assert!(auth.starts_with("Digest username=\"Mufasa\""));
    }

    #[test]
    fn rejects_unknown_algorithms_and_missing_fields() {
        assert!(parse_challenge("Digest realm=\"r\", nonce=\"n\", algorithm=SHA-256").is_none());
        assert!(parse_challenge("Digest realm=\"r\"").is_none());
        assert!(parse_challenge("Basic realm=\"r\"").is_none());
        let sess = parse_challenge(
            "Digest realm=\"r\", nonce=\"n\", algorithm=MD5-sess, qop=auth, stale=true",
        )
        .unwrap();
        assert!(sess.stale);
        assert!(sess
            .authorization("u", "p", "GET", "/", 1, "c")
            .contains("algorithm=MD5-sess"));
    }

    #[test]
    fn basic_is_base64_of_user_colon_pass() {
        assert_eq!(basic_authorization("user", "pass"), "Basic dXNlcjpwYXNz");
        assert_eq!(client_nonce().len(), 16);
    }
}
