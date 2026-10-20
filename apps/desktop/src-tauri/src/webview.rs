//! The main window, created in `setup` so portable mode can give it its own WebView2 profile.
//!
//! Tauri creates configured windows itself with the profile under `%LOCALAPPDATA%\<identifier>`,
//! which an installed copy shares. In portable mode (a `portable.txt` next to the executable)
//! the profile must live with the rest of the data, in `<data>\webview`, so nothing is left
//! behind and nothing is inherited (theme, UI preferences, cache) from another copy.

use std::path::PathBuf;

use riptide_log::Paths;
use tauri::{WebviewWindowBuilder, Wry};

use crate::MAIN_WINDOW;

/// Folder name of the portable WebView2 profile inside the data directory.
pub const PORTABLE_WEBVIEW_DIR: &str = "webview";

/// Where the WebView2 profile goes: `<data>\webview` in portable mode, `None` (Tauri's default,
/// `%LOCALAPPDATA%\<identifier>`) for an installed copy.
#[must_use]
pub fn webview_data_dir(paths: &Paths) -> Option<PathBuf> {
    paths
        .portable
        .then(|| paths.data_dir.join(PORTABLE_WEBVIEW_DIR))
}

/// Creates the configured main window (the config has `"create": false`), with the portable
/// profile folder when portable mode is on.
///
/// # Errors
///
/// Fails when the window config is missing or the window cannot be created.
pub fn create_main_window(app: &tauri::App<Wry>, paths: &Paths) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let mut builder = WebviewWindowBuilder::from_config(app.handle(), &config)?;
    if let Some(dir) = webview_data_dir(paths) {
        tracing::info!(dir = %dir.display(), "portable: WebView2 profile next to the app");
        builder = builder.data_directory(dir);
    }
    #[cfg(all(feature = "cdp", windows))]
    if let Some(port) = std::env::var("RIPTIDE_CDP_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        tracing::warn!(port, "test build: WebView2 remote debugging is ON");
        builder = builder.additional_browser_args(&format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port} --remote-allow-origins=*"
        ));
    }
    builder.build()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_keeps_the_webview_profile_in_the_data_folder() {
        let p = Paths::in_dir(r"D:\Apps\Riptide\data", true);
        assert_eq!(
            webview_data_dir(&p),
            Some(PathBuf::from(r"D:\Apps\Riptide\data").join("webview"))
        );
    }

    #[test]
    fn an_installed_copy_keeps_tauris_default_profile() {
        let p = Paths::in_dir(r"C:\Users\x\AppData\Local\Riptide", false);
        assert_eq!(webview_data_dir(&p), None);
    }
}
