//! Self-update through `tauri-plugin-updater`, the only call Riptide makes on its own
//! (everything else starts with the user), and one the user can switch off.
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
    /// What the UI shows about `update`.
    pub fn from_update(update: &Update) -> UpdateInfo {
        UpdateInfo {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone().filter(|b| !b.trim().is_empty()),
            date: update.date.map(|d| d.to_string()),
        }
    }
}

/// Why an update step failed. The UI turns the kind into a plain-language message and decides
/// whether "Retry" makes sense; `message` is the technical detail (it goes to the log, never
/// to a toast).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateErrorKind {
    /// No connection to the update server (offline, DNS, refused, timeout).
    Offline,
    /// The server answered, but not with a release (HTTP 4xx/5xx, nothing published).
    Server,
    /// The answer is not a valid manifest (invalid JSON, missing fields, no entry for Windows).
    Manifest,
    /// The package is not signed by Riptide's key (signature missing, malformed or not matching
    /// the bytes). Nothing is installed.
    Signature,
    /// The package could not be downloaded completely (connection lost, wrong status).
    Download,
    /// The installer could not be started.
    Install,
    /// Transfers are running and the caller did not ask to interrupt them.
    DownloadsActive,
    /// There is no update to act on (check first).
    NoUpdate,
}

/// A failed update step, as the webview receives it (`{ kind, message }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateError {
    /// What went wrong, for the UI.
    pub kind: UpdateErrorKind,
    /// Technical detail for the log.
    pub message: String,
}

impl UpdateError {
    /// An error of `kind` with technical `message`.
    pub fn new(kind: UpdateErrorKind, message: impl std::fmt::Display) -> Self {
        Self {
            kind,
            message: message.to_string(),
        }
    }
}

/// Which step of the flow an error comes from; the plugin reports a dropped connection and a bad
/// manifest with the same error types, so the step disambiguates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Check,
    Download,
    Install,
}

fn classify(phase: Phase, e: &tauri_plugin_updater::Error) -> UpdateErrorKind {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(r) if r.is_connect() || r.is_timeout() || r.is_request() => {
            UpdateErrorKind::Offline
        }
        E::Reqwest(r) if r.is_decode() && phase == Phase::Check => UpdateErrorKind::Manifest,
        E::Reqwest(_) | E::Network(_) if phase == Phase::Download => UpdateErrorKind::Download,
        // a check that got a non-success status ends here (the plugin reports it as "no release")
        E::ReleaseNotFound | E::Reqwest(_) | E::Network(_) => UpdateErrorKind::Server,
        E::Serialization(_)
        | E::Semver(_)
        | E::UrlParse(_)
        | E::TargetNotFound(_)
        | E::TargetsNotFound(_)
        | E::EmptyEndpoints
        | E::InsecureTransportProtocol => UpdateErrorKind::Manifest,
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => UpdateErrorKind::Signature,
        _ => match phase {
            Phase::Install => UpdateErrorKind::Install,
            Phase::Download => UpdateErrorKind::Download,
            Phase::Check => UpdateErrorKind::Server,
        },
    }
}

fn plugin_error(phase: Phase, e: tauri_plugin_updater::Error) -> UpdateError {
    let kind = classify(phase, &e);
    tracing::warn!(error = %e, ?kind, ?phase, "update step failed");
    UpdateError::new(kind, e)
}

/// `true` in builds made with the `update-e2e` feature, which alone may honour
/// `RIPTIDE_UPDATE_ENDPOINT`. A release binary cannot be redirected, whatever the environment
/// says: the code path is not compiled into it (the environment variable is only read when this
/// is `true`).
pub const E2E_ENDPOINT_OVERRIDE: bool = cfg!(feature = "update-e2e");

/// The manifest URL to ask: the channel's, unless `allow_override` and `override_url` is a
/// valid URL (test builds only; see [`E2E_ENDPOINT_OVERRIDE`]).
pub fn resolve_endpoint(
    channel: Channel,
    override_url: Option<&str>,
    allow_override: bool,
) -> Result<url::Url, UpdateError> {
    if allow_override {
        if let Some(raw) = override_url.filter(|v| !v.trim().is_empty()) {
            return url::Url::parse(raw.trim()).map_err(|e| {
                UpdateError::new(
                    UpdateErrorKind::Manifest,
                    format!("bad update endpoint: {e}"),
                )
            });
        }
    }
    url::Url::parse(channel.endpoint()).map_err(|e| UpdateError::new(UpdateErrorKind::Manifest, e))
}

/// The updater plugin, with the one rule the plugin gets subtly wrong by default: a release is
/// an update only when it has higher semver *precedence* than the running build. The default
/// uses the `semver` crate's total order, which breaks ties with the build metadata, so a feed
/// announcing `0.1.0-beta.1+other` would be offered to a running `0.1.0-beta.1` forever. Semver
/// says build metadata never counts.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new()
        .default_version_comparator(|current, release| {
            release.version.cmp_precedence(&current) == std::cmp::Ordering::Greater
        })
        .build()
}

/// Upper bound of one manifest request; without it a captive portal could hang the check.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Asks `endpoint` for a release newer than the running version (semver precedence of the
/// `semver` crate: `alpha < beta < rc < stable`, numeric identifiers numerically, build
/// metadata ignored).
pub async fn check_endpoint<R: Runtime>(
    app: &AppHandle<R>,
    endpoint: url::Url,
) -> Result<Option<Update>, UpdateError> {
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| plugin_error(Phase::Check, e))?
        .timeout(CHECK_TIMEOUT)
        .build()
        .map_err(|e| plugin_error(Phase::Check, e))?;
    updater
        .check()
        .await
        .map_err(|e| plugin_error(Phase::Check, e))
}

/// Download progress, emitted as [`PROGRESS_EVENT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UpdateProgress {
    /// Bytes received so far.
    pub downloaded: u64,
    /// Package size when the server said it.
    pub total: Option<u64>,
}

/// Event the webview listens to for the package download.
pub const PROGRESS_EVENT: &str = "update-progress";

/// Downloads `update` and verifies its signature (the plugin does both; nothing unverified is
/// ever returned). `on_progress` is called per chunk.
pub async fn download_package(
    update: &Update,
    mut on_progress: impl FnMut(UpdateProgress),
) -> Result<Vec<u8>, UpdateError> {
    let mut downloaded = 0u64;
    update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                on_progress(UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| plugin_error(Phase::Download, e))
}

/// The update found by the last check, plus its verified package once downloaded.
#[derive(Default)]
struct Slot {
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
    downloading: bool,
}

/// The update found by the last [`check_update`] and its downloaded package, kept for
/// [`download_update`] and [`install_update`].
#[derive(Default)]
pub struct PendingUpdate(Mutex<Slot>);

impl PendingUpdate {
    fn with<T>(&self, f: impl FnOnce(&mut Slot) -> T) -> T {
        let mut guard = self.0.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }

    /// Remembers the outcome of a check. A package downloaded for another version is dropped.
    pub fn set_update(&self, found: Option<Update>) {
        self.with(|slot| {
            if slot.update.as_ref().map(|u| &u.version) != found.as_ref().map(|u| &u.version) {
                slot.bytes = None;
            }
            slot.update = found;
        });
    }

    /// Whether a check found an update that is still waiting.
    pub fn has_update(&self) -> bool {
        self.with(|slot| slot.update.is_some())
    }

    /// The downloaded and signature-checked package, if there is one.
    pub fn package(&self) -> Option<Vec<u8>> {
        self.with(|slot| slot.bytes.clone())
    }

    /// Whether a download is running right now.
    pub fn is_downloading(&self) -> bool {
        self.with(|slot| slot.downloading)
    }
}

/// Asks the channel's manifest whether a newer version exists; `None` when up to date.
#[tauri::command]
pub async fn check_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, PendingUpdate>,
    channel: Channel,
) -> Result<Option<UpdateInfo>, UpdateError> {
    let requested = channel;
    let channel = channel.effective(&app.package_info().version.to_string());
    if channel != requested {
        tracing::info!(
            ?requested,
            ?channel,
            "pre-release build: using the beta feed"
        );
    }
    let override_url = if E2E_ENDPOINT_OVERRIDE {
        std::env::var("RIPTIDE_UPDATE_ENDPOINT").ok()
    } else {
        None
    };
    let endpoint = resolve_endpoint(channel, override_url.as_deref(), E2E_ENDPOINT_OVERRIDE)?;
    let found = check_endpoint(&app, endpoint).await?;
    let info = found.as_ref().map(UpdateInfo::from_update);
    tracing::info!(?channel, available = ?info.as_ref().map(|i| &i.version), "update check");
    pending.set_update(found);
    Ok(info)
}

/// Downloads the package of the update found by the last check and verifies its signature
/// (progress arrives as [`PROGRESS_EVENT`]). Nothing is installed. A failure keeps the update, so
/// "Retry" simply calls this again.
#[tauri::command]
pub async fn download_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, PendingUpdate>,
) -> Result<(), UpdateError> {
    download_pending(&app, &pending).await
}

/// [`download_update`] on an app handle and the pending slot (what the command wraps).
pub async fn download_pending<R: Runtime>(
    app: &AppHandle<R>,
    pending: &PendingUpdate,
) -> Result<(), UpdateError> {
    use tauri::Emitter;
    enum Next {
        Ready,
        Busy,
        Nothing,
        Fetch(Box<Update>),
    }
    let next = pending.with(|slot| {
        if slot.bytes.is_some() {
            Next::Ready
        } else if slot.downloading {
            Next::Busy
        } else if let Some(update) = slot.update.clone() {
            slot.downloading = true;
            Next::Fetch(Box::new(update))
        } else {
            Next::Nothing
        }
    });
    let update = match next {
        Next::Ready => return Ok(()),
        Next::Busy => {
            return Err(UpdateError::new(
                UpdateErrorKind::Download,
                "a download is already running",
            ))
        }
        Next::Nothing => {
            return Err(UpdateError::new(
                UpdateErrorKind::NoUpdate,
                "no update to download; check again",
            ))
        }
        Next::Fetch(update) => update,
    };
    tracing::info!(version = %update.version, "downloading update");
    let mut last_emit = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);
    let result = download_package(&update, |p| {
        // at most ~10 events a second, plus the last one
        if last_emit.elapsed() >= std::time::Duration::from_millis(100)
            || Some(p.downloaded) == p.total
        {
            last_emit = std::time::Instant::now();
            if let Err(e) = app.emit(PROGRESS_EVENT, p) {
                tracing::debug!(error = %e, "update progress event not delivered");
            }
        }
    })
    .await;
    pending.with(|slot| {
        slot.downloading = false;
        if let Ok(bytes) = &result {
            slot.bytes = Some(bytes.clone());
        }
    });
    result.map(|_| ())
}

/// How many of `downloads` an installation would interrupt: queued, probing, downloading or
/// verifying ones.
pub fn count_active(downloads: &[riptide_core::Download]) -> usize {
    use riptide_core::DownloadState as S;
    downloads
        .iter()
        .filter(|d| {
            matches!(
                d.state,
                S::Queued | S::Probing | S::Downloading | S::Verifying
            )
        })
        .count()
}

/// The rule that no install starts over running transfers unless the caller said so.
pub fn ensure_idle(active: usize, interrupt_downloads: bool) -> Result<(), UpdateError> {
    if active > 0 && !interrupt_downloads {
        return Err(UpdateError::new(
            UpdateErrorKind::DownloadsActive,
            format!("{active} downloads are active"),
        ));
    }
    Ok(())
}

/// Number of downloads that an installation would interrupt (see [`count_active`]). The UI offers
/// "install when downloads finish" while this is above zero.
#[tauri::command]
pub async fn active_download_count<R: Runtime>(app: AppHandle<R>) -> Result<usize, String> {
    let Some(engine) = app.try_state::<riptide_engine::Engine>() else {
        return Ok(0);
    };
    let all = engine.list().await.map_err(|e| e.to_string())?;
    Ok(count_active(&all))
}

/// The order of an install: transfers are stopped at a checkpoint first (the installer replaces
/// the files and, on Windows, ends this process), then `install` runs. If it fails, `revive`
/// brings the engine back and the error goes to the caller, so a failed update never leaves the
/// app running with a dead engine.
async fn install_in_order<S, SF, R, RF>(
    shutdown: S,
    install: impl FnOnce() -> Result<(), UpdateError>,
    revive: R,
) -> Result<(), UpdateError>
where
    S: FnOnce() -> SF,
    SF: std::future::Future<Output = ()>,
    R: FnOnce() -> RF,
    RF: std::future::Future<Output = ()>,
{
    shutdown().await;
    match install() {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!(error = %e.message, "update install failed; restarting the engine");
            revive().await;
            Err(e)
        }
    }
}

/// Installs the downloaded update (downloading it first if that has not happened), stops the
/// engine at a checkpoint (transfers resume after the restart) and runs the installer, which
/// replaces the app and starts it again. Does not return on success; a failed install returns its
/// error and leaves the engine running.
///
/// Never interrupts transfers on its own: with downloads running and `interrupt_downloads`
/// false it fails with [`UpdateErrorKind::DownloadsActive`] before touching anything.
#[tauri::command]
pub async fn install_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, PendingUpdate>,
    interrupt_downloads: bool,
) -> Result<(), UpdateError> {
    let engine = app
        .try_state::<riptide_engine::Engine>()
        .map(|e| e.inner().clone());
    if let Some(engine) = &engine {
        let all = engine.list().await.map_err(|e| {
            UpdateError::new(
                UpdateErrorKind::Install,
                format!("downloads not readable: {e}"),
            )
        })?;
        ensure_idle(count_active(&all), interrupt_downloads)?;
    }
    download_pending(&app, &pending).await?;
    let (update, bytes) = pending.with(|slot| (slot.update.clone(), slot.bytes.clone()));
    let (Some(update), Some(bytes)) = (update, bytes) else {
        return Err(UpdateError::new(
            UpdateErrorKind::NoUpdate,
            "no update to install; check again",
        ));
    };
    tracing::info!(version = %update.version, "installing update");
    install_in_order(
        || async {
            if let Some(engine) = &engine {
                engine.shutdown().await;
            }
        },
        || {
            update
                .install(&bytes)
                .map_err(|e| plugin_error(Phase::Install, e))
        },
        || async {
            if let Some(engine) = &engine {
                if let Err(e) = engine.revive().await {
                    tracing::error!(error = %e, "the engine could not be restarted after a failed update");
                }
                crate::torrent::start_if_needed(engine).await;
            }
        },
    )
    .await?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn a_failed_install_restarts_what_was_shut_down() {
        use std::sync::Mutex;
        let log = Mutex::new(Vec::<&str>::new());
        let r = install_in_order(
            || async { log.lock().unwrap().push("shutdown") },
            || {
                log.lock().unwrap().push("install");
                Err(UpdateError::new(UpdateErrorKind::Install, "disk full"))
            },
            || async { log.lock().unwrap().push("revive") },
        )
        .await;
        assert_eq!(
            r,
            Err(UpdateError::new(UpdateErrorKind::Install, "disk full"))
        );
        assert_eq!(*log.lock().unwrap(), ["shutdown", "install", "revive"]);

        // a successful install leaves the engine down: the app restarts into the new version
        log.lock().unwrap().clear();
        let r = install_in_order(
            || async { log.lock().unwrap().push("shutdown") },
            || {
                log.lock().unwrap().push("install");
                Ok(())
            },
            || async { log.lock().unwrap().push("revive") },
        )
        .await;
        assert_eq!(r, Ok(()));
        assert_eq!(*log.lock().unwrap(), ["shutdown", "install"]);
    }

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

    #[test]
    fn the_channel_decides_the_feed_and_only_test_builds_can_redirect_it() {
        let url = |c: Channel| resolve_endpoint(c, None, false).unwrap().to_string();
        assert_eq!(url(Channel::Stable), STABLE_ENDPOINT);
        assert_eq!(url(Channel::Beta), BETA_ENDPOINT);
        let local = Some("http://127.0.0.1:9/latest.json");
        // a release build ignores the override
        assert_eq!(
            resolve_endpoint(Channel::Beta, local, false)
                .unwrap()
                .as_str(),
            BETA_ENDPOINT
        );
        // a test build honours it
        assert_eq!(
            resolve_endpoint(Channel::Beta, local, true)
                .unwrap()
                .as_str(),
            "http://127.0.0.1:9/latest.json"
        );
        assert_eq!(
            resolve_endpoint(Channel::Beta, Some(""), true)
                .unwrap()
                .as_str(),
            BETA_ENDPOINT
        );
        assert_eq!(
            resolve_endpoint(Channel::Beta, Some("not a url"), true)
                .unwrap_err()
                .kind,
            UpdateErrorKind::Manifest
        );
        // the switch is a compile-time feature of this crate, off unless asked for
        assert_eq!(E2E_ENDPOINT_OVERRIDE, cfg!(feature = "update-e2e"));
    }

    #[test]
    fn running_downloads_block_an_install_unless_the_caller_interrupts() {
        use riptide_core::{Download, DownloadId, DownloadState as S};
        let mk = |i: i64, state: S| {
            let mut d = Download::new(DownloadId(i), "http://x/f", "f", "D:/x", 0);
            d.state = state;
            d
        };
        let none = [
            mk(1, S::Completed),
            mk(2, S::Paused),
            mk(3, S::Failed { retryable: true }),
            mk(4, S::Cancelled),
        ];
        assert_eq!(count_active(&none), 0);
        let some = [
            mk(1, S::Downloading),
            mk(2, S::Queued),
            mk(3, S::Probing),
            mk(4, S::Verifying),
            mk(5, S::Completed),
        ];
        assert_eq!(count_active(&some), 4);

        assert!(ensure_idle(0, false).is_ok());
        assert!(ensure_idle(4, true).is_ok());
        let e = ensure_idle(4, false).unwrap_err();
        assert_eq!(e.kind, UpdateErrorKind::DownloadsActive);
    }

    #[test]
    fn errors_reach_the_webview_as_kind_and_message() {
        let e = UpdateError::new(UpdateErrorKind::DownloadsActive, "2 downloads are active");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({ "kind": "downloads_active", "message": "2 downloads are active" })
        );
        for (kind, name) in [
            (UpdateErrorKind::Offline, "offline"),
            (UpdateErrorKind::Server, "server"),
            (UpdateErrorKind::Manifest, "manifest"),
            (UpdateErrorKind::Signature, "signature"),
            (UpdateErrorKind::Download, "download"),
            (UpdateErrorKind::Install, "install"),
            (UpdateErrorKind::NoUpdate, "no_update"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
        }
    }

    #[test]
    fn plugin_errors_map_to_the_right_kind_per_step() {
        use tauri_plugin_updater::Error as E;
        assert_eq!(
            classify(Phase::Check, &E::ReleaseNotFound),
            UpdateErrorKind::Server
        );
        assert_eq!(
            classify(Phase::Check, &E::TargetNotFound("windows-x86_64".into())),
            UpdateErrorKind::Manifest
        );
        assert_eq!(
            classify(Phase::Download, &E::Network("status 404".into())),
            UpdateErrorKind::Download
        );
        assert_eq!(
            classify(Phase::Download, &E::SignatureUtf8("x".into())),
            UpdateErrorKind::Signature
        );
        assert_eq!(
            classify(Phase::Install, &E::MissingSignedVersion),
            UpdateErrorKind::Signature
        );
        assert_eq!(
            classify(Phase::Install, &E::TempDirNotFound),
            UpdateErrorKind::Install
        );
    }
}
