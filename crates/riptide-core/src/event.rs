//! Events the engine emits. The desktop app forwards them to the UI, the CLI prints them.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::download::{DownloadId, QueueId};
use crate::queue::PostAction;
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
    /// Extra live numbers of a torrent (peers, upload, seeding), sent next to its
    /// [`Event::Progress`] at the same coalesced rate.
    TorrentStats {
        /// Download.
        id: DownloadId,
        /// Peers connected right now.
        peers: u32,
        /// Peers seen so far (connected or not).
        peers_seen: u32,
        /// Upload speed in bytes per second.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        up_bps: u64,
        /// Bytes uploaded over the torrent's whole life (survives restarts).
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        uploaded: u64,
        /// Uploaded divided by the selected size, times 1000 (`1500` = ratio 1.5).
        ratio_milli: u32,
        /// Complete and still uploading by the user's choice.
        seeding: bool,
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
    /// A queue was created.
    QueueCreated {
        /// The new queue.
        queue_id: QueueId,
    },
    /// A queue's name, limit, schedule, post action or started/stopped switch changed.
    QueueUpdated {
        /// Queue.
        queue_id: QueueId,
    },
    /// A queue was deleted; its items went back to *Main*.
    QueueDeleted {
        /// The former queue.
        queue_id: QueueId,
    },
    /// The order of the queues themselves changed.
    QueuesReordered,
    /// The items of a queue were reordered.
    QueueOrderChanged {
        /// Queue.
        queue_id: QueueId,
    },
    /// A download moved to another queue.
    DownloadMoved {
        /// Download.
        id: DownloadId,
        /// Queue it left.
        from: QueueId,
        /// Queue it joined.
        to: QueueId,
    },
    /// The last unfinished item of a queue completed. The engine does not act on `action`;
    /// the app layer performs it (shutdown, sleep, … — platform code).
    QueueFinished {
        /// Queue that drained.
        queue_id: QueueId,
        /// The queue's post-queue action, if one is set.
        action: Option<PostAction>,
    },
}

impl Event {
    /// The download this event is about (`None` for queue events).
    pub const fn download_id(&self) -> Option<DownloadId> {
        match self {
            Event::Added { id }
            | Event::Probed { id, .. }
            | Event::StateChanged { id, .. }
            | Event::Progress { id, .. }
            | Event::TorrentStats { id, .. }
            | Event::SegmentProgress { id, .. }
            | Event::Completed { id, .. }
            | Event::Failed { id, .. }
            | Event::Removed { id }
            | Event::DownloadMoved { id, .. } => Some(*id),
            Event::QueueCreated { .. }
            | Event::QueueUpdated { .. }
            | Event::QueueDeleted { .. }
            | Event::QueuesReordered
            | Event::QueueOrderChanged { .. }
            | Event::QueueFinished { .. } => None,
        }
    }

    /// `true` for the high-frequency variants that the engine coalesces.
    pub const fn is_progress(&self) -> bool {
        matches!(
            self,
            Event::Progress { .. } | Event::TorrentStats { .. } | Event::SegmentProgress { .. }
        )
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
    fn queue_finished_json_carries_the_action() {
        let e = Event::QueueFinished {
            queue_id: QueueId(3),
            action: Some(PostAction::RunCommand {
                command: "done.cmd".into(),
            }),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "queue_finished");
        assert_eq!(json["queue_id"], 3);
        assert_eq!(json["action"]["action"], "run_command");
        assert_eq!(json["action"]["command"], "done.cmd");
        assert_eq!(serde_json::from_value::<Event>(json).unwrap(), e);
        let none = Event::QueueFinished {
            queue_id: QueueId(1),
            action: None,
        };
        assert_eq!(
            serde_json::to_value(&none).unwrap()["action"],
            serde_json::Value::Null
        );
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
        assert_eq!(p.download_id(), Some(DownloadId(2)));
        assert_eq!(Event::QueuesReordered.download_id(), None);
        assert!(p.is_progress());
        assert!(!Event::Removed { id: DownloadId(2) }.is_progress());
    }
}
