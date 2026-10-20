//! Provider-neutral request/response types and the [`Provider`] trait.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AiError, Result};

/// The ten supported providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// OpenAI.
    OpenAi,
    /// Anthropic (Messages API).
    Anthropic,
    /// Google Gemini (generateContent).
    Gemini,
    /// DeepSeek.
    DeepSeek,
    /// OpenRouter.
    OpenRouter,
    /// DeepInfra.
    DeepInfra,
    /// Mistral AI.
    Mistral,
    /// Groq.
    Groq,
    /// Together AI.
    Together,
    /// Fireworks AI.
    Fireworks,
}

impl ProviderKind {
    /// All providers, in UI order.
    pub const ALL: [ProviderKind; 10] = [
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
        ProviderKind::DeepSeek,
        ProviderKind::OpenRouter,
        ProviderKind::DeepInfra,
        ProviderKind::Mistral,
        ProviderKind::Groq,
        ProviderKind::Together,
        ProviderKind::Fireworks,
    ];

    /// Stable lowercase id (config files, secret-store target names, CLI flag).
    pub fn id(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::DeepSeek => "deepseek",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::DeepInfra => "deepinfra",
            ProviderKind::Mistral => "mistral",
            ProviderKind::Groq => "groq",
            ProviderKind::Together => "together",
            ProviderKind::Fireworks => "fireworks",
        }
    }

    /// Human-readable brand name.
    pub fn display_name(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "OpenAI",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::DeepSeek => "DeepSeek",
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::DeepInfra => "DeepInfra",
            ProviderKind::Mistral => "Mistral AI",
            ProviderKind::Groq => "Groq",
            ProviderKind::Together => "Together AI",
            ProviderKind::Fireworks => "Fireworks AI",
        }
    }

    /// The provider's public API root (no trailing slash). Docs:
    /// OpenAI <https://platform.openai.com/docs/api-reference>,
    /// Anthropic <https://docs.anthropic.com/en/api/messages>,
    /// Gemini <https://ai.google.dev/api/generate-content>,
    /// DeepSeek <https://api-docs.deepseek.com/>,
    /// OpenRouter <https://openrouter.ai/docs/api-reference/overview>,
    /// DeepInfra <https://deepinfra.com/docs/openai_api>,
    /// Mistral <https://docs.mistral.ai/api/>,
    /// Groq <https://console.groq.com/docs/api-reference>,
    /// Together <https://docs.together.ai/reference/chat-completions-1>,
    /// Fireworks <https://docs.fireworks.ai/api-reference/post-chatcompletions>.
    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            ProviderKind::DeepSeek => "https://api.deepseek.com",
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderKind::DeepInfra => "https://api.deepinfra.com/v1/openai",
            ProviderKind::Mistral => "https://api.mistral.ai/v1",
            ProviderKind::Groq => "https://api.groq.com/openai/v1",
            ProviderKind::Together => "https://api.together.xyz/v1",
            ProviderKind::Fireworks => "https://api.fireworks.ai/inference/v1",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for ProviderKind {
    type Err = AiError;
    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim().to_ascii_lowercase();
        ProviderKind::ALL
            .into_iter()
            .find(|k| k.id() == s)
            .ok_or_else(|| AiError::Config(format!("unknown provider '{s}'")))
    }
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Instructions to the model.
    System,
    /// The human / the app.
    User,
    /// A previous model reply.
    Assistant,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Author.
    pub role: Role,
    /// Plain text.
    pub content: String,
}

impl Message {
    /// A system message.
    pub fn system(c: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: c.into(),
        }
    }
    /// A user message.
    pub fn user(c: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: c.into(),
        }
    }
    /// An assistant message.
    pub fn assistant(c: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: c.into(),
        }
    }
}

/// A chat completion request. The model comes from the provider instance.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionRequest {
    /// Conversation; system messages are lifted out for Anthropic and Gemini.
    pub messages: Vec<Message>,
    /// Output cap; clamped to the provider's `max_tokens_cap` option.
    pub max_tokens: u32,
    /// Sampling temperature; omitted when `None` (some models reject non-default values).
    pub temperature: Option<f32>,
    /// Ask for a single JSON object. Native where [`Capabilities::json_mode`] is true; for
    /// Anthropic it is emulated with an instruction appended to the system prompt.
    pub json_mode: bool,
}

impl CompletionRequest {
    /// A request with sensible defaults (`max_tokens` 256, no temperature, no JSON mode).
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            max_tokens: 256,
            temperature: None,
            json_mode: false,
        }
    }
}

/// Token accounting for one completion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated.
    pub completion_tokens: u64,
    /// Sum of both.
    pub total_tokens: u64,
    /// True when the provider did not report usage and the numbers are a rough estimate
    /// (about four characters per token).
    pub estimated: bool,
}

impl Usage {
    /// Estimates usage from text lengths.
    pub fn estimate(prompt_chars: usize, completion_chars: usize) -> Self {
        let p = (prompt_chars as u64).div_ceil(4);
        let c = (completion_chars as u64).div_ceil(4);
        Self {
            prompt_tokens: p,
            completion_tokens: c,
            total_tokens: p + c,
            estimated: true,
        }
    }
}

/// A finished completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The assistant's text.
    pub text: String,
    /// Model id the provider says it used (falls back to the requested one).
    pub model: String,
    /// Token usage (always present, possibly estimated).
    pub usage: Usage,
    /// Why generation stopped, in the provider's words.
    pub finish_reason: Option<String>,
}

/// What an adapter can do. Model-dependent in practice; this is the API-level answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Native JSON output mode.
    pub json_mode: bool,
    /// Tool / function calling exists in the API (not yet exposed by this layer).
    pub tools: bool,
    /// Streaming exists in the API (not yet exposed by this layer).
    pub streaming: bool,
}

/// One entry of a model list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Id to pass as the model.
    pub id: String,
    /// Friendly name when the provider has one.
    pub display_name: Option<String>,
}

/// Result of [`Provider::test_connection`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionReport {
    /// Models the key can see.
    pub model_count: usize,
    /// Round-trip time of the check.
    pub latency: Duration,
}

/// A chat-completion provider. Object safe.
#[async_trait]
pub trait Provider: Send + Sync + fmt::Debug {
    /// Which provider this is.
    fn kind(&self) -> ProviderKind;
    /// API-level capabilities.
    fn capabilities(&self) -> Capabilities;
    /// The model id used for completions.
    fn model(&self) -> &str;
    /// Lists the models the key can use.
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;
    /// Runs one completion. Enforces the token budget, `max_tokens` cap, timeout and retries.
    async fn complete(&self, request: CompletionRequest) -> Result<Completion>;
    /// Cheap authenticated check (a model-list call); proves the key and the URL work.
    async fn test_connection(&self) -> Result<ConnectionReport> {
        let start = std::time::Instant::now();
        let models = self.list_models().await?;
        Ok(ConnectionReport {
            model_count: models.len(),
            latency: start.elapsed(),
        })
    }
}
