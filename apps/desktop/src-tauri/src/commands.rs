//! `#[tauri::command]`s the webview may invoke. Thin: every command is one engine call, errors
//! become strings (already redacted), and nothing received here is used as a path except the
//! folder the user picked in the app's own dialogs (hard rule 5 is about the browser bridge).
//!
//! Commands live in their own module (not at the crate root) because Tauri's command macro
//! both `#[macro_export]`s a helper macro and re-exports it with the function's visibility;
//! a `pub` command at the crate root therefore defines the same name twice (E0255).

use riptide_core::{Download, DownloadId, DownloadRequest, Segment};
use riptide_engine::{Engine, EngineSettings, ProbeInfo};
use serde::Serialize;
use tauri::State;

/// Milestone label embedded in the binary.
pub const BUILD_MILESTONE: &str = "M1";

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
    /// Milestone the binary was built from.
    pub build: &'static str,
}

/// Returns the application's name, version and milestone.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        build: BUILD_MILESTONE,
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
pub async fn add_download(engine: State<'_, Engine>, request: DownloadRequest) -> CmdResult<i64> {
    engine.add(request).await.map(|id| id.0).map_err(err)
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

/// Pauses a download.
#[tauri::command]
pub async fn pause_download(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.pause(DownloadId(id)).await.map_err(err)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_name_and_version() {
        let info = app_info();
        assert_eq!(info.name, "Riptide");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.version.is_empty());
        assert_eq!(info.build, "M1");
    }

    #[test]
    fn app_info_serializes_with_the_field_names_the_ui_expects() {
        let json = serde_json::to_value(app_info()).unwrap();
        assert_eq!(json["name"], "Riptide");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["build"], "M1");
        assert_eq!(json.as_object().unwrap().len(), 3);
    }
}
