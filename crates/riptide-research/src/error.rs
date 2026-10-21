//! Typed errors for the source adapters. Messages never contain request URLs with queries.

use std::time::Duration;

/// Everything that can go wrong talking to one source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResearchError {
    /// The attempt timed out.
    #[error("the source did not answer in time")]
    Timeout,
    /// DNS / TCP / TLS failure or a broken connection.
    #[error("network error: {0}")]
    Network(String),
    /// 429, or the source asked us to slow down.
    #[error("rate limited by the source (retry after {retry_after:?})")]
    RateLimited {
        /// The server's hint, when present.
        retry_after: Option<Duration>,
    },
    /// Any other non-success status after retries.
    #[error("the source answered HTTP {status}")]
    Http {
        /// Last status code.
        status: u16,
    },
    /// 2xx with a body that is not what the API documents.
    #[error("unexpected answer from the source: {0}")]
    Parse(String),
    /// Bad local input (empty query, bad base URL).
    #[error("invalid request: {0}")]
    Invalid(String),
}

impl ResearchError {
    /// Worth another attempt?
    pub fn is_retryable(&self) -> bool {
        match self {
            ResearchError::Timeout
            | ResearchError::Network(_)
            | ResearchError::RateLimited { .. } => true,
            ResearchError::Http { status } => *status >= 500,
            ResearchError::Parse(_) | ResearchError::Invalid(_) => false,
        }
    }

    /// Short stable code for the UI to translate.
    pub fn code(&self) -> &'static str {
        match self {
            ResearchError::Timeout => "timeout",
            ResearchError::Network(_) => "network",
            ResearchError::RateLimited { .. } => "rate_limited",
            ResearchError::Http { .. } => "http",
            ResearchError::Parse(_) => "parse",
            ResearchError::Invalid(_) => "invalid",
        }
    }
}

/// Result alias.
pub type Result<T> = std::result::Result<T, ResearchError>;
