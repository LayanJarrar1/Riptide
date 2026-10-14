//! Speed (exponential moving average over one-second samples) and ETA.

use std::time::{Duration, Instant};

/// Smooths the byte counter into a readable bytes-per-second figure.
#[derive(Debug, Clone)]
pub struct SpeedMeter {
    alpha: f64,
    ema_bps: f64,
    last_bytes: u64,
    last_at: Instant,
    primed: bool,
}

impl SpeedMeter {
    /// `alpha` is the weight of the newest sample (0.3 reacts in ~3 s, 0.1 in ~10 s).
    pub fn new(now: Instant, start_bytes: u64, alpha: f64) -> Self {
        SpeedMeter {
            alpha: alpha.clamp(0.01, 1.0),
            ema_bps: 0.0,
            last_bytes: start_bytes,
            last_at: now,
            primed: false,
        }
    }

    /// Default smoothing for the UI (reacts within a few seconds, does not flicker).
    pub fn for_ui(now: Instant, start_bytes: u64) -> Self {
        Self::new(now, start_bytes, 0.3)
    }

    /// Feeds the current total; returns the smoothed speed. Samples closer than 100 ms apart are
    /// ignored (returns the previous value) to keep the estimate stable.
    pub fn sample(&mut self, total_bytes: u64, now: Instant) -> u64 {
        let dt = now.saturating_duration_since(self.last_at).as_secs_f64();
        if dt < 0.1 {
            return self.bps();
        }
        let delta = total_bytes.saturating_sub(self.last_bytes) as f64;
        let instant = delta / dt;
        if self.primed {
            self.ema_bps += self.alpha * (instant - self.ema_bps);
        } else {
            self.ema_bps = instant;
            self.primed = true;
        }
        self.last_bytes = total_bytes;
        self.last_at = now;
        self.bps()
    }

    /// Current smoothed speed in bytes per second.
    pub fn bps(&self) -> u64 {
        self.ema_bps.max(0.0).round() as u64
    }

    /// Seconds until `remaining` bytes arrive at the current speed; `None` when idle.
    pub fn eta(&self, remaining: u64) -> Option<u64> {
        if self.ema_bps < 1.0 {
            return None;
        }
        Some((remaining as f64 / self.ema_bps).ceil() as u64)
    }

    /// Resets the counter (after a pause, so the gap is not counted as zero speed).
    pub fn reset(&mut self, now: Instant, total_bytes: u64) {
        self.last_at = now;
        self.last_bytes = total_bytes;
        self.ema_bps = 0.0;
        self.primed = false;
    }
}

/// Seconds since `Duration`, for the ETA field on events.
pub fn secs(d: Duration) -> u64 {
    d.as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_rate_converges_and_eta_follows() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::for_ui(t0, 0);
        for i in 1..=10u64 {
            m.sample(i * 1_000_000, t0 + Duration::from_secs(i));
        }
        let bps = m.bps();
        assert!((950_000..=1_050_000).contains(&bps), "{bps}");
        assert_eq!(m.eta(10_000_000), Some(10));
    }

    #[test]
    fn smooths_bursts_and_ignores_too_close_samples() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::for_ui(t0, 0);
        m.sample(1_000_000, t0 + Duration::from_secs(1));
        let before = m.bps();
        assert_eq!(
            m.sample(9_000_000, t0 + Duration::from_millis(1050)),
            before,
            "sub-100 ms sample ignored"
        );
        m.sample(9_000_000, t0 + Duration::from_secs(2));
        assert!(
            m.bps() > before && m.bps() < 8_000_000,
            "ema moves toward the burst without jumping: {}",
            m.bps()
        );
        assert_eq!(SpeedMeter::for_ui(t0, 0).eta(5), None);
    }
}
