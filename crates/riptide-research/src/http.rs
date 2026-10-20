//! The HTTP client every adapter uses: polite User-Agent (no personal data), per-source minimum
//! spacing between requests, a timeout per attempt, and bounded retry with backoff.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use reqwest::{redirect, Client, RequestBuilder, StatusCode};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::error::{ResearchError, Result};

/// Sent with every request. Identifies the app and where its source is; nothing about the user.
pub const USER_AGENT: &str = concat!(
    "Riptide/",
    env!("CARGO_PKG_VERSION"),
    " (open-access research search; +https://github.com/LayanJarrar1/Riptide)"
);

/// Largest body read from a source.
const MAX_BODY: usize = 8 * 1024 * 1024;

/// Retry and timing knobs. Tests shrink them.
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    /// Timeout of one attempt.
    pub timeout: Duration,
    /// Attempts including the first.
    pub max_attempts: u32,
    /// First backoff; doubles.
    pub base_delay: Duration,
    /// A `Retry-After` above this is not waited out.
    pub max_retry_after: Duration,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            timeout: Duration::from_secs(20),
            max_attempts: 3,
            base_delay: Duration::from_millis(600),
            max_retry_after: Duration::from_secs(10),
        }
    }
}

/// A client plus the pacing for one source.
#[derive(Clone, Debug)]
pub struct Http {
    client: Client,
    min_interval: Duration,
    last: Arc<Mutex<Option<Instant>>>,
    tuning: Tuning,
}

impl Http {
    /// A client that waits at least `min_interval` between two requests it sends.
    pub fn new(min_interval: Duration) -> Self {
        Self::with_tuning(min_interval, Tuning::default())
    }

    /// Like [`Http::new`] with explicit timing.
    pub fn with_tuning(min_interval: Duration, tuning: Tuning) -> Self {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .redirect(redirect::Policy::limited(5))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());
        Http {
            client,
            min_interval,
            last: Arc::new(Mutex::new(None)),
            tuning,
        }
    }

    /// Starts a GET request (headers can be added before sending through [`Http::text`]).
    pub fn get(&self, url: &str) -> RequestBuilder {
        self.client.get(url)
    }

    async fn pace(&self) {
        if self.min_interval.is_zero() {
            return;
        }
        let mut last = self.last.lock().await;
        if let Some(prev) = *last {
            let due = prev + self.min_interval;
            if due > Instant::now() {
                tokio::time::sleep_until(due).await;
            }
        }
        *last = Some(Instant::now());
    }

    /// Sends the request built by `make` (once per attempt) and returns the body text of the
    /// first 2xx answer, retrying timeouts, connection errors, 429 and 5xx.
    pub async fn text<F>(&self, make: F) -> Result<String>
    where
        F: Fn() -> RequestBuilder,
    {
        let mut attempt = 1;
        loop {
            self.pace().await;
            let err = match self.once(make()).await {
                Ok(body) => return Ok(body),
                Err(e) => e,
            };
            if !err.is_retryable() || attempt >= self.tuning.max_attempts {
                return Err(err);
            }
            let wait = match &err {
                ResearchError::RateLimited {
                    retry_after: Some(ra),
                } => {
                    if *ra > self.tuning.max_retry_after {
                        return Err(err);
                    }
                    *ra
                }
                _ => self.tuning.base_delay * 2u32.saturating_pow(attempt - 1),
            };
            tracing::debug!(
                attempt,
                wait_ms = wait.as_millis() as u64,
                "retrying a source request"
            );
            tokio::time::sleep(wait).await;
            attempt += 1;
        }
    }

    /// [`Http::text`] parsed as JSON.
    pub async fn json<F>(&self, make: F) -> Result<serde_json::Value>
    where
        F: Fn() -> RequestBuilder,
    {
        let body = self.text(make).await?;
        serde_json::from_str(&body)
            .map_err(|_| ResearchError::Parse("body is not valid JSON".into()))
    }

    async fn once(&self, rb: RequestBuilder) -> Result<String> {
        let resp = rb
            .timeout(self.tuning.timeout)
            .send()
            .await
            .map_err(net_err)?;
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let bytes = resp.bytes().await.map_err(net_err)?;
        if bytes.len() > MAX_BODY {
            return Err(ResearchError::Parse("answer too large".into()));
        }
        if status.is_success() {
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        Err(match status {
            StatusCode::TOO_MANY_REQUESTS => ResearchError::RateLimited { retry_after },
            StatusCode::FORBIDDEN if retry_after.is_some() => {
                ResearchError::RateLimited { retry_after }
            }
            s => ResearchError::Http { status: s.as_u16() },
        })
    }
}

fn net_err(e: reqwest::Error) -> ResearchError {
    if e.is_timeout() {
        ResearchError::Timeout
    } else if e.is_connect() {
        ResearchError::Network("could not connect".into())
    } else {
        ResearchError::Network("the request failed".into())
    }
}

/// Percent-encodes a query-string value.
pub(crate) fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
