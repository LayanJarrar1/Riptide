//! The `Download` record and the small value types it is made of.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use crate::state::DownloadState;

/// Milliseconds since the Unix epoch, UTC. Kept as a plain integer so the core has no clock
/// dependency; the store and the UI format it.
pub type UnixMillis = i64;

/// Default number of connections for a new download.
pub const DEFAULT_CONNECTIONS: u32 = 8;
/// Hard upper bound on connections per download.
pub const MAX_CONNECTIONS: u32 = 32;

/// Row id of a download. Assigned by the store.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(#[cfg_attr(feature = "ts", ts(type = "number"))] pub i64);

/// Row id of a queue. Queue `1` is the built-in *Main* queue.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QueueId(#[cfg_attr(feature = "ts", ts(type = "number"))] pub i64);

impl QueueId {
    /// The built-in *Main* queue that every install has.
    pub const MAIN: QueueId = QueueId(1);
}

/// Row id of a category (Compressed, Documents, …).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CategoryId(#[cfg_attr(feature = "ts", ts(type = "number"))] pub i64);

/// Hash algorithm for post-completion verification.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgo {
    /// MD5 (128 bit). Weak, but what many sites publish.
    Md5,
    /// SHA-1 (160 bit).
    Sha1,
    /// SHA-256 (256 bit).
    Sha256,
}

impl ChecksumAlgo {
    /// Stable lower-case name (`"md5"`, `"sha1"`, `"sha256"`).
    pub const fn name(self) -> &'static str {
        match self {
            ChecksumAlgo::Md5 => "md5",
            ChecksumAlgo::Sha1 => "sha1",
            ChecksumAlgo::Sha256 => "sha256",
        }
    }

    /// Inverse of [`Self::name`]; accepts a few common spellings (`sha-256`, `SHA256`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().replace('-', "").as_str() {
            "md5" => Some(ChecksumAlgo::Md5),
            "sha1" => Some(ChecksumAlgo::Sha1),
            "sha256" => Some(ChecksumAlgo::Sha256),
            _ => None,
        }
    }

    /// Number of hex characters a digest of this algorithm has.
    pub const fn hex_len(self) -> usize {
        match self {
            ChecksumAlgo::Md5 => 32,
            ChecksumAlgo::Sha1 => 40,
            ChecksumAlgo::Sha256 => 64,
        }
    }
}

impl fmt::Display for ChecksumAlgo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An expected or computed digest, lower-case hex.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Checksum {
    /// Algorithm.
    pub algo: ChecksumAlgo,
    /// Lower-case hexadecimal digest.
    pub hex: String,
}

impl Checksum {
    /// Builds a checksum, normalising the hex to lower case and validating its length and
    /// alphabet. Returns `None` if `hex` is not a valid digest for `algo`.
    pub fn new(algo: ChecksumAlgo, hex: &str) -> Option<Self> {
        let hex = hex.trim().to_ascii_lowercase();
        if hex.len() != algo.hex_len() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(Checksum { algo, hex })
    }

    /// Guesses the algorithm from the digest length (32 → MD5, 40 → SHA-1, 64 → SHA-256).
    pub fn guess(hex: &str) -> Option<Self> {
        let algo = match hex.trim().len() {
            32 => ChecksumAlgo::Md5,
            40 => ChecksumAlgo::Sha1,
            64 => ChecksumAlgo::Sha256,
            _ => return None,
        };
        Self::new(algo, hex)
    }
}

/// Scheduling priority inside a queue.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Runs after everything else.
    Low,
    /// The default.
    #[default]
    Normal,
    /// Runs before everything else.
    High,
}

impl Priority {
    /// Stable lower-case name.
    pub const fn name(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
        }
    }

    /// Inverse of [`Self::name`].
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "low" => Some(Priority::Low),
            "normal" => Some(Priority::Normal),
            "high" => Some(Priority::High),
            _ => None,
        }
    }
}

/// One download as persisted in the `downloads` table. Secrets are **not** here — see
/// [`Secrets`], which lives in its own table and is wiped on completion or removal.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    /// Row id.
    pub id: DownloadId,
    /// URL as the user or the browser gave it.
    pub url: String,
    /// URL after redirects, once probed.
    pub final_url: Option<String>,
    /// Sanitised file name (no directory part). Final name on disk; the partial file is
    /// `<filename>.rtpart` next to it.
    pub filename: String,
    /// Destination directory.
    pub dir: PathBuf,
    /// Total size in bytes if the server told us.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size: Option<u64>,
    /// Whether the server honours `Range` (learned by the probe).
    pub supports_ranges: bool,
    /// `ETag` captured at probe time; used with `If-Range` on every resume.
    pub etag: Option<String>,
    /// `Last-Modified` captured at probe time (raw header value).
    pub last_modified: Option<String>,
    /// `Content-Type` from the probe.
    pub content_type: Option<String>,
    /// Lifecycle state.
    pub state: DownloadState,
    /// Category (by extension) if assigned.
    pub category_id: Option<CategoryId>,
    /// Queue this download belongs to.
    pub queue_id: QueueId,
    /// Place in the queue's manual order (smaller runs first among equal priorities). Set by
    /// the store when the download is added, moved or reordered; ignored on insert/update.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub queue_position: i64,
    /// Priority within the queue.
    pub priority: Priority,
    /// Per-download speed cap in bytes per second, `None` for unlimited.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub speed_limit: Option<u64>,
    /// Maximum simultaneous connections for this download (1..=[`MAX_CONNECTIONS`]).
    pub connections: u32,
    /// Digest the user expects; checked after completion.
    pub checksum_expected: Option<Checksum>,
    /// Digest computed from the finished file.
    pub checksum_actual: Option<Checksum>,
    /// Human-readable error for `Failed`.
    pub error: Option<String>,
    /// When the item was added.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub added_at: UnixMillis,
    /// First time it left `Queued`.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub started_at: Option<UnixMillis>,
    /// When it reached `Completed`.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub completed_at: Option<UnixMillis>,
}

impl Download {
    /// A new download in its [`DownloadState::initial`] state with default settings.
    pub fn new(
        id: DownloadId,
        url: impl Into<String>,
        filename: impl Into<String>,
        dir: impl Into<PathBuf>,
        added_at: UnixMillis,
    ) -> Self {
        Download {
            id,
            url: url.into(),
            final_url: None,
            filename: filename.into(),
            dir: dir.into(),
            size: None,
            supports_ranges: false,
            etag: None,
            last_modified: None,
            content_type: None,
            state: DownloadState::initial(),
            category_id: None,
            queue_id: QueueId::MAIN,
            queue_position: 0,
            priority: Priority::Normal,
            speed_limit: None,
            connections: DEFAULT_CONNECTIONS,
            checksum_expected: None,
            checksum_actual: None,
            error: None,
            added_at,
            started_at: None,
            completed_at: None,
        }
    }

    /// Full path of the finished file.
    pub fn final_path(&self) -> PathBuf {
        self.dir.join(&self.filename)
    }

    /// Full path of the in-progress file (`<filename>.rtpart`).
    pub fn partial_path(&self) -> PathBuf {
        self.dir.join(format!("{}.rtpart", self.filename))
    }

    /// Applies a state transition, returning [`crate::Error::IllegalTransition`] if the table
    /// forbids it. On success `state` is updated and the result echoes the new state.
    pub fn transition(&mut self, to: DownloadState) -> Result<DownloadState, crate::Error> {
        self.state = self.state.transition(to)?;
        Ok(self.state)
    }

    /// Whether a resume can continue from the existing partial file: the server supports ranges
    /// and we know the size and at least one validator.
    pub fn can_resume_in_place(&self) -> bool {
        self.supports_ranges
            && self.size.is_some()
            && (self.etag.is_some() || self.last_modified.is_some())
    }
}

/// What a caller (UI, CLI, browser extension) provides to create a download. Everything but the
/// URL is optional and falls back to settings / probe results.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DownloadRequest {
    /// The URL to fetch.
    pub url: String,
    /// Destination directory; `None` = category default.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// File name override; `None` = from `Content-Disposition` / URL.
    #[serde(default)]
    pub filename: Option<String>,
    /// Queue; `None` = *Main*.
    #[serde(default)]
    pub queue_id: Option<QueueId>,
    /// Category override; `None` = by extension.
    #[serde(default)]
    pub category_id: Option<CategoryId>,
    /// Priority; `None` = normal.
    #[serde(default)]
    pub priority: Option<Priority>,
    /// Connections; `None` = settings default.
    #[serde(default)]
    pub connections: Option<u32>,
    /// Speed cap in bytes/s.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub speed_limit: Option<u64>,
    /// Expected digest.
    #[serde(default)]
    pub checksum: Option<Checksum>,
    /// Add as `Paused` instead of `Queued` ("start later").
    #[serde(default)]
    pub start_paused: bool,
    /// Referrer page (sent as `Referer`; not a secret).
    #[serde(default)]
    pub referrer: Option<String>,
    /// User agent to mimic (the browser's); `None` = Riptide's own.
    #[serde(default)]
    pub user_agent: Option<String>,
    /// Cookies, headers and credentials. Stored separately, never logged.
    #[serde(default)]
    pub secrets: Option<Secrets>,
}

impl DownloadRequest {
    /// A request with only the URL set.
    pub fn new(url: impl Into<String>) -> Self {
        DownloadRequest {
            url: url.into(),
            ..Default::default()
        }
    }
}

/// Username / password for HTTP Basic or Digest auth.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    /// User name.
    pub username: String,
    /// Password.
    pub password: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Per-download secrets: lives in the `secrets` table, deleted on completion/removal, never logged
/// (the `Debug` impl redacts every value) and only ever sent to the origin they were captured for.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Secrets {
    /// Raw `Cookie` header value.
    #[serde(default)]
    pub cookies: Option<String>,
    /// Extra request headers (name, value).
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// HTTP auth credentials.
    #[serde(default)]
    pub auth: Option<Credentials>,
}

impl Secrets {
    /// `true` when nothing is set.
    pub fn is_empty(&self) -> bool {
        self.cookies.is_none() && self.headers.is_empty() && self.auth.is_none()
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("cookies", &self.cookies.as_ref().map(|_| "[REDACTED]"))
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("auth", &self.auth.as_ref().map(|c| c.username.as_str()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_download_has_sane_defaults() {
        let d = Download::new(
            DownloadId(7),
            "https://x/y.zip",
            "y.zip",
            "C:/dl",
            1_700_000_000_000,
        );
        assert_eq!(d.state, DownloadState::Queued);
        assert_eq!(d.queue_id, QueueId::MAIN);
        assert_eq!(d.connections, DEFAULT_CONNECTIONS);
        assert_eq!(d.final_path(), PathBuf::from("C:/dl").join("y.zip"));
        assert_eq!(
            d.partial_path(),
            PathBuf::from("C:/dl").join("y.zip.rtpart")
        );
        assert!(!d.can_resume_in_place());
    }

    #[test]
    fn transition_updates_state_or_leaves_it_alone() {
        let mut d = Download::new(DownloadId(1), "u", "f", ".", 0);
        assert_eq!(
            d.transition(DownloadState::Probing),
            Ok(DownloadState::Probing)
        );
        assert_eq!(d.state, DownloadState::Probing);
        assert!(d.transition(DownloadState::Completed).is_err());
        assert_eq!(
            d.state,
            DownloadState::Probing,
            "a failed transition changes nothing"
        );
    }

    #[test]
    fn checksum_validates_length_and_alphabet() {
        assert!(Checksum::new(ChecksumAlgo::Md5, "d41d8cd98f00b204e9800998ecf8427e").is_some());
        assert!(
            Checksum::new(ChecksumAlgo::Md5, "D41D8CD98F00B204E9800998ECF8427E")
                .is_some_and(|c| c.hex == "d41d8cd98f00b204e9800998ecf8427e")
        );
        assert!(Checksum::new(ChecksumAlgo::Sha256, "abc").is_none());
        assert!(Checksum::new(ChecksumAlgo::Md5, "zz1d8cd98f00b204e9800998ecf8427e").is_none());
        assert_eq!(
            Checksum::guess(&"a".repeat(64)).map(|c| c.algo),
            Some(ChecksumAlgo::Sha256)
        );
        assert_eq!(
            Checksum::guess(&"a".repeat(40)).map(|c| c.algo),
            Some(ChecksumAlgo::Sha1)
        );
        assert_eq!(Checksum::guess("abc"), None);
        assert_eq!(
            ChecksumAlgo::from_name("SHA-256"),
            Some(ChecksumAlgo::Sha256)
        );
    }

    #[test]
    fn secrets_debug_never_prints_values() {
        let s = Secrets {
            cookies: Some("session=supersecret".into()),
            headers: vec![("X-Token".into(), "tok-123".into())],
            auth: Some(Credentials {
                username: "bob".into(),
                password: "hunter2".into(),
            }),
        };
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("supersecret"));
        assert!(!dbg.contains("tok-123"));
        assert!(!dbg.contains("hunter2"));
        assert!(dbg.contains("X-Token"), "header names are fine to show");
        assert!(dbg.contains("bob"));
        let c = format!("{:?}", s.auth.unwrap());
        assert!(c.contains("[REDACTED]") && !c.contains("hunter2"));
    }

    #[test]
    fn request_json_is_lenient_about_missing_fields() {
        let r: DownloadRequest = serde_json::from_str(r#"{"url":"https://a/b"}"#).unwrap();
        assert_eq!(r.url, "https://a/b");
        assert!(!r.start_paused);
        assert!(r.secrets.is_none());
    }

    #[test]
    fn download_serde_round_trip() {
        let mut d = Download::new(DownloadId(3), "https://a/b.iso", "b.iso", "/tmp", 1);
        d.state = DownloadState::Failed { retryable: false };
        d.checksum_expected = Checksum::guess(&"0".repeat(32));
        let json = serde_json::to_string(&d).unwrap();
        let back: Download = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }
}
