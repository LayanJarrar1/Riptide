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

/// Tauri event asking the webview to open Settings → About (an update notification was clicked).
pub const OPEN_UPDATE_EVENT: &str = "riptide://open-update";

/// Id of the "update available" notification. Download ids are small and positive, so this
/// can never be mistaken for one.
pub const UPDATE_NOTIFICATION_ID: i32 = i32::MAX;

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
            let result = if n.id() == UPDATE_NOTIFICATION_ID {
                handle.emit(OPEN_UPDATE_EVENT, ())
            } else {
                handle.emit(OPEN_DOWNLOAD_EVENT, i64::from(n.id()))
            };
            if let Err(e) = result {
                tracing::debug!(error = %e, "notification click not delivered");
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

/// Shows the system notification of a finished or failed download; clicking it opens that
/// download's details.
///
/// On Windows the plugin would park one thread per notification until the user clicks or
/// dismisses it (the toast API only reports that through a blocking wait), and a toast nobody
/// touches never ends: 1,000 completed downloads left 1,000 threads. So the toast is shown here
/// and at most [`toast::MAX_WAITING`] of them may wait for a click at the same time; the rest
/// are shown without click-through.
pub fn show<R: Runtime>(app: &AppHandle<R>, id: DownloadId, title: &str, body: &str) {
    let Ok(nid) = i32::try_from(id.0) else {
        return;
    };
    #[cfg(windows)]
    {
        let handle = app.clone();
        let on_click = move || {
            crate::show_main_window(&handle);
            if let Err(e) = handle.emit(OPEN_DOWNLOAD_EVENT, i64::from(nid)) {
                tracing::debug!(error = %e, "notification click not delivered");
            }
        };
        if let Err(e) = toast::show(&app.config().identifier, title, body, Box::new(on_click)) {
            tracing::warn!(error = %e, "notification not shown");
        }
    }
    #[cfg(not(windows))]
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

/// Windows toasts with a bounded number of click waiters.
#[cfg(windows)]
pub mod toast {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Most toasts that may wait for a click (or dismissal) at once; each costs a thread.
    pub const MAX_WAITING: usize = 8;

    static WAITING: AtomicUsize = AtomicUsize::new(0);

    /// Threads currently waiting on a toast.
    #[must_use]
    pub fn waiting() -> usize {
        WAITING.load(Ordering::SeqCst)
    }

    /// Shows a toast; `on_click` runs if the user clicks it while a waiter is available.
    ///
    /// # Errors
    ///
    /// The toast could not be shown.
    pub fn show(
        identifier: &str,
        title: &str,
        body: &str,
        on_click: Box<dyn FnOnce() + Send>,
    ) -> Result<(), String> {
        let mut n = notify_rust::Notification::new();
        n.summary(title).body(body).auto_icon();
        // like the plugin: the app identity only when installed (a dev build runs from target/debug or target/release and must borrow PowerShell identity to be allowed to toast)
        let from_target = std::env::current_exe().ok().is_some_and(|exe| {
            exe.parent().is_some_and(|dir| {
                let dir = dir.to_string_lossy();
                dir.ends_with(r"target\debug") || dir.ends_with(r"target\release")
            })
        });
        if !from_target {
            n.app_id(identifier);
        }
        let handle = n.show().map_err(|e| e.to_string())?;
        if WAITING
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |w| {
                (w < MAX_WAITING).then_some(w + 1)
            })
            .is_err()
        {
            // the cap is reached: no click-through; dropping the handle frees everything
            return Ok(());
        }
        let spawned = std::thread::Builder::new()
            .name("toast-wait".into())
            .spawn(move || {
                let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                    if matches!(
                        response,
                        notify_rust::NotificationResponse::Default
                            | notify_rust::NotificationResponse::Action(_)
                    ) {
                        on_click();
                    }
                });
                WAITING.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            WAITING.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// Shows the "update available" system notification (the webview decides when, once per
/// version, and translates `title` and `body`). Clicking it opens Settings → About.
#[tauri::command]
pub fn notify_update<R: Runtime>(app: AppHandle<R>, title: String, body: String) {
    if let Err(e) = app
        .notification()
        .builder()
        .id(UPDATE_NOTIFICATION_ID)
        .title(title)
        .body(body)
        .show()
    {
        tracing::warn!(error = %e, "update notification not shown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_id_is_not_a_download_id() {
        assert_eq!(UPDATE_NOTIFICATION_ID, i32::MAX);
        assert_ne!(i32::try_from(1_u64).ok(), Some(UPDATE_NOTIFICATION_ID));
    }

    #[test]
    fn title_is_the_file_name() {
        assert_eq!(title_for(r"C:\Downloads\movie.mkv", "x"), "movie.mkv");
        assert_eq!(title_for("/home/u/تقرير.pdf", "x"), "تقرير.pdf");
        assert_eq!(title_for("", "Riptide"), "Riptide");
    }

    /// Regression test for the per-download thread leak: a toast nobody touches never ends, and
    /// the plugin parked one thread per toast waiting for it, so 1,000 downloads left 1,000
    /// threads. Showing toasts must keep the number of waiting threads at the cap.
    #[cfg(windows)]
    #[test]
    fn untouched_toasts_never_pile_up_waiting_threads() {
        for i in 0..80 {
            // a toast that may fail to show on a machine without a desktop; that is fine here
            let _ = toast::show(
                "app.riptide.test",
                &format!("finished {i}"),
                "body",
                Box::new(|| {}),
            );
        }
        assert!(
            toast::waiting() <= toast::MAX_WAITING,
            "{} threads wait for toasts, the cap is {}",
            toast::waiting(),
            toast::MAX_WAITING
        );
    }
}
