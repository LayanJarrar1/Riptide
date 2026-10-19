//! One client for every OpenAI-compatible chat-completions API (OpenAI, DeepSeek, OpenRouter,
//! DeepInfra, Mistral, Groq, Together, Fireworks), with per-provider quirks in a small table.

use std::fmt;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{AiError, Result};
use crate::http::{HttpCore, ProviderOptions};
use crate::key::ApiKey;
use crate::types::{
    Capabilities, Completion, CompletionRequest, Message, ModelInfo, Provider, ProviderKind, Role,
    Usage,
};

/// Sent to OpenRouter as `HTTP-Referer` / `X-Title` so Riptide shows up in their app list.
const OPENROUTER_REFERER: &str = "https://github.com/LayanJarrar1/Riptide";
const OPENROUTER_TITLE: &str = "Riptide";

/// OpenAI-compatible adapter.
pub struct OpenAiCompat {
    kind: ProviderKind,
    model: String,
    core: HttpCore,
}

impl fmt::Debug for OpenAiCompat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompat")
            .field("kind", &self.kind)
            .field("model", &self.model)
            .field("base_url", &self.core.base())
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl OpenAiCompat {
    /// Builds the adapter. `kind` must be one of the OpenAI-compatible providers.
    /// `base_url` overrides the default and exists for tests and gateways; it must be https or
    /// loopback http.
    pub fn new(
        kind: ProviderKind,
        key: ApiKey,
        model: impl Into<String>,
        base_url: Option<&str>,
        opts: ProviderOptions,
    ) -> Result<Self> {
        if matches!(kind, ProviderKind::Anthropic | ProviderKind::Gemini) {
            return Err(AiError::Config(format!("{kind} is not OpenAI-compatible")));
        }
        let model = model.into();
        if model.trim().is_empty() {
            return Err(AiError::Config("model id is empty".into()));
        }
        let core = HttpCore::new(kind.default_base_url(), base_url, key, opts)?;
        Ok(Self { kind, model, core })
    }

    /// OpenAI's newer models reject `max_tokens`; every other provider here still wants it.
    fn token_param(&self) -> &'static str {
        match self.kind {
            ProviderKind::OpenAi => "max_completion_tokens",
            _ => "max_tokens",
        }
    }

    fn request(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        let (n, v) = self.core.secret_header_with(
            "authorization",
            &format!("Bearer {}", self.core.key().expose()),
        )?;
        let mut rb = rb.header(n, v);
        if self.kind == ProviderKind::OpenRouter {
            rb = rb
                .header("HTTP-Referer", OPENROUTER_REFERER)
                .header("X-Title", OPENROUTER_TITLE);
        }
        Ok(rb)
    }

    fn body(&self, req: &CompletionRequest, max_tokens: u32) -> Value {
        let msgs: Vec<Value> = req
            .messages
            .iter()
            .map(|m| json!({ "role": role_str(m), "content": m.content }))
            .collect();
        let mut body = json!({ "model": self.model, "messages": msgs });
        body[self.token_param()] = json!(max_tokens);
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if req.json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }
        body
    }
}

fn role_str(m: &Message) -> &'static str {
    match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn u64_at(v: &Value, ptr: &str) -> Option<u64> {
    v.pointer(ptr).and_then(Value::as_u64)
}

#[async_trait]
impl Provider for OpenAiCompat {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn capabilities(&self) -> Capabilities {
        // API-level support. Whether a given model honours these is model-dependent
        // (e.g. DeepSeek's reasoner ignores JSON mode and tools).
        Capabilities {
            json_mode: true,
            tools: true,
            streaming: true,
        }
    }

    /// Docs: OpenAI `GET /models`; Together returns a bare array, the rest `{ "data": [...] }`.
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let v = self
            .core
            .send_json("list_models", || self.request(self.core.get("/models")))
            .await?;
        let arr = match &v {
            Value::Array(a) => a,
            _ => v
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| AiError::InvalidResponse("no model list in response".into()))?,
        };
        Ok(arr
            .iter()
            .filter_map(|m| {
                let id = m.get("id").and_then(Value::as_str)?;
                // Together tags non-chat models (image, embedding...) with a `type`.
                if let Some(t) = m.get("type").and_then(Value::as_str) {
                    if t != "chat" && t != "language" {
                        return None;
                    }
                }
                let display_name = m
                    .get("display_name")
                    .or_else(|| m.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Some(ModelInfo {
                    id: id.to_string(),
                    display_name,
                })
            })
            .collect())
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion> {
        let max_tokens = self.core.begin(&request)?;
        let body = self.body(&request, max_tokens);
        let v = self
            .core
            .send_json("complete", || {
                Ok(self
                    .request(self.core.post("/chat/completions"))?
                    .json(&body))
            })
            .await?;
        let choice = v
            .pointer("/choices/0")
            .ok_or_else(|| AiError::InvalidResponse("no choices in response".into()))?;
        let text = match choice.pointer("/message/content") {
            Some(Value::String(s)) => s.clone(),
            // Some gateways return content parts.
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
            _ => {
                return Err(AiError::InvalidResponse(
                    "choice has no message content".into(),
                ))
            }
        };
        let usage = match (
            u64_at(&v, "/usage/prompt_tokens"),
            u64_at(&v, "/usage/completion_tokens"),
        ) {
            (Some(p), Some(c)) => Usage {
                prompt_tokens: p,
                completion_tokens: c,
                total_tokens: u64_at(&v, "/usage/total_tokens").unwrap_or(p + c),
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
            finish_reason: choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}
