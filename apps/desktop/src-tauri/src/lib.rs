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

pub mod ai;
pub mod clipboard;
pub mod commands;
pub mod crawl;
pub mod exit;
pub mod extension;
pub mod files;
pub mod integration;
pub mod ipc_server;
pub mod media;
pub mod notify;
pub mod power;
pub mod protocol;
pub mod queues;
pub mod research;
pub mod shell;
pub mod torrent;
pub mod tray;
pub mod updater;
pub mod webview;

pub use commands::{AppInfo, BUILD_LABEL, PRODUCT_NAME};

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

/// [`log_filter`] for a whole command line (program name first). A launch that opens a link or
/// file carries data, never flags, so nothing in it can change the log level.
pub fn launch_log_filter(args: &[String], env_rust_log: Option<String>) -> String {
    if protocol::is_open_launch(args) {
        return log_filter(Vec::new(), env_rust_log);
    }
    log_filter(args.iter().cloned(), env_rust_log)
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
                version = env!("CARGO_PKG_VERSION"),
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
/// Downloads folder (on Windows the Downloads known folder, so a folder moved in its
/// properties or redirected to OneDrive is followed), or `<data>/downloads` when the OS does
/// not report one.
fn engine_config() -> Result<EngineConfig, riptide_log::Error> {
    let paths = riptide_log::Paths::resolve()?;
    paths.ensure_dirs()?;
    let default_dir: PathBuf =
        dirs::download_dir().unwrap_or_else(|| paths.data_dir.join("downloads"));
    Ok(EngineConfig {
        db_path: paths.db_path,
        default_dir,
        // a new installation gets Downloads/Riptide/<category> folders, made once
        organize_new_install: true,
    })
}

/// Everything the app does once Tauri is up: the engine, the window, the tray, the servers.
fn startup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // the engine first: when it cannot open (a database from a newer version, a locked or
    // unwritable data folder) there is no half-built window to explain, only the message
    let paths = riptide_log::Paths::resolve()?;
    let config = engine_config()?;
    let engine = tauri::async_runtime::block_on(Engine::open(config))?;
    // the window: portable mode gives it a profile folder next to the app
    webview::create_main_window(app, &paths)?;
    app.manage(engine.clone());
    // video support: the bundled (or updated) yt-dlp and ffmpeg
    let data_dir = riptide_log::Paths::resolve()?.data_dir;
    let tools = media::resolve(&data_dir);
    match &tools {
        Some(t) => {
            tracing::info!(yt_dlp = %t.exe().display(), ffmpeg = t.ffmpeg().is_some(), "video support found")
        }
        None => tracing::warn!("video support not installed (no yt-dlp)"),
    }
    engine.set_media_tools(tools);
    // AI (off until configured), research and the download index
    app.manage(ai::service_for(&media::DataDir(data_dir.clone())));
    app.manage(research::ResearchState::default());
    app.manage(media::DataDir(data_dir));

    // launched by a magnet link or a .torrent file: the webview asks for it once it is up
    if let Some(target) = protocol::open_target(&std::env::args().collect::<Vec<_>>()) {
        app.state::<protocol::PendingOpen>().set(target);
    }

    // torrents already in the list continue; with none, no port is opened (privacy)
    let torrent_engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        torrent::start_if_needed(&torrent_engine).await;
    });

    // tray, notifications, clipboard monitor; started by Windows → stay in the tray
    if let Err(e) = tray::install(app.handle(), engine.clone()) {
        tracing::warn!(error = %e, "no tray icon");
    }
    notify::install(app.handle(), engine.clone());
    clipboard::install(app.handle());
    if shell::started_minimized(std::env::args()) {
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            let _ = window.hide();
        }
    }

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
            // an upgrade keeps working registrations of the earlier form, and moves
            // them to the current one
            if let Ok(exe) = std::env::current_exe() {
                protocol::migrate_legacy(&exe, registry.as_mut());
            }
        }
    });

    // engine → webview: one Tauri event per engine event, dropped if the webview lags
    let handle = app.handle().clone();
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    // a queue with a post-queue action finished: the countdown dialog
                    // must be seen, even when the window sits in the tray
                    if matches!(
                        &event,
                        riptide_core::Event::QueueFinished {
                            action: Some(_),
                            ..
                        }
                    ) {
                        show_main_window(&handle);
                    }
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

    #[cfg(feature = "update-e2e")]
    updater::e2e_autorun(app.handle().clone());
    Ok(())
}

/// The text of the box shown when the app cannot start: English and Arabic together (the
/// engine is not up, so the language setting may be unreadable), the cause, and where the log
/// is.
pub fn startup_failure_message(error: &(dyn std::error::Error + 'static), logs: &str) -> String {
    let newer = matches!(
        error.downcast_ref::<riptide_engine::Error>(),
        Some(riptide_engine::Error::Store(
            riptide_store::Error::TooNew { .. }
        ))
    );
    let (hint_en, hint_ar) = if newer {
        (
            "The data was written by a newer version of Riptide. Install that version again.",
            "البيانات كُتبت بإصدار أحدث من Riptide. ثبّت ذلك الإصدار من جديد.",
        )
    } else {
        (
            "Check that the data folder is writable and that Riptide is not already running.",
            "تأكد أن مجلد البيانات قابل للكتابة وأن Riptide ليس قيد التشغيل.",
        )
    };
    format!(
        "Riptide could not start.\n{hint_en}\n\n{error}\n\nLogs: {logs}\n\n\
         تعذّر تشغيل Riptide.\n{hint_ar}\n\nالسجلات: {logs}"
    )
}

/// Logs a startup failure, shows it in a native message box and exits with code 1. The box
/// opens from its own thread so the event loop keeps running while it is up.
fn report_startup_failure(app: &tauri::AppHandle, error: &(dyn std::error::Error + 'static)) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

    tracing::error!(error = %error, "the app could not start");
    let logs = riptide_log::Paths::resolve()
        .map(|p| p.logs_dir.display().to_string())
        .unwrap_or_default();
    let text = startup_failure_message(error, &logs);
    let handle = app.clone();
    std::thread::spawn(move || {
        handle
            .dialog()
            .message(text)
            .title("Riptide")
            .kind(MessageDialogKind::Error)
            .blocking_show();
        handle.exit(1);
    });
}

/// Builds and runs the Tauri application; returns when the last window closes.
///
/// # Exits
///
/// A startup failure (a database that cannot be opened, a window that cannot be created) is
/// shown in a message box and ends the process with code 1; a Tauri runtime failure (missing
/// WebView2, unreadable configuration) is logged and ends it with code 1 too.
pub fn run() {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        build = BUILD_LABEL,
        "starting Riptide desktop"
    );
    let result = tauri::Builder::default()
        // first: a second launch hands over to the running instance and exits before it opens
        // the database (its crash recovery would otherwise re-queue the live transfers)
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // a magnet link or .torrent file launched while Riptide runs opens its file choice
            protocol::handle_second_launch(app, &args);
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(exit::ExitGuard::default())
        .plugin(updater::plugin())
        .manage(updater::PendingUpdate::default())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![shell::MINIMIZED_ARG]),
        ))
        .manage(shell::Shell::default())
        .manage(protocol::PendingOpen::default())
        .manage(std::sync::Arc::new(crawl::Crawls::default()))
        .manage(std::sync::Arc::new(files::FileOps::default()))
        .setup(|app| {
            // a failure here would otherwise end the process silently (there is no console in a
            // release build): say what happened, then exit with an error code
            if let Err(e) = startup(app) {
                report_startup_failure(app.handle(), e.as_ref());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // close-to-tray first: when it hid the window, downloads keep going
                if shell::close_to_tray(window, api) {
                    return;
                }
                // active downloads + confirmOnExit: the webview asks, then calls `quit_app`
                if exit::confirm_before_close(window, api) {
                    return;
                }
                // stop transfers at a checkpoint; their records stay Downloading and resume on
                // the next start (crash-safe resume).
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
            commands::batch_expand,
            commands::batch_import,
            commands::add_batch,
            crawl::start_crawl,
            crawl::cancel_crawl,
            crawl::discard_crawl,
            crawl::add_crawl_files,
            ai::ai_status,
            ai::ai_save_settings,
            ai::ai_set_key,
            ai::ai_remove_key,
            ai::ai_list_models,
            ai::ai_test_connection,
            ai::ai_last_request,
            ai::ai_forget_all,
            ai::ai_suggest_name,
            clipboard::clipboard_magnets,
            research::research_start,
            research::research_cancel,
            research::research_retry_source,
            research::research_settings,
            research::research_save_settings,
            research::research_set_secret,
            research::research_remove_secret,
            research::research_preview,
            research::research_add,
            research::research_thumbnail,
            research::seen_check,
            research::seen_settings,
            research::seen_set_skip,
            research::seen_forget,
            commands::pick_text_file,
            commands::save_text_file,
            commands::open_web_url,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::restart_download,
            commands::remove_download,
            commands::pause_all,
            commands::resume_all,
            commands::get_engine_settings,
            commands::set_engine_settings,
            commands::set_speed_limit,
            exit::set_exit_guard,
            exit::quit_app,
            commands::disk_free,
            commands::open_file,
            commands::show_in_folder,
            commands::pick_folder,
            files::fs_places,
            files::fs_list,
            files::fs_entry,
            files::fs_create_folder,
            files::fs_rename,
            files::fs_trash,
            files::fs_trashed,
            files::fs_restore,
            files::fs_properties,
            files::fs_search,
            files::fs_plan,
            files::fs_transfer,
            files::fs_cancel,
            commands::integration_status,
            commands::register_browser_host,
            commands::unregister_browser_host,
            extension::extension_info,
            extension::open_extension_folder,
            extension::open_extensions_page,
            media::probe_media,
            media::media_tools_info,
            media::update_media_tools,
            updater::check_update,
            updater::download_update,
            updater::active_download_count,
            updater::install_update,
            updater::open_release_page,
            notify::notify_update,
            queues::list_queues,
            queues::queue_items,
            queues::create_queue,
            queues::update_queue,
            queues::delete_queue,
            queues::move_to_queue,
            queues::start_queue,
            queues::stop_queue,
            queues::reorder_queue_items,
            queues::reorder_queues,
            queues::validate_post_action,
            queues::run_post_action,
            torrent::torrent_preview,
            torrent::add_torrent,
            torrent::torrent_web_seed_add,
            torrent::list_torrents,
            torrent::torrent_live,
            torrent::pick_torrent_file,
            protocol::protocol_status,
            protocol::register_protocols,
            protocol::unregister_protocols,
            protocol::take_pending_open,
            shell::set_shell_prefs,
            shell::autostart_enabled,
            shell::set_autostart,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        // Tauri itself failed (no WebView2, unreadable configuration): no window and no dialog
        // plugin to tell the user, so the log and the exit code are all there is
        tracing::error!(error = %e, "the Riptide desktop application stopped with an error");
        eprintln!("riptide: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_launch_never_changes_the_log_level() {
        let link = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=x";
        for list in [
            // the registered form, with a link that was split into extra arguments
            vec!["riptide", "--open-link", "--", link, "--log-level", "trace"],
            vec!["riptide", "--open-link", "--", link, "--log-level=trace"],
            // an earlier registration (`"exe" "%1"`)
            vec!["riptide", link, "--log-level", "trace"],
        ] {
            let all: Vec<String> = list.iter().map(|s| (*s).to_string()).collect();
            assert_eq!(launch_log_filter(&all, None), "info", "{list:?}");
        }
        // an ordinary launch still honours the flag
        let plain: Vec<String> = ["riptide", "--log-level", "debug"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(launch_log_filter(&plain, None), "debug");
    }

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
    fn the_startup_box_has_both_languages_the_cause_and_the_log_folder() {
        let too_new = riptide_engine::Error::Store(riptide_store::Error::TooNew {
            found: 9,
            supported: 3,
        });
        let text = startup_failure_message(&too_new, "C:\\Riptide\\logs");
        assert!(text.contains("could not start") && text.contains("newer version"));
        assert!(text.contains("تعذّر تشغيل") && text.contains("أحدث"));
        assert!(text.contains("database schema version 9"));
        assert_eq!(text.matches("C:\\Riptide\\logs").count(), 2);

        let locked = std::io::Error::other("the file is locked");
        let text = startup_failure_message(&locked, "L");
        assert!(text.contains("the file is locked") && text.contains("writable"));
        assert!(text.contains("قابل للكتابة") && !text.contains("newer version"));
    }

    #[test]
    fn engine_config_points_inside_the_data_dir_or_downloads() {
        let cfg = engine_config().unwrap();
        assert!(cfg.db_path.ends_with("riptide.sqlite"));
        assert!(cfg.default_dir.is_absolute());
    }
}
