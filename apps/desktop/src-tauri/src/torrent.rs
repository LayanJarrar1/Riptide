//! BitTorrent commands for the webview. Thin, like the rest of [`crate::commands`]: each command
//! is one engine call; the engine (`riptide_engine::Engine`, with `riptide-torrent` behind it)
//! does the work and owns every rule about names and folders.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `torrent_preview` | `source` (magnet link or `.torrent` path), `dir?` | `TorrentPreviewInfo` (files, token) |
//! | `add_torrent` | `add: TorrentAdd` | new download id |
//! | `torrent_web_seed_add` | `request: WebSeedRequest` | ids of the new downloads |
//! | `list_torrents` | – | `TorrentInfo` of every torrent |
//! | `torrent_live` | `id` | `TorrentLive` (per-file progress, peers, speeds, ratio) |
//! | `pick_torrent_file` | – | path of the `.torrent` the user chose in the OS dialog, or `null` |
//!
//! The torrent engine starts lazily (the first preview, add, resume or verify) or at launch when
//! a torrent is active (downloading, queued, checking, or seeding by the user choice), so an app
//! with nothing to transfer opens no listening port and contacts no tracker or DHT node. Pause, resume, cancel, remove and "start over" (a full
//! re-check) are the ordinary download commands.

use riptide_core::{
    DownloadId, TorrentAdd, TorrentInfo, TorrentLive, TorrentPreviewInfo, TorrentSourceRequest,
};
use riptide_engine::Engine;
use tauri::State;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A magnet link or a `.torrent` file (path) as a [`TorrentSourceRequest`]; the add dialog's
/// text box hands over whatever the user pasted.
pub fn source_from_text(text: &str) -> Option<TorrentSourceRequest> {
    let t = text.trim();
    if t.len() >= 8 && t[..8].eq_ignore_ascii_case("magnet:?") {
        return Some(TorrentSourceRequest::Magnet { uri: t.to_string() });
    }
    None
}

/// Starts the torrent engine at launch only when a torrent needs the network right now (see
/// [`Engine::enable_torrents_if_active`]), in the background. Failing to start is logged, not
/// fatal: the rest of the app keeps working and the next torrent command tries again.
pub async fn start_if_needed(engine: &Engine) {
    if let Err(e) = engine.enable_torrents_if_active().await {
        tracing::error!(error = %e, "the torrent engine could not start");
    }
}

/// Resolves a source into its file list (see [`Engine::torrent_preview`]).
pub async fn preview(
    engine: &Engine,
    source: TorrentSourceRequest,
    dir: Option<String>,
) -> CmdResult<TorrentPreviewInfo> {
    engine.enable_torrents().await.map_err(err)?;
    let dir = dir
        .filter(|d| !d.trim().is_empty())
        .map(std::path::PathBuf::from);
    engine.torrent_preview(source, dir).await.map_err(err)
}

/// Adds the previewed torrent with the user's choice (see [`Engine::add_torrent`]).
pub async fn add(engine: &Engine, add: TorrentAdd) -> CmdResult<i64> {
    engine.enable_torrents().await.map_err(err)?;
    engine.add_torrent(add).await.map(|id| id.0).map_err(err)
}

/// The web-seed download request of the add dialog.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSeedRequest {
    /// Token of the preview.
    pub token: String,
    /// Chosen file indices (`null` = every file).
    pub only_files: Option<Vec<u32>>,
    /// Destination folder (`null` = the default).
    pub dir: Option<String>,
    /// Add paused.
    #[serde(default)]
    pub start_paused: bool,
}

/// What came of it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSeedResult {
    /// Ids of the new downloads.
    pub added: Vec<i64>,
    /// Files left out (not an http(s) address, or aimed at a private network).
    pub refused: usize,
}

/// Downloads the chosen files of a previewed torrent from its web seed as ordinary downloads
/// (see [`Engine::torrent_web_seed_add`]).
pub async fn web_seed_add(engine: &Engine, request: WebSeedRequest) -> CmdResult<WebSeedResult> {
    let dir = request
        .dir
        .filter(|d| !d.trim().is_empty())
        .map(std::path::PathBuf::from);
    let done = engine
        .torrent_web_seed_add(
            &request.token,
            request.only_files,
            dir,
            request.start_paused,
        )
        .await
        .map_err(err)?;
    Ok(WebSeedResult {
        added: done.added.iter().map(|d| d.0).collect(),
        refused: done.refused,
    })
}

/// Downloads the chosen files of a previewed torrent from its first web seed.
#[tauri::command]
pub async fn torrent_web_seed_add(
    engine: State<'_, Engine>,
    request: WebSeedRequest,
) -> CmdResult<WebSeedResult> {
    web_seed_add(&engine, request).await
}

/// Resolves a magnet link or a `.torrent` file into the files it would write; nothing is added.
#[tauri::command]
pub async fn torrent_preview(
    engine: State<'_, Engine>,
    source: TorrentSourceRequest,
    dir: Option<String>,
) -> CmdResult<TorrentPreviewInfo> {
    preview(&engine, source, dir).await
}

/// Adds the torrent behind a preview token; returns the new download id.
#[tauri::command]
pub async fn add_torrent(engine: State<'_, Engine>, request: TorrentAdd) -> CmdResult<i64> {
    add(&engine, request).await
}

/// The stored facts of every torrent (files, selection, totals).
#[tauri::command]
pub async fn list_torrents(engine: State<'_, Engine>) -> CmdResult<Vec<TorrentInfo>> {
    engine.torrents().await.map_err(err)
}

/// Live numbers of one torrent for the details drawer.
#[tauri::command]
pub async fn torrent_live(engine: State<'_, Engine>, id: i64) -> CmdResult<TorrentLive> {
    engine.torrent_live(DownloadId(id)).await.map_err(err)
}

/// Native "open" dialog for a `.torrent` file; the path, `None` on cancel. The path comes from
/// the user's own pick in the OS dialog.
#[tauri::command]
pub async fn pick_torrent_file(app: tauri::AppHandle) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Torrent", &["torrent"])
            .blocking_pick_file()
    })
    .await
    .map_err(err)?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_magnet_links_are_read_as_text() {
        assert!(matches!(
            source_from_text("  magnet:?xt=urn:btih:abc  "),
            Some(TorrentSourceRequest::Magnet { .. })
        ));
        assert!(matches!(
            source_from_text("MAGNET:?xt=urn:btih:abc"),
            Some(TorrentSourceRequest::Magnet { .. })
        ));
        // a pasted path or URL is not a torrent source: files come from the OS dialog
        assert!(source_from_text("C:\\Users\\me\\x.torrent").is_none());
        assert!(source_from_text("https://example.com/x.torrent").is_none());
        assert!(source_from_text("magnet").is_none());
    }
}
