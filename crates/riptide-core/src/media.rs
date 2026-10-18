//! Media pages (YouTube, Vimeo, ...): what a probe of a video page found and which qualities the
//! user can pick. Pure values and functions; `riptide-media` fills them from `yt-dlp -J` and
//! runs the download.
//!
//! The quality the user picked is stored on the record as a short, validated key
//! ([`MediaSelection::key`]: `best`, `h720`, `audio-m4a`, `audio-mp3`), never as a raw yt-dlp
//! selector, so nothing a browser sends ends up as a free-form argument.

use serde::{Deserialize, Serialize};

/// One format a media page offers (a row of `yt-dlp -F`).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MediaFormat {
    /// yt-dlp format id (`137`, `140`, `hls-720p`, ...).
    pub id: String,
    /// Container extension (`mp4`, `webm`, `m4a`).
    pub ext: String,
    /// Width in pixels (video only).
    pub width: Option<u32>,
    /// Height in pixels (video only).
    pub height: Option<u32>,
    /// Frames per second (video only).
    pub fps: Option<f32>,
    /// Video codec, `None` for audio-only formats.
    pub vcodec: Option<String>,
    /// Audio codec, `None` for video-only formats.
    pub acodec: Option<String>,
    /// Total bitrate in kbit/s, if known.
    pub tbr: Option<f32>,
    /// Size in bytes (exact or approximate, see `size_is_approx`).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub filesize: Option<u64>,
    /// `true` when `filesize` is yt-dlp's estimate.
    pub size_is_approx: bool,
}

impl MediaFormat {
    /// Has a video stream.
    pub fn has_video(&self) -> bool {
        self.vcodec.is_some()
    }

    /// Has an audio stream.
    pub fn has_audio(&self) -> bool {
        self.acodec.is_some()
    }

    /// The resolution yt-dlp's `res` sort uses: the smaller side (so a vertical 1080x1920
    /// short is "1080p", like a landscape 1920x1080 video).
    pub fn resolution(&self) -> Option<u32> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => Some(w.min(h)),
            (_, h) => h,
        }
    }
}

/// What the user can pick in the quality menu.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaChoiceKind {
    /// Best video + best audio, merged.
    Best,
    /// The best video no larger than `height`, + audio, merged into MP4.
    Video,
    /// Audio only.
    Audio,
}

/// One entry of the quality menu.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaChoice {
    /// The [`MediaSelection::key`] to send back with `add`.
    pub key: String,
    /// Best, a resolution, or audio only.
    pub kind: MediaChoiceKind,
    /// Resolution in lines for [`MediaChoiceKind::Video`] (and the best one for `Best`).
    pub height: Option<u32>,
    /// Extension of the finished file (`mp4`, `m4a`, `mp3`).
    pub ext: String,
    /// Approximate size of the finished file in bytes.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub approx_size: Option<u64>,
}

/// What a probe of a media page found.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    /// Canonical page URL (`webpage_url`).
    pub url: String,
    /// yt-dlp's id of the video.
    pub id: String,
    /// Title.
    pub title: String,
    /// Channel / uploader, if known.
    pub uploader: Option<String>,
    /// Site extractor (`youtube`, `vimeo`, `generic`, ...).
    pub extractor: String,
    /// Thumbnail URL.
    pub thumbnail: Option<String>,
    /// Duration in seconds.
    pub duration: Option<f64>,
    /// A live (or upcoming) stream; such pages are refused.
    pub is_live: bool,
    /// Some formats are DRM-protected (they are left out of `formats`).
    pub drm: bool,
    /// Formats without DRM, as yt-dlp lists them (worst first).
    pub formats: Vec<MediaFormat>,
    /// The quality menu, best first.
    pub choices: Vec<MediaChoice>,
}

/// The quality a media download was added with (the record's `media_format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaSelection {
    /// Best video + audio.
    Best,
    /// Best video no larger than this many lines (smaller side), + audio.
    MaxHeight(u32),
    /// Best audio as M4A (AAC, usually no re-encode).
    AudioM4a,
    /// Best audio converted to MP3.
    AudioMp3,
}

/// Largest resolution a `h<N>` key may name.
pub const MAX_MEDIA_HEIGHT: u32 = 8640;

impl MediaSelection {
    /// The stable key (`best`, `h720`, `audio-m4a`, `audio-mp3`).
    pub fn key(self) -> String {
        match self {
            MediaSelection::Best => "best".into(),
            MediaSelection::MaxHeight(h) => format!("h{h}"),
            MediaSelection::AudioM4a => "audio-m4a".into(),
            MediaSelection::AudioMp3 => "audio-mp3".into(),
        }
    }

    /// Inverse of [`Self::key`]; anything else (including raw yt-dlp selectors) is `None`.
    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "best" => Some(MediaSelection::Best),
            "audio-m4a" => Some(MediaSelection::AudioM4a),
            "audio-mp3" => Some(MediaSelection::AudioMp3),
            _ => {
                let digits = key.strip_prefix('h')?;
                if digits.is_empty()
                    || digits.len() > 4
                    || !digits.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                let h: u32 = digits.parse().ok()?;
                (1..=MAX_MEDIA_HEIGHT)
                    .contains(&h)
                    .then_some(MediaSelection::MaxHeight(h))
            }
        }
    }

    /// Extension of the finished file.
    pub const fn final_ext(self) -> &'static str {
        match self {
            MediaSelection::Best | MediaSelection::MaxHeight(_) => "mp4",
            MediaSelection::AudioM4a => "m4a",
            MediaSelection::AudioMp3 => "mp3",
        }
    }

    /// Audio only.
    pub const fn is_audio(self) -> bool {
        matches!(self, MediaSelection::AudioM4a | MediaSelection::AudioMp3)
    }
}

/// Sites the add dialog treats as video pages without asking yt-dlp first. Anything else can
/// still be probed; this list only decides when the quality picker appears immediately.
pub const MEDIA_HOSTS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "vimeo.com",
    "dailymotion.com",
    "dai.ly",
    "twitch.tv",
    "soundcloud.com",
    "bilibili.com",
    "streamable.com",
    "ted.com",
    "tiktok.com",
    "instagram.com",
    "facebook.com",
    "fb.watch",
    "x.com",
    "twitter.com",
    "reddit.com",
    "rumble.com",
    "odysee.com",
];

/// `true` when `host` (lower-case, no port) is one of [`MEDIA_HOSTS`] or a subdomain of one.
pub fn is_media_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    MEDIA_HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// Average bitrate of yt-dlp's `--audio-quality 0` MP3 (VBR V0), for size estimates.
const MP3_V0_BPS: f64 = 245_000.0;

fn size_of(f: &MediaFormat, duration: Option<f64>) -> Option<u64> {
    f.filesize.or_else(|| {
        let (tbr, secs) = (f.tbr?, duration?);
        (tbr > 0.0 && secs > 0.0).then(|| (f64::from(tbr) * 1000.0 / 8.0 * secs) as u64)
    })
}

fn better_audio<'a>(formats: &'a [MediaFormat], prefer_ext: &str) -> Option<&'a MediaFormat> {
    let audio: Vec<&MediaFormat> = formats
        .iter()
        .filter(|f| f.has_audio() && !f.has_video())
        .collect();
    let rank = |f: &&MediaFormat| (f.tbr.unwrap_or(0.0) * 1000.0) as u64;
    audio
        .iter()
        .copied()
        .filter(|f| f.ext == prefer_ext)
        .max_by_key(rank)
        .or_else(|| audio.iter().copied().max_by_key(rank))
}

/// The video format yt-dlp would most likely pick for "best no larger than `max`": the largest
/// resolution not above it, MP4 first, then the highest bitrate.
fn video_for(formats: &[MediaFormat], max: Option<u32>) -> Option<&MediaFormat> {
    let candidates: Vec<&MediaFormat> = formats
        .iter()
        .filter(|f| f.has_video())
        .filter(|f| match (max, f.resolution()) {
            (Some(m), Some(r)) => r <= m,
            _ => true,
        })
        .collect();
    let top = candidates.iter().filter_map(|f| f.resolution()).max();
    candidates
        .into_iter()
        .filter(|f| f.resolution() == top)
        .max_by_key(|f| (f.ext == "mp4", (f.tbr.unwrap_or(0.0) * 1000.0) as u64))
}

fn video_size(formats: &[MediaFormat], v: &MediaFormat, duration: Option<f64>) -> Option<u64> {
    let video = size_of(v, duration)?;
    if v.has_audio() {
        return Some(video);
    }
    let prefer = if v.ext == "mp4" { "m4a" } else { "webm" };
    let audio = better_audio(formats, prefer)
        .and_then(|a| size_of(a, duration))
        .unwrap_or(0);
    Some(video + audio)
}

/// Builds the quality menu from the formats: *Best*, one entry per resolution offered (largest
/// first, at most eight), then audio only as M4A and MP3. Sizes are estimates.
pub fn media_choices(formats: &[MediaFormat], duration: Option<f64>) -> Vec<MediaChoice> {
    let mut out = Vec::new();
    let mut heights: Vec<u32> = formats
        .iter()
        .filter(|f| f.has_video())
        .filter_map(MediaFormat::resolution)
        .collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();

    let has_video = formats.iter().any(MediaFormat::has_video);
    if has_video {
        let best = video_for(formats, None);
        out.push(MediaChoice {
            key: MediaSelection::Best.key(),
            kind: MediaChoiceKind::Best,
            height: best.and_then(MediaFormat::resolution),
            ext: "mp4".into(),
            approx_size: best.and_then(|v| video_size(formats, v, duration)),
        });
        for h in heights.iter().take(8) {
            let v = video_for(formats, Some(*h));
            out.push(MediaChoice {
                key: MediaSelection::MaxHeight(*h).key(),
                kind: MediaChoiceKind::Video,
                height: Some(*h),
                ext: "mp4".into(),
                approx_size: v.and_then(|v| video_size(formats, v, duration)),
            });
        }
    }
    let any_audio = formats.iter().any(MediaFormat::has_audio);
    if any_audio {
        let m4a = better_audio(formats, "m4a")
            .or_else(|| video_for(formats, None).filter(|v| v.has_audio()));
        out.push(MediaChoice {
            key: MediaSelection::AudioM4a.key(),
            kind: MediaChoiceKind::Audio,
            height: None,
            ext: "m4a".into(),
            approx_size: m4a.and_then(|a| size_of(a, duration)),
        });
        out.push(MediaChoice {
            key: MediaSelection::AudioMp3.key(),
            kind: MediaChoiceKind::Audio,
            height: None,
            ext: "mp3".into(),
            approx_size: duration
                .filter(|d| *d > 0.0)
                .map(|d| (MP3_V0_BPS / 8.0 * d) as u64),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(id: &str, ext: &str, w: u32, h: u32, size: u64) -> MediaFormat {
        MediaFormat {
            id: id.into(),
            ext: ext.into(),
            width: Some(w),
            height: Some(h),
            vcodec: Some("avc1".into()),
            filesize: Some(size),
            ..Default::default()
        }
    }

    fn audio(id: &str, ext: &str, tbr: f32, size: u64) -> MediaFormat {
        MediaFormat {
            id: id.into(),
            ext: ext.into(),
            acodec: Some("mp4a".into()),
            tbr: Some(tbr),
            filesize: Some(size),
            ..Default::default()
        }
    }

    #[test]
    fn selection_keys_round_trip_and_reject_everything_else() {
        for s in [
            MediaSelection::Best,
            MediaSelection::MaxHeight(720),
            MediaSelection::AudioM4a,
            MediaSelection::AudioMp3,
        ] {
            assert_eq!(MediaSelection::parse(&s.key()), Some(s));
        }
        for bad in [
            "",
            "h",
            "h0",
            "h99999",
            "h7x",
            "bv*+ba",
            "-f",
            "h720 --exec calc",
            "BEST",
            "h-1",
        ] {
            assert_eq!(MediaSelection::parse(bad), None, "{bad}");
        }
        assert_eq!(MediaSelection::AudioMp3.final_ext(), "mp3");
        assert_eq!(MediaSelection::MaxHeight(480).final_ext(), "mp4");
    }

    #[test]
    fn hosts_match_domains_and_subdomains_only() {
        assert!(is_media_host("www.youtube.com"));
        assert!(is_media_host("youtu.be"));
        assert!(is_media_host("M.YouTube.com"));
        assert!(is_media_host("player.vimeo.com"));
        assert!(!is_media_host("notyoutube.com"));
        assert!(!is_media_host("youtube.com.evil.test"));
        assert!(!is_media_host("example.com"));
    }

    #[test]
    fn choices_list_best_resolutions_and_audio_with_sizes() {
        let formats = vec![
            audio("139", "m4a", 48.0, 1_000),
            audio("140", "m4a", 129.0, 3_000),
            audio("251", "webm", 140.0, 3_500),
            video("160", "mp4", 256, 144, 2_000),
            video("136", "mp4", 1280, 720, 40_000),
            video("247", "webm", 1280, 720, 35_000),
            video("137", "mp4", 1920, 1080, 80_000),
            video("248", "webm", 1920, 1080, 70_000),
        ];
        let c = media_choices(&formats, Some(60.0));
        let keys: Vec<&str> = c.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            ["best", "h1080", "h720", "h144", "audio-m4a", "audio-mp3"]
        );
        // best = 1080p mp4 + the best m4a
        assert_eq!(c[0].height, Some(1080));
        assert_eq!(c[0].approx_size, Some(83_000));
        assert_eq!(c[2].approx_size, Some(43_000));
        assert_eq!(c[4].approx_size, Some(3_000));
        assert_eq!(c[5].approx_size, Some(245_000 / 8 * 60));
    }

    #[test]
    fn vertical_videos_use_the_smaller_side_and_sizes_fall_back_to_bitrate() {
        let mut v = video("1", "mp4", 1080, 1920, 0);
        v.filesize = None;
        v.tbr = Some(800.0);
        v.acodec = Some("mp4a".into());
        let c = media_choices(&[v], Some(10.0));
        assert_eq!(c[1].key, "h1080");
        assert_eq!(c[1].approx_size, Some(1_000_000));
        // a muxed format still offers audio only
        assert!(c.iter().any(|c| c.key == "audio-m4a"));
    }

    #[test]
    fn audio_only_pages_offer_only_audio() {
        let c = media_choices(&[audio("a", "mp3", 128.0, 100)], None);
        let keys: Vec<&str> = c.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["audio-m4a", "audio-mp3"]);
        assert_eq!(c[1].approx_size, None);
    }
}
