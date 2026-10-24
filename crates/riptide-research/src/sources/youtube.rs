//! Video search with the bundled yt-dlp: `yt-dlp --flat-playlist -J --no-warnings
//! "ytsearchN:<query>"`, no API key and nothing downloaded. Each result is a *page* hit (the watch
//! URL); adding it goes through the normal media download flow, which asks yt-dlp for the
//! formats at that point.
//!
//! The process is behind [`VideoSearchRunner`] so tests feed recorded output. The real runner
//! ([`ProcessRunner`]) gives yt-dlp a deadline and caps how much output it reads; the query is a
//! single argument, never a shell string.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncReadExt;

use super::{check_query, clamp, SearchSource};
use crate::error::{ResearchError, Result};
use crate::types::{is_web_url, jstr, tidy, Category, Hit, HitKind, Source};

/// Most results asked of yt-dlp.
const MAX_RESULTS: usize = 25;
/// Longest a yt-dlp search may run.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(40);
/// Most bytes of yt-dlp output that are read (a flat search of 25 entries is far smaller).
pub const OUTPUT_CAP: usize = 4 * 1024 * 1024;

/// Runs one yt-dlp search and returns its standard output (the JSON document).
#[async_trait]
pub trait VideoSearchRunner: Send + Sync {
    /// Searches for `query`, asking for `count` results.
    async fn run(&self, count: usize, query: &str) -> Result<String>;
}

/// The real thing: spawns the yt-dlp executable.
#[derive(Debug, Clone)]
pub struct ProcessRunner {
    exe: PathBuf,
    timeout: Duration,
}

impl ProcessRunner {
    /// A runner for the yt-dlp at `exe` with the default deadline.
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        ProcessRunner {
            exe: exe.into(),
            timeout: RUN_TIMEOUT,
        }
    }

    /// Uses another deadline (tests).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The command line (kept separate so a test can check it): one argument each.
    pub(crate) fn args(count: usize, query: &str) -> Vec<String> {
        vec![
            "--flat-playlist".into(),
            "-J".into(),
            "--no-warnings".into(),
            "--no-cache-dir".into(),
            format!("ytsearch{count}:{query}"),
        ]
    }
}

#[async_trait]
impl VideoSearchRunner for ProcessRunner {
    async fn run(&self, count: usize, query: &str) -> Result<String> {
        if !self.exe.is_file() {
            return Err(ResearchError::Invalid("yt-dlp is not installed".into()));
        }
        let mut cmd = tokio::process::Command::new(&self.exe);
        cmd.args(Self::args(count, query))
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console window flashes up
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| ResearchError::Invalid(format!("could not start yt-dlp: {}", e.kind())))?;
        let Some(stdout) = child.stdout.take() else {
            return Err(ResearchError::Parse("no output pipe".into()));
        };
        let work = async {
            let mut buf = Vec::new();
            // one byte over the cap tells "too big" from "exactly at the cap"
            stdout
                .take(OUTPUT_CAP as u64 + 1)
                .read_to_end(&mut buf)
                .await
                .map_err(|e| ResearchError::Network(format!("read failed: {}", e.kind())))?;
            if buf.len() > OUTPUT_CAP {
                return Err(ResearchError::Parse("yt-dlp output is too large".into()));
            }
            let status = child
                .wait()
                .await
                .map_err(|e| ResearchError::Network(format!("wait failed: {}", e.kind())))?;
            if !status.success() {
                return Err(ResearchError::Network("yt-dlp could not search".into()));
            }
            Ok(String::from_utf8_lossy(&buf).into_owned())
        };
        tokio::time::timeout(self.timeout, work)
            .await
            .unwrap_or(Err(ResearchError::Timeout))
    }
}

/// The video search source.
pub struct Youtube {
    runner: Box<dyn VideoSearchRunner>,
}

impl Youtube {
    /// A source that searches through `runner`.
    pub fn new(runner: Box<dyn VideoSearchRunner>) -> Self {
        Youtube { runner }
    }
}

fn is_video_id(id: &str) -> bool {
    (6..=20).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_youtube_host(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| {
            ["youtube.com", "youtu.be", "youtube-nocookie.com"]
                .iter()
                .any(|d| h == *d || h.ends_with(&format!(".{d}")))
        })
}

/// The watch page of an entry: its own URL when that is a YouTube link, else built from the id.
fn watch_url(e: &Value, id: &str) -> Option<String> {
    for key in ["webpage_url", "url"] {
        if let Some(u) = e.get(key).and_then(jstr) {
            if is_web_url(u) && is_youtube_host(u) {
                return Some(u.to_string());
            }
        }
    }
    is_video_id(id).then(|| format!("https://www.youtube.com/watch?v={id}"))
}

/// The widest thumbnail with a web address.
fn thumbnail(e: &Value) -> Option<String> {
    e.get("thumbnails")?
        .as_array()?
        .iter()
        .filter_map(|t| {
            let u = jstr(t.get("url")?).filter(|u| is_web_url(u))?;
            Some((t.get("width").and_then(Value::as_u64).unwrap_or(0), u))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, u)| u.to_string())
}

/// Turns yt-dlp's flat-playlist JSON into hits. Entries without a title or a usable watch URL,
/// and live streams (the media pipeline cannot record those), are dropped.
pub(crate) fn parse_flat_search(json: &str, limit: usize) -> Result<Vec<Hit>> {
    let v: Value = serde_json::from_str(json)
        .map_err(|e| ResearchError::Parse(format!("yt-dlp JSON: {e}")))?;
    let entries = v
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| ResearchError::Parse("no entries".into()))?;
    let mut hits = Vec::new();
    for e in entries {
        if hits.len() >= limit {
            break;
        }
        let live = e
            .get("live_status")
            .and_then(jstr)
            .is_some_and(|s| s == "is_live" || s == "is_upcoming");
        let (Some(id), Some(title)) = (
            e.get("id").and_then(jstr),
            e.get("title").and_then(jstr).map(|t| tidy(t, 300)),
        ) else {
            continue;
        };
        if live || title.is_empty() {
            continue;
        }
        let Some(url) = watch_url(e, id) else {
            continue;
        };
        let mut h = Hit::new(Source::Youtube, id, &title, &url);
        h.kind = HitKind::Page;
        h.page_url = Some(url);
        h.filename = Some(title);
        h.publisher = ["channel", "uploader"]
            .iter()
            .find_map(|k| e.get(*k).and_then(jstr))
            .map(|c| tidy(c, 120));
        h.authors = h.publisher.iter().cloned().collect();
        h.duration_secs = e
            .get("duration")
            .and_then(Value::as_f64)
            .filter(|d| d.is_finite() && *d >= 1.0)
            .map(|d| d as u64);
        h.thumbnail = thumbnail(e);
        hits.push(h);
    }
    Ok(hits)
}

#[async_trait]
impl SearchSource for Youtube {
    fn source(&self) -> Source {
        Source::Youtube
    }

    fn max_queries(&self) -> usize {
        2
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        if !matches!(category, Category::All | Category::Video) {
            return Ok(Vec::new());
        }
        let q = check_query(query)?;
        let wanted = clamp(limit).min(MAX_RESULTS);
        // ask for a few more than needed: live streams and unusable entries are dropped
        let json = self.runner.run((wanted + 3).min(MAX_RESULTS), &q).await?;
        parse_flat_search(&json, wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/youtube_search.json");

    struct Fixed(Result<String>);

    #[async_trait]
    impl VideoSearchRunner for Fixed {
        async fn run(&self, _count: usize, _query: &str) -> Result<String> {
            self.0.clone()
        }
    }

    #[test]
    fn flat_search_json_becomes_page_hits_with_only_what_yt_dlp_said() {
        let hits = parse_flat_search(FIXTURE, 10).unwrap();
        let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        // the live stream, the javascript: link and the nameless entry are gone
        assert_eq!(
            ids,
            [
                "youtube:dQw4w9WgXcQ",
                "youtube:abcdefghijk",
                "youtube:noDurat_1-2"
            ]
        );
        let first = &hits[0];
        assert_eq!(first.kind, HitKind::Page);
        assert_eq!(first.source, Source::Youtube);
        assert_eq!(first.duration_secs, Some(1421));
        assert_eq!(first.publisher.as_deref(), Some("Anime Vault"));
        assert_eq!(
            first.thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq720.jpg")
        );
        assert_eq!(
            first.download_url,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        // nothing is invented for fields the entry lacks
        let bare = &hits[2];
        assert_eq!(bare.duration_secs, None);
        assert_eq!(bare.thumbnail, None);
        assert_eq!(bare.size, None);
        assert_eq!(bare.year, None);
        assert_eq!(bare.publisher.as_deref(), Some("Reviewer Rex"));
        assert_eq!(
            bare.download_url,
            "https://www.youtube.com/watch?v=noDurat_1-2"
        );
    }

    #[test]
    fn the_limit_and_bad_json_are_handled() {
        assert_eq!(parse_flat_search(FIXTURE, 1).unwrap().len(), 1);
        assert!(matches!(
            parse_flat_search("not json", 5),
            Err(ResearchError::Parse(_))
        ));
        assert!(matches!(
            parse_flat_search("{}", 5),
            Err(ResearchError::Parse(_))
        ));
        assert!(parse_flat_search(r#"{"entries":[]}"#, 5)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_command_line_keeps_the_query_in_one_argument() {
        let args = ProcessRunner::args(8, "a b; rm -rf / \"x\"");
        assert_eq!(args.last().unwrap(), "ytsearch8:a b; rm -rf / \"x\"");
        assert_eq!(args.len(), 5);
        assert!(args.contains(&"--flat-playlist".to_string()));
    }

    #[tokio::test]
    async fn the_source_uses_the_runner_and_only_serves_video() {
        let y = Youtube::new(Box::new(Fixed(Ok(FIXTURE.to_string()))));
        let hits = y.search_in(Category::Video, "naruto", 5).await.unwrap();
        assert_eq!(hits.len(), 3);
        assert!(y
            .search_in(Category::Documents, "naruto", 5)
            .await
            .unwrap()
            .is_empty());
        let down = Youtube::new(Box::new(Fixed(Err(ResearchError::Invalid(
            "yt-dlp is not installed".into(),
        )))));
        assert_eq!(
            down.search_in(Category::Video, "x", 5)
                .await
                .unwrap_err()
                .code(),
            "invalid"
        );
    }

    #[tokio::test]
    async fn a_missing_executable_fails_without_spawning_anything() {
        let r = ProcessRunner::new("Z:/definitely/not/here/yt-dlp.exe");
        let e = r.run(5, "x").await.unwrap_err();
        assert_eq!(e.code(), "invalid");
    }
}
