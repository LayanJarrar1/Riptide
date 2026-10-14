//! Mutable state shared by every request: per-path counters, the deterministic "flaky" PRNG,
//! the in-flight counter behind `/limit` and the origin strings behind `/redirect?cross=1` and
//! `/needs-referer`.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::content::splitmix64;

/// State handed to the router of one listener.
///
/// Two listeners (the base origin and the "other" origin used by cross-origin redirects) share the
/// same counters through [`ServerState::sibling`], and each knows its own origin and the other's.
pub struct ServerState {
    shared: Arc<Shared>,
    origin: String,
    alt_origin: String,
}

impl ServerState {
    /// Creates fresh state. `origin` is the URL this router is reachable at (for example
    /// `http://127.0.0.1:8080`), `alt_origin` the other listener's.
    pub fn new(seed: u64, origin: impl Into<String>, alt_origin: impl Into<String>) -> Self {
        Self {
            shared: Arc::new(Shared::new(seed)),
            origin: origin.into(),
            alt_origin: alt_origin.into(),
        }
    }

    /// State for the other listener: same counters and PRNG, swapped origins.
    pub fn sibling(&self, origin: impl Into<String>, alt_origin: impl Into<String>) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            origin: origin.into(),
            alt_origin: alt_origin.into(),
        }
    }

    /// The origin this router serves (`http://host:port`, no trailing slash).
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The other listener's origin (`http://host:port`, no trailing slash).
    #[must_use]
    pub fn alt_origin(&self) -> &str {
        &self.alt_origin
    }

    /// The seed the server-level PRNG (`/flaky`) and the digest nonce were derived from.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.shared.seed
    }

    /// Number of `/limit` bodies currently being streamed.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.shared.limit_in_flight.load(Ordering::SeqCst)
    }

    pub(crate) fn shared(&self) -> &Shared {
        &self.shared
    }
}

/// Counters shared by both origins.
pub(crate) struct Shared {
    seed: u64,
    rng: Mutex<u64>,
    /// Requests seen by `/retry-after`.
    pub(crate) retry_after_hits: AtomicU64,
    /// Requests seen by `/etag-changes`.
    pub(crate) etag_changes_hits: AtomicU64,
    limit_in_flight: Arc<AtomicUsize>,
    /// When `/expires` was first requested.
    pub(crate) expires_first_hit: OnceLock<Instant>,
    /// Nonce offered by `/needs-auth/digest`.
    pub(crate) digest_nonce: String,
    /// Opaque value offered by `/needs-auth/digest`.
    pub(crate) digest_opaque: String,
}

impl Shared {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            rng: Mutex::new(splitmix64(seed)),
            retry_after_hits: AtomicU64::new(0),
            etag_changes_hits: AtomicU64::new(0),
            limit_in_flight: Arc::new(AtomicUsize::new(0)),
            expires_first_hit: OnceLock::new(),
            digest_nonce: format!("{:016x}", splitmix64(seed ^ 0x006E_6F6E_6365)),
            digest_opaque: format!("{:016x}", splitmix64(seed ^ 0x6F70_6171_7565)),
        }
    }

    /// Next value of the server-level PRNG (deterministic per seed and request order).
    pub(crate) fn next_random(&self) -> u64 {
        let mut guard = self
            .rng
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = splitmix64(*guard);
        *guard
    }

    /// Next PRNG value scaled to `[0, 1)`.
    pub(crate) fn next_unit(&self) -> f64 {
        (self.next_random() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Registers one more in-flight `/limit` body unless `limit` are already running.
    pub(crate) fn try_acquire(&self, limit: usize) -> Option<InFlightGuard> {
        let previous = self.limit_in_flight.fetch_add(1, Ordering::SeqCst);
        if previous < limit {
            Some(InFlightGuard(Arc::clone(&self.limit_in_flight)))
        } else {
            self.limit_in_flight.fetch_sub(1, Ordering::SeqCst);
            None
        }
    }
}

/// Decrements the in-flight counter when dropped (i.e. when the body stream ends or is dropped).
pub(crate) struct InFlightGuard(Arc<AtomicUsize>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
