//! Logging and data-directory resolution shared by the desktop app and the CLI.
//!
//! * [`init`] installs a `tracing` subscriber that writes daily-rotated files into the logs
//!   directory and, optionally, to stderr. Every line passes through the redaction layer
//!   ([`redact`]) so cookies, auth headers, tokens, passwords and URL query strings never reach
//!   disk.
//! * [`Paths`] resolves where Riptide keeps its data: `%LOCALAPPDATA%\Riptide` (or the XDG data
//!   dir), or `data/` next to the executable when a `portable.txt` marker exists.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

mod init;
pub mod paths;
pub mod redact;

pub use init::{build, init, list_log_files, LogConfig, LogGuard};
pub use paths::Paths;
pub use redact::{redact_line, redact_url, RedactingWriter};

/// Errors from logging setup and path resolution.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The data or logs directory could not be created or written.
    #[error("cannot use directory {path}: {source}")]
    Directory {
        /// Directory that failed.
        path: std::path::PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// The executable location or the per-user data directory could not be determined.
    #[error("cannot resolve data directory: {0}")]
    Resolve(String),
    /// The `--log-level` / `RUST_LOG` filter string is malformed.
    #[error("invalid log filter {filter:?}: {reason}")]
    Filter {
        /// The offending filter.
        filter: String,
        /// Parser message.
        reason: String,
    },
    /// A global subscriber was already installed (only one `init` per process).
    #[error("a tracing subscriber is already installed")]
    AlreadyInitialised,
    /// The rolling file appender could not be built.
    #[error("cannot open log file: {0}")]
    Appender(String),
}
