//! Anthropic Messages API. Docs: <https://docs.anthropic.com/en/api/messages> and
//! <https://docs.anthropic.com/en/api/models-list>.

use std::fmt;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{AiError, Result};
use crate::http::{HttpCore, ProviderOptions};
use crate::key::ApiKey;
use crate::stream::anthropic_event;
use crate::types::{
    Capabilities, Completion, CompletionRequest, ModelInfo, Provider, ProviderKind, Role, Usage,
};

/// Pinned API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Appended to the system prompt when JSON mode is requested (the API has no JSON switch).
const JSON_HINT: &str = "Respond with a single valid JSON object and nothing else.";

/// Anthropic adapter.
pub struct Anthropic {
    model: String,
    core: HttpCore,
}

impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anthropic")
            .field("model", &self.model)
            .field("base_url", &self.core.base())
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl Anthropic {
    /// Builds the adapter; `base_url` is for tests / gateways (https or loopback http).
    pub fn new(
        key: ApiKey,
        model: impl Into<String>,
        base_url: Option<&str>,
        opts: ProviderOptions,
    ) -> Result<Self> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(AiError::Config("model id is empty".into()));
        }
        let core = HttpCore::new(
            ProviderKind::Anthropic.default_base_url(),
            base_url,
            key,
            opts,
        )?;
        Ok(Self { model, core })
    }

    fn request(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        let (n, v) = self.core.secret_header("x-api-key")?;
        Ok(rb
            .header(n, v)
            .header("anthropic-version", ANTHROPIC_VERSION))
    }

    /// The request body shared by `complete` and `complete_stream`.
    fn build_body(&self, request: &CompletionRequest) -> Result<Value> {
        let max_tokens = self.core.begin(request)?;
        let mut system: Vec<String> = request
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.clone())
            .collect();
        if request.json_mode {
            system.push(JSON_HINT.to_string());
        }
        let messages: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let role = if m.role == Role::Assistant {
                    "assistant"
                } else {
                    "user"
                };
                json!({ "role": role, "content": m.content })
            })
            .collect();
        if messages.is_empty() {
            return Err(AiError::BadRequest {
                message: "no user message".into(),
            });
        }
        let mut body =
            json!({ "model": self.model, "max_tokens": max_tokens, "messages": messages });
        if !system.is_empty() {
            body["system"] = json!(system.join(
                "

"
            ));
        }
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }
        Ok(body)
    }
}

#[async_trait]
impl Provider for Anthropic {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn capabilities(&self) -> Capabilities {
        // No native JSON mode: `json_mode` requests are emulated with a system-prompt hint.
        Capabilities {
            json_mode: false,
            tools: true,
            streaming: true,
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let v = self
            .core
            .send_json("list_models", || {
                self.request(self.core.get("/models?limit=1000"))
            })
            .await?;
        let arr = v
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::InvalidResponse("no model list in response".into()))?;
        Ok(arr
            .iter()
            .filter_map(|m| {
                Some(ModelInfo {
                    id: m.get("id").and_then(Value::as_str)?.to_string(),
                    display_name: m
                        .get("display_name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
            })
            .collect())
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion> {
        let body = self.build_body(&request)?;
        let v = self
            .core
            .send_json("complete", || {
                Ok(self.request(self.core.post("/messages"))?.json(&body))
            })
            .await?;
        let text: String = v
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::InvalidResponse("no content in response".into()))?
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        let usage = match (
            v.pointer("/usage/input_tokens").and_then(Value::as_u64),
            v.pointer("/usage/output_tokens").and_then(Value::as_u64),
        ) {
            (Some(p), Some(c)) => Usage {
                prompt_tokens: p,
                completion_tokens: c,
                total_tokens: p + c,
                estimated: false,
            },
            _ => Usage::estimate(
                request.messages.iter().map(|m| m.content.len()).sum(),
                text.len(),
            ),
        };
        self.core.finish(&usage);
        Ok(Completion {
            text,
            model: v
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&self.model)
                .to_string(),
            usage,
            finish_reason: v
                .get("stop_reason")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<Completion> {
        let mut body = self.build_body(&request)?;
        body["stream"] = json!(true);
        let prompt_chars = request.messages.iter().map(|m| m.content.len()).sum();
        self.core
            .run_stream(
                "complete_stream",
                || Ok(self.request(self.core.post("/messages"))?.json(&body)),
                anthropic_event,
                prompt_chars,
                &self.model,
                on_delta,
            )
            .await
    }
}
