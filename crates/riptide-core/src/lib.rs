//! Pure domain model for Riptide: downloads, segments, the state machine and the segment planner.
//!
//! This crate knows nothing about IO, async runtimes, Tauri, SQLite or any platform API. Everything
//! in it is a plain value or a pure function so that the engine (`riptide-net`), the store
//! (`riptide-store`), the IPC layer and the UI can all share one vocabulary.
//!
//! * [`Download`] / [`Segment`] — the persisted model (see `ARCHITECTURE.md`).
//! * [`DownloadState`] — the state machine with its exhaustive transition table.
//! * [`segment::plan_segments`] / [`segment::split_segment`] — the segment planner.
//! * [`Event`] — what the engine emits to whoever is listening.
//! * [`Queue`] / [`Schedule`] — queues as data and the pure schedule logic (`active_at`,
//!   `next_transition`) the engine's timer uses.
//! * [`Category`] — file categories by extension, shared with the store seed and the UI.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

pub mod batch;
pub mod category;
pub mod dedup;
pub mod dispatch;
pub mod download;
pub mod error;
pub mod event;
pub mod filename;
pub mod media;
pub mod queue;
pub mod schedule;
pub mod segment;
pub mod state;
pub mod throttle;

pub use batch::{
    export_url_list, import_url_list, is_http_url, parse_url_list, BatchError, BatchOptions,
    UrlList, MAX_EXPANSION,
};
pub use category::Category;
pub use dispatch::pick_next;
pub use download::{
    CategoryId, Checksum, ChecksumAlgo, Credentials, Download, DownloadId, DownloadKind,
    DownloadRequest, Priority, QueueId, Secrets, UnixMillis, DEFAULT_CONNECTIONS, MAX_CONNECTIONS,
};
pub use error::Error;
pub use event::Event;
pub use filename::{sanitize_filename, split_extension, unique_path, MAX_FILENAME_BYTES};
pub use media::{
    is_media_host, media_choices, MediaChoice, MediaChoiceKind, MediaFormat, MediaInfo,
    MediaSelection,
};
pub use queue::{PostAction, Queue, QueueSpec, MAX_QUEUE_CONCURRENT, MAX_QUEUE_NAME_CHARS};
pub use schedule::{
    Clock, DaySet, LocalDateTime, ManualClock, Schedule, Weekday, Window, MINUTES_PER_DAY,
    SECS_PER_DAY,
};
pub use segment::{
    plan_segments, split_segment, ByteRange, Segment, SegmentState, MIN_SEGMENT_BYTES,
    MIN_SPLIT_REMAINDER_BYTES,
};
pub use state::DownloadState;
pub use throttle::Throttle;
