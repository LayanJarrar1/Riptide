//! Runs several adapters (and several query phrasings) concurrently and merges the answers:
//! repeated items collapse into one, sources are interleaved so no single source floods the
//! list, and a failing source is reported next to the results instead of failing the search.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use serde::Serialize;

use crate::error::ResearchError;
use crate::sources::SearchSource;
use crate::types::{Hit, Source};

/// Longest a single source may take for all its queries together.
const SOURCE_DEADLINE: Duration = Duration::from_secs(60);

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

/// Searches every adapter with every query, `per_source` hits per adapter and query.
pub async fn search_all(
    adapters: &[Arc<dyn SearchSource>],
    queries: &[String],
    per_source: usize,
) -> SearchOutcome {
    let queries: Vec<String> = queries
        .iter()
        .map(|q| q.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|q| !q.is_empty())
        .collect();
    let runs = adapters.iter().map(|a| async {
        let work = async {
            let mut hits = Vec::new();
            let mut first_err: Option<ResearchError> = None;
            let mut any_ok = false;
            for q in queries.iter().take(a.max_queries()) {
                match a.search(q, per_source).await {
                    Ok(h) => {
                        any_ok = true;
                        hits.extend(h);
                    }
                    Err(e) => {
                        first_err.get_or_insert(e);
                    }
                }
            }
            match (any_ok, first_err) {
                (false, Some(e)) => Err(e),
                _ => Ok(hits),
            }
        };
        let result = tokio::time::timeout(SOURCE_DEADLINE, work)
            .await
            .unwrap_or(Err(ResearchError::Timeout));
        (a.source(), result)
    });
    let mut per_source_hits: Vec<Vec<Hit>> = Vec::new();
    let mut failures = Vec::new();
    for (source, result) in join_all(runs).await {
        match result {
            Ok(h) => per_source_hits.push(h),
            Err(e) => failures.push(SourceFailure {
                source,
                code: e.code().into(),
                message: e.to_string(),
            }),
        }
    }
    SearchOutcome {
        hits: merge(per_source_hits),
        failures,
        queries,
    }
}

/// Interleaves the lists (first of each, then second of each...) and drops repeats: the same
/// hit id, the same download address, or the same paper (DOI / arXiv id / PMC id).
pub fn merge(lists: Vec<Vec<Hit>>) -> Vec<Hit> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for list in &lists {
            let Some(hit) = list.get(i) else { continue };
            let mut keys = riptide_core::dedup::keys(&hit.identity());
            // a file name + size match is a weak key; never merge on it here
            keys.retain(|k| !k.starts_with("file:"));
            keys.push(format!("id:{}", hit.id));
            if keys.iter().any(|k| seen.contains(k)) {
                continue;
            }
            seen.extend(keys);
            out.push(hit.clone());
        }
    }
    out
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
