//! Video support: finds the bundled yt-dlp / ffmpeg, hands them to the engine, and exposes the
//! media commands.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `probe_media` | `url` | `MediaInfo` (title, formats, quality menu) |
//! | `media_tools_info` | – | `MediaToolsInfo` (installed?, yt-dlp version) |
//! | `update_media_tools` | – | the new yt-dlp version (`yt-dlp -U` on the app's own copy) |
//!
//! Where yt-dlp comes from, first match wins:
//!
//! 1. `<data>\tools\yt-dlp.exe`: the copy *Update video support* keeps up to date (YouTube
//!    changes often; the installed copy next to the app is replaced only by app updates).
//! 2. `yt-dlp.exe` next to `riptide-desktop.exe` (installer and portable zip).
//! 3. `RIPTIDE_YTDLP` / `RIPTIDE_FFMPEG` (development).
//! 4. Debug builds only: `src-tauri/binaries/yt-dlp-<target>.exe` as staged by
//!    `scripts/fetch-media-tools.mjs`.
//!
//! ffmpeg is never updated by the app; it is looked up next to the app, then `RIPTIDE_FFMPEG`,
//! then (debug) `src-tauri/binaries/`.

use std::path::{Path, PathBuf};

use riptide_core::MediaInfo;
use riptide_engine::Engine;
use riptide_media::YtDlp;
use serde::Serialize;
use tauri::State;

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// Folder under the data dir that holds the updatable yt-dlp copy.
pub const TOOLS_DIR: &str = "tools";

/// The data folder (installed or portable), as resolved at startup.
#[derive(Debug, Clone)]
pub struct DataDir(pub PathBuf);

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Debug builds: the staging folder of `scripts/fetch-media-tools.mjs`.
fn staged(name: &str) -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
    let prefix = format!("{name}-");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(EXE))
        })
}

fn from_env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// The bundled yt-dlp (not the updated copy).
fn bundled_yt_dlp() -> Option<PathBuf> {
    exe_dir()
        .map(|d| d.join(format!("yt-dlp{EXE}")))
        .filter(|p| p.is_file())
        .or_else(|| from_env("RIPTIDE_YTDLP"))
        .or_else(|| staged("yt-dlp"))
}

fn updated_copy(data: &Path) -> PathBuf {
    data.join(TOOLS_DIR).join(format!("yt-dlp{EXE}"))
}

fn ffmpeg() -> Option<PathBuf> {
    exe_dir()
        .map(|d| d.join(format!("ffmpeg{EXE}")))
        .filter(|p| p.is_file())
        .or_else(|| from_env("RIPTIDE_FFMPEG"))
        .or_else(|| staged("ffmpeg"))
}

/// The tools to use, or `None` when yt-dlp is not installed.
pub fn resolve(data: &Path) -> Option<YtDlp> {
    // cookie temp files live in <data>\tmp; a hard kill leaves them behind, so sweep at start-up
    let removed =
        riptide_media::sweep_stale_cookie_files(&cookie_dir(data), riptide_media::STALE_AFTER);
    if removed > 0 {
        tracing::info!(removed, "removed leftover cookie temp files");
    }
    let copy = updated_copy(data);
    let exe = if copy.is_file() {
        Some(copy)
    } else {
        bundled_yt_dlp()
    }?;
    Some(YtDlp::new(exe, ffmpeg()).with_cookie_dir(cookie_dir(data)))
}

/// The app's private folder for cookie temp files (inside the per-user data folder).
fn cookie_dir(data: &Path) -> PathBuf {
    data.join("tmp")
}

/// What Settings → About shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaToolsInfo {
    /// yt-dlp was found.
    pub available: bool,
    /// `yt-dlp --version`, when it answers.
    pub version: Option<String>,
    /// ffmpeg was found (needed to merge video and audio).
    pub ffmpeg: bool,
}

/// Asks yt-dlp what a page offers.
#[tauri::command]
pub async fn probe_media(engine: State<'_, Engine>, url: String) -> Result<MediaInfo, String> {
    engine
        .probe_media(&url, None)
        .await
        .map_err(|e| riptide_log::redact_line(&e.to_string()))
}

/// Whether video support is installed, and its version.
#[tauri::command]
pub async fn media_tools_info(engine: State<'_, Engine>) -> Result<MediaToolsInfo, String> {
    let Some(tool) = engine.media_tools() else {
        return Ok(MediaToolsInfo {
            available: false,
            version: None,
            ffmpeg: false,
        });
    };
    Ok(MediaToolsInfo {
        available: tool.is_available(),
        version: tool.version().await.ok(),
        ffmpeg: tool.ffmpeg().is_some_and(Path::is_file),
    })
}

/// *Update video support*: copies the bundled yt-dlp into `<data>\tools\` (once), runs
/// `yt-dlp -U` on that copy (a download from yt-dlp's GitHub releases, only on this click) and
/// switches the engine to it. Returns the version afterwards.
#[tauri::command]
pub async fn update_media_tools(
    engine: State<'_, Engine>,
    data: State<'_, DataDir>,
) -> Result<String, String> {
    let copy = updated_copy(&data.0);
    if !copy.is_file() {
        let bundled = bundled_yt_dlp().ok_or("video support is not installed")?;
        if let Some(parent) = copy.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&bundled, &copy).map_err(|e| e.to_string())?;
    }
    let tool = YtDlp::new(copy, ffmpeg()).with_cookie_dir(cookie_dir(&data.0));
    let version = tool
        .update()
        .await
        .map_err(|e| riptide_log::redact_line(&e.to_string()))?;
    engine.set_media_tools(Some(tool));
    tracing::info!(%version, "video support updated");
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_updated_copy_wins() {
        let dir = tempfile::tempdir().unwrap();
        let copy = updated_copy(dir.path());
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::write(&copy, b"").unwrap();
        assert_eq!(resolve(dir.path()).unwrap().exe(), copy.as_path());
    }
}
