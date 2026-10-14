//! Rate limiter for progress events (≤ 10 per second per download).

use std::time::{Duration, Instant};

/// Allows at most one event per `min_interval`. The caller supplies `now` so the logic is
/// deterministic in tests.
#[derive(Debug, Clone)]
pub struct Throttle {
    min_interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    /// Interval used for progress events: 100 ms, i.e. ten per second.
    pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

    /// A throttle that lets one event through every `min_interval`.
    pub const fn new(min_interval: Duration) -> Self {
        Throttle {
            min_interval,
            last: None,
        }
    }

    /// A throttle tuned for progress events.
    pub const fn for_progress() -> Self {
        Self::new(Self::PROGRESS_INTERVAL)
    }

    /// `true` if an event may be emitted at `now`; records the emission when it returns `true`.
    /// The first call always passes.
    pub fn allow(&mut self, now: Instant) -> bool {
        match self.last {
            Some(last) if now.duration_since(last) < self.min_interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }

    /// Forces the next [`Self::allow`] to pass (used right before a state change so the final
    /// progress value is never lost).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_one_per_interval() {
        let t0 = Instant::now();
        let mut t = Throttle::new(Duration::from_millis(100));
        assert!(t.allow(t0));
        assert!(!t.allow(t0 + Duration::from_millis(50)));
        assert!(!t.allow(t0 + Duration::from_millis(99)));
        assert!(t.allow(t0 + Duration::from_millis(100)));
        assert!(!t.allow(t0 + Duration::from_millis(150)));
        t.reset();
        assert!(t.allow(t0 + Duration::from_millis(150)));
    }

    #[test]
    fn ten_per_second() {
        let t0 = Instant::now();
        let mut t = Throttle::for_progress();
        let passed = (0..1000)
            .filter(|ms| t.allow(t0 + Duration::from_millis(*ms)))
            .count();
        assert_eq!(passed, 10);
    }
}
