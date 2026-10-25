//! `#[tauri::command]`s the webview may invoke. Thin: every command is one engine call, errors
//! become strings (already redacted), and nothing received here is used as a path except the
//! folder the user picked in the app's own dialogs (the browser bridge has its own, stricter rules).
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `app_info` | – | name, version, build label |
//! | `list_downloads` | – | every `Download` |
//! | `get_download` | `id` | one `Download` or `null` |
//! | `get_segments` | `id` | the download's `Segment`s |
//! | `add_download` | `request: DownloadRequest` | new id |
//! | `probe_url` | `url`, `referrer?` | `ProbeInfo` |
//! | `find_duplicate` | `url`, `filename?`, `dir?` | the matching `Download` or `null` |
//! | `batch_expand` | `pattern` | the URLs of a pattern like `file[001-120].zip`, or a typed `BatchError` |
//! | `batch_import` | `text` | `UrlList` (URLs of a pasted/loaded list, plus what was ignored) |
//! | `add_batch` | `urls`, `options: BatchOptions` | `BatchOutcome` (added ids, skipped duplicates) |
//! | `pick_text_file` / `save_text_file` | – / `name`, `text`, `kind?` (`txt` default, `md`, `bib`, `ris`) | the chosen file's text or `null` / whether it was saved |
//! | `open_web_url` | `url` (http/https, ≤ 2048 bytes, no control characters) | – (opens the default browser) |
//! | `pause_download` / `resume_download` / `cancel_download` | `id` | – |
//! | `restart_download` | `id` | – (starts over from byte zero) |
//! | `remove_download` | `id`, `deleteFile` | – |
//! | `pause_all` / `resume_all` | – | – |
//! | `get_engine_settings` / `set_engine_settings` | – / `settings` | `EngineSettings` / – |
//! | `set_speed_limit` | `id`, `limit` (bytes/s or `null`) | – (applies to a running download) |
//! | `set_exit_guard` / `quit_app` | `enabled` / – | – (see [`crate::exit`]) |
//! | `disk_free` | `path` | free bytes on that folder's volume |
//! | `open_file` / `show_in_folder` | `path` | – |
//! | `pick_folder` | – | the chosen folder or `null` |
//! | `integration_status` / `register_browser_host` / `unregister_browser_host` | – | `IntegrationStatus` |
//! | `extension_info` / `open_extension_folder` / `open_extensions_page` | – / `browser` | see [`crate::extension`] |
//!
//! Commands live in their own module (not at the crate root) because Tauri's command macro
//! both `#[macro_export]`s a helper macro and re-exports it with the function's visibility;
//! a `pub` command at the crate root therefore defines the same name twice (E0255).

use std::path::{Path, PathBuf};

use riptide_core::{
    BatchError, BatchOptions, Download, DownloadId, DownloadRequest, Segment, UrlList,
};
use riptide_engine::{BatchOutcome, Engine, EngineSettings, ProbeInfo};
use serde::Serialize;
use tauri::State;

use crate::integration::{self, browser_registry, IntegrationStatus, Layout, Registry};

/// Build label embedded in the binary: `release` for optimised builds (what ships), `dev` for
/// debug builds. The version is reported separately.
pub const BUILD_LABEL: &str = if cfg!(debug_assertions) {
    "dev"
} else {
    "release"
};

/// Product name shown in the window title and the About screen.
pub const PRODUCT_NAME: &str = "Riptide";

/// Commands answer with a plain string on failure; the UI shows it as-is.
type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Static facts about the running application, reported to the UI once at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    /// Product name.
    pub name: &'static str,
    /// Crate version (`CARGO_PKG_VERSION`), inherited from the workspace.
    pub version: &'static str,
    /// Build label of the binary.
    pub build: &'static str,
}

/// Returns the application's name, version and build label.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        build: BUILD_LABEL,
    }
}

/// Every download, oldest first.
#[tauri::command]
pub async fn list_downloads(engine: State<'_, Engine>) -> CmdResult<Vec<Download>> {
    engine.list().await.map_err(err)
}

/// One download.
#[tauri::command]
pub async fn get_download(engine: State<'_, Engine>, id: i64) -> CmdResult<Option<Download>> {
    engine.get(DownloadId(id)).await.map_err(err)
}

/// Segments of a download.
#[tauri::command]
pub async fn get_segments(engine: State<'_, Engine>, id: i64) -> CmdResult<Vec<Segment>> {
    engine.segments(DownloadId(id)).await.map_err(err)
}

/// Adds a download; returns its id.
#[tauri::command]
pub async fn add_download(
    engine: State<'_, Engine>,
    request: DownloadRequest,
    allow_duplicates: Option<bool>,
) -> CmdResult<i64> {
    engine
        .add_checked(request, allow_duplicates.unwrap_or(false))
        .await
        .map(|id| id.0)
        .map_err(err)
}

/// Probes a URL for the add dialog's preview.
#[tauri::command]
pub async fn probe_url(
    engine: State<'_, Engine>,
    url: String,
    referrer: Option<String>,
) -> CmdResult<ProbeInfo> {
    engine
        .probe(&url, None, referrer.as_deref())
        .await
        .map_err(err)
}

/// A download that would be a duplicate of `url` (fragment ignored) or of `filename` in `dir`;
/// `null` when there is none. The add dialog asks before adding.
#[tauri::command]
pub async fn find_duplicate(
    engine: State<'_, Engine>,
    url: String,
    filename: Option<String>,
    dir: Option<String>,
) -> CmdResult<Option<Download>> {
    let parsed = riptide_engine::parse_http_url(&url).map_err(err)?;
    engine
        .find_duplicate(
            &parsed,
            filename.as_deref().filter(|f| !f.trim().is_empty()),
            dir.as_deref()
                .filter(|d| !d.trim().is_empty())
                .map(Path::new),
        )
        .await
        .map_err(err)
}

/// The URLs a batch pattern (`file[001-120].zip`, `img_[a-z].jpg`, ...) stands for, at most
/// [`riptide_core::MAX_EXPANSION`]; the error is typed so the UI can word it in either language.
#[tauri::command]
pub fn batch_expand(pattern: String) -> Result<Vec<String>, BatchError> {
    riptide_core::batch::expand(&pattern)
}

/// The URLs of a pasted or loaded list (one per line, `#` comments, http(s) only, no repeats).
#[tauri::command]
pub fn batch_import(text: String) -> UrlList {
    riptide_core::import_url_list(&text)
}

/// Adds the checked URLs of a batch in one transaction; duplicates are skipped and reported.
#[tauri::command]
pub async fn add_batch(
    engine: State<'_, Engine>,
    urls: Vec<String>,
    options: BatchOptions,
) -> CmdResult<BatchOutcome> {
    engine.add_batch(options.requests(&urls)).await.map_err(err)
}

/// Largest list file the import accepts (a million short URLs is far below this).
const MAX_LIST_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Native "open" dialog for a `.txt` URL list; answers the file's text, `None` on cancel. The
/// path comes from the user's own pick in the OS dialog and is only read, never executed.
#[tauri::command]
pub async fn pick_text_file(app: tauri::AppHandle) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Text", &["txt", "list", "lst", "csv"])
            .blocking_pick_file()
    })
    .await
    .map_err(err)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let len = tokio::fs::metadata(&path).await.map_err(err)?.len();
    if len > MAX_LIST_FILE_BYTES {
        return Err("the file is too large to be a URL list".into());
    }
    let bytes = tokio::fs::read(&path).await.map_err(err)?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Dialog filter name and extensions for a `save_text_file` kind; unknown kinds mean `txt`.
fn save_filter(kind: Option<&str>) -> (&'static str, &'static [&'static str]) {
    match kind {
        Some("md") => ("Markdown", &["md"]),
        Some("bib") => ("BibTeX", &["bib"]),
        Some("ris") => ("RIS", &["ris"]),
        _ => ("Text", &["txt"]),
    }
}

/// Longest URL `open_web_url` accepts.
const MAX_WEB_URL_LEN: usize = 2048;

/// Validates a URL that is about to be opened in the browser: http(s) only, no control
/// characters, at most [`MAX_WEB_URL_LEN`] bytes. Returns the URL unchanged when it passes.
fn checked_web_url(url: &str) -> Result<String, String> {
    if url.len() > MAX_WEB_URL_LEN {
        return Err("the link is too long".into());
    }
    if url.chars().any(char::is_control) {
        return Err("the link contains control characters".into());
    }
    if !riptide_core::batch::is_http_url(url) {
        return Err("only http and https links can be opened".into());
    }
    Ok(url.to_owned())
}

/// Opens an http(s) link in the default browser (after validation; see [`checked_web_url`]).
#[tauri::command]
pub fn open_web_url(app: tauri::AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = checked_web_url(&url)?;
    app.opener().open_url(url, None::<&str>).map_err(err)
}

/// Native "save" dialog; writes `text` to the chosen file. `false` when the user cancels.
/// `kind` picks the dialog filter: `txt` (default), `md`, `bib` or `ris`.
#[tauri::command]
pub async fn save_text_file(
    app: tauri::AppHandle,
    name: String,
    text: String,
    kind: Option<String>,
) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let suggested = riptide_core::sanitize_filename(&name);
    let (filter_name, extensions) = save_filter(kind.as_deref());
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(suggested)
            .add_filter(filter_name, extensions)
            .blocking_save_file()
    })
    .await
    .map_err(err)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(false);
    };
    tokio::fs::write(&path, text).await.map_err(err)?;
    Ok(true)
}

/// Pauses a download.
#[tauri::command]
pub async fn pause_download(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.pause(DownloadId(id)).await.map_err(err)
}

/// Starts a download over from byte zero (progress and the part file are discarded).
#[tauri::command]
pub async fn restart_download(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.restart(DownloadId(id)).await.map_err(err)
}

/// Free bytes on the volume that holds `path` (the folder need not exist yet).
#[tauri::command]
pub async fn disk_free(path: String) -> CmdResult<u64> {
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || riptide_engine::free_space(&path))
        .await
        .map_err(err)?
        .map_err(err)
}

/// Resumes or retries a download.
#[tauri::command]
pub async fn resume_download(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.resume(DownloadId(id)).await.map_err(err)
}

/// Cancels a download.
#[tauri::command]
pub async fn cancel_download(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.cancel(DownloadId(id)).await.map_err(err)
}

/// Removes a download from the list, optionally deleting the finished file.
#[tauri::command]
pub async fn remove_download(
    engine: State<'_, Engine>,
    id: i64,
    delete_file: bool,
) -> CmdResult<()> {
    engine
        .remove(DownloadId(id), delete_file)
        .await
        .map_err(err)
}

/// Pauses everything.
#[tauri::command]
pub async fn pause_all(engine: State<'_, Engine>) -> CmdResult<()> {
    engine.pause_all().await.map_err(err)
}

/// Resumes everything.
#[tauri::command]
pub async fn resume_all(engine: State<'_, Engine>) -> CmdResult<()> {
    engine.resume_all().await.map_err(err)
}

/// The engine's settings.
#[tauri::command]
pub fn get_engine_settings(engine: State<'_, Engine>) -> EngineSettings {
    engine.settings()
}

/// Replaces the engine's settings.
#[tauri::command]
pub async fn set_engine_settings(
    engine: State<'_, Engine>,
    settings: EngineSettings,
) -> CmdResult<()> {
    engine.set_settings(settings).await.map_err(err)
}

/// Sets one download's own speed limit (bytes per second, `null` = unlimited).
#[tauri::command]
pub async fn set_speed_limit(
    engine: State<'_, Engine>,
    id: i64,
    limit: Option<u64>,
) -> CmdResult<()> {
    engine
        .set_speed_limit(DownloadId(id), limit)
        .await
        .map_err(err)
}

/// Opens a finished file with its default application.
#[tauri::command]
pub fn open_file(app: tauri::AppHandle, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(path, None::<&str>).map_err(err)
}

/// Shows a file in Explorer.
#[tauri::command]
pub fn show_in_folder(app: tauri::AppHandle, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(path).map_err(err)
}

/// Native folder picker; `None` when the user cancels.
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
            .await
            .map_err(err)?;
    Ok(picked.map(|p| p.to_string()))
}

/// Runs `op` on the registry of this platform off the async runtime; the "unsupported" status
/// where there is none.
async fn with_integration<F>(op: F) -> CmdResult<IntegrationStatus>
where
    F: FnOnce(&Layout, &mut dyn Registry) -> CmdResult<IntegrationStatus> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let Some(mut registry) = browser_registry() else {
            return Ok(IntegrationStatus::unsupported());
        };
        let layout = Layout::for_this_app()?;
        op(&layout, registry.as_mut())
    })
    .await
    .map_err(err)?
}

/// Whether the browser host is registered (Settings → Integration).
#[tauri::command]
pub async fn integration_status() -> CmdResult<IntegrationStatus> {
    with_integration(|layout, registry| Ok(integration::status(layout, registry))).await
}

/// Registers the Native Messaging host for Chrome, Edge and Firefox (per user, no elevation).
#[tauri::command]
pub async fn register_browser_host() -> CmdResult<IntegrationStatus> {
    with_integration(integration::register).await
}

/// Removes the Native Messaging host registration and remembers that the user turned it off.
#[tauri::command]
pub async fn unregister_browser_host() -> CmdResult<IntegrationStatus> {
    with_integration(integration::unregister).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_urls_accept_only_http_and_https() {
        assert_eq!(
            checked_web_url("https://example.org/a?b=1").as_deref(),
            Ok("https://example.org/a?b=1")
        );
        assert!(checked_web_url("http://example.org").is_ok());
        for bad in [
            "",
            "javascript:alert(1)",
            "file:///C:/Windows/win.ini",
            "data:text/html,<b>x</b>",
            "https://exa\u{0}mple.org",
            "https://example.org/\r\nx",
            "ftp://example.org",
        ] {
            assert!(checked_web_url(bad).is_err(), "{bad:?} must be rejected");
        }
        let long = format!("https://example.org/{}", "a".repeat(MAX_WEB_URL_LEN));
        assert!(checked_web_url(&long).is_err());
    }

    #[test]
    fn save_filters_fall_back_to_text() {
        assert_eq!(save_filter(None).1, &["txt"]);
        assert_eq!(save_filter(Some("md")).1, &["md"]);
        assert_eq!(save_filter(Some("bib")).1, &["bib"]);
        assert_eq!(save_filter(Some("ris")).1, &["ris"]);
        assert_eq!(save_filter(Some("exe")).1, &["txt"]);
    }

    #[test]
    fn app_info_reports_name_and_version() {
        let info = app_info();
        assert_eq!(info.name, "Riptide");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.version.is_empty());
        assert_eq!(info.build, BUILD_LABEL);
    }

    #[test]
    fn the_build_label_follows_the_profile() {
        // an optimised build says `release`, never `dev`
        assert_eq!(BUILD_LABEL == "dev", cfg!(debug_assertions));
        assert!(BUILD_LABEL == "dev" || BUILD_LABEL == "release");
    }

    #[test]
    fn app_info_serializes_with_the_field_names_the_ui_expects() {
        let json = serde_json::to_value(app_info()).unwrap();
        assert_eq!(json["name"], "Riptide");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["build"], BUILD_LABEL);
        assert_eq!(json.as_object().unwrap().len(), 3);
    }
}
