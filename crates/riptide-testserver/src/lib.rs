//! Local HTTP test server for Riptide. Dev/test only.
//!
//! Every byte-serving endpoint streams deterministic bytes derived from `?seed=` and `?size=`
//! (see [`bytes_at`]), so a test can compare the SHA-256 of what the engine wrote with
//! [`expected_sha256_hex`] without fixtures. The crate is both a library (tests call
//! [`TestServer::spawn`] in-process) and a binary (`cargo run -p riptide-testserver`); both use
//! the same [`router`].
//!
//! Endpoints (all accept `size=` and `seed=`; `GET` routes also answer `HEAD`):
//!
//! | Endpoint | Behaviour |
//! |---|---|
//! | `/ok` | Single-range `206`, `416` with `Content-Range: bytes */size`, `If-Range` (ETag or date; mismatch → `200`), strong `ETag`, `Last-Modified`, `Content-Length`, `Accept-Ranges: bytes` |
//! | `/noranges` | Ignores `Range`, always `200`, no `Accept-Ranges` |
//! | `/nolength` | Chunked transfer, no `Content-Length` |
//! | `/slow?bps=` | Throttled bandwidth, ranges like `/ok` (`bps=0` = unthrottled) |
//! | `/drop?after=` | Correct headers, connection closed after `after` body bytes (default `size / 2`) |
//! | `/flaky?rate=` | With probability `rate` (default 0.5, deterministic PRNG seeded per server) answers `500`/`502`/`503` or closes right after the headers; otherwise like `/ok` |
//! | `/retry-after?fails=` | First `fails` (default 1) requests per server: `503` + `Retry-After: 1` |
//! | `/limit?conns=` | `429` while more than `conns` (default 2) bodies are in flight |
//! | `/redirect?n=&cross=&code=&to=` | Chain of `n` hops ending at `/ok` (or `/echo-headers` with `to=echo-headers`); `code` = 301/302/303/307/308 (default 302) or `cycle`; `cross=1` makes the first hop absolute to the other origin, other hops use relative `Location`s |
//! | `/etag-changes?after=` | After `after` (default 1) requests the ETag becomes `"v2-…"` and the bytes use `seed + 1` |
//! | `/wrong-length?delta=` | `Content-Length` = body length + `delta` (default 1024); body is `size` bytes. Positive: hyper aborts the connection after the body (short read). Negative: hyper sends exactly `Content-Length` bytes and the response carries `Connection: close` |
//! | `/expires?after=` | `403` once more than `after` seconds (default 5) passed since the first request |
//! | `/needs-cookie` | `403` unless `Cookie` contains `session=riptide` |
//! | `/needs-referer` | `403` unless `Referer`'s host:port is this origin |
//! | `/needs-auth/basic` | `401` + `WWW-Authenticate: Basic realm="riptide"` unless `user:pass` |
//! | `/needs-auth/digest` | `401` + RFC 7616 MD5 challenge (`qop="auth"`) unless a valid digest for `user`/`pass` |
//! | `/gzip` | gzip body + `Content-Encoding: gzip` even for `Accept-Encoding: identity`; `Content-Length` is the compressed length; whole body built in memory |
//! | `/cd/arabic` | `Content-Disposition: attachment; filename*=UTF-8''%D9%85%D9%84%D9%81%20%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A.zip` |
//! | `/cd/evil` | `Content-Disposition: attachment; filename="../../CON%01.txt. "; filename*=UTF-8''..%2F..%2FCON%01.txt.%20` (the control char is `%01`: a raw `0x01` is not a legal header octet) |
//! | `/corrupt?flip=` | Flips `flip` (default 1) bits at [`corrupt_bit_positions`]; `ETag`/`Content-Length` unchanged |
//! | `/batch/<name>` | `file001.zip` ... `file120.zip` and `img_a.jpg` ... `img_z.jpg`: per-name seed and size (4-260 KiB, see [`batch_item`]), like `/ok`; any other name is `404` |
//! | `/echo-headers` | Request headers as a JSON object |
//! | `/` | Plain-text index |
//!
//! Multi-range requests (`bytes=0-1,5-6`) and malformed `Range` headers are answered with the
//! whole resource (`200`). Connection "resets" are implemented by failing the body stream, which
//! makes hyper close the TCP connection before the declared length is reached.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod batch;
mod content;
mod handlers;
mod router;
mod server;
mod state;

pub use batch::{exists as batch_exists, item as batch_item, BatchItem};
pub use content::{
    bytes_at, corrupt_bit_positions, etag, expected_bytes, expected_sha256_hex, splitmix64,
    CHUNK_SIZE, DEFAULT_SEED, DEFAULT_SIZE, LAST_MODIFIED,
};
pub use router::router;
pub use server::TestServer;
pub use state::ServerState;
