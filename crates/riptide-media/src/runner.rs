//! Running yt-dlp: probe, download with progress, stop, version and self-update.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use riptide_core::{MediaInfo, MediaSelection};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::args::{check_url, download_args, probe_args, RunArgs, COMMON};
use crate::cookies::CookieFile;
use crate::parse::{
    classify_error, parse_file_line, parse_info, parse_progress_line, ProgressTracker,
};
use crate::{Error, MediaProgress, Result};

/// How long a probe may take (YouTube answers in a few seconds; slow sites take longer).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(120);
/// How long `--version` may take.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `-U` may take.
pub const UPDATE_TIMEOUT: Duration = Duration::from_secs(300);
/// Progress is forwarded at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
/// Bytes of stderr kept for the error message.
const STDERR_KEEP: usize = 16 * 1024;

/// The bundled tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtDlp {
    exe: PathBuf,
    ffmpeg: Option<PathBuf>,
}

/// One media download (a run of yt-dlp; run it again to resume).
#[derive(Clone, PartialEq, Eq)]
pub struct DownloadJob {
    /// Page URL.
    pub url: String,
    /// Quality.
    pub selection: MediaSelection,
    /// Destination folder.
    pub dir: PathBuf,
    /// File name without extension, already sanitised (`riptide_core::sanitize_filename`).
    pub stem: String,
    /// `Cookie` header captured by the browser for the page. Never logged.
    pub cookies: Option<String>,
    /// Speed cap in bytes per second.
    pub rate_limit: Option<u64>,
    /// Proxy URL (`http://`, `socks5://`).
    pub proxy: Option<String>,
    /// The probe's size estimate, so progress has a total from the first line.
    pub expected_total: Option<u64>,
}

impl std::fmt::Debug for DownloadJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadJob")
            .field("selection", &self.selection)
            .field("dir", &self.dir)
            .field("stem", &self.stem)
            .field("cookies", &self.cookies.as_ref().map(|_| "[REDACTED]"))
            .finish_non_exhaustive()
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaOutcome {
    /// The finished, merged file.
    Completed {
        /// Where it is.
        path: PathBuf,
        /// Its size.
        size: u64,
    },
    /// Stopped on request; the partial files stay for the next run.
    Stopped,
}

impl YtDlp {
    /// Tools at these paths (`ffmpeg` is the executable itself, passed as `--ffmpeg-location`).
    pub fn new(exe: impl Into<PathBuf>, ffmpeg: Option<PathBuf>) -> YtDlp {
        YtDlp {
            exe: exe.into(),
            ffmpeg,
        }
    }

    /// The yt-dlp executable.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// The ffmpeg executable, if one is bundled.
    pub fn ffmpeg(&self) -> Option<&Path> {
        self.ffmpeg.as_deref()
    }

    /// Whether the yt-dlp executable exists.
    pub fn is_available(&self) -> bool {
        self.exe.is_file()
    }

    fn command(&self) -> Result<Command> {
        if !self.is_available() {
            return Err(Error::ToolMissing(self.exe.clone()));
        }
        let mut cmd = Command::new(&self.exe);
        cmd.env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console window flashes up
            cmd.creation_flags(0x0800_0000);
        }
        Ok(cmd)
    }

    async fn output(
        &self,
        args: &[std::ffi::OsString],
        limit: Duration,
    ) -> Result<(bool, String, String)> {
        let mut cmd = self.command()?;
        cmd.args(args);
        let child = cmd.spawn()?;
        let out = tokio::time::timeout(limit, child.wait_with_output())
            .await
            .map_err(|_| Error::Timeout)??;
        Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }

    /// `yt-dlp --version`.
    pub async fn version(&self) -> Result<String> {
        let (ok, out, err) = self.output(&["--version".into()], VERSION_TIMEOUT).await?;
        let v = out.trim().to_string();
        if !ok || v.is_empty() {
            return Err(classify_error(&err));
        }
        Ok(v)
    }

    /// `yt-dlp -U`: yt-dlp replaces its own executable with the latest release from GitHub.
    /// Returns the version afterwards. Only ever run on a copy the app owns (its data folder),
    /// and only when the user asks.
    pub async fn update(&self) -> Result<String> {
        let mut args: Vec<std::ffi::OsString> = COMMON.iter().map(Into::into).collect();
        args.push("-U".into());
        let (ok, _out, err) = self.output(&args, UPDATE_TIMEOUT).await?;
        if !ok {
            return Err(classify_error(&err));
        }
        self.version().await
    }

    /// Probes a page with `yt-dlp -J --no-playlist`. `cookies` is the browser's `Cookie`
    /// header for the page, if any.
    pub async fn probe(&self, url: &str, cookies: Option<&str>) -> Result<MediaInfo> {
        let url = check_url(url)?;
        let jar = match cookies.filter(|c| !c.trim().is_empty()) {
            Some(c) => Some(CookieFile::write(&url, c)?),
            None => None,
        };
        let args = probe_args(&url, jar.as_ref().map(CookieFile::path));
        let (ok, out, err) = self.output(&args, PROBE_TIMEOUT).await?;
        drop(jar);
        if !ok {
            return Err(classify_error(&err));
        }
        parse_info(&out)
    }

    /// Runs one download until it finishes, fails or `stop` fires. Progress goes to
    /// `progress` (at most ten updates a second, but the final figure is always delivered); a
    /// full channel drops updates rather than slowing yt-dlp down.
    pub async fn download(
        &self,
        job: &DownloadJob,
        progress: mpsc::Sender<MediaProgress>,
        stop: CancellationToken,
    ) -> Result<MediaOutcome> {
        let url = check_url(&job.url)?;
        if job.stem.is_empty() || job.stem.contains(['/', '\\']) {
            return Err(Error::Parse("invalid output name".into()));
        }
        std::fs::create_dir_all(&job.dir)?;
        let jar = match job.cookies.as_deref().filter(|c| !c.trim().is_empty()) {
            Some(c) => Some(CookieFile::write(&url, c)?),
            None => None,
        };
        let args = download_args(&RunArgs {
            url: &url,
            selection: job.selection,
            dir: &job.dir,
            stem: &job.stem,
            ffmpeg: self.ffmpeg.as_deref(),
            cookies: jar.as_ref().map(CookieFile::path),
            rate_limit: job.rate_limit,
            proxy: job.proxy.as_deref(),
        });
        let mut cmd = self.command()?;
        cmd.args(&args);
        let mut child = cmd.spawn()?;
        tracing::info!(selection = %job.selection.key(), "yt-dlp started");

        let stderr = child.stderr.take();
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut s) = stderr {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = s.read(&mut chunk).await {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.len() > 2 * STDERR_KEEP {
                        buf.drain(..buf.len() - STDERR_KEEP);
                    }
                }
            }
            String::from_utf8_lossy(&buf).into_owned()
        });

        let Some(stdout) = child.stdout.take() else {
            return Err(Error::Parse("no stdout".into()));
        };
        let mut lines = BufReader::new(stdout);
        let mut tracker = ProgressTracker::new(job.expected_total);
        let mut last_sent: Option<Instant> = None;
        // the newest update the throttle held back; flushed when yt-dlp's output ends so the
        // last figure (the one that reaches the total) is never lost
        let mut held: Option<MediaProgress> = None;
        let mut final_file: Option<PathBuf> = None;
        let mut raw = Vec::new();
        loop {
            raw.clear();
            tokio::select! {
                () = stop.cancelled() => {
                    kill_tree(&mut child).await;
                    let _ = stderr_task.await;
                    drop(jar);
                    tracing::info!("yt-dlp stopped");
                    return Ok(MediaOutcome::Stopped);
                }
                read = lines.read_until(b'\n', &mut raw) => {
                    let n = read?;
                    if n == 0 {
                        break;
                    }
                    let line = String::from_utf8_lossy(&raw);
                    if let Some(p) = parse_progress_line(&line) {
                        let overall = tracker.update(&p);
                        let due = last_sent.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
                        if due {
                            last_sent = Some(Instant::now());
                            held = None;
                            let _ = progress.try_send(overall);
                        } else {
                            held = Some(overall);
                        }
                    } else if let Some(path) = parse_file_line(&line) {
                        final_file = Some(PathBuf::from(path));
                    }
                }
            }
        }
        if let Some(last) = held {
            // bounded wait: a full channel must not park the download forever
            let _ = tokio::time::timeout(Duration::from_secs(1), progress.send(last)).await;
        }
        let status = tokio::select! {
            () = stop.cancelled() => {
                kill_tree(&mut child).await;
                let _ = stderr_task.await;
                return Ok(MediaOutcome::Stopped);
            }
            s = child.wait() => s?,
        };
        let err = stderr_task.await.unwrap_or_default();
        drop(jar);
        if !status.success() {
            return Err(classify_error(&err));
        }
        let expected = job
            .dir
            .join(format!("{}.{}", job.stem, job.selection.final_ext()));
        // trust the printed path only inside the destination folder
        let path = final_file
            .filter(|p| p.parent() == Some(job.dir.as_path()) && p.is_file())
            .unwrap_or(expected);
        let size = std::fs::metadata(&path)
            .map_err(|_| Error::Parse(format!("yt-dlp finished without {}", file_name(&path))))?
            .len();
        Ok(MediaOutcome::Completed { path, size })
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Stops yt-dlp and whatever it started (ffmpeg merges and HLS downloads run as children).
async fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let mut taskkill = Command::new("taskkill");
        taskkill
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        let _ = taskkill.status().await;
    }
    let _ = child.kill().await;
}

/// `true` for a file yt-dlp leaves behind while working on `<stem>.<ext>`: `.part` and `.ytdl`
/// files, fragments, per-format intermediates (`<stem>.f137.mp4`) and merge temporaries. The
/// finished file itself (`<stem>.mp4`) is not one.
pub fn is_partial_name(name: &str, stem: &str) -> bool {
    let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) else {
        return false;
    };
    if rest.ends_with(".part") || rest.ends_with(".ytdl") || rest.contains(".part-Frag") {
        return true;
    }
    if rest.starts_with("temp.") || rest.contains(".temp.") {
        return true;
    }
    // <stem>.f137.mp4, <stem>.f399-1.webm, <stem>.fhls-720p.mp4
    match rest.split_once('.') {
        Some((first, ext)) => {
            let id = first.strip_prefix('f').unwrap_or_default();
            !ext.is_empty()
                && !id.is_empty()
                && (id.starts_with(|c: char| c.is_ascii_digit()) || id.contains('-'))
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
        None => false,
    }
}

/// Every partial file of `<stem>` in `dir` (see [`is_partial_name`]).
pub fn partial_files(dir: &Path, stem: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| is_partial_name(&e.file_name().to_string_lossy(), stem))
        .map(|e| e.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_names_are_recognised_and_the_final_file_is_not() {
        let stem = "Big Buck Bunny";
        for p in [
            "Big Buck Bunny.f137.mp4.part",
            "Big Buck Bunny.f137.mp4",
            "Big Buck Bunny.f140.m4a",
            "Big Buck Bunny.f399-1.webm",
            "Big Buck Bunny.fhls-720p.mp4",
            "Big Buck Bunny.mp4.ytdl",
            "Big Buck Bunny.temp.mp4",
            "Big Buck Bunny.mp4.part-Frag12",
            "Big Buck Bunny.mp4.part",
        ] {
            assert!(is_partial_name(p, stem), "{p}");
        }
        for f in [
            "Big Buck Bunny.mp4",
            "Big Buck Bunny.m4a",
            "Big Buck Bunny (1).f137.mp4",
            "Other.f137.mp4",
            "Big Buck Bunny.final.mp4",
            "Big Buck Bunny",
        ] {
            assert!(!is_partial_name(f, stem), "{f}");
        }
    }

    #[test]
    fn missing_tool_is_reported() {
        let y = YtDlp::new("Z:/definitely/not/here/yt-dlp.exe", None);
        assert!(!y.is_available());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(matches!(
            rt.block_on(y.version()),
            Err(Error::ToolMissing(_))
        ));
    }
}
