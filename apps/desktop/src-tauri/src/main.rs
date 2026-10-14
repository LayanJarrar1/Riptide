//! Riptide desktop application entry point.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let filter = riptide_desktop_lib::log_filter(std::env::args(), std::env::var("RUST_LOG").ok());
    // Keep the guard alive for the whole process so the last lines are flushed on exit.
    let _log_guard = riptide_desktop_lib::start_logging(&filter);
    riptide_desktop_lib::run();
}
