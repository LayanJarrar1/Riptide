//! Serializable AI settings. Contains no secrets: keys live in a [`crate::SecretStore`].

use serde::{Deserialize, Serialize};

use crate::anthropic::Anthropic;
use crate::error::{AiError, Result};
use crate::gemini::Gemini;
use crate::http::{validate_base_url, ProviderOptions};
use crate::key::ApiKey;
use crate::openai_compat::OpenAiCompat;
use crate::types::{Provider, ProviderKind};

/// User-facing AI configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// Which provider to use.
    pub provider: ProviderKind,
    /// Model id for that provider.
    pub model: String,
    /// Optional base URL override (https, or loopback http for local gateways).
    pub base_url: Option<String>,
    /// Per-request output cap (a hard ceiling on top of whatever a feature asks for).
    pub max_tokens: u32,
    /// Session token budget; `None` is unlimited. Not persisted usage: resets each run.
    pub session_token_budget: Option<u64>,
    /// Privacy flag for the research feature: may it read the local download history to build
    /// prompts? **Off by default.** Nothing in this crate reads history; the flag is stored here
    /// so the research layer has one place to ask, and must refuse to send history while false.
    pub use_download_history: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenAi,
            model: String::new(),
            base_url: None,
            max_tokens: 1024,
            session_token_budget: Some(200_000),
            use_download_history: false,
        }
    }
}

impl AiConfig {
    /// Checks the fields; the base URL must be https (or loopback http).
    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() {
            return Err(AiError::Config("model id is empty".into()));
        }
        if self.max_tokens == 0 {
            return Err(AiError::Config("max_tokens must be at least 1".into()));
        }
        if let Some(u) = &self.base_url {
            validate_base_url(u)?;
        }
        Ok(())
    }

    /// Builds the adapter for this config. `opts.max_tokens_cap` is set from `max_tokens` and
    /// the budget from `session_token_budget` unless the caller already supplied a limited one.
    pub fn build_provider(
        &self,
        key: ApiKey,
        mut opts: ProviderOptions,
    ) -> Result<Box<dyn Provider>> {
        self.validate()?;
        opts.max_tokens_cap = self.max_tokens;
        if opts.budget.limit().is_none() {
            opts.budget = crate::http::TokenBudget::new(self.session_token_budget);
        }
        build_provider(
            self.provider,
            key,
            &self.model,
            self.base_url.as_deref(),
            opts,
        )
    }
}

/// Builds the right adapter for `kind`.
pub fn build_provider(
    kind: ProviderKind,
    key: ApiKey,
    model: &str,
    base_url: Option<&str>,
    opts: ProviderOptions,
) -> Result<Box<dyn Provider>> {
    Ok(match kind {
        ProviderKind::Anthropic => Box::new(Anthropic::new(key, model, base_url, opts)?),
        ProviderKind::Gemini => Box::new(Gemini::new(key, model, base_url, opts)?),
        other => Box::new(OpenAiCompat::new(other, key, model, base_url, opts)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_flag_defaults_off_and_roundtrips() {
        let c = AiConfig::default();
        assert!(!c.use_download_history);
        let s = serde_json::to_string(&c).unwrap();
        let back: AiConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
        let empty: AiConfig = serde_json::from_str("{}").unwrap();
        assert!(!empty.use_download_history);
    }

    #[test]
    fn validation() {
        let mut c = AiConfig {
            model: "m".into(),
            ..AiConfig::default()
        };
        assert!(c.validate().is_ok());
        c.base_url = Some("http://evil.example".into());
        assert!(c.validate().is_err());
        c.base_url = Some("http://127.0.0.1:9".into());
        assert!(c.validate().is_ok());
        c.model.clear();
        assert!(c.validate().is_err());
    }
}
