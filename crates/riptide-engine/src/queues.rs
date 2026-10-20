//! Queue management, the schedule timer, the "queue drained" announcement and the per-host
//! connection caps learned from `429` / `503`.
//!
//! # Semantics
//!
//! * A **queue** is a row (name, `max_concurrent`, order, started/stopped switch, schedule,
//!   post-queue action). *Main* (id 1) always exists and cannot be deleted. A stopped queue
//!   keeps its items `Queued` and starts none; stopping it also sends its *running* items back
//!   to `Queued` (`Paused`, then `Queued`), so starting the queue resumes them.
//! * **Order.** Within a queue: priority (high first), then the manual position
//!   ([`Engine::reorder`], [`Engine::move_to_queue`] appends), then the id.
//! * **Limits.** [`crate::EngineSettings::max_active`] (global) and each queue's
//!   `max_concurrent` hold together; see [`riptide_core::pick_next`].
//! * **Schedules.** A timer task asks each scheduled queue's [`riptide_core::Schedule`]
//!   whether it should be running *now* (local wall-clock) and starts or stops the queue when
//!   the answer changes. It is edge-triggered: a manual start/stop sticks until the next
//!   change, and a queue with a schedule is brought in line when the engine opens or the
//!   schedule is edited. A one-shot `start_at` is cleared once it has fired. **A sleeping PC
//!   is not woken (v1)**: nothing runs while the machine sleeps; on resume the queue is
//!   started or stopped by the next timer pass (within [`SCHEDULE_POLL`]).
//! * **Post-queue action.** When the last unfinished item (`Queued`, `Probing`, `Downloading`,
//!   `Verifying`) of a queue completes the engine emits [`Event::QueueFinished`] once, with the
//!   queue's `on_done`. The engine never shuts anything down; the app layer does (rule 9). The
//!   action stays set after it fired; the app clears it with [`Engine::update_queue`] if it
//!   wants one-shot behaviour.
//! * **Learned host caps.** A `429` / `503` from a host halves the connections used on that
//!   host (see [`riptide_net::HostLimiter::report_throttled`]); the new cap is stored under
//!   `host_cap.<host>` in the settings table and applied after a restart too, until
//!   [`Engine::forget_host_cap`].

use std::time::Duration;

use riptide_core::{DownloadState, LocalDateTime, Queue, QueueSpec};
use riptide_net::{CapObserver, HostLimiter};

use super::*;

/// Longest the schedule timer sleeps between two looks at the clock. Bounds the delay after a
/// PC resumes from sleep or the user changes the system clock.
pub const SCHEDULE_POLL: Duration = Duration::from_secs(30);

/// Settings-table key prefix of learned per-host caps.
pub(crate) const HOST_CAP_PREFIX: &str = "host_cap.";

/// The system's local wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime(chrono::Local::now().naive_local().and_utc().timestamp())
    }
}

/// A host limiter that starts from the learned caps and persists every reduction.
pub(crate) fn make_limiter(
    per_host: usize,
    store: &Arc<Mutex<Store>>,
    caps: &Arc<Mutex<HashMap<String, u32>>>,
) -> Arc<HostLimiter> {
    let learned: HashMap<String, usize> = caps
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|(h, c)| (h.clone(), *c as usize))
        .collect();
    let store = Arc::clone(store);
    let caps = Arc::clone(caps);
    let observer: CapObserver = Arc::new(move |host: &str, cap: usize| {
        let cap = u32::try_from(cap).unwrap_or(u32::MAX);
        caps.lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(host.to_string(), cap);
        tracing::info!(host, cap, "learned a lower connection cap for this host");
        let store = Arc::clone(&store);
        let key = format!("{HOST_CAP_PREFIX}{host}");
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let mut guard = store.lock().unwrap_or_else(|p| p.into_inner());
                if let Err(e) = guard.set_setting(&key, &cap.to_string()) {
                    tracing::warn!(error = %e, "could not persist the learned host cap");
                }
            });
        }
    });
    HostLimiter::with_caps(per_host, learned, Some(observer))
}

impl Engine {
    // ------------------------------------------------------------------ queries

    /// All queues in display order.
    pub async fn queues(&self) -> Result<Vec<Queue>> {
        db(&self.inner.store, |s| s.queues()).await
    }

    /// One queue.
    pub async fn queue(&self, id: QueueId) -> Result<Option<Queue>> {
        db(&self.inner.store, move |s| s.get_queue(id)).await
    }

    /// Downloads of a queue in their manual order.
    pub async fn queue_items(&self, id: QueueId) -> Result<Vec<Download>> {
        db(&self.inner.store, move |s| s.list_downloads_in_queue(id)).await
    }

    // ------------------------------------------------------------------ queue CRUD

    /// Creates a started queue after the existing ones. Emits [`Event::QueueCreated`]; a queue
    /// with a schedule is brought in line with it right away.
    ///
    /// # Errors
    ///
    /// [`Error::Core`] (`Invalid`) for a bad name, limit or schedule, or a name that is taken.
    pub async fn create_queue(&self, spec: QueueSpec) -> Result<QueueId> {
        let id = db(&self.inner.store, move |s| s.create_queue(&spec)).await?;
        self.emit(Event::QueueCreated { queue_id: id });
        self.inner.sched_wake.notify_one();
        Ok(id)
    }

    /// Replaces a queue's name, limit, schedule and post action (not its started/stopped
    /// switch or order). A changed schedule is applied at once: the queue is started or
    /// stopped to match the new answer. Emits [`Event::QueueUpdated`].
    pub async fn update_queue(&self, id: QueueId, spec: QueueSpec) -> Result<Queue> {
        let queue = db(&self.inner.store, move |s| s.update_queue(id, &spec)).await?;
        self.inner
            .sched_applied
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        self.emit(Event::QueueUpdated { queue_id: id });
        self.inner.sched_wake.notify_one();
        self.inner.wake.notify_one();
        Ok(queue)
    }

    /// Deletes a queue; its items (running ones keep running) go to the end of *Main* in their
    /// current order. Emits [`Event::DownloadMoved`] for each, then [`Event::QueueDeleted`].
    ///
    /// # Errors
    ///
    /// [`Error::Core`] (`Invalid`) for *Main*; [`Error::Store`] (`QueueNotFound`) otherwise.
    pub async fn delete_queue(&self, id: QueueId) -> Result<()> {
        let moved = db(&self.inner.store, move |s| s.delete_queue(id)).await?;
        {
            let mut running = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
            for d in &moved {
                if let Some(r) = running.get_mut(d) {
                    r.queue_id = QueueId::MAIN;
                }
            }
        }
        self.inner
            .sched_applied
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        for d in moved {
            self.emit(Event::DownloadMoved {
                id: d,
                from: id,
                to: QueueId::MAIN,
            });
        }
        self.emit(Event::QueueDeleted { queue_id: id });
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Moves a download to the end of another queue. A running download keeps running unless
    /// the target queue is stopped, in which case it goes back to `Queued`. Emits
    /// [`Event::DownloadMoved`].
    pub async fn move_to_queue(&self, id: DownloadId, queue: QueueId) -> Result<()> {
        let from = db(&self.inner.store, move |s| s.move_download(id, queue)).await?;
        if from == queue {
            return Ok(());
        }
        if let Some(r) = self
            .inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(&id)
        {
            r.queue_id = queue;
        }
        self.emit(Event::DownloadMoved {
            id,
            from,
            to: queue,
        });
        let target_enabled = self.queue(queue).await?.is_some_and(|q| q.enabled);
        if !target_enabled {
            self.stop_running(id, StopReason::Requeue);
        }
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Starts a queue: its `Queued` items begin, within the queue and global limits.
    pub async fn start_queue(&self, id: QueueId) -> Result<()> {
        self.set_queue_running(id, true).await
    }

    /// Stops a queue: running items go back to `Queued` and nothing new starts until
    /// [`Engine::start_queue`] (or the schedule) starts it again.
    pub async fn stop_queue(&self, id: QueueId) -> Result<()> {
        self.set_queue_running(id, false).await
    }

    /// Puts the items of `queue` in the given order; items not listed follow in their current
    /// order. Every id must belong to the queue. Emits [`Event::QueueOrderChanged`].
    pub async fn reorder(&self, queue: QueueId, order: &[DownloadId]) -> Result<()> {
        let order = order.to_vec();
        db(&self.inner.store, move |s| {
            s.reorder_queue_items(queue, &order)
        })
        .await?;
        self.emit(Event::QueueOrderChanged { queue_id: queue });
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Puts the queues themselves in the given order. Emits [`Event::QueuesReordered`].
    pub async fn reorder_queues(&self, order: &[QueueId]) -> Result<()> {
        let order = order.to_vec();
        db(&self.inner.store, move |s| s.reorder_queues(&order)).await?;
        self.emit(Event::QueuesReordered);
        self.inner.wake.notify_one();
        Ok(())
    }

    async fn set_queue_running(&self, id: QueueId, enabled: bool) -> Result<()> {
        let changed = db(&self.inner.store, move |s| s.set_queue_enabled(id, enabled)).await?;
        if changed {
            self.emit(Event::QueueUpdated { queue_id: id });
        }
        if enabled {
            self.inner.wake.notify_one();
        } else {
            let ids: Vec<DownloadId> = self
                .inner
                .running
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .filter(|(_, r)| r.queue_id == id)
                .map(|(d, _)| *d)
                .collect();
            for d in ids {
                self.stop_running(d, StopReason::Requeue);
            }
        }
        Ok(())
    }

    /// Keeps the *Main* queue's own limit equal to the global `max_active` (the setting that
    /// predates per-queue limits).
    pub(crate) async fn sync_main_queue_limit(&self, max_active: u32) -> Result<()> {
        let updated = db(&self.inner.store, move |s| {
            let Some(main) = s.get_queue(QueueId::MAIN)? else {
                return Ok(false);
            };
            let limit = max_active.clamp(1, riptide_core::MAX_QUEUE_CONCURRENT);
            if main.max_concurrent == limit {
                return Ok(false);
            }
            let mut spec = main.spec();
            spec.max_concurrent = limit;
            s.update_queue(QueueId::MAIN, &spec)?;
            Ok(true)
        })
        .await?;
        if updated {
            self.emit(Event::QueueUpdated {
                queue_id: QueueId::MAIN,
            });
        }
        Ok(())
    }

    // ------------------------------------------------------------------ schedule timer

    /// Looks at every scheduled queue once and starts or stops it to match its schedule at
    /// the engine clock's *now*. The timer task calls this; call it yourself after the system
    /// clock changed or the machine resumed, and in tests after moving a
    /// [`riptide_core::ManualClock`].
    pub async fn check_schedules(&self) -> Result<()> {
        self.evaluate_schedules().await.map(|_| ())
    }

    /// One pass; returns how long to sleep before the next one.
    async fn evaluate_schedules(&self) -> Result<Duration> {
        let _pass = self.inner.sched_pass.lock().await;
        let now = self.inner.clock.now();
        let mut soonest: Option<LocalDateTime> = None;
        for q in self.queues().await? {
            if q.schedule.is_unscheduled() {
                self.inner
                    .sched_applied
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&q.id);
                continue;
            }
            let level = q.schedule.active_at(now);
            let previous = self
                .inner
                .sched_applied
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(q.id, level);
            if previous != Some(level) {
                tracing::info!(
                    queue = q.id.0,
                    running = level,
                    "schedule changed the queue"
                );
                self.set_queue_running(q.id, level).await?;
            }
            if q.schedule.start_due(now) {
                // the one-shot has fired; from now on only the daily window (if any) rules
                let mut schedule = q.schedule;
                schedule.start_at = None;
                let id = q.id;
                db(&self.inner.store, move |s| {
                    s.set_queue_schedule(id, &schedule)
                })
                .await?;
                self.emit(Event::QueueUpdated { queue_id: id });
            }
            if let Some(t) = q.schedule.next_transition(now) {
                soonest = Some(soonest.map_or(t, |s| s.min(t)));
            }
        }
        Ok(soonest
            .map(|t| Duration::from_secs(u64::try_from(t.0 - now.0).unwrap_or(1).max(1)))
            .map_or(SCHEDULE_POLL, |d| d.min(SCHEDULE_POLL)))
    }

    pub(crate) async fn schedule_timer(self) {
        loop {
            if self.inner.shutting_down.load(Ordering::SeqCst) {
                return;
            }
            let wait = match self.evaluate_schedules().await {
                Ok(wait) => wait,
                Err(e) => {
                    tracing::error!(error = %e, "schedule timer");
                    SCHEDULE_POLL
                }
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.inner.sched_wake.notified() => {}
            }
        }
    }

    // ------------------------------------------------------------------ queue finished

    /// After `id` completed: if nothing else in its queue is unfinished, announce it (once
    /// until the queue starts something again).
    pub(crate) async fn announce_drained_after(&self, id: DownloadId) {
        let found = db(&self.inner.store, move |s| {
            let Some(d) = s.get_download(id)? else {
                return Ok(None);
            };
            let open = s.list_downloads_in_queue(d.queue_id)?.iter().any(|o| {
                matches!(
                    o.state,
                    DownloadState::Queued
                        | DownloadState::Probing
                        | DownloadState::Downloading
                        | DownloadState::Verifying
                )
            });
            Ok(if open { None } else { s.get_queue(d.queue_id)? })
        })
        .await;
        let Ok(Some(queue)) = found else {
            return;
        };
        let first = self
            .inner
            .announced
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(queue.id);
        if first {
            self.emit(Event::QueueFinished {
                queue_id: queue.id,
                action: queue.on_done,
            });
        }
    }

    // ------------------------------------------------------------------ learned host caps

    /// Connection caps learned from `429` / `503` answers, by lower-case host name.
    pub fn host_caps(&self) -> BTreeMap<String, u32> {
        self.inner
            .host_caps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|(h, c)| (h.clone(), *c))
            .collect()
    }

    /// Forgets the learned cap of `host`; the configured per-host limit applies again to
    /// transfers started afterwards.
    pub async fn forget_host_cap(&self, host: &str) -> Result<()> {
        let host = host.to_ascii_lowercase();
        self.inner
            .host_caps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&host);
        let key = format!("{HOST_CAP_PREFIX}{host}");
        // the settings table has no delete; an empty value is "unset" everywhere else too
        db(&self.inner.store, move |s| s.set_setting(&key, "")).await?;
        let per_host = self.settings().per_host as usize;
        if let Ok(mut l) = self.inner.limiter.write() {
            *l = make_limiter(per_host, &self.inner.store, &self.inner.host_caps);
        }
        Ok(())
    }
}
