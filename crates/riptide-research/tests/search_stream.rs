//! Sources run concurrently and each one's hits are reported the moment they arrive.
#![allow(clippy::unwrap_used, missing_docs)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use riptide_research::{
    search_stream, Category, Dedup, Hit, ResearchError, SearchEvent, SearchSource, Source,
};

struct Fake {
    source: Source,
    delay_ms: u64,
    result: Result<usize, ResearchError>,
}

#[async_trait]
impl SearchSource for Fake {
    fn source(&self) -> Source {
        self.source
    }
    async fn search_in(
        &self,
        category: Category,
        query: &str,
        _limit: usize,
    ) -> Result<Vec<Hit>, ResearchError> {
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        match &self.result {
            Ok(n) => Ok((0..*n)
                .map(|i| {
                    Hit::new(
                        self.source,
                        &format!("{query}-{i}-{}", category.id()),
                        &format!("{} result {i} for {query}", self.source.label()),
                        &format!("https://example.org/{}/{query}/{i}.bin", self.source.id()),
                    )
                })
                .collect()),
            Err(e) => Err(e.clone()),
        }
    }
}

fn fake(
    source: Source,
    delay_ms: u64,
    result: Result<usize, ResearchError>,
) -> Arc<dyn SearchSource> {
    Arc::new(Fake {
        source,
        delay_ms,
        result,
    })
}

#[tokio::test]
async fn fast_sources_report_before_slow_ones_and_failures_do_not_stop_others() {
    let adapters = vec![
        fake(Source::Arxiv, 400, Ok(2)),
        fake(Source::Gutenberg, 20, Ok(3)),
        fake(Source::Nasa, 60, Err(ResearchError::Http { status: 503 })),
        fake(Source::Commons, 5_000, Ok(1)),
    ];
    let start = Instant::now();
    let mut log: Vec<(Duration, String)> = Vec::new();
    let dedup = std::sync::Mutex::new(Dedup::new());
    search_stream(
        &adapters,
        &["frogs".to_string()],
        5,
        Category::All,
        Duration::from_millis(900),
        &dedup,
        |ev| {
            let t = start.elapsed();
            let s = match ev {
                SearchEvent::Started(s) => format!("start {}", s.id()),
                SearchEvent::Hits { source, hits } => {
                    format!("hits {} {}", source.id(), hits.len())
                }
                SearchEvent::Finished { source, found } => format!("done {} {found}", source.id()),
                SearchEvent::Failed(f) => format!("failed {} {}", f.source.id(), f.code),
            };
            log.push((t, s));
        },
    )
    .await;
    let at = |needle: &str| {
        log.iter()
            .find(|(_, s)| s == needle)
            .unwrap_or_else(|| panic!("{needle} missing in {log:?}"))
            .0
    };
    // everything starts at once
    assert!(at("start commons") < Duration::from_millis(20));
    // the fast source is on screen long before the slow one
    assert!(at("hits gutenberg 3") < Duration::from_millis(200));
    assert!(at("hits arxiv 2") > Duration::from_millis(350));
    assert!(at("hits gutenberg 3") < at("hits arxiv 2"));
    assert!(at("done gutenberg 3") >= at("hits gutenberg 3"));
    // a failing source is reported as failed, the rest carry on
    assert!(log.iter().any(|(_, s)| s == "failed nasa http"));
    // the slow source hit the per-source deadline
    assert!(
        log.iter().any(|(_, s)| s == "failed commons timeout"),
        "{log:?}"
    );
    assert!(start.elapsed() < Duration::from_millis(2500));
}

#[tokio::test]
async fn the_same_item_is_reported_once_across_rounds() {
    let adapters = vec![fake(Source::Arxiv, 1, Ok(2))];
    let dedup = std::sync::Mutex::new(Dedup::new());
    let mut n = 0;
    for _ in 0..2 {
        search_stream(
            &adapters,
            &["same".to_string()],
            5,
            Category::Papers,
            Duration::from_secs(1),
            &dedup,
            |ev| {
                if let SearchEvent::Hits { hits, .. } = ev {
                    n += hits.len();
                }
            },
        )
        .await;
    }
    assert_eq!(n, 2);
}
