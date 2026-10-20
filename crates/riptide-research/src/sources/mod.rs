//! One adapter per source. Each turns a keyword query into [`Hit`]s that have a direct, open
//! download link; items without one are dropped, never guessed.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Hit, Source};

pub mod arxiv;
pub mod commons;
pub mod crossref;
pub mod datagov;
pub mod europepmc;
pub mod github;
pub mod internet_archive;
pub mod openalex;

/// A searchable open source.
#[async_trait]
pub trait SearchSource: Send + Sync {
    /// Which source this is.
    fn source(&self) -> Source;
    /// How many query phrasings this source can afford per search (slow or tightly rate-limited
    /// services take fewer).
    fn max_queries(&self) -> usize {
        3
    }
    /// Searches for `query` and returns at most `limit` hits (limit is clamped to 1..=50).
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>>;
}

/// Builds the adapter for `source` talking to the real service.
pub fn default_source(source: Source) -> Arc<dyn SearchSource> {
    match source {
        Source::OpenAlex => Arc::new(openalex::OpenAlex::new()),
        Source::Crossref => Arc::new(crossref::Crossref::new()),
        Source::Arxiv => Arc::new(arxiv::Arxiv::new()),
        Source::EuropePmc => Arc::new(europepmc::EuropePmc::new()),
        Source::InternetArchive => Arc::new(internet_archive::InternetArchive::new()),
        Source::Commons => Arc::new(commons::Commons::new()),
        Source::Github => Arc::new(github::Github::new()),
        Source::DataGov => Arc::new(datagov::DataGov::new()),
    }
}

/// Minimum spacing between two requests to each service, from their published etiquette
/// (arXiv: one request per three seconds; GitHub search: ten per minute unauthenticated).
pub(crate) fn pacing(source: Source) -> Duration {
    match source {
        Source::Arxiv => Duration::from_millis(3100),
        Source::Github => Duration::from_millis(2200),
        Source::OpenAlex | Source::Crossref => Duration::from_millis(250),
        Source::EuropePmc | Source::InternetArchive | Source::Commons | Source::DataGov => {
            Duration::from_millis(300)
        }
    }
}

pub(crate) fn check_query(query: &str) -> Result<String> {
    let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return Err(ResearchError::Invalid("empty query".into()));
    }
    if q.chars().count() > 300 {
        return Err(ResearchError::Invalid("query too long".into()));
    }
    Ok(q)
}

pub(crate) fn clamp(limit: usize) -> usize {
    limit.clamp(1, 50)
}

/// Shared by adapters: a client with this source's pacing.
pub(crate) fn client_for(source: Source) -> Http {
    Http::new(pacing(source))
}
