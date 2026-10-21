//! `riptide ai test`: checks a provider connection with a key from the environment or the
//! Windows Credential Manager. The key is never accepted as an argument and never printed.

use anyhow::{bail, Context};
use riptide_ai::{AiError, ApiKey, ProviderKind, ProviderOptions};

use crate::i18n::{t, tf, Lang};

/// Environment variable that carries the key (preferred over the credential store).
const KEY_ENV: &str = "RIPTIDE_AI_KEY";

fn load_key(kind: ProviderKind) -> anyhow::Result<Option<ApiKey>> {
    if let Ok(v) = std::env::var(KEY_ENV) {
        if !v.trim().is_empty() {
            return Ok(Some(ApiKey::new(v)?));
        }
    }
    Ok(riptide_ai::platform_store().get(kind)?)
}

fn reason(lang: Lang, e: &AiError) -> String {
    match e {
        AiError::Unauthorized => t(lang, "ai.err.unauthorized").to_string(),
        AiError::RateLimited { .. } => t(lang, "ai.err.rate").to_string(),
        AiError::ModelNotFound => t(lang, "ai.err.model").to_string(),
        AiError::Timeout => t(lang, "ai.err.timeout").to_string(),
        AiError::Network(_) => t(lang, "ai.err.network").to_string(),
        other => other.to_string(),
    }
}

/// Runs the connection test and prints the result.
pub async fn test(
    lang: Lang,
    provider: &str,
    model: Option<String>,
    base_url: Option<String>,
) -> anyhow::Result<()> {
    let kind: ProviderKind = provider.parse().context("unknown provider")?;
    let Some(key) = load_key(kind)? else {
        bail!("{}", t(lang, "ai.key_missing"));
    };
    let wanted = model
        .clone()
        .unwrap_or_else(|| "connection-test".to_string());
    let p = riptide_ai::build_provider(
        kind,
        key,
        &wanted,
        base_url.as_deref(),
        ProviderOptions::default(),
    )?;
    let report = match p.test_connection().await {
        Ok(r) => r,
        Err(e) => bail!(
            "{}",
            tf(lang, "ai.failed", &[("reason", &reason(lang, &e))])
        ),
    };
    println!(
        "{}",
        tf(
            lang,
            "ai.ok",
            &[
                ("provider", kind.display_name()),
                ("n", &report.model_count.to_string()),
                ("ms", &report.latency.as_millis().to_string()),
            ]
        )
    );
    if let Some(m) = model {
        let found = p
            .list_models()
            .await
            .map(|l| l.iter().any(|x| x.id == m))
            .unwrap_or(false);
        let key = if found {
            "ai.model_found"
        } else {
            "ai.model_missing"
        };
        println!("{}", tf(lang, key, &[("model", &m)]));
    }
    Ok(())
}
