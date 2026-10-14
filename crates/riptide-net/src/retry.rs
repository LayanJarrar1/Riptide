//! Exponential backoff with jitter, honouring `Retry-After`.

use std::time::Duration;

/// When and how often to retry a transient failure.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Attempts before giving up (the first try counts).
    pub max_attempts: u32,
    /// Delay before the first retry.
    pub base: Duration,
    /// Multiplier per attempt.
    pub factor: f64,
    /// Longest delay.
    pub cap: Duration,
    /// ± fraction of the delay randomised (0.25 = ±25 %), so many segments do not retry in
    /// lockstep against a struggling server.
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 10,
            base: Duration::from_millis(500),
            factor: 2.0,
            cap: Duration::from_secs(30),
            jitter: 0.25,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries (tests, `--no-retry`).
    pub fn none() -> Self {
        RetryPolicy {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// Whether another attempt is allowed after `attempt` failures.
    pub fn allows(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }

    /// Delay before retry number `attempt` (1 = first retry). A `Retry-After` from the server
    /// wins over the schedule (capped at ten times `cap`, so a hostile value cannot park us).
    /// `unit` is a random number in `[0, 1)` supplied by the caller for the jitter.
    pub fn delay_with(&self, attempt: u32, retry_after_secs: Option<u64>, unit: f64) -> Duration {
        if let Some(secs) = retry_after_secs {
            return Duration::from_secs(secs).min(self.cap * 10);
        }
        let exp = self.factor.powi(attempt.saturating_sub(1) as i32);
        let raw = self.base.as_secs_f64() * exp;
        let capped = raw.min(self.cap.as_secs_f64());
        let jitter = 1.0 + (unit * 2.0 - 1.0) * self.jitter;
        Duration::from_secs_f64((capped * jitter).max(0.0))
    }

    /// [`Self::delay_with`] with a random jitter.
    pub fn delay(&self, attempt: u32, retry_after_secs: Option<u64>) -> Duration {
        use rand::Rng;
        self.delay_with(attempt, retry_after_secs, rand::rng().random::<f64>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_exponentially_and_caps() {
        let p = RetryPolicy {
            jitter: 0.0,
            ..RetryPolicy::default()
        };
        assert_eq!(p.delay_with(1, None, 0.5), Duration::from_millis(500));
        assert_eq!(p.delay_with(2, None, 0.5), Duration::from_millis(1000));
        assert_eq!(p.delay_with(3, None, 0.5), Duration::from_millis(2000));
        assert_eq!(p.delay_with(20, None, 0.5), Duration::from_secs(30));
    }

    #[test]
    fn jitter_stays_within_bounds_and_retry_after_wins() {
        let p = RetryPolicy::default();
        let lo = p.delay_with(2, None, 0.0);
        let hi = p.delay_with(2, None, 0.999_999);
        assert_eq!(lo, Duration::from_millis(750));
        assert!(hi > Duration::from_millis(1249) && hi <= Duration::from_millis(1250));
        assert_eq!(p.delay_with(5, Some(7), 0.1), Duration::from_secs(7));
        assert_eq!(
            p.delay_with(1, Some(100_000), 0.1),
            Duration::from_secs(300),
            "hostile Retry-After is capped"
        );
        assert!(p.allows(9));
        assert!(!p.allows(10));
        assert!(!RetryPolicy::none().allows(1));
    }
}
