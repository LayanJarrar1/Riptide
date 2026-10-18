//! Store error type.

use riptide_core::{DownloadId, QueueId};

/// Everything that can go wrong in the store.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// SQLite itself.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// A model rule was violated (for example an illegal state transition).
    #[error(transparent)]
    Core(#[from] riptide_core::Error),
    /// The download does not exist.
    #[error("download {} not found", .0 .0)]
    NotFound(DownloadId),
    /// The queue does not exist.
    #[error("queue {} not found", .0 .0)]
    QueueNotFound(QueueId),
    /// A row holds a value the model cannot represent (bad enum name, negative size, …).
    #[error("corrupt row: {0}")]
    Corrupt(String),
    /// A migration failed and was rolled back.
    #[error("migration {version} ({name}) failed and was rolled back: {source}")]
    Migration {
        /// Version that failed.
        version: u32,
        /// Its name.
        name: String,
        /// SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// The file was written by a newer Riptide.
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    TooNew {
        /// Version in the file.
        found: u32,
        /// Highest version this build knows.
        supported: u32,
    },
    /// JSON (headers in the secrets table).
    #[error("corrupt JSON in {what}: {source}")]
    Json {
        /// Which column.
        what: &'static str,
        /// Parser error.
        #[source]
        source: serde_json::Error,
    },
}
