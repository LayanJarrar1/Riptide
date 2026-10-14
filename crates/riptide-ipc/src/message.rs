//! The message schema. Tagged JSON (`"type": "..."`), snake_case, lenient about unknown fields on
//! input so older clients keep working against newer apps.
//!
//! The top-level `id` is the request correlation id (JSON-RPC style); downloads are always
//! referred to as `download_id` so the two never collide after flattening.

use serde::{Deserialize, Serialize};

use riptide_core::{Download, DownloadId, DownloadRequest, Event, Secrets};

/// Bumped only for incompatible changes; reported in [`ResponseBody::Pong`].
pub const PROTOCOL_VERSION: u32 = 1;

/// Client → server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Caller-chosen correlation id, echoed in the response.
    pub id: u64,
    /// What is being asked.
    #[serde(flatten)]
    pub body: RequestBody,
}

impl Request {
    /// A request with the given id.
    pub const fn new(id: u64, body: RequestBody) -> Self {
        Request { id, body }
    }
}

/// Request payloads. The first four (`ping`, `probe`, `add`, `refresh`) are the only ones the
/// Native Messaging host forwards from the browser (hard rule 5); the rest are for the CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestBody {
    /// Liveness + version check.
    Ping,
    /// Ask the engine what a URL is (size, name, resumable) without adding it.
    Probe {
        /// URL to probe.
        url: String,
        /// Page the link was found on (sent as `Referer`).
        #[serde(default)]
        referrer: Option<String>,
        /// Browser user agent to mimic.
        #[serde(default)]
        user_agent: Option<String>,
        /// Cookies / headers / auth for that URL. Never logged.
        #[serde(default)]
        secrets: Option<Secrets>,
    },
    /// Add a download.
    Add {
        /// The download to create.
        request: DownloadRequest,
    },
    /// Give an existing download (expired link) a fresh URL and secrets.
    Refresh {
        /// Download to update.
        download_id: DownloadId,
        /// New URL.
        url: String,
        /// New referrer, if any.
        #[serde(default)]
        referrer: Option<String>,
        /// New secrets, if any.
        #[serde(default)]
        secrets: Option<Secrets>,
    },
    /// List every download.
    List,
    /// One download.
    Get {
        /// Which.
        download_id: DownloadId,
    },
    /// Pause.
    Pause {
        /// Which.
        download_id: DownloadId,
    },
    /// Resume / retry.
    Resume {
        /// Which.
        download_id: DownloadId,
    },
    /// Cancel (keeps the list entry).
    Cancel {
        /// Which.
        download_id: DownloadId,
    },
    /// Remove from the list.
    Remove {
        /// Which.
        download_id: DownloadId,
        /// Also delete the file on disk.
        #[serde(default)]
        delete_file: bool,
    },
    /// Start receiving [`ResponseBody::Event`] frames on this connection until it closes.
    Subscribe,
    /// Ask the app to shut down (CLI `riptide quit`).
    Shutdown,
}

/// Server → client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The request id this answers; `0` for unsolicited frames (events).
    pub id: u64,
    /// The answer.
    #[serde(flatten)]
    pub body: ResponseBody,
}

impl Response {
    /// A response for request `id`.
    pub const fn new(id: u64, body: ResponseBody) -> Self {
        Response { id, body }
    }

    /// An error response.
    pub fn error(id: u64, code: ErrorCode, message: impl Into<String>) -> Self {
        Response {
            id,
            body: ResponseBody::Error {
                code,
                message: message.into(),
            },
        }
    }
}

/// What a probe learned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// URL after redirects.
    pub final_url: String,
    /// Sanitised filename the download would get.
    pub filename: String,
    /// Size if known.
    pub size: Option<u64>,
    /// Whether the server honours `Range`.
    pub supports_ranges: bool,
    /// `Content-Type` if present.
    pub content_type: Option<String>,
}

/// Response payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseBody {
    /// Answer to `ping`.
    Pong {
        /// App version (`CARGO_PKG_VERSION` of the server).
        version: String,
        /// [`PROTOCOL_VERSION`] of the server.
        protocol: u32,
    },
    /// Answer to `probe`.
    Probed {
        /// Findings.
        result: ProbeResult,
    },
    /// Answer to `add`.
    Added {
        /// The new download.
        download_id: DownloadId,
    },
    /// Answer to `list`.
    Downloads {
        /// Every download.
        downloads: Vec<Download>,
    },
    /// Answer to `get`.
    Download {
        /// The download (boxed: it is far larger than the other variants).
        download: Box<Download>,
    },
    /// Generic success (`pause`, `resume`, `cancel`, `remove`, `refresh`, `subscribe`, `shutdown`).
    Ok,
    /// An engine event, sent after `subscribe`; `id` is `0`.
    Event {
        /// The event.
        event: Event,
    },
    /// The request failed.
    Error {
        /// Machine-readable class.
        code: ErrorCode,
        /// Human-readable detail (already redacted; safe to show).
        message: String,
    },
}

/// Error classes a client can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Malformed or unknown request.
    BadRequest,
    /// No such download.
    NotFound,
    /// The state machine forbids it (for example resuming a completed download).
    IllegalTransition,
    /// Not implemented in this build / milestone.
    Unsupported,
    /// Anything else; see `message`.
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_json_shape() {
        let r = Request::new(7, RequestBody::Ping);
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"id":7,"type":"ping"}"#
        );

        let add = Request::new(
            8,
            RequestBody::Add {
                request: DownloadRequest::new("https://a/b"),
            },
        );
        let json = serde_json::to_value(&add).unwrap();
        assert_eq!(json["type"], "add");
        assert_eq!(json["request"]["url"], "https://a/b");
        let back: Request = serde_json::from_value(json).unwrap();
        assert_eq!(back, add);

        let pause = Request::new(
            9,
            RequestBody::Pause {
                download_id: DownloadId(3),
            },
        );
        assert_eq!(
            serde_json::to_string(&pause).unwrap(),
            r#"{"id":9,"type":"pause","download_id":3}"#,
            "request id and download id never collide"
        );
    }

    #[test]
    fn unknown_fields_and_missing_optionals_are_tolerated() {
        let r: Request = serde_json::from_str(
            r#"{"id":1,"type":"probe","url":"https://x/y","future_field":true}"#,
        )
        .unwrap();
        assert!(
            matches!(r.body, RequestBody::Probe { ref url, referrer: None, .. } if url == "https://x/y")
        );
        let r: Request =
            serde_json::from_str(r#"{"id":2,"type":"remove","download_id":2}"#).unwrap();
        assert_eq!(
            r.body,
            RequestBody::Remove {
                download_id: DownloadId(2),
                delete_file: false
            }
        );
    }

    #[test]
    fn unknown_type_is_rejected() {
        let err =
            serde_json::from_str::<Request>(r#"{"id":1,"type":"exec","cmd":"calc"}"#).unwrap_err();
        assert!(err.to_string().contains("unknown variant"), "{err}");
    }

    #[test]
    fn response_json_shape() {
        let r = Response::error(3, ErrorCode::NotFound, "no download 9");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["id"], 3);
        assert_eq!(json["type"], "error");
        assert_eq!(json["code"], "not_found");

        let a = Response::new(
            8,
            ResponseBody::Added {
                download_id: DownloadId(5),
            },
        );
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"id":8,"type":"added","download_id":5}"#
        );

        let e = Response::new(
            0,
            ResponseBody::Event {
                event: Event::Removed { id: DownloadId(4) },
            },
        );
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "event");
        assert_eq!(json["event"]["type"], "removed");
        let back: Response = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }
}
