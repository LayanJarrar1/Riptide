//! Runs several adapters (and several query phrasings) concurrently and merges the answers:
//! repeated items collapse into one, sources are interleaved so no single source floods the
//! list, and a failing source is reported next to the results instead of failing the search.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;

use crate::error::ResearchError;
use crate::sources::SearchSource;
use crate::types::{Category, Hit, Source};

/// Longest a single source may take for all its queries together.
pub const SOURCE_DEADLINE: Duration = Duration::from_secs(45);

/// A source that could not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFailure {
    /// Which source.
    pub source: Source,
    /// Stable error code (`timeout`, `network`, `rate_limited`, `http`, `parse`, `invalid`).
    pub code: String,
    /// English description for logs and tooltips.
    pub message: String,
}

impl SourceFailure {
    fn of(source: Source, e: &ResearchError) -> Self {
        SourceFailure {
            source,
            code: e.code().into(),
            message: e.to_string(),
        }
    }
}

/// Everything a search produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOutcome {
    /// Merged, de-duplicated, interleaved hits.
    pub hits: Vec<Hit>,
    /// Sources that failed (the others still answered).
    pub failures: Vec<SourceFailure>,
    /// The query phrasings that were run.
    pub queries: Vec<String>,
}

/// What a running search reports, source by source, as the answers arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEvent {
    /// The source started working.
    Started(Source),
    /// New, not-yet-seen hits from a source (after de-duplication against everything so far).
    Hits {
        /// Which source.
        source: Source,
        /// The new hits, in the source's own order.
        hits: Vec<Hit>,
    },
    /// The source answered all its queries.
    Finished {
        /// Which source.
        source: Source,
        /// How many hits it contributed in total (after de-duplication).
        found: usize,
    },
    /// The source failed or timed out; the others carry on.
    Failed(SourceFailure),
}

/// Remembers what was already shown so a repeated item is reported once. Share one across
/// the rounds of a search (and a retry of a single source).
#[derive(Debug, Default)]
pub struct Dedup {
    seen: HashSet<String>,
}

impl Dedup {
    /// An empty memory.
    pub fn new() -> Self {
        Self::default()
    }

    /// True the first time an item (or the same paper under another id) is offered.
    pub fn admit(&mut self, hit: &Hit) -> bool {
        let mut keys = riptide_core::dedup::keys(&hit.identity());
        // a file name + size match is a weak key; never merge on it here
        keys.retain(|k| !k.starts_with("file:"));
        keys.push(format!("id:{}", hit.id));
        // the same paper often arrives twice with different DOIs (preprint and journal)
        let title: String = hit
            .title
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if title.chars().count() >= 24 {
            keys.push(format!("title:{title}"));
        }
        if keys.iter().any(|k| self.seen.contains(k)) {
            return false;
        }
        self.seen.extend(keys);
        true
    }
}

fn clean_queries(queries: &[String]) -> Vec<String> {
    queries
        .iter()
        .map(|q| q.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|q| !q.is_empty())
        .collect()
}

/// One message from a source task: its hits for one query, or (with `last`) how it ended.
type Msg = (Source, Result<Vec<Hit>, ResearchError>, bool);

/// Runs one adapter over the queries, sending each query's hits as soon as they are in.
/// Fails only when every query failed.
async fn run_source(
    a: Arc<dyn SearchSource>,
    queries: Vec<String>,
    per_source: usize,
    category: Category,
    deadline: Duration,
    tx: mpsc::UnboundedSender<Msg>,
) {
    let source = a.source();
    let work = async {
        let mut first_err: Option<ResearchError> = None;
        let mut any_ok = false;
        for q in queries.iter().take(a.max_queries()) {
            match a.search_in(category, q, per_source).await {
                Ok(h) => {
                    any_ok = true;
                    let _ = tx.send((source, Ok(h), false));
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        match (any_ok, first_err) {
            (false, Some(e)) => Err(e),
            _ => Ok(()),
        }
    };
    let result = tokio::time::timeout(deadline, work)
        .await
        .unwrap_or(Err(ResearchError::Timeout));
    // the last message of a source says whether it ended well
    let _ = tx.send((source, result.map(|()| Vec::new()), true));
}

/// Searches every adapter concurrently and reports each source's hits the moment they arrive
/// (`on_event` is called from this task, never concurrently). Returns when all sources ended.
/// Dropping the future cancels everything in flight.
pub async fn search_stream(
    adapters: &[Arc<dyn SearchSource>],
    queries: &[String],
    per_source: usize,
    category: Category,
    deadline: Duration,
    dedup: &mut Dedup,
    mut on_event: impl FnMut(SearchEvent),
) {
    let queries = clean_queries(queries);
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    for a in adapters {
        on_event(SearchEvent::Started(a.source()));
    }
    let runs = futures::future::join_all(adapters.iter().map(|a| {
        run_source(
            a.clone(),
            queries.clone(),
            per_source,
            category,
            deadline,
            tx.clone(),
        )
    }));
    drop(tx);
    let mut found: HashMap<Source, usize> = HashMap::new();
    let consume = async {
        while let Some((source, result, last)) = rx.recv().await {
            match (result, last) {
                (Ok(hits), false) => {
                    let new: Vec<Hit> = hits.into_iter().filter(|h| dedup.admit(h)).collect();
                    if !new.is_empty() {
                        *found.entry(source).or_default() += new.len();
                        on_event(SearchEvent::Hits { source, hits: new });
                    }
                }
                (Ok(_), true) => on_event(SearchEvent::Finished {
                    source,
                    found: found.get(&source).copied().unwrap_or(0),
                }),
                (Err(e), _) => on_event(SearchEvent::Failed(SourceFailure::of(source, &e))),
            }
        }
    };
    tokio::join!(runs, consume);
}

/// Searches every adapter with every query, `per_source` hits per adapter and query.
pub async fn search_all(
    adapters: &[Arc<dyn SearchSource>],
    queries: &[String],
    per_source: usize,
) -> SearchOutcome {
    search_all_in(adapters, queries, per_source, Category::All).await
}

/// [`search_all`] for one category.
pub async fn search_all_in(
    adapters: &[Arc<dyn SearchSource>],
    queries: &[String],
    per_source: usize,
    category: Category,
) -> SearchOutcome {
    let mut per_source_hits: Vec<(Source, Vec<Hit>)> = Vec::new();
    let mut failures = Vec::new();
    let mut dedup = Dedup::new();
    search_stream(
        adapters,
        queries,
        per_source,
        category,
        SOURCE_DEADLINE,
        &mut dedup,
        |ev| match ev {
            SearchEvent::Hits { source, hits } => {
                match per_source_hits.iter_mut().find(|(s, _)| *s == source) {
                    Some((_, v)) => v.extend(hits),
                    None => per_source_hits.push((source, hits)),
                }
            }
            SearchEvent::Failed(f) => failures.push(f),
            SearchEvent::Started(_) | SearchEvent::Finished { .. } => {}
        },
    )
    .await;
    SearchOutcome {
        hits: interleave(per_source_hits.into_iter().map(|(_, v)| v).collect()),
        failures,
        queries: clean_queries(queries),
    }
}

/// Interleaves the lists: the first of each, then the second of each, and so on.
fn interleave(lists: Vec<Vec<Hit>>) -> Vec<Hit> {
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..longest {
        for list in &lists {
            if let Some(h) = list.get(i) {
                out.push(h.clone());
            }
        }
    }
    out
}

/// Interleaves the lists (first of each, then second of each...) and drops repeats: the same
/// hit id, the same download address, or the same paper (DOI / arXiv id / PMC id).
pub fn merge(lists: Vec<Vec<Hit>>) -> Vec<Hit> {
    let mut dedup = Dedup::new();
    interleave(lists)
        .into_iter()
        .filter(|h| dedup.admit(h))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(src: Source, id: &str, url: &str) -> Hit {
        Hit::new(src, id, id, url)
    }

    #[test]
    fn merge_interleaves_and_collapses_the_same_paper() {
        let mut a1 = hit(Source::OpenAlex, "a1", "https://x/a1.pdf");
        a1.doi = Some("10.1234/p".into());
        let a2 = hit(Source::OpenAlex, "a2", "https://x/a2.pdf");
        let mut c1 = hit(Source::Crossref, "c1", "https://y/c1.pdf");
        c1.doi = Some("10.1234/P".into());
        let mut r1 = hit(Source::Arxiv, "r1", "https://arxiv.org/pdf/2101.00001v2");
        r1.arxiv = Some("2101.00001".into());
        let mut r2 = hit(
            Source::EuropePmc,
            "r2",
            "https://arxiv.org/pdf/2101.00001v1",
        );
        r2.arxiv = Some("2101.00001".into());
        let merged = merge(vec![vec![a1, a2], vec![c1], vec![r1], vec![r2]]);
        let ids: Vec<&str> = merged.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["open-alex:a1", "arxiv:r1", "open-alex:a2"]);
    }
}
