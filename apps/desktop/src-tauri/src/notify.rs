//! System notifications for finished downloads: one when a download completes and one when it
//! fails, only while "notify on complete" is on and the main window is not focused (the in-app
//! toast covers the focused case). Clicking a notification brings the window up and asks the
//! webview to open that download's details ([`OPEN_DOWNLOAD_EVENT`]).

use std::collections::HashSet;

use riptide_core::{DownloadId, Event};
use riptide_engine::Engine;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

use crate::shell;

/// Tauri event asking the webview to show a download's details; the payload is its id.
pub const OPEN_DOWNLOAD_EVENT: &str = "riptide://open-download";

/// Notification title for a finished download: the file name of `path`, or `fallback`.
pub fn title_for(path: &str, fallback: &str) -> String {
    // both separators, so a Windows path reads the same on every OS
    path.rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .map_or_else(|| fallback.to_string(), str::to_string)
}

/// Registers the click handler and starts watching engine events.
pub fn install<R: Runtime>(app: &AppHandle<R>, engine: Engine) {
    let handle = app.clone();
    let registered = app.notification().on_action(move |performed| {
        crate::show_main_window(&handle);
        if let Some(n) = performed.notification() {
            if let Err(e) = handle.emit(OPEN_DOWNLOAD_EVENT, i64::from(n.id())) {
                tracing::debug!(error = %e, "open-download not delivered");
            }
        }
    });
    if let Err(e) = registered {
        tracing::warn!(error = %e, "notification clicks will not be reported");
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(watch(handle, engine));
}

async fn watch<R: Runtime>(app: AppHandle<R>, engine: Engine) {
    let mut rx = engine.subscribe();
    // one failure notification per download per session (automatic retries can fail again)
    let mut failed_once: HashSet<DownloadId> = HashSet::new();
    loop {
        let event = match rx.recv().await {
            Ok(e) => e,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        match event {
            Event::Completed { id, path } => {
                failed_once.remove(&id);
                if should_notify(&app) {
                    let labels = shell::prefs(&app).labels;
                    show(
                        &app,
                        id,
                        &title_for(&path.to_string_lossy(), "Riptide"),
                        &labels.completed,
                    );
                }
            }
            Event::Failed { id, error, .. } => {
                if !should_notify(&app) || !failed_once.insert(id) {
                    continue;
                }
                let name = match engine.get(id).await {
                    Ok(Some(d)) => d.filename,
                    _ => "Riptide".to_string(),
                };
                let labels = shell::prefs(&app).labels;
                show(&app, id, &name, &labels.failed.replace("{reason}", &error));
            }
            _ => {}
        }
    }
}

/// On, and the main window is hidden, minimised or in the background.
fn should_notify<R: Runtime>(app: &AppHandle<R>) -> bool {
    if !shell::prefs(app).notify_on_complete {
        return false;
    }
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return true;
    };
    let focused = window.is_focused().unwrap_or(false);
    let visible = window.is_visible().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    !(focused && visible && !minimized)
}

fn show<R: Runtime>(app: &AppHandle<R>, id: DownloadId, title: &str, body: &str) {
    let Ok(nid) = i32::try_from(id.0) else {
        return;
    };
    if let Err(e) = app
        .notification()
        .builder()
        .id(nid)
        .title(title)
        .body(body)
        .show()
    {
        tracing::warn!(error = %e, "notification not shown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_the_file_name() {
        assert_eq!(title_for(r"C:\Downloads\movie.mkv", "x"), "movie.mkv");
        assert_eq!(title_for("/home/u/تقرير.pdf", "x"), "تقرير.pdf");
        assert_eq!(title_for("", "Riptide"), "Riptide");
    }
}
