//! Strict validation of the media (video page) messages a browser may send.
//!
//! The generic schema check in [`crate::check_request`] only proves a frame parses. Media
//! requests additionally end up as arguments of an external tool, so every field is checked
//! here before anything is forwarded:
//!
//! * the URL is `http(s)` with a host, no credentials, no control characters, at most
//!   [`MAX_URL_LEN`] bytes;
//! * the quality (`media_format`) is one of the fixed keys of [`MediaSelection`] (`best`,
//!   `h<N>`, `audio-m4a`, `audio-mp3`), never a raw yt-dlp selector;
//! * every text field has a size limit;
//! * cookies are kept only when the sender names the origin they were read for
//!   (`cookie_origin`, a host-only field that is never forwarded) and that origin is the
//!   origin of the page URL; otherwise they are dropped. Extra headers and credentials never
//!   travel with a media request;
//! * `dir` is refused: the browser never picks a folder.

use riptide_core::{DownloadKind, MediaSelection, Secrets};
use riptide_ipc::{ErrorCode, Request, RequestBody, Response};
use serde_json::Value;
use url::Url;

/// Longest URL accepted (bytes).
pub const MAX_URL_LEN: usize = 2048;
/// Longest `Cookie` header accepted for one media request (bytes).
pub const MAX_COOKIE_LEN: usize = 16 * 1024;
/// Longest suggested file name (bytes).
pub const MAX_NAME_LEN: usize = 255;
/// Longest user agent (bytes).
pub const MAX_UA_LEN: usize = 512;

/// A reason to refuse a media request; the text is safe to show.
type Refusal = &'static str;

/// Parses `raw` as a plain `http(s)` URL.
fn http_url(raw: &str) -> Result<Url, Refusal> {
    if raw.len() > MAX_URL_LEN {
        return Err("url is too long");
    }
    if raw.chars().any(char::is_control) {
        return Err("url contains control characters");
    }
    let url = Url::parse(raw).map_err(|_| "url is not valid")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https urls are accepted");
    }
    if url.host_str().is_none() {
        return Err("url has no host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("url must not carry credentials");
    }
    Ok(url)
}

fn text_ok(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}

/// Keeps `cookies` only if `claimed` is the origin of `page`.
fn scoped_cookies(cookies: Option<String>, claimed: Option<&str>, page: &Url) -> Option<String> {
    let cookies = cookies.filter(|c| !c.is_empty() && c.len() <= MAX_COOKIE_LEN)?;
    let claimed = http_url(claimed?).ok()?;
    (claimed.origin() == page.origin()).then_some(cookies)
}

fn refuse(id: u64, reason: Refusal) -> Response {
    Response::error(id, ErrorCode::BadRequest, reason)
}

/// Validates and normalises the media variants of `request` in place. `frame` is the raw JSON
/// the request was parsed from (it carries the host-only `cookie_origin`). Requests that are not
/// media requests are left untouched.
#[allow(clippy::result_large_err)]
pub fn check_media(request: &mut Request, frame: &Value) -> Result<(), Response> {
    let id = request.id;
    match &mut request.body {
        RequestBody::ProbeMedia { url, cookies } => {
            let page = http_url(url).map_err(|r| refuse(id, r))?;
            let claimed = frame.get("cookie_origin").and_then(Value::as_str);
            *cookies = scoped_cookies(cookies.take(), claimed, &page);
            Ok(())
        }
        RequestBody::Add { request: add } => {
            if add.kind != DownloadKind::Media {
                if add.media_format.is_some() {
                    return Err(refuse(id, "media_format is only valid with kind media"));
                }
                return Ok(());
            }
            let page = http_url(&add.url).map_err(|r| refuse(id, r))?;
            if let Some(key) = &add.media_format {
                if MediaSelection::parse(key).is_none() {
                    return Err(refuse(id, "unknown media quality"));
                }
            }
            if add.dir.is_some() {
                return Err(refuse(id, "the browser cannot choose a folder"));
            }
            if add
                .filename
                .as_deref()
                .is_some_and(|n| !text_ok(n, MAX_NAME_LEN))
            {
                return Err(refuse(id, "filename is too long or has control characters"));
            }
            if add
                .user_agent
                .as_deref()
                .is_some_and(|u| !text_ok(u, MAX_UA_LEN))
            {
                return Err(refuse(
                    id,
                    "user agent is too long or has control characters",
                ));
            }
            if let Some(referrer) = &add.referrer {
                http_url(referrer)
                    .map_err(|_| refuse(id, "referrer is not a valid http(s) url"))?;
            }
            let claimed = frame
                .get("request")
                .and_then(|r| r.get("cookie_origin"))
                .and_then(Value::as_str);
            let cookies = add.secrets.take().and_then(|s| s.cookies);
            add.secrets = scoped_cookies(cookies, claimed, &page).map(|c| Secrets {
                cookies: Some(c),
                ..Secrets::default()
            });
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[allow(clippy::result_large_err)]
    fn run(frame: Value) -> Result<Request, Response> {
        crate::check_request(&frame)
    }

    fn message(r: &Response) -> String {
        match &r.body {
            riptide_ipc::ResponseBody::Error { code, message } => {
                assert_eq!(*code, ErrorCode::BadRequest);
                message.clone()
            }
            other => panic!("not an error: {other:?}"),
        }
    }

    fn add(extra: Value) -> Value {
        let mut request = json!({
            "url": "https://www.youtube.com/watch?v=x", "kind": "media", "media_format": "h720"
        });
        for (k, v) in extra.as_object().unwrap() {
            request[k] = v.clone();
        }
        json!({"id": 1, "type": "add", "request": request})
    }

    fn cookies_of(r: &Request) -> Option<String> {
        match &r.body {
            RequestBody::Add { request } => {
                request.secrets.as_ref().and_then(|s| s.cookies.clone())
            }
            RequestBody::ProbeMedia { cookies, .. } => cookies.clone(),
            _ => panic!(),
        }
    }

    #[test]
    fn good_media_messages_pass() {
        for key in ["best", "h1080", "h144", "audio-m4a", "audio-mp3"] {
            run(add(json!({"media_format": key}))).unwrap();
        }
        run(add(json!({"media_format": null}))).unwrap();
        run(json!({"id": 2, "type": "probe_media", "url": "http://vimeo.com/1"})).unwrap();
        // old messages keep working: a plain file add is untouched
        run(json!({"id": 3, "type": "add", "request": {"url": "https://a/b.zip"}})).unwrap();
    }

    #[test]
    fn bad_urls_are_refused() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://x/y",
            "not a url",
            "https://user:pw@example.com/",
            "https://example.com/\u{7}x",
            "https://",
        ] {
            let e = run(add(json!({"url": url}))).unwrap_err();
            message(&e);
            let e = run(json!({"id": 9, "type": "probe_media", "url": url})).unwrap_err();
            assert_eq!(e.id, 9);
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        message(&run(add(json!({"url": long}))).unwrap_err());
    }

    #[test]
    fn bad_quality_keys_are_refused() {
        for key in [
            "bestvideo+bestaudio",
            "-o /tmp/x",
            "h0",
            "h99999",
            "h-1",
            "H720",
            "audio-flac",
            "",
            "best ",
            "h720;calc",
        ] {
            let e = run(add(json!({"media_format": key}))).unwrap_err();
            assert_eq!(message(&e), "unknown media quality", "{key}");
        }
        let e = run(json!({
            "id": 1, "type": "add",
            "request": {"url": "https://a/b", "media_format": "h720"}
        }))
        .unwrap_err();
        assert!(message(&e).contains("only valid with kind media"));
    }

    #[test]
    fn oversized_fields_and_folders_are_refused() {
        message(&run(add(json!({"filename": "a".repeat(MAX_NAME_LEN + 1)}))).unwrap_err());
        message(&run(add(json!({"filename": "a\u{0}b"}))).unwrap_err());
        message(&run(add(json!({"user_agent": "u".repeat(MAX_UA_LEN + 1)}))).unwrap_err());
        message(&run(add(json!({"referrer": "javascript:1"}))).unwrap_err());
        message(&run(add(json!({"dir": "C:\\Windows"}))).unwrap_err());
        // oversized cookies are dropped, not forwarded
        let big = "a=".to_string() + &"b".repeat(MAX_COOKIE_LEN);
        let ok = run(add(json!({
            "secrets": {"cookies": big}, "cookie_origin": "https://www.youtube.com"
        })))
        .unwrap();
        assert_eq!(cookies_of(&ok), None);
    }

    #[test]
    fn cookies_survive_only_for_the_page_origin() {
        let jar = json!({"cookies": "SID=1; HSID=2"});
        let same = run(add(json!({
            "secrets": jar, "cookie_origin": "https://www.youtube.com"
        })))
        .unwrap();
        assert_eq!(cookies_of(&same).as_deref(), Some("SID=1; HSID=2"));

        for origin in [
            Some("https://evil.example"),
            Some("http://www.youtube.com"),
            Some("https://www.youtube.com:8443"),
            Some("https://youtube.com"),
            Some("garbage"),
            None,
        ] {
            let mut extra = json!({"secrets": jar});
            if let Some(o) = origin {
                extra["cookie_origin"] = json!(o);
            }
            let r = run(add(extra)).unwrap();
            assert_eq!(cookies_of(&r), None, "{origin:?}");
        }

        let probe = run(json!({
            "id": 4, "type": "probe_media", "url": "https://vimeo.com/1",
            "cookies": "a=b", "cookie_origin": "https://vimeo.com"
        }))
        .unwrap();
        assert_eq!(cookies_of(&probe).as_deref(), Some("a=b"));
        let cross = run(json!({
            "id": 4, "type": "probe_media", "url": "https://vimeo.com/1",
            "cookies": "a=b", "cookie_origin": "https://other.example"
        }))
        .unwrap();
        assert_eq!(cookies_of(&cross), None);
    }

    #[test]
    fn media_adds_never_carry_headers_or_credentials() {
        let r = run(add(json!({
            "secrets": {
                "cookies": "a=b",
                "headers": [["Authorization", "Bearer x"]],
                "auth": {"username": "u", "password": "p"}
            },
            "cookie_origin": "https://www.youtube.com"
        })))
        .unwrap();
        let RequestBody::Add { request } = r.body else {
            panic!()
        };
        let s = request.secrets.unwrap();
        assert_eq!(s.cookies.as_deref(), Some("a=b"));
        assert!(s.headers.is_empty() && s.auth.is_none());
    }
}
