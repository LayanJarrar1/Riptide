//! General web search through the backend the user chose. Work in progress.

use async_trait::async_trait;

use super::SearchSource;
use crate::error::{ResearchError, Result};
use crate::types::{Category, Hit, Source};

/// Where web results come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebBackend {
    /// Brave Search API with the user's subscription token.
    Brave {
        /// The API token (never logged).
        api_key: String,
    },
    /// A SearXNG instance (validated base URL).
    SearxNg {
        /// Base URL of the instance.
        base_url: String,
    },
}

/// Web search adapter.
pub struct WebSearch {
    #[allow(dead_code)]
    backend: WebBackend,
}

impl WebSearch {
    /// Builds the adapter for `backend`.
    pub fn new(backend: WebBackend) -> Self {
        WebSearch { backend }
    }
}

#[async_trait]
impl SearchSource for WebSearch {
    fn source(&self) -> Source {
        Source::Web
    }

    async fn search_in(
        &self,
        _category: Category,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<Hit>> {
        Err(ResearchError::Invalid("not implemented yet".into()))
    }
}
