//! Per-host connection cap shared by every transfer in the process.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Limits simultaneous connections per host so one download (or several to the same CDN)
/// never looks like an attack. One instance per engine, shared by all transfers.
#[derive(Debug)]
pub struct HostLimiter {
    per_host: usize,
    hosts: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl HostLimiter {
    /// `per_host` simultaneous connections to any one host (0 is treated as 1).
    pub fn new(per_host: usize) -> Arc<Self> {
        Arc::new(HostLimiter {
            per_host: per_host.max(1),
            hosts: Mutex::new(HashMap::new()),
        })
    }

    /// Waits for a free slot on `host`. Dropping the permit frees it.
    pub async fn acquire(&self, host: &str) -> OwnedSemaphorePermit {
        let semaphore = {
            let mut map = self.hosts.lock().unwrap_or_else(|p| p.into_inner());
            Arc::clone(
                map.entry(host.to_ascii_lowercase())
                    .or_insert_with(|| Arc::new(Semaphore::new(self.per_host))),
            )
        };
        // the semaphore is never closed, so acquire cannot fail
        match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => std::future::pending().await,
        }
    }

    /// The configured cap.
    pub fn per_host(&self) -> usize {
        self.per_host
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
}
