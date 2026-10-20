//! Shared helpers for the mock-server tests. No network beyond loopback.
#![allow(dead_code, missing_docs)]

pub mod sse;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use riptide_ai::{
    build_provider, ApiKey, CompletionRequest, Message, Provider, ProviderKind, ProviderOptions,
    RetryPolicy, TokenBudget,
};
use serde_json::{json, Value};
use wiremock::MockServer;

/// A fake key used by every mock test. Long and distinctive so leaks are easy to spot.
pub const KEY: &str = "sk-test-LEAKCANARY-9f8e7d6c5b4a3210";
pub const MODEL: &str = "test-model";

/// Per-provider wire details the mock needs.
pub struct Spec {
    pub complete_path: String,
    pub models_path: &'static str,
}

pub fn spec(kind: ProviderKind) -> Spec {
    match kind {
        ProviderKind::Anthropic => Spec {
            complete_path: "/messages".into(),
            models_path: "/models",
        },
        ProviderKind::Gemini => Spec {
            complete_path: format!("/models/{MODEL}:generateContent"),
            models_path: "/models",
        },
        _ => Spec {
            complete_path: "/chat/completions".into(),
            models_path: "/models",
        },
    }
}

/// A successful completion body in the provider's own shape. `usage` false omits token counts.
pub fn ok_body(kind: ProviderKind, text: &str, usage: bool) -> Value {
    match kind {
        ProviderKind::Anthropic => {
            let mut v = json!({
                "id": "msg_1", "type": "message", "role": "assistant", "model": MODEL,
                "content": [{ "type": "text", "text": text }], "stop_reason": "end_turn"
            });
            if usage {
                v["usage"] = json!({ "input_tokens": 11, "output_tokens": 4 });
            }
            v
        }
        ProviderKind::Gemini => {
            let mut v = json!({
                "candidates": [{ "content": { "role": "model", "parts": [{ "text": text }] },
                                 "finishReason": "STOP" }],
                "modelVersion": MODEL
            });
            if usage {
                v["usageMetadata"] = json!({
                    "promptTokenCount": 11, "candidatesTokenCount": 4, "totalTokenCount": 15
                });
            }
            v
        }
        _ => {
            let mut v = json!({
                "id": "c1", "model": MODEL,
                "choices": [{ "index": 0, "message": { "role": "assistant", "content": text },
                               "finish_reason": "stop" }]
            });
            if usage {
                v["usage"] =
                    json!({ "prompt_tokens": 11, "completion_tokens": 4, "total_tokens": 15 });
            }
            v
        }
    }
}

/// A model-list body plus the ids we expect back.
pub fn models_body(kind: ProviderKind) -> (Value, Vec<&'static str>) {
    match kind {
        ProviderKind::Anthropic => (
            json!({ "data": [{ "id": "m-one", "display_name": "One" }, { "id": "m-two" }],
                    "has_more": false }),
            vec!["m-one", "m-two"],
        ),
        ProviderKind::Gemini => (
            json!({ "models": [
                { "name": "models/gem-one", "displayName": "Gem One",
                  "supportedGenerationMethods": ["generateContent"] },
                { "name": "models/embed", "supportedGenerationMethods": ["embedContent"] }
            ] }),
            vec!["gem-one"],
        ),
        ProviderKind::Together => (
            json!([{ "id": "t-chat", "type": "chat" }, { "id": "t-image", "type": "image" }]),
            vec!["t-chat"],
        ),
        _ => (
            json!({ "object": "list", "data": [{ "id": "m-one" }, { "id": "m-two" }] }),
            vec!["m-one", "m-two"],
        ),
    }
}

pub fn key() -> ApiKey {
    ApiKey::new(KEY).expect("test key")
}

/// Fast options: 1 ms backoff, 3 attempts, 2 s timeout.
pub fn fast_opts() -> ProviderOptions {
    ProviderOptions {
        max_tokens_cap: 4096,
        timeout: Duration::from_secs(2),
        retry: RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(5),
        },
        budget: TokenBudget::unlimited(),
    }
}

pub fn provider(
    kind: ProviderKind,
    server: &MockServer,
    opts: ProviderOptions,
) -> Box<dyn Provider> {
    build_provider(kind, key(), MODEL, Some(&server.uri()), opts).expect("provider")
}

pub fn simple_request() -> CompletionRequest {
    let mut r = CompletionRequest::new(vec![
        Message::system("You are terse."),
        Message::user("Say ready."),
    ]);
    r.max_tokens = 32;
    r
}

/// Collects tracing output (all levels, all targets) so tests can grep it for secrets.
#[derive(Clone, Default)]
pub struct LogCapture(pub Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("lock")).into_owned()
    }
    pub fn install(&self) -> tracing::subscriber::DefaultGuard {
        let sub = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(sub)
    }
}

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Asserts `haystack` does not contain the test key, with a message that does not echo it.
pub fn assert_no_key(haystack: &str, what: &str) {
    assert!(!haystack.contains(KEY), "the API key leaked into {what}");
    assert!(
        !haystack.contains("LEAKCANARY"),
        "part of the API key leaked into {what}"
    );
}

/// Formats an error every way an app might (Display, Debug, alternate, source chain).
pub fn error_strings(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = format!("{e} | {e:?} | {e:#?}");
    let mut cur = e.source();
    while let Some(s) = cur {
        out.push_str(&format!(" | {s} | {s:?}"));
        cur = s.source();
    }
    out
}
