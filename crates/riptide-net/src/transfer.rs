//! The transfer: N connections, one preallocated file, work stealing, checkpoints, restart on
//! a changed file, verification, atomic rename.
//!
//! ```text
//!  segment tasks ──chunks──► writer (blocking thread) ──► PartFile.write_at
//!       ▲                         │ updates Segment.written (bytes really on disk)
//!       └── coordinator: spawns / splits / retries, ticks progress, syncs + checkpoints
//! ```
//!
//! The coordinator never persists anything itself: it emits [`TransferEvent::Checkpoint`] with
//! the segment table at least once per second and at every state change, and the owner of the
//! download (app or CLI) writes it to the store. `Segment.written` only ever counts bytes the
//! writer has handed to the OS, so a checkpoint never claims more than the file holds.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::{Client, Method};
use riptide_core::{plan_segments, split_segment, Checksum, DownloadId, Segment, SegmentState};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::context::RequestContext;
use crate::error::{Error, Result};
use crate::file::PartFile;
use crate::http::{ensure_success, send};
use crate::limiter::HostLimiter;
use crate::probe::{parse_content_range, parse_content_range_total, pick_validator};
use crate::rate::RateLimiter;
use crate::retry::RetryPolicy;
use crate::speed::SpeedMeter;
use crate::verify::verify_file;

/// Tunables for one transfer.
#[derive(Debug, Clone)]
pub struct TransferConfig {
    /// Connections for this download (1..=32).
    pub connections: u32,
    /// Smallest initial segment.
    pub min_segment: u64,
    /// A running segment is split for a free connection only if at least this much remains.
    pub min_split: u64,
    /// Retry schedule per segment.
    pub retry: RetryPolicy,
    /// How often `Progress` is emitted (≤ 10/s).
    pub progress_interval: Duration,
    /// How often the file is synced and a `Checkpoint` emitted.
    pub checkpoint_interval: Duration,
    /// Replace an existing final file instead of failing with `TargetExists`.
    pub overwrite: bool,
    /// Bandwidth buckets every connection draws from before a chunk reaches the writer (the
    /// download's own limit, the global one). Their rates may change while the transfer runs.
    pub rate_limits: Vec<Arc<RateLimiter>>,
    /// Test fault point: fail right after the atomic rename, before `Completed` is reported,
    /// the way a crash at that instant would leave things.
    #[doc(hidden)]
    pub crash_after_rename: bool,
}

impl Default for TransferConfig {
    fn default() -> Self {
        TransferConfig {
            connections: riptide_core::DEFAULT_CONNECTIONS,
            min_segment: riptide_core::MIN_SEGMENT_BYTES,
            min_split: riptide_core::MIN_SPLIT_REMAINDER_BYTES,
            retry: RetryPolicy::default(),
            progress_interval: Duration::from_millis(250),
            checkpoint_interval: Duration::from_secs(1),
            overwrite: false,
            rate_limits: Vec::new(),
            crash_after_rename: false,
        }
    }
}

/// What the owner needs to know while a transfer runs.
#[derive(Debug, Clone, PartialEq)]
pub enum TransferEvent {
    /// Connections are about to open.
    Started {
        /// Total size if known.
        size: Option<u64>,
        /// Segments in the plan.
        segments: usize,
        /// Bytes already on disk from a previous run.
        resumed_bytes: u64,
    },
    /// Periodic totals.
    Progress {
        /// Bytes on disk.
        downloaded: u64,
        /// Total if known.
        total: Option<u64>,
        /// Smoothed speed.
        speed_bps: u64,
        /// Seconds left if computable.
        eta_secs: Option<u64>,
        /// Segments with an open connection.
        connections: u32,
    },
    /// One segment's progress.
    SegmentProgress {
        /// Index.
        index: u32,
        /// Bytes on disk from its start.
        written: u64,
        /// Its length (grows for an unknown-size stream).
        len: u64,
    },
    /// The segment table to persist. Emitted at least once per second and on every change of
    /// state; the file has been synced before it is sent.
    Checkpoint {
        /// All segments.
        segments: Vec<Segment>,
    },
    /// A segment hit a transient error and will try again after `delay`.
    Retrying {
        /// Segment.
        index: u32,
        /// Attempt number that failed.
        attempt: u32,
        /// Wait before the next one.
        delay: Duration,
        /// What happened (already redacted).
        error: String,
    },
    /// The server's file changed (or it stopped honouring ranges); everything was discarded
    /// and the transfer begins again from zero.
    Restarted {
        /// Why.
        reason: String,
    },
    /// All bytes are on disk; the checksum is being computed.
    Verifying,
    /// Done: the file has its final name.
    Completed {
        /// Final path.
        path: PathBuf,
    },
}

/// Everything a transfer needs; the owner builds it from the probe and the store.
#[derive(Debug, Clone)]
pub struct TransferPlan {
    /// For event correlation only.
    pub download_id: DownloadId,
    /// Where to fetch from (the probe's final URL).
    pub url: Url,
    /// Total size, `None` for an unknown-length stream.
    pub size: Option<u64>,
    /// Whether ranged requests are honoured.
    pub supports_ranges: bool,
    /// `ETag` or `Last-Modified` for `If-Range`.
    pub validator: Option<String>,
    /// Existing segments to resume from; empty for a fresh start.
    pub segments: Vec<Segment>,
    /// The `.rtpart` path.
    pub part_path: PathBuf,
    /// Where the finished file goes.
    pub final_path: PathBuf,
    /// Expected digest, checked before the rename.
    pub checksum: Option<Checksum>,
}

/// How a transfer ended without an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Verified and renamed.
    Completed {
        /// Final path.
        path: PathBuf,
        /// Final segment table (all `done`).
        segments: Vec<Segment>,
    },
    /// Stopped by the caller; the part file and `segments` allow a resume.
    Stopped {
        /// Segment table at the moment of stopping.
        segments: Vec<Segment>,
    },
}

/// Runs a transfer to completion, a stop, or an error. On `Err` the part file and the last
/// `Checkpoint` are still valid for a later resume (except for non-retryable errors that
/// require a restart, which the owner decides).
pub async fn transfer(
    client: Client,
    ctx: Arc<RequestContext>,
    plan: TransferPlan,
    cfg: TransferConfig,
    events: mpsc::Sender<TransferEvent>,
    stop: CancellationToken,
    limiter: Arc<HostLimiter>,
) -> Result<Outcome> {
    let mut plan = plan;
    if let Some(outcome) = adopt_finished_file(&plan, &events).await? {
        return Ok(outcome);
    }
    let mut restarts = 0u32;
    loop {
        match run_once(&client, &ctx, &plan, &cfg, &events, &stop, &limiter).await? {
            Run::Finished(outcome) => return Ok(outcome),
            Run::Restart {
                validator,
                size,
                reason,
                no_ranges,
            } => {
                restarts += 1;
                tracing::warn!(restarts, %reason, "restarting transfer from zero");
                if restarts > 2 {
                    return Err(Error::ValidatorChanged);
                }
                // the stored table must stop describing the old part file and before anyone is told:
                // events are ordered, so this reset is saved ahead of anything the next run says
                let _ = events
                    .send(TransferEvent::Checkpoint {
                        segments: Vec::new(),
                    })
                    .await;
                let _ = events.send(TransferEvent::Restarted { reason }).await;
                if let Some(v) = validator {
                    plan.validator = Some(v);
                }
                if let Some(s) = size {
                    plan.size = Some(s);
                }
                if no_ranges {
                    // the server's range support is broken (it sends more than it announced):
                    // one plain streaming connection, and its announced total is not trusted
                    plan.supports_ranges = false;
                    plan.size = None;
                }
                // second restart: the server keeps answering 200, so it does not really do ranges
                if restarts == 2 {
                    plan.supports_ranges = false;
                }
                plan.segments.clear();
                let _ = std::fs::remove_file(&plan.part_path);
            }
        }
    }
}

enum Run {
    Finished(Outcome),
    Restart {
        validator: Option<String>,
        size: Option<u64>,
        reason: String,
        no_ranges: bool,
    },
}

/// Shared between coordinator, segment tasks and the writer.
struct Shared {
    segments: Vec<Segment>,
}

type State = Arc<Mutex<Shared>>;

/// A piece of body on its way to disk.
struct Chunk {
    index: usize,
    offset: u64,
    data: Bytes,
}

/// How a segment task ended.
enum SegEnd {
    Done,
    Stopped,
    /// `200` to a ranged request / `416`: the file changed or ranges stopped working.
    Restart {
        validator: Option<String>,
        size: Option<u64>,
        reason: String,
        /// Ranges are not trustworthy on this server: continue without them.
        no_ranges: bool,
    },
}

fn lock(state: &State) -> std::sync::MutexGuard<'_, Shared> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

async fn run_once(
    client: &Client,
    ctx: &Arc<RequestContext>,
    plan: &TransferPlan,
    cfg: &TransferConfig,
    events: &mpsc::Sender<TransferEvent>,
    stop: &CancellationToken,
    limiter: &Arc<HostLimiter>,
) -> Result<Run> {
    let streaming = plan.size.is_none() || !plan.supports_ranges;
    // without ETag and Last-Modified nothing would notice the file changing between two
    // connections' requests, so such a download uses one connection
    let connections = if streaming || plan.validator.is_none() {
        1
    } else {
        cfg.connections.clamp(1, riptide_core::MAX_CONNECTIONS)
    };

    // --- segment table: resume what is there, or plan afresh --------------------------------
    let mut segments: Vec<Segment> = plan
        .segments
        .iter()
        .map(|s| Segment {
            state: if s.is_complete() {
                SegmentState::Done
            } else {
                SegmentState::Pending
            },
            ..s.clone()
        })
        .collect();
    // the part file must still be there with exactly the preallocated length: a deleted,
    // truncated or extended file is not what the table describes
    let part_ok = |size: u64| std::fs::metadata(&plan.part_path).is_ok_and(|m| m.len() == size);
    let can_resume = plan.supports_ranges
        && plan.validator.is_some()
        && plan
            .size
            .is_some_and(|size| covers(&segments, size) && part_ok(size));
    if !can_resume {
        segments.clear();
    }
    if segments.is_empty() {
        let size = plan.size.unwrap_or(0);
        segments = if streaming {
            vec![Segment::pending(
                plan.download_id,
                0,
                riptide_core::ByteRange::new(0, size),
            )]
        } else {
            plan_segments(size, connections, cfg.min_segment)
                .into_iter()
                .enumerate()
                .map(|(i, r)| Segment::pending(plan.download_id, i as u32, r))
                .collect()
        };
    }
    // a segment that already holds all its bytes (or an empty file's single segment) is done
    for s in &mut segments {
        if s.is_complete() && !streaming {
            s.written = s.len();
            s.state = SegmentState::Done;
        }
    }
    let resumed_bytes: u64 = segments.iter().map(|s| s.written).sum();

    // --- file --------------------------------------------------------------------------------
    let file = Arc::new(PartFile::open(&plan.part_path, plan.size)?);
    if !can_resume && !file.is_empty()? {
        file.truncate(0)?;
        if let Some(size) = plan.size {
            file.truncate(size)?;
        }
    }

    let segment_count = segments.len();
    let state: State = Arc::new(Mutex::new(Shared { segments }));
    let _ = events
        .send(TransferEvent::Started {
            size: plan.size,
            segments: segment_count,
            resumed_bytes,
        })
        .await;

    // the plan itself is worth persisting before the first byte: a crash in the first second
    // then resumes against the same segment table instead of re-planning
    checkpoint(&file, &state, events, streaming).await?;

    // --- writer ------------------------------------------------------------------------------
    let (tx, rx) = mpsc::channel::<Chunk>(64);
    let writer = spawn_writer(
        Arc::clone(&file),
        Arc::clone(&state),
        rx,
        streaming && plan.size.is_none(),
    );

    // --- segment tasks ------------------------------------------------------------------------
    let run_token = stop.child_token();
    let mut tasks: JoinSet<(usize, Result<SegEnd>)> = JoinSet::new();
    let mut spawn = |index: usize, tasks: &mut JoinSet<(usize, Result<SegEnd>)>| {
        {
            let mut s = lock(&state);
            s.segments[index].state = SegmentState::Active;
        }
        let task = SegmentTask {
            index,
            client: client.clone(),
            ctx: Arc::clone(ctx),
            url: plan.url.clone(),
            validator: plan.validator.clone(),
            streaming,
            declared: plan.size,
            state: Arc::clone(&state),
            tx: tx.clone(),
            policy: cfg.retry.clone(),
            limiter: Arc::clone(limiter),
            rate_limits: cfg.rate_limits.clone(),
            stop: run_token.clone(),
            events: events.clone(),
        };
        tasks.spawn(async move {
            let r = task.run().await;
            (index, r)
        });
    };

    fill_connections(
        &state,
        connections,
        cfg.min_split,
        &mut spawn,
        &mut tasks,
        plan.download_id,
        streaming,
    );

    // --- coordinator loop ------------------------------------------------------------------------
    let mut speed = SpeedMeter::for_ui(Instant::now(), resumed_bytes);
    let mut progress_tick = tokio::time::interval(cfg.progress_interval);
    progress_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_checkpoint = Instant::now();
    let mut failure: Option<Error> = None;
    let mut restart: Option<(Option<String>, Option<u64>, String, bool)> = None;
    let mut stopped = false;

    loop {
        tokio::select! {
            biased;
            () = stop.cancelled(), if !stopped => {
                stopped = true;
                run_token.cancel();
            }
            Some(joined) = tasks.join_next() => {
                let (index, result) = match joined {
                    Ok(r) => r,
                    Err(e) => (usize::MAX, Err(Error::Protocol(format!("segment task panicked: {e}")))),
                };
                match result {
                    Ok(SegEnd::Done) => {
                        // only the state flips here; `written` stays the writer's figure so a
                        // checkpoint never claims bytes still queued for the disk
                        if index != usize::MAX {
                            lock(&state).segments[index].state = SegmentState::Done;
                        }
                        if failure.is_none() && restart.is_none() && !stopped {
                            fill_connections(&state, connections, cfg.min_split, &mut spawn, &mut tasks, plan.download_id, streaming);
                        }
                    }
                    Ok(SegEnd::Stopped) => {
                        if index != usize::MAX {
                            lock(&state).segments[index].state = SegmentState::Pending;
                        }
                    }
                    Ok(SegEnd::Restart { validator, size, reason, no_ranges }) => {
                        if restart.is_none() {
                            restart = Some((validator, size, reason, no_ranges));
                            run_token.cancel();
                        }
                    }
                    Err(e) => {
                        if index != usize::MAX {
                            lock(&state).segments[index].state = SegmentState::Failed;
                        }
                        if failure.is_none() {
                            tracing::warn!(index, error = %e, "segment failed permanently");
                            failure = Some(e);
                            run_token.cancel();
                        }
                    }
                }
                if tasks.is_empty() {
                    break;
                }
            }
            _ = progress_tick.tick() => {
                let (downloaded, active, snapshot) = {
                    let s = lock(&state);
                    let downloaded = s.segments.iter().map(|g| g.written).sum::<u64>();
                    let active = s.segments.iter().filter(|g| g.state == SegmentState::Active).count() as u32;
                    (downloaded, active, s.segments.clone())
                };
                let total = total_of(plan.size, &snapshot);
                let bps = speed.sample(downloaded, Instant::now());
                let eta = total.and_then(|t| speed.eta(t.saturating_sub(downloaded)));
                let _ = events.try_send(TransferEvent::Progress { downloaded, total, speed_bps: bps, eta_secs: eta, connections: active });
                for g in snapshot.iter().filter(|g| g.state == SegmentState::Active) {
                    let _ = events.try_send(TransferEvent::SegmentProgress { index: g.index, written: g.written, len: g.len() });
                }
                if last_checkpoint.elapsed() >= cfg.checkpoint_interval {
                    last_checkpoint = Instant::now();
                    checkpoint(&file, &state, events, streaming).await?;
                }
            }
        }
        if tasks.is_empty() {
            break;
        }
    }

    // --- drain the writer, then decide ------------------------------------------------------
    drop(tx);
    writer
        .await
        .map_err(|e| Error::Protocol(format!("writer task failed: {e}")))??;
    checkpoint(&file, &state, events, streaming).await?;

    if let Some((validator, size, reason, no_ranges)) = restart {
        return Ok(Run::Restart {
            validator,
            size,
            reason,
            no_ranges,
        });
    }
    if let Some(e) = failure {
        return Err(e);
    }
    let segments = lock(&state).segments.clone();
    let complete = segments
        .iter()
        .all(|s| s.state == SegmentState::Done && (streaming || s.is_complete()));
    if stopped || !complete {
        return Ok(Run::Finished(Outcome::Stopped {
            segments: persistable(streaming, segments),
        }));
    }

    // --- verify + rename ----------------------------------------------------------------------
    if streaming {
        // every byte is in: only now may the table say so (earlier checkpoints of a stream
        // never do), so a crash right after the rename can recognise its own finished file
        let _ = events
            .send(TransferEvent::Checkpoint {
                segments: segments.clone(),
            })
            .await;
    }
    let _ = events.send(TransferEvent::Verifying).await;
    if let Some(expected) = &plan.checksum {
        verify_file(plan.part_path.clone(), expected).await?;
    }
    let file =
        Arc::try_unwrap(file).map_err(|_| Error::Protocol("part file still shared".into()))?;
    let final_path = plan.final_path.clone();
    let overwrite = cfg.overwrite;
    tokio::task::spawn_blocking(move || file.finish(&final_path, overwrite))
        .await
        .map_err(|e| Error::Protocol(format!("rename task failed: {e}")))?
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::TargetExists(plan.final_path.clone())
            } else {
                Error::Io(e)
            }
        })?;
    if cfg.crash_after_rename {
        return Err(Error::Protocol("injected crash after the rename".into()));
    }
    let _ = events
        .send(TransferEvent::Completed {
            path: plan.final_path.clone(),
        })
        .await;
    Ok(Run::Finished(Outcome::Completed {
        path: plan.final_path.clone(),
        segments,
    }))
}

/// Crash recovery for the instant between the atomic rename and the record becoming
/// `Completed`: the part file is gone, the final file is there, and the stored table says every
/// byte was written. The file is adopted only if its length is exactly what the table (and the
/// probe) say and, when a checksum is expected, its content matches. Anything else is left
/// alone for the normal path, which never overwrites a file it did not write.
async fn adopt_finished_file(
    plan: &TransferPlan,
    events: &mpsc::Sender<TransferEvent>,
) -> Result<Option<Outcome>> {
    if plan.segments.is_empty() || plan.part_path.exists() {
        return Ok(None);
    }
    let Ok(meta) = std::fs::metadata(&plan.final_path) else {
        return Ok(None);
    };
    let total: u64 = plan.segments.iter().map(|s| s.written).sum();
    let table_done =
        covers(&plan.segments, total) && plan.segments.iter().all(Segment::is_complete);
    if !meta.is_file()
        || !table_done
        || meta.len() != total
        || plan.size.is_some_and(|s| s != total)
    {
        return Ok(None);
    }
    let _ = events.send(TransferEvent::Verifying).await;
    if let Some(expected) = &plan.checksum {
        verify_file(plan.final_path.clone(), expected).await?;
    }
    let _ = events
        .send(TransferEvent::Completed {
            path: plan.final_path.clone(),
        })
        .await;
    Ok(Some(Outcome::Completed {
        path: plan.final_path.clone(),
        segments: plan.segments.clone(),
    }))
}

/// The server sent more bytes than its `Content-Range` announced: ranges cannot be trusted.
fn overrun_restart() -> SegEnd {
    SegEnd::Restart {
        validator: None,
        size: None,
        reason:
            "server sent more bytes than its Content-Range announced; downloading without ranges"
                .into(),
        no_ranges: true,
    }
}

/// A segment table is resumable only if it proves itself: contiguous from 0 to exactly `size`,
/// with no segment claiming more bytes than it spans. Anything else restarts from zero.
fn covers(segments: &[Segment], size: u64) -> bool {
    !segments.is_empty()
        && segments[0].start == 0
        && segments.windows(2).all(|w| w[0].end == w[1].start)
        && segments.last().map(|s| s.end) == Some(size)
        && segments.iter().all(|s| s.written <= s.len())
}

/// What may be persisted: a streaming transfer cannot resume (it restarts at byte 0), so its
/// progress is never recorded; otherwise a stored "complete" streaming segment could later be
/// mistaken for a finished ranged one.
fn persistable(streaming: bool, segments: Vec<Segment>) -> Vec<Segment> {
    if !streaming {
        return segments;
    }
    segments
        .into_iter()
        .map(|mut g| {
            g.written = 0;
            g.end = 0;
            g.state = SegmentState::Pending;
            g
        })
        .collect()
}

/// Total size for progress: only the declared size counts; an unknown-length stream reports
/// `None` so the UI shows an indeterminate bar rather than a moving target.
fn total_of(size: Option<u64>, _segments: &[Segment]) -> Option<u64> {
    size
}

/// Starts pending segments until `connections` are busy; when none are pending, steals work by
/// splitting the segment with the most bytes left.
fn fill_connections(
    state: &State,
    connections: u32,
    min_split: u64,
    spawn: &mut impl FnMut(usize, &mut JoinSet<(usize, Result<SegEnd>)>),
    tasks: &mut JoinSet<(usize, Result<SegEnd>)>,
    download_id: DownloadId,
    streaming: bool,
) {
    loop {
        let next = {
            let mut s = lock(state);
            let active = s
                .segments
                .iter()
                .filter(|g| g.state == SegmentState::Active)
                .count() as u32;
            if active >= connections {
                None
            } else if let Some(i) = s.segments.iter().position(|g| {
                matches!(g.state, SegmentState::Pending | SegmentState::Failed)
                    && (streaming || !g.is_complete())
            }) {
                Some(i)
            } else {
                // work stealing
                let candidate = s
                    .segments
                    .iter()
                    .filter(|g| g.state == SegmentState::Active && g.remaining() >= min_split)
                    .max_by_key(|g| g.remaining())
                    .map(|g| g.index as usize);
                candidate.and_then(|i| {
                    let new_index = s.segments.len() as u32;
                    let (lower, upper) = split_segment(&s.segments[i], new_index, min_split)?;
                    debug_assert_eq!(upper.download_id, download_id);
                    s.segments[i].end = lower.end;
                    s.segments.push(upper);
                    Some(s.segments.len() - 1)
                })
            }
        };
        match next {
            Some(i) => spawn(i, tasks),
            None => break,
        }
    }
}

/// Syncs the file and emits the segment table.
async fn checkpoint(
    file: &Arc<PartFile>,
    state: &State,
    events: &mpsc::Sender<TransferEvent>,
    streaming: bool,
) -> Result<()> {
    let f = Arc::clone(file);
    checkpoint_with(move || f.sync(), state, events, streaming).await
}

/// [`checkpoint`] with the sync as a parameter. The segment table is cloned BEFORE the sync
/// starts: the writer keeps raising `written` while `sync_data` runs, and those bytes are not
/// durable yet. Emitting a table cloned afterwards could claim bytes a power cut would lose,
/// leaving a silent hole on resume. The earlier snapshot is what gets emitted.
async fn checkpoint_with(
    sync: impl FnOnce() -> std::io::Result<()> + Send + 'static,
    state: &State,
    events: &mpsc::Sender<TransferEvent>,
    streaming: bool,
) -> Result<()> {
    let segments = persistable(streaming, lock(state).segments.clone());
    tokio::task::spawn_blocking(sync)
        .await
        .map_err(|e| Error::Protocol(format!("sync task failed: {e}")))??;
    let _ = events.send(TransferEvent::Checkpoint { segments }).await;
    Ok(())
}

/// Accounts `len` bytes written at `start` to `seg`. Only an unknown-size stream (`grow`) may
/// extend the segment with the data; a ranged segment keeps its planned end, because work
/// stealing may have shrunk it while chunks beyond the new end were still queued, and growing it
/// back would overlap the new upper segment (the table would stop covering the file and bytes
/// would count twice).
fn record_write(seg: &mut Segment, start: u64, len: u64, grow: bool) {
    let end = start + len;
    if grow && end > seg.end {
        seg.end = end;
    }
    let written = end.saturating_sub(seg.start);
    if written > seg.written {
        seg.written = written.min(seg.len());
    }
}

/// Chunks are coalesced per segment up to this size before one positioned write.
const WRITE_COALESCE_BYTES: usize = 1024 * 1024;

/// The blocking writer: the only thing that touches the file during the transfer.
///
/// Network chunks are small (tens of KiB) and, with many connections, arrive interleaved; one
/// syscall per chunk would make this thread the bottleneck. Contiguous chunks of the same
/// segment are therefore accumulated and written in ~1 MiB pieces — immediately when the
/// channel runs dry, so an idle stream never waits on a half-full buffer. `Segment.written`
/// is raised only after the bytes have been handed to the OS.
fn spawn_writer(
    file: Arc<PartFile>,
    state: State,
    mut rx: mpsc::Receiver<Chunk>,
    grow: bool,
) -> tokio::task::JoinHandle<Result<()>> {
    tokio::task::spawn_blocking(move || {
        let mut pending: std::collections::HashMap<usize, (u64, Vec<u8>)> =
            std::collections::HashMap::new();
        let flush =
            |index: usize, start: u64, buf: &[u8], file: &PartFile, state: &State| -> Result<()> {
                if buf.is_empty() {
                    return Ok(());
                }
                file.write_at(start, buf)?;
                let mut s = lock(state);
                record_write(&mut s.segments[index], start, buf.len() as u64, grow);
                Ok(())
            };

        loop {
            // take one chunk (blocking), then whatever else is already queued
            let first = match rx.blocking_recv() {
                Some(c) => c,
                None => break,
            };
            let mut batch = vec![first];
            while let Ok(c) = rx.try_recv() {
                batch.push(c);
                if batch.len() >= 256 {
                    break;
                }
            }
            for chunk in batch {
                let entry = pending
                    .entry(chunk.index)
                    .or_insert_with(|| (chunk.offset, Vec::new()));
                let contiguous = entry.0 + entry.1.len() as u64 == chunk.offset;
                if !contiguous {
                    let (start, buf) = std::mem::replace(entry, (chunk.offset, Vec::new()));
                    flush(chunk.index, start, &buf, &file, &state)?;
                }
                entry.1.extend_from_slice(&chunk.data);
                if entry.1.len() >= WRITE_COALESCE_BYTES {
                    let (start, buf) = std::mem::replace(
                        entry,
                        (chunk.offset + chunk.data.len() as u64, Vec::new()),
                    );
                    flush(chunk.index, start, &buf, &file, &state)?;
                }
            }
            // the channel is empty now: write everything out so progress and checkpoints see it
            if rx.is_empty() {
                for (index, (start, buf)) in pending.drain() {
                    flush(index, start, &buf, &file, &state)?;
                }
            }
        }
        for (index, (start, buf)) in pending.drain() {
            flush(index, start, &buf, &file, &state)?;
        }
        Ok(())
    })
}

/// One connection's worth of work.
struct SegmentTask {
    index: usize,
    client: Client,
    ctx: Arc<RequestContext>,
    url: Url,
    validator: Option<String>,
    streaming: bool,
    declared: Option<u64>,
    state: State,
    tx: mpsc::Sender<Chunk>,
    policy: RetryPolicy,
    limiter: Arc<HostLimiter>,
    rate_limits: Vec<Arc<RateLimiter>>,
    stop: CancellationToken,
    events: mpsc::Sender<TransferEvent>,
}

impl SegmentTask {
    async fn run(self) -> Result<SegEnd> {
        let mut attempt = 0u32;
        loop {
            let (start, written, end) = {
                let s = lock(&self.state);
                let g = &s.segments[self.index];
                (g.start, g.written, g.end)
            };
            if !self.streaming && start + written >= end {
                return Ok(SegEnd::Done);
            }
            if self.stop.is_cancelled() {
                return Ok(SegEnd::Stopped);
            }
            let host = self.url.host_str().unwrap_or("").to_string();
            let permit = self.limiter.acquire(&host).await;
            let before: u64 = lock(&self.state).segments[self.index].written;
            let result = if self.streaming {
                self.stream_once().await
            } else {
                self.ranged_once(start + written, end).await
            };
            drop(permit);
            match result {
                Ok(end) => return Ok(end),
                Err(e) => {
                    if matches!(
                        e,
                        Error::Status {
                            status: 429 | 503,
                            ..
                        }
                    ) {
                        if let Some(cap) = self.limiter.report_throttled(&host) {
                            tracing::info!(host = %host, cap, "server is throttling: fewer connections");
                        }
                    }
                    let progressed = lock(&self.state).segments[self.index].written > before;
                    if progressed {
                        attempt = 0;
                    }
                    attempt += 1;
                    if self.stop.is_cancelled() {
                        return Ok(SegEnd::Stopped);
                    }
                    if !e.is_retryable() || !self.policy.allows(attempt) {
                        return Err(if attempt > 1 {
                            Error::RetriesExhausted {
                                attempts: attempt,
                                last: Box::new(e),
                            }
                        } else {
                            e
                        });
                    }
                    let delay = self.policy.delay(attempt, e.retry_after_secs());
                    tracing::debug!(index = self.index, attempt, ?delay, error = %e, "segment retry");
                    let _ = self.events.try_send(TransferEvent::Retrying {
                        index: self.index as u32,
                        attempt,
                        delay,
                        error: e.to_string(),
                    });
                    tokio::select! {
                        () = self.stop.cancelled() => return Ok(SegEnd::Stopped),
                        () = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    /// `Range: bytes=offset-(end-1)` with `If-Range`; expects `206`.
    async fn ranged_once(&self, offset: u64, end: u64) -> Result<SegEnd> {
        let range = format!("bytes={}-{}", offset, end - 1);
        let validator = self.validator.clone();
        let sent = send(
            &self.client,
            &self.ctx,
            Method::GET,
            self.url.clone(),
            move |r| {
                let r = r.header(RANGE, range.clone());
                match &validator {
                    Some(v) => r.header(IF_RANGE, v.clone()),
                    None => r,
                }
            },
        )
        .await?;
        let status = sent.response.status().as_u16();
        let headers = sent.response.headers().clone();
        match status {
            206 => {
                let cr = headers
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range);
                match cr {
                    Some((s, _, total)) => {
                        if s != offset {
                            return Err(Error::Protocol(format!(
                                "Content-Range starts at {s}, asked for {offset}"
                            )));
                        }
                        if let (Some(t), Some(d)) = (total, self.declared) {
                            if t != d {
                                return Err(Error::LengthMismatch {
                                    declared: d,
                                    received: t,
                                });
                            }
                        }
                    }
                    None => {
                        return Err(Error::Protocol("206 without a usable Content-Range".into()))
                    }
                }
                self.pump(sent.response, offset, Some(end))
                    .await
                    .map(|(end, _)| end)
            }
            200 => Ok(SegEnd::Restart {
                validator: pick_validator(
                    headers.get(ETAG).and_then(|v| v.to_str().ok()),
                    headers.get(LAST_MODIFIED).and_then(|v| v.to_str().ok()),
                )
                .map(str::to_owned),
                size: headers
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok()),
                reason:
                    "server answered 200 to a ranged request (file changed or ranges unsupported)"
                        .into(),
                no_ranges: false,
            }),
            416 => {
                let actual = headers
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range_total);
                match (actual, self.declared) {
                    (Some(a), Some(d)) if a != d => Err(Error::LengthMismatch {
                        declared: d,
                        received: a,
                    }),
                    _ => Ok(SegEnd::Restart {
                        validator: None,
                        size: actual,
                        reason: "416 for a range inside the known size".into(),
                        no_ranges: false,
                    }),
                }
            }
            _ => {
                ensure_success(&sent.response)?;
                Err(Error::Protocol(format!(
                    "unexpected {status} to a ranged request"
                )))
            }
        }
    }

    /// Plain `GET` for servers without ranges or without a known length; always from zero.
    async fn stream_once(&self) -> Result<SegEnd> {
        {
            let mut s = lock(&self.state);
            let g = &mut s.segments[self.index];
            g.written = 0;
            if self.declared.is_none() {
                g.end = 0;
            }
        }
        let sent = send(
            &self.client,
            &self.ctx,
            Method::GET,
            self.url.clone(),
            |r| r,
        )
        .await?;
        ensure_success(&sent.response)?;
        let declared = sent
            .response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .or(self.declared);
        let (end, received) = self.pump(sent.response, 0, None).await?;
        if let SegEnd::Done = end {
            // `received` counts bytes handed to the writer (exact); the writer's own accounting
            // may lag by a few chunks, so it is not consulted here
            if let Some(d) = declared {
                if received != d {
                    return Err(Error::LengthMismatch {
                        declared: d,
                        received,
                    });
                }
            }
        }
        Ok(end)
    }

    /// Streams the body to the writer from `offset`, honouring a shrinking `end` and the stop.
    /// Returns how the segment ended and the offset reached (bytes sent to the writer).
    ///
    /// `request_end` is the end of the range this response was asked for (`None` when
    /// streaming). Reaching it without the segment having been shrunk by work stealing, the body
    /// must be over: a server that keeps sending is not honouring its own `Content-Range`
    /// (CDNs that compress on the fly do this), so ranges cannot be trusted on it.
    async fn pump(
        &self,
        response: reqwest::Response,
        mut pos: u64,
        request_end: Option<u64>,
    ) -> Result<(SegEnd, u64)> {
        let mut body = response.bytes_stream();
        loop {
            let next = tokio::select! {
                biased;
                () = self.stop.cancelled() => return Ok((SegEnd::Stopped, pos)),
                n = body.next() => n,
            };
            let Some(chunk) = next else { break };
            let chunk: Bytes = chunk?;
            let cur_end = if self.streaming {
                u64::MAX
            } else {
                lock(&self.state).segments[self.index].end
            };
            if pos >= cur_end {
                return self.range_done(&mut body, pos, cur_end, request_end).await;
            }
            let allowed = (cur_end - pos).min(chunk.len() as u64) as usize;
            if allowed < chunk.len() && request_end == Some(cur_end) {
                // the excess sits inside this very chunk: same lie, same answer
                return Ok((overrun_restart(), pos));
            }
            let data = if allowed < chunk.len() {
                chunk.slice(..allowed)
            } else {
                chunk
            };
            let len = data.len() as u64;
            for bucket in &self.rate_limits {
                tokio::select! {
                    biased;
                    () = self.stop.cancelled() => return Ok((SegEnd::Stopped, pos)),
                    () = bucket.acquire(len) => {}
                }
            }
            if self
                .tx
                .send(Chunk {
                    index: self.index,
                    offset: pos,
                    data,
                })
                .await
                .is_err()
            {
                return Ok((SegEnd::Stopped, pos));
            }
            pos += len;
            if pos >= cur_end {
                return self.range_done(&mut body, pos, cur_end, request_end).await;
            }
        }
        if self.streaming {
            return Ok((SegEnd::Done, pos));
        }
        let end = lock(&self.state).segments[self.index].end;
        if pos < end {
            // the body ended early: a dropped connection, worth a retry from `pos`
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("body ended at {pos}, expected {end}"),
            )));
        }
        Ok((SegEnd::Done, pos))
    }

    /// The segment's bytes are all here. If it was not shrunk by work stealing, peek at the body
    /// once: more data means the server ignores the range it announced.
    async fn range_done<S>(
        &self,
        body: &mut S,
        pos: u64,
        cur_end: u64,
        request_end: Option<u64>,
    ) -> Result<(SegEnd, u64)>
    where
        S: futures::Stream<Item = reqwest::Result<Bytes>> + Unpin,
    {
        if request_end == Some(cur_end) {
            let peek = tokio::time::timeout(std::time::Duration::from_secs(1), body.next()).await;
            if let Ok(Some(Ok(extra))) = peek {
                if !extra.is_empty() {
                    return Ok((overrun_restart(), pos));
                }
            }
        }
        Ok((SegEnd::Done, pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riptide_core::ByteRange;

    fn seg(i: u32, a: u64, b: u64) -> Segment {
        Segment::pending(DownloadId(1), i, ByteRange::new(a, b))
    }

    #[test]
    fn only_a_table_covering_exactly_the_size_is_resumable() {
        assert!(covers(&[seg(0, 0, 10)], 10));
        assert!(covers(&[seg(0, 0, 4), seg(1, 4, 10)], 10));
        assert!(!covers(&[], 10));
        assert!(!covers(&[seg(0, 0, 9)], 10), "short table");
        assert!(!covers(&[seg(0, 0, 11)], 10), "long table");
        assert!(!covers(&[seg(0, 1, 10)], 10), "does not start at 0");
        assert!(!covers(&[seg(0, 0, 4), seg(1, 5, 10)], 10), "gap");
        let mut over = seg(0, 0, 10);
        over.written = 11;
        assert!(!covers(&[over], 10), "claims more than it spans");
    }

    #[test]
    fn streaming_progress_is_never_persisted() {
        let mut g = seg(0, 0, 500);
        g.written = 500;
        g.state = SegmentState::Done;
        let kept = persistable(true, vec![g.clone()]);
        assert_eq!((kept[0].written, kept[0].end), (0, 0));
        assert_eq!(persistable(false, vec![g.clone()])[0].written, 500);
    }

    fn state_of(segments: Vec<Segment>) -> State {
        Arc::new(Mutex::new(Shared { segments }))
    }

    #[tokio::test]
    async fn checkpoint_never_claims_bytes_written_during_the_sync() {
        let mut g = seg(0, 0, 100);
        g.written = 10;
        let state = state_of(vec![g]);
        let (tx, mut rx) = mpsc::channel(4);
        let during = Arc::clone(&state);
        // the writer advances `written` while sync_data is running
        let sync = move || {
            lock(&during).segments[0].written = 90;
            Ok(())
        };
        checkpoint_with(sync, &state, &tx, false).await.unwrap();
        match rx.recv().await.unwrap() {
            TransferEvent::Checkpoint { segments } => {
                assert_eq!(segments[0].written, 10, "snapshot is from before the sync");
            }
            other => panic!("unexpected {other:?}"),
        }
        // the live table did advance, and the next checkpoint will carry it
        assert_eq!(lock(&state).segments[0].written, 90);
    }

    #[tokio::test]
    async fn a_failed_sync_emits_no_checkpoint() {
        let state = state_of(vec![seg(0, 0, 10)]);
        let (tx, mut rx) = mpsc::channel(4);
        let sync = || Err(std::io::Error::other("disk gone"));
        assert!(checkpoint_with(sync, &state, &tx, false).await.is_err());
        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[test]
    fn a_split_segment_does_not_grow_back_over_its_neighbour() {
        // a 8 MiB ranged segment, one connection busy, one free: work stealing splits it
        let mib = 1024 * 1024;
        let state = state_of(vec![{
            let mut g = seg(0, 0, 8 * mib);
            g.state = SegmentState::Active;
            g.written = mib;
            g
        }]);
        let mut tasks: JoinSet<(usize, Result<SegEnd>)> = JoinSet::new();
        let mut spawned = Vec::new();
        fill_connections(
            &state,
            2,
            2 * mib,
            &mut |i, _| {
                lock(&state).segments[i].state = SegmentState::Active;
                spawned.push(i);
            },
            &mut tasks,
            DownloadId(1),
            false,
        );
        assert_eq!(spawned, vec![1]);
        let lower_end = lock(&state).segments[0].end;
        assert!(lower_end < 8 * mib);
        // the pump had already queued data past the new end; the writer lands it late
        {
            let mut s = lock(&state);
            record_write(&mut s.segments[0], mib, 7 * mib, false);
        }
        let s = lock(&state);
        assert_eq!(s.segments[0].end, lower_end, "end must not grow back");
        assert!(s.segments[0].written <= s.segments[0].len());
        assert!(covers(&s.segments, 8 * mib), "table still covers the file");
        let total: u64 = s.segments.iter().map(|g| g.written).sum();
        assert!(total <= 8 * mib, "progress never exceeds the file size");
    }

    #[test]
    fn an_unknown_size_stream_still_grows_with_its_data() {
        let mut g = seg(0, 0, 0);
        record_write(&mut g, 0, 500, true);
        record_write(&mut g, 500, 24, true);
        assert_eq!((g.end, g.written), (524, 524));
    }
}
