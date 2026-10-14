//! Subscriber construction.

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};

use crate::redact::RedactingWriter;
use crate::Error;

/// How to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    /// Directory for the daily files (created if missing).
    pub dir: PathBuf,
    /// `tracing_subscriber::EnvFilter` directive, e.g. `"info"` or `"riptide_net=debug,info"`.
    /// This is what `--log-level` sets; `RUST_LOG` is the CLI's fallback for it.
    pub filter: String,
    /// File name prefix: files are `<prefix>.YYYY-MM-DD.log`.
    pub file_prefix: String,
    /// Also print (redacted) lines to stderr. On for the CLI, off for the desktop app.
    pub stderr: bool,
    /// Keep at most this many daily files; older ones are deleted on rotation.
    pub max_files: usize,
}

impl LogConfig {
    /// Default filter when nothing is given.
    pub const DEFAULT_FILTER: &'static str = "info";

    /// Sensible defaults for `dir`: `info`, prefix `riptide`, no stderr, 14 files.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        LogConfig {
            dir: dir.into(),
            filter: Self::DEFAULT_FILTER.to_string(),
            file_prefix: "riptide".to_string(),
            stderr: false,
            max_files: 14,
        }
    }

    /// Sets the filter directive.
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = filter.into();
        self
    }

    /// Enables or disables the stderr mirror.
    pub fn with_stderr(mut self, stderr: bool) -> Self {
        self.stderr = stderr;
        self
    }
}

/// Keeps the background writer alive. Drop it last (at the end of `main`) so the final lines are
/// flushed to disk.
#[derive(Debug)]
#[must_use = "dropping the guard stops the log writer; keep it until the process exits"]
pub struct LogGuard {
    _file: WorkerGuard,
}

/// Builds the subscriber without installing it. Used by [`init`] and by tests, which install it
/// with `tracing::subscriber::with_default` instead of globally.
pub fn build(
    config: &LogConfig,
) -> Result<(impl tracing::Subscriber + Send + Sync + 'static, LogGuard), Error> {
    let filter = EnvFilter::try_new(&config.filter).map_err(|e| Error::Filter {
        filter: config.filter.clone(),
        reason: e.to_string(),
    })?;

    std::fs::create_dir_all(&config.dir).map_err(|source| Error::Directory {
        path: config.dir.clone(),
        source,
    })?;

    let file = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix(&config.file_prefix)
        .filename_suffix("log")
        .max_log_files(config.max_files.max(1))
        .build(&config.dir)
        .map_err(|e| Error::Appender(e.to_string()))?;
    let (file_writer, file_guard) = tracing_appender::non_blocking(RedactingWriter::new(file));

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_writer(file_writer);

    let stderr_layer = config.stderr.then(|| {
        fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_writer(|| RedactingWriter::new(std::io::stderr()))
    });

    let subscriber = Registry::default()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer);
    Ok((subscriber, LogGuard { _file: file_guard }))
}

/// Installs the global subscriber. Call once, early in `main`, and keep the guard alive.
pub fn init(config: &LogConfig) -> Result<LogGuard, Error> {
    let (subscriber, guard) = build(config)?;
    tracing::subscriber::set_global_default(subscriber).map_err(|_| Error::AlreadyInitialised)?;
    tracing::info!(
        dir = %config.dir.display(),
        filter = %config.filter,
        "logging started"
    );
    Ok(guard)
}

/// Lists the log files in `dir`, newest first, for the "open logs" / support bundle features.
pub fn list_log_files(dir: &Path, prefix: &str) -> Result<Vec<PathBuf>, Error> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| Error::Directory {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".log"))
        })
        .collect();
    files.sort();
    files.reverse();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_filter_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = LogConfig::new(tmp.path()).with_filter("this is [not] a filter");
        let err = build(&cfg).err().unwrap();
        assert!(matches!(err, Error::Filter { .. }), "{err}");
    }

    #[test]
    fn build_creates_the_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nested").join("logs");
        let cfg = LogConfig::new(&dir);
        let (_sub, guard) = build(&cfg).unwrap();
        assert!(dir.is_dir());
        drop(guard);
    }
}
