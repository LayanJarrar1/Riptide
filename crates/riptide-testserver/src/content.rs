//! Deterministic content.
//!
//! Every byte-serving endpoint derives its body from a `seed` and a `size`, so a test can compute
//! the expected bytes (or just their SHA-256) locally instead of shipping fixtures. The content is
//! random-access: byte `i` of the resource lives in 8-byte block `i / 8`, and block `b` is
//! `splitmix64(seed ^ b)` in little-endian order. Serving a `Range` therefore costs nothing extra.

use std::collections::BTreeSet;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::Stream;
use sha2::{Digest, Sha256};

use crate::state::InFlightGuard;

/// Seed used when a request does not pass `?seed=`.
pub const DEFAULT_SEED: u64 = 0x5EED_5EED_5EED_5EED;

/// Size used when a request does not pass `?size=` (1 MiB).
pub const DEFAULT_SIZE: u64 = 1024 * 1024;

/// Size of the chunks a body is streamed in (64 KiB). Bodies are never materialised in full.
pub const CHUNK_SIZE: usize = 64 * 1024;

/// The fixed `Last-Modified` value every resource reports.
pub const LAST_MODIFIED: &str = "Wed, 01 Oct 2025 00:00:00 GMT";

/// SplitMix64 mixing function: a bijective 64-bit hash with full avalanche.
#[must_use]
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Returns `len` bytes of the resource identified by `seed`, starting at byte `offset`.
///
/// This is random-access: the prefix before `offset` is never generated, so it is cheap to serve
/// any `Range`. `bytes_at(seed, off, len)` always equals `expected_bytes(seed, size)[off..off + len]`
/// for every `size >= off + len`.
#[must_use]
pub fn bytes_at(seed: u64, offset: u64, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 8);
    if len == 0 {
        return out;
    }
    let mut block = offset / 8;
    let skip = (offset % 8) as usize;
    out.extend_from_slice(&splitmix64(seed ^ block).to_le_bytes()[skip..]);
    while out.len() < len {
        block += 1;
        out.extend_from_slice(&splitmix64(seed ^ block).to_le_bytes());
    }
    out.truncate(len);
    out
}

/// The whole resource `seed` of `size` bytes.
///
/// This allocates `size` bytes; use [`expected_sha256_hex`] for large sizes. Panics (capacity
/// overflow) when `size` does not fit in memory.
#[must_use]
pub fn expected_bytes(seed: u64, size: u64) -> Vec<u8> {
    let len = usize::try_from(size).unwrap_or(usize::MAX);
    bytes_at(seed, 0, len)
}

/// Lower-case hex SHA-256 of `expected_bytes(seed, size)`, computed in [`CHUNK_SIZE`] chunks so it
/// works for multi-GB sizes without allocating the body.
#[must_use]
pub fn expected_sha256_hex(seed: u64, size: u64) -> String {
    let mut hasher = Sha256::new();
    let mut offset = 0u64;
    while offset < size {
        let len = (size - offset).min(CHUNK_SIZE as u64) as usize;
        hasher.update(bytes_at(seed, offset, len));
        offset += len as u64;
    }
    hex::encode(hasher.finalize())
}

/// The strong `ETag` a resource reports: `"<seed as 16 hex digits><size as 16 hex digits>"`,
/// quotes included.
#[must_use]
pub fn etag(seed: u64, size: u64) -> String {
    format!("\"{seed:016x}{size:016x}\"")
}

/// The sorted, distinct bit positions (`0..size * 8`) that `/corrupt?flip=N` flips.
///
/// Positions are derived from `seed` only, so a test can predict which bits are wrong. At most
/// `size * 8` positions exist, so `flips` is clamped to that.
#[must_use]
pub fn corrupt_bit_positions(seed: u64, size: u64, flips: u64) -> Vec<u64> {
    let total_bits = size.saturating_mul(8);
    let wanted = flips.min(total_bits);
    let mut set = BTreeSet::new();
    let mut i = 0u64;
    while (set.len() as u64) < wanted {
        let candidate = splitmix64(splitmix64(seed) ^ i.wrapping_add(0xC0FF_EE00)) % total_bits;
        set.insert(candidate);
        i += 1;
    }
    set.into_iter().collect()
}

/// Everything the body stream needs to know. Crate-private: handlers build it, `body_stream`
/// consumes it.
pub(crate) struct Delivery {
    /// Resource seed.
    pub seed: u64,
    /// First byte to send (absolute offset in the resource).
    pub start: u64,
    /// One past the last byte to send.
    pub end: u64,
    /// Chunk size in bytes.
    pub chunk: usize,
    /// Throttle in bytes per second (`None` = unthrottled).
    pub bps: Option<u64>,
    /// After this many body bytes the stream fails, which makes hyper close the connection.
    pub drop_after: Option<u64>,
    /// Sorted absolute bit positions to flip.
    pub flips: Arc<[u64]>,
    /// Held while the stream is alive; released (its `Drop`) when it finishes or is dropped.
    pub _guard: Option<InFlightGuard>,
}

struct Progress {
    delivery: Delivery,
    pos: u64,
    remaining_before_drop: Option<u64>,
    done: bool,
}

/// Streams `delivery.start..delivery.end` of the resource in chunks, applying the throttle, the
/// bit flips and the drop point. Yields an `io::Error` at the drop point.
pub(crate) fn body_stream(
    delivery: Delivery,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let progress = Progress {
        pos: delivery.start,
        remaining_before_drop: delivery.drop_after,
        done: false,
        delivery,
    };
    futures::stream::unfold(progress, |mut p| async move {
        if p.done {
            return None;
        }
        // The drop point is checked before the end of the body on purpose: a drop point equal to
        // the body length (used by `/wrong-length?delta>0`) must fail the stream instead of ending
        // it, because hyper aborts an under-long body without flushing it.
        if p.remaining_before_drop == Some(0) {
            p.done = true;
            // hyper buffers ~400 KiB of body before flushing and tears the connection down as
            // soon as the body fails, so without a pause here not even the headers would reach
            // the client. Returning Pending once lets hyper flush what it buffered; the short
            // sleep lets a slow reader drain the socket before the close.
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            let err = io::Error::new(
                io::ErrorKind::ConnectionReset,
                "riptide-testserver: dropping the connection on purpose",
            );
            return Some((Err(err), p));
        }
        if p.pos >= p.delivery.end {
            return None;
        }
        let mut len = (p.delivery.end - p.pos).min(p.delivery.chunk as u64);
        if let Some(remaining) = p.remaining_before_drop {
            len = len.min(remaining);
        }
        if let Some(bps) = p.delivery.bps {
            let secs = len as f64 / bps as f64;
            tokio::time::sleep(Duration::from_secs_f64(secs)).await;
        }
        let mut buf = bytes_at(p.delivery.seed, p.pos, len as usize);
        apply_flips(&p.delivery.flips, p.pos, &mut buf);
        p.pos += len;
        if let Some(remaining) = p.remaining_before_drop.as_mut() {
            *remaining -= len;
        }
        Some((Ok(Bytes::from(buf)), p))
    })
}

/// Flips every bit position in `flips` (sorted, absolute) that falls inside the chunk that starts
/// at byte `chunk_start`.
fn apply_flips(flips: &[u64], chunk_start: u64, buf: &mut [u8]) {
    if flips.is_empty() || buf.is_empty() {
        return;
    }
    let first_bit = chunk_start * 8;
    let end_bit = first_bit + buf.len() as u64 * 8;
    let lo = flips.partition_point(|&bit| bit < first_bit);
    let hi = flips.partition_point(|&bit| bit < end_bit);
    for &bit in &flips[lo..hi] {
        let byte = ((bit - first_bit) / 8) as usize;
        let shift = (bit % 8) as u8;
        buf[byte] ^= 1 << shift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_at_is_random_access() {
        let size = 10_000u64;
        let whole = expected_bytes(DEFAULT_SEED, size);
        assert_eq!(whole.len(), 10_000);
        let cases = [
            (0u64, 1usize),
            (0, 8),
            (3, 5),
            (7, 9),
            (8, 16),
            (4_093, 2_000),
            (9_999, 1),
            (123, 0),
        ];
        for (off, len) in cases {
            let part = bytes_at(DEFAULT_SEED, off, len);
            let start = off as usize;
            assert_eq!(
                part.as_slice(),
                &whole[start..start + len],
                "off={off} len={len}"
            );
        }
        // Different seeds give different content.
        assert_ne!(
            expected_bytes(DEFAULT_SEED, 64),
            expected_bytes(DEFAULT_SEED + 1, 64)
        );
    }

    #[test]
    fn streaming_sha_matches_whole_buffer_sha() {
        let size = 3 * CHUNK_SIZE as u64 + 1234;
        let whole = expected_bytes(7, size);
        let direct = hex::encode(Sha256::digest(&whole));
        assert_eq!(expected_sha256_hex(7, size), direct);
        assert_eq!(expected_sha256_hex(7, 0), hex::encode(Sha256::digest([])));
    }

    #[test]
    fn corrupt_positions_are_distinct_sorted_and_in_range() {
        let pos = corrupt_bit_positions(DEFAULT_SEED, 1000, 10);
        assert_eq!(pos.len(), 10);
        assert!(pos.windows(2).all(|w| w[0] < w[1]));
        assert!(pos.iter().all(|&p| p < 8000));
        assert_eq!(corrupt_bit_positions(DEFAULT_SEED, 0, 5), Vec::<u64>::new());
        assert_eq!(corrupt_bit_positions(DEFAULT_SEED, 1, 100).len(), 8);
    }

    #[test]
    fn apply_flips_only_touches_bits_in_chunk() {
        let flips = [3u64, 80, 81, 1_000_000];
        let mut buf = vec![0u8; 16];
        apply_flips(&flips, 0, &mut buf);
        assert_eq!(buf[0], 0b1000);
        assert_eq!(buf[10], 0b11);
        assert!(buf
            .iter()
            .enumerate()
            .all(|(i, &b)| b == 0 || i == 0 || i == 10));
        let mut buf = vec![0u8; 4];
        apply_flips(&flips, 9, &mut buf);
        assert_eq!(buf, [0, 0b11, 0, 0]);
    }
}
