//! Parsing what yt-dlp prints: the `-J` info JSON, the progress lines our
//! `--progress-template` asks for, and its `ERROR:` lines.

use riptide_core::{media_choices, MediaFormat, MediaInfo};
use serde_json::Value;

use crate::{Error, MediaProgress, Result};

/// Prefix of the progress lines (see [`crate::args::PROGRESS_TEMPLATE`]).
pub const PROGRESS_PREFIX: &str = "RIPTIDE-P ";
/// Prefix of the line naming the finished file (see [`crate::args::FILE_TEMPLATE`]).
pub const FILE_PREFIX: &str = "RIPTIDE-F ";

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn u64_field(v: &Value, key: &str) -> Option<u64> {
    let n = v.get(key)?;
    n.as_u64()
        .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
}

fn u32_field(v: &Value, key: &str) -> Option<u32> {
    u64_field(v, key).and_then(|n| u32::try_from(n).ok())
}

fn f32_field(v: &Value, key: &str) -> Option<f32> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|f| f.is_finite() && *f > 0.0)
        .map(|f| f as f32)
}

/// yt-dlp writes `"none"` for a missing stream and sometimes leaves the codec out entirely.
fn codec(v: &Value, key: &str) -> Option<String> {
    str_field(v, key).filter(|c| c != "none")
}

fn is_drm(v: &Value) -> bool {
    matches!(v.get("has_drm"), Some(Value::Bool(true)))
}

fn format_from(v: &Value) -> Option<MediaFormat> {
    let id = str_field(v, "format_id")?;
    let explicit_none = |k: &str| v.get(k).and_then(Value::as_str) == Some("none");
    if (explicit_none("vcodec") && explicit_none("acodec"))
        || str_field(v, "ext").as_deref() == Some("mhtml")
    {
        // storyboards and other rows without a playable stream
        return None;
    }
    let mut vcodec = codec(v, "vcodec");
    let mut acodec = codec(v, "acodec");
    let width = u32_field(v, "width");
    let height = u32_field(v, "height");
    // some extractors leave the codecs out but give a size (video) or `audio_ext`
    if vcodec.is_none() && acodec.is_none() {
        let video_ext = str_field(v, "video_ext").filter(|e| e != "none");
        let audio_ext = str_field(v, "audio_ext").filter(|e| e != "none");
        if video_ext.is_some() || height.is_some() {
            vcodec = Some("unknown".into());
        }
        if audio_ext.is_some() {
            acodec = Some("unknown".into());
        }
        if vcodec.is_none() && acodec.is_none() {
            // storyboards (`mhtml`) and other non-media rows
            return None;
        }
    }
    let (filesize, size_is_approx) = match u64_field(v, "filesize") {
        Some(s) => (Some(s), false),
        None => (u64_field(v, "filesize_approx"), true),
    };
    Some(MediaFormat {
        id,
        ext: str_field(v, "ext").unwrap_or_else(|| "bin".into()),
        width,
        height,
        fps: f32_field(v, "fps"),
        vcodec,
        acodec,
        tbr: f32_field(v, "tbr"),
        filesize,
        size_is_approx: filesize.is_some() && size_is_approx,
    })
}

/// Parses the JSON `yt-dlp -J --no-playlist <url>` prints.
///
/// # Errors
///
/// [`Error::Parse`] for something that is not an info object, [`Error::Playlist`] for a
/// playlist, [`Error::Live`] for a live or upcoming stream, [`Error::Drm`] when every format is
/// DRM-protected, [`Error::Unsupported`] when there is nothing downloadable.
pub fn parse_info(json: &str) -> Result<MediaInfo> {
    let v: Value = serde_json::from_str(json.trim()).map_err(|e| Error::Parse(e.to_string()))?;
    if !v.is_object() {
        return Err(Error::Parse("the info is not a JSON object".into()));
    }
    if matches!(
        str_field(&v, "_type").as_deref(),
        Some("playlist" | "multi_video")
    ) {
        return Err(Error::Playlist);
    }
    let live_status = str_field(&v, "live_status");
    let is_live = v.get("is_live").and_then(Value::as_bool).unwrap_or(false)
        || matches!(live_status.as_deref(), Some("is_live" | "is_upcoming"));
    if is_live {
        return Err(Error::Live);
    }

    // a single-format page has its format fields at the top level and no `formats`
    let raw_formats: Vec<&Value> = match v.get("formats").and_then(Value::as_array) {
        Some(list) => list.iter().collect(),
        None => vec![&v],
    };
    let drm = raw_formats.iter().any(|f| is_drm(f)) || is_drm(&v);
    let formats: Vec<MediaFormat> = raw_formats
        .into_iter()
        .filter(|f| !is_drm(f))
        .filter_map(format_from)
        .collect();
    if formats.is_empty() {
        return Err(if drm { Error::Drm } else { Error::Unsupported });
    }

    let duration = v
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite() && *d >= 0.0);
    let choices = media_choices(&formats, duration);
    Ok(MediaInfo {
        url: str_field(&v, "webpage_url")
            .or_else(|| str_field(&v, "original_url"))
            .unwrap_or_default(),
        id: str_field(&v, "id").unwrap_or_default(),
        title: str_field(&v, "title")
            .or_else(|| str_field(&v, "fulltitle"))
            .unwrap_or_else(|| "video".into()),
        uploader: str_field(&v, "uploader").or_else(|| str_field(&v, "channel")),
        extractor: str_field(&v, "extractor_key")
            .or_else(|| str_field(&v, "extractor"))
            .unwrap_or_else(|| "generic".into())
            .to_ascii_lowercase(),
        thumbnail: str_field(&v, "thumbnail").filter(|t| t.starts_with("https://")),
        duration,
        is_live,
        drm,
        formats,
        choices,
    })
}

/// Turns yt-dlp's stderr into an [`Error`]: the last `ERROR:` line decides; DRM and live
/// streams get their own variants.
pub fn classify_error(stderr: &str) -> Error {
    let line = stderr
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("ERROR:").map(str::trim))
        .or_else(|| stderr.lines().rev().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("yt-dlp failed")
        .to_string();
    let lower = line.to_ascii_lowercase();
    if lower.contains("drm") {
        return Error::Drm;
    }
    if lower.contains("live event")
        || lower.contains("is live")
        || lower.contains("premieres in")
        || lower.contains("live stream")
    {
        return Error::Live;
    }
    if lower.contains("unsupported url") {
        return Error::Unsupported;
    }
    let class = classify_message(&lower);
    let retryable = class == ErrorClass::Refused
        || [
            "http error 5",
            "http error 429",
            "timed out",
            "timeout",
            "connection",
            "getaddrinfo",
            "temporary failure",
            "network",
            "unable to download",
        ]
        .iter()
        .any(|k| lower.contains(k));
    let mut message: String = line.chars().take(300).collect();
    if message.is_empty() {
        message = "yt-dlp failed".into();
    }
    let js_runtime_missing = stderr
        .to_ascii_lowercase()
        .contains("no supported javascript runtime");
    Error::Tool {
        message,
        retryable,
        class,
        js_runtime_missing,
    }
}

/// What kind of refusal a yt-dlp error line is, for the cookie policy, the client ladder and
/// the friendly message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// The video needs a signed-in user (age gate, private, members only, bot check).
    Login,
    /// The site refused this request (stale page state, 403, PO token): a different client or
    /// a later try may work.
    Refused,
    /// Anything else.
    Other,
}

impl ErrorClass {
    /// The short tag in front of the message (see [`crate::Error`]'s `Display`), `None` for
    /// [`ErrorClass::Other`].
    pub fn tag(self) -> Option<&'static str> {
        match self {
            ErrorClass::Login => Some("login"),
            ErrorClass::Refused => Some("refused"),
            ErrorClass::Other => None,
        }
    }
}

const LOGIN_MARKERS: &[&str] = &[
    "sign in to confirm",
    "this video is private",
    "private video",
    "members-only",
    "members only",
    "join this channel",
    "login required",
    "log in to",
    "http error 401",
    "use --cookies",
];

const CLIENT_RETRY_MARKERS: &[&str] = &[
    "page needs to be reloaded",
    "sign in to confirm",
    "http error 403",
    "precondition check failed",
    "po_token",
    "po token",
];

/// The (lower-cased) error says "this needs a signed-in user".
pub(crate) fn needs_login(lower: &str) -> bool {
    LOGIN_MARKERS.iter().any(|k| lower.contains(k))
}

/// A different player client may get past this (lower-cased) error.
pub(crate) fn client_retryable(lower: &str) -> bool {
    // a video that is gone or private will not come back with another client
    if lower.contains("video unavailable") || lower.contains("private video") {
        return false;
    }
    CLIENT_RETRY_MARKERS.iter().any(|k| lower.contains(k))
}

/// The (lower-cased) error says the page state, or the cookies bound to it, is stale.
pub(crate) fn page_reload(lower: &str) -> bool {
    lower.contains("page needs to be reloaded")
}

fn classify_message(lower: &str) -> ErrorClass {
    if needs_login(lower) {
        ErrorClass::Login
    } else if client_retryable(lower) {
        ErrorClass::Refused
    } else {
        ErrorClass::Other
    }
}

/// One line of the progress template.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressLine {
    /// `downloading`, `finished`, `error`.
    pub status: String,
    /// Bytes of this stream on disk.
    pub downloaded: Option<u64>,
    /// Exact size of this stream.
    pub total: Option<u64>,
    /// yt-dlp's estimate of this stream's size (fragmented formats).
    pub total_estimate: Option<u64>,
    /// Bytes per second.
    pub speed: Option<f64>,
    /// Seconds left for this stream.
    pub eta: Option<u64>,
    /// Format id of the stream (`137`, `140`, ...).
    pub format_id: Option<String>,
}

fn num(field: &str) -> Option<f64> {
    let f = field.trim();
    if f.is_empty() || f == "NA" || f == "None" {
        return None;
    }
    f.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0)
}

/// Parses a `RIPTIDE-P status|downloaded|total|estimate|speed|eta|format_id` line; `None` for
/// any other line.
pub fn parse_progress_line(line: &str) -> Option<ProgressLine> {
    let rest = line.trim().strip_prefix(PROGRESS_PREFIX.trim_end())?.trim();
    let parts: Vec<&str> = rest.split('|').collect();
    if parts.len() < 6 {
        return None;
    }
    let as_u64 = |s: &str| num(s).map(|n| n as u64);
    Some(ProgressLine {
        status: parts[0].trim().to_string(),
        downloaded: as_u64(parts[1]),
        total: as_u64(parts[2]),
        total_estimate: as_u64(parts[3]),
        speed: num(parts[4]),
        eta: as_u64(parts[5]),
        format_id: parts
            .get(6)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && *s != "NA")
            .map(str::to_owned),
    })
}

/// Parses the `RIPTIDE-F <path>` line yt-dlp prints after moving the finished file.
pub fn parse_file_line(line: &str) -> Option<&str> {
    line.trim_end_matches(['\r', '\n'])
        .strip_prefix(FILE_PREFIX)
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != "NA")
}

/// Sums progress over the streams of one run: yt-dlp fetches the video stream, then the audio
/// stream, each starting again at zero.
#[derive(Debug, Clone, Default)]
pub struct ProgressTracker {
    expected_total: Option<u64>,
    /// (format id, downloaded, size) per stream, in the order they started.
    streams: Vec<(String, u64, Option<u64>)>,
}

impl ProgressTracker {
    /// A tracker; `expected_total` is the probe's estimate for the chosen quality.
    pub fn new(expected_total: Option<u64>) -> Self {
        ProgressTracker {
            expected_total: expected_total.filter(|t| *t > 0),
            streams: Vec::new(),
        }
    }

    /// Folds one line in and returns the overall progress.
    pub fn update(&mut self, line: &ProgressLine) -> MediaProgress {
        let key = line.format_id.clone().unwrap_or_default();
        let size = line.total.or(line.total_estimate);
        let downloaded = line.downloaded.unwrap_or(0);
        match self.streams.iter_mut().find(|(k, ..)| *k == key) {
            Some(entry) => {
                entry.1 = downloaded;
                if size.is_some() {
                    entry.2 = size;
                }
            }
            None => self.streams.push((key, downloaded, size)),
        }
        let done: u64 = self.streams.iter().map(|(_, d, _)| *d).sum();
        let known: u64 = self
            .streams
            .iter()
            .map(|(_, d, s)| s.unwrap_or(*d).max(*d))
            .sum();
        let total = match self.expected_total {
            Some(e) => Some(e.max(known)),
            None if known > 0 => Some(known),
            None => None,
        };
        let speed = line.speed.map(|s| s as u64).unwrap_or(0);
        let eta = match (total, speed) {
            (Some(t), s) if s > 0 => Some(t.saturating_sub(done) / s),
            _ => line.eta,
        };
        MediaProgress {
            downloaded: done,
            total,
            speed_bps: speed,
            eta_secs: eta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOUTUBE: &str = include_str!("../tests/fixtures/youtube.json");
    const DRM: &str = include_str!("../tests/fixtures/drm.json");
    const LIVE: &str = include_str!("../tests/fixtures/live.json");

    #[test]
    fn youtube_info_parses_into_formats_and_choices() {
        let info = parse_info(YOUTUBE).unwrap();
        assert_eq!(info.id, "aqz-KE-bpKQ");
        assert_eq!(
            info.title,
            "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film"
        );
        assert_eq!(info.extractor, "youtube");
        assert_eq!(info.duration, Some(635.0));
        assert!(!info.is_live && !info.drm);
        assert!(info
            .thumbnail
            .as_deref()
            .unwrap()
            .starts_with("https://i.ytimg.com/"));
        // the storyboard row is gone, every other row is kept
        assert!(info.formats.iter().all(|f| f.ext != "mhtml"));
        let f137 = info.formats.iter().find(|f| f.id == "137").unwrap();
        assert_eq!((f137.width, f137.height), (Some(1920), Some(1080)));
        assert_eq!(f137.fps, Some(60.0));
        assert!(f137.has_video() && !f137.has_audio());
        let f251 = info.formats.iter().find(|f| f.id == "251").unwrap();
        assert!(f251.size_is_approx);
        let keys: Vec<&str> = info.choices.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "best",
                "h2160",
                "h1080",
                "h720",
                "h360",
                "audio-m4a",
                "audio-mp3"
            ]
        );
        let h720 = info.choices.iter().find(|c| c.key == "h720").unwrap();
        // 136 (mp4 720p60) + 140 (m4a)
        assert_eq!(h720.approx_size, Some(98_765_432 + 10_284_929));
    }

    #[test]
    fn drm_only_pages_are_refused_and_mixed_pages_drop_drm_formats() {
        assert!(matches!(parse_info(DRM), Err(Error::Drm)));
        let mut v: Value = serde_json::from_str(YOUTUBE).unwrap();
        for f in v["formats"].as_array_mut().unwrap() {
            if f["format_id"] == "137" {
                f["has_drm"] = Value::Bool(true);
            }
        }
        let info = parse_info(&v.to_string()).unwrap();
        assert!(info.drm);
        assert!(info.formats.iter().all(|f| f.id != "137"));
    }

    #[test]
    fn live_and_playlists_are_refused() {
        assert!(matches!(parse_info(LIVE), Err(Error::Live)));
        let upcoming = r#"{"id":"x","title":"t","live_status":"is_upcoming","formats":[]}"#;
        assert!(matches!(parse_info(upcoming), Err(Error::Live)));
        let playlist = r#"{"_type":"playlist","id":"PL1","entries":[]}"#;
        assert!(matches!(parse_info(playlist), Err(Error::Playlist)));
        assert!(matches!(parse_info("not json"), Err(Error::Parse(_))));
        assert!(matches!(parse_info("[1]"), Err(Error::Parse(_))));
    }

    #[test]
    fn single_format_pages_use_the_top_level_fields() {
        let json = r#"{"id":"v1","title":"Clip","extractor_key":"Generic","webpage_url":"https://e.test/v",
            "format_id":"mp4","ext":"mp4","width":640,"height":360,"vcodec":"avc1","acodec":"mp4a","filesize":1234}"#;
        let info = parse_info(json).unwrap();
        assert_eq!(info.extractor, "generic");
        assert_eq!(info.formats.len(), 1);
        assert_eq!(info.choices[0].approx_size, Some(1234));
        assert_eq!(info.thumbnail, None);
    }

    #[test]
    fn errors_are_classified() {
        let drm = "WARNING: x\nERROR: [youtube] abc: This video is DRM protected\n";
        assert!(matches!(classify_error(drm), Error::Drm));
        let live = "ERROR: [youtube] abc: This live event will begin in 3 hours.";
        assert!(matches!(classify_error(live), Error::Live));
        assert!(matches!(
            classify_error("ERROR: Unsupported URL: https://example.com/"),
            Error::Unsupported
        ));
        let e = classify_error("ERROR: [youtube] abc: Video unavailable");
        assert!(
            matches!(&e, Error::Tool { message, retryable: false, .. } if message == "[youtube] abc: Video unavailable")
        );
        let e = classify_error(
            "ERROR: unable to download video data: HTTP Error 503: Service Unavailable",
        );
        assert!(e.is_retryable());
        assert!(matches!(classify_error(""), Error::Tool { .. }));
    }

    #[test]
    fn refusals_are_classified_for_the_ladder_and_the_cookie_policy() {
        let class = |s: &str| match classify_error(s) {
            Error::Tool {
                class, retryable, ..
            } => (class, retryable),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            class("ERROR: [youtube] x: The page needs to be reloaded."),
            (ErrorClass::Refused, true)
        );
        assert_eq!(
            class(
                "ERROR: [youtube] x: Sign in to confirm your age. This video may be inappropriate"
            ),
            (ErrorClass::Login, false)
        );
        assert_eq!(
            class("ERROR: [youtube] x: Sign in to confirm you're not a bot."),
            (ErrorClass::Login, false)
        );
        assert_eq!(
            class("ERROR: [youtube] x: Private video. Sign in if you've been granted access"),
            (ErrorClass::Login, false)
        );
        assert_eq!(
            class("ERROR: unable to download video data: HTTP Error 403: Forbidden"),
            (ErrorClass::Refused, true)
        );
        assert_eq!(
            class("ERROR: [youtube] x: Video unavailable. This content isn't available"),
            (ErrorClass::Other, false)
        );
        assert!(!client_retryable(
            "private video. sign in if you've been granted access"
        ));
        assert!(!client_retryable("video unavailable. http error 403"));
        // the JS-runtime warning is remembered for the Details text
        let e = classify_error(
            "WARNING: [youtube] No supported JavaScript runtime could be found.
ERROR: [youtube] x: The page needs to be reloaded.",
        );
        assert!(matches!(
            e,
            Error::Tool {
                js_runtime_missing: true,
                ..
            }
        ));
        assert!(e.to_string().starts_with("[rt:refused] "), "{e}");
        assert!(e.to_string().contains("JavaScript runtime"), "{e}");
        // without the warning, no hint
        let e = classify_error("ERROR: [youtube] x: The page needs to be reloaded.");
        assert!(!e.to_string().contains("JavaScript"), "{e}");
    }

    #[test]
    fn progress_lines_parse_with_missing_fields() {
        let l =
            parse_progress_line("RIPTIDE-P downloading|1048576|4194304|NA|524288.5|6|137").unwrap();
        assert_eq!(l.status, "downloading");
        assert_eq!(l.downloaded, Some(1_048_576));
        assert_eq!(l.total, Some(4_194_304));
        assert_eq!(l.total_estimate, None);
        assert_eq!(l.speed, Some(524_288.5));
        assert_eq!(l.eta, Some(6));
        assert_eq!(l.format_id.as_deref(), Some("137"));
        let l = parse_progress_line("RIPTIDE-P downloading|10.0|NA|2000.7|NA|NA|NA").unwrap();
        assert_eq!(
            (l.downloaded, l.total_estimate, l.speed, l.format_id),
            (Some(10), Some(2000), None, None)
        );
        assert!(parse_progress_line("[download]  12.5% of 10MiB").is_none());
        assert!(parse_progress_line("RIPTIDE-P a|b").is_none());
        assert_eq!(
            parse_file_line("RIPTIDE-F C:\\dl\\x.mp4\r\n"),
            Some("C:\\dl\\x.mp4")
        );
        assert_eq!(parse_file_line("RIPTIDE-F NA"), None);
    }

    #[test]
    fn tracker_sums_video_then_audio() {
        let mut t = ProgressTracker::new(Some(1_000));
        let line = |d: u64, total: u64, id: &str| ProgressLine {
            status: "downloading".into(),
            downloaded: Some(d),
            total: Some(total),
            total_estimate: None,
            speed: Some(100.0),
            eta: None,
            format_id: Some(id.into()),
        };
        let p = t.update(&line(400, 800, "137"));
        assert_eq!(
            (p.downloaded, p.total, p.eta_secs),
            (400, Some(1_000), Some(6))
        );
        t.update(&line(800, 800, "137"));
        let p = t.update(&line(150, 300, "140"));
        assert_eq!(p.downloaded, 950);
        assert_eq!(
            p.total,
            Some(1_100),
            "the real sizes beat a smaller estimate"
        );
        let mut no_estimate = ProgressTracker::new(None);
        let p = no_estimate.update(&line(5, 10, "18"));
        assert_eq!(p.total, Some(10));
    }
}
