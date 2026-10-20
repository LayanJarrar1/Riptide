//! Jamendo (Creative Commons music; needs the user's free client id). Work in progress.

use async_trait::async_trait;

use super::{client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::Http;
use crate::types::{Category, Hit, Source};

/// Jamendo adapter.
pub struct Jamendo {
    #[allow(dead_code)]
    base: String,
    #[allow(dead_code)]
    client_id: String,
    #[allow(dead_code)]
    http: Http,
}

impl Jamendo {
    /// Talks to `api.jamendo.com` with the user's client id.
    pub fn new(client_id: &str) -> Self {
        Self::with_base(
            "https://api.jamendo.com",
            client_id,
            client_for(Source::Jamendo),
        )
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, client_id: &str, http: Http) -> Self {
        Jamendo {
            base: base.trim_end_matches('/').into(),
            client_id: client_id.trim().into(),
            http,
        }
    }
}

#[async_trait]
impl SearchSource for Jamendo {
    fn source(&self) -> Source {
        Source::Jamendo
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
