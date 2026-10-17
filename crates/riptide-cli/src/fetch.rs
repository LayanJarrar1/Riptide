//! `riptide fetch` and `riptide bench`: the standalone engine, driven from the terminal, with
//! the same persistence rules as the app (every checkpoint goes to SQLite, so a killed process
//! resumes byte-exactly).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use riptide_core::{
    sanitize_filename, unique_path, Checksum, Credentials, Download, DownloadId, DownloadState,
    Secrets,
};
use riptide_net::{
    build_client, part_path_for, probe, strip_query, transfer, ClientOptions, HostLimiter, Outcome,
    RequestContext, TransferConfig, TransferEvent, TransferPlan,
};
use riptide_store::Store;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::i18n::{t, tf, Lang};

/// Everything `fetch` needs from the command line.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub url: String,
    /// File or directory; `None` = current directory and the server's name.
    pub output: Option<PathBuf>,
    pub connections: u32,
    pub checksum: Option<Checksum>,
    pub headers: Vec<(String, String)>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub credentials: Option<Credentials>,
    pub insecure: bool,
    pub overwrite: bool,
    /// No progress output (bench, tests).
    pub quiet: bool,
    /// Ignore a partial file and start from zero.
    pub no_resume: bool,
}

/// What a finished fetch looked like.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FetchReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub resumed_from: u64,
    pub connections: u32,
    pub elapsed_ms: u128,
    pub avg_bps: u64,
    pub peak_bps: u64,
    pub retries: u32,
}

/// The store behind a mutex: `rusqlite::Connection` is `Send` but not `Sync`, and every call is
/// made from the blocking pool.
pub type SharedStore = Arc<Mutex<Store>>;

async fn db<T, F>(store: &SharedStore, f: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Store) -> riptide_store::Result<T> + Send + 'static,
{
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || {
        let mut guard = store.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    })
    .await
    .context("store task")?
    .context("store")
}

/// Runs one download to completion, a pause (Ctrl+C) or a failure. The record and its segments
/// live in `store`, so running the same command again resumes.
pub async fn fetch(
    opts: FetchOptions,
    store: SharedStore,
    lang: Lang,
) -> anyhow::Result<FetchReport> {
    let client = build_client(&ClientOptions {
        user_agent: opts.user_agent.clone(),
        accept_invalid_certs: opts.insecure,
    })?;
    let url = riptide_net::parse_http_url(&opts.url)?;
    if opts.insecure && !opts.quiet {
        eprintln!("{}", t(lang, "fetch.insecure"));
    }
    let secrets = Secrets {
        cookies: None,
        headers: opts.headers.clone(),
        auth: opts.credentials.clone(),
    };
    let ctx = Arc::new(
        RequestContext::new(&url)
            .with_referrer(opts.referrer.as_deref())
            .with_user_agent(opts.user_agent.as_deref())
            .with_secrets(&secrets),
    );

    if !opts.quiet {
        eprintln!(
            "{}",
            tf(lang, "fetch.probing", &[("url", &strip_query(&opts.url))])
        );
    }
    let probe = probe(&client, &opts.url, &ctx).await?;

    // --- where it goes -------------------------------------------------------------------------
    let (dir, filename) = match &opts.output {
        Some(p) if p.is_dir() => (p.clone(), probe.filename.clone()),
        Some(p) => {
            let name = p
                .file_name()
                .map(|n| sanitize_filename(&n.to_string_lossy()))
                .unwrap_or_else(|| probe.filename.clone());
            let dir = p
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            (dir, name)
        }
        None => (std::env::current_dir()?, probe.filename.clone()),
    };
    let dir = if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()?.join(dir)
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    // --- the record: resume an existing one or create a new one --------------------------------
    let url_key = opts.url.clone();
    let dir_key = dir.clone();
    let name_key = filename.clone();
    let existing = db(&store, move |s| {
        Ok(s.list_downloads()?.into_iter().find(|d| {
            d.url == url_key && d.dir == dir_key && d.filename == name_key && !d.state.is_terminal()
        }))
    })
    .await?;

    let (mut record, mut segments) = match existing {
        Some(d) if !opts.no_resume => {
            let id = d.id;
            let segs = db(&store, move |s| s.segments(id)).await?;
            let same_validator =
                d.etag.as_deref().or(d.last_modified.as_deref()) == probe.validator();
            if !same_validator && !segs.is_empty() {
                if !opts.quiet {
                    eprintln!("{}", t(lang, "fetch.changed"));
                }
                (d, Vec::new())
            } else {
                if !opts.quiet && !segs.is_empty() {
                    let on_disk: u64 = segs.iter().map(|s| s.written).sum();
                    eprintln!(
                        "{}",
                        tf(lang, "fetch.resuming", &[("bytes", &human_bytes(on_disk))])
                    );
                }
                (d, segs)
            }
        }
        Some(d) => (d, Vec::new()),
        None => {
            let final_path = if opts.overwrite {
                dir.join(&filename)
            } else {
                unique_path(&dir, &filename, |p| p.exists())
            };
            let name = final_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(filename.clone());
            let d = Download::new(
                DownloadId(0),
                opts.url.clone(),
                name,
                dir.clone(),
                now_millis(),
            );
            let mut d = d;
            d.connections = opts.connections;
            d.checksum_expected = opts.checksum.clone();
            let id = db(&store, move |s| s.insert_download(&d)).await?;
            let d = db(&store, move |s| s.get_download(id))
                .await?
                .ok_or_else(|| anyhow!("record vanished"))?;
            (d, Vec::new())
        }
    };

    record.final_url = Some(probe.final_url.to_string());
    record.size = probe.size;
    record.supports_ranges = probe.supports_ranges;
    record.etag = probe.etag.clone();
    record.last_modified = probe.last_modified.clone();
    record.content_type = probe.content_type.clone();
    record.connections = opts.connections;
    if opts.checksum.is_some() {
        record.checksum_expected = opts.checksum.clone();
    }
    let id = record.id;
    let to_save = record.clone();
    db(&store, move |s| s.update_download(&to_save)).await?;

    // Bring the record back to Queued along legal arrows (a record still marked active was left
    // by a crash), then Queued → Probing → Downloading, each its own transaction.
    let state = record.state;
    db(&store, move |s| {
        match state {
            DownloadState::Probing | DownloadState::Downloading => {
                s.set_state(id, DownloadState::Paused, None)?;
                s.set_state(id, DownloadState::Queued, None)?;
            }
            DownloadState::Verifying => {
                s.set_state(
                    id,
                    DownloadState::Failed { retryable: true },
                    Some("interrupted while verifying"),
                )?;
                s.set_state(id, DownloadState::Queued, None)?;
            }
            DownloadState::Paused | DownloadState::Failed { .. } => {
                s.set_state(id, DownloadState::Queued, None)?;
            }
            DownloadState::Queued | DownloadState::Completed | DownloadState::Cancelled => {}
        }
        s.set_state(id, DownloadState::Probing, None)?;
        s.set_state(id, DownloadState::Downloading, None)?;
        Ok(())
    })
    .await?;
    if segments.is_empty() {
        db(&store, move |s| s.replace_segments(id, &[])).await?;
    }
    for s in &mut segments {
        s.download_id = id;
    }

    // --- the transfer ------------------------------------------------------------------------
    let final_path = record.final_path();
    let plan = TransferPlan {
        download_id: id,
        url: probe.final_url.clone(),
        size: probe.size,
        supports_ranges: probe.supports_ranges,
        validator: probe.validator().map(str::to_owned),
        segments,
        part_path: part_path_for(&final_path),
        final_path: final_path.clone(),
        checksum: record.checksum_expected.clone(),
    };
    let cfg = TransferConfig {
        connections: opts.connections,
        overwrite: opts.overwrite,
        ..TransferConfig::default()
    };
    let stop = CancellationToken::new();
    let ctrl_c = {
        let stop = stop.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                stop.cancel();
            }
        })
    };

    let (tx, mut rx) = mpsc::channel::<TransferEvent>(256);
    let started = Instant::now();
    let consumer_store = Arc::clone(&store);
    let quiet = opts.quiet;
    let has_checksum = record.checksum_expected.is_some();
    let consumer = tokio::spawn(async move {
        let mut stats = Stats::default();
        let mut line = ProgressLine::new(quiet, lang);
        while let Some(event) = rx.recv().await {
            match event {
                TransferEvent::Started { resumed_bytes, .. } => stats.resumed_from = resumed_bytes,
                TransferEvent::Progress {
                    downloaded,
                    total,
                    speed_bps,
                    eta_secs,
                    connections,
                } => {
                    stats.peak_bps = stats.peak_bps.max(speed_bps);
                    line.render(downloaded, total, speed_bps, eta_secs, connections);
                }
                TransferEvent::Checkpoint { segments } => {
                    let segs = segments;
                    let _ = db(&consumer_store, move |s| s.replace_segments(id, &segs)).await;
                }
                TransferEvent::Retrying {
                    index,
                    attempt,
                    delay,
                    error,
                } => {
                    stats.retries += 1;
                    line.note(&tf(
                        lang,
                        "fetch.retrying",
                        &[
                            ("n", &index.to_string()),
                            ("attempt", &attempt.to_string()),
                            ("secs", &format!("{:.1}", delay.as_secs_f64())),
                            ("error", &error),
                        ],
                    ));
                }
                TransferEvent::Restarted { reason } => {
                    line.note(&tf(lang, "fetch.restarted", &[("reason", &reason)]))
                }
                TransferEvent::Verifying => {
                    if has_checksum {
                        line.note(t(lang, "fetch.verifying"));
                    }
                }
                TransferEvent::SegmentProgress { .. } | TransferEvent::Completed { .. } => {}
            }
        }
        line.finish();
        stats
    });

    let result = transfer(
        client,
        ctx,
        plan,
        cfg,
        tx,
        stop.clone(),
        // public servers reset or throttle beyond ~8 parallel streams from one address
        // (measured against a CDN test file); a setting comes later
        HostLimiter::new(8),
    )
    .await;
    ctrl_c.abort();
    let stats = consumer.await.context("progress task")?;
    let elapsed = started.elapsed();

    match result {
        Ok(Outcome::Completed { path, .. }) => {
            db(&store, move |s| {
                s.set_state(id, DownloadState::Verifying, None)?;
                s.set_state(id, DownloadState::Completed, None)?;
                Ok(())
            })
            .await?;
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let transferred = bytes.saturating_sub(stats.resumed_from);
            let avg = if elapsed.as_secs_f64() > 0.0 {
                (transferred as f64 / elapsed.as_secs_f64()) as u64
            } else {
                0
            };
            if !opts.quiet {
                eprintln!(
                    "{}",
                    tf(
                        lang,
                        "fetch.done",
                        &[
                            ("path", &path.display().to_string()),
                            ("bytes", &human_bytes(bytes)),
                            ("time", &human_duration(elapsed)),
                            ("speed", &human_speed(avg))
                        ]
                    )
                );
            }
            Ok(FetchReport {
                path,
                bytes,
                resumed_from: stats.resumed_from,
                connections: opts.connections,
                elapsed_ms: elapsed.as_millis(),
                avg_bps: avg,
                peak_bps: stats.peak_bps,
                retries: stats.retries,
            })
        }
        Ok(Outcome::Stopped { segments }) => {
            let on_disk: u64 = segments.iter().map(|s| s.written).sum();
            db(&store, move |s| {
                s.set_state(id, DownloadState::Paused, None)
            })
            .await?;
            bail!(
                "{}",
                tf(lang, "fetch.paused", &[("bytes", &human_bytes(on_disk))])
            );
        }
        Err(e) => {
            let retryable = e.is_retryable();
            let message = e.to_string();
            db(&store, move |s| {
                s.set_state(id, DownloadState::Failed { retryable }, Some(&message))
            })
            .await?;
            Err(anyhow!(e)).context(t(lang, "fetch.failed").to_string())
        }
    }
}

/// `riptide bench`: the same URL with 1, 8 and 32 connections (or the given list) into a
/// temporary folder, reporting time and speed for each.
pub async fn bench(
    url: String,
    connections: Vec<u32>,
    json: bool,
    lang: Lang,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir().context("temp dir")?;
    let mut reports = Vec::new();
    for (i, &n) in connections.iter().enumerate() {
        let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory()?));
        let opts = FetchOptions {
            url: url.clone(),
            output: Some(dir.path().join(format!("bench-{i}.bin"))),
            connections: n,
            checksum: None,
            headers: Vec::new(),
            referrer: None,
            user_agent: None,
            credentials: None,
            insecure: false,
            overwrite: true,
            quiet: true,
            no_resume: true,
        };
        if !json {
            eprintln!("{}", tf(lang, "bench.running", &[("n", &n.to_string())]));
        }
        let report = fetch(opts, store, lang).await?;
        let _ = std::fs::remove_file(&report.path);
        reports.push(report);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
        return Ok(());
    }
    println!(
        "{:>12} {:>12} {:>14} {:>14} {:>8}",
        t(lang, "bench.col.conn"),
        t(lang, "bench.col.time"),
        t(lang, "bench.col.avg"),
        t(lang, "bench.col.peak"),
        t(lang, "bench.col.retries")
    );
    for r in &reports {
        println!(
            "{:>12} {:>12} {:>14} {:>14} {:>8}",
            r.connections,
            human_duration(Duration::from_millis(r.elapsed_ms as u64)),
            human_speed(r.avg_bps),
            human_speed(r.peak_bps),
            r.retries
        );
    }
    if let (Some(first), Some(best)) = (reports.first(), reports.iter().max_by_key(|r| r.avg_bps)) {
        if first.avg_bps > 0 {
            println!(
                "{}",
                tf(
                    lang,
                    "bench.summary",
                    &[
                        ("n", &best.connections.to_string()),
                        (
                            "x",
                            &format!("{:.1}", best.avg_bps as f64 / first.avg_bps as f64)
                        )
                    ]
                )
            );
        }
    }
    Ok(())
}

#[derive(Default)]
struct Stats {
    resumed_from: u64,
    peak_bps: u64,
    retries: u32,
}

/// One line on stderr, redrawn in place; notes are printed on their own lines above it.
struct ProgressLine {
    quiet: bool,
    lang: Lang,
    dirty: bool,
    last: Instant,
}

impl ProgressLine {
    fn new(quiet: bool, lang: Lang) -> Self {
        ProgressLine {
            quiet,
            lang,
            dirty: false,
            last: Instant::now() - Duration::from_secs(1),
        }
    }

    fn render(
        &mut self,
        downloaded: u64,
        total: Option<u64>,
        bps: u64,
        eta: Option<u64>,
        connections: u32,
    ) {
        if self.quiet || self.last.elapsed() < Duration::from_millis(200) {
            return;
        }
        self.last = Instant::now();
        let bar = match total {
            Some(t) if t > 0 => {
                let frac = (downloaded as f64 / t as f64).clamp(0.0, 1.0);
                let filled = (frac * 24.0).round() as usize;
                format!(
                    "[{}{}] {:>3}%",
                    "#".repeat(filled),
                    "-".repeat(24 - filled),
                    (frac * 100.0).round() as u64
                )
            }
            _ => "[------------------------]   ?%".to_string(),
        };
        let total_text = total.map(human_bytes).unwrap_or_else(|| "?".into());
        let eta_text = eta
            .map(|s| human_duration(Duration::from_secs(s)))
            .unwrap_or_else(|| "--".into());
        let line = format!(
            "\r{bar}  {} / {}  {}  {} {}  {} {}",
            human_bytes(downloaded),
            total_text,
            human_speed(bps),
            eta_text,
            t(self.lang, "fetch.left"),
            connections,
            t(self.lang, "fetch.conn")
        );
        let mut err = std::io::stderr();
        let _ = write!(err, "{line:<100}");
        let _ = err.flush();
        self.dirty = true;
    }

    fn note(&mut self, text: &str) {
        if self.quiet {
            return;
        }
        let mut err = std::io::stderr();
        if self.dirty {
            let _ = writeln!(err);
            self.dirty = false;
        }
        let _ = writeln!(err, "{text}");
    }

    fn finish(&mut self) {
        if self.dirty {
            let _ = writeln!(std::io::stderr());
            self.dirty = false;
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `1.50 GB`, `820 MB`, `12 KB`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.2} {}", UNITS[u])
    }
}

/// `48.2 MB/s`.
pub fn human_speed(bps: u64) -> String {
    format!("{}/s", human_bytes(bps))
}

/// `1m 20s`, `2h 05m`.
pub fn human_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humans() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1536), "1.50 KB");
        assert_eq!(human_bytes(48_234_496), "46.0 MB");
        assert_eq!(human_bytes(6_227_702_579), "5.80 GB");
        assert_eq!(human_speed(1024 * 1024), "1.00 MB/s");
        assert_eq!(human_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(human_duration(Duration::from_secs(80)), "1m 20s");
        assert_eq!(human_duration(Duration::from_secs(3900)), "1h 05m");
    }
}
