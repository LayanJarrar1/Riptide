//! Riptide desktop shell: the Tauri v2 backend that hosts the Angular UI.
//!
//! This crate is the thin adapter between the engine (`riptide-engine`, which owns the store,
//! the scheduler and the transfers) and the webview. The engine never sees a Tauri type (Hard
//! rule 9); everything the UI can ask for is a `#[tauri::command]` in [`commands`], and every
//! engine event is forwarded to the webview as the `riptide://event` Tauri event.
//!
//! The same engine also answers the local IPC endpoint ([`ipc_server`]), which is how the
//! browser extension (through the `riptide-host` Native Messaging host) and the CLI reach it;
//! [`integration`] registers that host with the browsers, per user.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::path::PathBuf;

use riptide_engine::{Engine, EngineConfig};
use tauri::{Emitter, Manager};

pub mod commands;
pub mod integration;
pub mod ipc_server;

pub use commands::{AppInfo, BUILD_MILESTONE, PRODUCT_NAME};

/// Label of the main window (see `tauri.conf.json`).
pub const MAIN_WINDOW: &str = "main";

/// Brings the main window to the front: restored, shown and focused. Used when a second
/// launch hands over and when the browser adds a download.
pub fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Name of the Tauri event that carries every `riptide_core::Event`.
pub const EVENT_NAME: &str = "riptide://event";

/// The `tracing` filter for this run: `--log-level <filter>` / `--log-level=<filter>` on the
/// command line, else the `RUST_LOG` environment value, else `info`. Pure so it can be tested;
/// `main` passes `std::env::args()` and `std::env::var("RUST_LOG")`.
pub fn log_filter<I: IntoIterator<Item = String>>(args: I, env_rust_log: Option<String>) -> String {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--log-level=") {
            if !value.is_empty() {
                return value.to_string();
            }
        } else if arg == "--log-level" {
            if let Some(value) = args.next() {
                return value;
            }
        }
    }
    env_rust_log
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| riptide_log::LogConfig::DEFAULT_FILTER.to_string())
}

/// Resolves the data directory (installed, or portable when `portable.txt` sits next to the
/// executable), creates it and starts the logger. Returns the guard that must live until the
/// process exits. Failing to log is not fatal: the app still runs, and the reason is printed
/// to stderr once.
pub fn start_logging(filter: &str) -> Option<riptide_log::LogGuard> {
    let paths = match riptide_log::Paths::resolve() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("riptide: cannot resolve the data directory: {e}");
            return None;
        }
    };
    if let Err(e) = paths.ensure_dirs() {
        eprintln!("riptide: cannot create {}: {e}", paths.data_dir.display());
        return None;
    }
    let config = riptide_log::LogConfig::new(&paths.logs_dir)
        .with_filter(filter)
        .with_stderr(cfg!(debug_assertions));
    match riptide_log::init(&config) {
        Ok(guard) => {
            tracing::info!(
                portable = paths.portable,
                data_dir = %paths.data_dir.display(),
                "data directory resolved"
            );
            Some(guard)
        }
        Err(e) => {
            eprintln!("riptide: logging disabled: {e}");
            None
        }
    }
}

/// Where the engine keeps its database and where downloads go by default: the user's
/// Downloads folder, or `<data>/downloads` when the OS does not report one.
fn engine_config() -> Result<EngineConfig, riptide_log::Error> {
    let paths = riptide_log::Paths::resolve()?;
    paths.ensure_dirs()?;
    let default_dir: PathBuf =
        dirs::download_dir().unwrap_or_else(|| paths.data_dir.join("downloads"));
    Ok(EngineConfig {
        db_path: paths.db_path,
        default_dir,
    })
}

/// Builds and runs the Tauri application; returns when the last window closes.
///
/// # Panics
///
/// Panics if the runtime cannot start (missing WebView2, unreadable configuration, a database
/// that cannot be opened). There is no useful way to continue without a window or without the
/// store, so this is the one place a panic is intended.
pub fn run() {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        build = BUILD_MILESTONE,
        "starting Riptide desktop"
    );
    tauri::Builder::default()
        // first: a second launch hands over to the running instance and exits before it opens
        // the database (its crash recovery would otherwise re-queue the live transfers)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let config = engine_config()?;
            let engine = tauri::async_runtime::block_on(Engine::open(config))?;
            app.manage(engine.clone());

            // browser host / CLI → engine over the per-user pipe; an add brings the window up
            let handle = app.handle().clone();
            let ipc_engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                let endpoint = match riptide_ipc::default_endpoint() {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::error!(error = %e, "no IPC endpoint; the browser cannot reach the app");
                        return;
                    }
                };
                let on_added = move || show_main_window(&handle);
                if let Err(e) = ipc_server::serve(&endpoint, ipc_engine, on_added).await {
                    tracing::error!(error = %e, "IPC endpoint stopped");
                }
            });

            // register the browser host (idempotent; skipped once the user turned it off)
            tauri::async_runtime::spawn_blocking(|| {
                if let (Some(mut registry), Ok(layout)) = (
                    integration::platform_registry(),
                    integration::Layout::for_this_app(),
                ) {
                    integration::register_on_start(&layout, registry.as_mut());
                }
            });

            // engine → webview: one Tauri event per engine event, dropped if the webview lags
            let handle = app.handle().clone();
            let mut rx = engine.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            if let Err(e) = handle.emit(EVENT_NAME, &event) {
                                tracing::debug!(error = %e, "event not delivered to the webview");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(dropped = n, "webview fell behind on engine events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // stop transfers at a checkpoint; their records stay Downloading and resume on
                // the next start (hard rule 2). Close-to-tray arrives in M2.
                if let Some(engine) = window.try_state::<Engine>() {
                    let engine = engine.inner().clone();
                    tauri::async_runtime::block_on(engine.shutdown());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_downloads,
            commands::get_download,
            commands::get_segments,
            commands::add_download,
            commands::probe_url,
            commands::find_duplicate,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::restart_download,
            commands::remove_download,
            commands::pause_all,
            commands::resume_all,
            commands::get_engine_settings,
            commands::set_engine_settings,
            commands::disk_free,
            commands::open_file,
            commands::show_in_folder,
            commands::pick_folder,
            commands::integration_status,
            commands::register_browser_host,
            commands::unregister_browser_host,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Riptide desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn log_filter_prefers_flag_then_env_then_info() {
        assert_eq!(
            log_filter(args(&["riptide", "--log-level", "debug"]), None),
            "debug"
        );
        assert_eq!(
            log_filter(
                args(&["riptide", "--log-level=riptide_net=trace,info"]),
                Some("warn".into())
            ),
            "riptide_net=trace,info"
        );
        assert_eq!(log_filter(args(&["riptide"]), Some("warn".into())), "warn");
        assert_eq!(log_filter(args(&["riptide"]), Some("  ".into())), "info");
        assert_eq!(
            log_filter(args(&["riptide", "--log-level"]), None),
            "info",
            "dangling flag"
        );
        assert_eq!(
            log_filter(args(&["riptide", "--log-level="]), None),
            "info",
            "empty value"
        );
    }

    #[test]
    fn engine_config_points_inside_the_data_dir_or_downloads() {
        let cfg = engine_config().unwrap();
        assert!(cfg.db_path.ends_with("riptide.sqlite"));
        assert!(cfg.default_dir.is_absolute());
    }
}
