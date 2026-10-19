//! Video pages (YouTube, Vimeo, ...) through a bundled **yt-dlp** (Unlicense) sidecar.
//!
//! Pages such as YouTube stream through Media Source Extensions: the player fetches separate
//! audio and video ranges, so there is no file for a browser download to capture. yt-dlp knows
//! how to list a page's formats and fetch them; this crate drives it as a child process, the
//! same way for the desktop app and the CLI:
//!
//! * [`YtDlp::probe`] runs `yt-dlp -J --no-playlist` and turns the JSON into a
//!   [`riptide_core::MediaInfo`] with a ready-made quality menu. Live streams and pages whose
//!   formats are all DRM-protected are refused ([`Error::Live`], [`Error::Drm`]).
//! * [`YtDlp::download`] runs the download with `--newline --progress-template` and reports
//!   [`MediaProgress`] (downloaded, total, speed, ETA). Pause = stop the process; resume = run
//!   the same job again: yt-dlp continues its `.part` files (`--continue`).
//! * Cookies from the browser go to yt-dlp in a temporary Netscape cookie file
//!   ([`CookieFile`]) that only the current user can read and that is deleted when the run
//!   ends. Cookie values are never logged.
//!
//! No flag that bypasses DRM is ever passed (`--allow-unplayable-formats` and friends); see
//! [`args`]. Every argument is built by this crate from validated values: the URL must be
//! `http(s)` and follows `--`, the quality is a [`riptide_core::MediaSelection`], and the output
//! name is sanitised by `riptide-core`.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

pub mod args;
pub mod cookies;
pub mod ladder;
pub mod parse;
pub mod runner;

use std::path::PathBuf;

pub use args::PlayerClients;
pub use cookies::CookieFile;
pub use ladder::{Attempt, Ladder, MAX_ATTEMPTS};
pub use parse::{parse_info, parse_progress_line, ErrorClass, ProgressLine, ProgressTracker};
pub use runner::{is_partial_name, partial_files, DownloadJob, MediaOutcome, YtDlp};

/// Errors from probing or downloading a media page.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The yt-dlp executable is missing (not bundled, or not fetched in a dev build).
    #[error("video support is not installed (yt-dlp was not found)")]
    ToolMissing(PathBuf),
    /// Every format is DRM-protected.
    #[error("protected content: this video uses DRM and cannot be downloaded")]
    Drm,
    /// A live or upcoming stream.
    #[error("live streams are not supported")]
    Live,
    /// A playlist or channel URL.
    #[error("playlists are not supported; open a single video")]
    Playlist,
    /// yt-dlp does not know the page.
    #[error("this page has no video Riptide can download")]
    Unsupported,
    /// Not an `http(s)` URL.
    #[error("not an http(s) address")]
    InvalidUrl,
    /// The quality key is not one of [`riptide_core::MediaSelection`]'s.
    #[error("unknown quality {0:?}")]
    InvalidFormat(String),
    /// yt-dlp reported an error (its message, without the `ERROR:` prefix). The text is
    /// `[rt:login] ...` or `[rt:refused] ...` when the error is a known kind of refusal, so the
    /// apps can show a friendly message and keep the raw line behind "Details"; a missing
    /// JavaScript runtime adds a hint after ` | `.
    #[error("{}", tool_text(message, *class, *js_runtime_missing))]
    Tool {
        /// What yt-dlp said (one line, at most 300 characters).
        message: String,
        /// Worth retrying later (network trouble, HTTP 5xx, rate limits, site refusals).
        retryable: bool,
        /// Which kind of refusal this is.
        class: ErrorClass,
        /// yt-dlp warned that no JavaScript runtime is available.
        js_runtime_missing: bool,
    },
    /// yt-dlp printed something this crate could not understand.
    #[error("unexpected yt-dlp output: {0}")]
    Parse(String),
    /// yt-dlp did not answer in time.
    #[error("yt-dlp did not answer in time")]
    Timeout,
    /// Disk or process error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The Details hint for a failure that happened without a JavaScript runtime.
pub const JS_RUNTIME_HINT: &str = "YouTube extraction needs a JavaScript runtime for some videos; Riptide will gain one in a future release";

fn tool_text(message: &str, class: ErrorClass, js_runtime_missing: bool) -> String {
    let mut text = match class.tag() {
        Some(tag) => format!("[rt:{tag}] {message}"),
        None => message.to_string(),
    };
    if js_runtime_missing {
        text.push_str(" | ");
        text.push_str(JS_RUNTIME_HINT);
    }
    text
}

impl Error {
    /// The refusal class of a yt-dlp error, [`ErrorClass::Other`] for everything else.
    pub fn class(&self) -> ErrorClass {
        match self {
            Error::Tool { class, .. } => *class,
            _ => ErrorClass::Other,
        }
    }

    fn lower_message(&self) -> Option<String> {
        match self {
            Error::Tool { message, .. } => Some(message.to_ascii_lowercase()),
            _ => None,
        }
    }

    /// The error says a signed-in user is needed (age gate, private video, bot check).
    pub fn needs_login(&self) -> bool {
        self.lower_message().is_some_and(|m| parse::needs_login(&m))
    }

    /// The error says the page state or the cookies bound to it are stale.
    pub fn is_stale_page(&self) -> bool {
        self.lower_message().is_some_and(|m| parse::page_reload(&m))
    }

    /// Another YouTube player client may get past this error.
    pub fn client_retryable(&self) -> bool {
        self.lower_message()
            .is_some_and(|m| parse::client_retryable(&m))
    }

    /// Whether a later retry may succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Tool { retryable, .. } => *retryable,
            Error::Timeout | Error::Io(_) => true,
            _ => false,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Progress of a media download, summed over every stream yt-dlp fetches (video, then audio).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaProgress {
    /// Bytes on disk so far.
    pub downloaded: u64,
    /// Expected total, if known (an estimate for most sites).
    pub total: Option<u64>,
    /// Current speed in bytes per second.
    pub speed_bps: u64,
    /// Seconds left, if it can be estimated.
    pub eta_secs: Option<u64>,
}
