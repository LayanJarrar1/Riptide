//! A small in-memory cache for video probes (`yt-dlp -J`, 3 s or more each).
//!
//! The browser extension prefetches a page's qualities while the viewer watches, and the Add
//! dialog probes on paste; both usually ask again a moment later (popover opened, dialog
//! re-checked). Results live for [`PROBE_TTL`], at most [`PROBE_CAPACITY`] of them, in memory
//! only (nothing is written to disk, nothing survives a restart). [`ProbeCache::lock`] makes
//! concurrent probes of one page share a single yt-dlp run: the second caller waits for the
//! first, then finds its answer in the cache.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riptide_core::MediaInfo;
use tokio::sync::OwnedMutexGuard;
use url::Url;

/// How long a probe result stays valid.
pub const PROBE_TTL: Duration = Duration::from_secs(10 * 60);
/// How many results are kept.
pub const PROBE_CAPACITY: usize = 32;

/// The cache. Cheap to share behind an `Arc`.
#[derive(Debug)]
pub struct ProbeCache {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<HashMap<String, (Instant, MediaInfo)>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Default for ProbeCache {
    fn default() -> Self {
        Self::with_limits(PROBE_TTL, PROBE_CAPACITY)
    }
}

impl ProbeCache {
    /// A cache with its own lifetime and size bound.
    pub fn with_limits(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// A fresh result for `key`, if any.
    pub fn get(&self, key: &str) -> Option<MediaInfo> {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        match entries.get(key) {
            Some((at, info)) if at.elapsed() < self.ttl => Some(info.clone()),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    /// Stores a result, dropping expired entries and then the oldest ones beyond the bound.
    pub fn put(&self, key: String, info: MediaInfo) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        let ttl = self.ttl;
        entries.retain(|_, (at, _)| at.elapsed() < ttl);
        entries.insert(key, (Instant::now(), info));
        while entries.len() > self.capacity {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
    }

    /// Forgets everything (after yt-dlp was updated).
    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// Number of entries held (expired ones included until the next `put`/`get`).
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// `true` when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Waits for the turn to probe `key`. Hold the guard across the yt-dlp run, and look in
    /// the cache again right after getting it: whoever held it before may have filled it.
    pub async fn lock(&self, key: &str) -> OwnedMutexGuard<()> {
        let m = {
            let mut locks = self.locks.lock().unwrap_or_else(|p| p.into_inner());
            // forget keys nobody holds or waits for, so the map stays small
            locks.retain(|_, m| Arc::strong_count(m) > 1);
            Arc::clone(locks.entry(key.to_string()).or_default())
        };
        m.lock_owned().await
    }
}

/// The cache key for a page: the URL without its fragment (host case is folded by the
/// parser). `None` when it is not a URL.
pub fn normalize_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw.trim()).ok()?;
    url.set_fragment(None);
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn info(id: &str) -> MediaInfo {
        MediaInfo {
            url: format!("https://example.com/{id}"),
            id: id.into(),
            title: id.into(),
            uploader: None,
            extractor: "generic".into(),
            thumbnail: None,
            duration: None,
            is_live: false,
            drm: false,
            formats: vec![],
            choices: vec![],
        }
    }

    #[test]
    fn a_result_is_served_until_its_ttl_ends() {
        let c = ProbeCache::with_limits(Duration::from_millis(80), 4);
        c.put("a".into(), info("a"));
        assert_eq!(c.get("a").map(|i| i.id), Some("a".into()));
        std::thread::sleep(Duration::from_millis(120));
        assert!(c.get("a").is_none());
        assert!(c.is_empty(), "an expired entry is dropped on lookup");
    }

    #[test]
    fn the_oldest_entries_go_first_beyond_the_bound() {
        let c = ProbeCache::with_limits(Duration::from_secs(60), 2);
        for k in ["a", "b", "c"] {
            c.put(k.into(), info(k));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.len(), 2);
        assert!(c.get("a").is_none());
        assert!(c.get("b").is_some() && c.get("c").is_some());
    }

    #[test]
    fn urls_differing_only_in_fragment_share_a_key() {
        assert_eq!(
            normalize_url("https://WWW.youtube.com/watch?v=x#t=5"),
            normalize_url("https://www.youtube.com/watch?v=x")
        );
        assert_ne!(
            normalize_url("https://www.youtube.com/watch?v=x"),
            normalize_url("https://www.youtube.com/watch?v=y")
        );
        assert!(normalize_url("not a url").is_none());
    }

    #[tokio::test]
    async fn concurrent_probes_of_one_page_run_once() {
        let c = Arc::new(ProbeCache::default());
        let runs = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let (c, runs) = (Arc::clone(&c), Arc::clone(&runs));
            tasks.push(tokio::spawn(async move {
                if let Some(i) = c.get("k") {
                    return i;
                }
                let _turn = c.lock("k").await;
                if let Some(i) = c.get("k") {
                    return i;
                }
                runs.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                let i = info("k");
                c.put("k".into(), i.clone());
                i
            }));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap().id, "k");
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }
}
