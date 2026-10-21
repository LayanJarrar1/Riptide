//! Shared HTTP plumbing: options, token budget, retry with backoff, error mapping.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use reqwest::header::{HeaderName, HeaderValue, RETRY_AFTER};
use reqwest::{redirect, Client, RequestBuilder, StatusCode};
use serde_json::Value;
use url::Url;

use crate::error::{AiError, Result};
use crate::key::ApiKey;
use crate::stream::{Acc, Flow, SseEvent, SseParser};
use crate::types::{Completion, CompletionRequest, Usage};

/// Hard ceiling for a response body we are willing to read.
const MAX_BODY: usize = 8 * 1024 * 1024;
/// Longest provider message we keep in an error.
const MAX_MESSAGE_CHARS: usize = 200;

/// Session-wide token budget shared by every clone. Refuses new requests once spent.
#[derive(Debug, Clone)]
pub struct TokenBudget(Arc<BudgetInner>);

#[derive(Debug)]
struct BudgetInner {
    limit: Option<u64>,
    used: AtomicU64,
}

impl TokenBudget {
    /// A budget of `limit` tokens; `None` means unlimited.
    pub fn new(limit: Option<u64>) -> Self {
        Self(Arc::new(BudgetInner {
            limit,
            used: AtomicU64::new(0),
        }))
    }
    /// No limit.
    pub fn unlimited() -> Self {
        Self::new(None)
    }
    /// Tokens used so far.
    pub fn used(&self) -> u64 {
        self.0.used.load(Ordering::Relaxed)
    }
    /// The configured limit.
    pub fn limit(&self) -> Option<u64> {
        self.0.limit
    }
    /// Errors with [`AiError::BudgetExceeded`] if nothing is left.
    pub fn check(&self) -> Result<()> {
        match self.0.limit {
            Some(limit) if self.used() >= limit => Err(AiError::BudgetExceeded {
                used: self.used(),
                limit,
            }),
            _ => Ok(()),
        }
    }
    /// Records spent tokens.
    pub fn record(&self, tokens: u64) {
        self.0.used.fetch_add(tokens, Ordering::Relaxed);
    }
}

/// Retry policy for 429 / 5xx / timeouts / connection errors.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total attempts including the first (minimum 1).
    pub max_attempts: u32,
    /// First backoff; doubles each retry, with 50-100 % jitter.
    pub base_delay: Duration,
    /// Backoff ceiling.
    pub max_delay: Duration,
    /// A `Retry-After` longer than this is not waited out; the error is returned instead.
    pub max_retry_after: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(20),
            max_retry_after: Duration::from_secs(30),
        }
    }
}

/// Per-provider-instance options.
#[derive(Debug, Clone)]
pub struct ProviderOptions {
    /// Hard cap applied to every request's `max_tokens`.
    pub max_tokens_cap: u32,
    /// Timeout of one HTTP attempt.
    pub timeout: Duration,
    /// Retry behaviour.
    pub retry: RetryPolicy,
    /// Shared session budget.
    pub budget: TokenBudget,
}

impl Default for ProviderOptions {
    fn default() -> Self {
        Self {
            max_tokens_cap: 4096,
            timeout: Duration::from_secs(60),
            retry: RetryPolicy::default(),
            budget: TokenBudget::unlimited(),
        }
    }
}

/// Validates a base URL: `https`, or `http` only for a loopback host (local tests / local
/// gateways). Credentials, queries and fragments are refused.
pub fn validate_base_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim()).map_err(|_| AiError::Config("invalid base URL".into()))?;
    let loopback = match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => return Err(AiError::Config("base URL has no host".into())),
    };
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => return Err(AiError::Config("base URL must use https".into())),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AiError::Config(
            "base URL must not contain credentials".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AiError::Config(
            "base URL must not contain a query or fragment".into(),
        ));
    }
    Ok(url)
}

/// What every adapter embeds: client, base URL, key, options.
#[derive(Clone)]
pub(crate) struct HttpCore {
    client: Client,
    base: String,
    key: ApiKey,
    pub(crate) opts: ProviderOptions,
}

impl HttpCore {
    pub(crate) fn new(
        default_base: &str,
        base_override: Option<&str>,
        key: ApiKey,
        mut opts: ProviderOptions,
    ) -> Result<Self> {
        let base = validate_base_url(base_override.unwrap_or(default_base))?;
        let client = Client::builder()
            .redirect(redirect::Policy::none())
            .user_agent(concat!("Riptide/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| AiError::Config("could not build the HTTP client".into()))?;
        opts.retry.max_attempts = opts.retry.max_attempts.max(1);
        Ok(Self {
            client,
            base: base.as_str().trim_end_matches('/').to_string(),
            key,
            opts,
        })
    }

    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    pub(crate) fn key(&self) -> &ApiKey {
        &self.key
    }

    /// Applies the hard cap and the budget check; returns the effective `max_tokens`.
    pub(crate) fn begin(&self, req: &CompletionRequest) -> Result<u32> {
        self.opts.budget.check()?;
        if req.messages.is_empty() {
            return Err(AiError::BadRequest {
                message: "no messages".into(),
            });
        }
        Ok(req.max_tokens.clamp(1, self.opts.max_tokens_cap.max(1)))
    }

    /// Records usage against the budget.
    pub(crate) fn finish(&self, usage: &Usage) {
        self.opts.budget.record(usage.total_tokens);
    }

    pub(crate) fn get(&self, path: &str) -> RequestBuilder {
        self.client.get(format!("{}{}", self.base, path))
    }

    pub(crate) fn post(&self, path: &str) -> RequestBuilder {
        self.client.post(format!("{}{}", self.base, path))
    }

    /// A header value marked sensitive so `Debug` of the request never shows it.
    pub(crate) fn secret_header(&self, name: &str) -> Result<(HeaderName, HeaderValue)> {
        self.secret_header_with(name, self.key.expose())
    }

    pub(crate) fn secret_header_with(
        &self,
        name: &str,
        value: &str,
    ) -> Result<(HeaderName, HeaderValue)> {
        let n = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| AiError::Config("bad header name".into()))?;
        let mut v = HeaderValue::from_str(value)
            .map_err(|_| AiError::Config("the API key cannot be sent as a header".into()))?;
        v.set_sensitive(true);
        Ok((n, v))
    }

    /// Sends the request built by `make` (called once per attempt), retrying transient failures,
    /// and returns the parsed JSON body of the first 2xx answer.
    pub(crate) async fn send_json<F>(&self, label: &str, make: F) -> Result<Value>
    where
        F: Fn() -> Result<RequestBuilder>,
    {
        let policy = self.opts.retry;
        let mut attempt: u32 = 1;
        loop {
            let outcome = self.attempt(label, make()?).await;
            let err = match outcome {
                Ok(v) => return Ok(v),
                Err(e) => e,
            };
            if !err.is_retryable() || attempt >= policy.max_attempts {
                tracing::debug!(op = label, attempt, "giving up");
                return Err(err);
            }
            let wait = match &err {
                AiError::RateLimited {
                    retry_after: Some(ra),
                } => {
                    if *ra > policy.max_retry_after {
                        return Err(err);
                    }
                    *ra
                }
                _ => backoff(&policy, attempt),
            };
            tracing::debug!(
                op = label,
                attempt,
                wait_ms = wait.as_millis() as u64,
                "retrying"
            );
            tokio::time::sleep(wait).await;
            attempt += 1;
        }
    }

    /// Runs a streaming completion: `decode` turns each SSE event into text deltas (passed to
    /// `on_delta` as they arrive). Spent tokens are charged to the budget even when the stream
    /// breaks half way (an estimate from what was received).
    pub(crate) async fn run_stream<F, D>(
        &self,
        label: &str,
        make: F,
        decode: D,
        prompt_chars: usize,
        default_model: &str,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<Completion>
    where
        F: Fn() -> Result<RequestBuilder>,
        D: Fn(
            &SseEvent,
            &mut Acc,
            &mut (dyn FnMut(String) + Send),
            &dyn Fn(&str) -> String,
        ) -> Result<Flow>,
    {
        let mut acc = Acc::default();
        let key = self.key.clone();
        let scrub = move |t: &str| key.scrub(t);
        let outcome = self
            .stream_events(label, make, |ev| decode(ev, &mut acc, on_delta, &scrub))
            .await;
        match outcome {
            Ok(_) if acc.ended => {}
            Ok(_) => {
                self.finish(&Usage::estimate(prompt_chars, acc.text.len()));
                return Err(AiError::Network(
                    "the stream ended before the model finished".into(),
                ));
            }
            Err(e) => {
                if !acc.text.is_empty() {
                    self.finish(&Usage::estimate(prompt_chars, acc.text.len()));
                }
                return Err(e);
            }
        }
        let usage = acc.usage(prompt_chars);
        self.finish(&usage);
        Ok(Completion {
            text: acc.text,
            model: acc.model.unwrap_or_else(|| default_model.to_string()),
            usage,
            finish_reason: acc.finish_reason,
        })
    }

    /// How long to wait before retry number `attempt`, or `None` when the error is final.
    fn retry_wait(&self, err: &AiError, attempt: u32) -> Option<Duration> {
        let policy = self.opts.retry;
        if !err.is_retryable() || attempt >= policy.max_attempts {
            return None;
        }
        match err {
            AiError::RateLimited {
                retry_after: Some(ra),
            } => (*ra <= policy.max_retry_after).then_some(*ra),
            _ => Some(backoff(&policy, attempt)),
        }
    }

    /// Opens a streaming (SSE) response and feeds every event to `on_event` until it returns
    /// [`Flow::Done`] or the body ends. Connecting is retried like [`HttpCore::send_json`];
    /// once the body started nothing is retried. `opts.timeout` bounds the wait for the first
    /// byte and the silence between two chunks (a slow but live stream is not cut off). The
    /// returned flag says whether the body ended (true) or `on_event` stopped it (false).
    pub(crate) async fn stream_events<F, H>(
        &self,
        label: &str,
        make: F,
        mut on_event: H,
    ) -> Result<bool>
    where
        F: Fn() -> Result<RequestBuilder>,
        H: FnMut(&SseEvent) -> Result<Flow>,
    {
        let mut attempt: u32 = 1;
        let mut resp = loop {
            match self.open_stream(label, make()?).await {
                Ok(r) => break r,
                Err(e) => match self.retry_wait(&e, attempt) {
                    Some(wait) => {
                        tracing::debug!(
                            op = label,
                            attempt,
                            wait_ms = wait.as_millis() as u64,
                            "retrying"
                        );
                        tokio::time::sleep(wait).await;
                        attempt += 1;
                    }
                    None => return Err(e),
                },
            }
        };
        let mut parser = SseParser::new();
        let mut total: usize = 0;
        loop {
            let chunk = match tokio::time::timeout(self.opts.timeout, resp.chunk()).await {
                Err(_) => return Err(AiError::Timeout),
                Ok(Err(e)) => return Err(self.net_err(&e)),
                Ok(Ok(c)) => c,
            };
            let Some(chunk) = chunk else { break };
            total += chunk.len();
            if total > MAX_BODY {
                return Err(AiError::InvalidResponse("response too large".into()));
            }
            for ev in parser.push(&chunk)? {
                if on_event(&ev)? == Flow::Done {
                    return Ok(false);
                }
            }
        }
        for ev in parser.finish() {
            if on_event(&ev)? == Flow::Done {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn open_stream(&self, label: &str, rb: RequestBuilder) -> Result<reqwest::Response> {
        let rb = rb.header("accept", "text/event-stream");
        let mut resp = match tokio::time::timeout(self.opts.timeout, rb.send()).await {
            Err(_) => return Err(AiError::Timeout),
            Ok(Err(e)) => return Err(self.net_err(&e)),
            Ok(Ok(r)) => r,
        };
        let status = resp.status();
        tracing::debug!(op = label, status = status.as_u16(), "stream response");
        if status.is_success() {
            return Ok(resp);
        }
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let mut body = Vec::new();
        while body.len() < 64 * 1024 {
            match tokio::time::timeout(self.opts.timeout, resp.chunk()).await {
                Ok(Ok(Some(c))) => body.extend_from_slice(&c),
                _ => break,
            }
        }
        let message = self.error_message(&body);
        Err(map_status(status, retry_after, message, &body))
    }

    async fn attempt(&self, label: &str, rb: RequestBuilder) -> Result<Value> {
        let resp = rb
            .timeout(self.opts.timeout)
            .send()
            .await
            .map_err(|e| self.net_err(&e))?;
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let bytes = self.read_capped(resp).await?;
        tracing::debug!(
            op = label,
            status = status.as_u16(),
            bytes = bytes.len(),
            "response"
        );
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|_| AiError::InvalidResponse("body is not valid JSON".into()));
        }
        let message = self.error_message(&bytes);
        Err(map_status(status, retry_after, message, &bytes))
    }

    /// Reads the body, giving up as soon as it is clear it will pass [`MAX_BODY`]: from the
    /// declared length, or from the bytes received so far. Nothing past the cap is buffered.
    async fn read_capped(&self, mut resp: reqwest::Response) -> Result<Vec<u8>> {
        let too_large = || AiError::InvalidResponse("response too large".into());
        if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
            return Err(too_large());
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| self.net_err(&e))? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    fn net_err(&self, e: &reqwest::Error) -> AiError {
        if e.is_timeout() {
            AiError::Timeout
        } else if e.is_connect() {
            AiError::Network("could not connect to the provider".into())
        } else if e.is_decode() || e.is_body() {
            AiError::Network("the connection broke while reading the response".into())
        } else {
            AiError::Network("the request failed".into())
        }
    }

    /// Extracts the provider's error text, scrubbed of the key and truncated.
    fn error_message(&self, body: &[u8]) -> String {
        let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let msg = v
            .pointer("/error/message")
            .or_else(|| v.get("message"))
            .or_else(|| v.get("detail"))
            .or_else(|| v.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("the provider returned an error");
        let scrubbed = self.key.scrub(msg);
        scrubbed.chars().take(MAX_MESSAGE_CHARS).collect()
    }
}

fn map_status(
    status: StatusCode,
    retry_after: Option<Duration>,
    message: String,
    body: &[u8],
) -> AiError {
    match status.as_u16() {
        401 | 403 => AiError::Unauthorized,
        404 => AiError::ModelNotFound,
        408 => AiError::Timeout,
        429 => AiError::RateLimited { retry_after },
        500..=599 => AiError::ProviderDown {
            status: status.as_u16(),
        },
        400 | 422 => {
            // Gemini reports a bad key as 400 INVALID_ARGUMENT "API key not valid".
            let low = String::from_utf8_lossy(body).to_ascii_lowercase();
            if low.contains("api key not valid") || low.contains("api_key_invalid") {
                AiError::Unauthorized
            } else {
                AiError::BadRequest { message }
            }
        }
        _ => AiError::BadRequest { message },
    }
}

fn backoff(p: &RetryPolicy, attempt: u32) -> Duration {
    let exp = p.base_delay.saturating_mul(1u32 << (attempt - 1).min(16));
    let capped = exp.min(p.max_delay);
    let jitter: f64 = rand::rng().random_range(0.5..=1.0);
    capped.mul_f64(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rules() {
        assert!(validate_base_url("https://api.example.com/v1").is_ok());
        assert!(validate_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_base_url("http://localhost:1").is_ok());
        assert!(validate_base_url("http://example.com").is_err());
        assert!(validate_base_url("ftp://example.com").is_err());
        assert!(validate_base_url("https://u:p@example.com").is_err());
        assert!(validate_base_url("https://example.com/?k=1").is_err());
        assert!(validate_base_url("nonsense").is_err());
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        let p = RetryPolicy::default();
        for _ in 0..50 {
            let d1 = backoff(&p, 1);
            assert!(d1 >= Duration::from_millis(250) && d1 <= Duration::from_millis(500));
            assert!(backoff(&p, 30) <= p.max_delay);
        }
    }

    #[test]
    fn budget_refuses_when_spent() {
        let b = TokenBudget::new(Some(10));
        assert!(b.check().is_ok());
        b.record(10);
        assert!(matches!(
            b.check(),
            Err(AiError::BudgetExceeded {
                used: 10,
                limit: 10
            })
        ));
        assert!(TokenBudget::unlimited().check().is_ok());
    }
}
