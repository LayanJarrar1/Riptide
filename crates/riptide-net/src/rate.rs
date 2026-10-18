//! Bandwidth limiting: a token bucket whose rate can change while transfers run.
//!
//! One [`RateLimiter`] is shared by every connection of a download (the per-download limit), and
//! one by every download (the global limit); a connection asks each bucket in its
//! [`TransferConfig::rate_limits`](crate::TransferConfig::rate_limits) before it hands a chunk
//! to the writer. Holding back the reader is enough: TCP flow control then slows the sender.
//!
//! The bucket lets a chunk go through as soon as it is not in debt and charges the full chunk,
//! possibly going negative; later callers wait until the debt is repaid. Waits are cut into
//! slices of at most [`MAX_WAIT_SLICE`] so a new rate (or "unlimited") applies within that time.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest single sleep before the bucket re-reads its rate.
pub const MAX_WAIT_SLICE: Duration = Duration::from_millis(200);

/// Largest burst, as time worth of tokens at the current rate.
const BURST: Duration = Duration::from_millis(200);

#[derive(Debug)]
struct BucketState {
    /// Available bytes; negative while in debt.
    tokens: f64,
    /// When `tokens` was last refilled.
    last: Instant,
    /// The rate the state was computed for (a change resets the bucket).
    rate: u64,
}

/// A token bucket in bytes per second; a rate of `0` means unlimited.
#[derive(Debug)]
pub struct RateLimiter {
    rate: AtomicU64,
    state: Mutex<BucketState>,
}

impl RateLimiter {
    /// A bucket limited to `bps` bytes per second (`None` or `Some(0)` = unlimited).
    pub fn new(bps: Option<u64>) -> Self {
        let rate = bps.unwrap_or(0);
        RateLimiter {
            rate: AtomicU64::new(rate),
            state: Mutex::new(BucketState {
                tokens: 0.0,
                last: Instant::now(),
                rate,
            }),
        }
    }

    /// Changes the rate; waiting and future callers see it within [`MAX_WAIT_SLICE`].
    pub fn set_limit(&self, bps: Option<u64>) {
        self.rate.store(bps.unwrap_or(0), Ordering::Relaxed);
    }

    /// The current limit, `None` when unlimited.
    pub fn limit(&self) -> Option<u64> {
        match self.rate.load(Ordering::Relaxed) {
            0 => None,
            r => Some(r),
        }
    }

    /// Takes `bytes` tokens, or says how long to wait before asking again.
    fn try_take(&self, bytes: u64, now: Instant) -> Option<Duration> {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return None;
        }
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.rate != rate {
            // a new limit starts from an empty bucket: no burst carried over, no old debt
            s.rate = rate;
            s.tokens = 0.0;
            s.last = now;
        }
        let elapsed = now.saturating_duration_since(s.last).as_secs_f64();
        s.last = now;
        let burst = rate as f64 * BURST.as_secs_f64();
        s.tokens = (s.tokens + elapsed * rate as f64).min(burst);
        if s.tokens >= 0.0 {
            s.tokens -= bytes as f64;
            return None;
        }
        let wait = Duration::from_secs_f64(-s.tokens / rate as f64);
        Some(wait.clamp(Duration::from_millis(1), MAX_WAIT_SLICE))
    }

    /// Waits until `bytes` may pass. Returns at once when unlimited.
    pub async fn acquire(&self, bytes: u64) {
        while let Some(wait) = self.try_take(bytes, Instant::now()) {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_never_waits() {
        let r = RateLimiter::new(None);
        let now = Instant::now();
        for _ in 0..1000 {
            assert_eq!(r.try_take(1 << 20, now), None);
        }
        assert_eq!(r.limit(), None);
        assert_eq!(RateLimiter::new(Some(0)).limit(), None);
    }

    #[test]
    fn debt_is_repaid_at_the_rate() {
        let r = RateLimiter::new(Some(1000));
        let t0 = Instant::now();
        assert_eq!(r.try_take(500, t0), None, "first chunk goes into debt");
        let wait = r.try_take(500, t0).unwrap();
        assert!(wait <= MAX_WAIT_SLICE);
        // 0.5 s later the debt of 500 bytes is repaid
        assert_eq!(r.try_take(500, t0 + Duration::from_millis(500)), None);
    }

    #[test]
    fn rate_change_applies_immediately() {
        let r = RateLimiter::new(Some(10));
        let t0 = Instant::now();
        assert_eq!(r.try_take(10_000, t0), None);
        assert!(r.try_take(1, t0).is_some(), "deep in debt at 10 B/s");
        r.set_limit(Some(1_000_000));
        assert_eq!(r.try_take(1, t0), None, "new rate resets the debt");
        r.set_limit(None);
        assert_eq!(r.try_take(u64::MAX / 2, t0), None);
        assert_eq!(r.limit(), None);
    }

    #[tokio::test]
    async fn acquire_paces_the_stream() {
        let r = RateLimiter::new(Some(200_000));
        let start = Instant::now();
        for _ in 0..20 {
            r.acquire(10_000).await;
        }
        // 200 kB at 200 kB/s, the first chunk free: ~0.95 s
        let secs = start.elapsed().as_secs_f64();
        assert!((0.8..1.3).contains(&secs), "{secs}");
    }
}
