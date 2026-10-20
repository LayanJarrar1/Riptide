//! Settings -> AI: the bring-your-own-key AI configuration and its privacy controls.
//!
//! Privacy and secrecy rules enforced here (and covered by the tests below):
//!
//! * AI is **off** until the user picks a provider, stores a key and names a model. Nothing in
//!   the app calls a provider before that.
//! * The key goes to the Windows Credential Manager through `riptide-ai` and nowhere else: it is
//!   not in `ai-settings.json`, not in the database, not in the usage file, not in any event, and
//!   never returned to the webview. The UI only ever receives the masked hint (`••••abcd`).
//! * Provider error text is scrubbed of the key before it reaches the UI or the logs, and no
//!   code here logs request bodies.
//! * The exact messages of the last AI request are kept **in memory only**, for the "What will
//!   be sent" panel, and are gone when the app closes or the user presses *Forget everything*.
//! * A daily request and token budget is enforced before every call.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `ai_status` | - | [`AiStatus`] |
//! | `ai_save_settings` | `settings` | [`AiStatus`] |
//! | `ai_set_key` / `ai_remove_key` | `provider`, `key` / `provider` | [`AiStatus`] |
//! | `ai_list_models` | `provider`, `baseUrl?` | `[ModelView]` |
//! | `ai_test_connection` | - | [`ConnectionView`] |
//! | `ai_last_request` | - | the last request's messages, or `null` |
//! | `ai_forget_all` | - | [`AiStatus`] |
//! | `ai_suggest_name` | `url`, `size?` | [`riptide_research::assist::NameSuggestion`] |

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use riptide_ai::{
    AiError, ApiKey, CompletionRequest, Message, Provider, ProviderKind, ProviderOptions,
    SecretStore, TokenBudget,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::media::DataDir;

const SETTINGS_FILE: &str = "ai-settings.json";
const USAGE_FILE: &str = "ai-usage.json";

/// What the user chose in Settings -> AI. Contains no secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// Which provider.
    pub provider: ProviderKind,
    /// Model id (from the provider's list or typed by hand).
    pub model: String,
    /// Optional base URL for a compatible endpoint (https, or loopback http).
    pub base_url: Option<String>,
    /// Allow the AI to see the titles of what was downloaded before. Off by default.
    pub use_download_history: bool,
    /// Offer "Suggest name and category" in the Add dialog. Off by default.
    pub suggest_names: bool,
    /// Tokens per day; `None` = no limit.
    pub daily_token_limit: Option<u64>,
    /// Requests per day; `None` = no limit.
    pub daily_request_limit: Option<u32>,
    /// Hard cap on one answer's length.
    pub max_tokens: u32,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenAi,
            model: String::new(),
            base_url: None,
            use_download_history: false,
            suggest_names: false,
            daily_token_limit: Some(100_000),
            daily_request_limit: Some(200),
            max_tokens: 1024,
        }
    }
}

/// Spend so far today (local date), kept in `ai-usage.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    /// `YYYY-MM-DD`.
    pub day: String,
    /// Tokens the provider reported (or the estimate).
    pub tokens: u64,
    /// Requests sent.
    pub requests: u32,
}

/// A provider in the picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    /// Stable id.
    pub id: &'static str,
    /// Brand name.
    pub name: &'static str,
    /// Default API root.
    pub default_base_url: &'static str,
}

/// Everything the settings page needs. Never contains a key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    /// Current settings.
    pub settings: AiSettings,
    /// `••••abcd` of the stored key for the selected provider, if there is one.
    pub key_hint: Option<String>,
    /// Provider selected + key stored + model named: the AI features are on.
    pub configured: bool,
    /// Today's usage.
    pub usage: Usage,
    /// The ten providers.
    pub providers: Vec<ProviderView>,
}

/// A failed AI operation, safe to show: a stable code the UI translates and an English message
/// from which the key has been removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiFail {
    /// `not_configured`, `daily_limit`, `unauthorized`, `rate_limited`, `model_not_found`,
    /// `bad_request`, `timeout`, `network`, `provider_down`, `budget`, `invalid_response`,
    /// `config`, `store`.
    pub code: &'static str,
    /// English detail (scrubbed).
    pub message: String,
}

/// One model of the provider's list.
#[derive(Debug, Clone, Serialize)]
pub struct ModelView {
    /// Id to use as the model.
    pub id: String,
    /// Friendly name when the provider has one.
    pub name: Option<String>,
}

/// Result of "Test connection".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    /// Time of the authenticated model-list call.
    pub list_ms: u64,
    /// Models the key can see.
    pub model_count: usize,
    /// Time of a tiny completion with the chosen model.
    pub complete_ms: u64,
    /// Model the provider says it used.
    pub model: String,
    /// Tokens the provider reported for that completion.
    pub prompt_tokens: u64,
    /// Tokens generated.
    pub completion_tokens: u64,
    /// The numbers are an estimate because the provider reported none.
    pub estimated: bool,
}

/// The messages of an AI request, as sent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPreview {
    /// Which feature made it: `research-plan`, `research-rank`, `name`, `test`.
    pub feature: String,
    /// Host the request went to (never a path or key).
    pub endpoint: String,
    /// Model used.
    pub model: String,
    /// The exact messages.
    pub messages: Vec<Message>,
    /// Local time, `HH:MM:SS`.
    pub at: String,
}

/// The AI service: files in the data dir, the secret store, the last-request memory.
pub struct AiService {
    dir: PathBuf,
    secrets: Box<dyn SecretStore>,
    last: Mutex<Option<RequestPreview>>,
    /// Serialises read-modify-write of the usage file.
    usage_lock: Mutex<()>,
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Reads a JSON file, tolerating a UTF-8 byte-order mark (editors on Windows add one).
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

impl AiService {
    /// A service keeping its files in `dir`.
    pub fn new(dir: PathBuf, secrets: Box<dyn SecretStore>) -> Self {
        Self {
            dir,
            secrets,
            last: Mutex::new(None),
            usage_lock: Mutex::new(()),
        }
    }

    /// The saved settings (defaults when none).
    pub fn settings(&self) -> AiSettings {
        read_json(&self.dir.join(SETTINGS_FILE)).unwrap_or_default()
    }

    /// Validates and saves settings.
    pub fn save_settings(&self, mut s: AiSettings) -> Result<AiSettings, AiFail> {
        s.model = s.model.trim().to_string();
        s.base_url = s
            .base_url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());
        if let Some(u) = &s.base_url {
            riptide_ai::validate_base_url(u).map_err(|e| fail(&e, None))?;
        }
        s.max_tokens = s.max_tokens.clamp(64, 8192);
        s.daily_token_limit = s.daily_token_limit.filter(|&n| n > 0);
        s.daily_request_limit = s.daily_request_limit.filter(|&n| n > 0);
        let json = serde_json::to_string_pretty(&s).map_err(|e| AiFail {
            code: "config",
            message: e.to_string(),
        })?;
        std::fs::create_dir_all(&self.dir)
            .and_then(|()| write_atomic(&self.dir.join(SETTINGS_FILE), &json))
            .map_err(|e| AiFail {
                code: "store",
                message: format!("could not save: {e}"),
            })?;
        Ok(s)
    }

    /// Stores a key in the secret store; returns the masked hint.
    pub fn set_key(&self, provider: ProviderKind, raw: String) -> Result<String, AiFail> {
        let key = ApiKey::new(raw).map_err(|e| fail(&e, None))?;
        self.secrets
            .set(provider, &key)
            .map_err(|e| fail(&e, Some(&key)))?;
        Ok(key.masked())
    }

    /// The folder this service keeps its files in (the Research settings live next to them).
    pub fn data_dir(&self) -> &Path {
        &self.dir
    }

    /// A secret that is not a provider key (`brave`, `jamendo`), from the secret store.
    pub fn named_secret(&self, name: &str) -> Option<ApiKey> {
        self.secrets.get_named(name).ok().flatten()
    }

    /// `••••abcd` of a named secret, if stored.
    pub fn named_hint(&self, name: &str) -> Option<String> {
        self.named_secret(name).map(|k| k.masked())
    }

    /// Stores a named secret in the secret store; returns the masked hint.
    pub fn set_named_secret(&self, name: &str, raw: String) -> Result<String, AiFail> {
        let key = ApiKey::new(raw).map_err(|e| fail(&e, None))?;
        self.secrets
            .set_named(name, &key)
            .map_err(|e| fail(&e, Some(&key)))?;
        Ok(key.masked())
    }

    /// Removes a named secret.
    pub fn remove_named_secret(&self, name: &str) -> Result<(), AiFail> {
        self.secrets.delete_named(name).map_err(|e| fail(&e, None))
    }

    /// Removes the key of `provider`.
    pub fn remove_key(&self, provider: ProviderKind) -> Result<(), AiFail> {
        self.secrets.delete(provider).map_err(|e| fail(&e, None))
    }

    /// `••••abcd` for the stored key of `provider`.
    pub fn key_hint(&self, provider: ProviderKind) -> Option<String> {
        self.secrets
            .get(provider)
            .ok()
            .flatten()
            .map(|k| k.masked())
    }

    /// Whether the AI features are on.
    pub fn configured(&self) -> bool {
        let s = self.settings();
        !s.model.is_empty() && self.key_hint(s.provider).is_some()
    }

    /// Everything the settings page shows.
    pub fn status(&self) -> AiStatus {
        let settings = self.settings();
        AiStatus {
            key_hint: self.key_hint(settings.provider),
            configured: !settings.model.is_empty() && self.key_hint(settings.provider).is_some(),
            usage: self.usage(),
            providers: ProviderKind::ALL
                .iter()
                .map(|k| ProviderView {
                    id: k.id(),
                    name: k.display_name(),
                    default_base_url: k.default_base_url(),
                })
                .collect(),
            settings,
        }
    }

    /// Today's usage (a new day starts at zero).
    pub fn usage(&self) -> Usage {
        let u: Usage = read_json(&self.dir.join(USAGE_FILE)).unwrap_or_default();
        if u.day == today() {
            u
        } else {
            Usage {
                day: today(),
                ..Usage::default()
            }
        }
    }

    fn add_usage(&self, tokens: u64) {
        let _guard = self.usage_lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut u = self.usage();
        u.tokens += tokens;
        u.requests += 1;
        if let Ok(json) = serde_json::to_string(&u) {
            let _ = std::fs::create_dir_all(&self.dir)
                .and_then(|()| write_atomic(&self.dir.join(USAGE_FILE), &json));
        }
    }

    /// Builds the provider for the saved settings, after checking the daily budget. The
    /// returned provider's session budget is what is left of today's token allowance.
    pub fn provider(&self) -> Result<(Box<dyn Provider>, AiSettings), AiFail> {
        let s = self.settings();
        let key = self.secrets.get(s.provider).ok().flatten();
        let (Some(key), false) = (key, s.model.is_empty()) else {
            return Err(AiFail {
                code: "not_configured",
                message: "choose a provider, store an API key and pick a model first".into(),
            });
        };
        let usage = self.usage();
        if s.daily_request_limit.is_some_and(|l| usage.requests >= l) {
            return Err(AiFail {
                code: "daily_limit",
                message: "the daily request limit is used up".into(),
            });
        }
        let remaining = s.daily_token_limit.map(|l| l.saturating_sub(usage.tokens));
        if remaining == Some(0) {
            return Err(AiFail {
                code: "daily_limit",
                message: "the daily token limit is used up".into(),
            });
        }
        let opts = ProviderOptions {
            budget: TokenBudget::new(remaining),
            ..ProviderOptions::default()
        };
        let cfg = riptide_ai::AiConfig {
            provider: s.provider,
            model: s.model.clone(),
            base_url: s.base_url.clone(),
            max_tokens: s.max_tokens,
            session_token_budget: remaining,
            use_download_history: s.use_download_history,
        };
        let p = cfg
            .build_provider(key.clone(), opts)
            .map_err(|e| fail(&e, Some(&key)))?;
        Ok((p, s))
    }

    /// Remembers what is about to be sent (memory only) for the "What will be sent" panel.
    pub fn note_request(&self, feature: &str, s: &AiSettings, messages: &[Message]) {
        let base = s
            .base_url
            .clone()
            .unwrap_or_else(|| s.provider.default_base_url().to_string());
        let endpoint = url::Url::parse(&base)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let at = chrono::Local::now().format("%H:%M:%S").to_string();
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = Some(RequestPreview {
            feature: feature.to_string(),
            endpoint,
            model: s.model.clone(),
            messages: messages.to_vec(),
            at,
        });
    }

    /// Counts a finished request against the daily budget.
    pub fn note_usage(&self, tokens: u64) {
        self.add_usage(tokens);
    }

    /// The last request's messages.
    pub fn last_request(&self) -> Option<RequestPreview> {
        self.last.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The key of the selected provider, for scrubbing error text.
    pub fn current_key(&self) -> Option<ApiKey> {
        self.secrets.get(self.settings().provider).ok().flatten()
    }

    /// Turns a provider error into something safe to show.
    pub fn fail(&self, e: &AiError) -> AiFail {
        fail(e, self.current_key().as_ref())
    }

    /// Deletes every stored key, the usage counters, the last-request memory and the settings.
    pub fn forget_all(&self) -> Result<(), AiFail> {
        for kind in ProviderKind::ALL {
            self.secrets.delete(kind).map_err(|e| fail(&e, None))?;
        }
        for name in crate::research::SECRET_NAMES {
            self.secrets
                .delete_named(name)
                .map_err(|e| fail(&e, None))?;
        }
        for f in [SETTINGS_FILE, USAGE_FILE, crate::research::SETTINGS_FILE] {
            match std::fs::remove_file(self.dir.join(f)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(AiFail {
                        code: "store",
                        message: format!("could not delete {f}: {e}"),
                    })
                }
            }
        }
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

/// Maps a provider error to a stable code and a scrubbed message.
pub fn fail(e: &AiError, key: Option<&ApiKey>) -> AiFail {
    let code = match e {
        AiError::Unauthorized => "unauthorized",
        AiError::RateLimited { .. } => "rate_limited",
        AiError::ModelNotFound => "model_not_found",
        AiError::BadRequest { .. } => "bad_request",
        AiError::Timeout => "timeout",
        AiError::Network(_) => "network",
        AiError::ProviderDown { .. } => "provider_down",
        AiError::BudgetExceeded { .. } => "budget",
        AiError::InvalidResponse(_) => "invalid_response",
        AiError::Config(_) => "config",
        AiError::Store(_) => "store",
    };
    let mut message = e.to_string();
    if let Some(k) = key {
        message = k.scrub(&message);
    }
    AiFail { code, message }
}

// ---------------------------------------------------------------- commands

/// Current settings, key hint, usage and the provider list.
#[tauri::command]
pub fn ai_status(ai: State<'_, AiService>) -> AiStatus {
    ai.status()
}

/// Saves the settings (validated); answers the new status.
#[tauri::command]
pub fn ai_save_settings(
    ai: State<'_, AiService>,
    settings: AiSettings,
) -> Result<AiStatus, AiFail> {
    ai.save_settings(settings)?;
    Ok(ai.status())
}

/// Stores the API key of `provider` in the Credential Manager. The key is not returned.
#[tauri::command]
pub fn ai_set_key(
    ai: State<'_, AiService>,
    provider: ProviderKind,
    key: String,
) -> Result<AiStatus, AiFail> {
    ai.set_key(provider, key)?;
    Ok(ai.status())
}

/// Removes the stored key of `provider`.
#[tauri::command]
pub fn ai_remove_key(ai: State<'_, AiService>, provider: ProviderKind) -> Result<AiStatus, AiFail> {
    ai.remove_key(provider)?;
    Ok(ai.status())
}

/// Lists the models the stored key can use. `base_url` is the (unsaved) override of the form.
#[tauri::command]
pub async fn ai_list_models(
    ai: State<'_, AiService>,
    provider: ProviderKind,
    base_url: Option<String>,
) -> Result<Vec<ModelView>, AiFail> {
    let key = ai.secrets.get(provider).ok().flatten().ok_or(AiFail {
        code: "not_configured",
        message: "store an API key first".into(),
    })?;
    let base = base_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    if let Some(u) = &base {
        riptide_ai::validate_base_url(u).map_err(|e| fail(&e, Some(&key)))?;
    }
    // list_models does not use the model; the adapter just needs a non-empty name
    let p = riptide_ai::build_provider(
        provider,
        key.clone(),
        "model-list",
        base.as_deref(),
        ProviderOptions::default(),
    )
    .map_err(|e| fail(&e, Some(&key)))?;
    let mut models = p.list_models().await.map_err(|e| fail(&e, Some(&key)))?;
    models.sort_by_key(|m| m.id.to_lowercase());
    Ok(models
        .into_iter()
        .map(|m| ModelView {
            id: m.id,
            name: m.display_name,
        })
        .collect())
}

/// Checks the key (model list) and the chosen model (one tiny completion); reports latency
/// and the provider-reported token usage.
#[tauri::command]
pub async fn ai_test_connection(ai: State<'_, AiService>) -> Result<ConnectionView, AiFail> {
    let (p, s) = ai.provider()?;
    let report = p.test_connection().await.map_err(|e| ai.fail(&e))?;
    let messages = vec![Message::user("Reply with the single word: OK")];
    ai.note_request("test", &s, &messages);
    let mut req = CompletionRequest::new(messages);
    req.max_tokens = 64;
    let started = Instant::now();
    let done = p.complete(req).await.map_err(|e| ai.fail(&e))?;
    ai.note_usage(done.usage.total_tokens);
    Ok(ConnectionView {
        list_ms: report.latency.as_millis() as u64,
        model_count: report.model_count,
        complete_ms: started.elapsed().as_millis() as u64,
        model: done.model,
        prompt_tokens: done.usage.prompt_tokens,
        completion_tokens: done.usage.completion_tokens,
        estimated: done.usage.estimated,
    })
}

/// The messages of the last AI request, as they were sent (memory only).
#[tauri::command]
pub fn ai_last_request(ai: State<'_, AiService>) -> Option<RequestPreview> {
    ai.last_request()
}

/// Deletes every key, the usage counters, the last-request memory and the AI settings.
#[tauri::command]
pub fn ai_forget_all(ai: State<'_, AiService>) -> Result<AiStatus, AiFail> {
    ai.forget_all()?;
    Ok(ai.status())
}

/// Suggests a cleaner file name and a category. Only the URL's host, the file name taken from
/// its path (no query string) and the size reach the provider. Never applied automatically.
#[tauri::command]
pub async fn ai_suggest_name(
    ai: State<'_, AiService>,
    url: String,
    size: Option<u64>,
) -> Result<riptide_research::assist::NameSuggestion, AiFail> {
    let (host, filename) = name_inputs(&url).ok_or(AiFail {
        code: "config",
        message: "not an http(s) address".into(),
    })?;
    let (p, s) = ai.provider()?;
    if !s.suggest_names {
        return Err(AiFail {
            code: "not_configured",
            message: "name suggestions are switched off".into(),
        });
    }
    ai.note_request(
        "name",
        &s,
        &riptide_research::assist::name_messages(&host, &filename, size),
    );
    let out = riptide_research::assist::suggest_name(p.as_ref(), &host, &filename, size)
        .await
        .map_err(|e| ai.fail(&e))?;
    ai.note_usage(out.usage.total_tokens);
    Ok(out)
}

/// The host and the last path segment of `url`; never the query, fragment or credentials.
pub fn name_inputs(url: &str) -> Option<(String, String)> {
    let u = url::Url::parse(url.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_string();
    let name = u
        .path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|n| !n.is_empty())
        .unwrap_or("download")
        .to_string();
    let name: String = url::form_urlencoded::parse(name.as_bytes())
        .next()
        .map(|(k, _)| k.into_owned())
        .unwrap_or(name);
    Some((host, name))
}

/// Location of the AI files: the app's data folder.
pub fn service_for(data: &DataDir) -> AiService {
    AiService::new(data.0.clone(), riptide_ai::platform_store())
}

#[cfg(test)]
mod tests {
    use super::*;
    use riptide_ai::MemorySecretStore;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "sk-TESTKEY-0123456789-abcdWXYZ";

    fn service(dir: &Path) -> AiService {
        AiService::new(dir.to_path_buf(), Box::new(MemorySecretStore::new()))
    }

    fn all_files_text(dir: &Path) -> String {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| std::fs::read_to_string(e.unwrap().path()).ok())
            .collect()
    }

    #[test]
    fn off_until_configured() {
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        assert!(!svc.configured());
        assert_eq!(svc.provider().err().unwrap().code, "not_configured");
        // a key alone is not enough, nor a model alone
        svc.set_key(ProviderKind::DeepInfra, KEY.into()).unwrap();
        assert!(!svc.configured());
        svc.save_settings(AiSettings {
            provider: ProviderKind::DeepInfra,
            model: "m".into(),
            ..AiSettings::default()
        })
        .unwrap();
        assert!(svc.configured());
        assert!(!AiSettings::default().use_download_history);
        assert!(!AiSettings::default().suggest_names);
    }

    #[test]
    fn key_is_never_in_files_status_or_serialised_views() {
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        let hint = svc.set_key(ProviderKind::DeepInfra, KEY.into()).unwrap();
        assert_eq!(hint, "••••WXYZ");
        svc.save_settings(AiSettings {
            provider: ProviderKind::DeepInfra,
            model: "google/gemma".into(),
            ..AiSettings::default()
        })
        .unwrap();
        svc.add_usage(10);
        svc.note_request("test", &svc.settings(), &[Message::user("hi")]);
        let status = serde_json::to_string(&svc.status()).unwrap();
        let preview = serde_json::to_string(&svc.last_request()).unwrap();
        for text in [status, preview, all_files_text(d.path())] {
            assert!(!text.contains(KEY), "{text}");
            assert!(!text.contains("TESTKEY"), "{text}");
        }
        assert!(all_files_text(d.path()).contains("google/gemma"));
    }

    #[test]
    fn a_byte_order_mark_in_the_settings_file_is_tolerated() {
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        std::fs::write(
            d.path().join(SETTINGS_FILE),
            "\u{feff}{\"provider\":\"groq\",\"model\":\"m\",\"suggestNames\":true}",
        )
        .unwrap();
        let s = svc.settings();
        assert_eq!(s.provider, ProviderKind::Groq);
        assert!(s.suggest_names);
    }

    #[test]
    fn rejects_bad_base_urls_and_clamps_limits() {
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        let bad = AiSettings {
            base_url: Some("http://evil.example/v1".into()),
            ..AiSettings::default()
        };
        assert_eq!(svc.save_settings(bad).unwrap_err().code, "config");
        let ok = svc
            .save_settings(AiSettings {
                base_url: Some("  ".into()),
                max_tokens: 1,
                daily_token_limit: Some(0),
                ..AiSettings::default()
            })
            .unwrap();
        assert_eq!(ok.base_url, None);
        assert_eq!(ok.max_tokens, 64);
        assert_eq!(ok.daily_token_limit, None);
    }

    #[test]
    fn forget_everything_removes_keys_files_and_memory() {
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        for k in [ProviderKind::OpenAi, ProviderKind::Groq] {
            svc.set_key(k, KEY.into()).unwrap();
        }
        svc.save_settings(AiSettings {
            model: "m".into(),
            ..AiSettings::default()
        })
        .unwrap();
        svc.add_usage(5);
        svc.note_request("test", &svc.settings(), &[Message::user("x")]);
        svc.forget_all().unwrap();
        assert!(svc.last_request().is_none());
        assert!(svc.key_hint(ProviderKind::OpenAi).is_none());
        assert!(svc.key_hint(ProviderKind::Groq).is_none());
        assert_eq!(svc.usage().tokens, 0);
        assert_eq!(svc.settings(), AiSettings::default());
        assert!(!d.path().join(SETTINGS_FILE).exists());
        assert!(!d.path().join(USAGE_FILE).exists());
    }

    #[test]
    fn daily_limits_stop_requests_before_they_are_sent() {
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        svc.set_key(ProviderKind::DeepInfra, KEY.into()).unwrap();
        svc.save_settings(AiSettings {
            provider: ProviderKind::DeepInfra,
            model: "m".into(),
            daily_request_limit: Some(2),
            daily_token_limit: Some(100),
            ..AiSettings::default()
        })
        .unwrap();
        assert!(svc.provider().is_ok());
        svc.add_usage(10);
        svc.add_usage(10);
        assert_eq!(svc.provider().err().unwrap().code, "daily_limit");
        let d2 = tempfile::tempdir().unwrap();
        let svc = service(d2.path());
        svc.set_key(ProviderKind::DeepInfra, KEY.into()).unwrap();
        svc.save_settings(AiSettings {
            provider: ProviderKind::DeepInfra,
            model: "m".into(),
            daily_request_limit: None,
            daily_token_limit: Some(50),
            ..AiSettings::default()
        })
        .unwrap();
        svc.add_usage(60);
        assert_eq!(svc.provider().err().unwrap().code, "daily_limit");
        // yesterday's counters do not count today
        std::fs::write(
            d2.path().join(USAGE_FILE),
            r#"{"day":"2000-01-01","tokens":999999,"requests":999}"#,
        )
        .unwrap();
        assert!(svc.provider().is_ok());
    }

    #[tokio::test]
    async fn errors_from_the_provider_never_carry_the_key_and_logs_stay_clean() {
        let server = MockServer::start().await;
        // a hostile/buggy gateway that echoes the Authorization header in its error text
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(ResponseTemplate::new(400).set_body_string(format!(
                r#"{{"error":{{"message":"bad request from key {KEY}"}}}}"#
            )))
            .mount(&server)
            .await;
        let d = tempfile::tempdir().unwrap();
        let svc = service(d.path());
        svc.set_key(ProviderKind::DeepInfra, KEY.into()).unwrap();
        svc.save_settings(AiSettings {
            provider: ProviderKind::DeepInfra,
            model: "m".into(),
            base_url: Some(server.uri()),
            ..AiSettings::default()
        })
        .unwrap();
        let logs = std::sync::Arc::new(Mutex::new(Vec::<u8>::new()));
        struct W(std::sync::Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for W {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || W(sink.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let (p, _) = svc.provider().unwrap();
        let err = p.list_models().await.unwrap_err();
        let view = svc.fail(&err);
        let shown = serde_json::to_string(&view).unwrap();
        assert!(!shown.contains(KEY), "{shown}");
        assert!(!shown.contains("TESTKEY"), "{shown}");
        let log_text = String::from_utf8_lossy(&logs.lock().unwrap()).to_string();
        assert!(!log_text.contains("TESTKEY"), "{log_text}");
        assert!(!format!("{p:?}").contains("TESTKEY"));
    }

    #[test]
    fn name_inputs_drop_query_fragment_and_credentials() {
        let (h, n) =
            name_inputs("https://user:pw@cdn.example.org/a/b/My%20File.zip?token=SECRET&x=1#frag")
                .unwrap();
        assert_eq!(h, "cdn.example.org");
        assert_eq!(n, "My File.zip");
        assert_eq!(name_inputs("https://h.example/").unwrap().1, "download");
        assert!(name_inputs("ftp://h/x").is_none());
    }
}
