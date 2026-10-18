//! Per-host politeness: at most N requests in flight to a host, and a minimum gap between the
//! starts of two requests (the larger of the configured delay and the host's `Crawl-delay`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

/// The gate of one host.
#[derive(Debug)]
pub struct HostGate {
    slots: Arc<Semaphore>,
    state: Mutex<GateState>,
}

#[derive(Debug)]
struct GateState {
    delay: Duration,
    next_start: Instant,
}

impl HostGate {
    /// A gate admitting `max_in_flight` (at least 1) requests at once, `delay` apart.
    #[must_use]
    pub fn new(max_in_flight: usize, delay: Duration) -> HostGate {
        HostGate {
            slots: Arc::new(Semaphore::new(max_in_flight.max(1))),
            state: Mutex::new(GateState {
                delay,
                next_start: Instant::now(),
            }),
        }
    }

    /// Raises the minimum gap to `delay` if that is larger than the current one (a
    /// `Crawl-delay` learnt from `robots.txt`).
    pub fn raise_delay(&self, delay: Duration) {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.delay = st.delay.max(delay);
    }

    /// The current minimum gap.
    #[must_use]
    pub fn delay(&self) -> Duration {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).delay
    }

    /// Waits for a free slot and for this request's start time. Hold the permit until the
    /// response has been consumed. Cancel-safe: dropping the future gives the slot back (the
    /// reserved start time is not, which only makes the crawl slightly politer).
    pub async fn enter(&self) -> OwnedSemaphorePermit {
        // the semaphore is never closed, so acquiring cannot fail
        let permit = Arc::clone(&self.slots)
            .acquire_owned()
            .await
            .expect("the gate's semaphore is never closed");
        let start = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let start = st.next_start.max(Instant::now());
            st.next_start = start + st.delay;
            start
        };
        tokio::time::sleep_until(start).await;
        permit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn requests_are_spaced_by_the_delay() {
        let gate = Arc::new(HostGate::new(8, Duration::from_millis(500)));
        let t0 = Instant::now();
        let mut starts = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let g = Arc::clone(&gate);
            tasks.push(tokio::spawn(async move {
                let _p = g.enter().await;
                Instant::now()
            }));
        }
        for t in tasks {
            starts.push(t.await.unwrap() - t0);
        }
        starts.sort();
        for (i, s) in starts.iter().enumerate() {
            assert_eq!(*s, Duration::from_millis(500) * i as u32, "{starts:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn in_flight_is_capped() {
        let gate = Arc::new(HostGate::new(2, Duration::ZERO));
        let a = gate.enter().await;
        let _b = gate.enter().await;
        let third = tokio::spawn({
            let g = Arc::clone(&gate);
            async move {
                let _p = g.enter().await;
            }
        });
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(!third.is_finished(), "a third request must wait");
        drop(a);
        third.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn crawl_delay_only_ever_raises_the_gap() {
        let gate = HostGate::new(2, Duration::from_secs(1));
        gate.raise_delay(Duration::from_millis(10));
        assert_eq!(gate.delay(), Duration::from_secs(1));
        gate.raise_delay(Duration::from_secs(5));
        assert_eq!(gate.delay(), Duration::from_secs(5));
    }
}
