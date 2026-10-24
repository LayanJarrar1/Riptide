//! One adapter per source. Each turns a keyword query into [`Hit`]s that have a direct, open
//! download link; items without one are dropped, never guessed.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Category, Hit, Source};

pub mod arxiv;
pub mod blender;
pub mod commons;
pub mod crossref;
pub mod datagov;
pub mod europepmc;
pub mod github;
pub mod gutenberg;
pub mod internet_archive;
pub mod jamendo;
pub mod librivox;
pub mod nasa;
pub mod openalex;
pub mod opengameart;
pub mod openlibrary;
pub mod openverse;
pub mod websearch;
pub mod youtube;

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
    /// Used for [`Category::All`] and by sources that serve one kind of thing only.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        self.search_in(Category::All, query, limit).await
    }

    /// Like [`SearchSource::search`] for one category. Sources that cover several kinds of
    /// media (Internet Archive, Commons, Openverse, GitHub...) override this; the default
    /// ignores the category.
    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let _ = category;
        self.search(query, limit).await
    }
}

/// Builds the adapter for `source` talking to the real service. `None` for the two sources
/// that need the user's own setup ([`Source::Jamendo`]: client id, [`Source::Web`]: backend,
/// [`Source::Youtube`]: the bundled yt-dlp): build those with [`jamendo::Jamendo::new`],
/// [`websearch::WebSearch::new`] and [`youtube::Youtube::new`].
pub fn default_source(source: Source) -> Option<Arc<dyn SearchSource>> {
    Some(match source {
        Source::OpenAlex => Arc::new(openalex::OpenAlex::new()),
        Source::Crossref => Arc::new(crossref::Crossref::new()),
        Source::Arxiv => Arc::new(arxiv::Arxiv::new()),
        Source::EuropePmc => Arc::new(europepmc::EuropePmc::new()),
        Source::InternetArchive => Arc::new(internet_archive::InternetArchive::new()),
        Source::Commons => Arc::new(commons::Commons::new()),
        Source::Github => Arc::new(github::Github::new()),
        Source::DataGov => Arc::new(datagov::DataGov::new()),
        Source::Gutenberg => Arc::new(gutenberg::Gutenberg::new()),
        Source::OpenLibrary => Arc::new(openlibrary::OpenLibrary::new()),
        Source::LibriVox => Arc::new(librivox::LibriVox::new()),
        Source::Openverse => Arc::new(openverse::Openverse::new()),
        Source::Blender => Arc::new(blender::Blender::new()),
        Source::OpenGameArt => Arc::new(opengameart::OpenGameArt::new()),
        Source::Nasa => Arc::new(nasa::Nasa::new()),
        Source::Jamendo | Source::Web | Source::Youtube => return None,
    })
}

/// Minimum spacing between two requests to each service, from their published etiquette
/// (arXiv: one request per three seconds; GitHub search: ten per minute unauthenticated).
pub(crate) fn pacing(source: Source) -> Duration {
    match source {
        Source::Arxiv => Duration::from_millis(3100),
        Source::Github => Duration::from_millis(2200),
        Source::OpenAlex | Source::Crossref => Duration::from_millis(250),
        // Brave's free plan allows one request per second
        Source::Web => Duration::from_millis(1100),
        Source::OpenGameArt | Source::Youtube => Duration::from_millis(1000),
        Source::EuropePmc
        | Source::InternetArchive
        | Source::Commons
        | Source::DataGov
        | Source::Gutenberg
        | Source::OpenLibrary
        | Source::LibriVox
        | Source::Jamendo
        | Source::Openverse
        | Source::Blender
        | Source::Nasa => Duration::from_millis(300),
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
