//! Desktop-shell preferences the Rust side needs (close to tray, notifications, clipboard
//! monitor, the localized tray/notification strings) and the window-close policy.
//!
//! The settings themselves live in the webview (`SettingsService`); it pushes the subset below
//! with `set_shell_prefs` at start-up and on every change, together with the strings already
//! translated from `ar.json` / `en.json`, so the tray and the notifications follow the app
//! language live without a second translation table in Rust.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `set_shell_prefs` | `prefs: ShellPrefs` | – |
//! | `autostart_enabled` | – | whether the HKCU Run entry exists |
//! | `set_autostart` | `enabled` | – |

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use tauri::{Manager, Runtime};
use tauri_plugin_autostart::ManagerExt as _;

/// Command-line flag the autostart entry passes: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Strings the tray and the notifications show, translated by the webview.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ShellLabels {
    /// Tray menu: bring the window up.
    pub open: String,
    /// Tray menu: pause every download.
    pub pause_all: String,
    /// Tray menu: resume every download.
    pub resume_all: String,
    /// Tray menu: exit for real.
    pub quit: String,
    /// Tooltip with nothing running.
    pub tooltip_idle: String,
    /// Tooltip while downloading; `{count}` and `{speed}` are replaced.
    pub tooltip_active: String,
    /// Notification body when a download completes.
    pub completed: String,
    /// Notification body when a download fails; `{reason}` is replaced.
    pub failed: String,
    /// Summary notification after a burst of completions; `{count}` is replaced.
    pub completed_many: String,
    /// Byte units, smallest first: B, KB, MB, GB, TB.
    pub units: [String; 5],
    /// Suffix that turns a size into a speed (`/s`).
    pub per_second: String,
}

impl Default for ShellLabels {
    fn default() -> Self {
        Self {
            open: "Open Riptide".into(),
            pause_all: "Pause all".into(),
            resume_all: "Resume all".into(),
            quit: "Quit".into(),
            tooltip_idle: "Riptide".into(),
            tooltip_active: "Riptide: {count} active, {speed}".into(),
            completed: "Download complete".into(),
            failed: "Download failed: {reason}".into(),
            completed_many: "{count} downloads complete".into(),
            units: [
                "B".into(),
                "KB".into(),
                "MB".into(),
                "GB".into(),
                "TB".into(),
            ],
            per_second: "/s".into(),
        }
    }
}

/// The settings the shell acts on, as the webview last reported them.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ShellPrefs {
    /// Closing the window hides it; downloads continue.
    pub close_to_tray: bool,
    /// System notification on completion / failure while the window is not focused.
    pub notify_on_complete: bool,
    /// Offer to download links copied to the clipboard.
    pub clipboard_monitor: bool,
    /// Localized strings.
    pub labels: ShellLabels,
}

impl Default for ShellPrefs {
    /// Matches the webview's `DEFAULT_SETTINGS` except the clipboard monitor, which waits for
    /// the webview (it is the one that shows the prompt).
    fn default() -> Self {
        Self {
            close_to_tray: true,
            notify_on_complete: true,
            clipboard_monitor: false,
            labels: ShellLabels::default(),
        }
    }
}

/// Managed state: the current preferences and whether a real exit is under way.
#[derive(Debug, Default)]
pub struct Shell {
    prefs: Mutex<ShellPrefs>,
    quitting: AtomicBool,
}

impl Shell {
    /// A copy of the current preferences.
    pub fn prefs(&self) -> ShellPrefs {
        self.prefs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_prefs(&self, prefs: ShellPrefs) {
        *self.prefs.lock().unwrap_or_else(PoisonError::into_inner) = prefs;
    }

    /// Marks that the app is exiting for real (tray Quit); close-to-tray no longer applies.
    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    /// Whether [`Self::begin_quit`] was called.
    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }
}

/// Current preferences of the app (defaults before the state exists).
pub fn prefs<R: Runtime, M: Manager<R>>(manager: &M) -> ShellPrefs {
    manager
        .try_state::<Shell>()
        .map(|s| s.prefs())
        .unwrap_or_default()
}

/// Whether this launch asked to start hidden (autostart passes [`MINIMIZED_ARG`]).
pub fn started_minimized<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter().any(|a| a == MINIMIZED_ARG)
}

/// First step of the window-close handler. When close-to-tray is on (and this is not a tray
/// Quit), it cancels the close, hides the window and returns `true`: the caller stops there and
/// downloads continue. Otherwise it returns `false` and the caller goes on (a "confirm on exit"
/// prompt, then the engine shutdown).
pub fn close_to_tray<R: Runtime>(window: &tauri::Window<R>, api: &tauri::CloseRequestApi) -> bool {
    let Some(shell) = window.try_state::<Shell>() else {
        return false;
    };
    if shell.is_quitting() || !shell.prefs().close_to_tray {
        return false;
    }
    api.prevent_close();
    if let Err(e) = window.hide() {
        tracing::warn!(error = %e, "cannot hide the window; closing instead");
        return false;
    }
    true
}

/// Stores the webview's shell preferences and relabels the tray in the new language.
#[tauri::command]
pub fn set_shell_prefs<R: Runtime>(app: tauri::AppHandle<R>, prefs: ShellPrefs) {
    if let Some(shell) = app.try_state::<Shell>() {
        shell.set_prefs(prefs.clone());
    }
    crate::tray::relabel(&app, &prefs.labels);
}

/// Whether Riptide starts with Windows (the per-user Run entry exists).
#[tauri::command]
pub fn autostart_enabled<R: Runtime>(app: tauri::AppHandle<R>) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Adds or removes the per-user Run entry (`HKCU`, no elevation).
#[tauri::command]
pub fn set_autostart<R: Runtime>(app: tauri::AppHandle<R>, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_flag_is_detected() {
        assert!(started_minimized([
            "riptide".to_string(),
            MINIMIZED_ARG.to_string()
        ]));
        assert!(!started_minimized(["riptide".to_string()]));
    }

    #[test]
    fn prefs_deserialize_partially_with_defaults() {
        let prefs: ShellPrefs = serde_json::from_str(
            r#"{"closeToTray":false,"labels":{"quit":"خروج","units":["بايت","ك.ب","م.ب","ج.ب","ت.ب"]}}"#,
        )
        .unwrap();
        assert!(!prefs.close_to_tray);
        assert!(prefs.notify_on_complete);
        assert_eq!(prefs.labels.quit, "خروج");
        assert_eq!(prefs.labels.open, "Open Riptide");
        assert_eq!(prefs.labels.units[2], "م.ب");
    }
}
