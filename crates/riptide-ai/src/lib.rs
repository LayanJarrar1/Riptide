//! Bring-your-own-key AI provider layer for Riptide.
//!
//! Privacy: this crate makes **no network calls on its own**. A request leaves the machine only
//! when the user has configured a provider and a feature explicitly calls it. Keys are held in
//! [`ApiKey`] (never printed, zeroized on drop) and stored via a [`SecretStore`] (Windows
//! Credential Manager); they are never written to config, logs, errors or the database.
//!
//! Ten providers: OpenAI, Anthropic, Gemini, DeepSeek, OpenRouter, DeepInfra, Mistral, Groq,
//! Together and Fireworks. Eight share [`OpenAiCompat`]; [`Anthropic`] and [`Gemini`] have
//! their own clients. Build one with [`build_provider`] or [`AiConfig::build_provider`].

#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod anthropic;
pub mod config;
pub mod error;
pub mod gemini;
pub mod http;
pub mod key;
pub mod openai_compat;
pub mod secret;
pub mod stream;
pub mod types;

pub use anthropic::Anthropic;
pub use config::{build_provider, AiConfig};
pub use error::{AiError, Result};
pub use gemini::Gemini;
pub use http::{validate_base_url, ProviderOptions, RetryPolicy, TokenBudget};
pub use key::ApiKey;
pub use openai_compat::OpenAiCompat;
#[cfg(windows)]
pub use secret::CredentialManagerStore;
pub use secret::{platform_store, MemorySecretStore, SecretStore, DEFAULT_TARGET_PREFIX};
pub use types::{
    Capabilities, Completion, CompletionRequest, ConnectionReport, Message, ModelInfo, Provider,
    ProviderKind, Role, Usage,
};
