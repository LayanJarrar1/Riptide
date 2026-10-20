//! Redaction layer: every log line is scrubbed before it is written.
//!
//! The scrubbing is deliberately coarse. It runs on the formatted text, not on structured fields,
//! so it catches secrets regardless of how they were logged (`cookie = %value`, `?headers`,
//! `format!`, a panic message, a dependency's own log line). Rules, applied in order:
//!
//! 1. `scheme://user:pass@host` → `scheme://[REDACTED]@host`
//! 2. URL query strings are stripped: `https://h/p?token=x` → `https://h/p?[stripped]`
//! 3. `Bearer xxx` / `Basic xxx` / `Digest xxx` → `Bearer [REDACTED]` …
//! 4. `name=value` / `name: value` / `name="value"` where `name` is one of the secret-bearing
//!    names (`cookie`, `set-cookie`, `authorization`, `proxy-authorization`, `password`,
//!    `passwd`, `secret`, `token`, `access_token`, `refresh_token`, `api_key`, `x-api-key`,
//!    `x-auth-token`, `credentials`, `auth`): a quoted value is replaced up to its closing
//!    quote; an unquoted value is replaced up to the end of the line, because cookie strings
//!    contain spaces and semicolons and we would rather lose the rest of the line than leak.

use regex::{Regex, RegexBuilder};
use std::io::{self, Write};
use std::sync::LazyLock;

/// What a redacted value is replaced with.
pub const MASK: &str = "[REDACTED]";
/// What a URL query string is replaced with.
pub const QUERY_MASK: &str = "[stripped]";

static USERINFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<scheme>[A-Za-z][A-Za-z0-9+.\-]*://)(?P<userinfo>[^/\s@]+)@")
        .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

static QUERY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?P<url>[A-Za-z][A-Za-z0-9+.\-]*://[^\s"'<>?#]*)\?[^\s"'<>#]*"#)
        .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

/// A whole magnet link (the `tr` trackers inside it may carry a private passkey).
static MAGNET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)magnet:\?[^\s"'<>]*"#)
        .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

static BTIH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)xt=urn:btih:([a-z0-9]{32,40})")
        .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

/// A tracker announce URL: private trackers keep the passkey in the path.
static ANNOUNCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?P<host>(?:https?|udp)://[^/\s"'<>?#]+)/[^\s"'<>?#]*announce[^\s"'<>?#]*"#)
        .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

static AUTH_SCHEME: LazyLock<Regex> = LazyLock::new(|| {
    // `[` is excluded so an already-masked `Bearer [REDACTED]` is left alone (idempotence).
    RegexBuilder::new(
        r#"\b(?P<scheme>Bearer|Basic|Digest|Negotiate|NTLM)\s+[^\s,;\[\]\(\)\{\}'"]+"#,
    )
    .case_insensitive(true)
    .build()
    .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

static SECRET_KEY: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new(
        // key, then either `:`/`=` (headers, fields) or — when the key itself was quoted — also
        // `,` so Debug tuples `("Cookie", "v")` and JSON `{"cookie": "v"}` are covered.
        r#"(?P<key>\b(?:set-cookie|cookie|proxy-authorization|authorization|password|passwd|secret|access[_-]?token|refresh[_-]?token|id[_-]?token|token|api[_-]?key|x-api-key|x-auth-token|credentials?|auth)\b)(?P<sep>(?:["']\s*[:=,]|\s*[:=])\s*)"#,
    )
    .case_insensitive(true)
    .build()
    .unwrap_or_else(|e| unreachable!("static regex is valid: {e}"))
});

/// Scrubs one line of text. Idempotent.
pub fn redact_line(line: &str) -> String {
    // magnet links keep their info-hash and lose everything else (trackers, names)
    let s = MAGNET.replace_all(line, |c: &regex::Captures<'_>| match BTIH.captures(&c[0]) {
        Some(h) => format!("magnet:?xt=urn:btih:{}&{QUERY_MASK}", &h[1]),
        None => format!("magnet:?{QUERY_MASK}"),
    });
    let s = ANNOUNCE.replace_all(&s, "${host}/[stripped]");
    let s = USERINFO.replace_all(&s, "${scheme}[REDACTED]@");
    let s = QUERY.replace_all(&s, |c: &regex::Captures<'_>| {
        format!("{}?{QUERY_MASK}", &c["url"])
    });
    let s = AUTH_SCHEME.replace_all(&s, "${scheme} [REDACTED]");
    redact_secret_keys(&s)
}

/// Strips the query string (and fragment) from a URL for logging. Non-URLs are returned as-is.
pub fn redact_url(url: &str) -> String {
    let s = USERINFO.replace_all(url, "${scheme}[REDACTED]@");
    match s.find(['?', '#']) {
        Some(i) => format!("{}?{QUERY_MASK}", &s[..i]),
        None => s.into_owned(),
    }
}

fn redact_secret_keys(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(m) = SECRET_KEY.captures_at(input, pos) {
        let whole = m.get(0).map(|g| g.range());
        let Some(range) = whole else { break };
        out.push_str(&input[pos..range.end]);
        let rest = &input[range.end..];
        if rest.starts_with(MASK) {
            // already redacted (idempotence)
            pos = range.end;
            continue;
        }
        let consumed = match rest.as_bytes().first() {
            Some(b'"') => quoted_len(rest, b'"'),
            Some(b'\'') => quoted_len(rest, b'\''),
            // unquoted: everything up to the end of the line, keeping the line break itself
            _ => rest.find(['\n', '\r']).unwrap_or(rest.len()),
        };
        let quote = match rest.as_bytes().first() {
            Some(b'"') => "\"",
            Some(b'\'') => "'",
            _ => "",
        };
        out.push_str(quote);
        out.push_str(MASK);
        out.push_str(quote);
        pos = range.end + consumed;
    }
    out.push_str(&input[pos..]);
    out
}

/// Length of a quoted token starting at `s[0] == quote`, including both quotes and honouring
/// backslash escapes. If unterminated, the rest of the string.
fn quoted_len(s: &str, quote: u8) -> usize {
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    s.len()
}

/// A [`Write`] adapter that redacts every line before passing it to the inner writer.
///
/// Partial lines are buffered until a newline arrives (or until `flush`/drop), so a secret
/// cannot slip through by being split across two `write` calls.
#[derive(Debug)]
pub struct RedactingWriter<W: Write> {
    inner: W,
    buf: Vec<u8>,
}

impl<W: Write> RedactingWriter<W> {
    /// Wraps `inner`.
    pub fn new(inner: W) -> Self {
        RedactingWriter {
            inner,
            buf: Vec::new(),
        }
    }

    /// The wrapped writer.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    fn drain_lines(&mut self) -> io::Result<()> {
        while let Some(nl) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            let text = String::from_utf8_lossy(&line);
            self.inner.write_all(redact_line(&text).as_bytes())?;
        }
        Ok(())
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        self.drain_lines()?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.drain_lines()?;
        if !self.buf.is_empty() {
            let rest: Vec<u8> = std::mem::take(&mut self.buf);
            let text = String::from_utf8_lossy(&rest);
            self.inner.write_all(redact_line(&text).as_bytes())?;
        }
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn magnet_links_keep_only_the_info_hash() {
        let line = "adding magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D&dn=X done";
        let out = super::redact_line(line);
        assert_eq!(
            out,
            "adding magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&[stripped] done"
        );
        assert_eq!(super::redact_line(&out), out, "idempotent");
        assert!(!super::redact_line("MAGNET:?tr=udp://x").contains("udp"));
    }

    #[test]
    fn announce_urls_lose_their_passkey_path() {
        let out = super::redact_line(
            "announce to https://tracker.example:8443/SECRETPASSKEY/announce failed",
        );
        assert_eq!(
            out,
            "announce to https://tracker.example:8443/[stripped] failed"
        );
        let out = super::redact_line("udp://open.tracker.example:1337/announce");
        assert_eq!(out, "udp://open.tracker.example:1337/[stripped]");
        // ordinary download URLs keep their path
        assert_eq!(
            super::redact_line("GET https://cdn.example/files/a.zip"),
            "GET https://cdn.example/files/a.zip"
        );
    }

    use super::*;

    #[test]
    fn strips_query_strings_and_userinfo() {
        assert_eq!(
            redact_line("GET https://cdn.example.com/file.zip?token=abc123&sig=zz done"),
            "GET https://cdn.example.com/file.zip?[stripped] done"
        );
        assert_eq!(
            redact_line("ftp://alice:s3cret@host/path"),
            "ftp://[REDACTED]@host/path"
        );
        assert_eq!(redact_url("https://h/p?x=1#frag"), "https://h/p?[stripped]");
        assert_eq!(redact_url("https://h/p"), "https://h/p");
        assert_eq!(redact_url("not a url"), "not a url");
        assert_eq!(
            redact_line("riptide://add?url=https%3A%2F%2Fx"),
            "riptide://add?[stripped]"
        );
    }

    #[test]
    fn redacts_auth_schemes() {
        assert_eq!(
            redact_line("Authorization: Bearer eyJhbGciOi.xx.yy"),
            "Authorization: [REDACTED]"
        );
        assert_eq!(
            redact_line("hdr Basic dXNlcjpwYXNz end"),
            "hdr Basic [REDACTED] end"
        );
    }

    #[test]
    fn redacts_cookie_fields_quoted_and_unquoted() {
        let line = r#"INFO fetch cookie="session=abc123secret; theme=dark" size=42"#;
        let out = redact_line(line);
        assert!(!out.contains("abc123secret"), "{out}");
        assert_eq!(out, r#"INFO fetch cookie="[REDACTED]" size=42"#);

        let line = "INFO fetch Cookie: session=abc123secret; theme=dark";
        let out = redact_line(line);
        assert_eq!(out, "INFO fetch Cookie: [REDACTED]");

        let line = "set-cookie=sid=deadbeef; Path=/\n";
        assert_eq!(
            redact_line(line),
            "set-cookie=[REDACTED]\n",
            "line break is kept"
        );
    }

    #[test]
    fn redacts_passwords_and_tokens_but_keeps_other_fields() {
        let out = redact_line(r#"user="bob" password="hunter2" host=example"#);
        assert_eq!(out, r#"user="bob" password="[REDACTED]" host=example"#);
        let out = redact_line("api_key=xyz123, access-token: tkn");
        assert!(!out.contains("xyz123"));
        assert!(!out.contains("tkn"));
    }

    #[test]
    fn redacts_debug_tuples_and_json_objects() {
        let out = redact_line(r#"headers=[("Cookie", "sid=deadbeef"), ("Accept", "*/*")]"#);
        assert_eq!(
            out,
            r#"headers=[("Cookie", "[REDACTED]"), ("Accept", "*/*")]"#
        );
        let out = redact_line(r#"{"cookie": "a=b", "size": 3, "token":"t1"}"#);
        assert_eq!(
            out,
            r#"{"cookie": "[REDACTED]", "size": 3, "token":"[REDACTED]"}"#
        );
        // prose with a comma after a key word is left alone (only quoted keys take `,`)
        assert_eq!(
            redact_line("invalid token, retrying"),
            "invalid token, retrying"
        );
    }

    #[test]
    fn is_idempotent() {
        let once = redact_line(r#"cookie="a=b" url=https://x/y?z=1 Bearer abc"#);
        assert_eq!(redact_line(&once), once);
    }

    #[test]
    fn leaves_innocent_lines_alone() {
        for line in [
            "download 7 completed in 3.2s",
            "segment 3 written=1048576",
            "https://example.com/file.iso resumed at 42",
            "tokens per second: 5",
        ] {
            assert_eq!(redact_line(line), line);
        }
    }

    #[test]
    fn handles_escaped_quotes() {
        let out = redact_line(r#"password="a\"b" next=1"#);
        assert_eq!(out, r#"password="[REDACTED]" next=1"#);
    }

    #[test]
    fn writer_redacts_across_split_writes() {
        let mut w = RedactingWriter::new(Vec::new());
        w.write_all(b"cookie=sess").unwrap();
        w.write_all(b"ion=abc123secret\nnext line\n").unwrap();
        w.write_all(b"partial password=hunter2").unwrap();
        w.flush().unwrap();
        let text = String::from_utf8(w.get_ref().clone()).unwrap();
        assert_eq!(
            text,
            "cookie=[REDACTED]\nnext line\npartial password=[REDACTED]"
        );
    }
}
