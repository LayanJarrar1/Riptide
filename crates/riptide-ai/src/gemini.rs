//! Google Gemini API. Docs: <https://ai.google.dev/api/generate-content> and
//! <https://ai.google.dev/api/models>. The key travels in `x-goog-api-key`, never in the URL.

use std::fmt;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{AiError, Result};
use crate::http::{HttpCore, ProviderOptions};
use crate::key::ApiKey;
use crate::stream::gemini_event;
use crate::types::{
    Capabilities, Completion, CompletionRequest, ModelInfo, Provider, ProviderKind, Role, Usage,
};

/// Gemini adapter.
pub struct Gemini {
    model: String,
    core: HttpCore,
}

impl fmt::Debug for Gemini {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gemini")
            .field("model", &self.model)
            .field("base_url", &self.core.base())
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl Gemini {
    /// Builds the adapter. The model id (an optional `models/` prefix is stripped) may only
    /// contain letters, digits, `.`, `-` and `_` because it becomes part of the URL path.
    pub fn new(
        key: ApiKey,
        model: impl Into<String>,
        base_url: Option<&str>,
        opts: ProviderOptions,
    ) -> Result<Self> {
        let model: String = model.into();
        let model = model.trim().trim_start_matches("models/").to_string();
        if model.is_empty()
            || !model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(AiError::Config("invalid Gemini model id".into()));
        }
        let core = HttpCore::new(ProviderKind::Gemini.default_base_url(), base_url, key, opts)?;
        Ok(Self { model, core })
    }

    fn request(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        let (n, v) = self.core.secret_header("x-goog-api-key")?;
        Ok(rb.header(n, v))
    }

    /// The request body shared by `complete` and `complete_stream`.
    fn build_body(&self, request: &CompletionRequest) -> Result<Value> {
        let max_tokens = self.core.begin(request)?;
        let system: Vec<&str> = request
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .collect();
        let contents: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let role = if m.role == Role::Assistant {
                    "model"
                } else {
                    "user"
                };
                json!({ "role": role, "parts": [{ "text": m.content }] })
            })
            .collect();
        if contents.is_empty() {
            return Err(AiError::BadRequest {
                message: "no user message".into(),
            });
        }
        let mut gen = json!({ "maxOutputTokens": max_tokens });
        if let Some(t) = request.temperature {
            gen["temperature"] = json!(t);
        }
        if request.json_mode {
            gen["responseMimeType"] = json!("application/json");
        }
        let mut body = json!({ "contents": contents, "generationConfig": gen });
        if !system.is_empty() {
            body["systemInstruction"] = json!({ "parts": [{ "text": system.join("\n\n") }] });
        }
        Ok(body)
    }
}

#[async_trait]
impl Provider for Gemini {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            json_mode: true,
            tools: true,
            streaming: true,
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let v = self
            .core
            .send_json("list_models", || {
                self.request(self.core.get("/models?pageSize=1000"))
            })
            .await?;
        let arr = v
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::InvalidResponse("no model list in response".into()))?;
        Ok(arr
            .iter()
            .filter(|m| {
                m.get("supportedGenerationMethods")
                    .and_then(Value::as_array)
                    .is_none_or(|a| a.iter().any(|x| x.as_str() == Some("generateContent")))
            })
            .filter_map(|m| {
                let name = m.get("name").and_then(Value::as_str)?;
                Some(ModelInfo {
                    id: name.trim_start_matches("models/").to_string(),
                    display_name: m
                        .get("displayName")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
            })
            .collect())
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion> {
        let body = self.build_body(&request)?;
        let path = format!("/models/{}:generateContent", self.model);
        let v = self
            .core
            .send_json("complete", || {
                Ok(self.request(self.core.post(&path))?.json(&body))
            })
            .await?;
        let cand = v
            .pointer("/candidates/0")
            .ok_or_else(|| AiError::InvalidResponse("no candidates in response".into()))?;
        let text: String = cand
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        let g = |p: &str| v.pointer(p).and_then(Value::as_u64);
        let usage = match (
            g("/usageMetadata/promptTokenCount"),
            g("/usageMetadata/totalTokenCount"),
        ) {
            (Some(p), Some(t)) => Usage {
                prompt_tokens: p,
                completion_tokens: g("/usageMetadata/candidatesTokenCount")
                    .unwrap_or(t.saturating_sub(p)),
                total_tokens: t,
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
                .get("modelVersion")
                .and_then(Value::as_str)
                .unwrap_or(&self.model)
                .to_string(),
            usage,
            finish_reason: cand
                .get("finishReason")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<Completion> {
        let body = self.build_body(&request)?;
        let path = format!("/models/{}:streamGenerateContent?alt=sse", self.model);
        let prompt_chars = request.messages.iter().map(|m| m.content.len()).sum();
        self.core
            .run_stream(
                "complete_stream",
                || Ok(self.request(self.core.post(&path))?.json(&body)),
                gemini_event,
                prompt_chars,
                &self.model,
                on_delta,
            )
            .await
    }
}
