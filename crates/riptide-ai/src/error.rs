//! Typed errors. No variant ever carries the API key or a request body.

use std::time::Duration;

/// Everything that can go wrong in the provider layer.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// 401/403: the key is missing, wrong or lacks permission.
    #[error("the provider rejected the API key (unauthorized)")]
    Unauthorized,
    /// 429: too many requests. `retry_after` is the server's hint, if any.
    #[error("rate limited by the provider (retry after {retry_after:?})")]
    RateLimited {
        /// Server-provided wait (`Retry-After`), when present.
        retry_after: Option<Duration>,
    },
    /// 404 on a model: the configured model id does not exist for this key.
    #[error("model not found")]
    ModelNotFound,
    /// 4xx other than the above. The message is the provider's own text, scrubbed and truncated.
    #[error("bad request: {message}")]
    BadRequest {
        /// Redacted provider message (never the request body).
        message: String,
    },
    /// The per-request timeout elapsed.
    #[error("request timed out")]
    Timeout,
    /// Connection-level failure (DNS, TCP, TLS).
    #[error("network error: {0}")]
    Network(String),
    /// 5xx after all retries.
    #[error("the provider is unavailable (HTTP {status})")]
    ProviderDown {
        /// Last HTTP status seen.
        status: u16,
    },
    /// The session token budget is used up; nothing was sent.
    #[error("session token budget exceeded ({used} of {limit} tokens used)")]
    BudgetExceeded {
        /// Tokens used so far.
        used: u64,
        /// The configured limit.
        limit: u64,
    },
    /// The provider answered 2xx with a body we could not understand.
    #[error("invalid response from the provider: {0}")]
    InvalidResponse(String),
    /// Bad local configuration (base URL, key format, model id).
    #[error("invalid configuration: {0}")]
    Config(String),
    /// The secret store failed.
    #[error("secret store error: {0}")]
    Store(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, AiError>;

impl AiError {
    /// True when retrying the same request later may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AiError::RateLimited { .. }
                | AiError::Timeout
                | AiError::Network(_)
                | AiError::ProviderDown { .. }
        )
    }
}
