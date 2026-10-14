//! IPC error type.

/// Everything that can go wrong on the local channel.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Socket / pipe IO.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Malformed JSON or a message that does not match the schema.
    #[error("invalid message: {0}")]
    Json(#[from] serde_json::Error),
    /// A frame larger than [`crate::MAX_FRAME_BYTES`] was sent or announced.
    #[error("frame of {len} bytes exceeds the {max}-byte limit")]
    FrameTooLarge {
        /// Announced or actual length.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The endpoint name could not be determined (no user SID / runtime dir).
    #[error("cannot resolve IPC endpoint: {0}")]
    Endpoint(String),
    /// A platform call failed.
    #[error("{what} failed with OS error {code}")]
    Platform {
        /// The call.
        what: &'static str,
        /// `GetLastError()` / `errno`.
        code: u32,
    },
    /// Every pipe instance was busy for the whole retry window.
    #[error("the Riptide endpoint is busy; try again")]
    Busy,
    /// The endpoint does not exist: the app is not running.
    #[error("the Riptide app is not running (no endpoint at {0})")]
    NotRunning(String),
}
