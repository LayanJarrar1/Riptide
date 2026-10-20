//! Project Gutenberg through Gutendex. (Work in progress: replaced by the real adapter.)

use async_trait::async_trait;

use super::{client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Category, Hit, Source};

/// Project Gutenberg through Gutendex adapter.
pub struct Gutenberg {
    #[allow(dead_code)]
    base: String,
    #[allow(dead_code)]
    http: Http,
}

impl Gutenberg {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://gutendex.com", client_for(Source::Gutenberg))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Gutenberg {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Gutenberg {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchSource for Gutenberg {
    fn source(&self) -> Source {
        Source::Gutenberg
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
