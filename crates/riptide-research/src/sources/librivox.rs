//! LibriVox. (Work in progress: replaced by the real adapter.)

use async_trait::async_trait;

use super::{client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Category, Hit, Source};

/// LibriVox adapter.
pub struct LibriVox {
    #[allow(dead_code)]
    base: String,
    #[allow(dead_code)]
    http: Http,
}

impl LibriVox {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://librivox.org", client_for(Source::LibriVox))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        LibriVox {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for LibriVox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchSource for LibriVox {
    fn source(&self) -> Source {
        Source::LibriVox
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
