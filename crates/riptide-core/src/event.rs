//! Events the engine emits. The desktop app forwards them to the UI, the CLI prints them.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::download::DownloadId;
use crate::state::DownloadState;

/// Something happened to a download. Progress events are coalesced by the engine to at most ten
/// per second per download (see [`crate::Throttle`]); every other variant is emitted as it happens.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A download was created.
    Added {
        /// The new download.
        id: DownloadId,
    },
    /// The probe finished; size, validators and filename are known.
    Probed {
        /// Download.
        id: DownloadId,
        /// Total size if the server told us.
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        size: Option<u64>,
        /// Whether `Range` requests are honoured.
        supports_ranges: bool,
        /// Final sanitised filename.
        filename: String,
        /// `Content-Type` if present.
        content_type: Option<String>,
    },
    /// The state changed (always a legal transition).
    StateChanged {
        /// Download.
        id: DownloadId,
        /// Previous state.
        from: DownloadState,
        /// New state.
        to: DownloadState,
    },
    /// Periodic progress.
    Progress {
        /// Download.
        id: DownloadId,
        /// Bytes on disk.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        downloaded: u64,
        /// Total bytes if known.
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        total: Option<u64>,
        /// Smoothed speed in bytes per second.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        speed_bps: u64,
        /// Estimated seconds remaining, if computable.
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        eta_secs: Option<u64>,
        /// Active connections right now.
        connections: u32,
    },
    /// Per-segment progress (only consumed by the details window).
    SegmentProgress {
        /// Download.
        id: DownloadId,
        /// Segment index.
        index: u32,
        /// Bytes written from the segment start.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        written: u64,
        /// Segment length.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        len: u64,
    },
    /// The file is complete, verified and renamed to its final name.
    Completed {
        /// Download.
        id: DownloadId,
        /// Final path on disk.
        path: PathBuf,
    },
    /// The download failed.
    Failed {
        /// Download.
        id: DownloadId,
        /// Human-readable reason (already redacted).
        error: String,
        /// Whether an automatic retry is reasonable.
        retryable: bool,
    },
    /// The download was removed from the list.
    Removed {
        /// Download.
        id: DownloadId,
    },
}

impl Event {
    /// The download this event is about.
    pub const fn download_id(&self) -> DownloadId {
        match self {
            Event::Added { id }
            | Event::Probed { id, .. }
            | Event::StateChanged { id, .. }
            | Event::Progress { id, .. }
            | Event::SegmentProgress { id, .. }
            | Event::Completed { id, .. }
            | Event::Failed { id, .. }
            | Event::Removed { id } => *id,
        }
    }

    /// `true` for the high-frequency variants that the engine coalesces.
    pub const fn is_progress(&self) -> bool {
        matches!(self, Event::Progress { .. } | Event::SegmentProgress { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape_is_tagged_snake_case() {
        let e = Event::StateChanged {
            id: DownloadId(4),
            from: DownloadState::Queued,
            to: DownloadState::Probing,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "state_changed");
        assert_eq!(json["id"], 4);
        assert_eq!(json["from"]["state"], "queued");
        let back: Event = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn download_id_and_progress_flag() {
        let p = Event::Progress {
            id: DownloadId(2),
            downloaded: 1,
            total: None,
            speed_bps: 0,
            eta_secs: None,
            connections: 1,
        };
        assert_eq!(p.download_id(), DownloadId(2));
        assert!(p.is_progress());
        assert!(!Event::Removed { id: DownloadId(2) }.is_progress());
    }
}
