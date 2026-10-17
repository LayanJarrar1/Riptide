//! `riptide-host`: the Native Messaging host binary. The browser starts it; see the library
//! documentation for the protocol and the rules it enforces.
//!
//! stdout carries protocol frames only; diagnostics go to the daily log file
//! (`<data>/logs/host.*`), never to stdout or stderr.

use std::path::PathBuf;
use std::process::ExitCode;

use riptide_host::{
    parse_caller, run_session, AllowList, Bridge, IntegrationSettings, Launcher, DATA_DIR_ENV,
    ENDPOINT_ENV, INTEGRATION_FILE,
};

fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    riptide_log::Paths::resolve().ok().map(|p| p.data_dir)
}

fn start_logging(data_dir: Option<&PathBuf>) -> Option<riptide_log::LogGuard> {
    let paths = riptide_log::Paths::in_dir(data_dir?, false);
    paths.ensure_dirs().ok()?;
    let mut config = riptide_log::LogConfig::new(&paths.logs_dir).with_stderr(false);
    config.file_prefix = "host".to_string();
    riptide_log::init(&config).ok()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir = data_dir();
    let _log_guard = start_logging(data_dir.as_ref());

    let caller = parse_caller(&args);
    let settings = data_dir
        .as_ref()
        .map(|d| IntegrationSettings::load(&d.join(INTEGRATION_FILE)))
        .unwrap_or_default();
    let allow = AllowList::new(&settings);

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return ExitCode::from(1),
    };
    let end = runtime.block_on(async {
        let endpoint = match std::env::var(ENDPOINT_ENV) {
            Ok(e) if !e.is_empty() => e,
            _ => match riptide_ipc::default_endpoint() {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!(error = %e, "cannot resolve the app endpoint");
                    String::new()
                }
            },
        };
        let mut bridge = Bridge::new(endpoint, Launcher::for_this_host());
        let mut stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        run_session(
            &mut stdin,
            &mut stdout,
            caller.as_ref(),
            &allow,
            &mut bridge,
        )
        .await
    });
    tracing::info!(end = ?end, "browser session ended");
    ExitCode::from(end.exit_code())
}
