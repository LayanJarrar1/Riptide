//! Confirm-on-exit: closing the window while downloads are active asks first.
//!
//! The webview owns the preference (`confirmOnExit`) and the bilingual dialog; it mirrors the
//! preference here with [`set_exit_guard`]. When the window is asked to close, the close
//! handler calls [`confirm_before_close`]: with the guard on and downloads running it cancels
//! the close and emits [`CONFIRM_EXIT_EVENT`] with the number of active downloads; the dialog
//! then calls [`quit_app`] (Quit) or nothing (Cancel). The guard starts off, so a webview that
//! never loaded can never make the window impossible to close.
//!
//! Composition: a close-to-tray behaviour runs *before* this function in the close handler and
//! returns early when it hides the window; only a real quit reaches the prompt.

use std::sync::atomic::{AtomicBool, Ordering};

use riptide_engine::Engine;
use tauri::{Emitter, Manager};

/// Event emitted to the webview when a close needs the user's confirmation; the payload is the
/// number of active downloads.
pub const CONFIRM_EXIT_EVENT: &str = "riptide://confirm-exit";

/// Whether the close should be confirmed (mirrors the UI's `confirmOnExit`).
#[derive(Debug, Default)]
pub struct ExitGuard {
    enabled: AtomicBool,
}

impl ExitGuard {
    /// Turns the confirmation on or off.
    pub fn set(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether the confirmation is on.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// Whether a close with `active` running downloads must be confirmed first.
pub fn needs_confirmation(guard_enabled: bool, active: usize) -> bool {
    guard_enabled && active > 0
}

/// Called by the close handler. Returns `true` when it cancelled the close and asked the
/// webview to confirm; `false` when the window may close now.
pub fn confirm_before_close<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    api: &tauri::CloseRequestApi,
) -> bool {
    let enabled = window.try_state::<ExitGuard>().is_some_and(|g| g.enabled());
    let active = window
        .try_state::<Engine>()
        .map(|e| e.running_ids().len())
        .unwrap_or(0);
    if !needs_confirmation(enabled, active) {
        return false;
    }
    api.prevent_close();
    if let Err(e) = window.emit(CONFIRM_EXIT_EVENT, active) {
        // the webview cannot ask: do not trap the user, close as if confirmed
        tracing::warn!(error = %e, "confirm-exit prompt not delivered; closing");
        return false;
    }
    true
}

/// A quit asked for outside the window (⌘Q in the macOS menu): with the guard on and downloads
/// running, shows the window and asks like a close does; otherwise quits like the tray's Quit.
pub fn request_quit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let enabled = app.try_state::<ExitGuard>().is_some_and(|g| g.enabled());
    let active = app
        .try_state::<Engine>()
        .map(|e| e.running_ids().len())
        .unwrap_or(0);
    if needs_confirmation(enabled, active) {
        crate::show_main_window(app);
        if app.emit(CONFIRM_EXIT_EVENT, active).is_ok() {
            return;
        }
        tracing::warn!("confirm-exit prompt not delivered; quitting");
    }
    crate::tray::quit(app);
}

/// Mirrors the UI's `confirmOnExit` preference.
#[tauri::command]
pub fn set_exit_guard(guard: tauri::State<'_, ExitGuard>, enabled: bool) {
    guard.set(enabled);
}

/// Quit confirmed: stops the transfers at a checkpoint (they resume on the next start) and
/// exits.
#[tauri::command]
pub async fn quit_app(app: tauri::AppHandle) {
    if let Some(engine) = app.try_state::<Engine>() {
        let engine = engine.inner().clone();
        engine.shutdown().await;
    }
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_only_when_on_and_downloads_are_active() {
        assert!(needs_confirmation(true, 2));
        assert!(!needs_confirmation(true, 0));
        assert!(!needs_confirmation(false, 5));
        let guard = ExitGuard::default();
        assert!(!guard.enabled(), "off until the webview says otherwise");
        guard.set(true);
        assert!(guard.enabled());
    }
}
