//! The `Store`: one SQLite connection plus typed CRUD for every table.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};

use riptide_core::{
    CategoryId, Checksum, ChecksumAlgo, Credentials, Download, DownloadId, DownloadKind,
    DownloadState, PostAction, Priority, Queue, QueueId, QueueSpec, Schedule, Secrets, Segment,
    SegmentState,
};

use crate::migrate::{self, now_millis};
use crate::{Error, Result};

/// How long a writer waits for a lock before giving up (milliseconds).
pub const BUSY_TIMEOUT_MS: u64 = 5_000;

/// A category row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    /// Row id.
    pub id: CategoryId,
    /// Stable key (`compressed`, `documents`, …); the UI translates it.
    pub key: String,
    /// Lower-case extensions without dots.
    pub extensions: Vec<String>,
    /// Default folder, `None` for the global default.
    pub dir: Option<PathBuf>,
    /// Sort order.
    pub position: i64,
}

/// A progress update for one segment, flushed in batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentProgress {
    /// Download.
    pub download_id: DownloadId,
    /// Segment index.
    pub index: u32,
    /// Bytes on disk from the segment start.
    pub written: u64,
    /// New state.
    pub state: SegmentState,
}

/// The database handle. Not `Sync`; one per thread.
#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
}

/// `(cookies, headers_json, auth_user, auth_pass)` as read from the `secrets` table.
type SecretsRow = (Option<String>, String, Option<String>, Option<String>);

const DOWNLOAD_COLUMNS: &str = "id, url, final_url, filename, dir, size, supports_ranges, etag, \
    last_modified, content_type, state, retryable, category_id, queue_id, priority, speed_limit, \
    connections, checksum_algo, checksum_expected, checksum_actual_algo, checksum_actual, error, \
    added_at, started_at, completed_at, kind, media_format, queue_pos";

impl Store {
    /// Opens (or creates) the database file, applies PRAGMAs and migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let conn = Connection::open(path.as_ref())?;
        Self::init(conn)
    }

    /// A private in-memory database (tests, `--dry-run`).
    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        conn.busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))?;
        // journal_mode returns a row, so query it instead of pragma_update
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate::run(&mut conn)?;
        Ok(Store { conn })
    }

    /// Raw connection, for tests and one-off maintenance.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<u32> {
        migrate::current_version(&self.conn)
    }

    /// `"wal"` for file databases, `"memory"` for in-memory ones.
    pub fn journal_mode(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))?)
    }

    /// The effective busy timeout in milliseconds.
    pub fn busy_timeout_ms(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))?)
    }

    /// Runs `f` inside one `BEGIN IMMEDIATE ... COMMIT`: if it returns `Err` (or panics) every
    /// write it made is rolled back. `f` must not open a transaction itself, so only the plain
    /// single-statement methods (`insert_download`, `set_secrets`, `set_setting`, ...) may be
    /// called inside; those that start their own (`set_state`, `replace_segments`, ...) may not.
    pub fn transaction<T>(&mut self, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        match result {
            Ok(Ok(value)) => match self.conn.execute_batch("COMMIT") {
                Ok(()) => Ok(value),
                Err(e) => {
                    let _ = self.conn.execute_batch("ROLLBACK");
                    Err(e.into())
                }
            },
            Ok(Err(e)) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
            Err(panic) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                std::panic::resume_unwind(panic)
            }
        }
    }

    // ------------------------------------------------------------------ downloads

    /// Inserts `d` (its `id` is ignored) and returns the new id.
    pub fn insert_download(&mut self, d: &Download) -> Result<DownloadId> {
        let (algo, hex) = checksum_cols(d.checksum_expected.as_ref());
        let (aalgo, ahex) = checksum_cols(d.checksum_actual.as_ref());
        let position = self.next_position(d.queue_id)?;
        self.conn.execute(
            "INSERT INTO downloads (url, final_url, filename, dir, size, supports_ranges, etag, \
             last_modified, content_type, state, retryable, category_id, queue_id, priority, \
             speed_limit, connections, checksum_algo, checksum_expected, checksum_actual_algo, \
             checksum_actual, error, added_at, started_at, completed_at, kind, media_format, \
             queue_pos) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
             ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
            params![
                d.url,
                d.final_url,
                d.filename,
                path_str(&d.dir),
                d.size.map(to_db).transpose()?,
                d.supports_ranges,
                d.etag,
                d.last_modified,
                d.content_type,
                d.state.name(),
                d.state.retryable(),
                d.category_id.map(|c| c.0),
                d.queue_id.0,
                d.priority.name(),
                d.speed_limit.map(to_db).transpose()?,
                d.connections,
                algo,
                hex,
                aalgo,
                ahex,
                d.error,
                d.added_at,
                d.started_at,
                d.completed_at,
                d.kind.name(),
                d.media_format,
                position,
            ],
        )?;
        Ok(DownloadId(self.conn.last_insert_rowid()))
    }

    /// One download by id.
    pub fn get_download(&self, id: DownloadId) -> Result<Option<Download>> {
        let sql = format!("SELECT {DOWNLOAD_COLUMNS} FROM downloads WHERE id = ?1");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query_and_then([id.0], read_download)?;
        rows.next().transpose()
    }

    /// Every download, oldest first.
    pub fn list_downloads(&self) -> Result<Vec<Download>> {
        let sql = format!("SELECT {DOWNLOAD_COLUMNS} FROM downloads ORDER BY added_at, id");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_and_then([], read_download)?;
        rows.collect()
    }

    /// Downloads in a given state.
    pub fn list_downloads_in_state(&self, state: DownloadState) -> Result<Vec<Download>> {
        let sql = format!(
            "SELECT {DOWNLOAD_COLUMNS} FROM downloads WHERE state = ?1 ORDER BY added_at, id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_and_then([state.name()], read_download)?;
        let all: Vec<Download> = rows.collect::<Result<_>>()?;
        Ok(all.into_iter().filter(|d| d.state == state).collect())
    }

    /// Rewrites every column of `d` (matched by `d.id`) except its queue and place in the
    /// queue, which only [`Self::move_download`], [`Self::reorder_queue_items`] and
    /// [`Self::delete_queue`] change (a transfer saving its probe result must not undo a move
    /// made meanwhile). The state is written as-is; use [`Self::set_state`] when the
    /// transition must be checked.
    pub fn update_download(&mut self, d: &Download) -> Result<()> {
        let (algo, hex) = checksum_cols(d.checksum_expected.as_ref());
        let (aalgo, ahex) = checksum_cols(d.checksum_actual.as_ref());
        let n = self.conn.execute(
            "UPDATE downloads SET url = ?2, final_url = ?3, filename = ?4, dir = ?5, size = ?6, \
             supports_ranges = ?7, etag = ?8, last_modified = ?9, content_type = ?10, state = ?11, \
             retryable = ?12, category_id = ?13, priority = ?15, speed_limit = ?16, \
             connections = ?17, checksum_algo = ?18, checksum_expected = ?19, \
             checksum_actual_algo = ?20, checksum_actual = ?21, error = ?22, added_at = ?23, \
             started_at = ?24, completed_at = ?25, kind = ?26, media_format = ?27 WHERE id = ?1",
            params![
                d.id.0,
                d.url,
                d.final_url,
                d.filename,
                path_str(&d.dir),
                d.size.map(to_db).transpose()?,
                d.supports_ranges,
                d.etag,
                d.last_modified,
                d.content_type,
                d.state.name(),
                d.state.retryable(),
                d.category_id.map(|c| c.0),
                d.queue_id.0,
                d.priority.name(),
                d.speed_limit.map(to_db).transpose()?,
                d.connections,
                algo,
                hex,
                aalgo,
                ahex,
                d.error,
                d.added_at,
                d.started_at,
                d.completed_at,
                d.kind.name(),
                d.media_format,
            ],
        )?;
        if n == 0 {
            return Err(Error::NotFound(d.id));
        }
        Ok(())
    }

    /// Checked state change in one transaction: reads the current state, validates the transition
    /// against the core table, writes the new state (plus `error`, `started_at`, `completed_at`)
    /// and wipes the secrets row when the new state is terminal. Returns the previous state.
    pub fn set_state(
        &mut self,
        id: DownloadId,
        to: DownloadState,
        error: Option<&str>,
    ) -> Result<DownloadState> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, Option<bool>, Option<i64>)> = tx
            .query_row(
                "SELECT state, retryable, started_at FROM downloads WHERE id = ?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (name, retryable, started_at) = row.ok_or(Error::NotFound(id))?;
        let from = DownloadState::from_name(&name, retryable.unwrap_or(false))
            .ok_or_else(|| Error::Corrupt(format!("unknown state {name:?}")))?;
        from.transition(to)?;

        let now = now_millis();
        let started_at = match (started_at, to) {
            (None, DownloadState::Probing) => Some(now),
            (s, _) => s,
        };
        let completed_at = matches!(to, DownloadState::Completed).then_some(now);
        let error = match to {
            DownloadState::Failed { .. } => error.map(str::to_owned),
            _ => None,
        };
        tx.execute(
            "UPDATE downloads SET state = ?2, retryable = ?3, error = ?4, started_at = ?5, \
             completed_at = COALESCE(?6, completed_at) WHERE id = ?1",
            params![
                id.0,
                to.name(),
                to.retryable(),
                error,
                started_at,
                completed_at
            ],
        )?;
        if to.is_terminal() {
            tx.execute("DELETE FROM secrets WHERE download_id = ?1", [id.0])?;
        }
        tx.commit()?;
        Ok(from)
    }

    /// Deletes the download, its segments and its secrets. Returns `false` if it did not exist.
    pub fn remove_download(&mut self, id: DownloadId) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM downloads WHERE id = ?1", [id.0])?;
        Ok(n > 0)
    }

    // ------------------------------------------------------------------ segments

    /// Replaces the segment list of a download in one transaction. Every segment must belong to
    /// `id`.
    pub fn replace_segments(&mut self, id: DownloadId, segments: &[Segment]) -> Result<()> {
        if let Some(bad) = segments.iter().find(|s| s.download_id != id) {
            return Err(Error::Corrupt(format!(
                "segment {} belongs to download {} not {}",
                bad.index, bad.download_id.0, id.0
            )));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM segments WHERE download_id = ?1", [id.0])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO segments (download_id, idx, start_byte, end_byte, written, state) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for s in segments {
                ins.execute(params![
                    s.download_id.0,
                    s.index,
                    to_db(s.start)?,
                    to_db(s.end)?,
                    to_db(s.written)?,
                    s.state.name()
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Segments of a download ordered by index.
    pub fn segments(&self, id: DownloadId) -> Result<Vec<Segment>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT download_id, idx, start_byte, end_byte, written, state FROM segments \
             WHERE download_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_and_then([id.0], read_segment)?;
        rows.collect()
    }

    /// Applies a batch of progress updates in one transaction (the per-second flush).
    pub fn update_segments(&mut self, updates: &[SegmentProgress]) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut upd = tx.prepare_cached(
                "UPDATE segments SET written = ?3, state = ?4 WHERE download_id = ?1 AND idx = ?2",
            )?;
            for u in updates {
                upd.execute(params![
                    u.download_id.0,
                    u.index,
                    to_db(u.written)?,
                    u.state.name()
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Records a work-stealing split: shrinks `lower` (same index) and inserts `upper`, atomically.
    pub fn split_segment(&mut self, lower: &Segment, upper: &Segment) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute(
            "UPDATE segments SET end_byte = ?3, written = ?4, state = ?5 \
             WHERE download_id = ?1 AND idx = ?2",
            params![
                lower.download_id.0,
                lower.index,
                to_db(lower.end)?,
                to_db(lower.written)?,
                lower.state.name()
            ],
        )?;
        if n == 0 {
            return Err(Error::Corrupt(format!(
                "segment {} of download {} does not exist",
                lower.index, lower.download_id.0
            )));
        }
        tx.execute(
            "INSERT INTO segments (download_id, idx, start_byte, end_byte, written, state) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                upper.download_id.0,
                upper.index,
                to_db(upper.start)?,
                to_db(upper.end)?,
                to_db(upper.written)?,
                upper.state.name()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Sum of `written` over all segments of a download.
    pub fn downloaded_bytes(&self, id: DownloadId) -> Result<u64> {
        let sum: Option<i64> = self.conn.query_row(
            "SELECT SUM(written) FROM segments WHERE download_id = ?1",
            [id.0],
            |r| r.get(0),
        )?;
        from_db(sum.unwrap_or(0))
    }

    // ------------------------------------------------------------------ secrets

    /// Stores (or replaces) the secrets of a download.
    pub fn set_secrets(&mut self, id: DownloadId, secrets: &Secrets) -> Result<()> {
        let headers = serde_json::to_string(&secrets.headers).map_err(|source| Error::Json {
            what: "secrets.headers",
            source,
        })?;
        let n = self.conn.execute(
            "INSERT INTO secrets (download_id, cookies, headers, auth_user, auth_pass) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(download_id) DO UPDATE SET cookies = excluded.cookies, \
             headers = excluded.headers, auth_user = excluded.auth_user, \
             auth_pass = excluded.auth_pass",
            params![
                id.0,
                secrets.cookies,
                headers,
                secrets.auth.as_ref().map(|a| a.username.as_str()),
                secrets.auth.as_ref().map(|a| a.password.as_str()),
            ],
        );
        match n {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(Error::NotFound(id))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Secrets of a download, if any.
    pub fn secrets(&self, id: DownloadId) -> Result<Option<Secrets>> {
        let row: Option<SecretsRow> = self
            .conn
            .query_row(
                "SELECT cookies, headers, auth_user, auth_pass FROM secrets WHERE download_id = ?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((cookies, headers, user, pass)) = row else {
            return Ok(None);
        };
        let headers: Vec<(String, String)> =
            serde_json::from_str(&headers).map_err(|source| Error::Json {
                what: "secrets.headers",
                source,
            })?;
        let auth = match (user, pass) {
            (Some(username), Some(password)) => Some(Credentials { username, password }),
            _ => None,
        };
        Ok(Some(Secrets {
            cookies,
            headers,
            auth,
        }))
    }

    /// Deletes the secrets of a download (no-op if none).
    pub fn delete_secrets(&mut self, id: DownloadId) -> Result<()> {
        self.conn
            .execute("DELETE FROM secrets WHERE download_id = ?1", [id.0])?;
        Ok(())
    }

    // ------------------------------------------------------------------ settings

    /// A setting value.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Sets a setting.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Deletes a setting (no error when it is not there).
    pub fn remove_setting(&mut self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Every setting, sorted by key.
    pub fn settings(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ------------------------------------------------------------------ queues / categories

    /// All queues by position.
    pub fn queues(&self) -> Result<Vec<Queue>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {QUEUE_COLUMNS} FROM queues ORDER BY position, id"
        ))?;
        let rows = stmt.query_and_then([], read_queue)?;
        rows.collect()
    }

    /// One queue.
    pub fn get_queue(&self, id: QueueId) -> Result<Option<Queue>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {QUEUE_COLUMNS} FROM queues WHERE id = ?1"))?;
        let mut rows = stmt.query_and_then([id.0], read_queue)?;
        rows.next().transpose()
    }

    /// Creates a queue (no schedule, started) and returns its id. See [`Self::create_queue`].
    pub fn insert_queue(&mut self, name: &str, max_concurrent: u32) -> Result<QueueId> {
        self.create_queue(&QueueSpec::new(name, max_concurrent))
    }

    /// Creates a started queue after the existing ones.
    ///
    /// Errors: [`riptide_core::Error::Invalid`] (through [`Error::Core`]) for a bad spec or a
    /// name another queue already has.
    pub fn create_queue(&mut self, spec: &QueueSpec) -> Result<QueueId> {
        let spec = spec.clone().validated()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_name_free(&tx, &spec.name, None)?;
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM queues",
            [],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO queues (name, max_concurrent, position, enabled, schedule, on_done) \
             VALUES (?1, ?2, ?3, 1, ?4, ?5)",
            params![
                spec.name,
                spec.max_concurrent,
                position,
                schedule_json(&spec.schedule)?,
                on_done_json(spec.on_done.as_ref())?,
            ],
        )?;
        let id = QueueId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
    }

    /// Replaces a queue's name, limit, schedule and post action. Returns the stored queue.
    pub fn update_queue(&mut self, id: QueueId, spec: &QueueSpec) -> Result<Queue> {
        let spec = spec.clone().validated()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_name_free(&tx, &spec.name, Some(id))?;
        let n = tx.execute(
            "UPDATE queues SET name = ?2, max_concurrent = ?3, schedule = ?4, on_done = ?5 \
             WHERE id = ?1",
            params![
                id.0,
                spec.name,
                spec.max_concurrent,
                schedule_json(&spec.schedule)?,
                on_done_json(spec.on_done.as_ref())?,
            ],
        )?;
        if n == 0 {
            return Err(Error::QueueNotFound(id));
        }
        tx.commit()?;
        self.get_queue(id)?.ok_or(Error::QueueNotFound(id))
    }

    /// Starts (`true`) or stops (`false`) a queue. Returns `true` if the switch changed.
    pub fn set_queue_enabled(&mut self, id: QueueId, enabled: bool) -> Result<bool> {
        let before: Option<bool> = self
            .conn
            .query_row("SELECT enabled FROM queues WHERE id = ?1", [id.0], |r| {
                r.get(0)
            })
            .optional()?;
        let before = before.ok_or(Error::QueueNotFound(id))?;
        if before != enabled {
            self.conn.execute(
                "UPDATE queues SET enabled = ?2 WHERE id = ?1",
                params![id.0, enabled],
            )?;
        }
        Ok(before != enabled)
    }

    /// Replaces only the schedule (the engine uses this to clear a fired one-shot).
    pub fn set_queue_schedule(&mut self, id: QueueId, schedule: &Schedule) -> Result<()> {
        schedule.validate()?;
        let n = self.conn.execute(
            "UPDATE queues SET schedule = ?2 WHERE id = ?1",
            params![id.0, schedule_json(schedule)?],
        )?;
        if n == 0 {
            return Err(Error::QueueNotFound(id));
        }
        Ok(())
    }

    /// Deletes a queue; its downloads join the end of *Main* in their current order. Returns
    /// the ids that moved. *Main* itself cannot be deleted.
    pub fn delete_queue(&mut self, id: QueueId) -> Result<Vec<DownloadId>> {
        if id == QueueId::MAIN {
            return Err(
                riptide_core::Error::Invalid("the Main queue cannot be deleted".into()).into(),
            );
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: i64 =
            tx.query_row("SELECT COUNT(*) FROM queues WHERE id = ?1", [id.0], |r| {
                r.get(0)
            })?;
        if exists == 0 {
            return Err(Error::QueueNotFound(id));
        }
        let moved: Vec<DownloadId> = {
            let mut stmt =
                tx.prepare("SELECT id FROM downloads WHERE queue_id = ?1 ORDER BY queue_pos, id")?;
            let rows = stmt.query_map([id.0], |r| Ok(DownloadId(r.get(0)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut pos = max_position(&tx, QueueId::MAIN)?;
        for d in &moved {
            pos += 1;
            tx.execute(
                "UPDATE downloads SET queue_id = ?2, queue_pos = ?3 WHERE id = ?1",
                params![d.0, QueueId::MAIN.0, pos],
            )?;
        }
        tx.execute("DELETE FROM queues WHERE id = ?1", [id.0])?;
        tx.commit()?;
        Ok(moved)
    }

    /// Puts the queues in the given order (ids not listed follow, in their current order).
    pub fn reorder_queues(&mut self, order: &[QueueId]) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Vec<QueueId> = {
            let mut stmt = tx.prepare("SELECT id FROM queues ORDER BY position, id")?;
            let rows = stmt.query_map([], |r| Ok(QueueId(r.get(0)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let full = merge_order(&current, order)?;
        for (i, q) in full.iter().enumerate() {
            tx.execute(
                "UPDATE queues SET position = ?2 WHERE id = ?1",
                params![q.0, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves a download to the end of another queue. Returns the queue it left.
    pub fn move_download(&mut self, id: DownloadId, to: QueueId) -> Result<QueueId> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: i64 =
            tx.query_row("SELECT COUNT(*) FROM queues WHERE id = ?1", [to.0], |r| {
                r.get(0)
            })?;
        if exists == 0 {
            return Err(Error::QueueNotFound(to));
        }
        let from: Option<i64> = tx
            .query_row(
                "SELECT queue_id FROM downloads WHERE id = ?1",
                [id.0],
                |r| r.get(0),
            )
            .optional()?;
        let from = QueueId(from.ok_or(Error::NotFound(id))?);
        if from != to {
            let pos = max_position(&tx, to)? + 1;
            tx.execute(
                "UPDATE downloads SET queue_id = ?2, queue_pos = ?3 WHERE id = ?1",
                params![id.0, to.0, pos],
            )?;
        }
        tx.commit()?;
        Ok(from)
    }

    /// Orders the downloads of a queue: `order` first, then the rest in their current order.
    /// Every id in `order` must belong to `queue`. Positions are renumbered `1..=n`.
    pub fn reorder_queue_items(&mut self, queue: QueueId, order: &[DownloadId]) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM queues WHERE id = ?1",
            [queue.0],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(Error::QueueNotFound(queue));
        }
        let current: Vec<DownloadId> = {
            let mut stmt =
                tx.prepare("SELECT id FROM downloads WHERE queue_id = ?1 ORDER BY queue_pos, id")?;
            let rows = stmt.query_map([queue.0], |r| Ok(DownloadId(r.get(0)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let full = merge_order(&current, order)?;
        for (i, d) in full.iter().enumerate() {
            tx.execute(
                "UPDATE downloads SET queue_pos = ?2 WHERE id = ?1",
                params![d.0, i as i64 + 1],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Downloads of a queue in their manual order.
    pub fn list_downloads_in_queue(&self, queue: QueueId) -> Result<Vec<Download>> {
        let sql = format!(
            "SELECT {DOWNLOAD_COLUMNS} FROM downloads WHERE queue_id = ?1 ORDER BY queue_pos, id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_and_then([queue.0], read_download)?;
        rows.collect()
    }

    /// Position for a download appended to `queue`.
    fn next_position(&self, queue: QueueId) -> Result<i64> {
        Ok(max_position(&self.conn, queue)? + 1)
    }

    /// All categories by position.
    pub fn categories(&self) -> Result<Vec<Category>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, key, extensions, dir, position FROM categories ORDER BY position, id",
        )?;
        let rows = stmt.query_map([], |r| {
            let ext: String = r.get(2)?;
            let dir: Option<String> = r.get(3)?;
            Ok(Category {
                id: CategoryId(r.get(0)?),
                key: r.get(1)?,
                extensions: ext
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                dir: dir.map(PathBuf::from),
                position: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Updates the extension list and default folder of a category.
    pub fn update_category(
        &mut self,
        id: CategoryId,
        extensions: &[String],
        dir: Option<&Path>,
    ) -> Result<()> {
        let ext = extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        let n = self.conn.execute(
            "UPDATE categories SET extensions = ?2, dir = ?3 WHERE id = ?1",
            params![id.0, ext, dir.map(path_str)],
        )?;
        if n == 0 {
            return Err(Error::Corrupt(format!("category {} does not exist", id.0)));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------- helpers

const QUEUE_COLUMNS: &str = "id, name, max_concurrent, position, enabled, schedule, on_done";

fn max_position(conn: &Connection, queue: QueueId) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(queue_pos), 0) FROM downloads WHERE queue_id = ?1",
        [queue.0],
        |r| r.get(0),
    )?)
}

fn ensure_name_free(conn: &Connection, name: &str, except: Option<QueueId>) -> Result<()> {
    let taken: i64 = conn.query_row(
        "SELECT COUNT(*) FROM queues WHERE name = ?1 COLLATE NOCASE AND id != ?2",
        params![name, except.map_or(-1, |q| q.0)],
        |r| r.get(0),
    )?;
    if taken > 0 {
        return Err(
            riptide_core::Error::Invalid(format!("a queue named {name:?} already exists")).into(),
        );
    }
    Ok(())
}

/// `wanted` first (no duplicates, all must be in `current`), then the rest of `current`.
fn merge_order<T: Copy + PartialEq + std::fmt::Debug>(
    current: &[T],
    wanted: &[T],
) -> Result<Vec<T>> {
    let mut out: Vec<T> = Vec::with_capacity(current.len());
    for w in wanted {
        if !current.contains(w) {
            return Err(
                riptide_core::Error::Invalid(format!("{w:?} is not part of this list")).into(),
            );
        }
        if !out.contains(w) {
            out.push(*w);
        }
    }
    let rest: Vec<T> = current
        .iter()
        .filter(|c| !out.contains(c))
        .copied()
        .collect();
    out.extend(rest);
    Ok(out)
}

fn schedule_json(s: &Schedule) -> Result<Option<String>> {
    if s.is_unscheduled() {
        return Ok(None);
    }
    serde_json::to_string(s)
        .map(Some)
        .map_err(|source| Error::Json {
            what: "queues.schedule",
            source,
        })
}

fn on_done_json(a: Option<&PostAction>) -> Result<Option<String>> {
    a.map(serde_json::to_string)
        .transpose()
        .map_err(|source| Error::Json {
            what: "queues.on_done",
            source,
        })
}

fn read_queue(row: &Row<'_>) -> Result<Queue> {
    let schedule: Option<String> = row.get(5)?;
    let on_done: Option<String> = row.get(6)?;
    Ok(Queue {
        id: QueueId(row.get(0)?),
        name: row.get(1)?,
        max_concurrent: row.get(2)?,
        position: row.get(3)?,
        enabled: row.get(4)?,
        schedule: schedule
            .map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(|source| Error::Json {
                what: "queues.schedule",
                source,
            })?
            .unwrap_or_default(),
        on_done: on_done
            .map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(|source| Error::Json {
                what: "queues.on_done",
                source,
            })?,
    })
}

fn to_db(v: u64) -> Result<i64> {
    i64::try_from(v)
        .map_err(|_| Error::Corrupt(format!("value {v} does not fit in SQLite INTEGER")))
}

fn from_db(v: i64) -> Result<u64> {
    u64::try_from(v).map_err(|_| Error::Corrupt(format!("negative value {v}")))
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn checksum_cols(c: Option<&Checksum>) -> (Option<&'static str>, Option<&str>) {
    match c {
        Some(c) => (Some(c.algo.name()), Some(c.hex.as_str())),
        None => (None, None),
    }
}

fn read_checksum(
    algo: Option<String>,
    hex: Option<String>,
    what: &str,
) -> Result<Option<Checksum>> {
    match (algo, hex) {
        (None, None) => Ok(None),
        (Some(a), Some(h)) => {
            let algo = ChecksumAlgo::from_name(&a)
                .ok_or_else(|| Error::Corrupt(format!("{what}: unknown algorithm {a:?}")))?;
            Checksum::new(algo, &h)
                .ok_or_else(|| Error::Corrupt(format!("{what}: bad {a} digest {h:?}")))
                .map(Some)
        }
        _ => Err(Error::Corrupt(format!(
            "{what}: algorithm and digest must both be set"
        ))),
    }
}

fn read_download(row: &Row<'_>) -> Result<Download> {
    let state_name: String = row.get("state")?;
    let retryable: Option<bool> = row.get("retryable")?;
    let state = DownloadState::from_name(&state_name, retryable.unwrap_or(false))
        .ok_or_else(|| Error::Corrupt(format!("unknown state {state_name:?}")))?;
    let priority_name: String = row.get("priority")?;
    let priority = Priority::from_name(&priority_name)
        .ok_or_else(|| Error::Corrupt(format!("unknown priority {priority_name:?}")))?;
    let dir: String = row.get("dir")?;
    let size: Option<i64> = row.get("size")?;
    let speed_limit: Option<i64> = row.get("speed_limit")?;
    let kind_name: String = row.get("kind")?;
    let kind = DownloadKind::from_name(&kind_name)
        .ok_or_else(|| Error::Corrupt(format!("unknown kind {kind_name:?}")))?;
    Ok(Download {
        id: DownloadId(row.get("id")?),
        url: row.get("url")?,
        final_url: row.get("final_url")?,
        filename: row.get("filename")?,
        dir: PathBuf::from(dir),
        size: size.map(from_db).transpose()?,
        supports_ranges: row.get("supports_ranges")?,
        etag: row.get("etag")?,
        last_modified: row.get("last_modified")?,
        content_type: row.get("content_type")?,
        state,
        category_id: row.get::<_, Option<i64>>("category_id")?.map(CategoryId),
        queue_id: QueueId(row.get("queue_id")?),
        queue_position: row.get("queue_pos")?,
        priority,
        speed_limit: speed_limit.map(from_db).transpose()?,
        connections: row.get("connections")?,
        checksum_expected: read_checksum(
            row.get("checksum_algo")?,
            row.get("checksum_expected")?,
            "checksum_expected",
        )?,
        checksum_actual: read_checksum(
            row.get("checksum_actual_algo")?,
            row.get("checksum_actual")?,
            "checksum_actual",
        )?,
        error: row.get("error")?,
        added_at: row.get("added_at")?,
        started_at: row.get("started_at")?,
        completed_at: row.get("completed_at")?,
        kind,
        media_format: row.get("media_format")?,
    })
}

fn read_segment(row: &Row<'_>) -> Result<Segment> {
    let state_name: String = row.get(5)?;
    let state = SegmentState::from_name(&state_name)
        .ok_or_else(|| Error::Corrupt(format!("unknown segment state {state_name:?}")))?;
    Ok(Segment {
        download_id: DownloadId(row.get(0)?),
        index: row.get(1)?,
        start: from_db(row.get(2)?)?,
        end: from_db(row.get(3)?)?,
        written: from_db(row.get(4)?)?,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use riptide_core::{plan_segments, ByteRange, MIN_SEGMENT_BYTES};

    fn sample(url: &str) -> Download {
        let mut d = Download::new(
            DownloadId(0),
            url,
            "file.zip",
            "C:/Downloads",
            1_700_000_000_000,
        );
        d.size = Some(10 * MIN_SEGMENT_BYTES);
        d.supports_ranges = true;
        d.etag = Some("\"abc\"".into());
        d.checksum_expected = Checksum::guess(&"ab".repeat(32));
        d
    }

    #[test]
    fn transaction_commits_all_or_rolls_back_all() {
        let mut store = Store::open_in_memory().unwrap();
        let ids = store
            .transaction(|s| {
                (0..5)
                    .map(|i| s.insert_download(&sample(&format!("https://a/{i}.zip"))))
                    .collect::<Result<Vec<_>>>()
            })
            .unwrap();
        assert_eq!(ids.len(), 5);
        assert_eq!(store.list_downloads().unwrap().len(), 5);

        let failed: Result<()> = store.transaction(|s| {
            s.insert_download(&sample("https://a/x.zip"))?;
            s.insert_download(&sample("https://a/y.zip"))?;
            Err(Error::NotFound(DownloadId(99)))
        });
        assert!(failed.is_err());
        assert_eq!(store.list_downloads().unwrap().len(), 5);

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = store.transaction(|s| -> Result<()> {
                s.insert_download(&sample("https://a/z.zip"))?;
                panic!("boom");
            });
        }));
        assert!(panicked.is_err());
        assert_eq!(store.list_downloads().unwrap().len(), 5);
        // the connection is usable afterwards
        store.insert_download(&sample("https://a/ok.zip")).unwrap();
        assert_eq!(store.list_downloads().unwrap().len(), 6);
    }

    #[test]
    fn media_kind_and_format_round_trip() {
        let mut store = Store::open_in_memory().unwrap();
        let mut d = sample("https://www.youtube.com/watch?v=x");
        d.kind = DownloadKind::Media;
        d.media_format = Some("h720".into());
        let id = store.insert_download(&d).unwrap();
        let mut back = store.get_download(id).unwrap().unwrap();
        assert_eq!(back.kind, DownloadKind::Media);
        assert_eq!(back.media_format.as_deref(), Some("h720"));
        back.media_format = Some("audio-mp3".into());
        store.update_download(&back).unwrap();
        let again = store.get_download(id).unwrap().unwrap();
        assert_eq!(again.media_format.as_deref(), Some("audio-mp3"));
        let plain = store.insert_download(&sample("https://a/b.zip")).unwrap();
        assert_eq!(
            store.get_download(plain).unwrap().unwrap().kind,
            DownloadKind::Http
        );
    }

    #[test]
    fn opens_with_wal_and_busy_timeout_on_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("r.sqlite")).unwrap();
        assert_eq!(store.journal_mode().unwrap(), "wal");
        assert_eq!(
            store.busy_timeout_ms().unwrap(),
            i64::try_from(BUSY_TIMEOUT_MS).unwrap()
        );
        assert_eq!(store.schema_version().unwrap(), crate::SCHEMA_VERSION);
        let fk: i64 = store
            .connection()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn default_queue_and_categories_exist() {
        let store = Store::open_in_memory().unwrap();
        let queues = store.queues().unwrap();
        assert_eq!(queues.len(), 1);
        assert_eq!(queues[0].id, QueueId::MAIN);
        assert_eq!(queues[0].name, "Main");
        let cats = store.categories().unwrap();
        let keys: Vec<&str> = cats.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "compressed",
                "documents",
                "music",
                "video",
                "programs",
                "images",
                "other"
            ]
        );
        assert!(cats[0].extensions.contains(&"zip".to_string()));
        assert!(cats[6].extensions.is_empty());
        // the seed and `riptide_core::Category` are the same table (see category.rs)
        for (row, category) in cats.iter().zip(riptide_core::Category::ALL) {
            assert_eq!(row.key, category.key());
            assert_eq!(row.extensions, category.extensions(), "{}", category.key());
        }
    }

    #[test]
    fn insert_get_list_update_round_trip() {
        let mut store = Store::open_in_memory().unwrap();
        let mut d = sample("https://a/1.zip");
        let id = store.insert_download(&d).unwrap();
        d.id = id;
        d.queue_position = 1; // assigned by the store
        assert_eq!(store.get_download(id).unwrap(), Some(d.clone()));
        assert_eq!(store.get_download(DownloadId(999)).unwrap(), None);

        let id2 = store.insert_download(&sample("https://a/2.zip")).unwrap();
        let all = store.list_downloads().unwrap();
        assert_eq!(all.iter().map(|d| d.id).collect::<Vec<_>>(), [id, id2]);

        d.filename = "renamed.zip".into();
        d.speed_limit = Some(1_000_000);
        d.priority = Priority::High;
        d.checksum_actual = Checksum::guess(&"cd".repeat(32));
        store.update_download(&d).unwrap();
        assert_eq!(store.get_download(id).unwrap(), Some(d.clone()));

        d.id = DownloadId(12345);
        assert!(matches!(
            store.update_download(&d),
            Err(Error::NotFound(DownloadId(12345)))
        ));
    }

    #[test]
    fn set_state_enforces_the_transition_table() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.insert_download(&sample("https://a/x")).unwrap();

        assert_eq!(
            store.set_state(id, DownloadState::Probing, None).unwrap(),
            DownloadState::Queued
        );
        let d = store.get_download(id).unwrap().unwrap();
        assert_eq!(d.state, DownloadState::Probing);
        assert!(d.started_at.is_some(), "started_at set when leaving Queued");

        let err = store
            .set_state(id, DownloadState::Completed, None)
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::Core(riptide_core::Error::IllegalTransition { .. })
            ),
            "{err}"
        );
        assert_eq!(
            store.get_download(id).unwrap().unwrap().state,
            DownloadState::Probing
        );

        store
            .set_state(id, DownloadState::Failed { retryable: false }, Some("boom"))
            .unwrap();
        let d = store.get_download(id).unwrap().unwrap();
        assert_eq!(d.state, DownloadState::Failed { retryable: false });
        assert_eq!(d.error.as_deref(), Some("boom"));

        store.set_state(id, DownloadState::Queued, None).unwrap();
        assert_eq!(
            store.get_download(id).unwrap().unwrap().error,
            None,
            "error cleared on retry"
        );

        assert!(matches!(
            store.set_state(DownloadId(77), DownloadState::Probing, None),
            Err(Error::NotFound(DownloadId(77)))
        ));
    }

    #[test]
    fn segments_replace_update_split_and_sum() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.insert_download(&sample("https://a/x")).unwrap();
        let plan = plan_segments(10 * MIN_SEGMENT_BYTES, 4, MIN_SEGMENT_BYTES);
        let segs: Vec<Segment> = plan
            .iter()
            .enumerate()
            .map(|(i, r)| Segment::pending(id, i as u32, *r))
            .collect();
        store.replace_segments(id, &segs).unwrap();
        assert_eq!(store.segments(id).unwrap(), segs);
        assert_eq!(store.downloaded_bytes(id).unwrap(), 0);

        store
            .update_segments(&[
                SegmentProgress {
                    download_id: id,
                    index: 0,
                    written: 1000,
                    state: SegmentState::Active,
                },
                SegmentProgress {
                    download_id: id,
                    index: 1,
                    written: 500,
                    state: SegmentState::Failed,
                },
            ])
            .unwrap();
        let got = store.segments(id).unwrap();
        assert_eq!((got[0].written, got[0].state), (1000, SegmentState::Active));
        assert_eq!((got[1].written, got[1].state), (500, SegmentState::Failed));
        assert_eq!(store.downloaded_bytes(id).unwrap(), 1500);

        let (lower, upper) =
            riptide_core::split_segment(&got[3], 4, riptide_core::MIN_SPLIT_REMAINDER_BYTES)
                .unwrap();
        store.split_segment(&lower, &upper).unwrap();
        let got = store.segments(id).unwrap();
        assert_eq!(got.len(), 5);
        assert_eq!(got[3].end, upper.start);
        assert_eq!(got[4], upper);

        // the CHECK constraint refuses written > length
        let err = store
            .update_segments(&[SegmentProgress {
                download_id: id,
                index: 0,
                written: u64::MAX / 4,
                state: SegmentState::Active,
            }])
            .unwrap_err();
        assert!(matches!(err, Error::Sqlite(_)), "{err}");

        // wrong owner is rejected before touching the database
        let foreign = Segment::pending(DownloadId(999), 0, ByteRange::new(0, 1));
        assert!(matches!(
            store.replace_segments(id, &[foreign]),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn secrets_round_trip_and_are_wiped_on_terminal_states_and_removal() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.insert_download(&sample("https://a/x")).unwrap();
        assert_eq!(store.secrets(id).unwrap(), None);

        let s = Secrets {
            cookies: Some("session=abc".into()),
            headers: vec![("X-Token".into(), "t".into())],
            auth: Some(Credentials {
                username: "u".into(),
                password: "p".into(),
            }),
        };
        store.set_secrets(id, &s).unwrap();
        assert_eq!(store.secrets(id).unwrap(), Some(s.clone()));
        // replace
        let s2 = Secrets {
            cookies: None,
            ..s.clone()
        };
        store.set_secrets(id, &s2).unwrap();
        assert_eq!(store.secrets(id).unwrap(), Some(s2));

        // a download that does not exist cannot have secrets
        assert!(matches!(
            store.set_secrets(DownloadId(42), &s),
            Err(Error::NotFound(_))
        ));

        // wiped on completion
        store.set_state(id, DownloadState::Probing, None).unwrap();
        store
            .set_state(id, DownloadState::Downloading, None)
            .unwrap();
        store.set_state(id, DownloadState::Verifying, None).unwrap();
        store.set_state(id, DownloadState::Completed, None).unwrap();
        assert_eq!(store.secrets(id).unwrap(), None);
        assert!(store
            .get_download(id)
            .unwrap()
            .unwrap()
            .completed_at
            .is_some());

        // wiped on removal (cascade), along with segments
        let id2 = store.insert_download(&sample("https://a/y")).unwrap();
        store.set_secrets(id2, &s).unwrap();
        store
            .replace_segments(id2, &[Segment::pending(id2, 0, ByteRange::new(0, 10))])
            .unwrap();
        assert!(store.remove_download(id2).unwrap());
        assert!(!store.remove_download(id2).unwrap());
        assert_eq!(store.secrets(id2).unwrap(), None);
        assert!(store.segments(id2).unwrap().is_empty());
        let orphan_secrets: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM secrets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphan_secrets, 0);
    }

    #[test]
    fn settings_and_queues_and_categories() {
        let mut store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("lang").unwrap(), None);
        store.set_setting("lang", "ar").unwrap();
        store.set_setting("lang", "en").unwrap();
        store.set_setting("theme", "dark").unwrap();
        assert_eq!(store.setting("lang").unwrap().as_deref(), Some("en"));
        assert_eq!(
            store.settings().unwrap(),
            vec![
                ("lang".to_string(), "en".to_string()),
                ("theme".to_string(), "dark".to_string())
            ]
        );

        let q = store.insert_queue("Night", 2).unwrap();
        let queues = store.queues().unwrap();
        assert_eq!(queues.len(), 2);
        assert_eq!(queues[1].id, q);
        assert_eq!(queues[1].max_concurrent, 2);
        assert_eq!(queues[1].position, 1);

        let cats = store.categories().unwrap();
        store
            .update_category(
                cats[0].id,
                &["ZIP".into(), ".7z".into(), "".into()],
                Some(Path::new("D:/zips")),
            )
            .unwrap();
        let cats = store.categories().unwrap();
        assert_eq!(cats[0].extensions, ["zip", "7z"]);
        assert_eq!(cats[0].dir.as_deref(), Some(Path::new("D:/zips")));
    }

    #[test]
    fn list_by_state() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.insert_download(&sample("https://a/1")).unwrap();
        let b = store.insert_download(&sample("https://a/2")).unwrap();
        store.set_state(b, DownloadState::Paused, None).unwrap();
        let queued = store
            .list_downloads_in_state(DownloadState::Queued)
            .unwrap();
        assert_eq!(queued.iter().map(|d| d.id).collect::<Vec<_>>(), [a]);
        let paused = store
            .list_downloads_in_state(DownloadState::Paused)
            .unwrap();
        assert_eq!(paused.iter().map(|d| d.id).collect::<Vec<_>>(), [b]);
    }

    fn ids(ds: &[Download]) -> Vec<DownloadId> {
        ds.iter().map(|d| d.id).collect()
    }

    #[test]
    fn queue_crud_round_trips_schedule_and_post_action() {
        use riptide_core::{PostAction, Schedule};
        let mut store = Store::open_in_memory().unwrap();
        let mut spec = QueueSpec::new("Night", 2);
        spec.schedule = Schedule::only_at_night();
        spec.on_done = Some(PostAction::RunCommand {
            command: "notify.exe --done".into(),
        });
        let id = store.create_queue(&spec).unwrap();
        let q = store.get_queue(id).unwrap().unwrap();
        assert_eq!(q.spec(), spec);
        assert!(q.enabled, "new queues start started");
        assert_eq!(q.position, 1);

        // Main stays unscheduled with no action
        let main = store.get_queue(QueueId::MAIN).unwrap().unwrap();
        assert!(main.schedule.is_unscheduled());
        assert_eq!(main.on_done, None);

        spec.name = "Weekend".into();
        spec.max_concurrent = 5;
        spec.on_done = Some(PostAction::Shutdown);
        let q = store.update_queue(id, &spec).unwrap();
        assert_eq!(q.spec(), spec);

        assert!(store.set_queue_enabled(id, false).unwrap());
        assert!(!store.set_queue_enabled(id, false).unwrap(), "no change");
        assert!(!store.get_queue(id).unwrap().unwrap().enabled);

        store.set_queue_schedule(id, &Schedule::default()).unwrap();
        assert!(store
            .get_queue(id)
            .unwrap()
            .unwrap()
            .schedule
            .is_unscheduled());
    }

    #[test]
    fn queue_validation_and_errors() {
        let mut store = Store::open_in_memory().unwrap();
        let invalid = |e: Error| matches!(e, Error::Core(riptide_core::Error::Invalid(_)));
        assert!(invalid(
            store.create_queue(&QueueSpec::new("main", 1)).unwrap_err()
        ));
        assert!(invalid(
            store.create_queue(&QueueSpec::new("  ", 1)).unwrap_err()
        ));
        assert!(invalid(
            store.create_queue(&QueueSpec::new("x", 0)).unwrap_err()
        ));
        let id = store.create_queue(&QueueSpec::new("A", 1)).unwrap();
        // renaming to its own name is fine, to another queue's is not
        store.update_queue(id, &QueueSpec::new("A", 4)).unwrap();
        assert!(invalid(
            store
                .update_queue(id, &QueueSpec::new("Main", 4))
                .unwrap_err()
        ));
        assert!(matches!(
            store.update_queue(QueueId(99), &QueueSpec::new("Z", 1)),
            Err(Error::QueueNotFound(QueueId(99)))
        ));
        assert!(matches!(
            store.set_queue_enabled(QueueId(99), true),
            Err(Error::QueueNotFound(_))
        ));
        assert!(invalid(store.delete_queue(QueueId::MAIN).unwrap_err()));
        assert!(matches!(
            store.delete_queue(QueueId(99)),
            Err(Error::QueueNotFound(_))
        ));
    }

    #[test]
    fn items_get_increasing_positions_and_reorder() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.insert_download(&sample("https://a/1")).unwrap();
        let b = store.insert_download(&sample("https://a/2")).unwrap();
        let c = store.insert_download(&sample("https://a/3")).unwrap();
        let list = store.list_downloads_in_queue(QueueId::MAIN).unwrap();
        assert_eq!(ids(&list), [a, b, c]);
        assert_eq!(
            list.iter().map(|d| d.queue_position).collect::<Vec<_>>(),
            [1, 2, 3]
        );

        store.reorder_queue_items(QueueId::MAIN, &[c, a]).unwrap();
        let list = store.list_downloads_in_queue(QueueId::MAIN).unwrap();
        assert_eq!(ids(&list), [c, a, b], "unlisted items follow");
        assert_eq!(
            list.iter().map(|d| d.queue_position).collect::<Vec<_>>(),
            [1, 2, 3]
        );

        // an id from another queue (or unknown) is refused and changes nothing
        let other = store.create_queue(&QueueSpec::new("O", 1)).unwrap();
        let x = store.insert_download(&sample("https://a/4")).unwrap();
        store.move_download(x, other).unwrap();
        assert!(store.reorder_queue_items(QueueId::MAIN, &[x]).is_err());
        assert!(store.reorder_queue_items(QueueId(99), &[]).is_err());
        assert_eq!(
            ids(&store.list_downloads_in_queue(QueueId::MAIN).unwrap()),
            [c, a, b]
        );
    }

    #[test]
    fn move_appends_and_update_download_does_not_undo_it() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.insert_download(&sample("https://a/1")).unwrap();
        let b = store.insert_download(&sample("https://a/2")).unwrap();
        let night = store.create_queue(&QueueSpec::new("Night", 1)).unwrap();
        let stale = store.get_download(a).unwrap().unwrap();
        assert_eq!(store.move_download(a, night).unwrap(), QueueId::MAIN);
        assert_eq!(store.move_download(b, night).unwrap(), QueueId::MAIN);
        assert_eq!(store.move_download(b, night).unwrap(), night, "no-op move");
        let moved = store.list_downloads_in_queue(night).unwrap();
        assert_eq!(ids(&moved), [a, b]);
        assert_eq!(moved[1].queue_position, 2);

        // a transfer saving a record it read before the move must not put it back
        let mut stale = stale;
        stale.filename = "renamed.zip".into();
        store.update_download(&stale).unwrap();
        let now = store.get_download(a).unwrap().unwrap();
        assert_eq!(now.queue_id, night);
        assert_eq!(now.queue_position, 1);
        assert_eq!(now.filename, "renamed.zip");

        assert!(matches!(
            store.move_download(a, QueueId(99)),
            Err(Error::QueueNotFound(_))
        ));
        assert!(matches!(
            store.move_download(DownloadId(999), night),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn deleting_a_queue_sends_its_items_to_the_end_of_main() {
        let mut store = Store::open_in_memory().unwrap();
        let m1 = store.insert_download(&sample("https://a/m1")).unwrap();
        let night = store.create_queue(&QueueSpec::new("Night", 1)).unwrap();
        let mut d = sample("https://a/n1");
        d.queue_id = night;
        let n1 = store.insert_download(&d).unwrap();
        let n2 = store.insert_download(&d).unwrap();
        store.reorder_queue_items(night, &[n2, n1]).unwrap();
        let m2 = store.insert_download(&sample("https://a/m2")).unwrap();

        assert_eq!(store.delete_queue(night).unwrap(), [n2, n1]);
        assert!(store.get_queue(night).unwrap().is_none());
        let main = store.list_downloads_in_queue(QueueId::MAIN).unwrap();
        assert_eq!(ids(&main), [m1, m2, n2, n1]);
        assert!(main.iter().all(|d| d.queue_id == QueueId::MAIN));
    }

    #[test]
    fn queues_can_be_reordered() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.create_queue(&QueueSpec::new("A", 1)).unwrap();
        let b = store.create_queue(&QueueSpec::new("B", 1)).unwrap();
        store.reorder_queues(&[b, a]).unwrap();
        let order: Vec<_> = store.queues().unwrap().iter().map(|q| q.id).collect();
        assert_eq!(order, [b, a, QueueId::MAIN]);
        assert!(store.reorder_queues(&[QueueId(99)]).is_err());
    }
}
