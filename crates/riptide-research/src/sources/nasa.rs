//! NASA Image and Video Library. (Work in progress: replaced by the real adapter.)

use async_trait::async_trait;

use super::{client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Category, Hit, Source};

/// NASA Image and Video Library adapter.
pub struct Nasa {
    #[allow(dead_code)]
    base: String,
    #[allow(dead_code)]
    http: Http,
}

impl Nasa {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://images-api.nasa.gov", client_for(Source::Nasa))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Nasa {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Nasa {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchSource for Nasa {
    fn source(&self) -> Source {
        Source::Nasa
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
