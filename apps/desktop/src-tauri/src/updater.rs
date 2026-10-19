//! Self-update through `tauri-plugin-updater`, the only network call Riptide makes that is not
//! one of the user's downloads (Hard rule 6), and one the user can switch off.
//!
//! Two channels, each a signed `latest*.json` manifest on GitHub Releases:
//!
//! * **stable**: [`STABLE_ENDPOINT`], `releases/latest/download/latest.json`. GitHub resolves
//!   `latest` to the newest release that is *not* marked pre-release, so pre-releases never
//!   reach this channel.
//! * **beta**: [`BETA_ENDPOINT`]. GitHub has no "latest including pre-releases" redirect, so
//!   every release (stable or pre-release) also re-uploads `latest-beta.json` to one fixed
//!   release tagged `channel-beta` (`gh release upload channel-beta latest-beta.json
//!   --clobber`, printed by `scripts/release.ps1` and done by `release.yml`). Beta users get
//!   whatever was published last, stable releases included.
//!
//! Manifests and installers are verified against the public key in `tauri.conf.json`
//! (`plugins.updater.pubkey`); the private key never enters the repository. Neither command
//! runs on its own: the UI decides when to check (a click, or at most once a day while "check
//! for updates" is on) and when to install.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Signed manifest of the newest stable release.
pub const STABLE_ENDPOINT: &str =
    "https://github.com/LayanJarrar1/Riptide/releases/latest/download/latest.json";

/// Signed manifest of the newest release of any kind (pre-releases included).
pub const BETA_ENDPOINT: &str =
    "https://github.com/LayanJarrar1/Riptide/releases/download/channel-beta/latest-beta.json";

/// GitHub release page of `version`, or `None` when it is not a plain semver string (the value
/// comes from the webview and ends up in a URL we open).
pub fn release_url(version: &str) -> Option<String> {
    let ok = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    ok.then(|| format!("https://github.com/LayanJarrar1/Riptide/releases/tag/v{version}"))
}

/// Opens the release page of `version` in the default browser ("What's new").
#[tauri::command]
pub fn open_release_page<R: Runtime>(app: AppHandle<R>, version: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = release_url(&version).ok_or_else(|| "invalid version".to_string())?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Which releases the user wants to be offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Releases not marked pre-release.
    Stable,
    /// Every release, pre-releases included.
    Beta,
}

/// `true` when `version` (`0.1.0-alpha.4`) is a semver pre-release: a `-` before any `+build`
/// metadata.
pub fn is_prerelease(version: &str) -> bool {
    version
        .split('+')
        .next()
        .is_some_and(|core| core.contains('-'))
}

impl Channel {
    /// The channel a build of `current_version` really follows. Every release so far is a
    /// GitHub pre-release, so the stable feed (`releases/latest`) never has anything for a
    /// pre-release build: those always use the beta feed, whatever was saved. A stable build
    /// keeps the saved channel. The saved setting itself is not changed.
    pub fn effective(self, current_version: &str) -> Channel {
        if is_prerelease(current_version) {
            Channel::Beta
        } else {
            self
        }
    }

    /// The manifest URL of this channel.
    pub fn endpoint(self) -> &'static str {
        match self {
            Channel::Stable => STABLE_ENDPOINT,
            Channel::Beta => BETA_ENDPOINT,
        }
    }
}

/// What the UI shows about an available update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    /// The offered version (semver, e.g. `0.1.0-alpha.3`).
    pub version: String,
    /// The running version.
    pub current_version: String,
    /// Release notes from the manifest, if any.
    pub notes: Option<String>,
    /// Publication date from the manifest, if any.
    pub date: Option<String>,
}

impl UpdateInfo {
    fn from_update(update: &Update) -> UpdateInfo {
        UpdateInfo {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone().filter(|b| !b.trim().is_empty()),
            date: update.date.map(|d| d.to_string()),
        }
    }
}

/// The update found by the last [`check_update`], kept for [`install_update`].
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

fn to_message(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Asks the channel's manifest whether a newer version exists; `None` when up to date.
#[tauri::command]
pub async fn check_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, PendingUpdate>,
    channel: Channel,
) -> Result<Option<UpdateInfo>, String> {
    let requested = channel;
    let channel = channel.effective(&app.package_info().version.to_string());
    if channel != requested {
        tracing::info!(
            ?requested,
            ?channel,
            "pre-release build: using the beta feed"
        );
    }
    let endpoint = url::Url::parse(channel.endpoint()).map_err(to_message)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(to_message)?
        .build()
        .map_err(to_message)?;
    let found = updater.check().await.map_err(|e| {
        tracing::warn!(error = %e, ?channel, "update check failed");
        to_message(e)
    })?;
    let info = found.as_ref().map(UpdateInfo::from_update);
    tracing::info!(?channel, available = ?info.as_ref().map(|i| &i.version), "update check");
    if let Ok(mut slot) = pending.0.lock() {
        *slot = found;
    }
    Ok(info)
}

/// Downloads the update found by the last check, verifies its signature, stops the engine at
/// a checkpoint (transfers resume after the restart) and runs the installer, which replaces
/// the app and starts it again. Does not return on success.
#[tauri::command]
pub async fn install_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .map_err(to_message)?
        .take()
        .ok_or_else(|| "no update to install; check again".to_string())?;
    tracing::info!(version = %update.version, "downloading update");
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(to_message)?;
    if let Some(engine) = app.try_state::<riptide_engine::Engine>() {
        engine.inner().clone().shutdown().await;
    }
    tracing::info!(version = %update.version, "installing update");
    update.install(bytes).map_err(to_message)?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_url_accepts_only_plain_versions() {
        assert_eq!(
            release_url("0.1.0-alpha.3").as_deref(),
            Some("https://github.com/LayanJarrar1/Riptide/releases/tag/v0.1.0-alpha.3")
        );
        assert_eq!(release_url(""), None);
        assert_eq!(release_url("1.0/../x"), None);
        assert_eq!(release_url("1.0 --flag"), None);
    }

    #[test]
    fn channels_parse_from_the_ui_names_and_point_at_the_repo() {
        let stable: Channel = serde_json::from_str("\"stable\"").unwrap();
        let beta: Channel = serde_json::from_str("\"beta\"").unwrap();
        assert_eq!(stable, Channel::Stable);
        assert_eq!(beta, Channel::Beta);
        assert!(serde_json::from_str::<Channel>("\"nightly\"").is_err());
        for channel in [Channel::Stable, Channel::Beta] {
            let url = url::Url::parse(channel.endpoint()).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("github.com"));
            assert!(url.path().starts_with("/LayanJarrar1/Riptide/releases/"));
        }
        assert!(STABLE_ENDPOINT.ends_with("/latest/download/latest.json"));
        assert!(BETA_ENDPOINT.ends_with("/channel-beta/latest-beta.json"));
    }

    #[test]
    fn pre_release_builds_always_follow_the_beta_feed() {
        assert!(is_prerelease("0.1.0-alpha.4"));
        assert!(is_prerelease("1.0.0-rc.1+build.7"));
        assert!(!is_prerelease("1.2.3"));
        assert!(!is_prerelease("1.2.3+meta-with-dash"));
        assert_eq!(Channel::Stable.effective("0.1.0-alpha.4"), Channel::Beta);
        assert_eq!(Channel::Beta.effective("0.1.0-alpha.4"), Channel::Beta);
        // a stable build keeps what the user saved
        assert_eq!(Channel::Stable.effective("1.0.0"), Channel::Stable);
        assert_eq!(Channel::Beta.effective("1.0.0"), Channel::Beta);
        assert_eq!(
            Channel::Stable.effective("0.1.0-alpha.4").endpoint(),
            BETA_ENDPOINT
        );
    }

    #[test]
    fn config_carries_the_public_key_and_the_stable_endpoint() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert!(updater["pubkey"].as_str().is_some_and(|k| k.len() > 40));
        assert_eq!(updater["endpoints"][0], STABLE_ENDPOINT);
        assert_eq!(updater["windows"]["installMode"], "passive");
    }
}
