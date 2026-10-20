//! Openverse. (Work in progress: replaced by the real adapter.)

use async_trait::async_trait;

use super::{client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Category, Hit, Source};

/// Openverse adapter.
pub struct Openverse {
    #[allow(dead_code)]
    base: String,
    #[allow(dead_code)]
    http: Http,
}

impl Openverse {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://api.openverse.org", client_for(Source::Openverse))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Openverse {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Openverse {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchSource for Openverse {
    fn source(&self) -> Source {
        Source::Openverse
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
