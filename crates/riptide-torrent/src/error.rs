//! Errors of the torrent engine.

use std::path::PathBuf;

/// Everything that can go wrong while adding, running or removing a torrent.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The magnet link is malformed, has no BitTorrent v1 info-hash, or is absurdly large.
    #[error("invalid magnet link: {0}")]
    InvalidMagnet(String),
    /// The `.torrent` data is malformed or breaks one of the sanity limits in [`crate::limits`].
    #[error("invalid torrent: {0}")]
    InvalidTorrent(String),
    /// A path would end up outside the chosen download folder.
    #[error("refusing to write outside the download folder: {0}")]
    PathEscape(PathBuf),
    /// The file selection is empty or names a file that does not exist.
    #[error("invalid file selection: {0}")]
    InvalidSelection(String),
    /// No torrent with this id.
    #[error("no such torrent: {0}")]
    NotFound(String),
    /// The settings are not acceptable.
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    /// The metadata of a magnet link did not arrive in time.
    #[error("timed out waiting for torrent metadata (no peers found?)")]
    MetadataTimeout,
    /// Disk error.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// Error reported by the BitTorrent library.
    #[error("torrent engine: {0}")]
    Engine(String),
    /// The state file could not be read or written.
    #[error("torrent state file: {0}")]
    State(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<librqbit::Error> for Error {
    fn from(e: librqbit::Error) -> Self {
        Error::Engine(e.to_string())
    }
}
