//! Riptide desktop shell: the Tauri v2 backend that hosts the Angular UI.
//!
//! This crate is the thin adapter between the engine crates (`riptide-core`, `-net`,
//! `-store`, …) and the webview. The engine never sees a Tauri type (Hard rule 9); everything
//! the UI can ask for is a `#[tauri::command]` in [`commands`].
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod commands;

pub use commands::{AppInfo, BUILD_MILESTONE, PRODUCT_NAME};

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

/// Builds and runs the Tauri application; returns when the last window closes.
///
/// # Panics
///
/// Panics if the runtime cannot start (missing WebView2, unreadable configuration). There is
/// no useful way to continue without a window, so this is the one place a panic is intended.
pub fn run() {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        build = BUILD_MILESTONE,
        "starting Riptide desktop"
    );
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::app_info])
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
}
