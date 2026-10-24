//! Helps the user install the bundled browser extension (Settings → Integration).
//!
//! Chrome and Edge only accept an unpacked extension through *Developer mode → Load unpacked*;
//! the app cannot do that for them. What it can do is put the two things in reach: the folder
//! the installer / portable zip ships (`extension\chrome`, `extension\edge` next to the
//! executable) and the browser's `…://extensions` page.
//!
//! The browser can only ask: the webview can ask for exactly two actions, both on fixed targets. The
//! folder is `<app folder>\extension\<chrome|edge>` and the page is the literal
//! `chrome://extensions` / `edge://extensions` opened with a browser executable found in the
//! registry's `App Paths` or its standard install folders. No path or URL from the caller is
//! ever used; the only argument is the [`Chromium`] enum.

use std::path::{Path, PathBuf};

use riptide_host::CHROMIUM_EXTENSION_ID;
use serde::{Deserialize, Serialize};

/// Folder (next to the app executable) that holds one built extension per browser.
pub const EXTENSION_DIR: &str = "extension";

/// A Chromium browser the extension can be loaded into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chromium {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
}

impl Chromium {
    /// Every supported browser.
    pub const ALL: [Chromium; 2] = [Chromium::Chrome, Chromium::Edge];

    /// Name of the folder inside [`EXTENSION_DIR`].
    pub fn folder_name(self) -> &'static str {
        match self {
            Chromium::Chrome => "chrome",
            Chromium::Edge => "edge",
        }
    }

    /// The fixed page that lists extensions.
    pub fn extensions_url(self) -> &'static str {
        match self {
            Chromium::Chrome => "chrome://extensions",
            Chromium::Edge => "edge://extensions",
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn exe_name(self) -> &'static str {
        match self {
            Chromium::Chrome => "chrome.exe",
            Chromium::Edge => "msedge.exe",
        }
    }

    /// Install locations under a `Program Files` or `LocalAppData` base.
    fn relative_exe(self) -> &'static str {
        match self {
            Chromium::Chrome => r"Google\Chrome\Application\chrome.exe",
            Chromium::Edge => r"Microsoft\Edge\Application\msedge.exe",
        }
    }
}

/// The folder with the built extension of `browser` next to the app (`exe_dir`).
pub fn extension_folder(exe_dir: &Path, browser: Chromium) -> PathBuf {
    exe_dir.join(EXTENSION_DIR).join(browser.folder_name())
}

/// Where the standard installers put `browser`, given the environment values of
/// `ProgramFiles`, `ProgramFiles(x86)` and `LOCALAPPDATA`.
pub fn standard_locations(browser: Chromium, bases: &[Option<String>]) -> Vec<PathBuf> {
    bases
        .iter()
        .flatten()
        .filter(|b| !b.is_empty())
        .map(|base| Path::new(base).join(browser.relative_exe()))
        .collect()
}

/// What the Settings page shows about the extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    /// The extension id the host trusts (what `chrome://extensions` must show).
    pub id: &'static str,
    /// Per browser.
    pub browsers: Vec<ExtensionBrowser>,
}

/// One browser's row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionBrowser {
    /// Which browser.
    pub browser: Chromium,
    /// The browser is installed on this machine.
    pub installed: bool,
    /// The folder to pick in *Load unpacked*.
    pub folder: String,
    /// The folder exists (a development build without the extension has none).
    pub folder_found: bool,
}

/// The folder next to the running app.
fn app_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "the app executable has no folder".to_string())
}

/// Finds the executable of `browser`: `App Paths` in the registry first, then the standard
/// install folders. `None` when it is not installed (or not on Windows).
pub fn find_browser(browser: Chromium) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let sub = format!(
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\{}",
            browser.exe_name()
        );
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Ok(key) = winreg::RegKey::predef(hive).open_subkey(&sub) {
                if let Ok(path) = key.get_value::<String, _>("") {
                    candidates.push(PathBuf::from(path.trim_matches('"')));
                }
            }
        }
    }
    let env = |name: &str| std::env::var(name).ok();
    candidates.extend(standard_locations(
        browser,
        &[
            env("ProgramFiles"),
            env("ProgramFiles(x86)"),
            env("LOCALAPPDATA"),
        ],
    ));
    candidates.into_iter().find(|p| p.is_file())
}

/// Browser and extension folder state, for the Settings page.
#[tauri::command]
pub fn extension_info() -> Result<ExtensionInfo, String> {
    let dir = app_dir()?;
    let browsers = Chromium::ALL
        .iter()
        .map(|&browser| {
            let folder = extension_folder(&dir, browser);
            ExtensionBrowser {
                browser,
                installed: find_browser(browser).is_some(),
                folder_found: folder.join("manifest.json").is_file(),
                folder: folder.to_string_lossy().into_owned(),
            }
        })
        .collect();
    Ok(ExtensionInfo {
        id: CHROMIUM_EXTENSION_ID,
        browsers,
    })
}

/// Opens the bundled extension folder in Explorer, so the user can pick it in *Load unpacked*.
#[tauri::command]
pub fn open_extension_folder(app: tauri::AppHandle, browser: Chromium) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let folder = extension_folder(&app_dir()?, browser);
    if !folder.join("manifest.json").is_file() {
        return Err("the extension folder is missing from this install".to_string());
    }
    app.opener()
        .open_path(folder.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Opens `chrome://extensions` (or `edge://extensions`) in that browser. The URL is a constant
/// and goes as one argument to the browser's own executable; no shell is involved.
#[tauri::command]
pub fn open_extensions_page(browser: Chromium) -> Result<(), String> {
    let exe = find_browser(browser).ok_or_else(|| "the browser is not installed".to_string())?;
    std::process::Command::new(exe)
        .arg(browser.extensions_url())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_fixed_names_under_the_app_folder() {
        let dir = Path::new(r"C:\Riptide");
        assert_eq!(
            extension_folder(dir, Chromium::Chrome),
            dir.join("extension").join("chrome")
        );
        assert_eq!(
            extension_folder(dir, Chromium::Edge),
            dir.join("extension").join("edge")
        );
    }

    #[test]
    fn only_chrome_and_edge_deserialize() {
        assert_eq!(
            serde_json::from_str::<Chromium>("\"chrome\"").unwrap(),
            Chromium::Chrome
        );
        assert_eq!(
            serde_json::from_str::<Chromium>("\"edge\"").unwrap(),
            Chromium::Edge
        );
        for bad in ["\"firefox\"", "\"C:\\\\evil.exe\"", "\"\"", "1"] {
            assert!(serde_json::from_str::<Chromium>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn urls_are_constants() {
        assert_eq!(Chromium::Chrome.extensions_url(), "chrome://extensions");
        assert_eq!(Chromium::Edge.extensions_url(), "edge://extensions");
    }

    #[test]
    fn standard_locations_skip_missing_bases() {
        let found = standard_locations(
            Chromium::Chrome,
            &[Some(r"C:\Program Files".into()), None, Some(String::new())],
        );
        assert_eq!(
            found,
            vec![PathBuf::from(r"C:\Program Files").join(r"Google\Chrome\Application\chrome.exe")]
        );
    }

    #[test]
    fn the_shipped_extension_id_matches_the_host_allow_list() {
        assert_eq!(CHROMIUM_EXTENSION_ID.len(), 32);
        assert!(CHROMIUM_EXTENSION_ID
            .bytes()
            .all(|b| (b'a'..=b'p').contains(&b)));
    }
}
