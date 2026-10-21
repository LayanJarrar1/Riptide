//! Riptide desktop application entry point.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let all: Vec<String> = std::env::args().collect();
    let filter = riptide_desktop_lib::launch_log_filter(&all, std::env::var("RUST_LOG").ok());
    // Keep the guard alive for the whole process so the last lines are flushed on exit.
    let log_guard = riptide_desktop_lib::start_logging(&filter);
    // The installer hooks: `--register-host` / `--unregister-host` run headless and exit. A launch
    // that opens a link or file never runs them (see `integration::run_cli`).
    let args: Vec<String> = all.iter().skip(1).cloned().collect();
    if let Some(code) = riptide_desktop_lib::integration::run_cli(&args) {
        drop(log_guard);
        std::process::exit(code);
    }
    riptide_desktop_lib::run();
}
