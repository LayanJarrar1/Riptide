//! Per-host connection cap shared by every transfer in the process.
//!
//! The cap starts at the configured value and can only be *lowered* while the process runs: a
//! transfer that sees `429` / `503` calls [`HostLimiter::report_throttled`], which halves the
//! number of connections that were in use on that host (at most once per [`COOLDOWN`]). The
//! engine persists the new cap and feeds it back with [`HostLimiter::with_caps`] after a
//! restart, so a server that only tolerates two connections is not hammered with eight every
//! time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Minimum time between two reductions of the same host's cap, so a burst of simultaneous
/// `429`s (one per connection) counts as one signal.
pub const COOLDOWN: Duration = Duration::from_secs(1);

/// Called with `(host, new_cap)` whenever a cap is lowered.
pub type CapObserver = Arc<dyn Fn(&str, usize) + Send + Sync>;

#[derive(Debug)]
struct Slot {
    sem: Arc<Semaphore>,
    /// Current cap.
    cap: AtomicUsize,
    /// Permits that must be destroyed instead of released because they were in use when the cap
    /// dropped.
    debt: AtomicUsize,
    last_cut: Mutex<Option<Instant>>,
}

/// A connection slot on one host; dropping it frees the slot (or retires it if the cap was
/// lowered while it was held).
#[derive(Debug)]
pub struct HostPermit {
    permit: Option<OwnedSemaphorePermit>,
    slot: Arc<Slot>,
}

impl Drop for HostPermit {
    fn drop(&mut self) {
        let mut owes = false;
        let mut debt = self.slot.debt.load(Ordering::SeqCst);
        while debt > 0 {
            match self.slot.debt.compare_exchange(
                debt,
                debt - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    owes = true;
                    break;
                }
                Err(now) => debt = now,
            }
        }
        if let Some(p) = self.permit.take() {
            if owes {
                p.forget();
            }
        }
    }
}

/// Limits simultaneous connections per host so one download (or several to the same CDN)
/// never looks like an attack. One instance per engine, shared by all transfers.
pub struct HostLimiter {
    per_host: usize,
    initial: HashMap<String, usize>,
    hosts: Mutex<HashMap<String, Arc<Slot>>>,
    observer: Option<CapObserver>,
}

impl std::fmt::Debug for HostLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostLimiter")
            .field("per_host", &self.per_host)
            .finish_non_exhaustive()
    }
}

impl HostLimiter {
    /// `per_host` simultaneous connections to any one host (0 is treated as 1).
    pub fn new(per_host: usize) -> Arc<Self> {
        Self::with_caps(per_host, HashMap::new(), None)
    }

    /// Like [`HostLimiter::new`] with learned per-host caps (each clamped to `1..=per_host`)
    /// and an observer told about every later reduction.
    pub fn with_caps(
        per_host: usize,
        learned: HashMap<String, usize>,
        observer: Option<CapObserver>,
    ) -> Arc<Self> {
        let per_host = per_host.max(1);
        Arc::new(HostLimiter {
            per_host,
            initial: learned
                .into_iter()
                .map(|(h, c)| (h.to_ascii_lowercase(), c.clamp(1, per_host)))
                .collect(),
            hosts: Mutex::new(HashMap::new()),
            observer,
        })
    }

    fn slot(&self, host: &str) -> Arc<Slot> {
        let host = host.to_ascii_lowercase();
        let mut map = self.hosts.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(map.entry(host.clone()).or_insert_with(|| {
            let cap = self.initial.get(&host).copied().unwrap_or(self.per_host);
            Arc::new(Slot {
                sem: Arc::new(Semaphore::new(cap)),
                cap: AtomicUsize::new(cap),
                debt: AtomicUsize::new(0),
                last_cut: Mutex::new(None),
            })
        }))
    }

    /// Waits for a free slot on `host`. Dropping the permit frees it.
    pub async fn acquire(&self, host: &str) -> HostPermit {
        let slot = self.slot(host);
        // the semaphore is never closed, so acquire cannot fail
        let permit = match Arc::clone(&slot.sem).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => std::future::pending().await,
        };
        HostPermit {
            permit: Some(permit),
            slot,
        }
    }

    /// The configured cap.
    pub fn per_host(&self) -> usize {
        self.per_host
    }

    /// The cap in force for `host` right now (configured or learned).
    pub fn cap_for(&self, host: &str) -> usize {
        self.slot(host).cap.load(Ordering::SeqCst)
    }

    /// Lowers `host`'s cap to `cap` (at least 1). Never raises it. Returns `true` if the cap
    /// changed. Connections already open finish normally; the surplus slots disappear as they
    /// are released.
    pub fn lower_cap(&self, host: &str, cap: usize) -> bool {
        let slot = self.slot(host);
        let cap = cap.max(1);
        let current = slot.cap.load(Ordering::SeqCst);
        if cap >= current {
            return false;
        }
        slot.cap.store(cap, Ordering::SeqCst);
        let surplus = current - cap;
        let forgotten = slot.sem.forget_permits(surplus);
        slot.debt.fetch_add(surplus - forgotten, Ordering::SeqCst);
        if let Some(o) = &self.observer {
            o(&host.to_ascii_lowercase(), cap);
        }
        true
    }

    /// The server answered `429` / `503` to a connection on `host`: halve the number of
    /// connections that were in use (at most once per [`COOLDOWN`], never below 1). Returns the
    /// new cap if it was lowered.
    pub fn report_throttled(&self, host: &str) -> Option<usize> {
        let slot = self.slot(host);
        {
            let mut last = slot.last_cut.lock().unwrap_or_else(|p| p.into_inner());
            if last.is_some_and(|t| t.elapsed() < COOLDOWN) {
                return None;
            }
            *last = Some(Instant::now());
        }
        let cap = slot.cap.load(Ordering::SeqCst);
        // the throttled connection has already released its slot, so count it back in
        let in_use = (cap.saturating_sub(slot.sem.available_permits()) + 1).min(cap);
        let next = (in_use / 2).max(1);
        self.lower_cap(host, next).then_some(next)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn caps_connections_per_host_independently() {
        let l = HostLimiter::new(2);
        let a1 = l.acquire("CDN.example").await;
        let _a2 = l.acquire("cdn.example").await;
        let _b1 = l.acquire("other.example").await;
        let third =
            tokio::time::timeout(Duration::from_millis(100), l.acquire("cdn.example")).await;
        assert!(
            third.is_err(),
            "third connection to the same host must wait"
        );
        drop(a1);
        let third =
            tokio::time::timeout(Duration::from_millis(500), l.acquire("cdn.example")).await;
        assert!(third.is_ok(), "freed slot is reused");
        assert_eq!(HostLimiter::new(0).per_host(), 1);
    }

    #[tokio::test]
    async fn lowering_the_cap_retires_slots_that_are_in_use() {
        let l = HostLimiter::new(4);
        let held = vec![
            l.acquire("h").await,
            l.acquire("h").await,
            l.acquire("h").await,
        ];
        assert!(l.lower_cap("h", 1));
        assert!(!l.lower_cap("h", 3), "never raised");
        assert_eq!(l.cap_for("h"), 1);
        drop(held);
        // exactly one slot is left: one acquire works, the second waits
        let a = l.acquire("h").await;
        let second = tokio::time::timeout(Duration::from_millis(100), l.acquire("h")).await;
        assert!(second.is_err(), "cap 1 holds after the surplus is retired");
        drop(a);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), l.acquire("h"))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn throttle_reports_halve_with_a_cooldown_and_notify() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let observer: CapObserver =
            Arc::new(move |h, c| sink.lock().unwrap().push((h.to_string(), c)));
        let l = HostLimiter::with_caps(8, HashMap::new(), Some(observer));
        let mut held = Vec::new();
        for _ in 0..7 {
            held.push(l.acquire("Host").await);
        }
        assert_eq!(l.report_throttled("host"), Some(4));
        assert_eq!(l.report_throttled("host"), None, "cooldown");
        assert_eq!(l.cap_for("host"), 4);
        assert_eq!(seen.lock().unwrap().as_slice(), [("host".to_string(), 4)]);
    }

    #[tokio::test]
    async fn learned_caps_are_applied_and_clamped() {
        let mut learned = HashMap::new();
        learned.insert("Slow.example".to_string(), 2);
        learned.insert("odd.example".to_string(), 99);
        let l = HostLimiter::with_caps(8, learned, None);
        assert_eq!(l.cap_for("slow.example"), 2);
        assert_eq!(l.cap_for("odd.example"), 8);
        assert_eq!(l.cap_for("other.example"), 8);
    }
}
