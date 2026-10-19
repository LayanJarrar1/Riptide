//! One handler per endpoint, plus the shared "serve a deterministic resource" logic that gives
//! every `/ok`-like endpoint the same `Range` / `If-Range` / `HEAD` behaviour.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use flate2::write::GzEncoder;
use flate2::Compression;
use md5::{Digest as _, Md5};

use crate::content::{self, Delivery, CHUNK_SIZE, DEFAULT_SEED, DEFAULT_SIZE, LAST_MODIFIED};
use crate::state::{InFlightGuard, RequestRecord, ServerState, Shared};

type Params = HashMap<String, String>;
type App = State<Arc<ServerState>>;

/// A plain-text error response (`400` for bad query parameters, `500` for internal failures).
#[derive(Debug)]
pub(crate) struct HttpError(StatusCode, String);

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        plain(self.0, self.1)
    }
}

impl From<header::InvalidHeaderValue> for HttpError {
    fn from(err: header::InvalidHeaderValue) -> Self {
        internal(format!("invalid header value: {err}"))
    }
}

pub(crate) type HandlerResult = Result<Response, HttpError>;

fn bad(msg: impl Into<String>) -> HttpError {
    HttpError(StatusCode::BAD_REQUEST, msg.into())
}

fn internal(msg: impl Into<String>) -> HttpError {
    HttpError(StatusCode::INTERNAL_SERVER_ERROR, msg.into())
}

/// A `text/plain; charset=utf-8` response.
pub(crate) fn plain(status: StatusCode, body: impl Into<String>) -> Response {
    let mut resp = Response::new(Body::from(body.into()));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}

// ---------------------------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------------------------

fn parse_u64(s: &str) -> Option<u64> {
    let t = s.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    }
}

/// `u64` parameter; accepts decimal or `0x`-prefixed hex.
fn param_u64(q: &Params, key: &str, default: u64) -> Result<u64, HttpError> {
    match q.get(key) {
        None => Ok(default),
        Some(v) => {
            parse_u64(v).ok_or_else(|| bad(format!("invalid {key}={v:?}: expected an integer")))
        }
    }
}

fn param_i64(q: &Params, key: &str, default: i64) -> Result<i64, HttpError> {
    match q.get(key) {
        None => Ok(default),
        Some(v) => v
            .trim()
            .parse()
            .map_err(|_| bad(format!("invalid {key}={v:?}: expected a signed integer"))),
    }
}

fn param_f64(q: &Params, key: &str, default: f64) -> Result<f64, HttpError> {
    match q.get(key) {
        None => Ok(default),
        Some(v) => v
            .trim()
            .parse()
            .map_err(|_| bad(format!("invalid {key}={v:?}: expected a number"))),
    }
}

fn param_bool(q: &Params, key: &str, default: bool) -> Result<bool, HttpError> {
    match q.get(key).map(|v| v.trim().to_ascii_lowercase()) {
        None => Ok(default),
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on") => Ok(true),
        Some(v) if matches!(v.as_str(), "" | "0" | "false" | "no" | "off") => Ok(false),
        Some(v) => Err(bad(format!("invalid {key}={v:?}: expected 0 or 1"))),
    }
}

// ---------------------------------------------------------------------------------------------
// Resource serving (shared by every /ok-like endpoint)
// ---------------------------------------------------------------------------------------------

/// What is being served.
pub(crate) struct Resource {
    pub(crate) seed: u64,
    pub(crate) size: u64,
    pub(crate) etag: String,
    /// Advertise `Accept-Ranges: bytes` and honour `Range`.
    ranges: bool,
    /// Send `Content-Length`. `false` makes hyper use chunked transfer encoding.
    known_length: bool,
    /// Added to the declared `Content-Length` (`/wrong-length`).
    length_delta: i64,
    /// Extra response headers (appended after the common ones).
    pub(crate) extra: Vec<(HeaderName, HeaderValue)>,
}

impl Resource {
    pub(crate) fn new(seed: u64, size: u64) -> Self {
        Self {
            seed,
            size,
            etag: content::etag(seed, size),
            ranges: true,
            known_length: true,
            length_delta: 0,
            extra: Vec::new(),
        }
    }

    fn from_params(q: &Params) -> Result<Self, HttpError> {
        let seed = param_u64(q, "seed", DEFAULT_SEED)?;
        let size = param_u64(q, "size", DEFAULT_SIZE)?;
        Ok(Self::new(seed, size))
    }
}

/// How the body is shaped on the wire.
#[derive(Default)]
pub(crate) struct Shaping {
    bps: Option<u64>,
    drop_after: Option<u64>,
    flips: Vec<u64>,
    guard: Option<InFlightGuard>,
    chunk: Option<usize>,
}

/// Outcome of parsing the `Range` header against a resource of a known size.
enum RangeSpec {
    /// No usable single range: serve the whole resource with `200`.
    Whole,
    /// A syntactically valid range that lies entirely outside the resource: `416`.
    Unsatisfiable,
    /// `[start, end)`.
    Part(u64, u64),
}

/// Parses a single `bytes=a-b` / `bytes=a-` / `bytes=-n` range. Multiple ranges and malformed
/// headers are ignored (the whole resource is served), as RFC 9110 permits.
fn parse_range(headers: &HeaderMap, size: u64) -> RangeSpec {
    let Some(value) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) else {
        return RangeSpec::Whole;
    };
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeSpec::Whole;
    };
    if spec.contains(',') {
        return RangeSpec::Whole;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeSpec::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    match (first.is_empty(), last.is_empty()) {
        (true, true) => RangeSpec::Whole,
        (true, false) => match last.parse::<u64>() {
            Ok(0) => RangeSpec::Unsatisfiable,
            Ok(_) if size == 0 => RangeSpec::Unsatisfiable,
            Ok(n) => RangeSpec::Part(size.saturating_sub(n), size),
            Err(_) => RangeSpec::Whole,
        },
        (false, true) => match first.parse::<u64>() {
            Ok(start) if start >= size => RangeSpec::Unsatisfiable,
            Ok(start) => RangeSpec::Part(start, size),
            Err(_) => RangeSpec::Whole,
        },
        (false, false) => match (first.parse::<u64>(), last.parse::<u64>()) {
            (Ok(start), Ok(end)) if end < start => RangeSpec::Whole,
            (Ok(start), Ok(_)) if start >= size => RangeSpec::Unsatisfiable,
            (Ok(start), Ok(end)) => RangeSpec::Part(start, end.saturating_add(1).min(size)),
            _ => RangeSpec::Whole,
        },
    }
}

/// `If-Range` matches when it is absent, equals the strong ETag or equals the fixed
/// `Last-Modified` date.
fn if_range_matches(headers: &HeaderMap, res: &Resource) -> bool {
    match headers.get(header::IF_RANGE).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(v) => {
            let v = v.trim();
            v == res.etag || v == LAST_MODIFIED
        }
    }
}

fn common_headers(h: &mut HeaderMap, res: &Resource) -> Result<(), HttpError> {
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::ETAG, HeaderValue::from_str(&res.etag)?);
    h.insert(
        header::LAST_MODIFIED,
        HeaderValue::from_static(LAST_MODIFIED),
    );
    if res.ranges {
        h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    for (name, value) in &res.extra {
        h.append(name.clone(), value.clone());
    }
    Ok(())
}

fn range_not_satisfiable(res: &Resource) -> HandlerResult {
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
    let h = resp.headers_mut();
    common_headers(h, res)?;
    h.insert(
        header::CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes */{}", res.size))?,
    );
    Ok(resp)
}

/// Serves `res` honouring `Range`, `If-Range` and `HEAD`, with the body shaped by `shaping`.
pub(crate) fn serve(
    method: &Method,
    headers: &HeaderMap,
    res: Resource,
    shaping: Shaping,
) -> HandlerResult {
    let size = res.size;
    let (status, start, end) = if res.ranges {
        match parse_range(headers, size) {
            RangeSpec::Whole => (StatusCode::OK, 0, size),
            RangeSpec::Unsatisfiable => return range_not_satisfiable(&res),
            RangeSpec::Part(start, end) => {
                if if_range_matches(headers, &res) {
                    (StatusCode::PARTIAL_CONTENT, start, end)
                } else {
                    (StatusCode::OK, 0, size)
                }
            }
        }
    } else {
        (StatusCode::OK, 0, size)
    };

    // When the declared length exceeds the body, ending the stream normally makes hyper abort
    // without flushing; failing the stream right after the last byte flushes first and then
    // closes, so the client sees the whole body and a connection error.
    let drop_after = if res.length_delta > 0 {
        Some(shaping.drop_after.unwrap_or(u64::MAX).min(end - start))
    } else {
        shaping.drop_after
    };

    // For HEAD hyper drops the body anyway; an empty body keeps the explicit Content-Length. For
    // a length-less resource a streaming body is kept so that hyper does not add
    // `Content-Length: 0` to the HEAD response.
    let body = if *method == Method::HEAD && res.known_length {
        Body::empty()
    } else {
        Body::from_stream(content::body_stream(Delivery {
            seed: res.seed,
            start,
            end,
            chunk: shaping.chunk.unwrap_or(CHUNK_SIZE),
            bps: shaping.bps,
            drop_after,
            flips: Arc::from(shaping.flips),
            _guard: shaping.guard,
        }))
    };

    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    let h = resp.headers_mut();
    common_headers(h, &res)?;
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end - 1, size))?,
        );
    }
    if res.known_length {
        let declared = (i128::from(end - start) + i128::from(res.length_delta)).max(0);
        h.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&declared.to_string())?,
        );
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------------------------

/// `/` — plain-text index.
pub(crate) async fn index(State(st): App) -> Response {
    let text = format!(
        "riptide-testserver\n\
         origin: {}\nalt origin: {}\n\n\
         Common query parameters: size= (bytes, default 1048576), seed= (u64, default {DEFAULT_SEED}).\n\n\
         /ok                      ranges, ETag, Last-Modified, Content-Length, If-Range, 416\n\
         /noranges                ignores Range, always 200, no Accept-Ranges\n\
         /nolength                chunked transfer, no Content-Length\n\
         /slow?bps=               throttled bandwidth (bytes per second), ranges like /ok\n\
         /drop?after=             closes the connection after N body bytes\n\
         /flaky?rate=             5xx or connection reset with probability rate (deterministic PRNG)\n\
         /retry-after?fails=      first N requests answer 503 + Retry-After: 1\n\
         /limit?conns=[&bps=]     429 when more than N bodies are in flight\n\
         /redirect?n=&cross=&code=&to=  redirect chain; cross=1 hops to the other origin\n\
         /etag-changes?after=     validator and bytes change after N requests\n\
         /wrong-length?delta=     Content-Length = size + delta, body is size bytes\n\
         /expires?after=          403 once N seconds passed since the first request\n\
         /needs-cookie            403 unless Cookie contains session=riptide\n\
         /needs-referer           403 unless Referer is this origin\n\
         /needs-auth/basic        401 unless Basic user:pass\n\
         /needs-auth/digest       401 unless RFC 7616 MD5 digest for user/pass\n\
         /gzip                    Content-Encoding: gzip even when identity was requested\n\
         /cd/arabic               Content-Disposition with an RFC 5987 Arabic filename\n\
         /cd/evil                 Content-Disposition with traversal, CON, %01 and trailing dot/space\n\
         /corrupt?flip=           flips N bits, ETag and Content-Length unchanged\n\
         /site/, /robots.txt      crawlable site: depth 3, loops, off-site link, 404, big pages, files\n\
         /echo-headers            request headers as JSON\n",
        st.origin(),
        st.alt_origin(),
    );
    plain(StatusCode::OK, text)
}

/// `/ok`
pub(crate) async fn ok(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

/// `/noranges`
pub(crate) async fn noranges(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let mut res = Resource::from_params(&q)?;
    res.ranges = false;
    serve(&method, &headers, res, Shaping::default())
}

/// `/nolength`
pub(crate) async fn nolength(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let mut res = Resource::from_params(&q)?;
    res.ranges = false;
    res.known_length = false;
    serve(&method, &headers, res, Shaping::default())
}

/// `/slow?bps=`
pub(crate) async fn slow(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let bps = param_u64(&q, "bps", 1024 * 1024)?;
    let shaping = if bps == 0 {
        Shaping::default()
    } else {
        // Smaller chunks when the budget is small, so the throttle is smooth (about 8 chunks/s).
        let chunk = (bps / 8).clamp(1024, CHUNK_SIZE as u64) as usize;
        Shaping {
            bps: Some(bps),
            chunk: Some(chunk),
            ..Shaping::default()
        }
    };
    serve(&method, &headers, Resource::from_params(&q)?, shaping)
}

/// `/drop?after=`
pub(crate) async fn drop_after(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let res = Resource::from_params(&q)?;
    let after = param_u64(&q, "after", res.size / 2)?;
    serve(
        &method,
        &headers,
        res,
        Shaping {
            drop_after: Some(after),
            ..Shaping::default()
        },
    )
}

/// `/flaky?rate=`
pub(crate) async fn flaky(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let rate = param_f64(&q, "rate", 0.5)?.clamp(0.0, 1.0);
    let res = Resource::from_params(&q)?;
    let shared = st.shared();
    if shared.next_unit() < rate {
        return match shared.next_random() % 4 {
            0 => Ok(plain(StatusCode::INTERNAL_SERVER_ERROR, "flaky: 500")),
            1 => Ok(plain(StatusCode::BAD_GATEWAY, "flaky: 502")),
            2 => Ok(plain(StatusCode::SERVICE_UNAVAILABLE, "flaky: 503")),
            // "Reset": headers go out, then the connection is closed before any body byte.
            _ => serve(
                &method,
                &headers,
                res,
                Shaping {
                    drop_after: Some(0),
                    ..Shaping::default()
                },
            ),
        };
    }
    serve(&method, &headers, res, Shaping::default())
}

/// `/retry-after?fails=`
pub(crate) async fn retry_after(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let fails = param_u64(&q, "fails", 1)?;
    let seen = st.shared().retry_after_hits.fetch_add(1, Ordering::SeqCst);
    if seen < fails {
        let mut resp = plain(StatusCode::SERVICE_UNAVAILABLE, "try again later");
        resp.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        return Ok(resp);
    }
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

/// `/limit?conns=[&bps=]`
pub(crate) async fn limit(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let conns = usize::try_from(param_u64(&q, "conns", 2)?).unwrap_or(usize::MAX);
    // optional `bps=` keeps each body in flight long enough for connections to overlap
    let bps = param_u64(&q, "bps", 0)?;
    match st.shared().try_acquire(conns) {
        Some(guard) => serve(
            &method,
            &headers,
            Resource::from_params(&q)?,
            Shaping {
                guard: Some(guard),
                bps: (bps > 0).then_some(bps),
                chunk: (bps > 0).then(|| (bps / 8).clamp(1024, CHUNK_SIZE as u64) as usize),
                ..Shaping::default()
            },
        ),
        None => {
            let mut resp = plain(StatusCode::TOO_MANY_REQUESTS, "too many connections");
            resp.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            Ok(resp)
        }
    }
}

const REDIRECT_CYCLE: [StatusCode; 5] = [
    StatusCode::MOVED_PERMANENTLY,
    StatusCode::FOUND,
    StatusCode::SEE_OTHER,
    StatusCode::TEMPORARY_REDIRECT,
    StatusCode::PERMANENT_REDIRECT,
];

/// `/redirect?n=&cross=&code=&to=`
pub(crate) async fn redirect(State(st): App, Query(q): Query<Params>) -> HandlerResult {
    let n = param_u64(&q, "n", 1)?;
    let cross = param_bool(&q, "cross", false)?;
    let code = q.get("code").map_or("302", String::as_str);
    let status = if code.eq_ignore_ascii_case("cycle") {
        REDIRECT_CYCLE[(n % 5) as usize]
    } else {
        code.parse::<u16>()
            .ok()
            .and_then(|c| StatusCode::from_u16(c).ok())
            .filter(|s| REDIRECT_CYCLE.contains(s))
            .ok_or_else(|| bad("code must be 301, 302, 303, 307, 308 or cycle"))?
    };
    let to = q.get("to").map_or("ok", String::as_str);
    let final_path = match to {
        "ok" => "/ok",
        "echo-headers" => "/echo-headers",
        _ => return Err(bad("to must be ok or echo-headers")),
    };

    let mut passthrough = String::new();
    for key in ["size", "seed"] {
        if q.contains_key(key) {
            let value = param_u64(&q, key, 0)?;
            passthrough.push_str(&format!("&{key}={value}"));
        }
    }

    let target = if n == 0 {
        match passthrough.strip_prefix('&') {
            Some(query) => format!("{final_path}?{query}"),
            None => final_path.to_owned(),
        }
    } else {
        let mut t = format!("/redirect?n={}&code={}", n - 1, code);
        if to != "ok" {
            t.push_str("&to=");
            t.push_str(to);
        }
        t.push_str(&passthrough);
        t
    };
    // Same-origin hops use a relative Location (clients must resolve it); the cross-origin hop
    // is absolute.
    let location = if cross {
        format!("{}{}", st.alt_origin(), target)
    } else {
        target
    };

    let mut resp = plain(status, format!("redirecting to {location}"));
    resp.headers_mut()
        .insert(header::LOCATION, HeaderValue::from_str(&location)?);
    Ok(resp)
}

/// `/etag-changes?after=`
pub(crate) async fn etag_changes(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let after = param_u64(&q, "after", 1)?;
    let seen = st.shared().etag_changes_hits.fetch_add(1, Ordering::SeqCst);
    let mut res = Resource::from_params(&q)?;
    if seen >= after {
        res.seed = res.seed.wrapping_add(1);
        let inner = content::etag(res.seed, res.size);
        res.etag = format!("\"v2-{}\"", inner.trim_matches('"'));
    }
    serve(&method, &headers, res, Shaping::default())
}

/// `/wrong-length?delta=`
pub(crate) async fn wrong_length(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let delta = param_i64(&q, "delta", 1024)?;
    let mut res = Resource::from_params(&q)?;
    res.length_delta = delta;
    if delta < 0 {
        // hyper stops writing at the declared length; close so the client cannot reuse the
        // connection and read the leftover bytes as the next response.
        res.extra
            .push((header::CONNECTION, HeaderValue::from_static("close")));
    }
    serve(&method, &headers, res, Shaping::default())
}

/// `/expires?after=`
pub(crate) async fn expires(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let after = param_u64(&q, "after", 5)?;
    let first = *st.shared().expires_first_hit.get_or_init(Instant::now);
    if first.elapsed() > Duration::from_secs(after) {
        return Ok(plain(StatusCode::FORBIDDEN, "signed URL expired"));
    }
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

/// `/needs-cookie`
pub(crate) async fn needs_cookie(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let present = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .any(|pair| pair.trim() == "session=riptide");
    if !present {
        return Ok(plain(
            StatusCode::FORBIDDEN,
            "cookie session=riptide required",
        ));
    }
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

/// `host:port` part of an absolute URL.
fn authority_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    rest.split(['/', '?', '#']).next()
}

/// `/needs-referer`
pub(crate) async fn needs_referer(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .and_then(authority_of);
    let own = authority_of(st.origin());
    if referer.is_none() || referer != own {
        return Ok(plain(
            StatusCode::FORBIDDEN,
            "referer from this origin required",
        ));
    }
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

/// `/needs-auth/basic`
pub(crate) async fn needs_auth_basic(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let (scheme, rest) = v.trim().split_once(' ')?;
            scheme.eq_ignore_ascii_case("basic").then_some(rest.trim())
        })
        .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
        .is_some_and(|creds| creds == b"user:pass");
    if !ok {
        let mut resp = plain(StatusCode::UNAUTHORIZED, "basic auth required");
        resp.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"riptide\""),
        );
        return Ok(resp);
    }
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

fn md5_hex(input: &str) -> String {
    hex::encode(Md5::digest(input.as_bytes()))
}

/// Parses the `k=v, k2="quoted, value"` list of a `Digest` credential. Backslash escapes inside
/// quoted strings are not interpreted.
fn parse_auth_params(input: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = input.trim();
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };
        let key = key
            .trim_matches(|c: char| c == ',' || c.is_whitespace())
            .to_ascii_lowercase();
        let after = after.trim_start();
        let (value, remaining) = match after.strip_prefix('"') {
            Some(quoted) => match quoted.find('"') {
                Some(end) => (&quoted[..end], &quoted[end + 1..]),
                None => (quoted, ""),
            },
            None => match after.find(',') {
                Some(i) => (&after[..i], &after[i + 1..]),
                None => (after, ""),
            },
        };
        out.insert(key, value.trim().to_owned());
        rest = remaining.trim_start().trim_start_matches(',').trim_start();
    }
    out
}

/// Verifies an RFC 7616 `Digest` credential (MD5 or MD5-sess, `qop=auth` or no qop) for
/// `user` / `pass` in realm `riptide` against the server's nonce.
fn digest_ok(shared: &Shared, method: &Method, headers: &HeaderMap) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some((scheme, rest)) = value.trim().split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("digest") {
        return false;
    }
    let p = parse_auth_params(rest);
    let get = |k: &str| p.get(k).map(String::as_str);
    let (Some(username), Some(realm), Some(nonce), Some(uri), Some(response)) = (
        get("username"),
        get("realm"),
        get("nonce"),
        get("uri"),
        get("response"),
    ) else {
        return false;
    };
    if username != "user" || realm != "riptide" || nonce != shared.digest_nonce {
        return false;
    }
    let algorithm = get("algorithm").map_or_else(|| "MD5".to_owned(), str::to_ascii_uppercase);
    let mut ha1 = md5_hex("user:riptide:pass");
    match algorithm.as_str() {
        "MD5" => {}
        "MD5-SESS" => {
            let Some(cnonce) = get("cnonce") else {
                return false;
            };
            ha1 = md5_hex(&format!("{ha1}:{nonce}:{cnonce}"));
        }
        _ => return false,
    }
    let ha2 = md5_hex(&format!("{}:{}", method.as_str(), uri));
    let expected = match get("qop") {
        None => md5_hex(&format!("{ha1}:{nonce}:{ha2}")),
        Some(qop) if qop.eq_ignore_ascii_case("auth") => {
            let (Some(nc), Some(cnonce)) = (get("nc"), get("cnonce")) else {
                return false;
            };
            md5_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"))
        }
        Some(_) => return false,
    };
    expected.eq_ignore_ascii_case(response)
}

/// `/needs-auth/digest`
pub(crate) async fn needs_auth_digest(
    State(st): App,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let shared = st.shared();
    if !digest_ok(shared, &method, &headers) {
        let mut resp = plain(StatusCode::UNAUTHORIZED, "digest auth required");
        let challenge = format!(
            "Digest realm=\"riptide\", qop=\"auth\", nonce=\"{}\", opaque=\"{}\", algorithm=MD5",
            shared.digest_nonce, shared.digest_opaque
        );
        resp.headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_str(&challenge)?);
        return Ok(resp);
    }
    serve(
        &method,
        &headers,
        Resource::from_params(&q)?,
        Shaping::default(),
    )
}

/// `/gzip` — the whole compressed body is built in memory (on the blocking pool), so keep `size`
/// moderate.
pub(crate) async fn gzip(method: Method, Query(q): Query<Params>) -> HandlerResult {
    let res = Resource::from_params(&q)?;
    let (seed, size) = (res.seed, res.size);
    let compressed = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        let mut offset = 0u64;
        while offset < size {
            let len = (size - offset).min(CHUNK_SIZE as u64) as usize;
            enc.write_all(&content::bytes_at(seed, offset, len))?;
            offset += len as u64;
        }
        enc.finish()
    })
    .await
    .map_err(|e| internal(format!("gzip task failed: {e}")))?
    .map_err(|e| internal(format!("gzip failed: {e}")))?;

    let len = compressed.len();
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(compressed)
    };
    let mut resp = Response::new(body);
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    h.insert(header::ETAG, HeaderValue::from_str(&res.etag)?);
    h.insert(
        header::LAST_MODIFIED,
        HeaderValue::from_static(LAST_MODIFIED),
    );
    h.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string())?,
    );
    Ok(resp)
}

/// `/cd/arabic`
pub(crate) async fn cd_arabic(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let mut res = Resource::from_params(&q)?;
    res.extra.push((
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(
            "attachment; filename*=UTF-8''%D9%85%D9%84%D9%81%20%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A.zip",
        ),
    ));
    serve(&method, &headers, res, Shaping::default())
}

/// `/cd/evil` — a raw `0x01` byte is not a legal header octet (hyper refuses to send it and
/// clients refuse to parse it), so the control character is sent percent-encoded as `%01` in the
/// plain `filename` and, decoded to the real `0x01` by RFC 5987 rules, in `filename*`.
pub(crate) async fn cd_evil(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let mut res = Resource::from_params(&q)?;
    res.extra.push((
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(
            "attachment; filename=\"../../CON%01.txt. \"; filename*=UTF-8''..%2F..%2FCON%01.txt.%20",
        ),
    ));
    serve(&method, &headers, res, Shaping::default())
}

/// `/corrupt?flip=`
pub(crate) async fn corrupt(
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let flip = param_u64(&q, "flip", 1)?;
    let res = Resource::from_params(&q)?;
    let flips = content::corrupt_bit_positions(res.seed, res.size, flip);
    serve(
        &method,
        &headers,
        res,
        Shaping {
            flips,
            ..Shaping::default()
        },
    )
}

/// `/echo-headers` — request headers as a JSON object (repeated headers joined with `", "`).
pub(crate) async fn echo_headers(headers: HeaderMap) -> Response {
    let mut map = serde_json::Map::new();
    for (name, value) in &headers {
        let text = String::from_utf8_lossy(value.as_bytes()).into_owned();
        match map.get_mut(name.as_str()) {
            Some(serde_json::Value::String(existing)) => {
                existing.push_str(", ");
                existing.push_str(&text);
            }
            _ => {
                map.insert(name.as_str().to_owned(), serde_json::Value::String(text));
            }
        }
    }
    axum::Json(serde_json::Value::Object(map)).into_response()
}

/// Logs one line per request (method, path without the query string, status).
pub(crate) async fn log_request(State(st): App, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    st.shared().record(RequestRecord {
        origin: st.origin().to_owned(),
        method: method.to_string(),
        target: req
            .uri()
            .path_and_query()
            .map_or_else(|| path.clone(), |pq| pq.as_str().to_owned()),
        has_cookie: req.headers().contains_key(header::COOKIE),
        has_authorization: req.headers().contains_key(header::AUTHORIZATION),
    });
    let resp = next.run(req).await;
    tracing::info!(%method, %path, status = resp.status().as_u16(), "request");
    resp
}

/// `/batch/<name>` - `fileNNN.zip` (001-120) and `img_<a-z>.jpg`; every other name is `404`.
/// Each name has its own seed and a default size ([`crate::batch::item`]); `?size=` overrides
/// the size. Ranges, `ETag` and `HEAD` behave like `/ok`.
pub(crate) async fn batch_item(
    Path(name): Path<String>,
    method: Method,
    Query(q): Query<Params>,
    headers: HeaderMap,
) -> HandlerResult {
    let Some(item) = crate::batch::item(&name) else {
        return Ok(plain(
            StatusCode::NOT_FOUND,
            format!(
                "no batch item named {name:?}
"
            ),
        ));
    };
    let size = param_u64(&q, "size", item.size)?;
    serve(
        &method,
        &headers,
        Resource::new(item.seed, size),
        Shaping::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(name: HeaderName, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, HeaderValue::from_str(value).unwrap());
        h
    }

    fn part(h: &HeaderMap, size: u64) -> Option<(u64, u64)> {
        match parse_range(h, size) {
            RangeSpec::Part(s, e) => Some((s, e)),
            _ => None,
        }
    }

    #[test]
    fn range_parsing() {
        let size = 1000;
        assert_eq!(
            part(&headers_with(header::RANGE, "bytes=0-99"), size),
            Some((0, 100))
        );
        assert_eq!(
            part(&headers_with(header::RANGE, "bytes=900-"), size),
            Some((900, 1000))
        );
        assert_eq!(
            part(&headers_with(header::RANGE, "bytes=-10"), size),
            Some((990, 1000))
        );
        assert_eq!(
            part(&headers_with(header::RANGE, "bytes=-5000"), size),
            Some((0, 1000))
        );
        assert_eq!(
            part(&headers_with(header::RANGE, "bytes=10-5000"), size),
            Some((10, 1000))
        );
        assert!(matches!(
            parse_range(&headers_with(header::RANGE, "bytes=1000-"), size),
            RangeSpec::Unsatisfiable
        ));
        assert!(matches!(
            parse_range(&headers_with(header::RANGE, "bytes=-0"), size),
            RangeSpec::Unsatisfiable
        ));
        assert!(matches!(
            parse_range(&headers_with(header::RANGE, "bytes=0-1,5-6"), size),
            RangeSpec::Whole
        ));
        assert!(matches!(
            parse_range(&headers_with(header::RANGE, "bytes=9-5"), size),
            RangeSpec::Whole
        ));
        assert!(matches!(
            parse_range(&headers_with(header::RANGE, "items=0-1"), size),
            RangeSpec::Whole
        ));
        assert!(matches!(
            parse_range(&HeaderMap::new(), size),
            RangeSpec::Whole
        ));
    }

    #[test]
    fn auth_param_parsing() {
        let p = parse_auth_params(
            "username=\"user\", realm=\"riptide\",nonce=\"abc\", uri=\"/x?a=1,2\", qop=auth, nc=00000001, cnonce=\"zz\", response=\"ff\"",
        );
        assert_eq!(p["username"], "user");
        assert_eq!(p["uri"], "/x?a=1,2");
        assert_eq!(p["qop"], "auth");
        assert_eq!(p["nc"], "00000001");
        assert_eq!(p["response"], "ff");
    }

    #[test]
    fn authority_extraction() {
        assert_eq!(
            authority_of("http://127.0.0.1:8080"),
            Some("127.0.0.1:8080")
        );
        assert_eq!(
            authority_of("http://127.0.0.1:8080/x?y#z"),
            Some("127.0.0.1:8080")
        );
        assert_eq!(authority_of("nope"), None);
    }

    #[test]
    fn params() {
        let mut q = Params::new();
        q.insert("size".into(), "0x10".into());
        q.insert("delta".into(), "-5".into());
        q.insert("cross".into(), "1".into());
        q.insert("rate".into(), "0.25".into());
        assert_eq!(param_u64(&q, "size", 1).unwrap(), 16);
        assert_eq!(param_u64(&q, "missing", 1).unwrap(), 1);
        assert_eq!(param_i64(&q, "delta", 0).unwrap(), -5);
        assert!(param_bool(&q, "cross", false).unwrap());
        assert!((param_f64(&q, "rate", 0.0).unwrap() - 0.25).abs() < f64::EPSILON);
        q.insert("size".into(), "ten".into());
        assert!(param_u64(&q, "size", 1).is_err());
    }
}
