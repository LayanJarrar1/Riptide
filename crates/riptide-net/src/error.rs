//! Engine errors and the one question the scheduler asks of them: is a retry worth it?

use std::path::PathBuf;

use riptide_core::ChecksumAlgo;

/// Everything that can go wrong while probing or transferring.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `reqwest` failed to connect, send or read.
    #[error("request failed: {0}")]
    Request(#[source] reqwest::Error),
    /// Disk.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The URL could not be parsed or is not http(s).
    #[error("invalid URL {url:?}: {reason}")]
    InvalidUrl {
        /// The offending URL (query string already stripped for logs).
        url: String,
        /// Why.
        reason: String,
    },
    /// The proxy URL is not usable (the message never contains its credentials).
    #[error("invalid proxy: {0}")]
    InvalidProxy(String),
    /// More than the allowed number of redirects.
    #[error("too many redirects (limit {max})")]
    TooManyRedirects {
        /// The limit.
        max: usize,
    },
    /// A response status the engine cannot work with.
    #[error("server answered {status}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Whether waiting and trying again makes sense (5xx, 429, 408).
        retryable: bool,
        /// `Retry-After` in seconds if the server said so.
        retry_after: Option<u64>,
    },
    /// 401/407 even after answering the challenge (or no credentials to answer it with).
    #[error("authentication failed ({scheme})")]
    AuthFailed {
        /// Challenge scheme the server asked for.
        scheme: String,
    },
    /// The server's `ETag`/`Last-Modified` no longer match what the partial file was fetched
    /// with, or it answered `200` to a ranged resume: the file changed, restart from zero.
    #[error("the file on the server changed; restarting from zero")]
    ValidatorChanged,
    /// `Content-Length` promised one size, the body delivered another.
    #[error("Content-Length declared {declared} bytes but {received} arrived")]
    LengthMismatch {
        /// Declared.
        declared: u64,
        /// Received.
        received: u64,
    },
    /// The finished file does not hash to the expected digest.
    #[error("checksum mismatch ({algo}): expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Algorithm.
        algo: ChecksumAlgo,
        /// Expected hex.
        expected: String,
        /// Actual hex.
        actual: String,
    },
    /// The final path is taken and overwriting was not allowed.
    #[error("{0} already exists")]
    TargetExists(PathBuf),
    /// Stopped by the caller (pause or cancel).
    #[error("stopped")]
    Stopped,
    /// A segment failed more times than the policy allows.
    #[error("gave up after {attempts} attempts: {last}")]
    RetriesExhausted {
        /// Attempts made.
        attempts: u32,
        /// The last error.
        #[source]
        last: Box<Error>,
    },
    /// The server broke the protocol (bad `Content-Range`, missing headers, …).
    #[error("unexpected response: {0}")]
    Protocol(String),
}

impl Error {
    /// Whether the scheduler should wait and try again.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Request(e) => {
                // connect, timeout, reset, incomplete body: yes; a builder/redirect bug: no
                e.is_connect() || e.is_timeout() || e.is_request() || e.is_body() || e.is_decode()
            }
            Error::Status { retryable, .. } => *retryable,
            Error::Io(e) => matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::Interrupted
            ),
            Error::RetriesExhausted { .. } => false,
            _ => false,
        }
    }

    /// `Retry-After` hint, if the error carries one.
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Error::Status { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Builds a [`Error::Status`] with the standard retryable classification.
    pub fn status(status: u16, retry_after: Option<u64>) -> Error {
        let retryable = status >= 500 || status == 429 || status == 408;
        Error::Status {
            status,
            retryable,
            retry_after,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Request(e.without_url())
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
